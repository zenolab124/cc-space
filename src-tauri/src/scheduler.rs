use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config;
use crate::routines::RoutineDefinition;

/// 机器级调度资源（launchd LaunchAgents / pmset 唤醒 / schtasks）是单例的，
/// 只归「默认数据目录」的实例所有。数据目录被 MONET_DATA_DIR 重定向的实例
/// （测试/多实例场景）读的是另一套 routines，若允许其装卸，会把默认实例
/// 注册的 agent 当孤儿清掉、再把自己的注册进真实系统——双向污染
pub fn owns_machine_schedule() -> bool {
    std::env::var("MONET_DATA_DIR").map_or(true, |v| v.trim().is_empty())
}

/// dev 构建不触碰系统调度注册面（launchd/schtasks）：cargo target 目录恰好有
/// 同名 runner 二进制，dev 会把 debug 产物拷进稳定路径并改写注册，回正式 .app
/// 又全部翻转重装——每次语境切换都重复弹系统后台项通知。
/// 显式卸载（删除任务）不门控：删除意图在 dev 下也应立即生效。
pub fn dev_build() -> bool {
    cfg!(debug_assertions)
}

pub fn register_routine(routine: &RoutineDefinition, runner_path: &Path) -> Result<(), String> {
    if !owns_machine_schedule() || dev_build() {
        return Ok(());
    }
    platform::register(routine, runner_path)
}

pub fn unregister_routine(routine_id: &str) -> Result<(), String> {
    if !owns_machine_schedule() {
        return Ok(());
    }
    platform::unregister(routine_id)
}

pub fn sync_all(routines: &[RoutineDefinition], runner_path: &Path) -> Result<(), String> {
    if !owns_machine_schedule() || dev_build() {
        return Ok(());
    }
    let known_ids: std::collections::HashSet<&str> =
        routines.iter().map(|r| r.id.as_str()).collect();
    platform::cleanup_orphans(&known_ids);

    for routine in routines {
        if routine.enabled {
            if !platform::is_registered(&routine.id) {
                // WARN 档：注册面变更（新装/重装）会触发系统后台项通知，留第一现场
                log::warn!("routine agent missing, registering: {}", routine.id);
                platform::register(routine, runner_path)?;
            } else if platform::needs_update(routine, runner_path) {
                log::warn!("routine agent outdated, reinstalling: {}", routine.id);
                let _ = platform::unregister(&routine.id);
                platform::register(routine, runner_path)?;
            }
        } else if platform::is_registered(&routine.id) {
            platform::unregister(&routine.id)?;
        }
    }
    Ok(())
}

pub fn runner_binary_path() -> PathBuf {
    config::data_dir().join("bin").join(runner_bin_name())
}

pub fn installed_runner_supports_environment_snapshot() -> bool {
    let path = runner_binary_path();
    path.exists() && runner_environment_protocol(&path).is_some()
}

static RUNNER_INSTALL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct PreparedRunnerInstall {
    target: PathBuf,
    source_protocol: Option<String>,
    tmp: Option<PathBuf>,
}

impl PreparedRunnerInstall {
    pub fn commit(mut self) -> Result<(), String> {
        let Some(source_protocol) = self.source_protocol.as_deref() else {
            return Ok(());
        };
        if let Some(tmp) = self.tmp.take() {
            if let Err(error) = replace_file(&tmp, &self.target) {
                self.tmp = Some(tmp);
                return Err(format!("failed to install prepared runner: {}", error));
            }
            #[cfg(unix)]
            if let Some(parent) = self.target.parent() {
                std::fs::File::open(parent)
                    .and_then(|dir| dir.sync_all())
                    .map_err(|error| format!("failed to sync runner directory: {}", error))?;
            }
        }

        if !is_codesigned(&self.target) {
            return Err(format!(
                "installed runner signature invalid: {}",
                self.target.display()
            ));
        }
        if runner_environment_protocol(&self.target).as_deref() != Some(source_protocol) {
            return Err(format!(
                "installed runner has an incompatible environment protocol: {}",
                self.target.display()
            ));
        }
        Ok(())
    }
}

