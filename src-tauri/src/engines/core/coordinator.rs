use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::{
    AttachOptions, CreateSessionRequest, EngineError, EngineErrorKind, EngineFuture,
    EngineRegistry, ForkSessionRequest, InteractionRef, InteractionRequest, InteractionResponse,
    NormalizedRuntimeEvent, RuntimeEventEnvelope, RuntimeEventSink, RuntimeId, RuntimeSession,
    SessionRef, SubscriptionHandle, TurnHandle, TurnRef, TurnRequest,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimePhase {
    Detached,
    Connecting,
    Idle,
    Running,
    AwaitingInteraction,
    Failed,
    Exited,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub session: SessionRef,
    pub runtime_id: RuntimeId,
    pub generation: u64,
    pub last_sequence: u64,
    pub sequence_consistent: bool,
    pub phase: RuntimePhase,
    pub active_turn_id: Option<String>,
    pub pending_interactions: Vec<InteractionRequest>,
    pub last_error: Option<String>,
}

impl RuntimeSnapshot {
    fn attached(runtime: &RuntimeSession) -> Self {
        Self {
            session: runtime.session.clone(),
            runtime_id: runtime.runtime_id.clone(),
            generation: runtime.generation,
            last_sequence: 0,
            sequence_consistent: true,
            phase: RuntimePhase::Idle,
            active_turn_id: None,
            pending_interactions: Vec::new(),
            last_error: None,
        }
    }
}

pub type RuntimeSnapshotSink = Arc<dyn Fn(RuntimeSnapshot) + Send + Sync>;
pub type CoordinatedEventSink = Arc<dyn Fn(RuntimeEventEnvelope) + Send + Sync>;

pub struct RuntimeCoordinator {
    registry: Arc<EngineRegistry>,
    snapshots: Mutex<BTreeMap<SessionRef, RuntimeSnapshot>>,
    sinks: Mutex<Vec<RuntimeSnapshotSink>>,
    event_sinks: Mutex<Vec<CoordinatedEventSink>>,
    subscriptions: Mutex<Vec<SubscriptionHandle>>,
}

impl RuntimeCoordinator {
    pub fn new(registry: Arc<EngineRegistry>) -> Arc<Self> {
        let coordinator = Arc::new(Self {
            registry,
            snapshots: Mutex::new(BTreeMap::new()),
            sinks: Mutex::new(Vec::new()),
            event_sinks: Mutex::new(Vec::new()),
            subscriptions: Mutex::new(Vec::new()),
        });
        coordinator.connect_runtime_events();
        coordinator
    }

    fn connect_runtime_events(self: &Arc<Self>) {
        for descriptor in self.registry.descriptors() {
            let Ok(adapter) = self.registry.adapter_arc(&descriptor.instance) else {
                continue;
            };
            let Some(runtime) = adapter.runtime() else {
                continue;
            };
            let weak = Arc::downgrade(self);
            let sink: RuntimeEventSink = Arc::new(move |event| {
                if let Some(coordinator) = weak.upgrade() {
                    coordinator.ingest(event);
                }
            });
            if let Ok(subscription) = runtime.subscribe_events(sink) {
                self.subscriptions
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(subscription);
            }
        }
    }

    pub fn subscribe(&self, sink: RuntimeSnapshotSink) {
        self.sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sink);
    }

    pub fn subscribe_events(&self, sink: CoordinatedEventSink) {
        self.event_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sink);
    }

    pub fn snapshot(&self, session: &SessionRef) -> Option<RuntimeSnapshot> {
        self.snapshots
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(session)
            .cloned()
    }

    pub fn snapshots(&self) -> Vec<RuntimeSnapshot> {
        self.snapshots
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// 返回权威快照并确认调用方已完成断号恢复。
    pub fn reconcile_snapshots(&self) -> Vec<RuntimeSnapshot> {
        let mut snapshots = self
            .snapshots
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for snapshot in snapshots.values_mut() {
            snapshot.sequence_consistent = true;
        }
        snapshots.values().cloned().collect()
    }

    pub fn ingest(&self, envelope: RuntimeEventEnvelope) {
        let event_name = runtime_event_name(&envelope.event);
        let accepted_envelope = envelope.clone();
        let snapshot_changed = !matches!(
            &envelope.event,
            NormalizedRuntimeEvent::ItemStarted { .. }
                | NormalizedRuntimeEvent::ItemDelta { .. }
                | NormalizedRuntimeEvent::ItemCompleted { .. }
        );
        let snapshot = {
            let mut snapshots = self
                .snapshots
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let current = snapshots
                .entry(envelope.session.clone())
                .or_insert_with(|| RuntimeSnapshot {
                    session: envelope.session.clone(),
                    runtime_id: envelope.runtime_id.clone(),
                    generation: envelope.generation,
                    last_sequence: 0,
                    sequence_consistent: true,
                    phase: RuntimePhase::Detached,
                    active_turn_id: None,
                    pending_interactions: Vec::new(),
                    last_error: None,
                });

            if envelope.generation < current.generation
                || (envelope.generation == current.generation
                    && envelope.sequence <= current.last_sequence)
            {
                return;
            }
            if envelope.generation == current.generation && current.phase == RuntimePhase::Exited {
                return;
            }
            if envelope.generation > current.generation {
                current.runtime_id = envelope.runtime_id.clone();
                current.generation = envelope.generation;
                current.last_sequence = 0;
                current.sequence_consistent = true;
                current.phase = RuntimePhase::Connecting;
                current.active_turn_id = None;
                current.pending_interactions.clear();
                current.last_error = None;
            }
            if current.last_sequence > 0 && envelope.sequence != current.last_sequence + 1 {
                current.sequence_consistent = false;
            }
            current.last_sequence = envelope.sequence;
            apply_event(current, envelope.event);
            current.clone()
        };
        if snapshot_changed {
            self.publish(snapshot);
        }
        let sinks = self
            .event_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        for sink in &sinks {
            sink(accepted_envelope.clone());
        }
        if !matches!(
            accepted_envelope.event,
            NormalizedRuntimeEvent::ItemDelta { .. }
        ) {
            log::debug!(
                "engine_runtime engineId={} instanceId={} sessionRef={} generation={} event={} elapsedMs=0 outcome=accepted",
                accepted_envelope.session.engine().engine_id(),
                accepted_envelope.session.engine().instance_id(),
                short_session_ref(&accepted_envelope.session),
                accepted_envelope.generation,
                event_name,
            );
        }
    }

    pub fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            let started = Instant::now();
            let instance = request.project.engine().clone();
            let result = async {
                let adapter = self.registry.adapter(&instance)?;
                let runtime = adapter.runtime().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorKind::Unsupported,
                        "engine instance does not provide a runtime",
                    )
                })?;
                let attached = runtime.create_session(request).await?;
                self.ensure_attached(&attached);
                Ok(attached)
            }
            .await;
            let (session_ref, generation) = result
                .as_ref()
                .map(|runtime| (short_session_ref(&runtime.session), runtime.generation))
                .unwrap_or_else(|_| ("pending".to_string(), 0));
            log_runtime_action(
                &instance,
                &session_ref,
                generation,
                "createSession",
                started,
                &result,
            );
            result
        })
    }

    pub fn fork_session(&self, request: ForkSessionRequest) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            let started = Instant::now();
            let observed = request.session.clone();
            let instance = observed.engine().clone();
            let result = async {
                let runtime = self.registry.runtime_for(&observed)?;
                let attached = runtime.fork_session(request).await?;
                self.ensure_attached(&attached);
                Ok(attached)
            }
            .await;
            let (session_ref, generation) = result
                .as_ref()
                .map(|runtime| (short_session_ref(&runtime.session), runtime.generation))
                .unwrap_or_else(|_| (short_session_ref(&observed), self.generation(&observed)));
            log_runtime_action(
                &instance,
                &session_ref,
                generation,
                "forkSession",
                started,
                &result,
            );
            result
        })
    }

    pub fn attach_session(
        &self,
        session: SessionRef,
        options: AttachOptions,
    ) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            let started = Instant::now();
            let observed = session.clone();
            let result = async {
                let attached = self
                    .registry
                    .runtime_for(&session)?
                    .attach_session(session, options)
                    .await?;
                self.ensure_attached(&attached);
                Ok(attached)
            }
            .await;
            let generation = result
                .as_ref()
                .map(|runtime| runtime.generation)
                .unwrap_or_else(|_| self.generation(&observed));
            log_runtime_action(
                observed.engine(),
                &short_session_ref(&observed),
                generation,
                "attachSession",
                started,
                &result,
            );
            result
        })
    }

    pub fn start_turn(
        &self,
        session: SessionRef,
        request: TurnRequest,
    ) -> EngineFuture<'_, TurnHandle> {
        Box::pin(async move {
            let started = Instant::now();
            let observed = session.clone();
            let generation = self.generation(&observed);
            let result = async {
                self.registry
                    .runtime_for(&session)?
                    .start_turn(session, request)
                    .await
            }
            .await;
            log_runtime_action(
                observed.engine(),
                &short_session_ref(&observed),
                generation,
                "startTurn",
                started,
                &result,
            );
            result
        })
    }

    pub fn steer_turn(&self, turn: TurnRef, input: Vec<super::InputItem>) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            let started = Instant::now();
            let observed = turn.session.clone();
            let generation = self.generation(&observed);
            let result = async {
                self.registry
                    .runtime_for(&turn.session)?
                    .steer_turn(turn, input)
                    .await
            }
            .await;
            log_runtime_action(
                observed.engine(),
                &short_session_ref(&observed),
                generation,
                "steerTurn",
                started,
                &result,
            );
            result
        })
    }

    pub fn interrupt_turn(&self, turn: TurnRef) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            let started = Instant::now();
            let observed = turn.session.clone();
            let generation = self.generation(&observed);
            let result = async {
                self.registry
                    .runtime_for(&turn.session)?
                    .interrupt_turn(turn)
                    .await
            }
            .await;
            log_runtime_action(
                observed.engine(),
                &short_session_ref(&observed),
                generation,
                "interruptTurn",
                started,
                &result,
            );
            result
        })
    }

    pub fn respond(
        &self,
        request: InteractionRef,
        response: InteractionResponse,
    ) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            let started = Instant::now();
            let observed = request.session.clone();
            let generation = self.generation(&observed);
            let result = async {
                self.registry
                    .runtime_for(&request.session)?
                    .respond(request, response)
                    .await
            }
            .await;
            log_runtime_action(
                observed.engine(),
                &short_session_ref(&observed),
                generation,
                "respondInteraction",
                started,
                &result,
            );
            result
        })
    }

    pub fn close_session(&self, session: SessionRef) -> EngineFuture<'_, ()> {
        Box::pin(async move {
            let started = Instant::now();
            let observed = session.clone();
            let generation = self.generation(&observed);
            let result = async {
                self.registry
                    .runtime_for(&session)?
                    .close_session(session.clone())
                    .await?;
                self.snapshots
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .remove(&session);
                Ok(())
            }
            .await;
            log_runtime_action(
                observed.engine(),
                &short_session_ref(&observed),
                generation,
                "closeSession",
                started,
                &result,
            );
            result
        })
    }

    fn generation(&self, session: &SessionRef) -> u64 {
        self.snapshot(session)
            .map(|snapshot| snapshot.generation)
            .unwrap_or(0)
    }

    fn ensure_attached(&self, runtime: &RuntimeSession) {
        let snapshot = {
            let mut snapshots = self
                .snapshots
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let current = snapshots
                .entry(runtime.session.clone())
                .or_insert_with(|| RuntimeSnapshot::attached(runtime));
            if current.generation < runtime.generation
                || current.runtime_id != runtime.runtime_id
                || matches!(current.phase, RuntimePhase::Detached | RuntimePhase::Exited)
            {
                *current = RuntimeSnapshot::attached(runtime);
            }
            current.clone()
        };
        self.publish(snapshot);
    }

    fn publish(&self, snapshot: RuntimeSnapshot) {
        let sinks = self
            .sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        for sink in &sinks {
            sink(snapshot.clone());
        }
    }
}

