use serde::{Deserialize, Serialize};

use super::EngineInstanceId;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryPagination {
    Native,
    Emulated,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeDelivery {
    Push,
    Watch,
    Poll,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamGranularity {
    Delta,
    Item,
    Final,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelCatalogKind {
    Dynamic,
    Configured,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InteractionKind {
    Command,
    FileChange,
    Permissions,
    Question,
    Plan,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCapabilities {
    pub pagination: HistoryPagination,
    pub change_delivery: ChangeDelivery,
    pub search: bool,
    pub assets: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingCapabilities {
    pub text: StreamGranularity,
    pub reasoning: StreamGranularity,
    pub tool_progress: StreamGranularity,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub create: bool,
    pub resume: bool,
    pub fork: bool,
    pub steer: bool,
    pub interrupt: bool,
    pub streaming: StreamingCapabilities,
    pub model_catalog: ModelCatalogKind,
    pub interactions: Vec<InteractionKind>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetCapabilities {
    pub assets: bool,
    pub automation: bool,
    pub configuration: bool,
    pub quota: bool,
    pub runtime_commands: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineCapabilities {
    pub history: HistoryCapabilities,
    pub runtime: Option<RuntimeCapabilities>,
    pub facets: FacetCapabilities,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDescriptor {
    pub instance: EngineInstanceId,
    pub display_name: String,
    pub enabled: bool,
    pub capabilities: EngineCapabilities,
    pub ui: EngineUiIntegration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineUiIntegration {
    pub identity: UiIdentityMode,
    pub session_surface: SessionSurface,
    pub install_guide_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UiIdentityMode {
    Structured,
    Native,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionSurface {
    Standard,
    Native,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineHealthStatus {
    Available,
    Degraded,
    Unavailable,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityHealth {
    pub available: bool,
    pub reason_code: Option<String>,
}

impl CapabilityHealth {
    pub fn available() -> Self {
        Self {
            available: true,
            reason_code: None,
        }
    }

    pub fn unavailable(reason_code: impl Into<String>) -> Self {
        Self {
            available: false,
            reason_code: Some(reason_code.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineHealth {
    pub instance: EngineInstanceId,
    pub status: EngineHealthStatus,
    pub installed: bool,
    pub authenticated: Option<bool>,
    pub version: Option<String>,
    pub version_supported: Option<bool>,
    pub executable_path: Option<String>,
    pub source: CapabilityHealth,
    pub runtime: CapabilityHealth,
    pub diagnostics: Vec<HealthDiagnostic>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionAvailability {
    pub available: bool,
    pub reason_code: Option<String>,
}

impl ActionAvailability {
    pub fn available() -> Self {
        Self {
            available: true,
            reason_code: None,
        }
    }

    pub fn unavailable(reason_code: impl Into<String>) -> Self {
        Self {
            available: false,
            reason_code: Some(reason_code.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActions {
    pub resume: ActionAvailability,
    pub fork: ActionAvailability,
    pub send: ActionAvailability,
    pub steer: ActionAvailability,
    pub interrupt: ActionAvailability,
    pub open_cwd: ActionAvailability,
}

impl SessionActions {
    pub fn read_only(reason_code: impl Into<String>) -> Self {
        let reason_code = reason_code.into();
        Self {
            resume: ActionAvailability::unavailable(reason_code.clone()),
            fork: ActionAvailability::unavailable(reason_code.clone()),
            send: ActionAvailability::unavailable(reason_code.clone()),
            steer: ActionAvailability::unavailable(reason_code.clone()),
            interrupt: ActionAvailability::unavailable(reason_code),
            open_cwd: ActionAvailability::available(),
        }
    }
}
