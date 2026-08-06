// 菜单栏常驻依赖 launchd Helper App 架构，仅 macOS 提供；
// 其他平台导出同名接口但报不支持，前端按平台隐藏入口。
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
const LAUNCH_AGENT_LABEL: &str = "io.github.zenolab124.monet.tray";

/// 用户「退出菜单栏」意图的持久化标记。
/// 存在 = 用户主动退过 tray，主应用启动不再自动拉起；
/// 设置页「启动菜单栏」按钮会清除它。monet-tray 退出时写入（双端共享此路径约定）。
#[cfg(target_os = "macos")]
pub fn disabled_marker_path() -> PathBuf {
    crate::config::data_dir().join("tray-disabled")
}

#[cfg(target_os = "macos")]
fn plist_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join("Library/LaunchAgents")
            .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
    })
}

#[cfg(target_os = "macos")]
fn tray_bin_path() -> Option<PathBuf> {
    // Helper App: Monet.app/Contents/Library/LoginItems/MonetTray.app/Contents/MacOS/monet-tray
    // 主应用二进制在: Monet.app/Contents/MacOS/app
    std::env::current_exe().ok().and_then(|p| {
        p.parent() // MacOS/
            .and_then(|d| d.parent()) // Contents/
            .map(|contents| {
                contents.join("Library/LoginItems/MonetTray.app/Contents/MacOS/monet-tray")
            })
    })
}

/// 已随本 app 版本完成 tray 换代的标记。版本号不再嵌进 plist——那会让每次
/// 发版都走 bootout+bootstrap 重装、每次都弹系统后台项通知；改为外置标记 +
/// kickstart -k 重启进程换新二进制（重启不重新注册，BTM 静默）
#[cfg(target_os = "macos")]
fn tray_version_marker() -> PathBuf {
    crate::config::data_dir().join("tray-version")
}

#[cfg(target_os = "macos")]
fn build_plist(bin: &std::path::Path) -> String {
    let bin_str = bin.to_string_lossy();
    let log_path = crate::config::data_dir().join("tray.log");
    let log_str = log_path.to_string_lossy();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LAUNCH_AGENT_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{bin_str}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>StandardErrorPath</key>
	<string>{log_str}</string>
</dict>
</plist>"#
    )
}

#[cfg(target_os = "macos")]
fn launchctl_targets() -> (String, String) {
    let uid = unsafe { libc::getuid() };
    let domain = format!("gui/{uid}");
    let service = format!("{domain}/{LAUNCH_AGENT_LABEL}");
    (domain, service)
}

