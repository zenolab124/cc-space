//! Codex CLI 本地环境检查与一键安装。

use std::path::Path;
use std::process::Command;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::codex_locator;
use crate::proc_ext::HideConsole;
use crate::streaming;

const INSTALL_SCRIPT_URL: &str = "https://chatgpt.com/codex/install.sh";
const OUTPUT_TAIL: usize = 2000;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvInfo {
    pub installed_version: Option<String>,
    pub binary_path: Option<String>,
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
    CodexEnvInfo {
        installed_version,
        binary_path,
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
    let new_version = binary_path
        .as_deref()
        .and_then(run_version);
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
        assert_eq!(parse_semver("Codex CLI"), None);
    }
}
