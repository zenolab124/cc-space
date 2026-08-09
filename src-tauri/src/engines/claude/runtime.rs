use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use chrono::Utc;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::default_instance;
use super::source::map_block;
use crate::engines::core::*;
use crate::permission::{PermissionObserverEvent, PermissionObserverHandle};
use crate::streaming::{StreamEvent, StreamingObserverEvent, StreamingObserverHandle};

#[derive(Clone)]
struct SessionState {
    runtime_id: RuntimeId,
    generation: u64,
    sequence: u64,
    active_turn_id: Option<String>,
}

pub struct ClaudeRuntime {
    instance: EngineInstanceId,
    app: AppHandle,
    sessions: Mutex<HashMap<String, SessionState>>,
    event_sinks: Mutex<Vec<RuntimeEventSink>>,
    observer: Mutex<Option<StreamingObserverHandle>>,
    permission_observer: Mutex<Option<PermissionObserverHandle>>,
    /// requestId → 发出 InteractionRequested 时构造的 reference。
    /// Resolved 时原样取回，保证与 coordinator 里 pending_interactions 的条目按值相等
    /// （turn_id 期间可能变化，现算会匹配不上导致 pending 永不出队）。
    interaction_refs: Mutex<HashMap<String, InteractionRef>>,
}

impl ClaudeRuntime {
    pub fn new(app: AppHandle) -> EngineResult<Arc<Self>> {
        let runtime = Arc::new(Self {
            instance: default_instance()?,
            app,
            sessions: Mutex::new(HashMap::new()),
            event_sinks: Mutex::new(Vec::new()),
            observer: Mutex::new(None),
            permission_observer: Mutex::new(None),
            interaction_refs: Mutex::new(HashMap::new()),
        });
        let weak = Arc::downgrade(&runtime);
        let handle = crate::streaming::subscribe_observer(Arc::new(move |event| {
            if let Some(runtime) = Weak::upgrade(&weak) {
                runtime.handle_observer_event(event);
            }
        }));
        *runtime
            .observer
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(handle);
        let weak = Arc::downgrade(&runtime);
        let permission_handle = crate::permission::subscribe_observer(Arc::new(move |event| {
            if let Some(runtime) = Weak::upgrade(&weak) {
                runtime.handle_permission_event(event);
            }
        }));
        *runtime
            .permission_observer
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(permission_handle);
        Ok(runtime)
    }