fn short_session_ref(session: &SessionRef) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in session.storage_key().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:012x}", hash = hash & 0xffff_ffff_ffff)
}

fn log_runtime_action<T>(
    instance: &super::EngineInstanceId,
    session_ref: &str,
    generation: u64,
    method: &str,
    started: Instant,
    result: &super::EngineResult<T>,
) {
    let elapsed = started.elapsed().as_millis();
    match result {
        Ok(_) => log::info!(
            "engine_runtime engineId={} instanceId={} sessionRef={} generation={} method={} elapsedMs={} outcome=ok",
            instance.engine_id(),
            instance.instance_id(),
            session_ref,
            generation,
            method,
            elapsed,
        ),
        Err(error) => log::warn!(
            "engine_runtime engineId={} instanceId={} sessionRef={} generation={} method={} elapsedMs={} outcome=error errorKind={:?}",
            instance.engine_id(),
            instance.instance_id(),
            session_ref,
            generation,
            method,
            elapsed,
            error.kind,
        ),
    }
}

fn runtime_event_name(event: &NormalizedRuntimeEvent) -> &'static str {
    match event {
        NormalizedRuntimeEvent::SessionAttached => "sessionAttached",
        NormalizedRuntimeEvent::SessionDetached => "sessionDetached",
        NormalizedRuntimeEvent::TurnStarted { .. } => "turnStarted",
        NormalizedRuntimeEvent::ItemStarted { .. } => "itemStarted",
        NormalizedRuntimeEvent::ItemDelta { .. } => "itemDelta",
        NormalizedRuntimeEvent::ItemCompleted { .. } => "itemCompleted",
        NormalizedRuntimeEvent::InteractionRequested { .. } => "interactionRequested",
        NormalizedRuntimeEvent::InteractionResolved { .. } => "interactionResolved",
        NormalizedRuntimeEvent::TurnCompleted { .. } => "turnCompleted",
        NormalizedRuntimeEvent::RuntimeError { .. } => "runtimeError",
        NormalizedRuntimeEvent::RuntimeExited => "runtimeExited",
        NormalizedRuntimeEvent::CapabilitiesChanged => "capabilitiesChanged",
    }
}