impl Drop for PreparedRunnerInstall {
    fn drop(&mut self) {
        if let Some(tmp) = self.tmp.take() {
            let _ = std::fs::remove_file(tmp);
        }
    }
}

pub fn prepare_runner_binary() -> Result<PreparedRunnerInstall, String> {
    let target = runner_binary_path();
    if dev_build() {
        return Ok(PreparedRunnerInstall {
            target,
            source_protocol: None,
            tmp: None,
        });
    }
    let source = bundled_runner_path();
    if !source.exists() {
        return Err(format!("runner binary not found at: {}", source.display()));
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create runner directory: {}", error))?;
    }

    let source_protocol = runner_environment_protocol(&source).ok_or_else(|| {
        "bundled runner does not support routine environment snapshots".to_string()
    })?;
    let needs_install = if target.exists() {
        runner_environment_protocol(&target).as_deref() != Some(source_protocol.as_str())
            || !is_codesigned(&target)
    } else {
        true
    };
    if !needs_install {
        return Ok(PreparedRunnerInstall {
            target,
            source_protocol: Some(source_protocol),
            tmp: None,
        });
    }

    let stem = target
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("monet-routine-runner");
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value))
        .unwrap_or_default();
    let sequence = RUNNER_INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let tmp = target.with_file_name(format!(
        "{}.install-{}-{}{}",
        stem,
        std::process::id(),
        sequence,
        extension
    ));
    let _ = std::fs::remove_file(&tmp);
    std::fs::copy(&source, &tmp)
        .map_err(|error| format!("failed to prepare runner binary: {}", error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("failed to set runner permissions: {}", error));
        }
    }
    // identifier 必须与构建期签名（build.sh 按二进制文件名生成）一致，
    // 否则重签兜底触发时 DR 漂移，用户已授予的 TCC 权限静默失效
    #[cfg(target_os = "macos")]
    crate::signing::sign_with_entitlements(
        &tmp,
        "io.github.zenolab124.monet.monet-routine-runner",
        Some(include_str!("../runner-entitlements.plist")),
    );

    if !is_codesigned(&tmp)
        || runner_environment_protocol(&tmp).as_deref() != Some(source_protocol.as_str())
    {
        let _ = std::fs::remove_file(&tmp);
        return Err("prepared runner failed validation".to_string());
    }
    if let Err(error) = sync_runner_file(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("failed to sync prepared runner: {}", error));
    }

    Ok(PreparedRunnerInstall {
        target,
        source_protocol: Some(source_protocol),
        tmp: Some(tmp),
    })
}

pub fn disable_runner() -> Result<(), String> {
    let path = runner_binary_path();
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path)
        .map_err(|error| format!("failed to disable routine runner: {}", error))?;
    if path.exists() {
        Err("routine runner remains executable".to_string())
    } else {
        Ok(())
    }
}

fn runner_environment_protocol(path: &Path) -> Option<String> {
    use crate::proc_ext::HideConsole;

    let output = std::process::Command::new(path)
        .arg("--environment-protocol")
        .hide_console()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (protocol, version) = value.split_once(':')?;
    if protocol == "1" && !version.is_empty() {
        Some(value)
    } else {
        None
    }
}

#[cfg(windows)]
fn sync_runner_file(path: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()
}

#[cfg(not(windows))]
fn sync_runner_file(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(source, target)
}

#[cfg(target_os = "macos")]
fn is_codesigned(path: &Path) -> bool {
    std::process::Command::new("codesign")
        .args(["--verify", path.to_string_lossy().as_ref()])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(not(target_os = "macos"))]
fn is_codesigned(_path: &Path) -> bool {
    true
}

pub fn bundled_runner_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(runner_bin_name());
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(runner_bin_name())
}

fn runner_bin_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "monet-routine-runner.exe"
    } else {
        "monet-routine-runner"
    }
}

