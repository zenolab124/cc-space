use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};

use chrono::Utc;
use serde_json::{json, Map, Value};

use super::app_server::{IncomingMessage, RequestId};
use super::source::{map_item_segments, message_text_phase};
use super::{default_instance, CodexSupervisor};
use crate::engines::core::*;
use crate::session_capabilities::{SessionCapabilityBundle, SessionCapabilityId};

#[derive(Clone)]
struct SessionState {
    runtime_id: RuntimeId,
    generation: u64,
    sequence: u64,
    active_turn_id: Option<String>,
    connection_epoch: Option<u64>,
    resume_params: Map<String, Value>,
}

#[derive(Clone, Copy)]
enum PendingInteractionKind {
    Command,
    FileChange,
    Permissions,
}

#[derive(Clone)]
struct PendingInteraction {
    native_id: RequestId,
    reference: InteractionRef,
    kind: PendingInteractionKind,
    requested_permissions: Option<Value>,
}

pub struct CodexRuntime {
    instance: EngineInstanceId,
    supervisor: Arc<CodexSupervisor>,
    sessions: Mutex<HashMap<String, SessionState>>,
    recovery_lock: Mutex<()>,
    streamed_items: Mutex<HashSet<String>>,
    text_phases: Mutex<HashMap<String, TextPhase>>,
    pending_interactions: Mutex<HashMap<String, PendingInteraction>>,
    event_sinks: Mutex<Vec<RuntimeEventSink>>,
}

impl CodexRuntime {
    pub fn new(supervisor: Arc<CodexSupervisor>) -> EngineResult<Arc<Self>> {
        let runtime = Arc::new(Self {
            instance: default_instance()?,
            supervisor: Arc::clone(&supervisor),
            sessions: Mutex::new(HashMap::new()),
            recovery_lock: Mutex::new(()),
            streamed_items: Mutex::new(HashSet::new()),
            text_phases: Mutex::new(HashMap::new()),
            pending_interactions: Mutex::new(HashMap::new()),
            event_sinks: Mutex::new(Vec::new()),
        });
        let weak = Arc::downgrade(&runtime);
        supervisor.subscribe(Arc::new(move |message| {
            if let Some(runtime) = Weak::upgrade(&weak) {
                runtime.handle_protocol_message(message);
            }
        }));
        Ok(runtime)
    }

