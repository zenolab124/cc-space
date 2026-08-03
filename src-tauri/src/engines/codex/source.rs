use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use super::super::core::*;
use super::app_server::IncomingMessage;
use super::{default_instance, CodexSupervisor};

const UNCLASSIFIED_PROJECT: &str = "uncategorized";
const PAGE_LIMIT: usize = 100;
const MAX_PAGES: usize = 100;
const THREAD_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadListResponse {
    data: Vec<CodexThread>,
    next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadReadResponse {
    thread: CodexThread,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexThread {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    preview: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    model_provider: String,
    created_at: i64,
    updated_at: i64,
    #[serde(default)]
    turns: Vec<CodexTurn>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexTurn {
    id: String,
    #[serde(default)]
    items: Vec<Value>,
    #[serde(default)]
    started_at: Option<i64>,
}

type ThreadCache = Arc<Mutex<Option<(Instant, Vec<CodexThread>)>>>;

pub struct CodexSource {
    instance: EngineInstanceId,
    supervisor: Arc<CodexSupervisor>,
    change_sinks: Arc<Mutex<Vec<SourceChangeSink>>>,
    thread_cache: ThreadCache,
}

impl CodexSource {
    pub fn new(supervisor: Arc<CodexSupervisor>) -> EngineResult<Self> {
        let instance = default_instance()?;
        let change_sinks: Arc<Mutex<Vec<SourceChangeSink>>> = Arc::new(Mutex::new(Vec::new()));
        let thread_cache: ThreadCache = Arc::new(Mutex::new(None));
        let callback_sinks = Arc::clone(&change_sinks);
        let callback_cache = Arc::clone(&thread_cache);
        let callback_instance = instance.clone();
        supervisor.subscribe(Arc::new(move |message| {
            let IncomingMessage::Notification { method, params } = message else {
                return;
            };
            let kind = match method.as_str() {
                "thread/closed" | "thread/archived" => SourceChangeKind::SessionRemoved,
                "turn/completed" | "thread/started" | "thread/name/updated" => {
                    SourceChangeKind::SessionChanged
                }
                _ => return,
            };
            *callback_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
            let thread_id = params.get("threadId").and_then(Value::as_str).or_else(|| {
                params
                    .get("thread")
                    .and_then(|thread| thread.get("id"))
                    .and_then(Value::as_str)
            });
            let change = SourceChange {
                kind: if thread_id.is_some() {
                    kind
                } else {
                    SourceChangeKind::FullRefresh
                },
                project: None,
                session: thread_id
                    .and_then(|id| SessionRef::new(callback_instance.clone(), id).ok()),
            };
            let sinks = callback_sinks
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            for sink in &sinks {
                sink(change.clone());
            }
        }));
        Ok(Self {
            instance,
            supervisor,
            change_sinks,
            thread_cache,
        })
    }

    pub fn supervisor(&self) -> &Arc<CodexSupervisor> {
        &self.supervisor
    }

    fn owns_project(&self, project: &ProjectRef) -> EngineResult<()> {
        if project.engine() == &self.instance {
            Ok(())
        } else {
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "Codex source does not own this project",
            ))
        }
    }

    fn owns_session(&self, session: &SessionRef) -> EngineResult<()> {
        if session.engine() == &self.instance {
            Ok(())
        } else {
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "Codex source does not own this session",
            ))
        }
    }

    fn list_all_threads(&self) -> EngineResult<Vec<CodexThread>> {
        if let Some((created_at, threads)) = self
            .thread_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            if created_at.elapsed() <= THREAD_CACHE_TTL {
                return Ok(threads.clone());
            }
        }
        let mut threads = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let value = self.supervisor.request(
                "thread/list",
                json!({
                    "cursor": cursor,
                    "limit": PAGE_LIMIT,
                    "sortKey": "updated_at",
                    "sortDirection": "desc"
                }),
            )?;
            let response: ThreadListResponse = serde_json::from_value(value).map_err(|error| {
                EngineError::new(
                    EngineErrorKind::Protocol,
                    format!("Codex returned an invalid thread list: {error}"),
                )
            })?;
            threads.extend(response.data);
            cursor = response.next_cursor;
            if cursor.is_none() {
                *self
                    .thread_cache
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) =
                    Some((Instant::now(), threads.clone()));
                return Ok(threads);
            }
        }
        Err(EngineError::new(
            EngineErrorKind::Protocol,
            "Codex thread list exceeded the pagination safety limit",
        ))
    }

    fn read_thread(&self, session: &SessionRef) -> EngineResult<CodexThread> {
        self.owns_session(session)?;
        let value = self.supervisor.request(
            "thread/read",
            json!({ "threadId": session.native_id(), "includeTurns": true }),
        )?;
        serde_json::from_value::<ThreadReadResponse>(value)
            .map(|response| response.thread)
            .map_err(|error| {
                EngineError::new(
                    EngineErrorKind::Protocol,
                    format!("Codex returned an invalid thread: {error}"),
                )
            })
    }

    fn project_native_id(thread: &CodexThread) -> String {
        if thread.cwd.trim().is_empty() {
            UNCLASSIFIED_PROJECT.to_string()
        } else {
            thread.cwd.clone()
        }
    }

    fn project_ref(&self, thread: &CodexThread) -> EngineResult<ProjectRef> {
        ProjectRef::new(self.instance.clone(), Self::project_native_id(thread))
    }

    fn map_summary(&self, thread: CodexThread) -> EngineResult<CoreSessionSummary> {
        let project = self.project_ref(&thread)?;
        let source_meta = if thread.model_provider.is_empty() {
            SourceMetadata::default()
        } else {
            SourceMetadata::new(BTreeMap::from([(
                "modelProvider".into(),
                Value::String(thread.model_provider.clone()),
            )]))?
        };
        Ok(CoreSessionSummary {
            reference: SessionRef::new(self.instance.clone(), thread.id)?,
            project,
            title: thread.name,
            preview: (!thread.preview.is_empty()).then_some(thread.preview),
            cwd: (!thread.cwd.is_empty()).then_some(thread.cwd),
            model: None,
            created_at: Some(epoch_seconds(thread.created_at)),
            updated_at: Some(epoch_seconds(thread.updated_at)),
            usage: None,
            source_meta,
        })
    }

    fn timeline(&self, session: &SessionRef) -> EngineResult<Vec<ConversationRecord>> {
        let thread = self.read_thread(session)?;
        let mut records = Vec::new();
        for turn in thread.turns {
            for item in &turn.items {
                if let Some(record) = map_item(session, &turn, item)? {
                    records.push(record);
                }
            }
        }
        Ok(records)
    }

    fn runtime_availability(&self) -> ActionAvailability {
        let path = match crate::codex_locator::locate() {
            Ok(path) => path,
            Err(_) => return ActionAvailability::unavailable("engine.codex.cliUnavailable"),
        };
        let version = super::adapter::cli_version(&path);
        if super::adapter::supported_version(version.as_deref()) == Some(false) {
            return ActionAvailability::unavailable("engine.codex.versionUnsupported");
        }
        match self
            .supervisor
            .request("account/read", json!({ "refreshToken": false }))
        {
            Ok(account) if super::adapter::account_is_authenticated(&account) => {
                ActionAvailability::available()
            }
            Ok(_) => ActionAvailability::unavailable("engine.codex.authenticationRequired"),
            Err(_) => ActionAvailability::unavailable("engine.codex.accountProbeFailed"),
        }
    }
}

