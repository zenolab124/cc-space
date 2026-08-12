use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use chrono::Local;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::widget_storage;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WidgetConfig {
    #[serde(default)]
    pub day_start_hour: i8,
    #[serde(default)]
    pub month_mode: String,
}

impl Default for WidgetConfig {
    fn default() -> Self {
        Self { day_start_hour: 0, month_mode: "natural".into() }
    }
}

fn widget_config_path() -> PathBuf {
    config::data_dir().join("widget-config.json")
}

pub fn read_widget_config() -> WidgetConfig {
    std::fs::read_to_string(widget_config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_widget_config() -> WidgetConfig {
    read_widget_config()
}

#[tauri::command]
pub fn set_widget_config(day_start_hour: i8, month_mode: String) -> Result<(), String> {
    let cfg = WidgetConfig { day_start_hour, month_mode };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(widget_config_path(), json).map_err(|e| e.to_string())
}

const LAUNCH_AGENT_LABEL: &str = "io.github.zenolab124.monet.widget-updater";
const SNAPSHOT_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
static SNAPSHOT_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn widget_updater_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("widget-updater")))
}

fn snapshot_needs_refresh_at(modified: Option<SystemTime>, now: SystemTime) -> bool {
    let Some(modified) = modified else {
        return true;
    };
    now.duration_since(modified)
        .map(|age| age >= SNAPSHOT_REFRESH_INTERVAL)
        // 未来时间戳通常来自时钟校准；此时不应反复刷新。
        .unwrap_or(false)
}

fn valid_widget_snapshot(contents: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(contents) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("todaySessions").is_some_and(|value| value.is_u64())
        && object.get("todayTokens").is_some_and(|value| value.is_u64())
        && object.get("models").is_some_and(|value| value.is_array())
        && object.get("updatedAt").is_some_and(|value| value.is_string())
}

fn snapshot_needs_refresh() -> Result<bool, String> {
    let Some(path) = widget_storage::shared_snapshot_path()? else {
        // 自签开发构建没有可授权的 App Group，只维护 ~/.monet 备份。
        return Ok(false);
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Ok(true);
    };
    if !valid_widget_snapshot(&contents) {
        return Ok(true);
    }
    let modified = std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    Ok(snapshot_needs_refresh_at(modified, SystemTime::now()))
}

fn refresh_snapshot_once() -> Result<(), String> {
    let updater = widget_updater_path()
        .ok_or_else(|| "widget snapshot refresh skipped: updater path unavailable".to_string())?;
    if !updater.exists() {
        return Err("widget snapshot refresh skipped: bundled updater missing".into());
    }

    match Command::new(&updater).output() {
        Ok(output) if output.status.success() => {
            log::info!("widget snapshot refreshed during startup");
            Ok(())
        }
        Ok(output) => {
            let stderr: String = String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(512)
                .collect();
            Err(format!(
                "widget snapshot startup refresh failed (status={}): {}",
                output.status,
                stderr
            ))
        }
        Err(error) => Err(format!("widget snapshot startup refresh failed: {error}")),
    }
}

pub(crate) fn refresh_snapshot_if_needed() -> Result<(), String> {
    if snapshot_needs_refresh()? {
        refresh_snapshot_once()
    } else {
        Ok(())
    }
}

/// release 启动时先在后台补齐首份快照，再注册周期刷新服务。
/// 即使系统后台项尚待批准，Widget 也能读取主应用本次启动生成的数据。
pub fn startup_sync() {
    if cfg!(debug_assertions) || !crate::scheduler::owns_machine_schedule() {
        return;
    }
    if let Err(error) = refresh_snapshot_if_needed() {
        log::warn!("{error}");
    }
    ensure_launch_agent();
}

