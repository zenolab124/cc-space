//! 性能监视：采集 app 相关进程的真实内存足迹（开发者 HUD 数据源）。
//! macOS 用 libproc 读 phys_footprint（与 Xcode/footprint 工具同口径，比 RSS 准），
//! WebKit 辅助进程（WebContent/GPU/Networking 独立于 app 进程树）按 coalition 归属，
//! CLI 子进程（claude 及其 mcp 子孙）按 ppid 树归属。其他平台暂返回空。

use serde::Serialize;

#[derive(Serialize, Clone, Default)]
pub struct ProcMem {
    pub pid: i32,
    pub name: String,
    pub footprint_mb: f64,
}

#[derive(Serialize, Default)]
pub struct PerfStats {
    pub main: ProcMem,
    /// 同 coalition 的 WebKit 辅助进程（WebContent 即 JS 堆 + DOM 所在）
    pub webkit: Vec<ProcMem>,
    /// 子进程树（claude CLI 及其子孙），按树根聚合
    pub cli: Vec<ProcMem>,
    pub total_mb: f64,
    /// 流式事件入口与 IPC 发射累计计数；前端按采样差值显示每秒速率。
    pub streaming: crate::streaming::StreamPerfCounters,
}

#[cfg(target_os = "macos")]
pub fn collect() -> PerfStats {
    macos::collect()
}

