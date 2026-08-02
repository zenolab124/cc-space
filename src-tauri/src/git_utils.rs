use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::proc_ext::HideConsole;

pub(crate) fn run_git_readonly_blocking(cwd: &Path, args: &[&str]) -> Result<Output, String> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .hide_console()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => return child.wait_with_output().map_err(|e| e.to_string()),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                return Err("git timeout".to_string());
            }
            None => std::thread::sleep(Duration::from_millis(30)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettingsRoots {
    pub project: PathBuf,
    pub local: PathBuf,
}

fn output_path(output: &Output) -> Option<PathBuf> {
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

pub(crate) fn parse_main_worktree(output: &[u8]) -> Option<PathBuf> {
    output
        .split(|b| *b == 0)
        .find_map(|field| field.strip_prefix(b"worktree "))
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
}

pub(crate) fn settings_roots(cwd: &Path) -> SettingsRoots {
    let project = run_git_readonly_blocking(cwd, &["rev-parse", "--show-toplevel"])
        .ok()
        .and_then(|output| output_path(&output))
        .unwrap_or_else(|| cwd.to_path_buf());

    let local = run_git_readonly_blocking(cwd, &["worktree", "list", "--porcelain", "-z"])
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| parse_main_worktree(&output.stdout))
        .unwrap_or_else(|| project.clone());

    SettingsRoots { project, local }
}

#[cfg(test)]
mod tests {
    use super::parse_main_worktree;
    use std::path::PathBuf;

    #[test]
    fn parses_main_worktree_from_nul_porcelain_output() {
        let raw = b"worktree /repo/main\0HEAD abc\0branch refs/heads/main\0\0worktree /repo/wt\0HEAD def\0\0";
        assert_eq!(parse_main_worktree(raw), Some(PathBuf::from("/repo/main")));
    }

    #[test]
    fn rejects_porcelain_without_worktree_record() {
        assert_eq!(
            parse_main_worktree(b"HEAD abc\0branch refs/heads/main\0"),
            None
        );
    }
}
