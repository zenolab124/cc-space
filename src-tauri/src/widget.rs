use std::path::PathBuf;
use std::process::Command;

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::config;

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

fn widget_container_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join("Library/Containers/io.github.zenolab124.monet.widget/Data/widget-data.json")
    })
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

    let updater = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("widget-updater")));
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

#[tauri::command]
pub fn update_widget(
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

    let container_result: Result<(), String> = (|| {
        let path = widget_container_path().ok_or("widget container path unavailable")?;
        let parent = path.parent().ok_or("widget container parent unavailable")?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create widget container directory: {e}"))?;
        std::fs::write(&path, &json)
            .map_err(|e| format!("write widget container data: {e}"))
    })();

    let backup_result: Result<(), String> = (|| {
        if let Some(parent) = backup_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&backup_path, &json).map_err(|e| e.to_string())
    })();

    match (container_result, backup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(container), Ok(())) => Err(format!(
            "widget container write failed; backup updated: {container}"
        )),
        (Ok(()), Err(backup)) => Err(format!("widget backup write failed: {backup}")),
        (Err(container), Err(backup)) => Err(format!(
            "widget container write failed: {container}; backup write failed: {backup}"
        )),
    }
}