    fn owns_session(&self, session: &SessionRef) -> EngineResult<()> {
        if session.engine() == &self.instance {
            Ok(())
        } else {
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "Claude runtime does not own this session",
            ))
        }
    }

    fn runtime_session(&self, session: SessionRef, new_generation: bool) -> RuntimeSession {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let state = sessions
            .entry(session.native_id().to_string())
            .or_insert(SessionState {
                runtime_id: RuntimeId(format!("claude-{}", session.native_id())),
                generation: 1,
                sequence: 0,
                active_turn_id: None,
            });
        if new_generation && state.sequence > 0 {
            state.generation += 1;
            state.sequence = 0;
            state.active_turn_id = None;
        }
        RuntimeSession {
            session,
            runtime_id: state.runtime_id.clone(),
            generation: state.generation,
        }
    }

    fn emit(&self, session_id: &str, event: NormalizedRuntimeEvent) {
        let envelope = {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let state = sessions
                .entry(session_id.to_string())
                .or_insert(SessionState {
                    runtime_id: RuntimeId(format!("claude-{session_id}")),
                    generation: 1,
                    sequence: 0,
                    active_turn_id: None,
                });
            match &event {
                NormalizedRuntimeEvent::TurnStarted { turn_id } => {
                    state.active_turn_id = Some(turn_id.clone())
                }
                NormalizedRuntimeEvent::TurnCompleted { .. } => state.active_turn_id = None,
                _ => {}
            }
            state.sequence += 1;
            let Ok(session) = SessionRef::new(self.instance.clone(), session_id) else {
                return;
            };
            RuntimeEventEnvelope {
                session,
                runtime_id: state.runtime_id.clone(),
                generation: state.generation,
                sequence: state.sequence,
                timestamp: Utc::now().to_rfc3339(),
                event,
            }
        };
        let sinks = self
            .event_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        for sink in &sinks {
            sink(envelope.clone());
        }
    }

    fn active_turn(&self, session_id: &str) -> String {
        self.sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(session_id)
            .and_then(|state| state.active_turn_id.clone())
            .unwrap_or_else(|| format!("turn-{session_id}"))
    }

    fn handle_observer_event(&self, event: StreamingObserverEvent) {
        match event {
            StreamingObserverEvent::Stream(event) => self.handle_stream_event(event),
            StreamingObserverEvent::Signal { name, payload }
                if name == "session-process-exited" =>
            {
                if let Some(session_id) = payload.get("session_id").and_then(Value::as_str) {
                    self.emit(session_id, NormalizedRuntimeEvent::RuntimeExited);
                }
            }
            StreamingObserverEvent::Signal { .. } => {}
        }
    }

    fn handle_stream_event(&self, event: StreamEvent) {
        let session_id = event.session_id().to_string();
        let turn_id = self.active_turn(&session_id);
        match event {
            StreamEvent::BlockStart {
                message_id,
                index,
                content_block,
                ..
            } => {
                let item_id = format!("{message_id}:{index}");
                self.emit(
                    &session_id,
                    NormalizedRuntimeEvent::ItemStarted {
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        status: ItemStatus::Running,
                    },
                );
                if let Ok(session) = SessionRef::new(self.instance.clone(), &session_id) {
                    if let Ok(segment) = map_block(&session, &message_id, content_block) {
                        self.emit(
                            &session_id,
                            NormalizedRuntimeEvent::ItemDelta {
                                turn_id,
                                item_id,
                                segment,
                            },
                        );
                    }
                }
            }
            StreamEvent::BlockDelta {
                message_id,
                index,
                delta,
                ..
            } => {
                let item_id = format!("{message_id}:{index}");
                let segment = match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => delta
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| Segment::Text { text: text.into() }),
                    Some("thinking_delta") => {
                        delta.get("thinking").and_then(Value::as_str).map(|text| {
                            Segment::Reasoning {
                                text: text.into(),
                                visibility: ReasoningVisibility::Summary,
                            }
                        })
                    }
                    Some("input_json_delta") => delta
                        .get("partial_json")
                        .and_then(Value::as_str)
                        .map(|text| Segment::Unknown {
                            type_name: "toolInputDelta".into(),
                            summary: Some(text.chars().take(512).collect()),
                        }),
                    _ => None,
                };
                if let Some(segment) = segment {
                    self.emit(
                        &session_id,
                        NormalizedRuntimeEvent::ItemDelta {
                            turn_id,
                            item_id,
                            segment,
                        },
                    );
                }
            }
            StreamEvent::BlockStop {
                message_id, index, ..
            } => self.emit(
                &session_id,
                NormalizedRuntimeEvent::ItemCompleted {
                    turn_id,
                    item_id: format!("{message_id}:{index}"),
                    status: ItemStatus::Completed,
                },
            ),
            StreamEvent::ToolResults { content, .. } => {
                if let Ok(session) = SessionRef::new(self.instance.clone(), &session_id) {
                    for block in content {
                        let item_id = match &block {
                            crate::models::ContentBlock::ToolResult { tool_use_id, .. } => {
                                format!("tool-result:{tool_use_id}")
                            }
                            _ => continue,
                        };
                        if let Ok(segment) = map_block(&session, "stream", block) {
                            self.emit(
                                &session_id,
                                NormalizedRuntimeEvent::ItemDelta {
                                    turn_id: turn_id.clone(),
                                    item_id,
                                    segment,
                                },
                            );
                        }
                    }
                }
            }
            StreamEvent::AssistantMessage {
                message_id,
                content,
                ..
            } => {
                if let Ok(session) = SessionRef::new(self.instance.clone(), &session_id) {
                    for (index, block) in content.into_iter().enumerate() {
                        if let Ok(segment) = map_block(&session, &message_id, block) {
                            self.emit(
                                &session_id,
                                NormalizedRuntimeEvent::ItemDelta {
                                    turn_id: turn_id.clone(),
                                    item_id: format!("{message_id}:{index}"),
                                    segment,
                                },
                            );
                        }
                    }
                }
            }
            StreamEvent::Result { .. } => self.emit(
                &session_id,
                NormalizedRuntimeEvent::TurnCompleted {
                    turn_id,
                    status: TurnStatus::Completed,
                    error: None,
                },
            ),
            StreamEvent::Error { message, .. } => self.emit(
                &session_id,
                NormalizedRuntimeEvent::TurnCompleted {
                    turn_id,
                    status: TurnStatus::Failed,
                    error: Some(message),
                },
            ),
        }
    }

    fn handle_permission_event(&self, event: PermissionObserverEvent) {
        match event {
            PermissionObserverEvent::Requested {
                request_id,
                session_id,
                tool_name,
                input,
            } => self.handle_permission_requested(request_id, session_id, tool_name, input),
            PermissionObserverEvent::Resolved {
                request_id,
                session_id,
            } => self.handle_permission_resolved(&request_id, &session_id),
        }
    }

    fn handle_permission_requested(
        &self,
        request_id: String,
        session_id: String,
        tool_name: String,
        input: Value,
    ) {
        let runtime = self.runtime_session(
            match SessionRef::new(self.instance.clone(), &session_id) {
                Ok(session) => session,
                Err(_) => return,
            },
            false,
        );
        let kind = match tool_name.as_str() {
            "Bash" => InteractionKind::Command,
            "AskUserQuestion" => InteractionKind::Question,
            "ExitPlanMode" | "EnterPlanMode" => InteractionKind::Plan,
            _ => InteractionKind::Unknown,
        };
        let reference = InteractionRef {
            session: runtime.session,
            runtime_id: runtime.runtime_id,
            request_id: request_id.clone(),
            turn_id: Some(self.active_turn(&session_id)),
        };
        self.interaction_refs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(request_id, reference.clone());
        self.emit(
            &session_id,
            NormalizedRuntimeEvent::InteractionRequested {
                request: InteractionRequest {
                    reference,
                    kind,
                    title: Some(tool_name),
                    payload: bounded_permission_payload(input),
                    options: vec![
                        InteractionOption {
                            id: "allow".into(),
                            label: "Allow".into(),
                            dangerous: false,
                        },
                        InteractionOption {
                            id: "deny".into(),
                            label: "Deny".into(),
                            dangerous: true,
                        },
                    ],
                },
            },
        );
    }

    /// 权限请求终结（无论经 legacy `respond_permission` 还是引擎 `respond`）：
    /// 补发 InteractionResolved，让 coordinator 出队并退出 AwaitingInteraction。
    /// 不做这一步，legacy 通道处理过的请求会在引擎侧永久残留。
    fn handle_permission_resolved(&self, request_id: &str, session_id: &str) {
        let reference = self
            .interaction_refs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(request_id);
        let Some(reference) = reference else {
            return;
        };
        self.emit(
            session_id,
            NormalizedRuntimeEvent::InteractionResolved {
                reference,
                decision: "resolved".into(),
            },
        );
    }
}

