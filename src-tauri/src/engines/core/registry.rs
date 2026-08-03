use std::collections::BTreeMap;
use std::sync::Arc;

use super::{
    AgentRuntime, EngineDescriptor, EngineError, EngineErrorKind, EngineFuture, EngineHealth,
    EngineInstanceId, EngineResult, SessionRef, SessionSource,
};

pub trait EngineAdapter: Send + Sync {
    fn descriptor(&self) -> EngineDescriptor;
    fn health(&self) -> EngineFuture<'_, EngineHealth>;
    fn session_source(&self) -> &dyn SessionSource;
    fn runtime(&self) -> Option<&dyn AgentRuntime>;
}

#[derive(Default)]
pub struct EngineRegistry {
    adapters: BTreeMap<EngineInstanceId, Arc<dyn EngineAdapter>>,
}

impl EngineRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Arc<dyn EngineAdapter>) -> EngineResult<()> {
        let instance = adapter.descriptor().instance;
        if self.adapters.contains_key(&instance) {
            return Err(EngineError::new(
                EngineErrorKind::Conflict,
                format!(
                    "engine instance already registered: {}",
                    instance.storage_key()
                ),
            ));
        }
        self.adapters.insert(instance, adapter);
        Ok(())
    }

    pub fn descriptors(&self) -> Vec<EngineDescriptor> {
        self.adapters
            .values()
            .map(|adapter| adapter.descriptor())
            .collect()
    }

    pub fn adapter(&self, instance: &EngineInstanceId) -> EngineResult<&dyn EngineAdapter> {
        self.adapters
            .get(instance)
            .map(AsRef::as_ref)
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorKind::NotFound,
                    "engine instance is not registered",
                )
            })
    }

    pub fn source_for(&self, session: &SessionRef) -> EngineResult<&dyn SessionSource> {
        Ok(self.adapter(session.engine())?.session_source())
    }

    pub fn runtime_for(&self, session: &SessionRef) -> EngineResult<&dyn AgentRuntime> {
        self.adapter(session.engine())?.runtime().ok_or_else(|| {
            EngineError::new(
                EngineErrorKind::Unsupported,
                "engine instance does not provide a runtime",
            )
        })
    }
}
