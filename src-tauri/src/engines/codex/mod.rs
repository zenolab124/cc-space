mod adapter;
pub mod app_server;
mod file_source;
mod runtime;
mod source;
mod supervisor;
mod usage;

pub use runtime::*;
pub use source::*;
pub use supervisor::*;

pub(crate) use usage::collect_local_usage;

use super::core::{EngineInstanceId, EngineResult};

pub const ENGINE_ID: &str = "codex";
pub const DEFAULT_INSTANCE_ID: &str = "default";

pub fn default_instance() -> EngineResult<EngineInstanceId> {
    EngineInstanceId::new(ENGINE_ID, DEFAULT_INSTANCE_ID)
}
pub use adapter::*;
