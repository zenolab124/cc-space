// Windows：runner 是 schtasks 触发的后台进程，console 子系统会在每次触发时
// 闪出黑窗；release 切 windows 子系统。stderr 诊断输出本就无人捕获（schtasks
// 不重定向），执行结果走独立 JSON 执行日志，切换后行为不变
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use chrono::Utc;
use serde::Serialize;

// 与主 App 共享同一份定位逻辑源文件（单一事实源）；
// runner 不引 app_lib 整个 crate，避免把 tauri 链进这个轻量二进制。
// allow(dead_code)：runner 只消费 locate_lightweight，其余入口是主 App 用的
#[path = "../claude_locator.rs"]
#[allow(dead_code)]
mod claude_locator;

// Codex CLI 走与主 App 相同的绝对路径探测，禁止依赖 launchd 的贫瘠 PATH。
#[path = "../codex_locator.rs"]
#[allow(dead_code)]
mod codex_locator;

// TCC 权限检测（--health-check 模式），与主 App 共享同一份源文件
#[path = "../tcc.rs"]
#[allow(dead_code)]
mod tcc;

// Routine 结构单一事实源：runner 的 update_routine_state 会整文件重写
// routines.json，本地副本缺字段会抹掉其他写者（UI/MCP）的数据
#[path = "../routine_types.rs"]
#[allow(dead_code)]
mod routine_types;
use routine_types::{RoutineDefinition, RoutineEngine};

// 每次执行按任务引擎继承其当前默认会话渠道。
#[path = "../routine_channel.rs"]
mod routine_channel;

// 两个执行入口共用参数、环境和进程组策略，避免手动运行与 cron 行为漂移。
#[path = "../routine_command.rs"]
mod routine_command;

#[path = "../routine_output.rs"]
mod routine_output;

// 唤醒计划单一事实源：active 模式下 runner 每次执行完续设下一批唤醒点，
// 形成「唤醒 → 跑任务 → 续设 → 回睡」闭环（主 App 不在场时链条不断）
#[path = "../wake.rs"]
#[allow(dead_code)]
mod wake;

// 增强 PATH 单一事实源：launchd 环境 PATH 极简，运行时补齐 homebrew/node
// 等落点（plist 不再烘焙 PATH 快照——注册期快照会陈旧且随启动语境漂移）
#[path = "../path_env.rs"]
mod path_env;

// Cron 表达式单一入口：存储用 vixie 惯例（1=Mon），cron crate 用 Quartz
// （1=Sun），本模块负责把 dow 字段映射后再交给 cron crate。runner 与主 App
// 共享一份实现（wake.rs 亦引用 crate::cron_expr::to_quartz_full）
#[path = "../cron_expr.rs"]
#[allow(dead_code)]
mod cron_expr;

// 运行标记单一事实源：spawn claude 后写入、收尾删除，主 App 据此
// 展示「运行中」并实现终止（stop_routine 置 cancelled 后杀进程组）
#[path = "../routine_run.rs"]
#[allow(dead_code)]
mod routine_run;

// Routine 执行环境由主 App 在启动同步时写快照，独立 runner 读取；
// 避免 launchd 丢失 App 启动时的 MONET_CLAUDE_ROOT / CLAUDE_CONFIG_DIR
#[path = "../routine_env.rs"]
mod routine_env;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionLog {
    routine_id: String,
    #[serde(default)]
    engine: RoutineEngine,
    started_at: String,
    finished_at: Option<String>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    /// 落盘会话 ID（agent cwd 目录下的 <id>.jsonl）。会话落盘设置关闭时为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    /// 用户手动终止（区别于执行失败）
    #[serde(skip_serializing_if = "Option::is_none")]
    cancelled: Option<bool>,
}