impl SessionSource for CodexSource {
    fn list_projects(&self, query: ProjectQuery) -> EngineFuture<'_, ProjectPage> {
        Box::pin(async move {
            let threads = self.list_all_threads()?;
            let mut grouped: BTreeMap<String, (usize, i64)> = BTreeMap::new();
            for thread in threads {
                let native_id = Self::project_native_id(&thread);
                let entry = grouped.entry(native_id).or_insert((0, thread.updated_at));
                entry.0 += 1;
                entry.1 = entry.1.max(thread.updated_at);
            }
            let mut projects = Vec::with_capacity(grouped.len());
            for (native_id, (session_count, last_active)) in grouped {
                let unclassified = native_id == UNCLASSIFIED_PROJECT;
                let display_name = if unclassified {
                    "Uncategorized".to_string()
                } else {
                    Path::new(&native_id)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .unwrap_or(&native_id)
                        .to_string()
                };
                projects.push(CoreProject {
                    reference: ProjectRef::new(self.instance.clone(), native_id.clone())?,
                    display_name,
                    display_path: (!unclassified).then_some(native_id),
                    session_count,
                    last_active: Some(epoch_seconds(last_active)),
                });
            }
            projects.sort_by(|left, right| right.last_active.cmp(&left.last_active));
            let (projects, next_cursor) = paginate(projects, query.cursor, query.limit);
            Ok(ProjectPage {
                projects,
                next_cursor,
            })
        })
    }

    fn list_sessions(
        &self,
        project: ProjectRef,
        query: SessionQuery,
    ) -> EngineFuture<'_, SessionPage> {
        Box::pin(async move {
            self.owns_project(&project)?;
            let mut sessions = Vec::new();
            for thread in self.list_all_threads()? {
                if Self::project_native_id(&thread) == project.native_id() {
                    sessions.push(self.map_summary(thread)?);
                }
            }
            let (sessions, next_cursor) = paginate(sessions, query.cursor, query.limit);
            Ok(SessionPage {
                sessions,
                next_cursor,
            })
        })
    }

    fn load_timeline(
        &self,
        session: SessionRef,
        page: TimelinePage,
    ) -> EngineFuture<'_, ConversationPage> {
        Box::pin(async move {
            let records = self.timeline(&session)?;
            let (records, next_cursor) = paginate(records, page.cursor, Some(page.limit));
            Ok(ConversationPage {
                records,
                next_cursor,
            })
        })
    }

    fn subscribe_changes(&self, sink: SourceChangeSink) -> EngineResult<SubscriptionHandle> {
        self.change_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sink);
        Ok(Box::new(()))
    }

    fn build_search_document(&self, session: SessionRef) -> EngineFuture<'_, SearchDocument> {
        Box::pin(async move {
            let thread = self.read_thread(&session)?;
            let title = thread
                .name
                .or_else(|| (!thread.preview.is_empty()).then(|| thread.preview.clone()));
            let mut text = String::new();
            for turn in thread.turns {
                for item in turn.items {
                    for segment in map_item_segments(&session, &item)? {
                        match segment {
                            Segment::Text { text: value }
                            | Segment::Reasoning { text: value, .. } => {
                                text.push_str(&value);
                                text.push('\n');
                            }
                            Segment::ToolCall { name, .. } => {
                                text.push_str(&name);
                                text.push('\n');
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(SearchDocument {
                session,
                title,
                text,
            })
        })
    }

    fn resolve_asset(&self, asset: AssetRef) -> EngineFuture<'_, ResolvedAsset> {
        Box::pin(async move {
            let thread = self.read_thread(&asset.session)?;
            for turn in thread.turns {
                for item in turn.items {
                    if item.get("id").and_then(Value::as_str) == Some(&asset.native_id)
                        && item.get("type").and_then(Value::as_str) == Some("imageView")
                    {
                        let path = item.get("path").and_then(Value::as_str).ok_or_else(|| {
                            EngineError::new(
                                EngineErrorKind::Protocol,
                                "Codex image item has no path",
                            )
                        })?;
                        let metadata = fs::metadata(path).map_err(|error| {
                            EngineError::new(EngineErrorKind::Io, error.to_string())
                        })?;
                        if metadata.len() > 32 * 1024 * 1024 {
                            return Err(EngineError::new(
                                EngineErrorKind::Protocol,
                                "Codex asset exceeds the transfer size limit",
                            ));
                        }
                        let bytes = fs::read(path).map_err(|error| {
                            EngineError::new(EngineErrorKind::Io, error.to_string())
                        })?;
                        return Ok(ResolvedAsset {
                            media_type: media_type_for_path(path).to_string(),
                            bytes,
                        });
                    }
                }
            }
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "Codex asset was not found",
            ))
        })
    }

    fn session_actions(&self, session: SessionRef) -> EngineFuture<'_, SessionActions> {
        Box::pin(async move {
            let thread = self.read_thread(&session)?;
            let cwd_available = !thread.cwd.is_empty() && Path::new(&thread.cwd).is_dir();
            let resume = if !cwd_available {
                ActionAvailability::unavailable("engine.session.cwdUnavailable")
            } else {
                self.runtime_availability()
            };
            Ok(SessionActions {
                resume: resume.clone(),
                fork: ActionAvailability::unavailable("engine.codex.forkUnavailable"),
                send: resume,
                steer: ActionAvailability::available(),
                interrupt: ActionAvailability::available(),
                open_cwd: if thread.cwd.is_empty() {
                    ActionAvailability::unavailable("engine.session.noCwd")
                } else {
                    ActionAvailability::available()
                },
            })
        })
    }
}