    fn read_models(&self) -> EngineResult<Vec<ModelDescriptor>> {
        let mut models = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..20 {
            let response = self
                .supervisor
                .request("model/list", json!({ "cursor": cursor, "limit": 100 }))?;
            let data = response
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    EngineError::new(
                        EngineErrorKind::Protocol,
                        "Codex returned an invalid model list",
                    )
                })?;
            for model in data {
                models.push(ModelDescriptor {
                    id: string_field(model, "id").unwrap_or_default(),
                    model: string_field(model, "model").unwrap_or_default(),
                    display_name: string_field(model, "displayName").unwrap_or_default(),
                    description: string_field(model, "description"),
                    is_default: model
                        .get("isDefault")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    hidden: model
                        .get("hidden")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    default_effort: model
                        .get("defaultReasoningEffort")
                        .and_then(Value::as_str)
                        .map(String::from),
                    efforts: model
                        .get("supportedReasoningEfforts")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|option| {
                            Some(ModelEffortOption {
                                id: string_field(option, "reasoningEffort")?,
                                description: string_field(option, "description"),
                            })
                        })
                        .collect(),
                    default_service_tier: model
                        .get("defaultServiceTier")
                        .and_then(Value::as_str)
                        .map(String::from),
                    service_tiers: model
                        .get("serviceTiers")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|tier| {
                            Some(ModelServiceTier {
                                id: string_field(tier, "id")?,
                                name: string_field(tier, "name")?,
                                description: string_field(tier, "description"),
                            })
                        })
                        .collect(),
                });
            }
            cursor = response
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(String::from);
            if cursor.is_none() {
                return Ok(models);
            }
        }
        Err(EngineError::new(
            EngineErrorKind::Protocol,
            "Codex model list exceeded the pagination safety limit",
        ))
    }

    fn owns_session(&self, session: &SessionRef) -> EngineResult<()> {
        if session.engine() == &self.instance {
            Ok(())
        } else {
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "Codex runtime does not own this session",
            ))
        }
    }

    fn runtime_session(
        &self,
        thread_id: &str,
        new_generation: bool,
    ) -> EngineResult<RuntimeSession> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let state = sessions
            .entry(thread_id.to_string())
            .or_insert(SessionState {
                runtime_id: RuntimeId(format!("codex-{thread_id}")),
                generation: 1,
                sequence: 0,
                active_turn_id: None,
                connection_epoch: None,
                resume_params: resume_params_from_request(thread_id, &Map::new()),
            });
        if new_generation && state.sequence > 0 {
            state.generation += 1;
            state.sequence = 0;
            state.active_turn_id = None;
        }
        Ok(RuntimeSession {
            session: SessionRef::new(self.instance.clone(), thread_id)?,
            runtime_id: state.runtime_id.clone(),
            generation: state.generation,
            source_meta: BTreeMap::new(),
        })
    }

    fn runtime_session_from_response(
        &self,
        thread_id: &str,
        new_generation: bool,
        response: &Value,
        requested_provider: Option<&str>,
        resume_params: Map<String, Value>,
        connection_epoch: u64,
    ) -> EngineResult<RuntimeSession> {
        let provider = response_model_provider(response).ok_or_else(|| {
            EngineError::new(
                EngineErrorKind::Protocol,
                "Codex thread response has no model provider",
            )
        })?;
        if requested_provider.is_some_and(|requested| requested != provider) {
            return Err(EngineError::new(
                EngineErrorKind::Conflict,
                "Codex resumed with a different model provider than requested",
            ));
        }
        let mut runtime = self.runtime_session(thread_id, new_generation)?;
        if let Some(state) = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(thread_id)
        {
            state.connection_epoch = Some(connection_epoch);
            state.resume_params = resume_params;
        }
        runtime
            .source_meta
            .insert("modelProvider".into(), Value::String(provider.to_string()));
        Ok(runtime)
    }

    fn session_resume_snapshot(&self, thread_id: &str) -> (Option<u64>, Map<String, Value>) {
        self.sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(thread_id)
            .map(|state| (state.connection_epoch, state.resume_params.clone()))
            .unwrap_or_else(|| (None, resume_params_from_request(thread_id, &Map::new())))
    }

    fn bind_session_epoch(&self, thread_id: &str, connection_epoch: u64) {
        if let Some(state) = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(thread_id)
        {
            state.connection_epoch = Some(connection_epoch);
        }
    }

    fn clear_session_transients(&self, thread_id: &str) {
        let item_prefix = format!("{thread_id}\u{1f}");
        self.streamed_items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|key| !key.starts_with(&item_prefix));
        self.text_phases
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|key, _| !key.starts_with(&item_prefix));
        self.pending_interactions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|_, pending| pending.reference.session.native_id() != thread_id);
    }

    fn resume_session(&self, session: &SessionRef, force: bool) -> EngineResult<RuntimeSession> {
        let recovery = self
            .recovery_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current_epoch = self.supervisor.current_connection_epoch();
        let (bound_epoch, params) = self.session_resume_snapshot(session.native_id());
        if !should_resume_session(bound_epoch, current_epoch, force) {
            drop(recovery);
            return self.runtime_session(session.native_id(), false);
        }

        let requested_provider = params
            .get("modelProvider")
            .and_then(Value::as_str)
            .map(String::from);
        let (response, connection_epoch) = self
            .supervisor
            .request_with_epoch("thread/resume", Value::Object(params.clone()))?;
        let runtime = self.runtime_session_from_response(
            session.native_id(),
            true,
            &response,
            requested_provider.as_deref(),
            params,
            connection_epoch,
        )?;
        self.clear_session_transients(session.native_id());
        drop(recovery);
        self.emit(session.native_id(), NormalizedRuntimeEvent::SessionAttached);
        Ok(runtime)
    }

    fn ensure_session_loaded(&self, session: &SessionRef) -> EngineResult<()> {
        self.supervisor.ensure_ready()?;
        let current_epoch = self.supervisor.current_connection_epoch();
        let (bound_epoch, _) = self.session_resume_snapshot(session.native_id());
        if should_resume_session(bound_epoch, current_epoch, false) {
            self.resume_session(session, false)?;
        }
        Ok(())
    }

    fn emit(&self, thread_id: &str, event: NormalizedRuntimeEvent) {
        let envelope = {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let state = sessions
                .entry(thread_id.to_string())
                .or_insert(SessionState {
                    runtime_id: RuntimeId(format!("codex-{thread_id}")),
                    generation: 1,
                    sequence: 0,
                    active_turn_id: None,
                    connection_epoch: None,
                    resume_params: resume_params_from_request(thread_id, &Map::new()),
                });
            match &event {
                NormalizedRuntimeEvent::TurnStarted { turn_id } => {
                    state.active_turn_id = Some(turn_id.clone());
                }
                NormalizedRuntimeEvent::TurnCompleted { .. } => state.active_turn_id = None,
                _ => {}
            }
            state.sequence += 1;
            RuntimeEventEnvelope {
                session: match SessionRef::new(self.instance.clone(), thread_id) {
                    Ok(session) => session,
                    Err(_) => return,
                },
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

    fn handle_protocol_message(&self, message: IncomingMessage) {
        match message {
            IncomingMessage::Notification { method, params } => {
                self.handle_notification(&method, params)
            }
            IncomingMessage::ServerRequest { id, method, params } => {
                self.handle_server_request(id, &method, params)
            }
            IncomingMessage::Response { .. } | IncomingMessage::ErrorResponse { .. } => {}
        }
    }

    fn handle_notification(&self, method: &str, params: Value) {
        let Some(thread_id) = string_field(&params, "threadId") else {
            return;
        };
        match method {
            "turn/started" => {
                if let Some(turn_id) = params.get("turn").and_then(|turn| string_field(turn, "id"))
                {
                    self.emit(&thread_id, NormalizedRuntimeEvent::TurnStarted { turn_id });
                }
            }
            "item/started" | "item/completed" => {
                let Some(turn_id) = string_field(&params, "turnId") else {
                    return;
                };
                let Some(item) = params.get("item") else {
                    return;
                };
                let item_id = string_field(item, "id").unwrap_or_else(|| "unknown-item".into());
                let item_key = runtime_item_key(&thread_id, &turn_id, &item_id);
                if method == "item/started"
                    && item.get("type").and_then(Value::as_str) == Some("agentMessage")
                {
                    if let Some(phase) =
                        message_text_phase(item.get("phase").and_then(Value::as_str))
                    {
                        self.text_phases
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .insert(item_key.clone(), phase);
                    }
                }
                let status = if method == "item/started" {
                    ItemStatus::Running
                } else {
                    item_status(item.get("status").and_then(Value::as_str))
                };
                let lifecycle = if method == "item/started" {
                    NormalizedRuntimeEvent::ItemStarted {
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        status,
                    }
                } else {
                    NormalizedRuntimeEvent::ItemCompleted {
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        status,
                    }
                };
                self.emit(&thread_id, lifecycle);
                if method == "item/started"
                    && item.get("type").and_then(Value::as_str) == Some("commandExecution")
                {
                    self.emit(
                        &thread_id,
                        NormalizedRuntimeEvent::ItemDelta {
                            turn_id: turn_id.clone(),
                            item_id: item_id.clone(),
                            segment: Segment::CommandExecution {
                                id: item_id.clone(),
                                command: item
                                    .get("command")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                cwd: item.get("cwd").and_then(Value::as_str).map(String::from),
                                output: None,
                                status: ItemStatus::Running,
                            },
                        },
                    );
                }
                if method != "item/completed"
                    || item.get("type").and_then(Value::as_str) == Some("userMessage")
                {
                    return;
                }
                let was_streamed = self
                    .streamed_items
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&item_key);
                self.text_phases
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&item_key);
                if was_streamed {
                    return;
                }
                let Ok(session) = SessionRef::new(self.instance.clone(), &thread_id) else {
                    return;
                };
                if let Ok(segments) = map_item_segments(&session, item) {
                    for segment in segments {
                        self.emit(
                            &thread_id,
                            NormalizedRuntimeEvent::ItemDelta {
                                turn_id: turn_id.clone(),
                                item_id: item_id.clone(),
                                segment,
                            },
                        );
                    }
                }
            }
            "item/agentMessage/delta"
            | "item/reasoning/textDelta"
            | "item/reasoning/summaryTextDelta"
            | "item/commandExecution/outputDelta" => {
                let Some(turn_id) = string_field(&params, "turnId") else {
                    return;
                };
                let Some(item_id) = string_field(&params, "itemId") else {
                    return;
                };
                let Some(delta) = string_field(&params, "delta") else {
                    return;
                };
                self.streamed_items
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(runtime_item_key(&thread_id, &turn_id, &item_id));
                let segment = match method {
                    "item/agentMessage/delta" => Segment::Text {
                        text: bounded_segment_text(delta),
                        phase: self
                            .text_phases
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .get(&runtime_item_key(&thread_id, &turn_id, &item_id))
                            .copied(),
                    },
                    "item/commandExecution/outputDelta" => Segment::CommandExecution {
                        id: item_id.clone(),
                        command: String::new(),
                        cwd: None,
                        output: Some(bounded_segment_text(delta)),
                        status: ItemStatus::Running,
                    },
                    _ => Segment::Reasoning {
                        text: bounded_segment_text(delta),
                        visibility: ReasoningVisibility::Summary,
                    },
                };
                self.emit(
                    &thread_id,
                    NormalizedRuntimeEvent::ItemDelta {
                        turn_id,
                        item_id,
                        segment,
                    },
                );
            }
            "turn/completed" => {
                let Some(turn) = params.get("turn") else {
                    return;
                };
                let Some(turn_id) = string_field(turn, "id") else {
                    return;
                };
                let status_value = turn.get("status").and_then(Value::as_str);
                let status = match status_value {
                    Some("interrupted") => TurnStatus::Interrupted,
                    Some("failed") => TurnStatus::Failed,
                    _ => TurnStatus::Completed,
                };
                let error = turn
                    .get("error")
                    .and_then(|error| string_field(error, "message"));
                self.pending_interactions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .retain(|_, pending| {
                        pending.reference.session.native_id() != thread_id
                            || pending.reference.turn_id.as_deref() != Some(turn_id.as_str())
                    });
                let item_prefix = format!("{thread_id}\u{1f}{turn_id}\u{1f}");
                self.streamed_items
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .retain(|item| !item.starts_with(&item_prefix));
                self.text_phases
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .retain(|item, _| !item.starts_with(&item_prefix));
                self.emit(
                    &thread_id,
                    NormalizedRuntimeEvent::TurnCompleted {
                        turn_id,
                        status,
                        error,
                    },
                );
            }
            "thread/closed" => {
                self.pending_interactions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .retain(|_, pending| pending.reference.session.native_id() != thread_id);
                self.emit(&thread_id, NormalizedRuntimeEvent::RuntimeExited);
            }
            _ => {}
        }
    }

    fn handle_server_request(&self, id: RequestId, method: &str, params: Value) {
        let kind = match method {
            "item/commandExecution/requestApproval" => PendingInteractionKind::Command,
            "item/fileChange/requestApproval" => PendingInteractionKind::FileChange,
            "item/permissions/requestApproval" => PendingInteractionKind::Permissions,
            _ => {
                if method.ends_with("/requestApproval") {
                    log::warn!("declining unsupported Codex approval method={method}");
                    let _ = self
                        .supervisor
                        .respond(id, json!({ "decision": "decline" }));
                }
                return;
            }
        };
        let decline = match kind {
            PendingInteractionKind::Permissions => json!({ "permissions": {}, "scope": "turn" }),
            PendingInteractionKind::Command | PendingInteractionKind::FileChange => {
                json!({ "decision": "decline" })
            }
        };
        let Some(thread_id) = string_field(&params, "threadId") else {
            let _ = self.supervisor.respond(id, decline);
            return;
        };
        let Some(turn_id) = string_field(&params, "turnId") else {
            let _ = self.supervisor.respond(id, decline);
            return;
        };
        let request_id = request_id_key(&id);
        let runtime = match self.runtime_session(&thread_id, false) {
            Ok(runtime) => runtime,
            Err(_) => return,
        };
        let reference = InteractionRef {
            session: runtime.session,
            runtime_id: runtime.runtime_id,
            request_id: request_id.clone(),
            turn_id: Some(turn_id),
        };
        let options = match kind {
            PendingInteractionKind::Command | PendingInteractionKind::FileChange => vec![
                interaction_option("accept", "Approve", false),
                interaction_option("acceptForSession", "Approve for session", false),
                interaction_option("decline", "Decline", true),
                interaction_option("cancel", "Decline and interrupt", true),
            ],
            PendingInteractionKind::Permissions => vec![
                interaction_option("grantTurn", "Grant for turn", false),
                interaction_option("grantSession", "Grant for session", false),
                interaction_option("decline", "Decline", true),
            ],
        };
        let interaction_kind = match kind {
            PendingInteractionKind::Command => InteractionKind::Command,
            PendingInteractionKind::FileChange => InteractionKind::FileChange,
            PendingInteractionKind::Permissions => InteractionKind::Permissions,
        };
        let requested_permissions = params.get("permissions").cloned();
        self.pending_interactions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                request_id,
                PendingInteraction {
                    native_id: id,
                    reference: reference.clone(),
                    kind,
                    requested_permissions,
                },
            );
        self.emit(
            &thread_id,
            NormalizedRuntimeEvent::InteractionRequested {
                request: InteractionRequest {
                    reference,
                    kind: interaction_kind,
                    title: string_field(&params, "reason"),
                    payload: bounded_payload(params),
                    options,
                },
            },
        );
    }
}

