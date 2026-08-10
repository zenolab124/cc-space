use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::models::{ContentBlock, MessageContent, SessionRecord, TokenUsage, ToolResultContent};

use super::super::core::*;
use super::default_instance;

pub struct ClaudeSource {
    instance: EngineInstanceId,
    change_sinks: Arc<Mutex<Vec<SourceChangeSink>>>,
    projects_cache: Mutex<Option<Vec<crate::models::Project>>>,
}

impl ClaudeSource {
    pub fn new() -> EngineResult<Self> {
        Ok(Self {
            instance: default_instance()?,
            change_sinks: Arc::new(Mutex::new(Vec::new())),
            projects_cache: Mutex::new(None),
        })
    }

    pub fn publish_change(&self, change: SourceChange) {
        *self
            .projects_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        let sinks = self
            .change_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        for sink in &sinks {
            sink(change.clone());
        }
    }

    fn project_snapshot(&self) -> Vec<crate::models::Project> {
        if let Some(projects) = self
            .projects_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .cloned()
        {
            return projects;
        }

        let discovered = crate::discovery::discover_all();
        let mut cache = self
            .projects_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.get_or_insert_with(|| discovered.clone()).clone()
    }

    fn owns_project(&self, project: &ProjectRef) -> EngineResult<()> {
        if project.engine() == &self.instance {
            Ok(())
        } else {
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "Claude source does not own this project",
            ))
        }
    }

    fn owns_session(&self, session: &SessionRef) -> EngineResult<()> {
        if session.engine() == &self.instance {
            Ok(())
        } else {
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "Claude source does not own this session",
            ))
        }
    }

    fn projects(&self) -> EngineResult<Vec<CoreProject>> {
        self.project_snapshot()
            .into_iter()
            .map(|project| {
                let reference = ProjectRef::new(self.instance.clone(), project.id)?;
                let display_name = Path::new(&project.display_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(&project.display_path)
                    .to_string();
                Ok(CoreProject {
                    reference,
                    display_name,
                    display_path: Some(project.display_path),
                    session_count: project.session_count,
                    last_active: project.last_active.map(epoch_seconds),
                })
            })
            .collect()
    }

    fn find_session(
        &self,
        native_id: &str,
    ) -> EngineResult<(String, crate::models::SessionSummary)> {
        for project in self.project_snapshot() {
            if let Some(session) = project
                .sessions
                .into_iter()
                .find(|session| session.id == native_id)
            {
                return Ok((project.id, session));
            }
        }
        Err(EngineError::new(
            EngineErrorKind::NotFound,
            "Claude session was not found",
        ))
    }

    fn map_summary(
        &self,
        project: ProjectRef,
        summary: crate::models::SessionSummary,
    ) -> EngineResult<CoreSessionSummary> {
        let usage = map_usage(&summary.total_tokens);
        let subagent_tokens = json!({
            "inputTokens": summary.subagent_tokens.input_tokens,
            "outputTokens": summary.subagent_tokens.output_tokens,
            "cacheCreationInputTokens": summary.subagent_tokens.cache_creation_input_tokens,
            "cachedInputTokens": summary.subagent_tokens.cache_read_input_tokens,
        });
        Ok(CoreSessionSummary {
            reference: SessionRef::new(self.instance.clone(), summary.id)?,
            project,
            title: summary.title,
            preview: summary.first_user_message,
            cwd: summary.cwd,
            model: summary.model,
            created_at: summary.timestamp,
            updated_at: Some(epoch_seconds(summary.last_modified)),
            usage: Some(usage),
            source_meta: SourceMetadata::new(BTreeMap::from([
                (
                    "gitBranch".into(),
                    summary.git_branch.map(Value::String).unwrap_or(Value::Null),
                ),
                (
                    "messageCount".into(),
                    Value::Number(summary.message_count.into()),
                ),
                ("fileSize".into(), Value::Number(summary.file_size.into())),
                (
                    "contextWindow".into(),
                    summary
                        .context_window
                        .map(|value| Value::Number(value.into()))
                        .unwrap_or(Value::Null),
                ),
                (
                    "version".into(),
                    summary.version.map(Value::String).unwrap_or(Value::Null),
                ),
                ("subagentTokens".into(), subagent_tokens),
            ]))?,
        })
    }

    fn timeline_records(&self, session: &SessionRef) -> EngineResult<Vec<ConversationRecord>> {
        self.owns_session(session)?;
        let (project_id, _) = self.find_session(session.native_id())?;
        let path = crate::config::projects_dir()
            .join(project_id)
            .join(format!("{}.jsonl", session.native_id()));
        let mut records = Vec::new();
        for (index, record) in crate::parser::parse_messages(&path).into_iter().enumerate() {
            if let Some(record) = map_record(session, record, index)? {
                records.push(record);
            }
        }
        Ok(records)
    }
}

