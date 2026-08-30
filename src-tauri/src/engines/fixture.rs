use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::Utc;
use serde_json::json;

use super::core::*;

pub struct FixtureEngine {
    descriptor: EngineDescriptor,
    project: CoreProject,
    session: CoreSessionSummary,
    second_project: CoreProject,
    second_session: CoreSessionSummary,
    timeline: Vec<ConversationRecord>,
    event_sinks: Mutex<Vec<RuntimeEventSink>>,
    sequence: AtomicU64,
}

impl FixtureEngine {
    pub fn new() -> Self {
        let instance = EngineInstanceId::new("fixture", "default").unwrap();
        let project_ref = ProjectRef::new(instance.clone(), "fixture-project").unwrap();
        let session_ref = SessionRef::new(instance.clone(), "fixture-session").unwrap();
        let second_project_ref = ProjectRef::new(instance.clone(), "fixture-project-two").unwrap();
        let second_session_ref = SessionRef::new(instance.clone(), "fixture-session-two").unwrap();
        let descriptor = EngineDescriptor {
            instance: instance.clone(),
            display_name: "Fixture Engine".into(),
            enabled: true,
            capabilities: EngineCapabilities {
                history: HistoryCapabilities {
                    pagination: HistoryPagination::Native,
                    change_delivery: ChangeDelivery::Push,
                    search: true,
                    assets: true,
                },
                runtime: Some(RuntimeCapabilities {
                    create: true,
                    resume: true,
                    fork: false,
                    fork_with_cwd: false,
                    send_while_running: true,
                    interrupt: true,
                    streaming: StreamingCapabilities {
                        text: StreamGranularity::Delta,
                        reasoning: StreamGranularity::Item,
                        tool_progress: StreamGranularity::Item,
                    },
                    model_catalog: ModelCatalogKind::Configured,
                    interactions: vec![InteractionKind::Command],
                }),
                facets: FacetCapabilities::default(),
            },
            ui: EngineUiIntegration {
                identity: UiIdentityMode::Structured,
                session_surface: SessionSurface::Standard,
                install_guide_url: None,
            },
        };
        let project = CoreProject {
            reference: project_ref.clone(),
            display_name: "Fixture Project".into(),
            display_path: Some("/workspace/fixture".into()),
            session_count: 1,
            last_active: Some("2026-01-01T00:00:00Z".into()),
        };
        let session = CoreSessionSummary {
            reference: session_ref.clone(),
            project: project_ref,
            title: Some("Fixture session".into()),
            preview: Some("Synthetic conversation".into()),
            cwd: Some("/workspace/fixture".into()),
            model: Some("fixture-model".into()),
            created_at: Some("2026-01-01T00:00:00Z".into()),
            updated_at: Some("2026-01-01T00:00:01Z".into()),
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: Some(15),
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
            }),
            source_meta: SourceMetadata::default(),
        };
        let second_project = CoreProject {
            reference: second_project_ref.clone(),
            display_name: "Fixture Project Two".into(),
            display_path: Some("/workspace/fixture-two".into()),
            session_count: 1,
            last_active: Some("2026-01-02T00:00:00Z".into()),
        };
        let second_session = CoreSessionSummary {
            reference: second_session_ref,
            project: second_project_ref,
            title: Some("Second fixture session".into()),
            preview: Some("Second synthetic conversation".into()),
            cwd: Some("/workspace/fixture-two".into()),
            model: Some("fixture-model".into()),
            created_at: Some("2026-01-02T00:00:00Z".into()),
            updated_at: Some("2026-01-02T00:00:01Z".into()),
            usage: None,
            source_meta: SourceMetadata::default(),
        };
        let timeline = vec![ConversationRecord {
            id: "fixture-record".into(),
            session: session_ref,
            turn_id: Some("fixture-turn".into()),
            parent_id: None,
            role: ConversationRole::Assistant,
            timestamp: Some("2026-01-01T00:00:01Z".into()),
            segments: vec![
                Segment::Text {
                    text: "Fixture response".into(),
                    phase: None,
                },
                Segment::Reasoning {
                    text: "Fixture reasoning".into(),
                    visibility: ReasoningVisibility::Summary,
                },
                Segment::ToolCall {
                    id: "fixture-call".into(),
                    name: "fixture_tool".into(),
                    input: json!({ "value": 1 }),
                    title: None,
                    presentation: None,
                },
                Segment::Unknown {
                    type_name: "future_item".into(),
                    summary: Some("Unknown items degrade safely".into()),
                },
            ],
            usage: None,
            source_meta: SourceMetadata::default(),
        }];

        Self {
            descriptor,
            project,
            session,
            second_project,
            second_session,
            timeline,
            event_sinks: Mutex::new(Vec::new()),
            sequence: AtomicU64::new(0),
        }
    }

    fn runtime_session(&self, session: SessionRef) -> RuntimeSession {
        RuntimeSession {
            session,
            runtime_id: RuntimeId("fixture-runtime".into()),
            generation: 1,
            source_meta: BTreeMap::new(),
        }
    }

    fn emit(&self, session: &SessionRef, event: NormalizedRuntimeEvent) {
        let envelope = RuntimeEventEnvelope {
            session: session.clone(),
            runtime_id: RuntimeId("fixture-runtime".into()),
            generation: 1,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed) + 1,
            timestamp: Utc::now().to_rfc3339(),
            event,
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

    fn owns_session(&self, session: &SessionRef) -> EngineResult<()> {
        if session.engine() == &self.descriptor.instance {
            Ok(())
        } else {
            Err(EngineError::new(
                EngineErrorKind::NotFound,
                "fixture engine does not own this session",
            ))
        }
    }
}