impl AgentRuntime for CodexRuntime {
    fn create_session(&self, request: CreateSessionRequest) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            if request.project.engine() != &self.instance {
                return Err(EngineError::new(
                    EngineErrorKind::NotFound,
                    "Codex runtime does not own this project",
                ));
            }
            self.supervisor.ensure_ready()?;
            let mut params = Map::new();
            if let Some(cwd) = request.cwd {
                params.insert("cwd".into(), Value::String(cwd));
            }
            copy_options(
                &request.options,
                &mut params,
                &[
                    "model",
                    "modelProvider",
                    "config",
                    "approvalPolicy",
                    "sandbox",
                    "serviceTier",
                    "ephemeral",
                ],
            );
            apply_channel_options(&request.options, &mut params)?;
            apply_session_capabilities(&request.options, &mut params)?;
            let requested_provider = params
                .get("modelProvider")
                .and_then(Value::as_str)
                .map(String::from);
            let request_params = params.clone();
            let (response, connection_epoch) = self
                .supervisor
                .request_with_epoch("thread/start", Value::Object(params))?;
            let thread_id = response
                .get("thread")
                .and_then(|thread| string_field(thread, "id"))
                .ok_or_else(|| {
                    EngineError::new(
                        EngineErrorKind::Protocol,
                        "Codex thread/start response has no thread id",
                    )
                })?;
            let runtime = self.runtime_session_from_response(
                &thread_id,
                true,
                &response,
                requested_provider.as_deref(),
                resume_params_from_request(&thread_id, &request_params),
                connection_epoch,
            )?;
            self.emit(&thread_id, NormalizedRuntimeEvent::SessionAttached);
            Ok(runtime)
        })
    }

    fn fork_session(&self, request: ForkSessionRequest) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            self.supervisor.ensure_ready()?;
            let params = fork_params(&request)?;
            let requested_provider = params
                .get("modelProvider")
                .and_then(Value::as_str)
                .map(String::from);
            let request_params = params.clone();
            let (response, connection_epoch) = self
                .supervisor
                .request_with_epoch("thread/fork", Value::Object(params))?;
            let thread_id = response
                .get("thread")
                .and_then(|thread| string_field(thread, "id"))
                .ok_or_else(|| {
                    EngineError::new(
                        EngineErrorKind::Protocol,
                        "Codex thread/fork response has no thread id",
                    )
                })?;
            let runtime = self.runtime_session_from_response(
                &thread_id,
                true,
                &response,
                requested_provider.as_deref(),
                resume_params_from_request(&thread_id, &request_params),
                connection_epoch,
            )?;
            self.emit(&thread_id, NormalizedRuntimeEvent::SessionAttached);
            Ok(runtime)
        })
    }

    fn attach_session(
        &self,
        session: SessionRef,
        options: AttachOptions,
    ) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            self.owns_session(&session)?;
            self.supervisor.ensure_ready()?;
            let mut params = Map::from_iter([(
                "threadId".into(),
                Value::String(session.native_id().to_string()),
            )]);
            copy_options(
                &options.options,
                &mut params,
                &[
                    "model",
                    "modelProvider",
                    "config",
                    "approvalPolicy",
                    "sandbox",
                    "serviceTier",
                ],
            );
            apply_channel_options(&options.options, &mut params)?;
            apply_session_capabilities(&options.options, &mut params)?;
            let requested_provider = params
                .get("modelProvider")
                .and_then(Value::as_str)
                .map(String::from);
            let resume_params = resume_params_from_request(session.native_id(), &params);
            let (response, connection_epoch) = self
                .supervisor
                .request_with_epoch("thread/resume", Value::Object(params))?;
            let runtime = self.runtime_session_from_response(
                session.native_id(),
                true,
                &response,
                requested_provider.as_deref(),
                resume_params,
                connection_epoch,
            )?;
            self.emit(session.native_id(), NormalizedRuntimeEvent::SessionAttached);
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
            self.ensure_session_loaded(&session)?;
            let mut params = Map::from_iter([
                (
                    "threadId".into(),
                    Value::String(session.native_id().to_string()),
                ),
                ("input".into(), Value::Array(map_input(request.input))),
            ]);
            copy_options(
                &request.options,
                &mut params,
                &[
                    "model",
                    "effort",
                    "summary",
                    "approvalPolicy",
                    "cwd",
                    "serviceTier",
                ],
            );
            let request_params = Value::Object(params);
            let response = match self
                .supervisor
                .request_with_epoch("turn/start", request_params.clone())
            {
                Ok(response) => response,
                Err(error) if error.kind == EngineErrorKind::NotFound => {
                    self.resume_session(&session, true)?;
                    self.supervisor
                        .request_with_epoch("turn/start", request_params)?
                }
                Err(error) => return Err(error),
            };
            self.bind_session_epoch(session.native_id(), response.1);
            let response = response.0;
            let turn_id = response
                .get("turn")
                .and_then(|turn| string_field(turn, "id"))
                .ok_or_else(|| {
                    EngineError::new(
                        EngineErrorKind::Protocol,
                        "Codex turn/start response has no turn id",
                    )
                })?;
            let runtime = self.runtime_session(session.native_id(), false)?;
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
        turn: TurnRef,
        input: Vec<InputItem>,
    ) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            self.owns_session(&turn.session)?;
            self.supervisor.request(
                "turn/steer",
                json!({
                    "threadId": turn.session.native_id(),
                    "expectedTurnId": turn.native_turn_id,
                    "input": map_input(input)
                }),
            )?;
            Ok(())
        })
    }

    fn interrupt_turn(&self, turn: TurnRef) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            self.owns_session(&turn.session)?;
            self.supervisor.request(
                "turn/interrupt",
                json!({
                    "threadId": turn.session.native_id(),
                    "turnId": turn.native_turn_id
                }),
            )?;
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
            let pending = take_pending_interaction(&self.pending_interactions, &request)?;
            if !approval_decision_allowed(pending.kind, &response.decision) {
                self.pending_interactions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(request.request_id.clone(), pending);
                return Err(EngineError::new(
                    EngineErrorKind::Protocol,
                    "Codex interaction decision is not valid for this request",
                ));
            }
            let result = match pending.kind {
                PendingInteractionKind::Command | PendingInteractionKind::FileChange => {
                    json!({ "decision": response.decision.clone() })
                }
                PendingInteractionKind::Permissions => permission_approval_response(
                    &response.decision,
                    pending
                        .requested_permissions
                        .clone()
                        .unwrap_or_else(|| json!({})),
                ),
            };
            if let Err(error) = self.supervisor.respond(pending.native_id.clone(), result) {
                self.pending_interactions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(request.request_id.clone(), pending);
                return Err(error);
            }
            let thread_id = request.session.native_id().to_string();
            self.emit(
                &thread_id,
                NormalizedRuntimeEvent::InteractionResolved {
                    reference: request,
                    decision: response.decision,
                },
            );
            Ok(())
        })
    }

    fn close_session(&self, session: SessionRef) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            self.owns_session(&session)?;
            let _ = self.supervisor.request(
                "thread/unsubscribe",
                json!({ "threadId": session.native_id() }),
            );
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