// ---------------------------------------------------------------------------
// Wake schedule management
// ---------------------------------------------------------------------------

pub fn sync_wake_schedule(
    routines: &[RoutineDefinition],
    policy: &str,
) -> crate::wake::SyncOutcome {
    if !owns_machine_schedule() {
        return crate::wake::SyncOutcome::Synced;
    }
    platform::sync_wake(routines, policy)
}

// Windows 专用：schtasks 的 WakeToRun 属性在 register 时需要读取策略；
// macOS 的唤醒策略读取在 routines.rs（wake_policy）
#[cfg(target_os = "windows")]
fn read_wake_policy_file() -> String {
    let path = config::data_dir().join("settings.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("routineWakePolicy")?.as_str().map(String::from))
        .unwrap_or_else(|| "passive".to_string())
}

// ---------------------------------------------------------------------------
// macOS: launchd
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::fs;
    use std::process::Command;

    const LABEL_PREFIX: &str = "io.github.zenolab124.monet.routine.";
    // 旧前缀（CC Space 时期）：仅用于查找/删除兼容，不再新建
    const LEGACY_LABEL_PREFIX: &str = "com.cc-space.routine.";

    fn launch_agents_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library")
            .join("LaunchAgents")
    }

    fn plist_path(routine_id: &str) -> PathBuf {
        launch_agents_dir().join(format!("{}{}.plist", LABEL_PREFIX, routine_id))
    }

    fn legacy_plist_path(routine_id: &str) -> PathBuf {
        launch_agents_dir().join(format!("{}{}.plist", LEGACY_LABEL_PREFIX, routine_id))
    }

    fn label(routine_id: &str) -> String {
        format!("{}{}", LABEL_PREFIX, routine_id)
    }

    pub fn is_registered(routine_id: &str) -> bool {
        // 新旧两套前缀任一在位即视为已注册
        plist_path(routine_id).exists() || legacy_plist_path(routine_id).exists()
    }

    pub fn needs_update(routine: &RoutineDefinition, runner_path: &Path) -> bool {
        let path = plist_path(&routine.id);
        let existing = match fs::read_to_string(&path) {
            Ok(s) => s,
            // 新前缀 plist 不存在：旧前缀在位则需要迁移（重建为新前缀+新路径）
            Err(_) => return true,
        };
        let calendar_intervals = match cron_to_calendar_intervals(&routine.cron_expression) {
            Ok(ci) => ci,
            Err(_) => return false,
        };
        let expected = generate_plist(
            &label(&routine.id),
            runner_path,
            &routine.id,
            &calendar_intervals,
        );
        if existing.trim() != expected.trim() {
            return true;
        }

        // plist 内容不变不代表 launchd 仍能启动任务。软件更新替换同一路径下的
        // runner 后，launchd 可能继续持有旧 LWCR（代码签名约束），服务仍可
        // print 却在触发时以 OS_REASON_CODESIGNING 退出。只恢复真实异常的任务，
        // 避免健康任务每次启动都重注册并反复触发系统后台项目通知。
        !crate::service_management::launchd_service_healthy(&label(&routine.id))
    }

    pub fn cleanup_orphans(known_ids: &std::collections::HashSet<&str>) {
        let agents_dir = launch_agents_dir();
        let entries = match fs::read_dir(&agents_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // 新旧两套前缀都识别；孤儿一律回收（unregister 内部同时清新旧 plist）
            let id = name.strip_suffix(".plist").and_then(|stem| {
                stem.strip_prefix(LABEL_PREFIX)
                    .or_else(|| stem.strip_prefix(LEGACY_LABEL_PREFIX))
            });
            if let Some(id) = id {
                if !known_ids.contains(id) {
                    // WARN 档：登录项清理是注册面破坏性动作，release 日志须留痕
                    log::warn!("cleaning up orphaned routine agent: {}", id);
                    let _ = unregister(id);
                }
            }
        }
    }

    pub fn register(routine: &RoutineDefinition, runner_path: &Path) -> Result<(), String> {
        let path = plist_path(&routine.id);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let calendar_intervals = cron_to_calendar_intervals(&routine.cron_expression)?;
        let plist_content = generate_plist(
            &label(&routine.id),
            runner_path,
            &routine.id,
            &calendar_intervals,
        );

        fs::write(&path, &plist_content)
            .map_err(|e| format!("failed to write plist: {}", e))?;

        let uid = Command::new("id").arg("-u").output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "501".to_string());
        let domain_target = format!("gui/{}", uid);

        let _ = Command::new("launchctl")
            .args(["bootout", &domain_target, &path.to_string_lossy()])
            .output();

        let output = Command::new("launchctl")
            .args(["bootstrap", &domain_target, &path.to_string_lossy()])
            .output()
            .map_err(|e| format!("launchctl bootstrap failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("already bootstrapped") {
                return Err(format!("launchctl bootstrap error: {}", stderr));
            }
        }

        Ok(())
    }

    pub fn unregister(routine_id: &str) -> Result<(), String> {
        // 新旧两套前缀都尝试卸载：bootout 各自 plist 后删文件（旧任务照常可删）
        let uid = Command::new("id").arg("-u").output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "501".to_string());
        let domain_target = format!("gui/{}", uid);

        for path in [plist_path(routine_id), legacy_plist_path(routine_id)] {
            let _ = Command::new("launchctl")
                .args(["bootout", &domain_target, &path.to_string_lossy()])
                .output();
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
        }
        Ok(())
    }

    fn generate_plist(
        label: &str,
        runner_path: &Path,
        routine_id: &str,
        calendar_intervals: &str,
    ) -> String {
        let log_path = config::data_dir()
            .join("routines")
            .join("logs")
            .join(routine_id);
        let _ = std::fs::create_dir_all(&log_path);
        let stdout_log = log_path.join("launchd.log");

        // plist 不烘焙 PATH：注册期快照随启动语境（终端/Finder）漂移，内容一变
        // needs_update 即误判重装 → 重复弹系统后台项通知；runner 运行时经
        // path_env::enhanced_path() 自行补齐（claude 子进程的 npx MCP 等需要）
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{runner}</string>
		<string>--routine-id</string>
		<string>{routine_id}</string>
	</array>
{calendar_intervals}
	<key>RunAtLoad</key>
	<true/>
	<key>StandardOutPath</key>
	<string>{stdout_log}</string>
	<key>StandardErrorPath</key>
	<string>{stdout_log}</string>
</dict>
</plist>
"#,
            label = label,
            runner = runner_path.display(),
            routine_id = routine_id,
            calendar_intervals = calendar_intervals,
            stdout_log = stdout_log.display(),
        )
    }

    fn cron_to_calendar_intervals(cron_expr: &str) -> Result<String, String> {
        use cron::Schedule;
        use std::str::FromStr;

        let full = crate::cron_expr::to_quartz_full(cron_expr);
        let schedule = Schedule::from_str(&full)
            .map_err(|e| format!("invalid cron: {}", e))?;

        #[allow(clippy::type_complexity)] // 局部临时元组，提取 type 别名增加反而不直观
        let mut entries: Vec<(u32, u32, Option<u32>, Option<u32>, Option<u32>)> = Vec::new();

        // Sample 366 occurrences (covers a full year cycle) to find the pattern.
        // 采样锚点必须固定历元：以「现在」起采，fallback 分支的截断窗口会随
        // 启动时刻漂移——同一 cron 每次生成不同 plist → needs_update 误判
        // 重装 → 重复弹系统后台项通知。锚定后同一表达式的展开恒定
        use chrono::TimeZone;
        let anchor = chrono::Local
            .with_ymd_and_hms(2024, 1, 1, 0, 0, 0)
            .single()
            .unwrap_or_else(chrono::Local::now);
        for dt in schedule.after(&anchor).take(366) {
            let min = dt.minute();
            let hour = dt.hour();
            let day = dt.day();
            let month = dt.month();
            let weekday = dt.weekday().num_days_from_sunday(); // 0=Sun

            let entry = (min, hour, Some(day), Some(month), Some(weekday));
            if !entries.contains(&entry) {
                entries.push(entry);
            }
            if entries.len() > 200 {
                break;
            }
        }

        // Analyze: if all entries share the same minute+hour and vary only by date,
        // it's a simple weekly/daily/day-of-month pattern.
        // 各分支输出前必须排序：HashSet 迭代序随进程随机，plist 字符串一变，
        // needs_update 即误判 → 每次启动重注册 → 系统重复弹后台项通知
        let all_same_time = entries.iter().all(|e| e.0 == entries[0].0 && e.1 == entries[0].1);
        let unique_weekdays: std::collections::HashSet<_> =
            entries.iter().filter_map(|e| e.4).collect();
        let unique_days: std::collections::HashSet<_> =
            entries.iter().filter_map(|e| e.2).collect();

        // 星期受限判据须先于「日号覆盖 ≥28 = daily」：周节奏 cron（如 * * 1,3）
        // 采样一年日号几乎全覆盖，先判 daily 会把它展开成每天跑
        if all_same_time && unique_weekdays.len() < 7 {
            // Weekly pattern — emit one dict per weekday
            let mut wds: Vec<u32> = unique_weekdays.into_iter().collect();
            wds.sort_unstable();
            let mut intervals = String::from("\t<key>StartCalendarInterval</key>\n\t<array>\n");
            for wd in &wds {
                intervals.push_str(&format!(
                    "\t\t<dict>\n\t\t\t<key>Hour</key>\n\t\t\t<integer>{}</integer>\n\t\t\t<key>Minute</key>\n\t\t\t<integer>{}</integer>\n\t\t\t<key>Weekday</key>\n\t\t\t<integer>{}</integer>\n\t\t</dict>\n",
                    entries[0].1, entries[0].0, wd
                ));
            }
            intervals.push_str("\t</array>");
            return Ok(intervals);
        }

        if all_same_time && unique_days.len() >= 28 {
            // Daily pattern (all days covered) — simplest: just hour+minute
            return Ok(format!(
                "\t<key>StartCalendarInterval</key>\n\t<dict>\n\t\t<key>Hour</key>\n\t\t<integer>{}</integer>\n\t\t<key>Minute</key>\n\t\t<integer>{}</integer>\n\t</dict>",
                entries[0].1, entries[0].0
            ));
        }

        if all_same_time {
            // Day-of-month pattern（如 */2 隔日、1,15 定日）— emit one dict per day。
            // 此前这类会误入 weekly 分支被展开成全七天 = 每天跑，调度语义错误
            let mut days: Vec<u32> = unique_days.into_iter().collect();
            days.sort_unstable();
            let mut intervals = String::from("\t<key>StartCalendarInterval</key>\n\t<array>\n");
            for day in &days {
                intervals.push_str(&format!(
                    "\t\t<dict>\n\t\t\t<key>Hour</key>\n\t\t\t<integer>{}</integer>\n\t\t\t<key>Minute</key>\n\t\t\t<integer>{}</integer>\n\t\t\t<key>Day</key>\n\t\t\t<integer>{}</integer>\n\t\t</dict>\n",
                    entries[0].1, entries[0].0, day
                ));
            }
            intervals.push_str("\t</array>");
            return Ok(intervals);
        }

        // Fallback: high-frequency or complex — use multiple calendar intervals
        // Cap at 48 entries (e.g. every 30 min = 48/day)
        let capped = &entries[..entries.len().min(48)];
        if capped.len() == 1 {
            let e = &capped[0];
            return Ok(format!(
                "\t<key>StartCalendarInterval</key>\n\t<dict>\n\t\t<key>Hour</key>\n\t\t<integer>{}</integer>\n\t\t<key>Minute</key>\n\t\t<integer>{}</integer>\n\t</dict>",
                e.1, e.0
            ));
        }

        let mut intervals = String::from("\t<key>StartCalendarInterval</key>\n\t<array>\n");
        // For sub-hourly patterns, just emit minute values
        let unique_minutes: std::collections::HashSet<_> = entries.iter().map(|e| e.0).collect();
        let unique_hours: std::collections::HashSet<_> = entries.iter().map(|e| e.1).collect();

        if unique_hours.len() == 24 {
            // Every-N-minutes pattern — emit per-minute dicts
            let mut mins: Vec<u32> = unique_minutes.into_iter().collect();
            mins.sort_unstable();
            for min in &mins {
                intervals.push_str(&format!(
                    "\t\t<dict>\n\t\t\t<key>Minute</key>\n\t\t\t<integer>{}</integer>\n\t\t</dict>\n",
                    min
                ));
            }
        } else {
            // 去重后排序输出：采样起点随启动时刻漂移，不排序则同一 cron
            // 每次生成的 plist 顺序都不同
            let mut seen = std::collections::HashSet::new();
            let mut times: Vec<(u32, u32)> = Vec::new();
            for e in capped {
                if seen.insert((e.1, e.0)) {
                    times.push((e.1, e.0));
                }
            }
            times.sort_unstable();
            for (hour, min) in &times {
                intervals.push_str(&format!(
                    "\t\t<dict>\n\t\t\t<key>Hour</key>\n\t\t\t<integer>{}</integer>\n\t\t\t<key>Minute</key>\n\t\t\t<integer>{}</integer>\n\t\t</dict>\n",
                    hour, min
                ));
            }
        }
        intervals.push_str("\t</array>");
        Ok(intervals)
    }

    // -----------------------------------------------------------------------
    // Wake schedule：实现在 crate::wake（pmset schedule 多点 + sudoers 静默）
    // -----------------------------------------------------------------------

    pub fn sync_wake(routines: &[RoutineDefinition], policy: &str) -> crate::wake::SyncOutcome {
        let cron_exprs: Vec<String> = routines
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.cron_expression.clone())
            .collect();
        crate::wake::sync(config::data_dir(), &cron_exprs, policy)
    }

    use chrono::Timelike;
    use chrono::Datelike;
}

