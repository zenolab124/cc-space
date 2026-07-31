use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

const SNAPSHOT_FILE: &str = "routine-environment.json";
const LOCK_FILE: &str = "routine-environment.lock";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutineEnvironment {
    mode: String,
    claude_config_dir: Option<String>,
}

fn snapshot_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SNAPSHOT_FILE)
}

fn lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LOCK_FILE)
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

#[allow(dead_code)]
fn write_synced(path: &Path, content: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

#[allow(dead_code)]
fn publish_snapshot(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
    if let Err(error) = write_synced(&tmp, content).and_then(|_| replace_file(&tmp, path)) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(windows)]
#[allow(dead_code)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[allow(dead_code)]
pub fn prepare_claude_config_dir<F, G>(
    data_dir: &Path,
    claude_config_dir: Option<&str>,
    force_invalidate: bool,
    prepare: F,
    abort: G,
) -> std::io::Result<()>
where
    F: FnOnce() -> std::io::Result<()>,
    G: FnOnce() -> std::io::Result<()>,
{
    fs::create_dir_all(data_dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path(data_dir))?;
    lock.lock_exclusive()?;

    let environment = match claude_config_dir {
        Some(value) => RoutineEnvironment {
            mode: "custom".to_string(),
            claude_config_dir: Some(value.to_string()),
        },
        None => RoutineEnvironment {
            mode: "default".to_string(),
            claude_config_dir: None,
        },
    };
    let content = serde_json::to_string_pretty(&environment)
        .map_err(|error| invalid_data(error.to_string()))?;
    let path = snapshot_path(data_dir);
    let changed = force_invalidate || fs::read_to_string(&path).ok().as_deref() != Some(&content);
    if force_invalidate {
        // 旧 runner 不理解快照，updating 到原子替换之间仍有一个极短窗口；
        // 替换后的新 runner 会阻塞在共享锁，成功时只读到完整快照，失败时只读到 updating。
        let result = write_synced(&path, r#"{"mode":"updating"}"#)
            .and_then(|_| prepare())
            .and_then(|_| publish_snapshot(&path, &content));
        if let Err(error) = result {
            return match abort() {
                Ok(()) => Err(error),
                Err(abort_error) => Err(std::io::Error::other(format!(
                    "{}; routine runner abort failed: {}",
                    error, abort_error
                ))),
            };
        }
    } else {
        if changed {
            write_synced(&path, r#"{"mode":"updating"}"#)?;
        }
        prepare()?;
        if changed {
            publish_snapshot(&path, &content)?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub fn read_claude_config_dir(data_dir: &Path) -> std::io::Result<Option<String>> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path(data_dir))?;
    FileExt::lock_shared(&lock)?;

    let content = fs::read_to_string(snapshot_path(data_dir))?;
    let environment: RoutineEnvironment =
        serde_json::from_str(&content).map_err(|error| invalid_data(error.to_string()))?;
    match environment.mode.as_str() {
        "default" if environment.claude_config_dir.is_none() => Ok(None),
        "custom" => environment
            .claude_config_dir
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(Some)
            .ok_or_else(|| invalid_data("custom routine environment has no Claude config dir")),
        _ => Err(invalid_data("invalid routine environment mode")),
    }
}
