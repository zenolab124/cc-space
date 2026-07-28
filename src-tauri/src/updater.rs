//! 更新通道（stable / nightly）与 updater 收口。
//!
//! 为什么不用前端 plugin-updater 的 `check()`：它的 CheckOptions 不含 endpoint 字段，
//! 而 tauri.conf 的 `endpoints` 数组语义是「依次尝试直到成功」——把两个通道都配上会让
//! 所有用户都被推 nightly。通道切换只能在 Rust 侧用 `updater_builder().endpoints()`
//! 动态指定，因此整条检查/下载链路一并收口到这里，前端只调 command + 听进度事件。
//!
//! 版本号语义：nightly 用 `1.0.3-nightly.YYYYMMDD`。semver 上它 > 1.0.2 且 < 1.0.3，
//! 于是 nightly 用户持续收 nightly，而稳定版发 1.0.3 时会自然把他们带回稳定线。

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const STABLE_ENDPOINT: &str =
    "https://github.com/zenolab124/monet/releases/latest/download/latest.json";
// nightly 走滚动 tag（固定叫 nightly，每次强制覆盖），避免 release 列表被日期 tag 淹掉
const NIGHTLY_ENDPOINT: &str =
    "https://github.com/zenolab124/monet/releases/download/nightly/nightly.json";

const CHANNEL_KEY: &str = "updateChannel";

pub const EVENT_PROGRESS: &str = "updater://progress";
pub const EVENT_DOWNLOADED: &str = "updater://downloaded";

/// 待安装更新。check 与 install 分两次调用，中间需要把 Update 存住。
#[derive(Default)]
pub struct PendingUpdate(pub Mutex<Option<Update>>);

/// 读通道设置。只认 "nightly"，其余一切值（含缺失、非法、null）都落回 stable——
/// 宁可少推预览版，也不能因为脏数据把用户静默切到 nightly。
fn current_channel() -> &'static str {
    match crate::config::read_app_setting(CHANNEL_KEY).and_then(|v| v.as_str().map(String::from)) {
        Some(s) if s == "nightly" => "nightly",
        _ => "stable",
    }
}

fn endpoint_for(channel: &str) -> &'static str {
    if channel == "nightly" {
        NIGHTLY_ENDPOINT
    } else {
        STABLE_ENDPOINT
    }
}

#[derive(Serialize, Clone)]
pub struct UpdateMeta {
    pub version: String,
    pub notes: String,
}

#[derive(Serialize, Clone)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
pub fn get_update_channel() -> String {
    current_channel().to_string()
}

#[tauri::command]
pub fn set_update_channel(channel: String) -> Result<(), String> {
    let c = if channel == "nightly" { "nightly" } else { "stable" };
    crate::config::write_app_setting(CHANNEL_KEY, serde_json::json!(c));
    Ok(())
}

#[tauri::command]
pub async fn updater_check(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateMeta>, String> {
    let endpoint = endpoint_for(current_channel());
    let url = endpoint
        .parse()
        .map_err(|e| format!("更新地址非法({endpoint}): {e}"))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    match updater.check().await {
        Ok(Some(u)) => {
            let meta = UpdateMeta {
                version: u.version.clone(),
                notes: u.body.clone().unwrap_or_default(),
            };
            *pending.0.lock().map_err(|e| e.to_string())? = Some(u);
            Ok(Some(meta))
        }
        Ok(None) => {
            *pending.0.lock().map_err(|e| e.to_string())? = None;
            Ok(None)
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn updater_install(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    // 先取出再 await：MutexGuard 不能跨 await 持有
    let update = {
        let mut guard = pending.0.lock().map_err(|e| e.to_string())?;
        guard.take().ok_or("没有待安装的更新")?
    };

    let downloaded = std::sync::atomic::AtomicU64::new(0);
    let emitter = app.clone();
    let finish_emitter = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                let n = downloaded
                    .fetch_add(chunk as u64, std::sync::atomic::Ordering::Relaxed)
                    + chunk as u64;
                let _ = emitter.emit(EVENT_PROGRESS, Progress { downloaded: n, total });
            },
            move || {
                let _ = finish_emitter.emit(EVENT_DOWNLOADED, ());
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