fn take_pending_interaction(
    interactions: &Mutex<HashMap<String, PendingInteraction>>,
    request: &InteractionRef,
) -> EngineResult<PendingInteraction> {
    let mut interactions = interactions
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let pending = interactions.get(&request.request_id).ok_or_else(|| {
        EngineError::new(
            EngineErrorKind::NotFound,
            "Codex interaction request is no longer pending",
        )
    })?;
    if pending.reference != *request {
        return Err(EngineError::new(
            EngineErrorKind::Conflict,
            "Codex interaction reference does not match pending request",
        ));
    }
    interactions.remove(&request.request_id).ok_or_else(|| {
        EngineError::new(
            EngineErrorKind::Conflict,
            "Codex interaction request was claimed concurrently",
        )
    })
}

fn approval_decision_allowed(kind: PendingInteractionKind, decision: &str) -> bool {
    match kind {
        PendingInteractionKind::Command | PendingInteractionKind::FileChange => {
            matches!(
                decision,
                "accept" | "acceptForSession" | "decline" | "cancel"
            )
        }
        PendingInteractionKind::Permissions => {
            matches!(decision, "grantTurn" | "grantSession" | "decline")
        }
    }
}

impl ModelCatalogProvider for CodexRuntime {
    fn list_models(&self) -> EngineFuture<'_, Vec<ModelDescriptor>> {
        Box::pin(async move { self.read_models() })
    }
}

