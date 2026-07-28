//! macOS TCC 系统权限检测（设置页权限体检 + runner health-check 共用）。
//! 与 permission.rs（Claude 工具权限桥）无关。
//!
//! 自包含铁律：不依赖 crate::config / tauri —— routine-runner 以 #[path]
//! 方式复用本文件，引入 tauri 会把整个 GUI 依赖链进轻量二进制。
//! 非 macOS 平台所有函数返回 "unknown"，调用侧无需 cfg。

#[cfg(target_os = "macos")]
mod native {
    extern "C" {
        pub fn monet_ae_permission(
            bundle_id: *const std::os::raw::c_char,
            ask: bool,
        ) -> i32;
        pub fn monet_ax_trusted() -> i32;
        pub fn monet_ax_prompt() -> i32;
        pub fn monet_screen_preflight() -> i32;
        pub fn monet_screen_request() -> i32;
        /// Network.framework TCP 探测：0=可达 1=静默失败 -1=错误
        pub fn monet_nw_probe(
            host: *const std::os::raw::c_char,
            port: *const std::os::raw::c_char,
            timeout_ms: i32,
        ) -> i32;
    }
}

#[cfg(target_os = "macos")]
fn status_name(code: i32) -> &'static str {
    match code {
        0 => "granted",
        1 => "denied",
        2 => "undetermined",
        3 => "targetNotRunning",
        _ => "unknown",
    }
}

/// 对目标 app 的自动化（Apple Events）权限。ask=false 纯查询零弹窗；
/// ask=true 未决时弹系统授权窗（阻塞至用户响应，调用方放 blocking 线程）
#[cfg(target_os = "macos")]
pub fn check_automation(bundle_id: &str, ask: bool) -> &'static str {
    let Ok(c) = std::ffi::CString::new(bundle_id) else {
        return "unknown";
    };
    status_name(unsafe { native::monet_ae_permission(c.as_ptr(), ask) })
}

#[cfg(target_os = "macos")]
pub fn check_accessibility() -> &'static str {
    status_name(unsafe { native::monet_ax_trusted() })
}

#[cfg(target_os = "macos")]
pub fn check_screen_capture() -> &'static str {
    status_name(unsafe { native::monet_screen_preflight() })
}

/// 屏幕录制授权请求：未决时弹窗；已 denied 不再弹（需深链系统设置）
#[cfg(target_os = "macos")]
pub fn request_screen_capture() -> &'static str {
    status_name(unsafe { native::monet_screen_request() })
}

/// 辅助功能授权引导：把本进程加入系统设置列表并弹引导窗
#[cfg(target_os = "macos")]
pub fn prompt_accessibility() -> &'static str {
    status_name(unsafe { native::monet_ax_prompt() })
}

#[cfg(target_os = "macos")]
fn is_private_v4(ip: &str) -> bool {
    let mut it = ip.split('.');
    let (Some(a), Some(b)) = (it.next(), it.next()) else {
        return false;
    };
    let (Ok(a), Ok(b)) = (a.parse::<u8>(), b.parse::<u8>()) else {
        return false;
    };
    // 只认 RFC1918，排除 TUN 常用的 198.18/100.64 等运营商/测试段
    a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

/// 局域网探测目标：取物理网卡的默认网关。它必然在局域网内且必然存在，
/// 不依赖用户配置了什么设备。
///
/// 不能用 `route -n get default`：开着 VPN/TUN 时默认路由指向 utunN，
/// 那条路由没有 gateway 字段，取到的是空值——而"有 TUN"恰恰是本检测最需要
/// 生效的场景。改从完整路由表里挑第一条网关为 RFC1918 地址的默认路由。
#[cfg(target_os = "macos")]
fn default_gateway() -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/netstat")
        .args(["-rn", "-f", "inet"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.split_whitespace().next() == Some("default"))
        .filter_map(|l| l.split_whitespace().nth(1))
        .find(|gw| is_private_v4(gw))
        .map(|s| s.to_string())
}

/// BSD socket 对照组。收到 RST（ConnectionRefused/Reset）同样算通——
/// 对端明确拒绝说明包已出网，正是我们要的「链路可达」信号。
#[cfg(target_os = "macos")]
fn bsd_reachable(host: &str, port: u16, timeout: std::time::Duration) -> bool {
    use std::io::ErrorKind;
    use std::net::{SocketAddr, TcpStream};
    let Ok(addr) = format!("{}:{}", host, port).parse::<SocketAddr>() else {
        return false;
    };
    match TcpStream::connect_timeout(&addr, timeout) {
        Ok(_) => true,
        Err(e) => matches!(
            e.kind(),
            ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset
        ),
    }
}

/// 本地网络权限：Apple 不提供查询 API，用双路对照推断。
///
/// 旧实现用 UDP 组播（mDNS）+ `send_to` 成败判定，三重不可靠：
/// ① 走 BSD socket，而本地网络隐私只管辖 Network.framework，测的是永远不出问题的路；
/// ② 用组播推断单播 TCP，两者判定路径不同；
/// ③ `send_to` 成败受组播路由状态影响（有 TUN 时尤其不稳），与权限无强相关。
/// 实测同一时刻它两个方向都会错：主进程报 granted，子进程跑同样逻辑报 denied。
///
/// 现在改为对同一目标跑两条路径，用对照消除网络噪声：
///   NW 通                → granted
///   NW 不通 + BSD 通     → denied（链路本身没问题，只有受管路径被挡 = 权限）
///   两条都不通           → unknown（网络/目标问题，无法判定权限）
#[cfg(target_os = "macos")]
pub fn check_local_network() -> &'static str {
    const PORT: u16 = 80;
    const TIMEOUT_MS: i32 = 1500;

    let Some(gw) = default_gateway() else {
        return "unknown";
    };
    let (Ok(host_c), Ok(port_c)) = (
        std::ffi::CString::new(gw.as_str()),
        std::ffi::CString::new(PORT.to_string()),
    ) else {
        return "unknown";
    };

    let nw_ok = unsafe { native::monet_nw_probe(host_c.as_ptr(), port_c.as_ptr(), TIMEOUT_MS) } == 0;
    if nw_ok {
        return "granted";
    }
    let bsd_ok = bsd_reachable(&gw, PORT, std::time::Duration::from_millis(TIMEOUT_MS as u64));
    if bsd_ok {
        "denied"
    } else {
        "unknown"
    }
}

/// 完全磁盘访问没有查询 API，试读 FDA 保护路径推断：
/// 明确的 PermissionDenied → denied；能读 → granted；路径不存在等 → 换下一个
#[cfg(target_os = "macos")]
pub fn check_full_disk_access() -> &'static str {
    let Some(home) = dirs::home_dir() else {
        return "unknown";
    };
    let probes = [
        // 系统级 TCC.db：必然存在且必受 FDA 保护，最可靠的探针
        std::path::PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Safari"),
    ];
    for p in probes {
        let readable = if p.extension().is_some() {
            std::fs::File::open(&p).map(|_| ())
        } else {
            std::fs::read_dir(&p).map(|_| ())
        };
        match readable {
            Ok(()) => return "granted",
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return "denied",
            Err(_) => continue,
        }
    }
    "unknown"
}

// --- 非 macOS stub：体检功能仅 macOS，其他平台一律 unknown ---

#[cfg(not(target_os = "macos"))]
pub fn check_automation(_bundle_id: &str, _ask: bool) -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn check_accessibility() -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn check_screen_capture() -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn request_screen_capture() -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn prompt_accessibility() -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn check_local_network() -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn check_full_disk_access() -> &'static str {
    "unknown"
}