fn apply_event(snapshot: &mut RuntimeSnapshot, event: NormalizedRuntimeEvent) {
    match event {
        NormalizedRuntimeEvent::SessionAttached => snapshot.phase = RuntimePhase::Idle,
        NormalizedRuntimeEvent::SessionDetached => snapshot.phase = RuntimePhase::Detached,
        NormalizedRuntimeEvent::TurnStarted { turn_id } => {
            snapshot.phase = RuntimePhase::Running;
            snapshot.active_turn_id = Some(turn_id);
        }
        NormalizedRuntimeEvent::InteractionRequested { request } => {
            snapshot.phase = RuntimePhase::AwaitingInteraction;
            snapshot.pending_interactions.push(request);
        }
        NormalizedRuntimeEvent::InteractionResolved { reference, .. } => {
            snapshot
                .pending_interactions
                .retain(|request| request.reference != reference);
            snapshot.phase = if snapshot.pending_interactions.is_empty() {
                RuntimePhase::Running
            } else {
                RuntimePhase::AwaitingInteraction
            };
        }
        NormalizedRuntimeEvent::TurnCompleted { error, .. } => {
            snapshot.phase = if error.is_some() {
                RuntimePhase::Failed
            } else {
                RuntimePhase::Idle
            };
            snapshot.active_turn_id = None;
            snapshot.pending_interactions.clear();
            snapshot.last_error = error;
        }
        NormalizedRuntimeEvent::RuntimeError { message, .. } => {
            snapshot.phase = RuntimePhase::Failed;
            snapshot.last_error = Some(message);
        }
        NormalizedRuntimeEvent::RuntimeExited => snapshot.phase = RuntimePhase::Exited,
        NormalizedRuntimeEvent::ItemStarted { .. }
        | NormalizedRuntimeEvent::ItemDelta { .. }
        | NormalizedRuntimeEvent::ItemCompleted { .. }
        | NormalizedRuntimeEvent::CapabilitiesChanged => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::core::EngineInstanceId;

    fn envelope(
        generation: u64,
        sequence: u64,
        event: NormalizedRuntimeEvent,
    ) -> RuntimeEventEnvelope {
        let session = SessionRef::new(
            EngineInstanceId::new("fixture", "default").unwrap(),
            "session",
        )
        .unwrap();
        RuntimeEventEnvelope {
            session,
            runtime_id: RuntimeId(format!("runtime-{generation}")),
            generation,
            sequence,
            timestamp: "2026-01-01T00:00:00Z".into(),
            event,
        }
    }

    #[test]
    fn ignores_old_generation_and_marks_sequence_gaps() {
        let registry = Arc::new(EngineRegistry::new());
        let coordinator = RuntimeCoordinator::new(registry);
        coordinator.ingest(envelope(
            2,
            1,
            NormalizedRuntimeEvent::TurnStarted {
                turn_id: "new".into(),
            },
        ));
        coordinator.ingest(envelope(
            1,
            99,
            NormalizedRuntimeEvent::TurnCompleted {
                turn_id: "old".into(),
                status: super::super::TurnStatus::Completed,
                error: None,
            },
        ));
        coordinator.ingest(envelope(2, 3, NormalizedRuntimeEvent::CapabilitiesChanged));

        let session = envelope(2, 1, NormalizedRuntimeEvent::CapabilitiesChanged).session;
        let snapshot = coordinator.snapshot(&session).unwrap();
        assert_eq!(snapshot.phase, RuntimePhase::Running);
        assert_eq!(snapshot.active_turn_id.as_deref(), Some("new"));
        assert!(!snapshot.sequence_consistent);

        let reconciled = coordinator.reconcile_snapshots();
        assert!(reconciled.iter().all(|value| value.sequence_consistent));
        assert!(coordinator.snapshot(&session).unwrap().sequence_consistent);
    }

    #[test]
    fn interaction_state_returns_to_running_after_response() {
        let registry = Arc::new(EngineRegistry::new());
        let coordinator = RuntimeCoordinator::new(registry);
        let session = envelope(1, 1, NormalizedRuntimeEvent::CapabilitiesChanged).session;
        let reference = InteractionRef {
            session: session.clone(),
            runtime_id: RuntimeId("runtime-1".into()),
            request_id: "approval".into(),
            turn_id: Some("turn".into()),
        };
        coordinator.ingest(envelope(
            1,
            1,
            NormalizedRuntimeEvent::TurnStarted {
                turn_id: "turn".into(),
            },
        ));
        coordinator.ingest(envelope(
            1,
            2,
            NormalizedRuntimeEvent::InteractionRequested {
                request: InteractionRequest {
                    reference: reference.clone(),
                    kind: super::super::InteractionKind::Command,
                    title: None,
                    payload: serde_json::Value::Null,
                    options: Vec::new(),
                },
            },
        ));
        coordinator.ingest(envelope(
            1,
            3,
            NormalizedRuntimeEvent::InteractionResolved {
                reference,
                decision: "allow".into(),
            },
        ));

        assert_eq!(
            coordinator.snapshot(&session).unwrap().phase,
            RuntimePhase::Running
        );
    }

    #[test]
    fn completed_turn_keeps_runtime_attached_and_idle() {
        let coordinator = RuntimeCoordinator::new(Arc::new(EngineRegistry::new()));
        let session = envelope(1, 1, NormalizedRuntimeEvent::CapabilitiesChanged).session;
        coordinator.ingest(envelope(
            1,
            1,
            NormalizedRuntimeEvent::TurnStarted {
                turn_id: "turn".into(),
            },
        ));
        coordinator.ingest(envelope(
            1,
            2,
            NormalizedRuntimeEvent::TurnCompleted {
                turn_id: "turn".into(),
                status: super::super::TurnStatus::Completed,
                error: None,
            },
        ));

        let snapshot = coordinator.snapshot(&session).unwrap();
        assert_eq!(snapshot.phase, RuntimePhase::Idle);
        assert!(snapshot.active_turn_id.is_none());
    }

    #[test]
    fn late_events_after_runtime_exit_do_not_revive_generation() {
        let coordinator = RuntimeCoordinator::new(Arc::new(EngineRegistry::new()));
        let session = envelope(3, 1, NormalizedRuntimeEvent::CapabilitiesChanged).session;
        coordinator.ingest(envelope(3, 1, NormalizedRuntimeEvent::RuntimeExited));
        coordinator.ingest(envelope(
            3,
            2,
            NormalizedRuntimeEvent::TurnCompleted {
                turn_id: "late".into(),
                status: super::super::TurnStatus::Completed,
                error: None,
            },
        ));

        let snapshot = coordinator.snapshot(&session).unwrap();
        assert_eq!(snapshot.phase, RuntimePhase::Exited);
        assert_eq!(snapshot.last_sequence, 1);
    }
}