fn copy_options(source: &BTreeMap<String, Value>, target: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(value) = source.get(*key) {
            target.insert((*key).to_string(), value.clone());
        }
    }
}

fn resume_params_from_request(thread_id: &str, request: &Map<String, Value>) -> Map<String, Value> {
    let mut params = Map::from_iter([("threadId".into(), Value::String(thread_id.to_string()))]);
    for key in [
        "model",
        "modelProvider",
        "config",
        "approvalPolicy",
        "sandbox",
        "serviceTier",
        "cwd",
        "baseInstructions",
        "developerInstructions",
        "personality",
    ] {
        if let Some(value) = request.get(key) {
            params.insert(key.to_string(), value.clone());
        }
    }
    params
}

fn should_resume_session(
    bound_epoch: Option<u64>,
    current_epoch: Option<u64>,
    force: bool,
) -> bool {
    force || current_epoch.is_none() || bound_epoch != current_epoch
}

fn response_model_provider(response: &Value) -> Option<&str> {
    response
        .get("modelProvider")
        .and_then(Value::as_str)
        .or_else(|| {
            response
                .get("thread")
                .and_then(|thread| thread.get("modelProvider"))
                .and_then(Value::as_str)
        })
        .filter(|provider| !provider.is_empty())
}

fn apply_channel_options(
    source: &BTreeMap<String, Value>,
    target: &mut Map<String, Value>,
) -> EngineResult<()> {
    let Some(channel_id) = source.get("channelId").and_then(Value::as_str) else {
        return Ok(());
    };
    let options = crate::channels::codex_runtime_channel_options(channel_id)
        .map_err(|message| EngineError::new(EngineErrorKind::Protocol, message))?;
    target.extend(options);
    Ok(())
}

fn apply_session_capabilities(
    source: &BTreeMap<String, Value>,
    target: &mut Map<String, Value>,
) -> EngineResult<()> {
    let Some(value) = source.get("sessionCapabilities") else {
        return Ok(());
    };
    let capabilities = serde_json::from_value::<Vec<SessionCapabilityId>>(value.clone())
        .map_err(|error| EngineError::new(EngineErrorKind::Protocol, error.to_string()))?;
    let bundle = SessionCapabilityBundle::new(capabilities);
    target.insert(
        "developerInstructions".into(),
        bundle
            .developer_instructions()
            .map_or(Value::Null, |instructions| {
                Value::String(instructions.to_string())
            }),
    );
    Ok(())
}

fn fork_params(request: &ForkSessionRequest) -> EngineResult<Map<String, Value>> {
    let mut params = Map::from_iter([(
        "threadId".into(),
        Value::String(request.session.native_id().to_string()),
    )]);
    if let Some(last_turn_id) = &request.last_turn_id {
        params.insert("lastTurnId".into(), Value::String(last_turn_id.clone()));
    }
    copy_options(
        &request.options,
        &mut params,
        &[
            "model",
            "modelProvider",
            "config",
            "approvalPolicy",
            "sandbox",
            "serviceTier",
            "cwd",
            "ephemeral",
        ],
    );
    apply_channel_options(&request.options, &mut params)?;
    apply_session_capabilities(&request.options, &mut params)?;
    Ok(params)
}

