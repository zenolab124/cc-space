use std::process::Command;
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use super::{default_instance, CodexRuntime, CodexSource, CodexSupervisor, CODEX_READINESS_EVENT};
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
                    fork: true,
                    send_while_running: true,
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
                    assets: true,
                    automation: true,
                    configuration: false,
                    quota: true,
                    runtime_commands: false,
                },
            },
            ui: EngineUiIntegration {
                identity: UiIdentityMode::Structured,
                session_surface: SessionSurface::Standard,
                install_guide_url: Some("https://developers.openai.com/codex/cli/".into()),
            },
        })
    }

    pub fn new(app: AppHandle) -> EngineResult<Arc<Self>> {
        let supervisor = CodexSupervisor::new();
        let readiness_app = app.clone();
        supervisor.subscribe_readiness(Arc::new(move |snapshot| {
            let _ = readiness_app.emit(CODEX_READINESS_EVENT, snapshot);
        }));
        let adapter = Arc::new(Self {
            descriptor: Self::descriptor()?,
            source: CodexSource::new(Arc::clone(&supervisor))?,
            runtime: CodexRuntime::new(Arc::clone(&supervisor))?,
            supervisor: Arc::clone(&supervisor),
        });
        supervisor.prewarm();
        Ok(adapter)
    }
}

impl EngineAdapter for CodexEngine {
    fn descriptor(&self) -> EngineDescriptor {
        self.descriptor.clone()
    }

    fn health(&self) -> EngineFuture<'_, EngineHealth> {
        Box::pin(async move {
            let history_probe = self.source.probe_history();
            let source = if history_probe.is_ok() {
                CapabilityHealth::available()
            } else {
                CapabilityHealth::unavailable("engine.codex.historyUnavailable")
            };
            let mut diagnostics = history_probe
                .as_ref()
                .err()
                .map(|error| HealthDiagnostic {
                    code: "historyProbeFailed".into(),
                    message: error.message.clone(),
                })
                .into_iter()
                .collect::<Vec<_>>();
            let path = match crate::codex_locator::locate() {
                Ok(path) => path,
                Err(error) => {
                    diagnostics.push(HealthDiagnostic {
                        code: "cliNotFound".into(),
                        message: error.clone(),
                    });
                    let runtime = missing_cli_runtime(&mut diagnostics);
                    return Ok(EngineHealth {
                        instance: self.descriptor.instance.clone(),
                        status: status_for(&source, &runtime),
                        installed: false,
                        authenticated: None,
                        version: None,
                        version_supported: None,
                        executable_path: None,
                        runtime,
                        source,
                        diagnostics,
                    });
                }
            };
            let version = cli_version(&path);
            let version_supported = supported_version(version.as_deref());
            if version_supported == Some(false) {
                diagnostics.push(HealthDiagnostic {
                    code: "versionUnsupported".into(),
                    message: "Codex CLI is older than the supported App Server baseline".into(),
                });
                let runtime = CapabilityHealth::unavailable("engine.codex.versionUnsupported");
                return Ok(EngineHealth {
                    instance: self.descriptor.instance.clone(),
                    status: status_for(&source, &runtime),
                    installed: true,
                    authenticated: None,
                    version,
                    version_supported,
                    executable_path: Some(path.to_string_lossy().to_string()),
                    source,
                    runtime,
                    diagnostics,
                });
            }
            let (authenticated, runtime) = match self
                .supervisor
                .request("account/read", json!({ "refreshToken": false }))
            {
                Ok(account) => {
                    let authenticated = account_is_authenticated(&account);
                    (
                        Some(authenticated),
                        if authenticated {
                            CapabilityHealth::available()
                        } else {
                            CapabilityHealth::unavailable("engine.codex.authenticationRequired")
                        },
                    )
                }
                Err(error) => {
                    diagnostics.push(HealthDiagnostic {
                        code: "accountProbeFailed".into(),
                        message: error.message,
                    });
                    (
                        None,
                        CapabilityHealth::unavailable("engine.codex.accountProbeFailed"),
                    )
                }
            };
            Ok(EngineHealth {
                instance: self.descriptor.instance.clone(),
                status: status_for(&source, &runtime),
                installed: true,
                authenticated,
                version,
                version_supported,
                executable_path: Some(path.to_string_lossy().to_string()),
                source,
                runtime,
                diagnostics,
            })
        })
    }

    fn session_source(&self) -> &dyn SessionSource {
        &self.source
    }

    fn runtime(&self) -> Option<&dyn AgentRuntime> {
        Some(self.runtime.as_ref())
    }

    fn assets(&self) -> Option<&dyn AssetProvider> {
        Some(self)
    }

    fn models(&self) -> Option<&dyn ModelCatalogProvider> {
        Some(self.runtime.as_ref())
    }

    fn quota(&self) -> Option<&dyn QuotaProvider> {
        Some(self)
    }
}

