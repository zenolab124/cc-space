use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde_json::Value;

use crate::config;
use crate::proc_ext::HideConsole;

static SCHEMA_CACHE: Mutex<Option<Value>> = Mutex::new(None);

const SCHEMA_URL: &str = "https://json.schemastore.org/claude-code-settings.json";

fn data_dir() -> PathBuf {
    config::data_dir().to_path_buf()
}

fn schema_cache_path() -> PathBuf {
    data_dir().join("settings-schema.json")
}

fn claude_settings_path() -> PathBuf {
    config::claude_root().join("settings.json")
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

/// 读取 schema：内存缓存 → 磁盘缓存 → 远程 fetch
fn load_schema() -> Option<Value> {
    {
        let guard = SCHEMA_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref s) = *guard {
            return Some(s.clone());
        }
    }

    // 磁盘缓存
    if let Ok(content) = fs::read_to_string(schema_cache_path()) {
        if let Ok(val) = serde_json::from_str::<Value>(&content) {
            let mut guard = SCHEMA_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(val.clone());
            return Some(val);
        }
    }

    None
}

/// 后台 fetch 远程 schema 并写入磁盘+内存缓存
fn fetch_and_cache_schema() {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let Ok(client) = client else { return };

    let Ok(resp) = client.get(SCHEMA_URL).send() else {
        return;
    };
    let Ok(text) = resp.text() else { return };

    if let Ok(val) = serde_json::from_str::<Value>(&text) {
        let dir = data_dir();
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(schema_cache_path(), &text);

        let mut guard = SCHEMA_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(val);
    }
}

/// 返回 schema 的 properties 部分（每个字段的 type/description/enum/default）
/// + 当前 settings.json 的完整值
#[tauri::command]
pub fn get_settings_schema() -> Value {
    let schema = load_schema();

    let properties = schema
        .as_ref()
        .and_then(|s| s.get("properties"))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    let defs = schema
        .as_ref()
        .and_then(|s| s.get("$defs"))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    serde_json::json!({
        "properties": properties,
        "$defs": defs,
        "hasSchema": schema.is_some(),
    })
}

/// 返回 settings.json 完整内容（原始 JSON）
#[tauri::command]
pub fn get_full_cli_settings() -> Value {
    let path = claude_settings_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Object(serde_json::Map::new()))
}

/// 更新 settings.json 中的指定字段（合并写入）
#[tauri::command]
pub fn update_cli_settings(updates: HashMap<String, Value>) -> Result<(), String> {
    let path = claude_settings_path();

    let mut current: serde_json::Map<String, Value> = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    for (key, value) in updates {
        if value.is_null() {
            current.remove(&key);
        } else {
            current.insert(key, value);
        }
    }

    let json_str = serde_json::to_string_pretty(&Value::Object(current))
        .map_err(|e| format!("序列化失败: {}", e))?;

    fs::write(&path, json_str).map_err(|e| format!("写入失败: {}", e))
}

/// 后台刷新 schema（非阻塞）
#[tauri::command]
pub fn refresh_settings_schema() {
    std::thread::spawn(fetch_and_cache_schema);
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

/// 定位 claude CLI 的真实二进制路径（解析 symlink）。
/// locator 拿到的是入口路径（可能是软链），这里 canonicalize 到真实文件；
/// versions 目录扫描保底——claude 装过但不在任何已知位置/PATH 时仍可读到二进制
pub fn find_claude_binary() -> Option<PathBuf> {
    if let Ok(located) = crate::claude_locator::locate() {
        return Some(fs::canonicalize(&located.path).unwrap_or(located.path));
    }
    let home = dirs::home_dir()?;
    let versions_dir = home.join(".local/share/claude/versions");
    if versions_dir.is_dir() {
        let mut versions: Vec<_> = fs::read_dir(&versions_dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_file()))
            .collect();
        versions.sort_by_key(|e| e.file_name());
        if let Some(latest) = versions.last() {
            return Some(latest.path());
        }
    }
    None
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

/// 从 CLI 二进制中提取字段默认值（Python 读二进制 + 正则模式匹配，不经 Agent）
/// 返回 JSON 字符串：[{key, default, confidence}]
pub fn extract_defaults_from_binary(fields_json: &str) -> Result<String, String> {
    let binary = find_claude_binary()
        .ok_or_else(|| "找不到 Claude CLI 二进制".to_string())?;
    eprintln!("[extract_defaults] binary: {}", binary.display());

    let script = r#"
import sys, json, re

binary_path = sys.argv[1]
fields = json.loads(sys.argv[2])

with open(binary_path, 'rb') as f:
    data = f.read()

code_markers = [b'??', b'===', b'!==', b'return', b'void 0']

results = []
for field in fields:
    key = field['key']
    needle = key.encode('utf-8')
    snippets = []
    pos = 0
    while len(snippets) < 5:
        idx = data.find(needle, pos)
        if idx == -1:
            break
        start = max(0, idx - 200)
        end = min(len(data), idx + len(needle) + 200)
        chunk = data[start:end]
        if any(m in chunk for m in code_markers):
            snippets.append(chunk.decode('utf-8', errors='replace'))
        pos = idx + 1

    default = None
    confidence = 'low'
    ek = re.escape(key)

    for ctx in snippets:
        if re.search(ek + r'===!1\)return!1;return!0', ctx):
            default, confidence = True, 'high'; break
        if re.search(ek + r'===!0[^0-9]', ctx):
            default, confidence = False, 'high'; break
        m = re.search(ek + r'\?\?(!0|true)', ctx)
        if m:
            default, confidence = True, 'high'; break
        m = re.search(ek + r'\?\?(!1|false)', ctx)
        if m:
            default, confidence = False, 'high'; break
        m = re.search(ek + r'\?\?"([^"]+)"', ctx)
        if m:
            default, confidence = m.group(1), 'high'; break
        m = re.search(ek + r'\?\?(\d+)', ctx)
        if m:
            default, confidence = int(m.group(1)), 'high'; break
        m = re.search(ek + r'!==void 0\)return[^;]+;return(!0|!1|true|false)', ctx)
        if m:
            val = m.group(1)
            default, confidence = val in ('!0', 'true'), 'high'; break
        if re.search(ek + r'!==!1', ctx):
            default, confidence = True, 'medium'

    results.append({'key': key, 'default': default, 'confidence': confidence})

print(json.dumps(results))
"#;

    let output = Command::new("python3")
        .hide_console()
        .args(["-c", script, &binary.display().to_string(), fields_json])
        .output()
        .map_err(|e| format!("python3 执行失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("python3 错误: {}", stderr.chars().take(300).collect::<String>()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(stdout)
}