fn map_input(input: Vec<InputItem>) -> Vec<Value> {
    input
        .into_iter()
        .map(|item| match item {
            InputItem::Text { text } => json!({ "type": "text", "text": text }),
            InputItem::Image { media_type, data } => {
                let url = if data.starts_with("data:") {
                    data
                } else {
                    format!("data:{media_type};base64,{data}")
                };
                json!({ "type": "image", "url": url })
            }
            InputItem::File { path } => {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");
                json!({ "type": "mention", "name": name, "path": path })
            }
            InputItem::Skill { name, path } => {
                json!({ "type": "skill", "name": name, "path": path })
            }
        })
        .collect()
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(String::from)
}

fn request_id_key(id: &RequestId) -> String {
    match id {
        RequestId::Number(value) => format!("number:{value}"),
        RequestId::String(value) => format!("string:{value}"),
    }
}

fn runtime_item_key(thread_id: &str, turn_id: &str, item_id: &str) -> String {
    format!("{thread_id}\u{1f}{turn_id}\u{1f}{item_id}")
}

fn interaction_option(id: &str, label: &str, dangerous: bool) -> InteractionOption {
    InteractionOption {
        id: id.into(),
        label: label.into(),
        dangerous,
    }
}

fn item_status(status: Option<&str>) -> ItemStatus {
    match status {
        Some("inProgress" | "running") => ItemStatus::Running,
        Some("completed" | "success") => ItemStatus::Completed,
        Some("declined") => ItemStatus::Declined,
        Some("interrupted" | "cancelled") => ItemStatus::Interrupted,
        Some("failed" | "error") => ItemStatus::Failed,
        _ => ItemStatus::Pending,
    }
}

fn bounded_payload(value: Value) -> Value {
    const MAX_BYTES: usize = 16 * 1024;
    if serde_json::to_vec(&value)
        .map(|encoded| encoded.len() <= MAX_BYTES)
        .unwrap_or(false)
    {
        value
    } else {
        json!({ "truncated": true })
    }
}

