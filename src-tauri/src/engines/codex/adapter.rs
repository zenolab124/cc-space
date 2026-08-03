use std::process::Command;
use std::sync::Arc;

use serde_json::json;

use super::{default_instance, CodexRuntime, CodexSource, CodexSupervisor};
use crate::engines::core::*;
use crate::proc_ext::HideConsole;

const MINIMUM_CODEX_VERSION: (u32, u32, u32) = (0, 146, 0);

pub struct CodexEngine {
    descriptor: EngineDescriptor,
    source: CodexSource,
    runtime: Arc<CodexRuntime>,
    supervisor: Arc<CodexSupervisor>,
}

impl CodexEngine {
    pub fn descriptor() -> EngineResult<EngineDescriptor> {
        let instance = default_instance()?;
        Ok(EngineDescriptor {
            instance,
            display_name: "Codex".into(),
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
                    steer: true,
                    interrupt: true,
                    streaming: StreamingCapabilities {
                        text: StreamGranularity::Delta,
                        reasoning: StreamGranularity::Delta,
                        tool_progress: StreamGranularity::Delta,
                    },
                    model_catalog: ModelCatalogKind::Dynamic,
                    interactions: vec![
                        InteractionKind::Command,
                        InteractionKind::FileChange,
                        InteractionKind::Permissions,
                    ],
                }),
                facets: FacetCapabilities {
                    assets: false,
                    automation: false,
                    configuration: false,
                    quota: true,
                    runtime_commands: false,
                },
            },
            ui: EngineUiIntegration {
                identity: UiIdentityMode::Structured,
                session_surface: SessionSurface::Standard,
                install_guide_url: Some("https://developers.openai.com/codex/cli/".into()),
                configuration_guide_url: Some(
                    "https://developers.openai.com/codex/config-basic/".into(),
                ),
            },
        })
    }

    pub fn new() -> EngineResult<Arc<Self>> {
        let supervisor = CodexSupervisor::new();
        Ok(Arc::new(Self {
            descriptor: Self::descriptor()?,
            source: CodexSource::new(Arc::clone(&supervisor))?,
            runtime: CodexRuntime::new(Arc::clone(&supervisor))?,
            supervisor,
        }))
    }
}

impl EngineAdapter for CodexEngine {
    fn descriptor(&self) -> EngineDescriptor {
        self.descriptor.clone()
    }

    fn health(&self) -> EngineFuture<'_, EngineHealth> {
        Box::pin(async move {
            let path = match crate::codex_locator::locate() {
                Ok(path) => path,
                Err(error) => {
                    return Ok(EngineHealth {
                        instance: self.descriptor.instance.clone(),
                        status: EngineHealthStatus::Unavailable,
                        installed: false,
                        authenticated: None,
                        version: None,
                        version_supported: None,
                        executable_path: None,
                        source: CapabilityHealth::unavailable("engine.codex.cliUnavailable"),
                        runtime: CapabilityHealth::unavailable("engine.codex.cliUnavailable"),
                        diagnostics: vec![HealthDiagnostic {
                            code: "cliNotFound".into(),
                            message: error,
                        }],
                    });
                }
            };
            let version = cli_version(&path);
            let version_supported = supported_version(version.as_deref());
            if version_supported == Some(false) {
                return Ok(EngineHealth {
                    instance: self.descriptor.instance.clone(),
                    status: EngineHealthStatus::Degraded,
                    installed: true,
                    authenticated: None,
                    version,
                    version_supported,
                    executable_path: Some(path.to_string_lossy().to_string()),
                    source: CapabilityHealth::unavailable("engine.codex.versionUnsupported"),
                    runtime: CapabilityHealth::unavailable("engine.codex.versionUnsupported"),
                    diagnostics: vec![HealthDiagnostic {
                        code: "versionUnsupported".into(),
                        message: "Codex CLI is older than the supported App Server baseline".into(),
                    }],
                });
            }
            if let Err(error) = self
                .supervisor
                .request("thread/list", json!({ "limit": 1 }))
            {
                return Ok(EngineHealth {
                    instance: self.descriptor.instance.clone(),
                    status: EngineHealthStatus::Degraded,
                    installed: true,
                    authenticated: None,
                    version,
                    version_supported,
                    executable_path: Some(path.to_string_lossy().to_string()),
                    source: CapabilityHealth::unavailable("engine.codex.handshakeFailed"),
                    runtime: CapabilityHealth::unavailable("engine.codex.handshakeFailed"),
                    diagnostics: vec![HealthDiagnostic {
                        code: "appServerHandshakeFailed".into(),
                        message: error.message,
                    }],
                });
            }
            match self
                .supervisor
                .request("account/read", json!({ "refreshToken": false }))
            {
                Ok(account) => {
                    let authenticated = account_is_authenticated(&account);
                    Ok(EngineHealth {
                        instance: self.descriptor.instance.clone(),
                        status: if authenticated {
                            EngineHealthStatus::Available
                        } else {
                            EngineHealthStatus::Degraded
                        },
                        installed: true,
                        authenticated: Some(authenticated),
                        version,
                        version_supported,
                        executable_path: Some(path.to_string_lossy().to_string()),
                        source: CapabilityHealth::available(),
                        runtime: if authenticated {
                            CapabilityHealth::available()
                        } else {
                            CapabilityHealth::unavailable("engine.codex.authenticationRequired")
                        },
                        diagnostics: Vec::new(),
                    })
                }
                Err(error) => Ok(EngineHealth {
                    instance: self.descriptor.instance.clone(),
                    status: EngineHealthStatus::Degraded,
                    installed: true,
                    authenticated: None,
                    version,
                    version_supported,
                    executable_path: Some(path.to_string_lossy().to_string()),
                    source: CapabilityHealth::available(),
                    runtime: CapabilityHealth::unavailable("engine.codex.accountProbeFailed"),
                    diagnostics: vec![HealthDiagnostic {
                        code: "accountProbeFailed".into(),
                        message: error.message,
                    }],
                }),
            }
        })
    }

    fn session_source(&self) -> &dyn SessionSource {
        &self.source
    }

    fn runtime(&self) -> Option<&dyn AgentRuntime> {
        Some(self.runtime.as_ref())
    }

    fn models(&self) -> Option<&dyn ModelCatalogProvider> {
        Some(self.runtime.as_ref())
    }

    fn quota(&self) -> Option<&dyn QuotaProvider> {
        Some(self)
    }
}

