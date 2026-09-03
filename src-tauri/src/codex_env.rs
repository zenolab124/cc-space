//! Codex CLI 本地环境检查与一键安装。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::codex_locator;
use crate::proc_ext::HideConsole;
use crate::streaming;

const INSTALL_SCRIPT_URL: &str = "https://chatgpt.com/codex/install.sh";
const LATEST_RELEASE_URL: &str = "https://releases.openai.com/codex/channels/latest";
const LATEST_CACHE_TTL: Duration = Duration::from_secs(3600);
const OUTPUT_TAIL: usize = 2000;

static LATEST_CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvInfo {
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub binary_path: Option<String>,
    pub desktop_version: Option<String>,
    pub version_mismatch: bool,
    pub active_runtime_source: codex_locator::CodexRuntimeSource,
    pub configured_runtime_source: codex_locator::CodexRuntimeSource,
    pub active_runtime_version: Option<String>,
    pub runtime_restart_required: bool,
    pub runtime_selection_suggested: bool,
    pub cache_version: Option<String>,
    pub cache_version_mismatch: bool,
    pub computer_use: Option<ComputerUseEnvInfo>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ComputerUseEnvStatus {
    Ready,
    Unavailable,
    NeedsSetup,
    NeedsRefresh,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseEnvInfo {
    pub status: ComputerUseEnvStatus,
    pub plugin_version: Option<String>,
    pub helper_version: Option<String>,
}

fn emit_install_progress(app: &AppHandle, phase: &str) {
    let _ = app.emit(
        "cli-install-progress",
        serde_json::json!({ "engine": "codex", "phase": phase }),
    );
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexInstallResult {
    pub success: bool,
    pub new_version: Option<String>,
    pub command: String,
    pub output_tail: String,
    pub binary_path: Option<String>,
}

fn parse_semver(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| token.trim_start_matches('v'))
        .find(|token| {
            let mut parts = token.split(['.', '-']);
            (0..3).all(|_| {
                parts.next().is_some_and(|part| {
                    !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())
                })
            })
        })
        .map(ToOwned::to_owned)
}

fn run_version(path: &Path) -> Option<String> {
    let output = Command::new(path)
        .arg("--version")
        .env("PATH", streaming::enhanced_path())
        .hide_console()
        .output()
        .ok()?;
    parse_semver(&String::from_utf8_lossy(&output.stdout))
        .or_else(|| parse_semver(&String::from_utf8_lossy(&output.stderr)))
}

type VersionParts<'a> = ((u64, u64, u64), Option<&'a str>);

fn version_parts(version: &str) -> Option<VersionParts<'_>> {
    let normalized = version.trim().trim_start_matches('v');
    let (core, prerelease) = normalized
        .split_once('-')
        .map_or((normalized, None), |(core, prerelease)| {
            (core, Some(prerelease))
        });
    let mut parts = core.split('.');
    let numbers = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    Some((numbers, prerelease))
}

fn semver_gt(latest: &str, installed: &str) -> bool {
    let Some((latest_numbers, latest_prerelease)) = version_parts(latest) else {
        return false;
    };
    let Some((installed_numbers, installed_prerelease)) = version_parts(installed) else {
        return false;
    };
    latest_numbers > installed_numbers
        || (latest_numbers == installed_numbers
            && latest_prerelease.is_none()
            && installed_prerelease.is_some())
}

fn versions_differ(left: &str, right: &str) -> bool {
    left.trim().trim_start_matches('v') != right.trim().trim_start_matches('v')
}

fn should_suggest_runtime_selection(
    version_mismatch: bool,
    update_available: bool,
    standalone_available: bool,
    desktop_available: bool,
    desktop_supported: bool,
) -> bool {
    version_mismatch
        && !update_available
        && standalone_available
        && desktop_available
        && desktop_supported
}

pub(crate) fn current_runtime_version() -> Option<String> {
    codex_locator::locate()
        .ok()
        .as_deref()
        .and_then(run_version)
}

pub(crate) fn cache_matches_version(version: Option<&str>) -> bool {
    let Some(version) = version else {
        return true;
    };
    match read_cache_version() {
        Some(cache_version) => !versions_differ(version, &cache_version),
        None => true,
    }
}