// ---------------------------------------------------------------------------
// Windows: Task Scheduler
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use crate::proc_ext::HideConsole;
    use std::fs;
    use std::process::Command;

    fn task_name(routine_id: &str) -> String {
        format!("Monet\\Routine-{}", routine_id)
    }

    // 旧任务名（CC Space 时期）：仅用于查找/删除兼容，不再新建
    fn legacy_task_name(routine_id: &str) -> String {
        format!("CC-Space\\Routine-{}", routine_id)
    }

    fn xml_path(routine_id: &str) -> PathBuf {
        config::data_dir()
            .join("routines")
            .join("tasks")
            .join(format!("{}.xml", routine_id))
    }

    pub fn is_registered(routine_id: &str) -> bool {
        // 新旧两套任务名任一存在即视为已注册
        let query = |tn: &str| {
            Command::new("schtasks").hide_console()
                .args(["/Query", "/TN", tn])
                .output()
                .map_or(false, |o| o.status.success())
        };
        query(&task_name(routine_id)) || query(&legacy_task_name(routine_id))
    }

    pub fn needs_update(_routine: &RoutineDefinition, _runner_path: &Path) -> bool {
        false
    }

    pub fn cleanup_orphans(_known_ids: &std::collections::HashSet<&str>) {}

    pub fn sync_wake(routines: &[RoutineDefinition], policy: &str) -> crate::wake::SyncOutcome {
        let wake = policy == "active";
        let runner_path = super::runner_binary_path();
        for routine in routines.iter().filter(|r| r.enabled) {
            if let Ok(xml) = generate_task_xml(&runner_path, &routine.id, &routine.cron_expression, wake) {
                let xml_file = xml_path(&routine.id);
                if let Some(parent) = xml_file.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::read_to_string(&xml_file).ok().as_deref() == Some(&xml) {
                    continue;
                }
                let _ = fs::write(&xml_file, &xml);
                let _ = Command::new("schtasks").hide_console()
                    .args(["/Create", "/TN", &task_name(&routine.id), "/XML", &xml_file.to_string_lossy(), "/F"])
                    .output();
                // 新任务已建，删除同 id 的旧任务名，避免新旧双份被调度重复执行
                let _ = Command::new("schtasks").hide_console()
                    .args(["/Delete", "/TN", &legacy_task_name(&routine.id), "/F"])
                    .output();
            }
        }
        crate::wake::SyncOutcome::Synced
    }

    pub fn register(routine: &RoutineDefinition, runner_path: &Path) -> Result<(), String> {
        let wake = super::read_wake_policy_file() == "active";
        let xml_file = xml_path(&routine.id);
        if let Some(parent) = xml_file.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let xml = generate_task_xml(runner_path, &routine.id, &routine.cron_expression, wake)?;
        fs::write(&xml_file, &xml).map_err(|e| format!("write task xml: {}", e))?;

        let output = Command::new("schtasks").hide_console()
            .args([
                "/Create",
                "/TN", &task_name(&routine.id),
                "/XML", &xml_file.to_string_lossy(),
                "/F",
            ])
            .output()
            .map_err(|e| format!("schtasks create: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "schtasks error: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // 新任务已建，删除同 id 的旧任务名，避免新旧双份被调度重复执行
        let _ = Command::new("schtasks").hide_console()
            .args(["/Delete", "/TN", &legacy_task_name(&routine.id), "/F"])
            .output();
        Ok(())
    }

    pub fn unregister(routine_id: &str) -> Result<(), String> {
        // 新旧两套任务名都尝试删除（旧任务照常可删）
        let _ = Command::new("schtasks").hide_console()
            .args(["/Delete", "/TN", &task_name(routine_id), "/F"])
            .output();
        let _ = Command::new("schtasks").hide_console()
            .args(["/Delete", "/TN", &legacy_task_name(routine_id), "/F"])
            .output();
        let xml = xml_path(routine_id);
        if xml.exists() {
            let _ = fs::remove_file(&xml);
        }
        Ok(())
    }

    fn generate_task_xml(
        runner_path: &Path,
        routine_id: &str,
        cron_expr: &str,
        wake: bool,
    ) -> Result<String, String> {
        use cron::Schedule;
        use std::str::FromStr;

        let full = crate::cron_expr::to_quartz_full(cron_expr);
        let schedule = Schedule::from_str(&full).map_err(|e| format!("invalid cron: {}", e))?;
        let next = schedule.upcoming(chrono::Local).next().ok_or("no next run")?;
        let start_time = next.format("%Y-%m-%dT%H:%M:%S").to_string();
        let wake_str = if wake { "true" } else { "false" };

        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{start_time}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
  </Triggers>
  <Settings>
    <StartWhenAvailable>true</StartWhenAvailable>
    <WakeToRun>{wake}</WakeToRun>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
  </Settings>
  <Actions>
    <Exec>
      <Command>{runner}</Command>
      <Arguments>--routine-id {routine_id}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
            start_time = start_time,
            runner = runner_path.display(),
            routine_id = routine_id,
            wake = wake_str,
        ))
    }
}