impl AssetProvider for CodexEngine {
    fn list_assets(&self, query: FacetQuery) -> EngineFuture<'_, FacetPage> {
        Box::pin(async move {
            if !matches!(query.kind.as_deref(), None | Some("skill")) {
                return Ok(FacetPage {
                    items: Vec::new(),
                    next_cursor: None,
                });
            }
            let Some(cwd) = query.cwd.as_deref() else {
                return Ok(FacetPage {
                    items: Vec::new(),
                    next_cursor: None,
                });
            };
            let response = self.supervisor.request(
                "skills/list",
                json!({ "cwds": [cwd], "forceReload": false }),
            )?;
            let items = codex_skill_items(&response, cwd);
            let (items, next_cursor) = paginate_assets(items, query.cursor, query.limit);
            Ok(FacetPage { items, next_cursor })
        })
    }
}

fn codex_skill_items(response: &Value, cwd: &str) -> Vec<FacetItem> {
    response
        .get("data")
        .and_then(Value::as_array)
        .and_then(|data| {
            data.iter()
                .find(|entry| entry.get("cwd").and_then(Value::as_str) == Some(cwd))
        })
        .and_then(|entry| entry.get("skills"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|skill| {
            skill
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .filter_map(|skill| {
            let name = skill.get("name")?.as_str()?.to_string();
            let description = skill
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string);
            let path = skill
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let scope = skill
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("user")
                .to_string();
            Some(FacetItem {
                id: format!("skill:{name}:{path}"),
                display_name: name.clone(),
                description: description.clone(),
                data: json!({
                    "name": name,
                    "description": description.unwrap_or_default(),
                    "argumentHint": Value::Null,
                    "version": Value::Null,
                    "source": scope,
                    "scope": scope,
                    "path": path,
                }),
            })
        })
        .collect()
}

fn paginate_assets(
    items: Vec<FacetItem>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> (Vec<FacetItem>, Option<String>) {
    let start = cursor
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default()
        .min(items.len());
    let end = (start + limit.unwrap_or(items.len())).min(items.len());
    let next_cursor = (end < items.len()).then(|| end.to_string());
    (
        items.into_iter().skip(start).take(end - start).collect(),
        next_cursor,
    )
}

fn status_for(source: &CapabilityHealth, runtime: &CapabilityHealth) -> EngineHealthStatus {
    match (source.available, runtime.available) {
        (true, true) => EngineHealthStatus::Available,
        (true, false) | (false, true) => EngineHealthStatus::Degraded,
        (false, false) => EngineHealthStatus::Unavailable,
    }
}

fn missing_cli_runtime(diagnostics: &mut Vec<HealthDiagnostic>) -> CapabilityHealth {
    if let Some(path) = crate::codex_locator::desktop_bundle_path() {
        diagnostics.push(HealthDiagnostic {
            code: "desktopBundleDetected".into(),
            message: format!(
                "ChatGPT desktop includes a bundled Codex binary at {}, but it is not a standalone CLI. Install the Codex CLI for Monet interactive runtime features.",
                path.display()
            ),
        });
        CapabilityHealth::unavailable("engine.codex.desktopBundleOnly")
    } else {
        CapabilityHealth::unavailable("engine.codex.cliUnavailable")
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

    #[test]
    fn maps_enabled_codex_skills_with_scope_and_path() {
        let items = codex_skill_items(
            &json!({
                "data": [{
                    "cwd": "/workspace/app",
                    "skills": [
                        {
                            "name": "review",
                            "description": "Review changes",
                            "path": "/workspace/app/.agents/skills/review/SKILL.md",
                            "scope": "repo",
                            "enabled": true
                        },
                        {
                            "name": "disabled",
                            "path": "/home/alice/.agents/skills/disabled/SKILL.md",
                            "scope": "user",
                            "enabled": false
                        }
                    ]
                }]
            }),
            "/workspace/app",
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].display_name, "review");
        assert_eq!(items[0].data["scope"], "repo");
        assert_eq!(
            items[0].data["path"],
            "/workspace/app/.agents/skills/review/SKILL.md"
        );
    }
}