fn map_item(
    session: &SessionRef,
    turn: &CodexTurn,
    item: &Value,
) -> EngineResult<Option<ConversationRecord>> {
    let type_name = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown-item")
        .to_string();
    let role = match type_name {
        "userMessage" => ConversationRole::User,
        "agentMessage" | "plan" | "reasoning" => ConversationRole::Assistant,
        "commandExecution" | "fileChange" | "mcpToolCall" | "dynamicToolCall" => {
            ConversationRole::Tool
        }
        _ => ConversationRole::Unknown,
    };
    Ok(Some(ConversationRecord {
        id,
        session: session.clone(),
        turn_id: Some(turn.id.clone()),
        parent_id: None,
        role,
        timestamp: turn.started_at.map(epoch_seconds),
        segments: map_item_segments(session, item)?,
        usage: None,
        source_meta: SourceMetadata::default(),
    }))
}

pub(crate) fn map_item_segments(session: &SessionRef, item: &Value) -> EngineResult<Vec<Segment>> {
    let type_name = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown-item")
        .to_string();
    let segments = match type_name {
        "userMessage" => item
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|input| match input.get("type").and_then(Value::as_str) {
                Some("text") => {
                    input
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| Segment::Text {
                            text: bounded_segment_text(text.to_string()),
                        })
                }
                Some(kind) => Some(Segment::Unknown {
                    type_name: format!("userInput:{kind}"),
                    summary: None,
                }),
                None => None,
            })
            .collect(),
        "agentMessage" => vec![Segment::Text {
            text: bounded_segment_text(
                item.get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ),
        }],
        "plan" => vec![Segment::Text {
            text: bounded_segment_text(
                item.get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ),
        }],
        "reasoning" => {
            let mut values = Vec::new();
            for field in ["summary", "content"] {
                if let Some(parts) = item.get(field).and_then(Value::as_array) {
                    for part in parts {
                        let text = part
                            .as_str()
                            .or_else(|| part.get("text").and_then(Value::as_str));
                        if let Some(text) = text {
                            values.push(text);
                        }
                    }
                }
            }
            vec![Segment::Reasoning {
                text: bounded_segment_text(values.join("\n")),
                visibility: ReasoningVisibility::Summary,
            }]
        }
        "commandExecution" => vec![Segment::CommandExecution {
            id,
            command: item
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cwd: item.get("cwd").and_then(Value::as_str).map(String::from),
            output: item
                .get("aggregatedOutput")
                .and_then(Value::as_str)
                .map(|value| bounded_segment_text(value.to_string())),
            status: map_status(item.get("status").and_then(Value::as_str)),
        }],
        "fileChange" => vec![Segment::FileChange {
            id,
            changes: item
                .get("changes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|change| FilePatch {
                    path: change
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    kind: change
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("update")
                        .to_string(),
                    diff: change
                        .get("diff")
                        .and_then(Value::as_str)
                        .map(|value| bounded_segment_text(value.to_string())),
                })
                .collect(),
            status: map_status(item.get("status").and_then(Value::as_str)),
        }],
        "mcpToolCall" | "dynamicToolCall" => vec![Segment::ToolCall {
            id,
            name: item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or(type_name)
                .to_string(),
            input: bounded_segment_value(item.get("arguments").cloned().unwrap_or(Value::Null)),
        }],
        "imageView" => vec![Segment::Attachment {
            asset: AssetRef {
                session: session.clone(),
                native_id: id,
            },
            media_type: media_type_for_path(
                item.get("path").and_then(Value::as_str).unwrap_or_default(),
            )
            .to_string(),
            title: None,
        }],
        _ => vec![Segment::Unknown {
            type_name: type_name.to_string(),
            summary: None,
        }],
    };
    Ok(segments)
}

