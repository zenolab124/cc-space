use std::collections::BTreeMap;
use std::sync::Arc;

use super::{
    AgentRuntime, EngineDescriptor, EngineError, EngineErrorKind, EngineFuture, EngineHealth,
    EngineInstanceId, EngineResult, SessionRef, SessionSource, SourceChange,
};

pub trait EngineAdapter: Send + Sync {
    fn descriptor(&self) -> EngineDescriptor;
    fn health(&self) -> EngineFuture<'_, EngineHealth>;
    fn session_source(&self) -> &dyn SessionSource;
    fn runtime(&self) -> Option<&dyn AgentRuntime>;
    fn assets(&self) -> Option<&dyn super::AssetProvider> {
        None
    }
    fn configuration(&self) -> Option<&dyn super::ConfigurationProvider> {
        None
    }
    fn quota(&self) -> Option<&dyn super::QuotaProvider> {
        None
    }
    fn runtime_commands(&self) -> Option<&dyn super::RuntimeCommandProvider> {
        None
    }
    fn models(&self) -> Option<&dyn super::ModelCatalogProvider> {
        None
    }
    /// 将引擎原生的外部数据变更送入适配器自己的订阅通道。
    ///
    /// 仅需要桥接现有文件监听器等外部信号源的适配器实现此方法；直接从协议
    /// 推送变更的适配器可以沿用默认空实现。
    fn notify_source_change(&self, _change: SourceChange) {}
}

#[derive(Default)]
pub struct EngineRegistry {
    entries: BTreeMap<EngineInstanceId, EngineRegistryEntry>,
}

struct EngineRegistryEntry {
    descriptor: EngineDescriptor,
    adapter: Option<Arc<dyn EngineAdapter>>,
}

impl EngineRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Arc<dyn EngineAdapter>) -> EngineResult<()> {
        let mut descriptor = adapter.descriptor();
        descriptor.enabled = true;
        let instance = descriptor.instance.clone();
        self.insert(
            instance,
            EngineRegistryEntry {
                descriptor,
                adapter: Some(adapter),
            },
        )
    }

    pub fn register_disabled(&mut self, mut descriptor: EngineDescriptor) -> EngineResult<()> {
        descriptor.enabled = false;
        let instance = descriptor.instance.clone();
        self.insert(
            instance,
            EngineRegistryEntry {
                descriptor,
                adapter: None,
            },
        )
    }

    pub fn register_unavailable(&mut self, mut descriptor: EngineDescriptor) -> EngineResult<()> {
        descriptor.enabled = true;
        let instance = descriptor.instance.clone();
        self.insert(
            instance,
            EngineRegistryEntry {
                descriptor,
                adapter: None,
            },
        )
    }

    fn insert(
        &mut self,
        instance: EngineInstanceId,
        entry: EngineRegistryEntry,
    ) -> EngineResult<()> {
        if self.entries.contains_key(&instance) {
            return Err(EngineError::new(
                EngineErrorKind::Conflict,
                format!(
                    "engine instance already registered: {}",
                    instance.storage_key()
                ),
            ));
        }
        self.entries.insert(instance, entry);
        Ok(())
    }

    pub fn descriptors(&self) -> Vec<EngineDescriptor> {
        self.entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect()
    }

    pub fn descriptor(&self, instance: &EngineInstanceId) -> EngineResult<EngineDescriptor> {
        self.entries
            .get(instance)
            .map(|entry| entry.descriptor.clone())
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorKind::NotFound,
                    "engine instance is not registered",
                )
            })
    }

    pub fn is_enabled(&self, instance: &EngineInstanceId) -> bool {
        self.entries
            .get(instance)
            .is_some_and(|entry| entry.descriptor.enabled)
    }

    pub fn is_available(&self, instance: &EngineInstanceId) -> bool {
        self.entries
            .get(instance)
            .is_some_and(|entry| entry.adapter.is_some())
    }

    pub fn adapter(&self, instance: &EngineInstanceId) -> EngineResult<&dyn EngineAdapter> {
        self.entries
            .get(instance)
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorKind::NotFound,
                    "engine instance is not registered",
                )
            })?
            .adapter
            .as_deref()
            .ok_or_else(|| unavailable_entry_error(self.entries.get(instance)))
    }

    pub fn adapter_arc(&self, instance: &EngineInstanceId) -> EngineResult<Arc<dyn EngineAdapter>> {
        self.entries
            .get(instance)
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorKind::NotFound,
                    "engine instance is not registered",
                )
            })?
            .adapter
            .clone()
            .ok_or_else(|| unavailable_entry_error(self.entries.get(instance)))
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

fn unavailable_entry_error(entry: Option<&EngineRegistryEntry>) -> EngineError {
    let message = if entry.is_some_and(|entry| entry.descriptor.enabled) {
        "engine instance failed to initialize"
    } else {
        "engine instance is disabled"
    };
    EngineError::new(EngineErrorKind::Unavailable, message)
}
