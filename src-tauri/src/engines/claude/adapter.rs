use std::process::Command;
use std::sync::Arc;

use serde_json::Value;
use tauri::AppHandle;

use super::{default_instance, ClaudeRuntime, ClaudeSource};
use crate::engines::core::*;
use crate::proc_ext::HideConsole;

pub struct ClaudeEngine {
    descriptor: EngineDescriptor,
    source: ClaudeSource,
    runtime: Arc<ClaudeRuntime>,
}

impl ClaudeEngine {
    pub fn descriptor() -> EngineResult<EngineDescriptor> {
        let instance = default_instance()?;
        Ok(EngineDescriptor {
            instance,
            display_name: "Claude Code".into(),
            enabled: true,
            capabilities: EngineCapabilities {
                history: HistoryCapabilities {
                    pagination: HistoryPagination::Emulated,
                    change_delivery: ChangeDelivery::Watch,
                    search: true,
                    assets: true,
                },
                runtime: Some(RuntimeCapabilities {
                    create: true,
                    resume: true,
                    fork: true,
                    send_while_running: false,
                    interrupt: true,
                    streaming: StreamingCapabilities {
                        text: StreamGranularity::Delta,
                        reasoning: StreamGranularity::Delta,
                        tool_progress: StreamGranularity::Item,
                    },
                    model_catalog: ModelCatalogKind::Configured,
                    interactions: vec![
                        InteractionKind::Command,
                        InteractionKind::Question,
                        InteractionKind::Plan,
                    ],
                }),
                facets: FacetCapabilities {
                    assets: true,
                    automation: true,
                    configuration: true,
                    quota: true,
                    runtime_commands: true,
                },
            },
            ui: EngineUiIntegration {
                identity: UiIdentityMode::Native,
                session_surface: SessionSurface::Native,
                install_guide_url: Some("https://code.claude.com/docs/en/installation".into()),
            },
        })
    }

    pub fn new(app: AppHandle) -> EngineResult<Arc<Self>> {
        crate::turn_signal::start_listener_if_installed(app.clone());
        Ok(Arc::new(Self {
            descriptor: Self::descriptor()?,
            source: ClaudeSource::new()?,
            runtime: ClaudeRuntime::new(app)?,
        }))
    }
}

impl EngineAdapter for ClaudeEngine {
    fn descriptor(&self) -> EngineDescriptor {
        self.descriptor.clone()
    }

    fn configuration_path(&self) -> Option<std::path::PathBuf> {
        Some(crate::config::claude_settings_path())
    }