pub fn ensure_launch_agent() {
    // dev 构建不触碰 launchd 注册面：cargo target 目录恰好有同名 updater 二进制，
    // dev 会把 plist 改写成指向 debug 产物，回正式 .app 又翻转回来——
    // 每次语境切换都重装并重复弹系统后台项通知（与 scheduler/tray 同门控）
    if cfg!(debug_assertions) {
        return;
    }
    // 机器级注册面只归默认数据目录实例所有（与 scheduler 同判据）
    if !crate::scheduler::owns_machine_schedule() {
        return;
    }
    // macOS 13+ 的后台项目由 SMAppService 接管，plist 必须来自 app bundle；
    // 不再把开发机路径写入用户的 ~/Library/LaunchAgents。
    if crate::service_management::available() {
        crate::background_services::ensure_widget_updater();
        return;
    }
    let Some(home) = dirs::home_dir() else { return };

    // 旧标签清理:更名前(com.ccspace.widget-updater)安装的 LaunchAgent 会在用户机
    // 残留,检测到旧 plist 就卸载并删除(失败不阻断新装)
    let legacy_plist = home
        .join("Library/LaunchAgents")
        .join("com.ccspace.widget-updater.plist");
    if legacy_plist.exists() {
        let _ = Command::new("launchctl")
            .args(["remove", "com.ccspace.widget-updater"])
            .output();
        let _ = std::fs::remove_file(&legacy_plist);
    }

    let plist_path = home.join("Library/LaunchAgents").join(format!("{LAUNCH_AGENT_LABEL}.plist"));

    let updater = widget_updater_path();
    let Some(updater) = updater else { return };
    if !updater.exists() {
        return;
    }

    let updater_str = updater.to_string_lossy();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LAUNCH_AGENT_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{updater_str}</string>
	</array>
	<key>StartInterval</key>
	<integer>1800</integer>
	<key>RunAtLoad</key>
	<true/>
	<key>StandardErrorPath</key>
	<string>/tmp/monet-widget-updater.log</string>
</dict>
</plist>"#
    );

    // 全等比较（原为 contains 判据）：模板演进时旧 plist 会静默残留旧配置
    let need_install = std::fs::read_to_string(&plist_path)
        .map(|existing| existing != plist)
        .unwrap_or(true);

    if need_install {
        // WARN 档：重装触发系统后台项通知，release 日志留第一现场
        log::warn!("widget updater agent outdated, reinstalling");
        let uid = Command::new("id").arg("-u").output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "501".to_string());
        // bootout/bootstrap 取代已废弃的 load/unload（与 tray/routine 同栈）
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{LAUNCH_AGENT_LABEL}")])
            .output();
        if let Some(parent) = plist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&plist_path, &plist).is_ok() {
            let _ = Command::new("launchctl")
                .args(["bootstrap", &format!("gui/{uid}"), &plist_path.to_string_lossy()])
                .output();
        }
    }
}

fn write_snapshot(path: &Path, json: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("snapshot path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create {}: {error}", parent.display()))?;

    let mut temporary_name = path.as_os_str().to_os_string();
    let sequence = SNAPSHOT_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
    let temporary_path = PathBuf::from(temporary_name);
    std::fs::write(&temporary_path, json)
        .map_err(|error| format!("write {}: {error}", temporary_path.display()))?;
    if let Err(error) = std::fs::rename(&temporary_path, path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(format!("replace {}: {error}", path.display()));
    }
    Ok(())
}

fn with_snapshot_lock<T>(operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let lock_path = config::data_dir().join("widget-data.lock");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("open widget snapshot lock: {error}"))?;
    FileExt::lock_exclusive(&lock)
        .map_err(|error| format!("lock widget snapshot: {error}"))?;
    let result = operation();
    let _ = FileExt::unlock(&lock);
    result
}

#[tauri::command]
pub fn update_widget(
    today_sessions: u32,
    today_tokens: u64,
    models: Vec<String>,
) -> Result<(), String> {
    with_snapshot_lock(|| update_widget_locked(today_sessions, today_tokens, models))
}

fn update_widget_locked(
    today_sessions: u32,
    today_tokens: u64,
    models: Vec<String>,
) -> Result<(), String> {
    let backup_path = config::data_dir().join("widget-data.json");

    let mut doc: serde_json::Value = std::fs::read_to_string(&backup_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let obj = doc.as_object_mut().ok_or("invalid widget data")?;
    obj.insert("todaySessions".into(), today_sessions.into());
    obj.insert("todayTokens".into(), today_tokens.into());
    obj.insert("models".into(), serde_json::json!(models));
    obj.insert("updatedAt".into(), Local::now().to_rfc3339().into());

    let json = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;

    let shared_result = match widget_storage::shared_snapshot_path() {
        Ok(Some(path)) => write_snapshot(&path, &json),
        Ok(None) => Ok(()),
        Err(error) => Err(error),
    };

    let backup_result = write_snapshot(&backup_path, &json);

    match (shared_result, backup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(shared), Ok(())) => Err(format!(
            "widget shared container write failed; backup updated: {shared}"
        )),
        (Ok(()), Err(backup)) => Err(format!("widget backup write failed: {backup}")),
        (Err(shared), Err(backup)) => Err(format!(
            "widget shared container write failed: {shared}; backup write failed: {backup}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_stale_snapshot_requires_startup_refresh() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        assert!(snapshot_needs_refresh_at(None, now));
        assert!(snapshot_needs_refresh_at(
            Some(now - SNAPSHOT_REFRESH_INTERVAL),
            now
        ));
        assert!(!snapshot_needs_refresh_at(
            Some(now - Duration::from_secs(60)),
            now
        ));
    }

    #[test]
    fn future_snapshot_timestamp_does_not_loop_refreshes() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        assert!(!snapshot_needs_refresh_at(
            Some(now + Duration::from_secs(60)),
            now
        ));
    }

    #[test]
    fn invalid_or_incomplete_snapshot_requires_refresh() {
        assert!(!valid_widget_snapshot(""));
        assert!(!valid_widget_snapshot(r#"{"todaySessions": 1}"#));
        assert!(valid_widget_snapshot(
            r#"{"todaySessions":1,"todayTokens":2,"models":[],"updatedAt":"now"}"#
        ));
    }
}
