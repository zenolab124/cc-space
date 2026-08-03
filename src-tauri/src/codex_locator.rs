use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FAIL_TTL: Duration = Duration::from_secs(60);

static MEM_HIT: Mutex<Option<PathBuf>> = Mutex::new(None);
static MEM_FAIL: Mutex<Option<Instant>> = Mutex::new(None);

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

pub fn locate() -> Result<PathBuf, String> {
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

    #[cfg(windows)]
    #[test]
    fn windows_rejects_command_shims() {
        assert!(candidate_paths().iter().all(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        }));
    }
}