impl QuotaProvider for CodexEngine {
    fn read_quota(&self, force_refresh: bool) -> EngineFuture<'_, serde_json::Value> {
        Box::pin(async move {
            let quota =
                crate::quota::load_provider_quota("codex", force_refresh).ok_or_else(|| {
                    EngineError::new(EngineErrorKind::NotFound, "quota provider not found")
                })?;
            serde_json::to_value(quota)
                .map_err(|error| EngineError::new(EngineErrorKind::Internal, error.to_string()))
        })
    }
}

pub(super) fn cli_version(path: &std::path::Path) -> Option<String> {
    let output = Command::new(path)
        .arg("--version")
        .env("PATH", crate::path_env::enhanced_path())
        .hide_console()
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|version| !version.is_empty())
}

fn parse_version(value: &str) -> Option<(u32, u32, u32)> {
    let numeric = value.split_whitespace().find(|part| {
        part.chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    })?;
    let mut parts = numeric.trim_start_matches('v').split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts
            .next()?
            .split(|character: char| !character.is_ascii_digit())
            .next()?
            .parse()
            .ok()?,
    ))
}

pub(super) fn supported_version(value: Option<&str>) -> Option<bool> {
    value
        .and_then(parse_version)
        .map(|version| version >= MINIMUM_CODEX_VERSION)
}

pub(super) fn account_is_authenticated(account: &serde_json::Value) -> bool {
    let requires_auth = account
        .get("requiresOpenaiAuth")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    account.get("account").is_some_and(|value| !value.is_null()) || !requires_auth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_cli_versions() {
        assert_eq!(parse_version("codex-cli 0.146.0"), Some((0, 146, 0)));
        assert_eq!(parse_version("codex 1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(supported_version(Some("codex-cli 0.145.0")), Some(false));
        assert_eq!(supported_version(Some("codex-cli 0.146.0")), Some(true));
    }

    #[test]
    fn account_authentication_respects_local_no_auth_mode() {
        assert!(account_is_authenticated(&serde_json::json!({
            "requiresOpenaiAuth": false,
            "account": null
        })));
        assert!(account_is_authenticated(&serde_json::json!({
            "requiresOpenaiAuth": true,
            "account": { "type": "chatgpt" }
        })));
        assert!(!account_is_authenticated(&serde_json::json!({
            "requiresOpenaiAuth": true,
            "account": null
        })));
    }
}
