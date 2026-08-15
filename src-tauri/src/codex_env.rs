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
    pub cache_version: Option<String>,
    pub cache_version_mismatch: bool,
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

fn version_parts(version: &str) -> Option<((u64, u64, u64), Option<&str>)> {
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

pub(crate) fn current_installed_version() -> Option<String> {
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
    let located = codex_locator::locate().ok();
    let binary_path = located
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let installed_version = located.as_ref().and_then(|path| run_version(path));
    let latest_version = fetch_latest_version();
    let update_available = matches!(
        (&installed_version, &latest_version),
        (Some(installed), Some(latest)) if semver_gt(latest, installed)
    );
    let desktop_version = codex_locator::desktop_bundle_path()
        .as_deref()
        .and_then(run_version);
    let version_mismatch = matches!(
        (&installed_version, &desktop_version),
        (Some(installed), Some(desktop)) if versions_differ(installed, desktop)
    );
    let cache_version = read_cache_version();
    let cache_version_mismatch = matches!(
        (&installed_version, &cache_version),
        (Some(installed), Some(cache)) if versions_differ(installed, cache)
    );
    CodexEnvInfo {
        installed_version,
        latest_version,
        update_available,
        binary_path,
        desktop_version,
        version_mismatch,
        cache_version,
        cache_version_mismatch,
    }
}

#[tauri::command]
pub async fn codex_env_check() -> Result<CodexEnvInfo, String> {
    tauri::async_runtime::spawn_blocking(codex_env_check_sync)
        .await
        .map_err(|error| format!("Codex 环境检查线程异常退出: {error}"))
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
    let binary_path = codex_locator::redetect().ok();
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
}
