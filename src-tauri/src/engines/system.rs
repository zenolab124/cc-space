use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::claude::ClaudeEngine;
use super::codex::CodexEngine;
use super::core::{
    EngineAdapter, EngineDescriptor, EngineError, EngineInstanceId, EngineRegistry,
    RuntimeCoordinator, RuntimeEventEnvelope, RuntimeSnapshot, SourceChange, SourceChangeSink,
    SubscriptionHandle,
};

const RUNTIME_SNAPSHOT_EVENT: &str = "engine-runtime-snapshot";
const RUNTIME_EVENTS_EVENT: &str = "engine-runtime-events";
const SOURCE_CHANGE_EVENT: &str = "engine-source-change";

static SYSTEM: OnceLock<Arc<EngineSystem>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInitializationError {
    pub engine_id: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceChangeEnvelope {
    pub instance: EngineInstanceId,
    pub change: SourceChange,
}

pub struct EngineSystem {
    registry: Arc<EngineRegistry>,
    coordinator: Arc<RuntimeCoordinator>,
    initialization_errors: Vec<EngineInitializationError>,
    _source_subscriptions: Vec<SubscriptionHandle>,
}

impl EngineSystem {
    fn build(app: AppHandle) -> Arc<Self> {
        let mut registry = EngineRegistry::new();
        let mut initialization_errors = Vec::new();

        register_configured_adapter(
            &mut registry,
            &mut initialization_errors,
            "claude-code",
            ClaudeEngine::descriptor(),
            || ClaudeEngine::new(app.clone()).map(|adapter| adapter as Arc<dyn EngineAdapter>),
        );
        register_configured_adapter(
            &mut registry,
            &mut initialization_errors,
            "codex",
            CodexEngine::descriptor(),
            || CodexEngine::new().map(|adapter| adapter as Arc<dyn EngineAdapter>),
        );

        let registry = Arc::new(registry);
        let coordinator = RuntimeCoordinator::new(Arc::clone(&registry));
        let runtime_app = app.clone();
        coordinator.subscribe(Arc::new(move |snapshot: RuntimeSnapshot| {
            let _ = runtime_app.emit(RUNTIME_SNAPSHOT_EVENT, snapshot);
        }));
        let event_sender = start_runtime_event_batcher(app.clone());
        coordinator.subscribe_events(Arc::new(move |event| {
            let _ = event_sender.send(event);
        }));

        let mut source_subscriptions = Vec::new();
        for descriptor in registry.descriptors() {
            let Ok(adapter) = registry.adapter_arc(&descriptor.instance) else {
                continue;
            };
            let source_app = app.clone();
            let instance_key = descriptor.instance.storage_key();
            let instance = descriptor.instance;
            let sink: SourceChangeSink = Arc::new(move |change| {
                super::search::invalidate(&instance, &change);
                let _ = source_app.emit(
                    SOURCE_CHANGE_EVENT,
                    SourceChangeEnvelope {
                        instance: instance.clone(),
                        change,
                    },
                );
            });
            match adapter.session_source().subscribe_changes(sink) {
                Ok(subscription) => source_subscriptions.push(subscription),
                Err(error) => log::warn!(
                    "engine source change subscription failed for {}: {}",
                    instance_key,
                    error
                ),
            }
        }

        Arc::new(Self {
            registry,
            coordinator,
            initialization_errors,
            _source_subscriptions: source_subscriptions,
        })
    }

    pub fn registry(&self) -> &Arc<EngineRegistry> {
        &self.registry
    }

    pub fn coordinator(&self) -> &Arc<RuntimeCoordinator> {
        &self.coordinator
    }

    pub fn initialization_errors(&self) -> &[EngineInitializationError] {
        &self.initialization_errors
    }
}

fn start_runtime_event_batcher(app: AppHandle) -> SyncSender<RuntimeEventEnvelope> {
    let (sender, receiver) = mpsc::sync_channel(4_096);
    std::thread::spawn(move || {
        while let Ok(first) = receiver.recv() {
            let mut batch = vec![first];
            let deadline = Instant::now() + Duration::from_millis(16);
            while batch.len() < 256 {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                match receiver.recv_timeout(remaining) {
                    Ok(event) => batch.push(event),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        let _ = app.emit(RUNTIME_EVENTS_EVENT, batch);
                        return;
                    }
                }
            }
            let _ = app.emit(RUNTIME_EVENTS_EVENT, batch);
        }
    });
    sender
}

