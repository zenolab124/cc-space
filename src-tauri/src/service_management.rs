use std::ffi::{CStr, CString};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceKind {
    LoginItem = 0,
    LaunchAgent = 1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
    Unavailable,
}

impl ServiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRegistered => "notRegistered",
            Self::Enabled => "enabled",
            Self::RequiresApproval => "requiresApproval",
            Self::NotFound => "notFound",
            Self::Unavailable => "unavailable",
        }
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    fn monet_sm_available() -> i32;
    fn monet_sm_status(kind: i32, value: *const std::ffi::c_char) -> i32;
    fn monet_sm_register(
        kind: i32,
        value: *const std::ffi::c_char,
        error: *mut std::ffi::c_char,
        error_capacity: usize,
    ) -> i32;
    fn monet_sm_unregister(
        kind: i32,
        value: *const std::ffi::c_char,
        error: *mut std::ffi::c_char,
        error_capacity: usize,
    ) -> i32;
    fn monet_sm_open_login_items();
}

#[cfg(target_os = "macos")]
fn c_string(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| format!("invalid ServiceManagement identifier: {value:?}"))
}

#[cfg(target_os = "macos")]
pub fn available() -> bool {
    unsafe { monet_sm_available() != 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn available() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn status(kind: ServiceKind, value: &str) -> ServiceStatus {
    let Ok(value) = c_string(value) else {
        return ServiceStatus::NotFound;
    };
    let raw = unsafe { monet_sm_status(kind as i32, value.as_ptr()) };
    match raw {
        0 => ServiceStatus::NotRegistered,
        1 => ServiceStatus::Enabled,
        2 => ServiceStatus::RequiresApproval,
        3 => ServiceStatus::NotFound,
        _ => ServiceStatus::Unavailable,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn status(_kind: ServiceKind, _value: &str) -> ServiceStatus {
    ServiceStatus::Unavailable
}

#[cfg(target_os = "macos")]
fn call_service(
    operation: unsafe extern "C" fn(
        i32,
        *const std::ffi::c_char,
        *mut std::ffi::c_char,
        usize,
    ) -> i32,
    kind: ServiceKind,
    value: &str,
) -> Result<(), String> {
    let value = c_string(value)?;
    let mut error = vec![0i8; 1024];
    let result = unsafe { operation(kind as i32, value.as_ptr(), error.as_mut_ptr(), error.len()) };
    if result == 0 {
        return Ok(());
    }

    let message = unsafe { CStr::from_ptr(error.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    Err(if message.is_empty() {
        "ServiceManagement operation failed".into()
    } else {
        message
    })
}

#[cfg(target_os = "macos")]
pub fn register(kind: ServiceKind, value: &str) -> Result<(), String> {
    call_service(monet_sm_register, kind, value)
}

#[cfg(not(target_os = "macos"))]
pub fn register(_kind: ServiceKind, _value: &str) -> Result<(), String> {
    Err("ServiceManagement is only available on macOS".into())
}

#[cfg(target_os = "macos")]
pub fn unregister(kind: ServiceKind, value: &str) -> Result<(), String> {
    if matches!(status(kind, value), ServiceStatus::NotRegistered) {
        return Ok(());
    }
    call_service(monet_sm_unregister, kind, value)
}

#[cfg(not(target_os = "macos"))]
pub fn unregister(_kind: ServiceKind, _value: &str) -> Result<(), String> {
    Err("ServiceManagement is only available on macOS".into())
}

#[cfg(target_os = "macos")]
pub fn open_login_items() -> Result<(), String> {
    if !available() {
        return Err("background item settings require macOS 13 or later".into());
    }
    unsafe { monet_sm_open_login_items() };
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn open_login_items() -> Result<(), String> {
    Err("background item settings are only available on macOS".into())
}

#[cfg(any(target_os = "macos", test))]
fn launchd_service_output_healthy(command_succeeded: bool, output: &str) -> bool {
    command_succeeded
        && !output.contains("needs LWCR update")
        && !output.contains("job state = spawn failed")
        && !output.contains("EX_CONFIG")
}

/// 判断受管后台服务在当前用户的 launchd domain 中是否可用。
/// SMAppService 的 Enabled 只表示注册/授权状态；软件更新替换可执行文件后，
/// 任务可能仍可 print，却因旧 LWCR（代码签名约束）进入 spawn failed。
#[cfg(target_os = "macos")]
pub fn launchd_service_healthy(label: &str) -> bool {
    let uid = unsafe { libc::getuid() };
    let service = format!("gui/{uid}/{label}");
    let Ok(output) = std::process::Command::new("/bin/launchctl")
        .args(["print", &service])
        .output()
    else {
        return false;
    };
    launchd_service_output_healthy(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
    )
}

#[cfg(not(target_os = "macos"))]
pub fn launchd_service_healthy(_label: &str) -> bool {
    false
}

/// 删除旧版本手写到 ~/Library/LaunchAgents 的 plist，避免它继续与
/// SMAppService 的受管注册竞争同一个后台项目。
#[cfg(target_os = "macos")]
pub fn remove_legacy_launch_agent(label: &str) {
    let Some(home) = dirs::home_dir() else { return };
    let plist = home
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));
    // 同名 SMAppService 也会出现在 gui/<uid>/<label>。旧 plist 不存在时
    // 禁止 bootout，否则会误把当前受管服务从 launchd domain 中移除。
    if !plist.exists() {
        return;
    }
    let uid = unsafe { libc::getuid() };
    let service = format!("gui/{uid}/{label}");
    let _ = std::process::Command::new("/bin/launchctl")
        .args(["bootout", &service])
        .output();
    let _ = std::fs::remove_file(plist);
}

#[cfg(not(target_os = "macos"))]
pub fn remove_legacy_launch_agent(_label: &str) {}

#[cfg(test)]
mod tests {
    use super::launchd_service_output_healthy;

    #[test]
    fn launchd_health_rejects_missing_and_failed_services() {
        assert!(!launchd_service_output_healthy(false, ""));
        assert!(!launchd_service_output_healthy(
            true,
            "state = spawn scheduled\njob state = spawn failed"
        ));
        assert!(!launchd_service_output_healthy(
            true,
            "properties = needs LWCR update | has LWCR"
        ));
        assert!(!launchd_service_output_healthy(
            true,
            "last exit code = 78: EX_CONFIG"
        ));
    }

    #[test]
    fn launchd_health_accepts_running_and_idle_services() {
        assert!(launchd_service_output_healthy(
            true,
            "state = running\njob state = running"
        ));
        assert!(launchd_service_output_healthy(
            true,
            "state = not running\nlast exit code = 0"
        ));
    }
}