fn main() {
    if let Some(code) = run_internal_command() {
        std::process::exit(code);
    }
    if env::args().any(|argument| argument == "--environment-protocol") {
        println!("1:{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // 权限体检模式：由主 App 经 launchd 触发（与真实定时任务相同的 TCC
    // 归因语境），自检后写结果文件退出。--prompt <kind> 时对指定权限发起
    // 请求式调用（弹系统授权窗，用户在权限页面点击驱动）
    if env::args().any(|a| a == "--health-check") {
        let args: Vec<String> = env::args().collect();
        let prompt = args
            .iter()
            .position(|a| a == "--prompt")
            .and_then(|i| args.get(i + 1))
            .cloned();
        run_health_check(prompt.as_deref());
        return;
    }

    let routine_id = parse_args();

    let routines = load_routines();
    let routine = routines
        .iter()
        .find(|r| r.id == routine_id)
        .unwrap_or_else(|| {
            eprintln!("routine not found: {}", routine_id);
            std::process::exit(1);
        })
        .clone();

    if !routine.enabled {
        eprintln!("routine is disabled, skipping");
        std::process::exit(0);
    }

    // File lock to prevent concurrent execution
    let lock_path = routine_run::lock_path(&data_dir(), &routine_id);
    let _ = fs::create_dir_all(lock_path.parent().unwrap());
    let lock_file = fs::File::create(&lock_path).unwrap_or_else(|e| {
        eprintln!("cannot create lock file: {}", e);
        std::process::exit(1);
    });

    use fs2::FileExt;
    if lock_file.try_lock_exclusive().is_err() {
        eprintln!("another instance is running, skipping");
        std::process::exit(0);
    }

    // Dedup: check if already ran in current cron period
    if should_skip(&routine) {
        eprintln!("already ran in current period, skipping");
        std::process::exit(0);
    }

    // Execute：launchd 环境贫瘠，只走轻量探测（手动配置/缓存/候选扫描），
    // login shell 重探测由主 App 负责并通过缓存共享结果
    let started_at = Utc::now().to_rfc3339();

    // 会话落盘（与 Agent 能力同一设置）：落盘时指定 session id 并记入执行日志，
    // 供事后在 agent cwd 目录定位完整会话
    let persist = agent_session_persist();
    let session_id = uuid::Uuid::new_v4().to_string();

    let output = routine_env::read_environment(&data_dir())
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("routine environment unavailable: {}", error),
            )
        })
        .and_then(|environment| {
            let executable = if routine.engine.is_claude_code() {
                claude_locator::locate_lightweight().map(|located| located.path)
            } else if routine.engine.is_codex() {
                codex_locator::locate()
            } else {
                Err(format!(
                    "unsupported routine engine: {}/{}",
                    routine.engine.engine_id, routine.engine.instance_id
                ))
            }
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::NotFound, error))?;
            let cwd = agent_cwd();
            let path = path_env::enhanced_path();
            let auth_executable = std::env::current_exe()?;
            let channel = routine_channel::resolve(
                &data_dir(),
                &routine.engine,
                &session_id,
                &auth_executable,
            )
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
            let result = (|| {
                let mut cmd = routine_command::build_routine_command(
                    routine_command::RoutineCommandSpec {
                        engine: &routine.engine,
                        executable: &executable,
                        prompt: &routine.prompt,
                        session_id: &session_id,
                        persist_session: persist,
                        cwd: &cwd,
                        path_env: &path,
                        claude_config_dir: environment.claude_config_dir.as_deref(),
                        codex_home: environment.codex_home.as_deref(),
                        channel: &channel,
                    },
                )
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
                match cmd.spawn() {
                    Ok(child) => {
                        // spawn 成功即写运行标记：主 App 终止能力与状态展示的事实源
                        routine_run::write_marker(
                            &data_dir(),
                            &routine_id,
                            &routine_run::RunningMarker {
                                pid: child.id(),
                                started_at: started_at.clone(),
                                source: "cron".to_string(),
                                cancelled: false,
                            },
                        );
                        child.wait_with_output()
                    }
                    Err(error) => Err(error),
                }
            })();
            channel.cleanup();
            result
        });

    // 收尾前读取终止标志：主 App stop_routine 杀进程前会置位 cancelled
    let cancelled = routine_run::read_marker(&data_dir(), &routine_id)
        .is_some_and(|m| m.cancelled);
    routine_run::remove_marker(&data_dir(), &routine_id);

    let finished_at = Utc::now().to_rfc3339();

    let log = match output {
        Ok(out) => ExecutionLog {
            routine_id: routine_id.clone(),
            engine: routine.engine.clone(),
            started_at: started_at.clone(),
            finished_at: Some(finished_at),
            exit_code: out.status.code(),
            stdout: truncate(
                &routine_output::normalize_routine_stdout(&routine.engine, &out.stdout),
                10240,
            ),
            stderr: truncate(&String::from_utf8_lossy(&out.stderr), 4096),
            session_id: (persist && routine.engine.is_claude_code()).then(|| session_id.clone()),
            cancelled: cancelled.then_some(true),
        },
        Err(e) => ExecutionLog {
            routine_id: routine_id.clone(),
            engine: routine.engine.clone(),
            started_at: started_at.clone(),
            finished_at: Some(finished_at),
            exit_code: Some(-1),
            stdout: String::new(),
            stderr: format!("spawn failed: {}", e),
            session_id: None,
            cancelled: None,
        },
    };

    write_log(&log);
    update_routine_state(&routine_id, &started_at);
    refresh_wake_schedule();
    // 用户刚手动终止 = 人在电脑前，此时把系统睡下去违背在场事实
    if !cancelled {
        maybe_sleep_after_run();
    }
}