impl AgentRuntime for ClaudeRuntime {
    fn create_session(&self, request: CreateSessionRequest) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            if request.project.engine() != &self.instance {
                return Err(EngineError::new(
                    EngineErrorKind::NotFound,
                    "Claude runtime does not own this project",
                ));
            }
            let session = SessionRef::new(self.instance.clone(), uuid::Uuid::new_v4().to_string())?;
            let runtime = self.runtime_session(session, true);
            self.emit(
                runtime.session.native_id(),
                NormalizedRuntimeEvent::SessionAttached,
            );
            Ok(runtime)
        })
    }

    fn attach_session(
        &self,
        session: SessionRef,
        _options: AttachOptions,
    ) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            self.owns_session(&session)?;
            let runtime = self.runtime_session(session, true);
            self.emit(
                runtime.session.native_id(),
                NormalizedRuntimeEvent::SessionAttached,
            );
            Ok(runtime)
        })
    }

    fn start_turn(
        &self,
        session: SessionRef,
        request: TurnRequest,
    ) -> EngineFuture<'_, TurnHandle> {
        Box::pin(async move {
            self.owns_session(&session)?;
            let cwd = request
                .options
                .get("cwd")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    EngineError::new(
                        EngineErrorKind::InvalidIdentity,
                        "Claude turn requires a working directory",
                    )
                })?
                .to_string();
            let (message, images) = map_input(request.input);
            let options = request.options;
            let app = self.app.clone();
            let session_id = session.native_id().to_string();
            let blocking_session_id = session_id.clone();
            let turn_id = uuid::Uuid::new_v4().to_string();
            self.emit(
                &session_id,
                NormalizedRuntimeEvent::TurnStarted {
                    turn_id: turn_id.clone(),
                },
            );
            let start_result = tauri::async_runtime::spawn_blocking(move || {
                crate::streaming::send_message(
                    &app,
                    &blocking_session_id,
                    &cwd,
                    &message,
                    option_string(&options, "model").as_deref(),
                    option_string(&options, "effort").as_deref(),
                    option_string(&options, "channel").as_deref(),
                    option_bool(&options, "advisor"),
                    option_bool(&options, "chrome"),
                    option_string(&options, "forkSource").as_deref(),
                    option_string(&options, "extraArgs").as_deref(),
                    Some(&images),
                    option_string(&options, "permissionMode").as_deref(),
                    Vec::new(),
                    option_bool(&options, "forceNew"),
                )
            })
            .await
            .map_err(|error| EngineError::new(EngineErrorKind::Internal, error.to_string()))?
            .map_err(|error| EngineError::new(EngineErrorKind::Unavailable, error));
            if let Err(error) = start_result {
                self.emit(
                    &session_id,
                    NormalizedRuntimeEvent::TurnCompleted {
                        turn_id,
                        status: TurnStatus::Failed,
                        error: Some(error.message.clone()),
                    },
                );
                return Err(error);
            }
            let runtime = self.runtime_session(session.clone(), false);
            Ok(TurnHandle {
                reference: TurnRef {
                    session,
                    runtime_id: runtime.runtime_id,
                    native_turn_id: turn_id,
                },
            })
        })
    }

    fn send_input_while_running(
        &self,
        _turn: TurnRef,
        _input: Vec<InputItem>,
    ) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            Err(EngineError::new(
                EngineErrorKind::Unsupported,
                "Claude runtime does not accept input while a turn is running",
            ))
        })
    }

    fn interrupt_turn(&self, turn: TurnRef) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            self.owns_session(&turn.session)?;
            crate::streaming::interrupt_session(turn.session.native_id())
                .map_err(|error| EngineError::new(EngineErrorKind::Io, error))?;
            self.emit(
                turn.session.native_id(),
                NormalizedRuntimeEvent::TurnCompleted {
                    turn_id: turn.native_turn_id,
                    status: TurnStatus::Interrupted,
                    error: None,
                },
            );
            Ok(())
        })
    }

    fn respond(
        &self,
        request: InteractionRef,
        response: InteractionResponse,
    ) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            self.owns_session(&request.session)?;
            let allow = matches!(
                response.decision.as_str(),
                "allow" | "allowAlways" | "accept" | "approve"
            );
            let updated_input = response
                .payload
                .as_ref()
                .and_then(|payload| payload.get("updatedInput"))
                .cloned();
            if !crate::permission::PermissionService::respond(
                &request.request_id,
                allow,
                response
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.get("message"))
                    .and_then(Value::as_str)
                    .map(String::from),
                updated_input,
                response
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.get("updatedPermissions"))
                    .cloned(),
            ) {
                return Err(EngineError::new(
                    EngineErrorKind::NotFound,
                    "Claude interaction request is no longer pending",
                ));
            }
            let session_id = request.session.native_id().to_string();
            // 与 observer 的 Resolved 路径互斥：谁先取到 reference 谁负责发事件。
            // PermissionService::respond 会唤醒 handle_connection 线程走 Resolved 通知，
            // 两边都无条件 emit 就会双发；用 remove 的返回值裁定，恰好一次。
            let reference = self
                .interaction_refs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&request.request_id);
            if let Some(reference) = reference {
                self.emit(
                    &session_id,
                    NormalizedRuntimeEvent::InteractionResolved {
                        reference,
                        decision: response.decision,
                    },
                );
            }
            Ok(())
        })
    }

    fn close_session(&self, session: SessionRef) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            self.owns_session(&session)?;
            crate::streaming::close_session(session.native_id());
            self.emit(session.native_id(), NormalizedRuntimeEvent::SessionDetached);
            self.sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(session.native_id());
            Ok(())
        })
    }

    fn subscribe_events(&self, sink: RuntimeEventSink) -> EngineResult<SubscriptionHandle> {
        self.event_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sink);
        Ok(Box::new(()))
    }
}