// ---------------------------------------------------------------------------
// Linux: systemd user timer
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn unit_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("systemd")
            .join("user")
    }

    fn service_name(routine_id: &str) -> String {
        format!("monet-routine-{}.service", routine_id)
    }

    fn timer_name(routine_id: &str) -> String {
        format!("monet-routine-{}.timer", routine_id)
    }

    // 旧 unit 名（CC Space 时期）：仅用于查找/删除兼容，不再新建
    fn legacy_service_name(routine_id: &str) -> String {
        format!("cc-space-routine-{}.service", routine_id)
    }

    fn legacy_timer_name(routine_id: &str) -> String {
        format!("cc-space-routine-{}.timer", routine_id)
    }

    pub fn is_registered(routine_id: &str) -> bool {
        // 新旧两套 timer 名任一存在即视为已注册
        unit_dir().join(timer_name(routine_id)).exists()
            || unit_dir().join(legacy_timer_name(routine_id)).exists()
    }

    pub fn needs_update(_routine: &RoutineDefinition, _runner_path: &Path) -> bool {
        false
    }

    pub fn cleanup_orphans(_known_ids: &std::collections::HashSet<&str>) {}

    pub fn sync_wake(_routines: &[RoutineDefinition], _policy: &str) -> crate::wake::SyncOutcome {
        crate::wake::SyncOutcome::Synced
    }

    pub fn register(routine: &RoutineDefinition, runner_path: &Path) -> Result<(), String> {
        let dir = unit_dir();
        let _ = fs::create_dir_all(&dir);

        let service = format!(
            "[Unit]\nDescription=Monet Routine: {name}\n\n[Service]\nType=oneshot\nExecStart={runner} --routine-id {id}\n",
            name = routine.name,
            runner = runner_path.display(),
            id = routine.id,
        );

        let on_calendar = cron_to_systemd_calendar(&routine.cron_expression)?;
        let timer = format!(
            "[Unit]\nDescription=Timer for Monet Routine: {name}\n\n[Timer]\nOnCalendar={cal}\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n",
            name = routine.name,
            cal = on_calendar,
        );

        fs::write(dir.join(service_name(&routine.id)), &service)
            .map_err(|e| format!("write service: {}", e))?;
        fs::write(dir.join(timer_name(&routine.id)), &timer)
            .map_err(|e| format!("write timer: {}", e))?;

        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();

        let output = Command::new("systemctl")
            .args(["--user", "enable", "--now", &timer_name(&routine.id)])
            .output()
            .map_err(|e| format!("systemctl enable: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "systemctl enable error: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    pub fn unregister(routine_id: &str) -> Result<(), String> {
        // 新旧两套 unit 名都尝试停用+删除（旧任务照常可删）
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", &timer_name(routine_id)])
            .output();
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", &legacy_timer_name(routine_id)])
            .output();

        let dir = unit_dir();
        let _ = fs::remove_file(dir.join(service_name(routine_id)));
        let _ = fs::remove_file(dir.join(timer_name(routine_id)));
        let _ = fs::remove_file(dir.join(legacy_service_name(routine_id)));
        let _ = fs::remove_file(dir.join(legacy_timer_name(routine_id)));

        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();

        Ok(())
    }

    fn cron_to_systemd_calendar(cron_expr: &str) -> Result<String, String> {
        let parts: Vec<&str> = cron_expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!("expected 5-field cron, got {}", parts.len()));
        }
        let (min, hour, dom, _mon, dow) = (parts[0], parts[1], parts[2], parts[3], parts[4]);

        // dow 走 vixie 惯例（0/7=Sun, 1=Mon…6=Sat），systemd OnCalendar 只吃命名
        // 周几（Mon..Sun），需要把 vixie 数字整段展开为命名——含任意范围（如 2-4）
        // 与 step。之前只对 `1-5/0-6/1-7` 三个硬编码范围替换，其他数字范围直接透传
        // 会被 systemd 拒识，routine 在 Linux 上一律建不起来
        let weekday_prefix = if dow != "*" {
            let converted = crate::cron_expr::vixie_dow_to_systemd(dow)
                .ok_or_else(|| format!("invalid dow field for systemd: {}", dow))?;
            format!("{} ", converted)
        } else {
            String::new()
        };

        let day_part = if dom == "*" {
            "*-*-*".to_string()
        } else {
            format!("*-*-{}", dom.replace("*/", "1/"))
        };

        let hour_part = if hour == "*" {
            "*".to_string()
        } else {
            hour.replace("*/", "0/")
        };

        let min_part = if min == "*" {
            "*".to_string()
        } else {
            min.replace("*/", "0/")
        };

        Ok(format!("{}{}:{}:00", weekday_prefix, hour_part, min_part))
    }
}