/// 续设唤醒计划（必须在回睡之前）。授权不在位时 wake::sync 静默返回，
/// 降级决策留给主 App——runner 无 UI，不弹任何系统框
fn refresh_wake_schedule() {
    if read_wake_policy() != "active" {
        return;
    }
    let cron_exprs: Vec<String> = load_routines()
        .iter()
        .filter(|r| r.enabled)
        .map(|r| r.cron_expression.clone())
        .collect();
    let _ = wake::sync(&data_dir(), &cron_exprs, "active");
}

fn run_health_check(prompt: Option<&str>) {
    // 预热 System Events：open 走 LaunchServices 不需要自动化权限，
    // 避免 AE 查询因目标未运行返回 procNotFound
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open")
            .args(["-g", "-a", "System Events"])
            .output();
        std::thread::sleep(std::time::Duration::from_millis(600));
        // 请求式调用：弹系统授权窗并阻塞至用户响应，随后照常快照
        match prompt {
            Some("automationSystemEvents") => {
                // 不用 AEDeterminePermissionToAutomateTarget(ask=true)：该预检
                // API 在 launchd 直启语境下不产生授权弹窗（policy 放行也一样，
                // 本机实测钉死）。改发真实无害 AE（只读计数），走完整授权路径：
                // 未决时真弹窗，osascript 为子进程、TCC 归因仍是 runner 自身
                let _ = Command::new("/usr/bin/osascript")
                    .args(["-e", "tell application \"System Events\" to count processes"])
                    .output();
            }
            Some("accessibility") => {
                let _ = tcc::prompt_accessibility();
            }
            Some("screenCapture") => {
                let _ = tcc::request_screen_capture();
            }
            Some("localNetwork") => {
                let _ = tcc::check_local_network();
            }
            _ => {}
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = prompt;
    let result = serde_json::json!({
        "checkedAt": Utc::now().to_rfc3339(),
        "permissions": {
            "automationSystemEvents": tcc::check_automation("com.apple.systemevents", false),
            "accessibility": tcc::check_accessibility(),
            "screenCapture": tcc::check_screen_capture(),
            "fullDiskAccess": tcc::check_full_disk_access(),
            "localNetwork": tcc::check_local_network(),
        },
    });
    // 结果路径与主 App 读取侧硬编码一致（launchd 语境无 MONET_DATA_DIR）
    let dir = dirs::home_dir().unwrap_or_default().join(".monet");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(
        dir.join("permissions-runner.json"),
        serde_json::to_string_pretty(&result).unwrap_or_default(),
    );
}

fn parse_args() -> String {
    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--routine-id" && i + 1 < args.len() {
            return args[i + 1].clone();
        }
        i += 1;
    }
    eprintln!("usage: monet-routine-runner --routine-id <uuid>");
    std::process::exit(1);
}