#[cfg(not(target_os = "macos"))]
pub fn collect() -> PerfStats {
    PerfStats::default()
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{PerfStats, ProcMem};
    use std::collections::HashMap;
    use std::ffi::c_void;

    const RUSAGE_INFO_V0: i32 = 0;
    const PROC_PIDT_SHORTBSDINFO: i32 = 13;
    const PROC_PIDCOALITIONINFO: i32 = 20;

    /// sys/resource.h struct rusage_info_v0
    #[repr(C)]
    #[derive(Default)]
    struct RusageInfoV0 {
        ri_uuid: [u8; 16],
        ri_user_time: u64,
        ri_system_time: u64,
        ri_pkg_idle_wkups: u64,
        ri_interrupt_wkups: u64,
        ri_pageins: u64,
        ri_wired_size: u64,
        ri_resident_size: u64,
        ri_phys_footprint: u64,
        ri_proc_start_abstime: u64,
        ri_proc_exit_abstime: u64,
    }

    /// sys/proc_info.h struct proc_bsdshortinfo
    #[repr(C)]
    #[derive(Default)]
    struct ProcBsdShortInfo {
        pbsi_pid: u32,
        pbsi_ppid: u32,
        pbsi_pgid: u32,
        pbsi_status: u32,
        pbsi_comm: [u8; 16],
        pbsi_flags: u32,
        pbsi_uid: u32,
        pbsi_gid: u32,
        pbsi_ruid: u32,
        pbsi_rgid: u32,
        pbsi_svuid: u32,
        pbsi_svgid: u32,
        pbsi_rfu: u32,
    }

    /// sys/proc_info.h struct proc_pidcoalitioninfo（COALITION_NUM_TYPES = 2）
    #[repr(C)]
    #[derive(Default)]
    struct ProcPidCoalitionInfo {
        coalition_id: [u64; 2],
        reserved1: u64,
        reserved2: u64,
        reserved3: u64,
    }

    extern "C" {
        fn proc_listallpids(buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut c_void, buffersize: i32) -> i32;
        // libproc.h 形参类型是 rusage_info_t*（即 void**），但调用惯例是把结构体地址
        // 直接 cast 传入、内核向该地址写整个结构（见 Apple top/footprint 源码），
        // 故此处按实际语义声明为 *mut c_void
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut c_void) -> i32;
        fn proc_name(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
    }

    fn footprint_mb(pid: i32) -> f64 {
        let mut info = RusageInfoV0::default();
        let ret = unsafe {
            proc_pid_rusage(pid, RUSAGE_INFO_V0, &mut info as *mut RusageInfoV0 as *mut c_void)
        };
        if ret != 0 {
            return 0.0;
        }
        info.ri_phys_footprint as f64 / 1024.0 / 1024.0
    }

    fn short_info(pid: i32) -> Option<ProcBsdShortInfo> {
        let mut info = ProcBsdShortInfo::default();
        let size = std::mem::size_of::<ProcBsdShortInfo>() as i32;
        let ret = unsafe {
            proc_pidinfo(pid, PROC_PIDT_SHORTBSDINFO, 0, &mut info as *mut _ as *mut c_void, size)
        };
        (ret == size).then_some(info)
    }

    fn coalition_of(pid: i32) -> Option<u64> {
        let mut info = ProcPidCoalitionInfo::default();
        let size = std::mem::size_of::<ProcPidCoalitionInfo>() as i32;
        let ret = unsafe {
            proc_pidinfo(pid, PROC_PIDCOALITIONINFO, 0, &mut info as *mut _ as *mut c_void, size)
        };
        (ret == size).then_some(info.coalition_id[0])
    }

    /// proc_name 返回最长 32 字节的进程名（足以区分 com.apple.WebKit.* 三种）
    fn name_of(pid: i32) -> String {
        let mut buf = [0u8; 64];
        let len = unsafe { proc_name(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        if len <= 0 {
            return String::new();
        }
        String::from_utf8_lossy(&buf[..len as usize]).into_owned()
    }

    fn all_pids() -> Vec<i32> {
        let count = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return vec![];
        }
        // 留余量：快照与读取之间可能有新进程
        let mut pids = vec![0i32; count as usize + 64];
        let bytes = (pids.len() * std::mem::size_of::<i32>()) as i32;
        let n = unsafe { proc_listallpids(pids.as_mut_ptr() as *mut c_void, bytes) };
        if n <= 0 {
            return vec![];
        }
        pids.truncate(n as usize);
        pids.retain(|&p| p > 0);
        pids
    }

    pub fn collect() -> PerfStats {
        let my_pid = std::process::id() as i32;
        let my_coalition = coalition_of(my_pid);

        let pids = all_pids();
        let mut ppid_map: HashMap<i32, i32> = HashMap::new();
        for &pid in &pids {
            if let Some(info) = short_info(pid) {
                ppid_map.insert(pid, info.pbsi_ppid as i32);
            }
        }

        // WebKit 辅助进程：名字匹配 + 同 coalition（排除 Safari 等其他 app 的 WebContent）
        let mut webkit: Vec<ProcMem> = vec![];
        if let Some(coal) = my_coalition {
            for &pid in &pids {
                let name = name_of(pid);
                if !name.starts_with("com.apple.WebKit") {
                    continue;
                }
                if coalition_of(pid) != Some(coal) {
                    continue;
                }
                webkit.push(ProcMem {
                    pid,
                    name: name.trim_start_matches("com.apple.WebKit.").to_string(),
                    footprint_mb: footprint_mb(pid),
                });
            }
        }
        webkit.sort_by(|a, b| b.footprint_mb.partial_cmp(&a.footprint_mb).unwrap_or(std::cmp::Ordering::Equal));

        // CLI 子进程树：从 main 出发 BFS ppid 关系；每棵子树聚合到直接子进程上
        let mut children_of: HashMap<i32, Vec<i32>> = HashMap::new();
        for (&pid, &ppid) in &ppid_map {
            children_of.entry(ppid).or_default().push(pid);
        }
        let mut cli: Vec<ProcMem> = vec![];
        for &child in children_of.get(&my_pid).map(|v| v.as_slice()).unwrap_or(&[]) {
            let name = name_of(child);
            if name.starts_with("com.apple.WebKit") {
                continue;
            }
            // 聚合整棵子树的 footprint（claude + 其 mcp 子进程）。
            // visited 防御 pid 复用竞态造成的 ppid 环——概率极低但死循环不可接受
            let mut sum = 0.0;
            let mut visited: std::collections::HashSet<i32> = std::collections::HashSet::new();
            let mut stack = vec![child];
            while let Some(p) = stack.pop() {
                if !visited.insert(p) {
                    continue;
                }
                sum += footprint_mb(p);
                if let Some(kids) = children_of.get(&p) {
                    stack.extend(kids);
                }
            }
            cli.push(ProcMem { pid: child, name, footprint_mb: sum });
        }
        cli.sort_by(|a, b| b.footprint_mb.partial_cmp(&a.footprint_mb).unwrap_or(std::cmp::Ordering::Equal));

        let main = ProcMem {
            pid: my_pid,
            name: "main".to_string(),
            footprint_mb: footprint_mb(my_pid),
        };
        let total_mb = main.footprint_mb
            + webkit.iter().map(|p| p.footprint_mb).sum::<f64>()
            + cli.iter().map(|p| p.footprint_mb).sum::<f64>();

        PerfStats {
            main,
            webkit,
            cli,
            total_mb,
            streaming: crate::streaming::stream_perf_counters(),
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// FFI 冒烟测试：结构体错位会静默产出 0 或垃圾值，必须实测
    #[test]
    fn smoke_collect() {
        let stats = super::collect();
        // 测试进程自身 footprint 必然为正且在合理区间（<10GB）
        assert!(stats.main.footprint_mb > 0.5, "footprint = {}", stats.main.footprint_mb);
        assert!(stats.main.footprint_mb < 10_240.0, "footprint = {}", stats.main.footprint_mb);
        assert_eq!(stats.main.pid, std::process::id() as i32);
        println!(
            "main={:.1}MB webkit={} cli={} total={:.1}MB",
            stats.main.footprint_mb,
            stats.webkit.len(),
            stats.cli.len(),
            stats.total_mb
        );
    }
}
