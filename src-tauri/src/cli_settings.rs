use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config;

fn claude_settings_path() -> PathBuf {
    config::claude_settings_path()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct CliSettings {
    pub model: Option<String>,
    pub effort_level: Option<String>,
    pub ultracode: bool,
    pub permission_mode: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CliSettingsLayer {
    model: Option<String>,
    effort_level: Option<String>,
    ultracode: Option<bool>,
    permission_mode: Option<String>,
}

fn read_settings_layer(path: &Path) -> CliSettingsLayer {
    let json: Option<Value> = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    CliSettingsLayer {
        model: json
            .as_ref()
            .and_then(|value| value.get("model"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        effort_level: json
            .as_ref()
            .and_then(|value| value.get("effortLevel"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        ultracode: json
            .as_ref()
            .and_then(|value| value.get("ultracode"))
            .and_then(Value::as_bool),
        permission_mode: json
            .as_ref()
            .and_then(|value| value.get("permissions"))
            .and_then(|value| value.get("defaultMode"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn merge_settings_layers(layers: impl IntoIterator<Item = CliSettingsLayer>) -> CliSettings {
    let mut merged = CliSettingsLayer::default();
    for layer in layers {
        if layer.model.is_some() {
            merged.model = layer.model;
        }
        if layer.effort_level.is_some() {
            merged.effort_level = layer.effort_level;
        }
        if layer.ultracode.is_some() {
            merged.ultracode = layer.ultracode;
        }
        if layer.permission_mode.is_some() {
            merged.permission_mode = layer.permission_mode;
        }
    }
    CliSettings {
        model: merged.model,
        effort_level: merged.effort_level,
        ultracode: merged.ultracode.unwrap_or(false),
        permission_mode: merged.permission_mode,
    }
}

fn settings_candidates_for_roots(
    user_settings: PathBuf,
    cwd: &Path,
    roots: crate::git_utils::SettingsRoots,
) -> Vec<PathBuf> {
    let mut paths = vec![
        user_settings,
        roots.project.join(".claude/settings.json"),
    ];
    let repository_local = roots.local.join(".claude/settings.local.json");
    let legacy_local = cwd.join(".claude/settings.local.json");
    if legacy_local != repository_local {
        paths.push(legacy_local);
    }
    paths.push(repository_local);
    paths
}

fn settings_candidates(cwd: Option<&Path>) -> Vec<PathBuf> {
    let user_settings = claude_settings_path();
    let Some(cwd) = cwd else {
        return vec![user_settings];
    };
    settings_candidates_for_roots(
        user_settings,
        cwd,
        crate::git_utils::settings_roots(cwd),
    )
}

pub fn get_cli_settings(cwd: Option<&Path>) -> CliSettings {
    merge_settings_layers(
        settings_candidates(cwd)
            .into_iter()
            .map(|path| read_settings_layer(&path)),
    )
}

#[cfg(test)]
mod settings_summary_tests {
    use super::{
        merge_settings_layers, read_settings_layer, settings_candidates_for_roots, CliSettings,
        CliSettingsLayer,
    };
    use crate::git_utils::SettingsRoots;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempSettingsFile {
        path: PathBuf,
        dir: PathBuf,
    }

    impl TempSettingsFile {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempSettingsFile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn temp_file(name: &str, content: &str) -> TempSettingsFile {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("monet-cli-settings-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        TempSettingsFile { path, dir }
    }

    #[test]
    fn orders_user_project_legacy_and_repository_local() {
        let paths = settings_candidates_for_roots(
            PathBuf::from("/home/me/.claude/settings.json"),
            std::path::Path::new("/repo/main/packages/app"),
            SettingsRoots {
                project: PathBuf::from("/repo/worktree"),
                local: PathBuf::from("/repo/main"),
            },
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/home/me/.claude/settings.json"),
                PathBuf::from("/repo/worktree/.claude/settings.json"),
                PathBuf::from("/repo/main/packages/app/.claude/settings.local.json"),
                PathBuf::from("/repo/main/.claude/settings.local.json"),
            ]
        );
    }

    #[test]
    fn omits_duplicate_legacy_local_candidate() {
        let paths = settings_candidates_for_roots(
            PathBuf::from("/home/me/.claude/settings.json"),
            std::path::Path::new("/repo/main"),
            SettingsRoots {
                project: PathBuf::from("/repo/main"),
                local: PathBuf::from("/repo/main"),
            },
        );
        assert_eq!(paths.len(), 3);
        assert_eq!(
            paths[2],
            PathBuf::from("/repo/main/.claude/settings.local.json")
        );
    }

    #[test]
    fn merges_each_scalar_by_layer_priority() {
        let merged = merge_settings_layers([
            CliSettingsLayer {
                model: Some("user-model".into()),
                effort_level: Some("medium".into()),
                ultracode: Some(true),
                permission_mode: Some("default".into()),
            },
            CliSettingsLayer {
                model: Some("project-model".into()),
                permission_mode: Some("plan".into()),
                ..Default::default()
            },
            CliSettingsLayer {
                effort_level: Some("high".into()),
                ultracode: Some(false),
                ..Default::default()
            },
        ]);
        assert_eq!(
            merged,
            CliSettings {
                model: Some("project-model".into()),
                effort_level: Some("high".into()),
                ultracode: false,
                permission_mode: Some("plan".into()),
            }
        );
    }

    #[test]
    fn invalid_json_and_wrong_types_leave_fields_unset() {
        let invalid = temp_file("invalid.json", "{oops");
        assert_eq!(read_settings_layer(invalid.path()), CliSettingsLayer::default());

        let wrong = temp_file(
            "wrong.json",
            r#"{"model":3,"effortLevel":false,"ultracode":"yes","permissions":{"defaultMode":[]}}"#,
        );
        assert_eq!(read_settings_layer(wrong.path()), CliSettingsLayer::default());
    }

    #[test]
    fn reads_supported_fields_only() {
        let path = temp_file(
            "settings.json",
            r#"{"model":"opus","effortLevel":"max","ultracode":true,"permissions":{"defaultMode":"dontAsk","allow":["Bash"]},"env":{"SECRET":"ignored"}}"#,
        );
        assert_eq!(
            read_settings_layer(path.path()),
            CliSettingsLayer {
                model: Some("opus".into()),
                effort_level: Some("max".into()),
                ultracode: Some(true),
                permission_mode: Some("dontAsk".into()),
            }
        );
    }
}

// ---------------------------------------------------------------------------
// MCP Server registration
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn is_codesigned(path: &std::path::Path) -> bool {
    std::process::Command::new("codesign")
        .args(["--verify", path.to_string_lossy().as_ref()])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(not(target_os = "macos"))]
fn is_codesigned(_path: &std::path::Path) -> bool {
    true
}

fn mcp_bin_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "monet-mcp.exe"
    } else {
        "monet-mcp"
    }
}

fn installed_mcp_path() -> PathBuf {
    config::data_dir().join("bin").join(mcp_bin_name())
}

fn bundled_mcp_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(mcp_bin_name());
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(mcp_bin_name())
}

fn install_mcp_binary() -> Result<PathBuf, String> {
    let source = bundled_mcp_path();
    let target = installed_mcp_path();

    if !source.exists() {
        return Err(format!("MCP binary not found at: {}", source.display()));
    }

    if let Some(parent) = target.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let needs_install = if target.exists() {
        let src_meta = fs::metadata(&source).map_err(|e| e.to_string())?;
        let dst_meta = fs::metadata(&target).map_err(|e| e.to_string())?;
        src_meta.len() != dst_meta.len() || !is_codesigned(&target)
    } else {
        true
    };

    if needs_install {
        fs::copy(&source, &target).map_err(|e| format!("install failed: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
        }
        #[cfg(target_os = "macos")]
        crate::signing::sign(&target, "io.github.zenolab124.monet.mcp");
    }

    Ok(target)
}

/// 启动自愈：MCP 已注册则同步安装的二进制到最新版并收敛签名形态。
/// 更名迁移：旧 "cc-space" 条目存在时自动执行完整 register_mcp（装新二进制 + 写新条目 + 清旧条目）
pub fn startup_sync_mcp() {
    std::thread::spawn(|| {
        let parsed: serde_json::Value = match fs::read_to_string(claude_settings_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => return,
        };
        let servers = parsed
            .get("mcpServers")
            .and_then(serde_json::Value::as_object);
        let has_monet = servers.is_some_and(|s| s.contains_key("monet"));
        let has_legacy = servers
            .and_then(|s| s.get("cc-space"))
            .and_then(|v| v.get("command"))
            .and_then(|c| c.as_str())
            .is_some_and(|cmd| cmd.contains("cc-space"));

        if has_legacy && !has_monet {
            // 旧条目在、新条目不在：执行完整注册（含清扫旧条目）
            if let Err(e) = register_mcp_inner() {
                log::warn!("MCP legacy→monet migration failed: {e}");
            }
        } else if has_monet {
            if let Err(e) = install_mcp_binary() {
                log::warn!("MCP binary startup sync failed: {e}");
            }
        }
    });
}

/// register_mcp 的核心逻辑，startup_sync 和手动注册共用
fn register_mcp_inner() -> Result<(), String> {
    let mcp_path = install_mcp_binary()?;
    let settings_path = claude_settings_path();

    let mut settings: serde_json::Map<String, serde_json::Value> =
        fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

    let mcp_servers = settings
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("mcpServers is not an object")?;

    let mut server_config = serde_json::Map::new();
    server_config.insert(
        "command".to_string(),
        serde_json::Value::String(mcp_path.to_string_lossy().to_string()),
    );
    server_config.insert("args".to_string(), serde_json::json!([]));

    if let Ok(dir) = std::env::var("MONET_DATA_DIR") {
        let mut env = serde_json::Map::new();
        env.insert(
            "MONET_DATA_DIR".to_string(),
            serde_json::Value::String(dir),
        );
        server_config.insert("env".to_string(), serde_json::Value::Object(env));
    }

    mcp_servers.insert(
        "monet".to_string(),
        serde_json::Value::Object(server_config),
    );

    let stale_legacy = mcp_servers
        .get("cc-space")
        .and_then(|v| v.get("command"))
        .and_then(|c| c.as_str())
        .is_some_and(|cmd| cmd.contains("cc-space"));
    if stale_legacy {
        mcp_servers.remove("cc-space");
        log::info!("removed stale legacy MCP registration 'cc-space' (superseded by 'monet')");
    }

    let json_str = serde_json::to_string_pretty(&serde_json::Value::Object(settings))
        .map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(&settings_path, json_str).map_err(|e| format!("写入失败: {}", e))
}

#[tauri::command]
pub fn get_mcp_status() -> serde_json::Value {
    let path = claude_settings_path();
    let settings: serde_json::Map<String, serde_json::Value> = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let registered = settings
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|servers| servers.contains_key("monet"));

    serde_json::json!({ "registered": registered })
}

#[tauri::command]
pub fn register_mcp() -> Result<(), String> {
    register_mcp_inner()
}

#[tauri::command]
pub fn unregister_mcp() -> Result<(), String> {
    let settings_path = claude_settings_path();

    let mut settings: serde_json::Map<String, serde_json::Value> =
        fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

    if let Some(servers) = settings
        .get_mut("mcpServers")
        .and_then(serde_json::Value::as_object_mut)
    {
        servers.remove("monet");
    }

    let json_str = serde_json::to_string_pretty(&serde_json::Value::Object(settings))
        .map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(&settings_path, json_str).map_err(|e| format!("写入失败: {}", e))
}

// ---------------------------------------------------------------------------
// Claude CLI 路径设置（设置页消费）
// ---------------------------------------------------------------------------

// 三个 command 一律 async + spawn_blocking：探测失败路径要跑 login shell
// （5s 超时），同步 command 会在主线程执行、冻结整个 UI

#[tauri::command]
pub async fn get_claude_binary_info() -> Result<crate::claude_locator::LocateInfo, String> {
    tauri::async_runtime::spawn_blocking(crate::claude_locator::current_info)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_claude_binary_path(
    path: Option<String>,
) -> Result<crate::claude_locator::LocateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::claude_locator::set_manual_path(path.as_deref())?;
        Ok(crate::claude_locator::current_info())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn redetect_claude_binary() -> Result<crate::claude_locator::LocateInfo, String> {
    tauri::async_runtime::spawn_blocking(crate::claude_locator::redetect_info)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Claude 数据根目录设置（设置页消费）
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeRootInfo {
    /// 当前进程实际生效的根路径（OnceLock 缓存值）
    pub effective: String,
    /// 设置项 claudeRoot 原值（未配置为 None）
    pub configured: Option<String>,
    /// 重启后将生效的路径来源：env / settings / default
    pub source: String,
    /// 默认路径 ~/.claude（供 UI 展示与「恢复默认」）
    pub default: String,
    /// 生效路径当前是否存在
    pub exists: bool,
    /// 即时解析结果与当前生效值不同 → 需重启才生效
    pub restart_required: bool,
}

fn claude_root_info() -> ClaudeRootInfo {
    let effective = config::claude_root();
    let resolved = config::resolve_claude_root();
    let configured = config::read_app_setting("claudeRoot")
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|s| !s.trim().is_empty());
    let source = if std::env::var("MONET_CLAUDE_ROOT").is_ok_and(|v| !v.trim().is_empty())
        || std::env::var("CLAUDE_CONFIG_DIR").is_ok_and(|v| !v.trim().is_empty())
    {
        "env"
    } else if configured.is_some() {
        "settings"
    } else {
        "default"
    };
    ClaudeRootInfo {
        effective: effective.display().to_string(),
        configured,
        source: source.to_string(),
        default: config::default_claude_root().display().to_string(),
        exists: effective.is_dir(),
        restart_required: *effective != resolved,
    }
}

#[tauri::command]
pub fn get_claude_root_info() -> ClaudeRootInfo {
    claude_root_info()
}

/// 设置自定义 Claude 数据根目录；None / 空串 = 恢复默认（删除设置键）。
/// 只写设置不动运行态——watcher/索引启动时绑定，统一重启生效
#[tauri::command]
pub fn set_claude_root(path: Option<String>) -> Result<ClaudeRootInfo, String> {
    let trimmed = path.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(p) => {
            let expanded = if let Some(rest) = p.strip_prefix("~/") {
                dirs::home_dir().unwrap_or_default().join(rest)
            } else {
                PathBuf::from(p)
            };
            if !expanded.is_dir() {
                return Err(format!("目录不存在：{}", expanded.display()));
            }
            config::write_app_setting("claudeRoot", serde_json::json!(p));
        }
        None => config::write_app_setting("claudeRoot", serde_json::Value::Null),
    }
    Ok(claude_root_info())
}