fn run_internal_command() -> Option<i32> {
    let mut args = env::args();
    let _program = args.next();
    if args.next().as_deref() != Some("--monet-codex-channel-token") {
        return None;
    }
    let Some(channel_id) = args.next() else {
        return Some(2);
    };
    if args.next().is_some() {
        return Some(2);
    }
    match routine_channel::channel_token(&data_dir(), &channel_id) {
        Ok(token) => {
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            if stdout.write_all(token.as_bytes()).is_err()
                || stdout.write_all(b"\n").is_err()
            {
                return Some(1);
            }
            Some(0)
        }
        Err(_) => Some(1),
    }
}

fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MONET_DATA_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::home_dir().unwrap_or_default().join(".monet")
    }
}

fn routines_path() -> PathBuf {
    data_dir().join("routines.json")
}

fn logs_dir(routine_id: &str) -> PathBuf {
    data_dir()
        .join("routines")
        .join("logs")
        .join(routine_id)
}

fn agent_cwd() -> PathBuf {
    let p = data_dir().join("agent");
    let _ = fs::create_dir_all(&p);
    p
}

fn load_routines() -> Vec<RoutineDefinition> {
    let path = routines_path();
    if !path.exists() {
        eprintln!("routines.json not found");
        std::process::exit(1);
    }
    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("read routines.json: {}", e);
        std::process::exit(1);
    });
    serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!("parse routines.json: {}", e);
        std::process::exit(1);
    })
}

fn should_skip(routine: &RoutineDefinition) -> bool {
    // Find the previous scheduled time before now
    use cron::Schedule;
    use std::str::FromStr;

    let full = crate::cron_expr::to_quartz_full(&routine.cron_expression);
    let schedule = match Schedule::from_str(&full) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let now = chrono::Local::now();
    // Get upcoming times and find the one just before now
    // by getting the next occurrence and subtracting one period
    let mut prev = None;
    // Walk backwards: get many upcoming from a past point
    let past = now - chrono::Duration::days(2);
    for dt in schedule.after(&past) {
        if dt > now {
            break;
        }
        prev = Some(dt);
    }

    // 近 2 天内没有到期的调度点（未来任务 / 停摆已久）：不该跑。
    // RunAtLoad 与 plist 重载会在非调度时刻拉起 runner，此分支是它们的闸门
    let prev_scheduled = match prev {
        Some(p) => p,
        None => return true,
    };

    // 到期且从未跑过 → 补跑
    let last_run = match &routine.last_run {
        Some(lr) => lr,
        None => return false,
    };

    let last_run_dt = match chrono::DateTime::parse_from_rfc3339(last_run) {
        Ok(dt) => dt.with_timezone(&chrono::Local),
        Err(_) => return false,
    };

    last_run_dt >= prev_scheduled
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}

fn write_log(log: &ExecutionLog) {
    let dir = logs_dir(&log.routine_id);
    let _ = fs::create_dir_all(&dir);

    let epoch_ms = chrono::DateTime::parse_from_rfc3339(&log.started_at)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or_else(|_| Utc::now().timestamp_millis());

    let path = dir.join(format!("{}.json", epoch_ms));
    if let Ok(json) = serde_json::to_string_pretty(log) {
        let _ = fs::write(&path, json);
    }
}