fn permission_approval_response(decision: &str, requested_permissions: Value) -> Value {
    let granted_permissions = match decision {
        "grantTurn" | "grantSession" => requested_permissions,
        _ => json!({}),
    };
    let scope = if decision == "grantSession" {
        "session"
    } else {
        "turn"
    };
    json!({ "permissions": granted_permissions, "scope": scope })
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::mpsc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::time::{Duration, Instant};

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
    fn maps_inputs_without_engine_specific_frontend_shapes() {
        let input = map_input(vec![
            InputItem::Text { text: "hi".into() },
            InputItem::File {
                path: "/workspace/readme.md".into(),
            },
            InputItem::Image {
                media_type: "image/png".into(),
                data: "encoded".into(),
            },
            InputItem::Skill {
                name: "review".into(),
                path: "/home/alice/.agents/skills/review/SKILL.md".into(),
            },
        ]);
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[1]["type"], "mention");
        assert_eq!(input[2]["url"], "data:image/png;base64,encoded");
        assert_eq!(
            input[3],
            json!({
                "type": "skill",
                "name": "review",
                "path": "/home/alice/.agents/skills/review/SKILL.md",
            })
        );
    }

    #[test]
    fn maps_fork_request_to_codex_thread_fork_contract() {
        let session = SessionRef::new(default_instance().unwrap(), "source-thread").unwrap();
        let params = fork_params(&ForkSessionRequest {
            session,
            last_turn_id: Some("turn-2".into()),
            options: BTreeMap::from([
                ("model".into(), Value::String("gpt-test".into())),
                ("sessionCapabilities".into(), json!(["html_visual"])),
                ("ignored".into(), Value::Bool(true)),
            ]),
        })
        .unwrap();
        assert_eq!(params["threadId"], "source-thread");
        assert_eq!(params["lastTurnId"], "turn-2");
        assert_eq!(params["model"], "gpt-test");
        assert!(params["developerInstructions"]
            .as_str()
            .is_some_and(|instructions| instructions.contains("HTML")));
        assert!(!params.contains_key("ignored"));
    }

    #[test]
    fn derives_reconnect_params_without_start_or_fork_only_fields() {
        let request = Map::from_iter([
            ("threadId".into(), Value::String("source-thread".into())),
            ("model".into(), Value::String("gpt-test".into())),
            (
                "modelProvider".into(),
                Value::String("provider-test".into()),
            ),
            ("config".into(), json!({ "model_providers": {} })),
            (
                "developerInstructions".into(),
                Value::String("session capability".into()),
            ),
            ("cwd".into(), Value::String("/workspace".into())),
            ("ephemeral".into(), Value::Bool(true)),
            ("lastTurnId".into(), Value::String("turn-2".into())),
        ]);
        let params = resume_params_from_request("resumed-thread", &request);
        assert_eq!(params["threadId"], "resumed-thread");
        assert_eq!(params["modelProvider"], "provider-test");
        assert_eq!(params["config"], json!({ "model_providers": {} }));
        assert_eq!(params["developerInstructions"], "session capability");
        assert_eq!(params["cwd"], "/workspace");
        assert!(!params.contains_key("ephemeral"));
        assert!(!params.contains_key("lastTurnId"));
    }

    #[test]
    fn reconnects_only_when_the_server_binding_is_missing_or_changed() {
        assert!(!should_resume_session(Some(4), Some(4), false));
        assert!(should_resume_session(Some(4), Some(5), false));
        assert!(should_resume_session(Some(4), None, false));
        assert!(should_resume_session(None, Some(4), false));
        assert!(should_resume_session(Some(4), Some(4), true));
    }

    #[test]
    fn maps_only_registered_session_capabilities_to_developer_instructions() {
        let mut params = Map::new();
        apply_session_capabilities(
            &BTreeMap::from([("sessionCapabilities".into(), json!(["html_visual"]))]),
            &mut params,
        )
        .unwrap();
        assert!(params["developerInstructions"]
            .as_str()
            .is_some_and(|instructions| instructions.contains("Monet")));

        let mut empty = Map::new();
        apply_session_capabilities(
            &BTreeMap::from([("sessionCapabilities".into(), json!([]))]),
            &mut empty,
        )
        .unwrap();
        assert_eq!(empty["developerInstructions"], Value::Null);

        let error = apply_session_capabilities(
            &BTreeMap::from([("sessionCapabilities".into(), json!(["arbitrary_prompt"]))]),
            &mut Map::new(),
        );
        assert!(error.is_err());
    }

    #[test]
    fn oversized_approval_payload_is_replaced() {
        let payload = bounded_payload(json!({ "data": "x".repeat(20_000) }));
        assert_eq!(payload, json!({ "truncated": true }));
    }

    #[test]
    fn permission_approval_grants_exactly_the_requested_profile() {
        let requested = json!({ "network": { "enabled": true } });
        assert_eq!(
            permission_approval_response("grantSession", requested.clone()),
            json!({ "permissions": requested, "scope": "session" })
        );
        assert_eq!(
            permission_approval_response("decline", json!({ "network": { "enabled": true } })),
            json!({ "permissions": {}, "scope": "turn" })
        );
    }

    #[test]
    fn completed_items_do_not_replay_streamed_or_user_content() {
        let runtime = CodexRuntime::new(CodexSupervisor::new()).unwrap();
        let (event_tx, event_rx) = mpsc::channel();
        runtime
            .subscribe_events(Arc::new(move |event| {
                let _ = event_tx.send(event);
            }))
            .unwrap();

        runtime.handle_notification(
            "item/started",
            json!({
                "threadId": "thread",
                "turnId": "turn",
                "item": {
                    "id": "agent",
                    "type": "agentMessage",
                    "phase": "commentary",
                    "status": "inProgress"
                }
            }),
        );
        runtime.handle_notification(
            "item/agentMessage/delta",
            json!({ "threadId": "thread", "turnId": "turn", "itemId": "agent", "delta": "OK" }),
        );
        runtime.handle_notification(
            "item/completed",
            json!({
                "threadId": "thread",
                "turnId": "turn",
                "item": {
                    "id": "agent",
                    "type": "agentMessage",
                    "text": "OK",
                    "phase": "commentary",
                    "status": "completed"
                }
            }),
        );
        runtime.handle_notification(
            "item/started",
            json!({
                "threadId": "thread",
                "turnId": "turn",
                "item": { "id": "user", "type": "userMessage", "status": "inProgress" }
            }),
        );
        runtime.handle_notification(
            "item/completed",
            json!({
                "threadId": "thread",
                "turnId": "turn",
                "item": {
                    "id": "user",
                    "type": "userMessage",
                    "content": [{ "type": "text", "text": "question" }],
                    "status": "completed"
                }
            }),
        );

        let events: Vec<_> = event_rx.try_iter().map(|event| event.event).collect();
        assert_eq!(events.len(), 5);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, NormalizedRuntimeEvent::ItemDelta { .. }))
                .count(),
            1
        );
        assert!(events.iter().any(|event| matches!(
            event,
            NormalizedRuntimeEvent::ItemDelta {
                segment: Segment::Text {
                    phase: Some(TextPhase::Progress),
                    ..
                },
                ..
            }
        )));
        assert!(runtime
            .text_phases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty());
    }

    #[test]
    fn command_start_preserves_metadata_before_output_deltas() {
        let runtime = CodexRuntime::new(CodexSupervisor::new()).unwrap();
        let (event_tx, event_rx) = mpsc::channel();
        runtime
            .subscribe_events(Arc::new(move |event| {
                let _ = event_tx.send(event);
            }))
            .unwrap();

        runtime.handle_notification(
            "item/started",
            json!({
                "threadId": "thread",
                "turnId": "turn",
                "item": {
                    "id": "command",
                    "type": "commandExecution",
                    "command": "pnpm test",
                    "cwd": "/workspace",
                    "status": "inProgress"
                }
            }),
        );
        runtime.handle_notification(
            "item/commandExecution/outputDelta",
            json!({
                "threadId": "thread",
                "turnId": "turn",
                "itemId": "command",
                "delta": "passed"
            }),
        );

        let events: Vec<_> = event_rx.try_iter().map(|event| event.event).collect();
        assert!(matches!(
            &events[1],
            NormalizedRuntimeEvent::ItemDelta {
                segment: Segment::CommandExecution {
                    command,
                    cwd: Some(cwd),
                    output: None,
                    ..
                },
                ..
            } if command == "pnpm test" && cwd == "/workspace"
        ));
        assert!(matches!(
            &events[2],
            NormalizedRuntimeEvent::ItemDelta {
                segment: Segment::CommandExecution {
                    output: Some(output),
                    ..
                },
                ..
            } if output == "passed"
        ));
    }

    #[test]
    fn concurrent_approval_responses_can_claim_a_request_only_once() {
        let session = SessionRef::new(default_instance().unwrap(), "thread").unwrap();
        let reference = InteractionRef {
            session,
            runtime_id: RuntimeId("runtime".into()),
            request_id: "request".into(),
            turn_id: Some("turn".into()),
        };
        let interactions = Mutex::new(HashMap::from([(
            reference.request_id.clone(),
            PendingInteraction {
                native_id: RequestId::Number(7),
                reference: reference.clone(),
                kind: PendingInteractionKind::Command,
                requested_permissions: None,
            },
        )]));

        let mut mismatched = reference.clone();
        mismatched.runtime_id = RuntimeId("other-runtime".into());
        assert_eq!(
            take_pending_interaction(&interactions, &mismatched)
                .err()
                .unwrap()
                .kind,
            EngineErrorKind::Conflict
        );
        assert!(interactions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(&reference.request_id));

        assert!(take_pending_interaction(&interactions, &reference).is_ok());
        assert_eq!(
            take_pending_interaction(&interactions, &reference)
                .err()
                .unwrap()
                .kind,
            EngineErrorKind::NotFound
        );
    }

    #[test]
    #[ignore = "requires an installed, signed-in Codex CLI and explicit MONET_CODEX_RUNTIME_SMOKE=1"]
    fn installed_runtime_completes_streams_interrupts_and_reconnects() {
        assert_eq!(
            std::env::var("MONET_CODEX_RUNTIME_SMOKE").as_deref(),
            Ok("1"),
            "set MONET_CODEX_RUNTIME_SMOKE=1 to run the Codex runtime smoke test"
        );
        let supervisor = CodexSupervisor::new();
        let runtime = CodexRuntime::new(Arc::clone(&supervisor)).unwrap();
        let (event_tx, event_rx) = mpsc::channel();
        runtime
            .subscribe_events(Arc::new(move |event| {
                let _ = event_tx.send(event);
            }))
            .unwrap();

        let models = resolve_ready(runtime.list_models()).unwrap();
        assert!(!models.is_empty(), "model/list should return a catalog");

        let mut create_options = BTreeMap::new();
        create_options.insert("ephemeral".into(), Value::Bool(true));
        create_options.insert("approvalPolicy".into(), Value::String("never".into()));
        let session = resolve_ready(runtime.create_session(CreateSessionRequest {
            project: ProjectRef::new(default_instance().unwrap(), "runtime-smoke").unwrap(),
            cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            options: create_options,
        }))
        .unwrap();

        let completed_turn = resolve_ready(runtime.start_turn(
            session.session.clone(),
            TurnRequest {
                input: vec![InputItem::Text {
                    text: "Reply with exactly OK. Do not use tools.".into(),
                }],
                options: BTreeMap::new(),
            },
        ))
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut saw_started = false;
        let mut saw_text = false;
        let mut previous_sequence = 0;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("Codex turn should complete before the smoke-test deadline");
            let event = event_rx
                .recv_timeout(remaining)
                .expect("Codex should emit runtime events");
            assert!(
                event.sequence > previous_sequence,
                "runtime event sequence should be monotonic"
            );
            previous_sequence = event.sequence;
            match event.event {
                NormalizedRuntimeEvent::TurnStarted { ref turn_id }
                    if turn_id == &completed_turn.reference.native_turn_id =>
                {
                    saw_started = true;
                }
                NormalizedRuntimeEvent::ItemDelta {
                    ref turn_id,
                    segment: Segment::Text { ref text, .. },
                    ..
                } if turn_id == &completed_turn.reference.native_turn_id && !text.is_empty() => {
                    saw_text = true;
                }
                NormalizedRuntimeEvent::InteractionRequested { request } => {
                    resolve_ready(runtime.respond(
                        request.reference,
                        InteractionResponse {
                            decision: "decline".into(),
                            payload: None,
                        },
                    ))
                    .unwrap();
                }
                NormalizedRuntimeEvent::TurnCompleted {
                    ref turn_id,
                    status,
                    ..
                } if turn_id == &completed_turn.reference.native_turn_id => {
                    assert_eq!(status, TurnStatus::Completed);
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_started, "turn/start should produce TurnStarted");
        assert!(saw_text, "the completed turn should stream text");

        let steered_turn = resolve_ready(runtime.start_turn(
            session.session.clone(),
            TurnRequest {
                input: vec![InputItem::Text {
                    text: "Run the shell command `sleep 5`. After it ends, reply DONE.".into(),
                }],
                options: BTreeMap::new(),
            },
        ))
        .unwrap();
        std::thread::sleep(Duration::from_millis(250));
        resolve_ready(runtime.send_input_while_running(
            steered_turn.reference.clone(),
            vec![InputItem::Text {
                text: "After the command ends, reply with exactly STEERED.".into(),
            }],
        ))
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("steered turn should settle before the smoke-test deadline");
            let event = event_rx
                .recv_timeout(remaining)
                .expect("Codex should emit a steered completion");
            if let NormalizedRuntimeEvent::TurnCompleted {
                turn_id, status, ..
            } = event.event
            {
                if turn_id == steered_turn.reference.native_turn_id {
                    assert_eq!(status, TurnStatus::Completed);
                    break;
                }
            }
        }

        let interrupted_turn = resolve_ready(runtime.start_turn(
            session.session.clone(),
            TurnRequest {
                input: vec![InputItem::Text {
                    text: "Run the shell command `sleep 30`. After it ends, reply DONE.".into(),
                }],
                options: BTreeMap::new(),
            },
        ))
        .unwrap();
        std::thread::sleep(Duration::from_millis(250));
        resolve_ready(runtime.interrupt_turn(interrupted_turn.reference.clone())).unwrap();

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("interrupted turn should settle before the smoke-test deadline");
            let event = event_rx
                .recv_timeout(remaining)
                .expect("Codex should emit an interrupted completion");
            if let NormalizedRuntimeEvent::TurnCompleted {
                turn_id, status, ..
            } = event.event
            {
                if turn_id == interrupted_turn.reference.native_turn_id {
                    assert_eq!(status, TurnStatus::Interrupted);
                    break;
                }
            }
        }
        resolve_ready(runtime.close_session(session.session)).unwrap();

        let listing = supervisor
            .request(
                "thread/list",
                json!({ "limit": 1, "sortKey": "updated_at", "sortDirection": "desc" }),
            )
            .unwrap();
        if let Some(existing_id) = listing
            .get("data")
            .and_then(Value::as_array)
            .and_then(|threads| threads.first())
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
        {
            let existing = SessionRef::new(default_instance().unwrap(), existing_id).unwrap();
            let first =
                resolve_ready(runtime.attach_session(existing.clone(), AttachOptions::default()))
                    .unwrap();
            supervisor.disconnect();
            std::thread::sleep(Duration::from_millis(100));
            let reconnected =
                resolve_ready(runtime.attach_session(existing.clone(), AttachOptions::default()))
                    .unwrap();
            assert!(reconnected.generation > first.generation);
            resolve_ready(runtime.close_session(existing)).unwrap();
        }
    }
}
