//! Claude Code 本地环境检查：版本对比 / 一键升级 / 多安装冲突诊断。
//! 只服务 Claude Code 一家（本产品定位），不做多 CLI 泛化。
//!
//! spawn 铁律遵守：claude 用 claude_locator 的绝对路径；npm 用 claude 同目录的
//! sibling 绝对路径（保证写进正确的 node 树）；brew 等其余命令注入 enhanced_path()。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::claude_locator;
use crate::proc_ext::HideConsole;
use crate::streaming;

const NPM_LATEST_URL: &str = "https://registry.npmjs.org/@anthropic-ai/claude-code/latest";
const LATEST_CACHE_TTL: Duration = Duration::from_secs(3600);
/// 升级输出保留的末尾长度（错误信息几乎总在末尾）
const OUTPUT_TAIL: usize = 2000;

static LATEST_CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// 检查
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEnvInfo {
    pub installed_version: Option<String>,
    /// npm registry 最新版（1h 缓存；离线/超时为 None，前端降级只显示本地版本）
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub binary_path: Option<String>,
    /// official（官方安装器/native）| npm | homebrew | unknown
    pub install_method: String,
}

fn emit_install_progress(app: &AppHandle, phase: &str) {
    let _ = app.emit(
        "cli-install-progress",
        serde_json::json!({ "engine": "claude", "phase": phase }),
    );
}

/// 从命令输出提取首个 semver（"2.1.199 (Claude Code)" → "2.1.199"）
fn parse_semver(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|t| t.trim_start_matches('v'))
        .find(|t| {
            let parts: Vec<&str> = t.split('.').collect();
            parts.len() >= 3 && parts.iter().take(3).all(|p| {
                !p.is_empty() && p.chars().take_while(|c| c.is_ascii_digit()).count() > 0
                    && p.chars().all(|c| c.is_ascii_digit() || c == '-' || c.is_ascii_alphanumeric())
            }) && parts[0].chars().all(|c| c.is_ascii_digit())
        })
        .map(|s| s.to_string())
}

/// 语义化比较：仅当 latest 的数字三段严格大于 installed 才算可升级
fn semver_gt(latest: &str, installed: &str) -> bool {
    let nums = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .take(3)
            .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .map(|s| s.parse().unwrap_or(0))
            .collect()
    };
    nums(latest) > nums(installed)
}

fn run_version(bin: &Path) -> Option<String> {
    let output = Command::new(bin)
        .arg("--version")
        .env("PATH", streaming::enhanced_path())
        .hide_console()
        .output()
        .ok()?;
    parse_semver(&String::from_utf8_lossy(&output.stdout))
        .or_else(|| parse_semver(&String::from_utf8_lossy(&output.stderr)))
}

/// 按路径特征推断安装方式
fn detect_method(path: &str) -> &'static str {
    let p = path.replace('\\', "/");
    if p.contains("/.claude/local/") || p.contains("/.local/bin/") || p.contains("/.local/state/claude/") {
        "official"
    } else if p.contains("/homebrew/") || p.contains("/Cellar/") || p.contains("/usr/local/Caskroom/") {
        "homebrew"
    } else if p.contains("/node_modules/")
        || p.contains("/.nvm/")
        || p.contains("/.fnm/")
        || p.contains("/.volta/")
        || p.contains("/nodejs/")
        || p.contains("/npm/")
    {
        "npm"
    } else {
        "unknown"
    }
}

fn fetch_latest_version() -> Option<String> {
    {
        let guard = LATEST_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, v)) = guard.as_ref() {
            if at.elapsed() < LATEST_CACHE_TTL {
                return Some(v.clone());
            }
        }
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let resp: serde_json::Value = client.get(NPM_LATEST_URL).send().ok()?.json().ok()?;
    let version = resp.get("version")?.as_str()?.to_string();
    let mut guard = LATEST_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((Instant::now(), version.clone()));
    Some(version)
}

fn claude_env_check_sync() -> ClaudeEnvInfo {
    let located = claude_locator::locate().ok();
    let binary_path = located.as_ref().map(|l| l.path.to_string_lossy().to_string());
    let installed_version = located.as_ref().and_then(|l| run_version(&l.path));
    let latest_version = fetch_latest_version();
    let update_available = matches!(
        (&installed_version, &latest_version),
        (Some(i), Some(l)) if semver_gt(l, i)
    );
    let install_method = binary_path.as_deref().map(detect_method).unwrap_or("unknown").to_string();
    ClaudeEnvInfo {
        installed_version,
        latest_version,
        update_available,
        binary_path,
        install_method,
    }
}

