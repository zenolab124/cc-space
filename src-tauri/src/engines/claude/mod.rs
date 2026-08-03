mod adapter;
mod runtime;
mod source;

pub use runtime::*;
pub use source::*;

use super::core::{EngineInstanceId, EngineResult};

pub const ENGINE_ID: &str = "claude-code";
pub const DEFAULT_INSTANCE_ID: &str = "default";

pub fn default_instance() -> EngineResult<EngineInstanceId> {
    EngineInstanceId::new(ENGINE_ID, DEFAULT_INSTANCE_ID)
}
pub use adapter::*;