    fn health(&self) -> EngineFuture<'_, EngineHealth> {
        Box::pin(async move {
            let located = crate::claude_locator::locate_lightweight();
            let (status, installed, version, path, runtime, diagnostics) = match located {
                Ok(located) => {
                    let version = cli_version(&located.path);
                    (
                        EngineHealthStatus::Available,
                        true,
                        version,
                        Some(located.path.to_string_lossy().to_string()),
                        CapabilityHealth::available(),
                        Vec::new(),
                    )
                }
                Err(error) => (
                    EngineHealthStatus::Degraded,
                    false,
                    None,
                    None,
                    CapabilityHealth::unavailable("engine.claude.cliUnavailable"),
                    vec![HealthDiagnostic {
                        code: "cliNotFound".into(),
                        message: error,
                    }],
                ),
            };
            Ok(EngineHealth {
                instance: self.descriptor.instance.clone(),
                status,
                installed,
                authenticated: None,
                version,
                version_supported: installed.then_some(true),
                executable_path: path,
                // 历史源直接读取用户配置的数据根，不依赖 CLI 是否安装。
                source: CapabilityHealth::available(),
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

    fn quota(&self) -> Option<&dyn QuotaProvider> {
        Some(self)
    }

    fn runtime_commands(&self) -> Option<&dyn RuntimeCommandProvider> {
        Some(self)
    }

    fn models(&self) -> Option<&dyn ModelCatalogProvider> {
        Some(self)
    }

    fn notify_source_change(&self, change: SourceChange) {
        self.source.publish_change(change);
    }
}

impl AssetProvider for ClaudeEngine {
    fn list_assets(&self, query: FacetQuery) -> EngineFuture<'_, FacetPage> {
        Box::pin(async move {
            let assets = if let Some(cwd) = query.cwd.as_deref() {
                let cwd = cwd.to_string();
                tauri::async_runtime::spawn_blocking(move || {
                    crate::workshop::collect_composer_assets(&cwd)
                })
                .await
                .map_err(|error| internal_error(error.to_string()))?
            } else {
                crate::workshop::get_workshop_assets()
                    .await
                    .map_err(internal_error)?
            };
            let value =
                serde_json::to_value(assets).map_err(|error| internal_error(error.to_string()))?;
            let mut items = Vec::new();
            let kinds: &[&str] = match query.kind.as_deref() {
                Some("skill") => &["skills"],
                Some("command") => &["commands"],
                Some("agent") => &["agents"],
                Some("mcp") => &["mcpServers"],
                Some(_) => &[],
                None => &["skills", "commands", "agents", "mcpServers"],
            };
            for kind in kinds {
                let singular = kind.trim_end_matches('s').trim_end_matches("Server");
                for item in value
                    .get(kind)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Unnamed")
                        .to_string();
                    items.push(FacetItem {
                        id: format!("{singular}:{name}"),
                        display_name: name,
                        description: item
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        data: item.clone(),
                    });
                }
            }
            let (items, next_cursor) = paginate(items, query.cursor, query.limit);
            Ok(FacetPage { items, next_cursor })
        })
    }
}

impl QuotaProvider for ClaudeEngine {
    fn read_quota(&self, force_refresh: bool) -> EngineFuture<'_, Value> {
        Box::pin(async move {
            let quota =
                crate::quota::load_provider_quota("claude", force_refresh).ok_or_else(|| {
                    EngineError::new(EngineErrorKind::NotFound, "quota provider not found")
                })?;
            serde_json::to_value(quota).map_err(|error| internal_error(error.to_string()))
        })
    }
}

impl RuntimeCommandProvider for ClaudeEngine {
    fn list_commands(&self, session: SessionRef) -> EngineFuture<'_, Vec<FacetItem>> {
        Box::pin(async move {
            if session.engine() != &self.descriptor.instance {
                return Err(EngineError::new(
                    EngineErrorKind::NotFound,
                    "Claude engine does not own this session",
                ));
            }
            let cwd = crate::discovery::discover_all()
                .into_iter()
                .find_map(|project| {
                    project
                        .sessions
                        .iter()
                        .find(|candidate| candidate.id == session.native_id())
                        .map(|candidate| candidate.cwd.clone().unwrap_or(project.display_path))
                })
                .ok_or_else(|| {
                    EngineError::new(EngineErrorKind::NotFound, "session was not found")
                })?;
            crate::runners::runner_commands_list(cwd)
                .into_iter()
                .map(|command| {
                    let data = serde_json::to_value(&command)
                        .map_err(|error| internal_error(error.to_string()))?;
                    Ok(FacetItem {
                        id: command.id,
                        display_name: command.alias.unwrap_or(command.cmd),
                        description: command.note,
                        data,
                    })
                })
                .collect()
        })
    }
}

impl ModelCatalogProvider for ClaudeEngine {
    fn list_models(&self) -> EngineFuture<'_, Vec<ModelDescriptor>> {
        Box::pin(async {
            Ok(["fable", "opus", "sonnet", "haiku"]
                .into_iter()
                .map(|model| ModelDescriptor {
                    id: model.to_string(),
                    model: model.to_string(),
                    display_name: model.to_string(),
                    description: None,
                    is_default: model == "sonnet",
                    hidden: false,
                    default_effort: None,
                    efforts: Vec::new(),
                    default_service_tier: None,
                    service_tiers: Vec::new(),
                })
                .collect())
        })
    }
}

fn paginate<T>(
    items: Vec<T>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> (Vec<T>, Option<String>) {
    let start = cursor
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = limit.unwrap_or(100).clamp(1, 500);
    let end = (start + limit).min(items.len());
    let next = (end < items.len()).then(|| end.to_string());
    (
        items
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect(),
        next,
    )
}

fn internal_error(message: impl ToString) -> EngineError {
    EngineError::new(EngineErrorKind::Internal, message.to_string())
}

fn cli_version(path: &std::path::Path) -> Option<String> {
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