fn map_input(input: Vec<InputItem>) -> (String, Vec<Value>) {
    let mut text = Vec::new();
    let mut images = Vec::new();
    for item in input {
        match item {
            InputItem::Text { text: value } => text.push(value),
            InputItem::Image { media_type, data } => images.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": data }
            })),
            InputItem::File { path } => text.push(format!("@{path}")),
        }
    }
    (text.join("\n"), images)
}

fn option_string(options: &std::collections::BTreeMap<String, Value>, key: &str) -> Option<String> {
    options.get(key).and_then(Value::as_str).map(String::from)
}

fn option_bool(options: &std::collections::BTreeMap<String, Value>, key: &str) -> bool {
    options.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn bounded_permission_payload(payload: Value) -> Value {
    const MAX_BYTES: usize = 32 * 1024;
    if serde_json::to_vec(&payload)
        .map(|encoded| encoded.len() <= MAX_BYTES)
        .unwrap_or(false)
    {
        payload
    } else {
        json!({ "truncated": true, "reason": "permission payload exceeded the size limit" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_input_maps_to_existing_claude_transport() {
        let (text, images) = map_input(vec![
            InputItem::Text {
                text: "hello".into(),
            },
            InputItem::File {
                path: "/workspace/file.rs".into(),
            },
            InputItem::Image {
                media_type: "image/png".into(),
                data: "encoded".into(),
            },
        ]);
        assert_eq!(text, "hello\n@/workspace/file.rs");
        assert_eq!(images.len(), 1);
    }
}