#[tauri::command]
pub async fn claude_env_check() -> Result<ClaudeEnvInfo, String> {
    tauri::async_runtime::spawn_blocking(claude_env_check_sync)
        .await
        .map_err(|error| format!("Claude 环境检查线程异常退出: {error}"))
}

// ---------------------------------------------------------------------------
// 升级
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeResult {
    pub success: bool,
    /// 升级后重测的版本（成功与否都测，"版本未变"也是有效结论）
    pub new_version: Option<String>,
    /// 执行的命令描述（供 UI 展示与用户复现）
    pub command: String,
    /// 输出末尾（错误信息几乎总在末尾）
    pub output_tail: String,
    pub binary_path: Option<String>,
}

fn tail(s: &str) -> String {
    let t = s.trim();
    if t.len() <= OUTPUT_TAIL {
        return t.to_string();
    }
    // 按字符边界截尾，避免多字节截断 panic
    let start = t.len() - OUTPUT_TAIL;
    let boundary = (start..t.len()).find(|&i| t.is_char_boundary(i)).unwrap_or(start);
    format!("…{}", &t[boundary..])
}

/// 找 claude 同 bin 目录的 npm（nvm/fnm/volta 场景保证升级写进同一 node 树）
fn sibling_npm(claude_path: &Path) -> Option<PathBuf> {
    let dir = claude_path.parent()?;
    let npm = dir.join("npm");
    npm.is_file().then_some(npm)
}

fn claude_env_upgrade_sync() -> Result<UpgradeResult, String> {
    let located = claude_locator::locate().map_err(|e| format!("未定位到 claude: {e}"))?;
    let method = detect_method(&located.path.to_string_lossy());

    let (mut cmd, desc): (Command, String) = match method {
        "npm" => {
            if let Some(npm) = sibling_npm(&located.path) {
                let mut c = Command::new(&npm);
                c.hide_console();
                c.args(["install", "-g", "@anthropic-ai/claude-code@latest"]);
                c.env("PATH", streaming::enhanced_path());
                (c, format!("{} install -g @anthropic-ai/claude-code@latest", npm.display()))
            } else {
                // 同目录无 npm：登录 shell 里的 npm 兜底
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
                let mut c = Command::new(shell);
                c.hide_console();
                c.args(["-l", "-c", "npm install -g @anthropic-ai/claude-code@latest"]);
                (c, "npm install -g @anthropic-ai/claude-code@latest".to_string())
            }
        }
        "homebrew" => {
            let mut c = Command::new("brew");
            c.args(["upgrade", "--cask", "claude-code"]);
            c.env("PATH", streaming::enhanced_path());
            // 已知坑：API 源安装偶发失败
            c.env("HOMEBREW_NO_INSTALL_FROM_API", "1");
            (c, "brew upgrade --cask claude-code".to_string())
        }
        // official 与 unknown 都走 CLI 内置自更新——官方路径，适配面最广
        _ => {
            let mut c = Command::new(&located.path);
            c.hide_console();
            c.arg("update");
            c.env("PATH", streaming::enhanced_path());
            (c, format!("{} update", located.path.display()))
        }
    };

    let output = cmd.output().map_err(|e| format!("升级命令启动失败: {e}"))?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // 升级后重测版本（缓存的 latest 不动，本地版本必须现测）
    let new_version = run_version(&located.path);

    Ok(UpgradeResult {
        success: output.status.success(),
        new_version,
        command: desc,
        output_tail: tail(&combined),
        binary_path: Some(located.path.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub async fn claude_env_upgrade() -> Result<UpgradeResult, String> {
    tauri::async_runtime::spawn_blocking(claude_env_upgrade_sync)
        .await
        .map_err(|error| format!("Claude Code 升级线程异常退出: {error}"))?
}

// ---------------------------------------------------------------------------
// 安装
// ---------------------------------------------------------------------------

/// 一键安装 Claude Code（官方安装脚本）。安装器双平台落点均为 ~/.local/bin，
/// locator 候选已覆盖；完成后清缓存重探测，"探测到 + 版本可测"才算成功
/// （脚本 exit 0 但没装上的情况按失败报）。
fn claude_env_install_sync(app: AppHandle) -> Result<UpgradeResult, String> {
    emit_install_progress(&app, "installing");
    #[cfg(windows)]
    let (mut cmd, desc): (Command, String) = {
        let mut c = Command::new("powershell");
        c.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://claude.ai/install.ps1 | iex",
        ]);
        c.hide_console();
        (c, "irm https://claude.ai/install.ps1 | iex".to_string())
    };
    #[cfg(not(windows))]
    let (mut cmd, desc): (Command, String) = {
        let mut c = Command::new("/bin/bash");
        c.args(["-c", "curl -fsSL https://claude.ai/install.sh | bash"]);
        c.env("PATH", streaming::enhanced_path());
        (c, "curl -fsSL https://claude.ai/install.sh | bash".to_string())
    };

    let output = match cmd.output() {
        Ok(output) => output,
        Err(error) => {
            emit_install_progress(&app, "failed");
            return Err(format!("安装命令启动失败: {error}"));
        }
    };
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    emit_install_progress(&app, "verifying");

    // 清缓存重探测（安装前的失败负缓存必须失效），再现测版本
    let info = claude_locator::redetect_info();
    let new_version = info.path.as_deref().and_then(|p| run_version(Path::new(p)));
    let success = output.status.success() && new_version.is_some();
    emit_install_progress(&app, if success { "completed" } else { "failed" });

    Ok(UpgradeResult {
        success,
        new_version,
        command: desc,
        output_tail: tail(&combined),
        binary_path: info.path,
    })
}

#[tauri::command]
pub async fn claude_env_install(app: AppHandle) -> Result<UpgradeResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env_install_sync(app))
        .await
        .map_err(|error| format!("Claude Code 安装线程异常退出: {error}"))?
}

