use std::path::PathBuf;

const SNAPSHOT_FILE_NAME: &str = "widget-data.json";

#[cfg(target_os = "macos")]
extern "C" {
    fn monet_app_group_is_configured() -> bool;
    fn monet_app_group_container_path() -> *mut std::ffi::c_char;
    fn monet_app_group_free_path(path: *mut std::ffi::c_char);
}

/// 返回 Widget 与主应用共同使用的 App Group 快照路径。
///
/// `Ok(None)` 表示当前构建尚未接入 Developer ID，调用方只维护本地备份；
/// `Err` 表示构建已声明 App Group，但系统没有授权该签名访问容器。
pub fn shared_snapshot_path() -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "macos")]
    unsafe {
        if !monet_app_group_is_configured() {
            return Ok(None);
        }

        let raw_path = monet_app_group_container_path();
        if raw_path.is_null() {
            return Err("App Group is configured but its container is unavailable".into());
        }
        let path = std::ffi::CStr::from_ptr(raw_path)
            .to_string_lossy()
            .into_owned();
        monet_app_group_free_path(raw_path);
        return Ok(Some(PathBuf::from(path).join(SNAPSHOT_FILE_NAME)));
    }

    #[cfg(not(target_os = "macos"))]
    Ok(None)
}
