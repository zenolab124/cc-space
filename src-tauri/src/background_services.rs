use serde::Serialize;

use crate::service_management::{self, ServiceKind, ServiceStatus};

pub const TRAY_SERVICE_ID: &str = "io.github.zenolab124.monet.tray";
pub const WIDGET_UPDATER_PLIST: &str = "io.github.zenolab124.monet.widget-updater.plist";
pub const WIDGET_UPDATER_LABEL: &str = "io.github.zenolab124.monet.widget-updater";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundServiceStatus {
    pub tray: String,
    pub widget_updater: String,
}

fn status(kind: ServiceKind, value: &str) -> ServiceStatus {
    service_management::status(kind, value)
}

fn registration_required(status: ServiceStatus) -> bool {
    matches!(
        status,
        ServiceStatus::NotRegistered | ServiceStatus::NotFound
    )
}

fn registration_usable(status: ServiceStatus) -> bool {
    matches!(
        status,
        ServiceStatus::Enabled | ServiceStatus::RequiresApproval
    )
}

fn runtime_recovery_required(status: ServiceStatus, launchd_healthy: bool) -> bool {
    matches!(status, ServiceStatus::Enabled) && !launchd_healthy
}

fn validate_registered_status(
    kind: ServiceKind,
    value: &str,
    label: &str,
) -> Result<ServiceStatus, String> {
    let registered = status(kind, value);
    if registration_usable(registered) {
        Ok(registered)
    } else {
        Err(format!(
            "{label} registration did not become active (status={})",
            registered.as_str()
        ))
    }
}

fn ensure_registered(
    kind: ServiceKind,
    value: &str,
    launchd_label: &str,
    label: &str,
) -> Result<ServiceStatus, String> {
    let current = status(kind, value);
    let launchd_healthy = !matches!(current, ServiceStatus::Enabled)
        || service_management::launchd_service_healthy(launchd_label);
    if runtime_recovery_required(current, launchd_healthy) {
        // bootout 可能让任务消失，软件更新也可能让现有任务因旧 LWCR 进入
        // spawn failed；两种情况下 SMAppService 都仍会报告 Enabled。
        // register 会报 already registered，只能先注销再注册。
        log::warn!("{label} is enabled but unhealthy, re-registering");
        service_management::unregister(kind, value)
            .map_err(|error| format!("{label} recovery unregister failed: {error}"))?;
        service_management::register(kind, value)
            .map_err(|error| format!("{label} recovery registration failed: {error}"))?;
        return validate_registered_status(kind, value, label);
    }
    if registration_usable(current) {
        return Ok(current);
    }
    // NotFound 也必须尝试注册：SMAppService 对从未注册过的服务（BTM 无记录）
    // 返回的就是 notFound，它不代表 bundle 里缺文件。真缺文件时 register
    // 会带具体 NSError 失败，那才是可信的错误。
    if registration_required(current) {
        service_management::register(kind, value)
            .map_err(|error| format!("{label} registration was rejected: {error}"))?;
        return validate_registered_status(kind, value, label);
    }
    Err("ServiceManagement is unavailable on this macOS version".into())
}

pub fn register_tray() -> Result<ServiceStatus, String> {
    if !service_management::available() {
        return Err("ServiceManagement is unavailable on this macOS version".into());
    }
    service_management::remove_legacy_launch_agent(TRAY_SERVICE_ID);
    ensure_registered(
        ServiceKind::LoginItem,
        TRAY_SERVICE_ID,
        TRAY_SERVICE_ID,
        "Menu bar helper",
    )
}

pub fn unregister_tray() -> Result<(), String> {
    if !service_management::available() {
        return Ok(());
    }
    service_management::unregister(ServiceKind::LoginItem, TRAY_SERVICE_ID)
}

pub fn register_widget_updater() -> Result<ServiceStatus, String> {
    if !service_management::available() {
        return Err("ServiceManagement is unavailable on this macOS version".into());
    }
    service_management::remove_legacy_launch_agent("com.ccspace.widget-updater");
    service_management::remove_legacy_launch_agent(WIDGET_UPDATER_LABEL);
    ensure_registered(
        ServiceKind::LaunchAgent,
        WIDGET_UPDATER_PLIST,
        WIDGET_UPDATER_LABEL,
        "Widget updater",
    )
}