fn fetch_latest_version() -> Option<String> {
    {
        let guard = LATEST_CACHE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some((at, version)) = guard.as_ref() {
            if at.elapsed() < LATEST_CACHE_TTL {
                return Some(version.clone());
            }
        }
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let response: serde_json::Value = client.get(LATEST_RELEASE_URL).send().ok()?.json().ok()?;
    let release_tag = response.get("tag_name")?.as_str()?;
    let version = parse_semver(release_tag.strip_prefix("rust-v")?)?;
    *LATEST_CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some((Instant::now(), version.clone()));
    Some(version)
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path == Path::new("~") || path.starts_with("~/") {
                dirs::home_dir()
                    .map(|home| home.join(path.strip_prefix("~/").unwrap_or(Path::new(""))))
                    .unwrap_or(path)
            } else {
                path
            }
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

fn read_cache_version() -> Option<String> {
    let bytes = std::fs::read(codex_home()?.join("models_cache.json")).ok()?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()?
        .get("client_version")?
        .as_str()
        .map(str::to_string)
}

#[cfg(target_os = "macos")]
fn plist_string(path: &Path, key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let after_key = contents.split_once(&format!("<key>{key}</key>"))?.1;
    let after_open = after_key.split_once("<string>")?.1;
    Some(after_open.split_once("</string>")?.0.trim().to_string())
}

#[cfg(target_os = "macos")]
fn latest_computer_use_plugin_version(home: &Path) -> Option<String> {
    std::fs::read_dir(
        home.join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use"),
    )
    .ok()?
    .filter_map(Result::ok)
    .filter_map(|entry| {
        let manifest =
            std::fs::read(entry.path().join(".codex-plugin").join("plugin.json")).ok()?;
        serde_json::from_slice::<serde_json::Value>(&manifest)
            .ok()?
            .get("version")?
            .as_str()
            .map(str::to_string)
    })
    .max_by(|left, right| {
        version_parts(left)
            .map(|parts| parts.0)
            .cmp(&version_parts(right).map(|parts| parts.0))
    })
}

#[cfg(target_os = "macos")]
fn computer_use_status(
    plugin_version: Option<&str>,
    helper_version: Option<&str>,
    helper_complete: bool,
) -> ComputerUseEnvStatus {
    let Some(plugin_version) = plugin_version else {
        return ComputerUseEnvStatus::Unavailable;
    };
    if !helper_complete {
        return ComputerUseEnvStatus::NeedsSetup;
    }
    let expected_build = plugin_version
        .split_once('-')
        .map_or(plugin_version, |(core, _)| core)
        .rsplit('.')
        .next();
    if helper_version != expected_build {
        return ComputerUseEnvStatus::NeedsRefresh;
    }
    ComputerUseEnvStatus::Ready
}

#[cfg(target_os = "macos")]
fn computer_use_env_info() -> Option<ComputerUseEnvInfo> {
    let home = codex_home()?;
    let plugin_version = latest_computer_use_plugin_version(&home);
    let helper = home.join("computer-use").join("Codex Computer Use.app");
    let service = helper
        .join("Contents")
        .join("MacOS")
        .join("SkyComputerUseService");
    let client = helper
        .join("Contents")
        .join("SharedSupport")
        .join("SkyComputerUseClient.app")
        .join("Contents")
        .join("MacOS")
        .join("SkyComputerUseClient");
    let helper_version = plist_string(
        &helper.join("Contents").join("Info.plist"),
        "CFBundleVersion",
    );
    let status = computer_use_status(
        plugin_version.as_deref(),
        helper_version.as_deref(),
        service.is_file() && client.is_file(),
    );
    Some(ComputerUseEnvInfo {
        status,
        plugin_version,
        helper_version,
    })
}

#[cfg(not(target_os = "macos"))]
fn computer_use_env_info() -> Option<ComputerUseEnvInfo> {
    None
}

fn tail(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= OUTPUT_TAIL {
        return trimmed.to_string();
    }
    let start = trimmed.len() - OUTPUT_TAIL;
    let boundary = (start..trimmed.len())
        .find(|&index| trimmed.is_char_boundary(index))
        .unwrap_or(start);
    format!("…{}", &trimmed[boundary..])
}

fn codex_env_check_sync() -> CodexEnvInfo {
    let standalone_path = codex_locator::locate_standalone().ok();
    let binary_path = standalone_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let installed_version = standalone_path.as_deref().and_then(run_version);
    let latest_version = fetch_latest_version();
    let update_available = matches!(
        (&installed_version, &latest_version),
        (Some(installed), Some(latest)) if semver_gt(latest, installed)
    );
    let desktop_path = codex_locator::desktop_bundle_path();
    let desktop_version = desktop_path.as_deref().and_then(run_version);
    let version_mismatch = matches!(
        (&installed_version, &desktop_version),
        (Some(installed), Some(desktop)) if versions_differ(installed, desktop)
    );
    let active_runtime_source = codex_locator::active_runtime_source();
    let configured_runtime_source = codex_locator::configured_runtime_source();
    let active_runtime_version = match active_runtime_source {
        codex_locator::CodexRuntimeSource::Standalone => installed_version.clone(),
        codex_locator::CodexRuntimeSource::Desktop => desktop_version.clone(),
    };
    let desktop_supported =
        crate::engines::codex::supported_version(desktop_version.as_deref()) != Some(false);
    let runtime_selection_suggested = should_suggest_runtime_selection(
        version_mismatch,
        update_available,
        standalone_path.is_some(),
        desktop_path.is_some(),
        desktop_supported,
    );
    let cache_version = read_cache_version();
    let cache_version_mismatch = matches!(
        (&active_runtime_version, &cache_version),
        (Some(runtime), Some(cache)) if versions_differ(runtime, cache)
    );
    CodexEnvInfo {
        installed_version,
        latest_version,
        update_available,
        binary_path,
        desktop_version,
        version_mismatch,
        active_runtime_source,
        configured_runtime_source,
        active_runtime_version,
        runtime_restart_required: active_runtime_source != configured_runtime_source,
        runtime_selection_suggested,
        cache_version,
        cache_version_mismatch,
        computer_use: computer_use_env_info(),
    }
}

#[tauri::command]
pub async fn codex_env_check() -> Result<CodexEnvInfo, String> {
    tauri::async_runtime::spawn_blocking(codex_env_check_sync)
        .await
        .map_err(|error| format!("Codex 环境检查线程异常退出: {error}"))
}

#[tauri::command]
pub fn codex_runtime_source_set(source: codex_locator::CodexRuntimeSource) -> Result<(), String> {
    let path = match source {
        codex_locator::CodexRuntimeSource::Standalone => codex_locator::locate_standalone()?,
        codex_locator::CodexRuntimeSource::Desktop => codex_locator::desktop_bundle_path()
            .ok_or_else(|| "ChatGPT 内置 Codex 运行时不可用".to_string())?,
    };
    let version = run_version(&path).ok_or_else(|| "无法读取所选 Codex 运行时版本".to_string())?;
    if crate::engines::codex::supported_version(Some(&version)) == Some(false) {
        return Err(format!(
            "Codex 运行时版本 {version} 低于 Monet 支持的最低版本"
        ));
    }
    let value =
        serde_json::to_value(source).map_err(|error| format!("运行时设置序列化失败: {error}"))?;
    crate::config::write_app_setting_checked(codex_locator::RUNTIME_SOURCE_SETTING_KEY, value)
}

fn codex_env_install_sync(app: AppHandle) -> Result<CodexInstallResult, String> {
    emit_install_progress(&app, "installing");
    #[cfg(windows)]
    let (mut command, description): (Command, String) = {
        let mut command = Command::new("npm");
        command.args(["install", "-g", "@openai/codex"]);
        command.env("PATH", streaming::enhanced_path());
        command.hide_console();
        (command, "npm install -g @openai/codex".to_string())
    };
    #[cfg(not(windows))]
    let (mut command, description): (Command, String) = {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &format!("curl -fsSL {INSTALL_SCRIPT_URL} | sh")]);
        command.env("PATH", streaming::enhanced_path());
        (command, format!("curl -fsSL {INSTALL_SCRIPT_URL} | sh"))
    };

    let output = match command.output() {
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
    let binary_path = codex_locator::redetect_standalone().ok();
    let new_version = binary_path.as_deref().and_then(run_version);
    let success = output.status.success() && new_version.is_some();
    emit_install_progress(&app, if success { "completed" } else { "failed" });

    Ok(CodexInstallResult {
        success,
        new_version,
        command: description,
        output_tail: tail(&combined),
        binary_path: binary_path.map(|path| path.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub async fn codex_env_install(app: AppHandle) -> Result<CodexInstallResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env_install_sync(app))
        .await
        .map_err(|error| format!("Codex 安装线程异常退出: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_versions() {
        assert_eq!(parse_semver("codex-cli 0.42.0"), Some("0.42.0".into()));
        assert_eq!(parse_semver("v1.2.3"), Some("1.2.3".into()));
        assert_eq!(
            parse_semver("rust-v0.147.0".strip_prefix("rust-v").unwrap()),
            Some("0.147.0".into())
        );
        assert_eq!(parse_semver("Codex CLI"), None);
    }

    #[test]
    fn compares_stable_and_prerelease_versions() {
        assert!(semver_gt("0.148.0", "0.147.0"));
        assert!(semver_gt("0.148.0", "0.148.0-alpha.9"));
        assert!(!semver_gt("0.148.0-alpha.9", "0.148.0"));
        assert!(!semver_gt("0.147.0", "0.147.0"));
    }

    #[test]
    fn detects_exact_runtime_version_differences() {
        assert!(versions_differ("0.148.0", "0.148.0-alpha.9"));
        assert!(!versions_differ("v0.147.0", "0.147.0"));
    }

    #[test]
    fn suggests_runtime_choice_only_when_upgrade_cannot_resolve_mismatch() {
        assert!(should_suggest_runtime_selection(
            true, false, true, true, true
        ));
        assert!(!should_suggest_runtime_selection(
            true, true, true, true, true
        ));
        assert!(!should_suggest_runtime_selection(
            false, false, true, true, true
        ));
        assert!(!should_suggest_runtime_selection(
            true, false, true, false, true
        ));
        assert!(!should_suggest_runtime_selection(
            true, false, true, true, false
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn classifies_computer_use_component_state() {
        assert_eq!(
            computer_use_status(Some("1.0.1000919"), Some("1000919"), true),
            ComputerUseEnvStatus::Ready
        );
        assert_eq!(
            computer_use_status(Some("1.0.1000920"), Some("1000919"), true),
            ComputerUseEnvStatus::NeedsRefresh
        );
        assert_eq!(
            computer_use_status(Some("1.0.1000919"), Some("1000919"), false),
            ComputerUseEnvStatus::NeedsSetup
        );
        assert_eq!(
            computer_use_status(None, Some("1000919"), true),
            ComputerUseEnvStatus::Unavailable
        );
    }
}
