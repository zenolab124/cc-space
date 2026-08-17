//! macOS TCC 系统权限检测（设置页权限体检 + runner health-check 共用）。
//! 与 permission.rs（Claude 工具权限桥）无关。
//!
//! 自包含铁律：不依赖 crate::config / tauri —— routine-runner 以 #[path]
//! 方式复用本文件，引入 tauri 会把整个 GUI 依赖链进轻量二进制。
//! 非 macOS 平台所有函数返回 "unknown"，调用侧无需 cfg。

#[cfg(target_os = "macos")]
mod native {
    extern "C" {
        pub fn monet_ae_permission(bundle_id: *const std::os::raw::c_char, ask: bool) -> i32;
        pub fn monet_ax_trusted() -> i32;
        pub fn monet_ax_prompt() -> i32;
        pub fn monet_screen_preflight() -> i32;
        pub fn monet_screen_request() -> i32;
        /// Network.framework TCP 探测：0=可用 1=权限阻止 2=其他网络问题 -1=错误
        pub fn monet_nw_probe(
            host: *const std::os::raw::c_char,
            port: *const std::os::raw::c_char,
            timeout_ms: i32,
            wait_for_grant: bool,
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
fn parse_default_gateway(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != "default" {
            return None;
        }
        let gateway = fields.next()?;
        let _flags = fields.next()?;
        let interface = fields.next()?;
        if interface == "lo0" || interface.starts_with("utun") {
            return None;
        }
        let address = gateway.split('%').next().unwrap_or(gateway);
        address
            .parse::<std::net::IpAddr>()
            .ok()
            .map(|_| gateway.to_string())
    })
}

/// 从完整路由表中选物理接口默认网关，避开 VPN/TUN 默认路由。先尝试 IPv4，
/// 再回退 IPv6；不把“局域网”等同于 RFC1918，兼容企业网和 IPv6-only 网络。
#[cfg(target_os = "macos")]
fn default_gateway() -> Option<String> {
    for family in ["inet", "inet6"] {
        let Ok(output) = std::process::Command::new("/usr/sbin/netstat")
            .args(["-rn", "-f", family])
            .output()
        else {
            continue;
        };
        if let Some(gateway) = parse_default_gateway(&String::from_utf8_lossy(&output.stdout)) {
            return Some(gateway);
        }
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalNetworkTarget {
    pub host: String,
    pub port: String,
}

fn is_local_host(host: &str) -> bool {
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        return match address {
            std::net::IpAddr::V4(address) => {
                address.is_private() || address.is_loopback() || address.is_link_local()
            }
            std::net::IpAddr::V6(address) => {
                let first = address.segments()[0];
                address.is_loopback()
                    || first & 0xfe00 == 0xfc00
                    || first & 0xffc0 == 0xfe80
            }
        };
    }

    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".local") || !host.contains('.')
}

fn local_network_target_from_url(raw: &str) -> Option<LocalNetworkTarget> {
    let url = reqwest::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    if !is_local_host(host) {
        return None;
    }
    Some(LocalNetworkTarget {
        host: host.to_string(),
        port: url.port_or_known_default()?.to_string(),
    })
}

fn local_network_target_from_value(root: &serde_json::Value) -> Option<LocalNetworkTarget> {
    const POINTERS: &[&str] = &[
        "/_ccSpace/connection/baseUrl",
        "/_ccSpace/claude/baseUrl",
        "/_ccSpace/codex/baseUrl",
        "/env/ANTHROPIC_BASE_URL",
        "/env/OPENAI_BASE_URL",
    ];
    POINTERS.iter().find_map(|pointer| {
        root.pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .and_then(local_network_target_from_url)
    })
}

/// 从渠道配置中寻找真实会访问的局域网目标，避免用无关的
/// 固定地址产生假阴性。找不到时由调用方回退到物理网关。
pub fn discover_local_network_target(data_dir: &std::path::Path) -> Option<LocalNetworkTarget> {
    let mut paths = std::fs::read_dir(data_dir.join("channels"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    paths.sort();

    paths.into_iter().find_map(|path| {
        let text = std::fs::read_to_string(path).ok()?;
        let value = serde_json::from_str(&text).ok()?;
        local_network_target_from_value(&value)
    })
}

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(target_os = "macos")]
static LAST_LOCAL_NETWORK_STATUS: AtomicI32 = AtomicI32::new(-2);

#[cfg(target_os = "macos")]
fn local_network_status(code: i32) -> &'static str {
    match code {
        0 => "granted",
        1 => "denied",
        2 => "unknown",
        _ => "unknown",
    }
}

#[cfg(target_os = "macos")]
fn probe_local_network(
    data_dir: &std::path::Path,
    wait_for_grant: bool,
    timeout_ms: i32,
) -> &'static str {
    const DISCARD_PORT: &str = "9";