/// 安装/更新 tray LaunchAgent（幂等）。
/// launchctl 有 IO 开销且可能慢，调用方须在非主线程执行。
#[cfg(target_os = "macos")]
pub fn ensure_launch_agent() {
    // 机器级 tray 注册面只归默认数据目录实例（与 scheduler 同判据）：
    // 副实例（MONET_DATA_DIR 重定向）读不到共享版本标记，会误判升级
    // 反复 kickstart -k 重启真 tray
    if !crate::scheduler::owns_machine_schedule() {
        return;
    }
    // macOS 13+ 的 LoginItem 由 SMAppService 管理；旧 LaunchAgent 仅作为
    // macOS 11–12 的兼容路径保留。
    if crate::service_management::available() {
        crate::background_services::ensure_tray();
        return;
    }
    // 用户主动退过 tray → 尊重意图，不装不拉起
    if disabled_marker_path().exists() {
        return;
    }
    install_and_start();
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_launch_agent() {}

/// 菜单栏常驻是否开启（= 用户未主动关闭）。
/// plist 的 RunAtLoad 决定开机自启，此标记决定要不要装/拉起 plist。
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn get_tray_enabled() -> bool {
    !disabled_marker_path().exists()
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn get_tray_enabled() -> bool {
    false
}

/// 设置页开关：开 = 清标记 + 安装 + 拉起；关 = 写标记 + bootout（终止进程）
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn set_tray_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let _ = std::fs::remove_file(disabled_marker_path());
        if crate::service_management::available() {
            return match crate::background_services::register_tray()? {
                crate::service_management::ServiceStatus::Enabled => Ok(()),
                crate::service_management::ServiceStatus::RequiresApproval => {
                    Err("menu bar helper requires approval in System Settings".into())
                }
                status => Err(format!("menu bar helper status: {}", status.as_str())),
            };
        }
        install_and_start();

        // 校验确实起来了，给前端可感知的失败
        let (_, service) = launchctl_targets();
        let ok = Command::new("launchctl")
            .args(["print", &service])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            Ok(())
        } else {
            Err("tray launch failed: service not registered".into())
        }
    } else {
        std::fs::write(disabled_marker_path(), "")
            .map_err(|e| format!("write disabled marker: {e}"))?;
        if crate::service_management::available() {
            crate::service_management::remove_legacy_launch_agent(
                crate::background_services::TRAY_SERVICE_ID,
            );
            return crate::background_services::unregister_tray();
        }
        let (_, service) = launchctl_targets();
        let _ = Command::new("launchctl")
            .args(["bootout", &service])
            .output();
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn set_tray_enabled(_enabled: bool) -> Result<(), String> {
    Err("menu bar tray is only available on macOS".into())
}

#[cfg(target_os = "macos")]
fn install_and_start() {
    let Some(plist_path) = plist_path() else { return };
    let Some(tray_bin) = tray_bin_path() else { return };
    if !tray_bin.exists() {
        // dev 模式（cargo target 目录）无 Helper App 布局，tray 不可用
        log::info!("monet-tray helper not found at {tray_bin:?}, skipping LaunchAgent install");
        return;
    }

    let plist = build_plist(&tray_bin);
    let (domain, service) = launchctl_targets();

    let need_install = std::fs::read_to_string(&plist_path)
        .map(|existing| existing != plist)
        .unwrap_or(true);

    let current_version = env!("CARGO_PKG_VERSION");

    if need_install {
        // plist 内容变化（首装/路径或模板变更）：bootout 杀旧进程 + bootstrap 起新
        // WARN 档：重装触发系统后台项通知，release 日志留第一现场
        log::warn!("tray agent plist changed, reinstalling");
        // 全新 macOS 账号可能没有 LaunchAgents 目录
        if let Some(parent) = plist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = Command::new("launchctl")
            .args(["bootout", &service])
            .output();
        if std::fs::write(&plist_path, &plist).is_ok() {
            let _ = Command::new("launchctl")
                .args(["bootstrap", &domain, &plist_path.to_string_lossy()])
                .output();
            let _ = std::fs::write(tray_version_marker(), current_version);
        }
    } else if std::fs::read_to_string(tray_version_marker())
        .map(|v| v.trim() != current_version)
        .unwrap_or(true)
    {
        // 版本升级：更新只替换了二进制，KeepAlive 常驻的旧版 tray 不会自行换代。
        // kickstart -k 杀旧起新——只重启进程不重新注册，不触发系统后台项通知
        let restarted = Command::new("launchctl")
            .args(["kickstart", "-k", &service])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        // 服务被 bootout 注销过（手动运维等场景）时 kickstart 找不到目标，回退 bootstrap
        let ok = restarted || {
            log::warn!("tray agent kickstart missed (service unregistered), re-bootstrapping");
            Command::new("launchctl")
                .args(["bootstrap", &domain, &plist_path.to_string_lossy()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        if ok {
            let _ = std::fs::write(tray_version_marker(), current_version);
        }
    } else {
        // 同版本：tray 可能被 kill/崩溃后未重启，kickstart 兜底。
        // kickstart 只能踢「已注册」的服务——服务被 bootout 注销过（手动运维等场景,
        // 实测事故）时它找不到目标静默死路,失败即回退 bootstrap 重新注册
        let kicked = Command::new("launchctl")
            .args(["kickstart", &service])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !kicked {
            log::warn!("tray agent kickstart missed (service unregistered), re-bootstrapping");
            let _ = Command::new("launchctl")
                .args(["bootstrap", &domain, &plist_path.to_string_lossy()])
                .output();
        }
    }
}
