use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const FAIL_TTL: Duration = Duration::from_secs(60);
pub const RUNTIME_SOURCE_SETTING_KEY: &str = "codexRuntimeSource";

static MEM_HIT: Mutex<Option<PathBuf>> = Mutex::new(None);
static MEM_FAIL: Mutex<Option<Instant>> = Mutex::new(None);
static ACTIVE_RUNTIME_SOURCE: OnceLock<CodexRuntimeSource> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexRuntimeSource {
    Standalone,
    Desktop,
}

impl CodexRuntimeSource {
    fn from_setting(value: &serde_json::Value) -> Option<Self> {
        match value.as_str()? {
            "standalone" => Some(Self::Standalone),
            "desktop" => Some(Self::Desktop),
            _ => None,
        }
    }
}

#[cfg(unix)]
fn is_valid_binary(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_file()
        && std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_valid_binary(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

fn nvm_latest_bin(home: &Path) -> Option<PathBuf> {
    let root = home.join(".nvm").join("versions").join("node");
    let latest = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            let version: Vec<u32> = entry
                .file_name()
                .to_string_lossy()
                .trim_start_matches('v')
                .split('.')
                .filter_map(|part| part.parse().ok())
                .collect();
            (version.len() == 3).then(|| (version, entry.path()))
        })
        .max_by(|left, right| left.0.cmp(&right.0))?;
    Some(latest.1.join("bin").join(binary_name()))
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "codex.exe"
    } else {
        "codex"
    }
}

fn absolute_root(path: PathBuf, home: Option<&Path>) -> Option<PathBuf> {
    if path.is_absolute() {
        return Some(path);
    }
    let suffix = path.strip_prefix("~").ok()?;
    Some(home?.join(suffix))
}

fn candidate_paths() -> Vec<PathBuf> {
    let home = dirs::home_dir().filter(|path| path.is_absolute());
    let mut candidates: Vec<PathBuf> = std::env::split_paths(&crate::path_env::enhanced_path())
        .filter_map(|directory| absolute_root(directory, home.as_deref()))
        .map(|directory| directory.join(binary_name()))
        .collect();

    if let Some(home) = home.as_deref() {
        candidates.extend([
            home.join(".local").join("bin").join(binary_name()),
            home.join(".cargo").join("bin").join(binary_name()),
            home.join(".volta").join("bin").join(binary_name()),
            home.join(".bun").join("bin").join(binary_name()),
        ]);
    }

    #[cfg(target_os = "macos")]
    {
        candidates.extend([
            PathBuf::from("/opt/homebrew/bin/codex"),
            PathBuf::from("/usr/local/bin/codex"),
        ]);
        if let Some(home) = home.as_deref() {
            candidates.push(home.join("Library").join("pnpm").join("codex"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        candidates.extend([
            PathBuf::from("/usr/local/bin/codex"),
            PathBuf::from("/usr/bin/codex"),
        ]);
        if let Some(home) = home.as_deref() {
            candidates.push(home.join(".local").join("share").join("pnpm").join("codex"));
        }
    }

    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if let Some(root) = absolute_root(PathBuf::from(appdata), home.as_deref()) {
                candidates.push(root.join("npm").join(binary_name()));
            }
        }
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            if let Some(root) = absolute_root(PathBuf::from(localappdata), home.as_deref()) {
                candidates.push(root.join("Programs").join("Codex").join(binary_name()));
            }
        }
    }

    if let Some(home) = home.as_deref() {
        if let Some(path) = nvm_latest_bin(home) {
            candidates.push(path);
        }
    }
    candidates
}

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("MONET_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".monet")))
        .map(|directory| directory.join("settings.json"))
}

pub fn configured_runtime_source() -> CodexRuntimeSource {
    settings_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|settings| settings.get(RUNTIME_SOURCE_SETTING_KEY).cloned())
        .as_ref()
        .and_then(CodexRuntimeSource::from_setting)
        .unwrap_or(CodexRuntimeSource::Standalone)
}

pub fn active_runtime_source() -> CodexRuntimeSource {
    *ACTIVE_RUNTIME_SOURCE.get_or_init(configured_runtime_source)
}

/// Detect the Codex binary bundled inside the ChatGPT desktop app without
/// treating it as a standalone CLI candidate. Monet only uses this path when
/// the user explicitly selects the desktop runtime.
pub fn desktop_bundle_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex",
        )];
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("Applications/ChatGPT.app/Contents/Resources/codex"));
        }
        candidates.into_iter().find(|path| is_valid_binary(path))
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn locate() -> Result<PathBuf, String> {
    match active_runtime_source() {
        CodexRuntimeSource::Standalone => locate_standalone(),
        CodexRuntimeSource::Desktop => desktop_bundle_path().ok_or_else(|| {
            "The selected ChatGPT bundled Codex runtime is not available".to_string()
        }),
    }
}

pub fn locate_standalone() -> Result<PathBuf, String> {
    {
        let mut hit = MEM_HIT.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(path) = hit.clone() {
            if is_valid_binary(&path) {
                return Ok(path);
            }
            *hit = None;
        }
    }

    {
        let failed = MEM_FAIL.lock().unwrap_or_else(|error| error.into_inner());
        if failed.is_some_and(|at| at.elapsed() < FAIL_TTL) {
            return Err("Codex CLI not found".into());
        }
    }

    for path in candidate_paths() {
        if is_valid_binary(&path) {
            *MEM_HIT.lock().unwrap_or_else(|error| error.into_inner()) = Some(path.clone());
            *MEM_FAIL.lock().unwrap_or_else(|error| error.into_inner()) = None;
            return Ok(path);
        }
    }

    *MEM_FAIL.lock().unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
    Err("Codex CLI not found".into())
}

pub fn is_available() -> bool {
    locate().is_ok()
}

/// 清除探测缓存后重新定位，供设置页安装完成后立即复测。
pub fn redetect_standalone() -> Result<PathBuf, String> {
    *MEM_HIT.lock().unwrap_or_else(|error| error.into_inner()) = None;
    *MEM_FAIL.lock().unwrap_or_else(|error| error.into_inner()) = None;
    locate_standalone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_absolute() {
        assert!(candidate_paths().iter().all(|path| path.is_absolute()));
    }

    #[test]
    fn expands_tilde_and_rejects_other_relative_roots() {
        let home = std::env::current_dir().unwrap();
        assert_eq!(
            absolute_root(PathBuf::from("~").join("bin"), Some(&home)),
            Some(home.join("bin")),
        );
        assert_eq!(
            absolute_root(PathBuf::from("relative/bin"), Some(&home)),
            None
        );
    }

    #[test]
    fn parses_supported_runtime_source_values() {
        assert_eq!(
            CodexRuntimeSource::from_setting(&serde_json::json!("standalone")),
            Some(CodexRuntimeSource::Standalone)
        );
        assert_eq!(
            CodexRuntimeSource::from_setting(&serde_json::json!("desktop")),
            Some(CodexRuntimeSource::Desktop)
        );
        assert_eq!(
            CodexRuntimeSource::from_setting(&serde_json::json!("unknown")),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_rejects_command_shims() {
        assert!(candidate_paths().iter().all(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        }));
    }
}