    let target = discover_local_network_target(data_dir).or_else(|| {
        default_gateway().map(|host| LocalNetworkTarget {
            host,
            port: DISCARD_PORT.to_string(),
        })
    });
    let Some(target) = target else {
        return "unknown";
    };
    let (Ok(host_c), Ok(port_c)) = (
        std::ffi::CString::new(target.host.as_str()),
        std::ffi::CString::new(target.port.as_str()),
    ) else {
        return "unknown";
    };

    let code = unsafe {
        native::monet_nw_probe(host_c.as_ptr(), port_c.as_ptr(), timeout_ms, wait_for_grant)
    };
    LAST_LOCAL_NETWORK_STATUS.store(code, Ordering::Relaxed);
    local_network_status(code)
}

/// 用户明确触发的快速检测。首次运行可能触发系统本地网络提示。
#[cfg(target_os = "macos")]
pub fn check_local_network(data_dir: &std::path::Path) -> &'static str {
    probe_local_network(data_dir, false, 2_000)
}

/// 用户点击“检测并授权”后调用。保持连接存活，等待系统授权结果。
#[cfg(target_os = "macos")]
pub fn request_local_network(data_dir: &std::path::Path) -> &'static str {
    probe_local_network(data_dir, true, 30_000)
}

/// 只读本进程本次运行中最近一次探测结果，不触发系统提示。
#[cfg(target_os = "macos")]
pub fn cached_local_network() -> &'static str {
    match LAST_LOCAL_NETWORK_STATUS.load(Ordering::Relaxed) {
        -2 => "unverified",
        code => local_network_status(code),
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
pub fn check_local_network(_data_dir: &std::path::Path) -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn request_local_network(_data_dir: &std::path::Path) -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn cached_local_network() -> &'static str {
    "unknown"
}

#[cfg(not(target_os = "macos"))]
pub fn check_full_disk_access() -> &'static str {
    "unknown"
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{
        discover_local_network_target, local_network_target_from_url, parse_default_gateway,
        LocalNetworkTarget,
    };

    #[test]
    fn chooses_physical_ipv4_gateway_after_tunnel_default() {
        let routes = "\
Destination Gateway Flags Netif\n\
default link#26 UCSg utun7\n\
default 203.0.113.1 UGScIg en0\n";
        assert_eq!(
            parse_default_gateway(routes).as_deref(),
            Some("203.0.113.1")
        );
    }

    #[test]
    fn accepts_scoped_ipv6_gateway() {
        let routes = "\
Destination Gateway Flags Netif\n\
default fe80::1%en7 UGcIg en7\n";
        assert_eq!(
            parse_default_gateway(routes).as_deref(),
            Some("fe80::1%en7")
        );
    }

    #[test]
    fn recognizes_local_channel_urls() {
        assert_eq!(
            local_network_target_from_url("http://10.23.45.67:8080/v1"),
            Some(LocalNetworkTarget {
                host: "10.23.45.67".into(),
                port: "8080".into(),
            })
        );
        assert_eq!(
            local_network_target_from_url("https://gateway.local/api"),
            Some(LocalNetworkTarget {
                host: "gateway.local".into(),
                port: "443".into(),
            })
        );
        assert_eq!(local_network_target_from_url("https://example.com"), None);
    }

    #[test]
    fn discovers_target_from_channel_directory() {
        let dir =
            std::env::temp_dir().join(format!("monet-local-network-test-{}", std::process::id()));
        let channels = dir.join("channels");
        std::fs::create_dir_all(&channels).unwrap();
        std::fs::write(
            channels.join("local.json"),
            r#"{"_ccSpace":{"connection":{"baseUrl":"http://localhost:11434/v1"}}}"#,
        )
        .unwrap();

        assert_eq!(
            discover_local_network_target(&dir),
            Some(LocalNetworkTarget {
                host: "localhost".into(),
                port: "11434".into(),
            })
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