fn update_routine_state(routine_id: &str, last_run: &str) {
    let path = routines_path();

    // Read-modify-write with file lock
    let file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return,
    };

    use fs2::FileExt;
    let _ = file.lock_exclusive();

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut routines: Vec<RoutineDefinition> = match serde_json::from_str(&content) {
        Ok(r) => r,
        Err(_) => return,
    };

    if let Some(r) = routines.iter_mut().find(|r| r.id == routine_id) {
        r.last_run = Some(last_run.to_string());
        r.next_run = compute_next_run(&r.cron_expression);
    }

    // 原子写：锁只保证写者互斥，主 App/MCP 的读者不持锁，裸写仍有撕裂窗口
    if let Ok(json) = serde_json::to_string_pretty(&routines) {
        let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    #[allow(clippy::incompatible_msrv)] // unlock 在当前工具链可用，MSRV 仅约束下游兼容
    let _ = file.unlock();
}

fn compute_next_run(cron_expr: &str) -> Option<String> {
    use cron::Schedule;
    use std::str::FromStr;

    let full = crate::cron_expr::to_quartz_full(cron_expr);
    let schedule = Schedule::from_str(&full).ok()?;
    let next = schedule.upcoming(chrono::Local).next()?;
    Some(next.to_rfc3339())
}

/// 回睡判据：近期无键鼠活动即视为无人使用。不以合盖为条件——
/// 开盖唤醒跑完同样要回睡，而 clamshell 外接屏使用中合盖为真却不能睡
const USER_IDLE_THRESHOLD_SECS: u64 = 600;

fn maybe_sleep_after_run() {
    if read_wake_policy() != "active" {
        return;
    }
    // HID 空闲读取失败按「用户在用」处理：宁可不睡，不可误睡
    let idle = hid_idle_secs();
    if idle.map_or(true, |s| s < USER_IDLE_THRESHOLD_SECS) {
        return;
    }
    // 有即将执行的 routine 则不休眠（5 分钟内）
    if has_imminent_routine() {
        return;
    }
    eprintln!(
        "active wake: user idle {}s, sleeping",
        idle.unwrap_or_default()
    );
    // 锁屏下 System Events sleep 会被静默拒绝（Apple Events 受限），
    // 首选 sudoers 白名单授权的 pmset sleepnow（强制睡眠，无 GUI 依赖）；
    // 授权不在位再降级 osascript，失败必须留痕
    if let Ok(o) = Command::new("sudo")
        .args(["-n", "/usr/bin/pmset", "sleepnow"])
        .output()
    {
        if o.status.success() {
            return;
        }
    }
    match Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to sleep")
        .output()
    {
        Ok(o) if !o.status.success() => eprintln!(
            "osascript sleep failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => eprintln!("osascript sleep spawn failed: {}", e),
        _ => {}
    }
}

fn read_wake_policy() -> String {
    let path = data_dir().join("settings.json");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("routineWakePolicy")?.as_str().map(String::from))
        .unwrap_or_else(|| "passive".to_string())
}

/// 会话落盘设置（与主 App channels::agent_session_persist 同一字段，默认落盘）
fn agent_session_persist() -> bool {
    let path = data_dir().join("settings.json");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("agentSessionPersist")?.as_bool())
        .unwrap_or(true)
}

/// 距上次键鼠输入的秒数（IOHIDSystem HIDIdleTime，纳秒），含蓝牙/USB 外接设备
fn hid_idle_secs() -> Option<u64> {
    let out = Command::new("ioreg")
        .args(["-c", "IOHIDSystem", "-d", "4", "-k", "HIDIdleTime"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.contains("\"HIDIdleTime\""))
        .and_then(|l| l.rsplit('=').next())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|ns| ns / 1_000_000_000)
}

fn has_imminent_routine() -> bool {
    use cron::Schedule;
    use std::str::FromStr;

    let routines = load_routines();
    let now = chrono::Local::now();
    let threshold = now + chrono::Duration::minutes(5);

    routines.iter().filter(|r| r.enabled).any(|r| {
        let full = crate::cron_expr::to_quartz_full(&r.cron_expression);
        Schedule::from_str(&full)
            .ok()
            .and_then(|s| s.upcoming(chrono::Local).next())
            .is_some_and(|next| next <= threshold)
    })
}