#[cfg(target_os = "macos")]
pub fn ensure_widget_updater() {
    if cfg!(debug_assertions) || !crate::scheduler::owns_machine_schedule() {
        return;
    }
    match register_widget_updater() {
        Ok(ServiceStatus::RequiresApproval) => {
            log::warn!("widget updater requires approval in System Settings");
        }
        Ok(_) => {}
        Err(error) => log::warn!("widget updater registration failed: {error}"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_widget_updater() {}

#[cfg(target_os = "macos")]
pub fn ensure_tray() {
    if cfg!(debug_assertions) || !crate::scheduler::owns_machine_schedule() {
        return;
    }
    if crate::tray_agent::disabled_marker_path().exists() {
        service_management::remove_legacy_launch_agent(TRAY_SERVICE_ID);
        return;
    }
    match register_tray() {
        Ok(ServiceStatus::RequiresApproval) => {
            log::warn!("menu bar helper requires approval in System Settings");
        }
        Ok(_) => {}
        Err(error) => log::warn!("menu bar helper registration failed: {error}"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_tray() {}

#[tauri::command]
pub fn get_background_service_status() -> BackgroundServiceStatus {
    BackgroundServiceStatus {
        tray: status(ServiceKind::LoginItem, TRAY_SERVICE_ID)
            .as_str()
            .into(),
        widget_updater: status(ServiceKind::LaunchAgent, WIDGET_UPDATER_PLIST)
            .as_str()
            .into(),
    }
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn retry_background_services() -> Result<BackgroundServiceStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        if !crate::scheduler::owns_machine_schedule() {
            return Err(
                "background service recovery is unavailable for an isolated data instance".into(),
            );
        }
        let mut errors = Vec::new();
        if let Err(error) = crate::widget::refresh_snapshot_if_needed() {
            errors.push(error);
        }
        if let Err(error) = register_widget_updater() {
            errors.push(error);
        }
        if !crate::tray_agent::disabled_marker_path().exists() {
            if let Err(error) = register_tray() {
                errors.push(error);
            }
        }

        if errors.is_empty() {
            Ok(get_background_service_status())
        } else {
            Err(errors.join("; "))
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn retry_background_services() -> Result<BackgroundServiceStatus, String> {
    Err("background service recovery is only available on macOS".into())
}

#[tauri::command]
pub fn open_background_item_settings() -> Result<(), String> {
    service_management::open_login_items()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_not_found_service_still_requires_registration() {
        assert!(registration_required(ServiceStatus::NotRegistered));
        assert!(registration_required(ServiceStatus::NotFound));
        assert!(!registration_required(ServiceStatus::Enabled));
        assert!(!registration_required(ServiceStatus::RequiresApproval));
        assert!(!registration_required(ServiceStatus::Unavailable));
        assert!(registration_usable(ServiceStatus::Enabled));
        assert!(registration_usable(ServiceStatus::RequiresApproval));
        assert!(!registration_usable(ServiceStatus::NotRegistered));
        assert!(!registration_usable(ServiceStatus::NotFound));
    }

    #[test]
    fn enabled_but_unhealthy_service_requires_runtime_recovery() {
        assert!(runtime_recovery_required(ServiceStatus::Enabled, false));
        assert!(!runtime_recovery_required(ServiceStatus::Enabled, true));
        assert!(!runtime_recovery_required(
            ServiceStatus::RequiresApproval,
            false
        ));
        assert!(!runtime_recovery_required(
            ServiceStatus::NotRegistered,
            false
        ));
    }

    #[test]
    fn embedded_background_service_definitions_stay_in_sync() {
        const WIDGET_PLIST: &str =
            include_str!("../../src-widget/io.github.zenolab124.monet.widget-updater.plist");
        const WIDGET_BUILD: &str = include_str!("../../src-widget/build.sh");
        const TRAY_INFO: &str = include_str!("../../src-tray/Info.plist");
        const MAIN_ENTITLEMENTS: &str = include_str!("../Monet.entitlements");
        const WIDGET_ENTITLEMENTS: &str =
            include_str!("../../src-widget/MonetWidgetExtension.entitlements");

        assert!(WIDGET_PLIST.contains(WIDGET_UPDATER_LABEL));
        assert!(WIDGET_PLIST.contains("Contents/MacOS/widget-updater"));
        assert!(WIDGET_BUILD.contains(WIDGET_UPDATER_PLIST));
        assert!(TRAY_INFO.contains(TRAY_SERVICE_ID));
        assert!(!MAIN_ENTITLEMENTS.contains("application-groups"));
        assert!(!WIDGET_ENTITLEMENTS.contains("temporary-exception"));
    }
}