#[cfg(test)]
fn register_adapter(
    registry: &mut EngineRegistry,
    errors: &mut Vec<EngineInitializationError>,
    engine_id: &str,
    adapter: Result<Arc<dyn EngineAdapter>, EngineError>,
) {
    let result = adapter.and_then(|adapter| registry.register(adapter));
    if let Err(error) = result {
        record_initialization_error(errors, engine_id, error);
    }
}

fn register_configured_adapter<F>(
    registry: &mut EngineRegistry,
    errors: &mut Vec<EngineInitializationError>,
    engine_id: &str,
    descriptor: Result<EngineDescriptor, EngineError>,
    build: F,
) where
    F: FnOnce() -> Result<Arc<dyn EngineAdapter>, EngineError>,
{
    let descriptor = match descriptor {
        Ok(descriptor) => descriptor,
        Err(error) => {
            record_initialization_error(errors, engine_id, error);
            return;
        }
    };
    if !super::preferences::is_enabled(&descriptor.instance) {
        if let Err(error) = registry.register_disabled(descriptor) {
            record_initialization_error(errors, engine_id, error);
        }
        return;
    }
    if let Err(error) = build().and_then(|adapter| registry.register(adapter)) {
        let catalog_error = registry.register_unavailable(descriptor);
        record_initialization_error(errors, engine_id, error);
        if let Err(catalog_error) = catalog_error {
            record_initialization_error(errors, engine_id, catalog_error);
        }
    }
}

fn record_initialization_error(
    errors: &mut Vec<EngineInitializationError>,
    engine_id: &str,
    error: EngineError,
) {
    log::warn!("engine initialization failed for {engine_id}: {error}");
    errors.push(EngineInitializationError {
        engine_id: engine_id.to_string(),
        message: error.message,
    });
}

pub fn initialize(app: AppHandle) -> Arc<EngineSystem> {
    SYSTEM.get_or_init(|| EngineSystem::build(app)).clone()
}

pub fn get() -> Result<&'static Arc<EngineSystem>, EngineError> {
    SYSTEM.get().ok_or_else(|| {
        EngineError::new(
            super::core::EngineErrorKind::Unavailable,
            "engine system is not initialized",
        )
    })
}

pub fn notify_source_change(instance: &EngineInstanceId, change: SourceChange) {
    let Ok(system) = get() else {
        return;
    };
    let Ok(adapter) = system.registry().adapter_arc(instance) else {
        return;
    };
    adapter.notify_source_change(change);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::core::{EngineDescriptor, EngineFuture, EngineHealth, SessionSource};

    struct BrokenAdapter;

    impl EngineAdapter for BrokenAdapter {
        fn descriptor(&self) -> EngineDescriptor {
            unreachable!()
        }

        fn health(&self) -> EngineFuture<'_, EngineHealth> {
            unreachable!()
        }

        fn session_source(&self) -> &dyn SessionSource {
            unreachable!()
        }

        fn runtime(&self) -> Option<&dyn super::super::core::AgentRuntime> {
            None
        }
    }

    #[test]
    fn failed_adapter_does_not_remove_registered_engines() {
        let mut registry = EngineRegistry::new();
        let mut errors = Vec::new();
        register_adapter(
            &mut registry,
            &mut errors,
            "broken",
            Err(EngineError::new(
                super::super::core::EngineErrorKind::Unavailable,
                "fixture failure",
            )),
        );

        assert!(registry.descriptors().is_empty());
        assert_eq!(errors.len(), 1);
        let _ = BrokenAdapter;
    }
}