impl Default for FixtureEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineAdapter for FixtureEngine {
    fn descriptor(&self) -> EngineDescriptor {
        self.descriptor.clone()
    }

    fn health(&self) -> EngineFuture<'_, EngineHealth> {
        Box::pin(async move {
            Ok(EngineHealth {
                instance: self.descriptor.instance.clone(),
                status: EngineHealthStatus::Available,
                installed: true,
                authenticated: Some(true),
                version: Some("fixture".into()),
                version_supported: Some(true),
                executable_path: None,
                source: CapabilityHealth::available(),
                runtime: CapabilityHealth::available(),
                diagnostics: Vec::new(),
            })
        })
    }

    fn session_source(&self) -> &dyn SessionSource {
        self
    }

    fn runtime(&self) -> Option<&dyn AgentRuntime> {
        Some(self)
    }
}

impl SessionSource for FixtureEngine {
    fn list_projects(&self, query: ProjectQuery) -> EngineFuture<'_, ProjectPage> {
        Box::pin(async move {
            let (projects, next_cursor) = paginate(
                vec![self.project.clone(), self.second_project.clone()],
                query.cursor,
                query.limit,
            );
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
            let sessions = if project == self.project.reference {
                vec![self.session.clone()]
            } else if project == self.second_project.reference {
                vec![self.second_session.clone()]
            } else {
                return Err(EngineError::new(
                    EngineErrorKind::NotFound,
                    "fixture project not found",
                ));
            };
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
            self.owns_session(&session)?;
            let timeline: Vec<_> = self
                .timeline
                .iter()
                .cloned()
                .map(|mut record| {
                    record.session = session.clone();
                    record
                })
                .collect();
            let start = page
                .cursor
                .as_deref()
                .and_then(|cursor| cursor.parse::<usize>().ok())
                .unwrap_or(0);
            let end = (start + page.limit).min(timeline.len());
            let next_cursor = (end < timeline.len()).then(|| end.to_string());
            Ok(ConversationPage {
                records: timeline[start..end].to_vec(),
                next_cursor,
            })
        })
    }

    fn subscribe_changes(&self, _sink: SourceChangeSink) -> EngineResult<SubscriptionHandle> {
        Ok(Box::new(()))
    }

    fn build_search_document(&self, session: SessionRef) -> EngineFuture<'_, SearchDocument> {
        Box::pin(async move {
            self.owns_session(&session)?;
            if session.native_id() == "fixture-failure-session" {
                return Err(EngineError::new(
                    EngineErrorKind::Protocol,
                    "fixture simulated source failure",
                ));
            }
            Ok(SearchDocument {
                title: if session == self.second_session.reference {
                    self.second_session.title.clone()
                } else {
                    self.session.title.clone()
                },
                session,
                text: "Fixture response Fixture reasoning fixture_tool".into(),
            })
        })
    }

    fn resolve_asset(&self, asset: AssetRef) -> EngineFuture<'_, ResolvedAsset> {
        Box::pin(async move {
            self.owns_session(&asset.session)?;
            Ok(ResolvedAsset {
                media_type: "text/plain".into(),
                bytes: b"fixture asset".to_vec(),
            })
        })
    }

    fn session_actions(&self, session: SessionRef) -> EngineFuture<'_, SessionActions> {
        Box::pin(async move {
            self.owns_session(&session)?;
            Ok(SessionActions {
                resume: ActionAvailability::available(),
                fork: ActionAvailability::unavailable("fixture.noFork"),
                send: ActionAvailability::available(),
                send_while_running: ActionAvailability::available(),
                interrupt: ActionAvailability::available(),
                open_cwd: ActionAvailability::available(),
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
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
        .min(values.len());
    let end = start
        .saturating_add(limit.unwrap_or(values.len()).max(1))
        .min(values.len());
    let next_cursor = (end < values.len()).then(|| end.to_string());
    (
        values.into_iter().skip(start).take(end - start).collect(),
        next_cursor,
    )
}

impl AgentRuntime for FixtureEngine {
    fn create_session(&self, _request: CreateSessionRequest) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            let session =
                SessionRef::new(self.descriptor.instance.clone(), "fixture-created-session")?;
            self.emit(&session, NormalizedRuntimeEvent::SessionAttached);
            Ok(self.runtime_session(session))
        })
    }

    fn attach_session(
        &self,
        session: SessionRef,
        _options: AttachOptions,
    ) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            self.owns_session(&session)?;
            self.emit(&session, NormalizedRuntimeEvent::SessionAttached);
            Ok(self.runtime_session(session))
        })
    }

    fn start_turn(
        &self,
        session: SessionRef,
        _request: TurnRequest,
    ) -> EngineFuture<'_, TurnHandle> {
        Box::pin(async move {
            self.owns_session(&session)?;
            let turn_id = "fixture-turn-live".to_string();
            let item_id = "fixture-item-live".to_string();
            self.emit(
                &session,
                NormalizedRuntimeEvent::TurnStarted {
                    turn_id: turn_id.clone(),
                },
            );
            self.emit(
                &session,
                NormalizedRuntimeEvent::ItemStarted {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    status: ItemStatus::Running,
                },
            );
            self.emit(
                &session,
                NormalizedRuntimeEvent::ItemDelta {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    segment: Segment::Text {
                        text: "fixture delta".into(),
                        phase: None,
                    },
                },
            );
            self.emit(
                &session,
                NormalizedRuntimeEvent::ItemCompleted {
                    turn_id: turn_id.clone(),
                    item_id,
                    status: ItemStatus::Completed,
                },
            );
            self.emit(
                &session,
                NormalizedRuntimeEvent::InteractionRequested {
                    request: InteractionRequest {
                        reference: InteractionRef {
                            session: session.clone(),
                            runtime_id: RuntimeId("fixture-runtime".into()),
                            request_id: "fixture-approval".into(),
                            turn_id: Some(turn_id.clone()),
                        },
                        kind: InteractionKind::Command,
                        title: Some("Run fixture command".into()),
                        payload: json!({ "command": "true" }),
                        options: vec![InteractionOption {
                            id: "allow".into(),
                            label: "Allow".into(),
                            dangerous: false,
                        }],
                    },
                },
            );
            Ok(TurnHandle {
                reference: TurnRef {
                    session,
                    runtime_id: RuntimeId("fixture-runtime".into()),
                    native_turn_id: turn_id,
                },
            })
        })
    }

    fn send_input_while_running(
        &self,
        turn: TurnRef,
        _input: Vec<InputItem>,
    ) -> EngineFuture<'_, ()> {
        Box::pin(async move { self.owns_session(&turn.session) })
    }

    fn interrupt_turn(&self, turn: TurnRef) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            self.owns_session(&turn.session)?;
            self.emit(
                &turn.session,
                NormalizedRuntimeEvent::TurnCompleted {
                    turn_id: turn.native_turn_id.clone(),
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
            let session = request.session.clone();
            self.emit(
                &session,
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
            self.emit(&session, NormalizedRuntimeEvent::SessionDetached);
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

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::{Arc, Mutex};
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
            Poll::Pending => panic!("fixture futures must resolve immediately"),
        }
    }

    #[test]
    fn registry_routes_source_and_runtime_without_engine_branches() {
        let engine = Arc::new(FixtureEngine::new());
        let session = engine.session.reference.clone();
        let mut registry = EngineRegistry::new();
        registry.register(engine).unwrap();

        let first_page = resolve_ready(registry.source_for(&session).unwrap().list_projects(
            ProjectQuery {
                cursor: None,
                limit: Some(1),
            },
        ))
        .unwrap();
        assert_eq!(first_page.projects.len(), 1);
        let second_page = resolve_ready(registry.source_for(&session).unwrap().list_projects(
            ProjectQuery {
                cursor: first_page.next_cursor,
                limit: Some(1),
            },
        ))
        .unwrap();
        assert_eq!(second_page.projects.len(), 1);
        assert!(registry.runtime_for(&session).is_ok());
    }

    #[test]
    fn registry_rejects_duplicate_instances() {
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FixtureEngine::new())).unwrap();
        let error = registry
            .register(Arc::new(FixtureEngine::new()))
            .unwrap_err();

        assert_eq!(error.kind, EngineErrorKind::Conflict);
    }

    #[test]
    fn disabled_registration_keeps_catalog_without_constructing_runtime_access() {
        let engine = FixtureEngine::new();
        let instance = engine.descriptor.instance.clone();
        let mut registry = EngineRegistry::new();
        registry.register_disabled(engine.descriptor()).unwrap();

        let descriptor = registry.descriptor(&instance).unwrap();
        assert!(!descriptor.enabled);
        assert!(!registry.is_enabled(&instance));
        assert_eq!(
            registry.adapter(&instance).err().unwrap().kind,
            EngineErrorKind::Unavailable
        );
    }

    #[test]
    fn failed_initialization_keeps_an_enabled_but_unavailable_catalog_entry() {
        let engine = FixtureEngine::new();
        let instance = engine.descriptor.instance.clone();
        let mut registry = EngineRegistry::new();
        registry.register_unavailable(engine.descriptor()).unwrap();

        assert!(registry.is_enabled(&instance));
        assert!(!registry.is_available(&instance));
        assert_eq!(
            registry.adapter(&instance).err().unwrap().message,
            "engine instance failed to initialize"
        );
    }

    #[test]
    fn source_contract_covers_history_search_assets_and_actions() {
        let engine = FixtureEngine::new();
        let project = engine.project.reference.clone();
        let session = engine.session.reference.clone();

        let sessions =
            resolve_ready(engine.list_sessions(project, SessionQuery::default())).unwrap();
        let page =
            resolve_ready(engine.load_timeline(session.clone(), TimelinePage::default())).unwrap();
        let search = resolve_ready(engine.build_search_document(session.clone())).unwrap();
        let actions = resolve_ready(engine.session_actions(session.clone())).unwrap();
        let asset = resolve_ready(engine.resolve_asset(AssetRef {
            session,
            native_id: "fixture-asset".into(),
        }))
        .unwrap();

        assert_eq!(sessions.sessions.len(), 1);
        assert_eq!(page.records.len(), 1);
        assert!(search.text.contains("fixture_tool"));
        assert!(actions.send.available);
        assert_eq!(asset.bytes, b"fixture asset");
    }

    #[test]
    fn source_contract_keeps_failures_structured() {
        let engine = FixtureEngine::new();
        let failure = SessionRef::new(
            engine.descriptor.instance.clone(),
            "fixture-failure-session",
        )
        .unwrap();
        let error = resolve_ready(engine.build_search_document(failure)).unwrap_err();
        assert_eq!(error.kind, EngineErrorKind::Protocol);
    }

    #[test]
    fn runtime_contract_emits_ordered_normalized_events() {
        let engine = FixtureEngine::new();
        let session = engine.session.reference.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let _subscription = engine
            .subscribe_events(Arc::new(move |event| {
                captured
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(event);
            }))
            .unwrap();

        let turn = resolve_ready(engine.start_turn(
            session,
            TurnRequest {
                input: vec![InputItem::Text {
                    text: "test".into(),
                }],
                options: BTreeMap::new(),
            },
        ))
        .unwrap();
        resolve_ready(engine.interrupt_turn(turn.reference)).unwrap();

        let events = events.lock().unwrap_or_else(|error| error.into_inner());
        assert!(events
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence));
        assert!(events.iter().any(|event| matches!(
            event.event,
            NormalizedRuntimeEvent::InteractionRequested { .. }
        )));
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(NormalizedRuntimeEvent::TurnCompleted {
                status: TurnStatus::Interrupted,
                ..
            })
        ));
    }
}