fn map_status(status: Option<&str>) -> ItemStatus {
    match status {
        Some("inProgress" | "running") => ItemStatus::Running,
        Some("completed" | "success") => ItemStatus::Completed,
        Some("declined") => ItemStatus::Declined,
        Some("interrupted" | "cancelled") => ItemStatus::Interrupted,
        Some("failed" | "error") => ItemStatus::Failed,
        _ => ItemStatus::Pending,
    }
}

fn paginate<T>(
    values: Vec<T>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> (Vec<T>, Option<String>) {
    let start = cursor
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
        .min(values.len());
    let end = start
        .saturating_add(limit.unwrap_or(values.len()))
        .min(values.len());
    let next_cursor = (end < values.len()).then(|| end.to_string());
    (
        values.into_iter().skip(start).take(end - start).collect(),
        next_cursor,
    )
}

fn epoch_seconds(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .to_rfc3339()
}

fn media_type_for_path(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_and_unknown_items() {
        let session = SessionRef::new(default_instance().unwrap(), "session").unwrap();
        assert!(matches!(
            map_item_segments(&session, &json!({"id":"a","type":"agentMessage","text":"hello"}))
                .unwrap()
                .as_slice(),
            [Segment::Text { text }] if text == "hello"
        ));
        assert_eq!(
            map_item_segments(
                &session,
                &json!({"id":"x","type":"futureItem","large":"x".repeat(20_000)})
            )
            .unwrap(),
            vec![Segment::Unknown {
                type_name: "futureItem".into(),
                summary: None
            }]
        );
    }

    #[test]
    fn unclassified_project_is_stable() {
        let thread = CodexThread {
            id: "id".into(),
            name: None,
            preview: String::new(),
            cwd: String::new(),
            model_provider: String::new(),
            created_at: 0,
            updated_at: 0,
            turns: Vec::new(),
        };
        assert_eq!(
            CodexSource::project_native_id(&thread),
            UNCLASSIFIED_PROJECT
        );
    }

    #[test]
    #[ignore = "requires an installed Codex CLI and explicit MONET_CODEX_SOURCE_READ_SMOKE=1"]
    fn installed_source_reads_history_without_mutation() {
        if std::env::var_os("MONET_CODEX_SOURCE_READ_SMOKE").is_none() {
            return;
        }
        let source = CodexSource::new(CodexSupervisor::new()).unwrap();
        let threads = source.list_all_threads().unwrap();
        if let Some(thread) = threads.first() {
            let session = SessionRef::new(default_instance().unwrap(), &thread.id).unwrap();
            let _records = source.timeline(&session).unwrap();
        }
    }
}