// ---------------------------------------------------------------------------
// 冲突诊断
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiagEntry {
    pub path: String,
    pub version: Option<String>,
    pub method: String,
    /// 是否 locator 当前生效的那个
    pub is_default: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiagReport {
    pub entries: Vec<DiagEntry>,
    /// 去重后仍有多个安装 → 提示冲突风险
    pub multiple: bool,
}

/// 登录 shell 跑 `which -a claude` 拿用户完整 PATH 视角下的所有安装
fn which_all() -> Vec<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let Ok(output) = Command::new(shell).hide_console().args(["-l", "-i", "-c", "which -a claude"]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

#[tauri::command]
pub fn claude_env_diagnose() -> DiagReport {
    let default_path = claude_locator::locate().ok().map(|l| l.path);
    let default_canon = default_path.as_ref().and_then(|p| p.canonicalize().ok());

    let mut candidates = which_all();
    if let Some(p) = &default_path {
        candidates.push(p.clone());
    }

    // canonicalize 去重（nvm 等场景 claude 是 symlink，多个入口可能指向同一实体）
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for path in candidates {
        let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(canon.clone()) {
            continue;
        }
        let is_default = default_canon.as_ref() == Some(&canon)
            || default_path.as_ref() == Some(&path);
        entries.push(DiagEntry {
            version: run_version(&path),
            method: detect_method(&path.to_string_lossy()).to_string(),
            path: path.to_string_lossy().to_string(),
            is_default,
        });
    }
    // 生效的排最前
    entries.sort_by_key(|e| !e.is_default);
    let multiple = entries.len() > 1;
    DiagReport { entries, multiple }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parse_and_compare() {
        assert_eq!(parse_semver("2.1.199 (Claude Code)").as_deref(), Some("2.1.199"));
        assert_eq!(parse_semver("v0.25.0").as_deref(), Some("0.25.0"));
        assert_eq!(parse_semver("Claude Code CLI"), None);
        assert!(semver_gt("2.1.201", "2.1.199"));
        assert!(semver_gt("2.2.0", "2.1.999"));
        assert!(!semver_gt("2.1.199", "2.1.199"));
        assert!(!semver_gt("2.1.198", "2.1.199"));
    }

    #[test]
    fn method_detection_by_path() {
        assert_eq!(detect_method("/Users/x/.local/bin/claude"), "official");
        assert_eq!(detect_method("/Users/x/.claude/local/claude"), "official");
        assert_eq!(detect_method("/opt/homebrew/bin/claude"), "homebrew");
        assert_eq!(detect_method("/Users/x/.nvm/versions/node/v22.1.0/bin/claude"), "npm");
        assert_eq!(detect_method("/usr/local/lib/node_modules/.bin/claude"), "npm");
        assert_eq!(detect_method("/opt/weird/claude"), "unknown");
    }

    #[test]
    fn output_tail_respects_char_boundary() {
        let long = format!("{}中文结尾", "a".repeat(OUTPUT_TAIL + 50));
        let t = tail(&long);
        assert!(t.starts_with('…'));
        assert!(t.ends_with("中文结尾"));
    }
}
