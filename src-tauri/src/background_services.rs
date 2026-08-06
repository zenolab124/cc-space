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

fn ensure_registered(kind: ServiceKind, value: &str, label: &str) -> Result<ServiceStatus, String> {
    let current = status(kind, value);
    match current {
        ServiceStatus::Enabled | ServiceStatus::RequiresApproval => Ok(current),
        ServiceStatus::NotRegistered => {
            service_management::register(kind, value)?;
            Ok(status(kind, value))
        }
        ServiceStatus::NotFound => Err(format!("{label} is not present in the app bundle")),
        ServiceStatus::Unavailable => {
            Err("ServiceManagement is unavailable on this macOS version".into())
        }
    }
}

pub fn register_tray() -> Result<ServiceStatus, String> {
    if !service_management::available() {
        return Err("ServiceManagement is unavailable on this macOS version".into());
    }
    service_management::remove_legacy_launch_agent(TRAY_SERVICE_ID);
    ensure_registered(ServiceKind::LoginItem, TRAY_SERVICE_ID, "Menu bar helper")
}

pub fn unregister_tray() -> Result<(), String> {
    if !service_management::available() {
        return Ok(());
    }
    service_management::unregister(ServiceKind::LoginItem, TRAY_SERVICE_ID)
}

#[cfg(target_os = "macos")]
pub fn ensure_widget_updater() {
    if cfg!(debug_assertions) || !crate::scheduler::owns_machine_schedule() {
        return;
    }
    service_management::remove_legacy_launch_agent("com.ccspace.widget-updater");
    service_management::remove_legacy_launch_agent(WIDGET_UPDATER_LABEL);
    match ensure_registered(
        ServiceKind::LaunchAgent,
        WIDGET_UPDATER_PLIST,
        "Widget updater",
    ) {
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
    service_management::remove_legacy_launch_agent(TRAY_SERVICE_ID);
    if crate::tray_agent::disabled_marker_path().exists() {
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

#[tauri::command]
pub fn open_background_item_settings() -> Result<(), String> {
    service_management::open_login_items()
}
