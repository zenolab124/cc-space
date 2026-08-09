use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use serde_json::{json, Value};

use super::super::core::*;
use super::app_server::IncomingMessage;
use super::{default_instance, CodexSupervisor};

const UNCLASSIFIED_PROJECT: &str = "uncategorized";
const PAGE_LIMIT: usize = 100;
const MAX_PAGES: usize = 100;
const THREAD_CACHE_TTL: Duration = Duration::from_secs(2);
const TOKEN_USAGE_TAIL_BYTES: u64 = 1024 * 1024;
const USAGE_CACHE_LIMIT: usize = 2048;

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
pub(super) struct CodexThread {
    pub(super) id: String,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) preview: String,
    #[serde(default)]
    pub(super) cwd: String,
    #[serde(default)]
    pub(super) model_provider: String,
    #[serde(default)]
    pub(super) path: Option<String>,
    pub(super) created_at: i64,
    pub(super) updated_at: i64,
    #[serde(default)]
    pub(super) turns: Vec<CodexTurn>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CodexTurn {
    pub(super) id: String,
    #[serde(default)]
    pub(super) items: Vec<Value>,
    #[serde(default)]
    pub(super) started_at: Option<i64>,
    #[serde(default)]
    pub(super) completed_at: Option<i64>,
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

    pub fn probe_history(&self) -> EngineResult<()> {
        self.list_all_threads().map(|_| ())
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
        let app_server_result = self.list_app_server_threads();
        let file_threads = super::file_source::list_threads()?;
        let mut merged = BTreeMap::new();
        for thread in file_threads {
            merged.insert(thread.id.clone(), thread);
        }
        if let Ok(app_threads) = &app_server_result {
            for thread in app_threads {
                // App Server has richer metadata and live state. Local-only rows
                // are still retained for archived sessions and CLI-less installs.
                merged.insert(thread.id.clone(), thread.clone());
            }
        } else if merged.is_empty() {
            return Err(
                app_server_result.expect_err("Codex App Server result was unexpectedly missing")
            );
        }
        let mut threads: Vec<_> = merged.into_values().collect();
        threads.sort_by_key(|thread| std::cmp::Reverse(thread.updated_at));
        *self
            .thread_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some((Instant::now(), threads.clone()));
        Ok(threads)
    }

    fn list_app_server_threads(&self) -> EngineResult<Vec<CodexThread>> {
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
                return Ok(threads);
            }
        }
        Err(EngineError::new(
            EngineErrorKind::Protocol,
            "Codex thread list exceeded the pagination safety limit",
        ))
    }

    fn read_app_server_thread(
        &self,
        session: &SessionRef,
        include_turns: bool,
    ) -> EngineResult<CodexThread> {
        self.supervisor
            .request(
                "thread/read",
                json!({
                    "threadId": session.native_id(),
                    "includeTurns": include_turns
                }),
            )
            .and_then(|value| {
                serde_json::from_value::<ThreadReadResponse>(value)
                    .map(|response| response.thread)
                    .map_err(|error| {
                        EngineError::new(
                            EngineErrorKind::Protocol,
                            format!("Codex returned an invalid thread: {error}"),
                        )
                    })
            })
    }

    fn read_thread_with_turns(&self, session: &SessionRef) -> EngineResult<CodexThread> {
        self.owns_session(session)?;
        let app_server_result = self.read_app_server_thread(session, true).or_else(|error| {
            if turns_unavailable_before_first_message(&error) {
                self.read_app_server_thread(session, false)
            } else {
                Err(error)
            }
        });
        match app_server_result {
            Ok(thread) => Ok(thread),
            Err(app_server_error) => {
                super::file_source::read_thread(session.native_id()).or(Err(app_server_error))
            }
        }
    }

    fn read_thread_metadata(&self, session: &SessionRef) -> EngineResult<CodexThread> {
        self.owns_session(session)?;
        let app_server_result = self.read_app_server_thread(session, false);
        match app_server_result {
            Ok(thread) => Ok(thread),
            Err(app_server_error) => {
                super::file_source::read_thread(session.native_id()).or(Err(app_server_error))
            }
        }
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
        let usage_snapshot = read_usage_snapshot(thread.path.as_deref());
        let mut source_values = BTreeMap::new();
        if !thread.model_provider.is_empty() {
            source_values.insert(
                "modelProvider".into(),
                Value::String(thread.model_provider.clone()),
            );
        }
        if let Some(context_window) = usage_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.context_window)
        {
            source_values.insert("contextWindow".into(), Value::from(context_window));
        }
        let source_meta = SourceMetadata::new(source_values)?;
        Ok(CoreSessionSummary {
            reference: SessionRef::new(self.instance.clone(), thread.id)?,
            project,
            title: thread.name,
            preview: (!thread.preview.is_empty()).then_some(thread.preview),
            cwd: (!thread.cwd.is_empty()).then_some(thread.cwd),
            model: usage_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.model.clone()),
            created_at: Some(epoch_seconds(thread.created_at)),
            updated_at: Some(epoch_seconds(thread.updated_at)),
            usage: usage_snapshot.map(|snapshot| snapshot.total),
            source_meta,
        })
    }

    fn timeline(&self, session: &SessionRef) -> EngineResult<Vec<ConversationRecord>> {
        let thread = self.read_thread_with_turns(session)?;
        let timeline_snapshot = read_timeline_snapshot(thread.path.as_deref());
        let mut records = Vec::new();
        for turn in thread.turns {
            let turn_snapshot = timeline_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.turns.get(&turn.id));
            let mut turn_records = Vec::new();
            for item in &turn.items {
                if let Some(record) = map_item(session, &turn, item)? {
                    turn_records.push(record);
                }
            }
            if let Some(snapshot) = turn_snapshot {
                let source_meta = turn_source_metadata(snapshot)?;
                for record in &mut turn_records {
                    if record.role != ConversationRole::User {
                        record.source_meta = source_meta.clone();
                    }
                }
                if let Some(record) = turn_records
                    .iter_mut()
                    .rev()
                    .find(|record| record.role == ConversationRole::Assistant)
                {
                    record.usage = snapshot.usage.clone();
                }
            }
            records.extend(turn_records);
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
            let thread = self.read_thread_with_turns(&session)?;
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
            let thread = self.read_thread_with_turns(&asset.session)?;
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
            let thread = self.read_thread_metadata(&session)?;
            let cwd_available = !thread.cwd.is_empty() && Path::new(&thread.cwd).is_dir();
            let runtime = self.runtime_availability();
            let cwd_runtime = if !cwd_available {
                ActionAvailability::unavailable("engine.session.cwdUnavailable")
            } else {
                runtime.clone()
            };
            Ok(SessionActions {
                resume: cwd_runtime.clone(),
                fork: runtime.clone(),
                send: cwd_runtime,
                send_while_running: runtime.clone(),
                interrupt: runtime,
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
        "commandExecution"
        | "fileChange"
        | "mcpToolCall"
        | "dynamicToolCall"
        | "collabAgentToolCall"
        | "toolResult" => ConversationRole::Tool,
        _ => ConversationRole::Unknown,
    };
    let timestamp = if type_name == "userMessage" {
        turn.started_at
    } else {
        turn.completed_at.or(turn.started_at)
    };
    Ok(Some(ConversationRecord {
        id,
        session: session.clone(),
        turn_id: Some(turn.id.clone()),
        parent_id: None,
        role,
        timestamp: timestamp.map(epoch_seconds),
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
        "toolResult" => vec![Segment::ToolResult {
            call_id: item
                .get("callId")
                .or_else(|| item.get("call_id"))
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string(),
            content: bounded_segment_value(item.get("content").cloned().unwrap_or(Value::Null)),
            is_error: item
                .get("isError")
                .or_else(|| item.get("is_error"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }],
        "collabAgentToolCall" => vec![Segment::ToolCall {
            id,
            name: item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("collaboration")
                .to_string(),
            input: bounded_segment_value(item.clone()),
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

#[derive(Clone, Debug)]
struct CodexUsageSnapshot {
    total: Usage,
    last: Usage,
    context_window: Option<u64>,
    model: Option<String>,
}

#[derive(Clone, Debug)]
struct CachedCodexUsage {
    length: u64,
    modified: Option<SystemTime>,
    snapshot: Option<CodexUsageSnapshot>,
}

static USAGE_CACHE: OnceLock<Mutex<HashMap<String, CachedCodexUsage>>> = OnceLock::new();

#[derive(Clone, Debug, Default)]
struct CodexTurnSnapshot {
    model: Option<String>,
    effort: Option<String>,
    usage: Option<Usage>,
}

#[derive(Clone, Debug, Default)]
struct CodexTimelineSnapshot {
    turns: BTreeMap<String, CodexTurnSnapshot>,
}

#[derive(Clone, Debug)]
struct CachedCodexTimeline {
    file_length: u64,
    processed_length: u64,
    modified: Option<SystemTime>,
    snapshot: CodexTimelineSnapshot,
    current_turn_id: Option<String>,
}

static TIMELINE_CACHE: OnceLock<Mutex<HashMap<String, CachedCodexTimeline>>> = OnceLock::new();

fn read_timeline_snapshot(path: Option<&str>) -> Option<CodexTimelineSnapshot> {
    let path = path?;
    let metadata = fs::metadata(path).ok()?;
    let length = metadata.len();
    let modified = metadata.modified().ok();
    let cache = TIMELINE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let (mut snapshot, mut current_turn_id, offset) = {
        let cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        match cache.get(path) {
            Some(cached)
                if cached.file_length == length
                    && (cached.modified == modified || modified.is_none()) =>
            {
                return Some(cached.snapshot.clone());
            }
            Some(cached) if cached.file_length < length => (
                cached.snapshot.clone(),
                cached.current_turn_id.clone(),
                cached.processed_length,
            ),
            _ => (CodexTimelineSnapshot::default(), None, 0),
        }
    };

    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut appended = Vec::with_capacity(length.saturating_sub(offset) as usize);
    file.read_to_end(&mut appended).ok()?;
    let mut processed_length = offset;
    for chunk in appended.split_inclusive(|byte| *byte == b'\n') {
        let complete_line = chunk.ends_with(b"\n");
        let line = chunk.strip_suffix(b"\n").unwrap_or(chunk);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        match serde_json::from_slice::<Value>(line) {
            Ok(value) => {
                update_timeline_snapshot(&mut snapshot, &mut current_turn_id, &value);
                processed_length = processed_length.saturating_add(chunk.len() as u64);
            }
            Err(_) if complete_line => {
                // A complete malformed row cannot become valid after a later append.
                processed_length = processed_length.saturating_add(chunk.len() as u64);
            }
            Err(_) => break,
        }
    }
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if cache.len() >= 12 && !cache.contains_key(path) {
        if let Some(key) = cache.keys().next().cloned() {
            cache.remove(&key);
        }
    }
    cache.insert(
        path.to_string(),
        CachedCodexTimeline {
            file_length: length,
            processed_length,
            modified,
            snapshot: snapshot.clone(),
            current_turn_id,
        },
    );
    Some(snapshot)
}

fn update_timeline_snapshot(
    snapshot: &mut CodexTimelineSnapshot,
    current_turn_id: &mut Option<String>,
    value: &Value,
) {
    if value.get("type").and_then(Value::as_str) == Some("turn_context") {
        let context = value.get("payload").unwrap_or(value);
        let Some(turn_id) = context
            .get("turn_id")
            .or_else(|| context.get("turnId"))
            .and_then(Value::as_str)
        else {
            return;
        };
        *current_turn_id = Some(turn_id.to_string());
        let turn = snapshot.turns.entry(turn_id.to_string()).or_default();
        turn.model = context
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(String::from);
        turn.effort = context
            .get("effort")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(String::from);
        return;
    }

    let Some(turn_id) = current_turn_id.as_deref() else {
        return;
    };
    if let Some(usage) = usage_snapshot_from_event(value) {
        snapshot.turns.entry(turn_id.to_string()).or_default().usage = Some(usage.last);
    }
}

fn turn_source_metadata(snapshot: &CodexTurnSnapshot) -> EngineResult<SourceMetadata> {
    let mut values = BTreeMap::new();
    if let Some(model) = &snapshot.model {
        values.insert("model".into(), Value::String(model.clone()));
    }
    if let Some(effort) = &snapshot.effort {
        values.insert("effort".into(), Value::String(effort.clone()));
    }
    SourceMetadata::new(values)
}

fn read_usage_snapshot(path: Option<&str>) -> Option<CodexUsageSnapshot> {
    let path = path?;
    let metadata = fs::metadata(path).ok()?;
    let length = metadata.len();
    let modified = metadata.modified().ok();
    let cache = USAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(cached) = cache.get(path) {
            if cached.length == length && (cached.modified == modified || modified.is_none()) {
                return cached.snapshot.clone();
            }
        }
    }

    let mut file = fs::File::open(path).ok()?;
    let read_length = length.min(TOKEN_USAGE_TAIL_BYTES);
    file.seek(SeekFrom::Start(length.saturating_sub(read_length)))
        .ok()?;
    let mut bytes = Vec::with_capacity(read_length as usize);
    file.take(read_length).read_to_end(&mut bytes).ok()?;
    let tail = String::from_utf8_lossy(&bytes);
    let mut usage = None;
    let mut model = None;
    for line in tail.lines().rev() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if usage.is_none() {
            usage = usage_snapshot_from_event(&value);
        }
        if model.is_none() {
            model = model_from_event(&value);
        }
        if usage.is_some() && model.is_some() {
            break;
        }
    }
    let snapshot = usage.map(|mut snapshot| {
        snapshot.model = model;
        snapshot
    });
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if cache.len() >= USAGE_CACHE_LIMIT && !cache.contains_key(path) {
        if let Some(key) = cache.keys().next().cloned() {
            cache.remove(&key);
        }
    }
    cache.insert(
        path.to_string(),
        CachedCodexUsage {
            length,
            modified,
            snapshot: snapshot.clone(),
        },
    );
    snapshot
}

fn usage_snapshot_from_event(value: &Value) -> Option<CodexUsageSnapshot> {
    let payload = value.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let info = payload.get("info")?;
    Some(CodexUsageSnapshot {
        total: normalized_usage(info.get("total_token_usage")?)?,
        last: normalized_usage(info.get("last_token_usage")?)?,
        context_window: info.get("model_context_window").and_then(Value::as_u64),
        model: None,
    })
}

fn model_from_event(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("turn_context") {
        return None;
    }
    value
        .get("payload")
        .unwrap_or(value)
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(String::from)
}

fn normalized_usage(value: &Value) -> Option<Usage> {
    let input_tokens = value.get("input_tokens")?.as_u64()?;
    let cached_input_tokens = value
        .get("cached_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let cache_creation_input_tokens = value
        .get("cache_write_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    Some(Usage {
        input_tokens: input_tokens
            .saturating_sub(cached_input_tokens)
            .saturating_sub(cache_creation_input_tokens),
        output_tokens: value.get("output_tokens")?.as_u64()?,
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        cached_input_tokens: Some(cached_input_tokens),
        cache_creation_input_tokens: Some(cache_creation_input_tokens),
    })
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

fn turns_unavailable_before_first_message(error: &EngineError) -> bool {
    error.kind == EngineErrorKind::Protocol
        && error.message.contains("includeTurns")
        && (error.message.contains("not materialized")
            || error.message.contains("ephemeral thread"))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Wake, Waker};

    use super::*;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn resolve_ready<T>(mut future: EngineFuture<'_, T>) -> EngineResult<T> {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        match Future::poll(future.as_mut(), &mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("Codex adapter futures must resolve immediately"),
        }
    }

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
            path: None,
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
    fn retries_thread_read_without_turns_for_pre_message_threads() {
        for message in [
            "Codex request failed: thread id is not materialized yet; includeTurns is unavailable before first user message",
            "Codex request failed: ephemeral threads do not support includeTurns",
        ] {
            assert!(turns_unavailable_before_first_message(&EngineError::new(
                EngineErrorKind::Protocol,
                message,
            )));
        }
        assert!(!turns_unavailable_before_first_message(&EngineError::new(
            EngineErrorKind::Protocol,
            "Codex request failed: thread not found",
        )));
    }

    #[test]
    fn maps_codex_token_usage_without_double_counting_cache() {
        let snapshot = usage_snapshot_from_event(&json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": 1200,
                        "cached_input_tokens": 900,
                        "cache_write_input_tokens": 25,
                        "output_tokens": 80,
                        "total_tokens": 1280
                    },
                    "last_token_usage": {
                        "input_tokens": 500,
                        "cached_input_tokens": 400,
                        "cache_write_input_tokens": 0,
                        "output_tokens": 30,
                        "total_tokens": 530
                    },
                    "model_context_window": 258400
                }
            }
        }))
        .unwrap();
        assert_eq!(snapshot.total.input_tokens, 275);
        assert_eq!(snapshot.total.cached_input_tokens, Some(900));
        assert_eq!(snapshot.total.cache_creation_input_tokens, Some(25));
        assert_eq!(snapshot.last.input_tokens, 100);
        assert_eq!(snapshot.context_window, Some(258400));
    }

    #[test]
    fn maps_codex_turn_context_model() {
        assert_eq!(
            model_from_event(&json!({
                "type": "turn_context",
                "payload": { "model": "gpt-5.6-sol" }
            })),
            Some("gpt-5.6-sol".into())
        );
        assert_eq!(model_from_event(&json!({ "type": "event_msg" })), None);
    }

    #[test]
    fn associates_codex_model_effort_and_usage_with_each_turn() {
        let mut snapshot = CodexTimelineSnapshot::default();
        let mut current_turn_id = None;
        update_timeline_snapshot(
            &mut snapshot,
            &mut current_turn_id,
            &json!({
                "type": "turn_context",
                "payload": {
                    "turn_id": "turn-1",
                    "model": "gpt-5.6-sol",
                    "effort": "high"
                }
            }),
        );
        update_timeline_snapshot(
            &mut snapshot,
            &mut current_turn_id,
            &json!({
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": {
                            "input_tokens": 100,
                            "cached_input_tokens": 40,
                            "output_tokens": 20
                        },
                        "last_token_usage": {
                            "input_tokens": 80,
                            "cached_input_tokens": 30,
                            "output_tokens": 12
                        }
                    }
                }
            }),
        );

        let turn = snapshot.turns.get("turn-1").unwrap();
        assert_eq!(turn.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(turn.effort.as_deref(), Some("high"));
        assert_eq!(turn.usage.as_ref().unwrap().input_tokens, 50);
        assert_eq!(turn.usage.as_ref().unwrap().cached_input_tokens, Some(30));
    }

    #[test]
    fn timeline_cache_retries_an_incomplete_jsonl_row_after_append() {
        use std::io::Write as _;

        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "monet-codex-timeline-cache-{}-{unique}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            br#"{"type":"turn_context","payload":{"turn_id":"turn-partial","model":"gpt-"#,
        )
        .unwrap();

        let first = read_timeline_snapshot(path.to_str()).unwrap();
        assert!(first.turns.is_empty());

        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(br#"5.6-sol","effort":"medium"}}"#).unwrap();
        file.flush().unwrap();

        let complete = read_timeline_snapshot(path.to_str()).unwrap();
        let turn = complete.turns.get("turn-partial").unwrap();
        assert_eq!(turn.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(turn.effort.as_deref(), Some("medium"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn usage_cache_refreshes_when_codex_appends_a_turn() {
        use std::io::Write as _;

        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "monet-codex-usage-cache-{}-{unique}.jsonl",
            std::process::id()
        ));
        let context = json!({
            "type": "turn_context",
            "payload": { "turn_id": "turn-1", "model": "model-1" }
        });
        let usage = json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": { "input_tokens": 10, "output_tokens": 2 },
                    "last_token_usage": { "input_tokens": 10, "output_tokens": 2 }
                }
            }
        });
        fs::write(&path, format!("{context}\n{usage}\n")).unwrap();

        let first = read_usage_snapshot(path.to_str()).unwrap();
        assert_eq!(first.model.as_deref(), Some("model-1"));
        assert_eq!(first.total.output_tokens, 2);

        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        let context = json!({
            "type": "turn_context",
            "payload": { "turn_id": "turn-2", "model": "model-2" }
        });
        let usage = json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": { "input_tokens": 20, "output_tokens": 5 },
                    "last_token_usage": { "input_tokens": 10, "output_tokens": 3 }
                }
            }
        });
        writeln!(file, "{context}").unwrap();
        writeln!(file, "{usage}").unwrap();
        file.flush().unwrap();

        let refreshed = read_usage_snapshot(path.to_str()).unwrap();
        assert_eq!(refreshed.model.as_deref(), Some("model-2"));
        assert_eq!(refreshed.total.output_tokens, 5);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn maps_collaboration_item_to_generic_tool_call() {
        let session = SessionRef::new(default_instance().unwrap(), "parent").unwrap();
        let segments = map_item_segments(
            &session,
            &json!({
                "id": "collab-1",
                "type": "collabAgentToolCall",
                "tool": "spawnAgent",
                "receiverThreadIds": ["child-1"],
                "prompt": "Review the engine boundary",
                "status": "completed",
                "agentsStates": {
                    "child-1": { "status": "running", "message": null }
                }
            }),
        )
        .unwrap();
        assert!(matches!(
            segments.as_slice(),
            [Segment::ToolCall { name, input, .. }]
                if name == "spawnAgent"
                    && input.get("receiverThreadIds").and_then(Value::as_array).is_some()
        ));
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

    #[test]
    #[ignore = "requires an installed Codex CLI and explicit MONET_CODEX_EMPTY_THREAD_SMOKE=1"]
    fn installed_source_reads_empty_runtime_thread_before_first_turn() {
        assert_eq!(
            std::env::var("MONET_CODEX_EMPTY_THREAD_SMOKE").as_deref(),
            Ok("1"),
            "set MONET_CODEX_EMPTY_THREAD_SMOKE=1 to run the empty-thread smoke test"
        );
        let supervisor = CodexSupervisor::new();
        let source = CodexSource::new(Arc::clone(&supervisor)).unwrap();
        let runtime = crate::engines::codex::CodexRuntime::new(supervisor).unwrap();
        let session = resolve_ready(runtime.create_session(CreateSessionRequest {
            project: ProjectRef::new(default_instance().unwrap(), "empty-thread-smoke").unwrap(),
            cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            options: BTreeMap::from([("ephemeral".into(), Value::Bool(true))]),
        }))
        .unwrap();

        assert!(source.timeline(&session.session).unwrap().is_empty());
        resolve_ready(runtime.close_session(session.session)).unwrap();
    }
}