impl SessionSource for ClaudeSource {
    fn list_projects(&self, query: ProjectQuery) -> EngineFuture<'_, ProjectPage> {
        Box::pin(async move {
            let projects = self.projects()?;
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
            let native_project = project.native_id().to_string();
            let source_project = self
                .project_snapshot()
                .into_iter()
                .find(|candidate| candidate.id == native_project)
                .ok_or_else(|| {
                    EngineError::new(EngineErrorKind::NotFound, "Claude project was not found")
                })?;
            let mut sessions = Vec::with_capacity(source_project.sessions.len());
            for summary in source_project.sessions {
                sessions.push(self.map_summary(project.clone(), summary)?);
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
            let records = self.timeline_records(&session)?;
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
            let records = self.timeline_records(&session)?;
            let mut text = String::new();
            for record in records {
                for segment in record.segments {
                    match segment {
                        Segment::Text { text: value, .. }
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
            let (_, summary) = self.find_session(session.native_id())?;
            Ok(SearchDocument {
                session,
                title: summary.title.or(summary.first_user_message),
                text,
            })
        })
    }

    fn resolve_asset(&self, asset: AssetRef) -> EngineFuture<'_, ResolvedAsset> {
        Box::pin(async move {
            self.owns_session(&asset.session)?;
            let (record_id, index) = asset.native_id.rsplit_once(':').ok_or_else(|| {
                EngineError::new(EngineErrorKind::InvalidIdentity, "invalid Claude asset id")
            })?;
            let index = index.parse::<usize>().map_err(|_| {
                EngineError::new(
                    EngineErrorKind::InvalidIdentity,
                    "invalid Claude asset index",
                )
            })?;
            let (project_id, _) = self.find_session(asset.session.native_id())?;
            let (media_type, bytes) = crate::image_protocol::resolve_engine_image(
                &project_id,
                asset.session.native_id(),
                record_id,
                index,
            )
            .ok_or_else(|| {
                EngineError::new(EngineErrorKind::NotFound, "Claude asset was not found")
            })?;
            Ok(ResolvedAsset { media_type, bytes })
        })
    }

    fn session_actions(&self, session: SessionRef) -> EngineFuture<'_, SessionActions> {
        Box::pin(async move {
            self.owns_session(&session)?;
            let (_, summary) = self.find_session(session.native_id())?;
            let runtime_available = crate::claude_locator::locate_lightweight().is_ok();
            let cwd_available = summary
                .cwd
                .as_deref()
                .is_some_and(|cwd| Path::new(cwd).is_dir());
            let resume = if !runtime_available {
                ActionAvailability::unavailable("engine.claude.cliUnavailable")
            } else if !cwd_available {
                ActionAvailability::unavailable("engine.session.cwdUnavailable")
            } else {
                ActionAvailability::available()
            };
            Ok(SessionActions {
                resume: resume.clone(),
                fork: resume.clone(),
                send: resume,
                send_while_running: ActionAvailability::unavailable(
                    "engine.claude.sendWhileRunningUnavailable",
                ),
                interrupt: ActionAvailability::available(),
                open_cwd: if summary.cwd.is_some() {
                    ActionAvailability::available()
                } else {
                    ActionAvailability::unavailable("engine.session.noCwd")
                },
            })
        })
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

fn epoch_seconds(seconds: f64) -> String {
    chrono::DateTime::from_timestamp(seconds as i64, 0)
        .unwrap_or_default()
        .to_rfc3339()
}

fn map_usage(usage: &TokenUsage) -> Usage {
    Usage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: Some(usage.total()),
        cached_input_tokens: Some(usage.cache_read_input_tokens),
        cache_creation_input_tokens: Some(usage.cache_creation_input_tokens),
    }
}

fn map_record(
    session: &SessionRef,
    record: SessionRecord,
    index: usize,
) -> EngineResult<Option<ConversationRecord>> {
    let (id, parent_id, role, timestamp, blocks, usage) = match record {
        SessionRecord::User(record) => {
            let Some(message) = record.message else {
                return Ok(None);
            };
            let blocks = match message.content {
                MessageContent::Text(text) => vec![ContentBlock::Text { text }],
                MessageContent::Blocks(blocks) => blocks,
            };
            (
                record.uuid,
                record.parent_uuid,
                ConversationRole::User,
                record.timestamp,
                blocks,
                None,
            )
        }
        SessionRecord::Assistant(record) => {
            let Some(message) = record.message else {
                return Ok(None);
            };
            (
                record.uuid.or(message.id),
                record.parent_uuid,
                ConversationRole::Assistant,
                record.timestamp,
                message.content,
                message.usage.as_ref().map(map_usage),
            )
        }
        _ => return Ok(None),
    };
    let record_id = id.unwrap_or_else(|| format!("record-{index}"));
    let mut segments = Vec::new();
    for block in blocks {
        segments.push(map_block(session, &record_id, block)?);
    }
    Ok(Some(ConversationRecord {
        id: record_id,
        session: session.clone(),
        turn_id: None,
        parent_id,
        role,
        timestamp,
        segments,
        usage,
        source_meta: SourceMetadata::default(),
    }))
}

pub(crate) fn map_block(
    session: &SessionRef,
    record_id: &str,
    block: ContentBlock,
) -> EngineResult<Segment> {
    Ok(match block {
        ContentBlock::Text { text } => Segment::Text {
            text: bounded_segment_text(text),
            phase: None,
        },
        ContentBlock::Thinking { thinking, .. } => Segment::Reasoning {
            text: bounded_segment_text(thinking),
            visibility: ReasoningVisibility::Summary,
        },
        ContentBlock::ToolUse { id, name, input } => Segment::ToolCall {
            id,
            name,
            input: bounded_segment_value(input),
            title: None,
            presentation: None,
        },
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => Segment::ToolResult {
            call_id: tool_use_id,
            content: match content {
                ToolResultContent::Text(text) => Value::String(bounded_segment_text(text)),
                ToolResultContent::Blocks(blocks) => {
                    bounded_segment_value(serde_json::to_value(blocks).unwrap_or(Value::Null))
                }
            },
            is_error,
            attachments: Vec::new(),
        },
        ContentBlock::Image { source } => Segment::Attachment {
            asset: AssetRef {
                session: session.clone(),
                native_id: format!("{record_id}:{}", source.img_index),
            },
            media_type: source.media_type,
            title: None,
        },
        ContentBlock::Document { source, title } => Segment::Unknown {
            type_name: format!("document:{}", source.media_type),
            summary: title,
        },
        ContentBlock::Unknown(value) => Segment::Unknown {
            type_name: value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            summary: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_returns_stable_cursor() {
        let (page, cursor) = paginate(vec![1, 2, 3], None, Some(2));
        let (next, end) = paginate(vec![1, 2, 3], cursor, Some(2));
        assert_eq!(page, vec![1, 2]);
        assert_eq!(next, vec![3]);
        assert!(end.is_none());
    }

    #[test]
    fn maps_unknown_blocks_without_raw_payload() {
        let session = SessionRef::new(default_instance().unwrap(), "session").unwrap();
        let segment = map_block(
            &session,
            "record",
            ContentBlock::Unknown(serde_json::json!({
                "type": "futureBlock",
                "large": "x".repeat(20_000)
            })),
        )
        .unwrap();
        assert_eq!(
            segment,
            Segment::Unknown {
                type_name: "futureBlock".into(),
                summary: None
            }
        );
    }

    #[test]
    fn usage_mapping_preserves_both_cache_token_kinds() {
        let usage = map_usage(&TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            cache_creation_input_tokens: 3,
            cache_read_input_tokens: 4,
        });

        assert_eq!(usage.total_tokens, Some(10));
        assert_eq!(usage.cache_creation_input_tokens, Some(3));
        assert_eq!(usage.cached_input_tokens, Some(4));
    }
}
