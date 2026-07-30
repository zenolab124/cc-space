use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::config;
use crate::proc_ext::HideConsole;
use crate::scheduler;

// ---------------------------------------------------------------------------
// Data structures（RoutineDefinition/RoutineSource 见 routine_types.rs 单一事实源）
// ---------------------------------------------------------------------------

pub use crate::routine_types::{RoutineDefinition, RoutineSource};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineExecutionLog {
    pub routine_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// 落盘会话 ID（agent_cwd 目录下的 <id>.jsonl）。会话落盘设置关闭时为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// 用户手动终止（区别于执行失败）。旧日志无此字段
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRow {
    #[serde(flatten)]
    pub definition: RoutineDefinition,
    pub last_execution: Option<RoutineExecutionLog>,
    pub is_running: bool,
    /// 正在运行时的开始时刻（RFC3339），供前端显示已耗时
    pub running_started_at: Option<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

static ROUTINES: Mutex<Option<Vec<RoutineDefinition>>> = Mutex::new(None);
/// 运行中任务：id → 开始时刻（RFC3339）
static RUNNING: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
/// 串行化 runner 协议判断、准备、提交与失败处置，避免同进程入口互相误删。
static SCHEDULER_ENVIRONMENT: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// File paths
// ---------------------------------------------------------------------------

fn routines_path() -> PathBuf {
    config::data_dir().join("routines.json")
}

// ---------------------------------------------------------------------------
// App settings (wake policy etc.)
// ---------------------------------------------------------------------------

fn read_app_setting(key: &str) -> Option<serde_json::Value> {
    config::read_app_setting(key)
}

fn write_app_setting(key: &str, value: serde_json::Value) {
    config::write_app_setting(key, value);
}

pub fn wake_policy() -> String {
    read_app_setting("routineWakePolicy")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "passive".to_string())
}

fn logs_dir(routine_id: &str) -> PathBuf {
    config::data_dir()
        .join("routines")
        .join("logs")
        .join(routine_id)
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn load_routines() -> Vec<RoutineDefinition> {
    let path = routines_path();
    if !path.exists() {
        return Vec::new();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_routines(data: &[RoutineDefinition]) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = crate::config::atomic_write(&routines_path(), &json);
    }
}

fn with_routines<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<RoutineDefinition>) -> R,
{
    let mut guard = ROUTINES.lock().unwrap();
    let store = guard.get_or_insert_with(load_routines);
    f(store)
}

pub fn invalidate_cache() {
    let mut guard = ROUTINES.lock().unwrap();
    *guard = None;
}

fn disable_runner_after_error(error: String, migration: bool) -> String {
    if !migration {
        return error;
    }
    match scheduler::disable_runner() {
        Ok(()) => error,
        Err(disable_error) => format!("{}; {}", error, disable_error),
    }
}

pub(crate) fn ensure_scheduler_environment() -> Result<(), String> {
    if !scheduler::owns_machine_schedule() || scheduler::dev_build() {
        return Ok(());
    }
    let _guard = SCHEDULER_ENVIRONMENT
        .lock()
        .map_err(|_| "routine environment lock poisoned".to_string())?;
    let runner_needs_migration = !scheduler::installed_runner_supports_environment_snapshot();
    let prepared_runner = scheduler::prepare_runner_binary().map_err(|error| {
        disable_runner_after_error(error, runner_needs_migration)
    })?;
    let claude_config_dir = config::claude_config_dir_env().map(|(_, value)| value);
    crate::routine_env::prepare_claude_config_dir(
        config::data_dir(),
        claude_config_dir.as_deref(),
        runner_needs_migration,
        || prepared_runner.commit().map_err(std::io::Error::other),
        || scheduler::disable_runner().map_err(std::io::Error::other),
    )
    .map_err(|error| format!("routine environment preparation failed: {}", error))
}

fn ensure_wake_scheduler_environment() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    ensure_scheduler_environment()?;
    Ok(())
}

fn suspend_machine_schedules_best_effort() {
    if !scheduler::owns_machine_schedule() || scheduler::dev_build() {
        return;
    }
    let routines_snapshot: Vec<RoutineDefinition> = with_routines(|routines| routines.clone());
    for routine in &routines_snapshot {
        if let Err(error) = scheduler::unregister_routine(&routine.id) {
            log::warn!("failed to suspend routine {}: {}", routine.id, error);
        }
    }
    let _ = scheduler::sync_wake_schedule(&[], "passive");
}

pub fn sync_scheduler() {
    if let Err(e) = ensure_scheduler_environment() {
        log::warn!("routine scheduler sync skipped: {}", e);
        suspend_machine_schedules_best_effort();
        return;
    }
    let runner_path = scheduler::runner_binary_path();
    let routines_snapshot: Vec<RoutineDefinition> = with_routines(|routines| routines.clone());
    if let Err(e) = scheduler::sync_all(&routines_snapshot, &runner_path) {
        log::warn!("routine scheduler sync (external change): {}", e);
    }
}

fn running_started_at(id: &str) -> Option<String> {
    RUNNING
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|s| s.get(id).cloned())
}

fn set_running(id: &str, started_at: Option<&str>) {
    let mut guard = RUNNING.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    match started_at {
        Some(at) => {
            map.insert(id.to_string(), at.to_string());
        }
        None => {
            map.remove(id);
        }
    }
}

/// 运行状态两源汇总：内存（本进程立即运行）优先，其次运行标记
/// （cron 触发的 runner 是独立进程，主 App 只能经 marker 感知）。
/// marker 在但执行锁空闲 = 执行方已死（崩溃/断电残留），顺手清理。
fn effective_running_started_at(id: &str) -> Option<String> {
    if let Some(at) = running_started_at(id) {
        return Some(at);
    }
    let data_dir = config::data_dir();
    let marker = crate::routine_run::read_marker(data_dir, id)?;
    if crate::routine_run::executor_alive(data_dir, id) {
        Some(marker.started_at)
    } else {
        crate::routine_run::remove_marker(data_dir, id);
        None
    }
}

// ---------------------------------------------------------------------------
// Cron helpers
// ---------------------------------------------------------------------------

fn validate_cron(cron_expr: &str) -> Result<(), String> {
    use cron::Schedule;
    use std::str::FromStr;
    // 存储用 vixie 惯例（1=Mon），cron crate 用 Quartz（1=Sun）——收口在 cron_expr 转换
    let full = crate::cron_expr::to_quartz_full(cron_expr);
    Schedule::from_str(&full).map_err(|e| format!("无效的 cron 表达式: {}", e))?;
    Ok(())
}

fn compute_next_run_full(cron_expr: &str) -> Option<String> {
    use cron::Schedule;
    use std::str::FromStr;
    let full = crate::cron_expr::to_quartz_full(cron_expr);
    let schedule = Schedule::from_str(&full).ok()?;
    let next = schedule.upcoming(chrono::Local).next()?;
    Some(next.to_rfc3339())
}

// ---------------------------------------------------------------------------
// Log helpers
// ---------------------------------------------------------------------------

fn read_latest_log(routine_id: &str) -> Option<RoutineExecutionLog> {
    let dir = logs_dir(routine_id);
    if !dir.exists() {
        return None;
    }
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|b| std::cmp::Reverse(b.file_name()));
    let latest = entries.first()?;
    let content = fs::read_to_string(latest.path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_execution_log(log: &RoutineExecutionLog) {
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

fn read_logs(routine_id: &str, limit: usize) -> Vec<RoutineExecutionLog> {
    let dir = logs_dir(routine_id);
    if !dir.exists() {
        return Vec::new();
    }
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .ok()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by_key(|b| std::cmp::Reverse(b.file_name()));
    entries
        .into_iter()
        .take(limit)
        .filter_map(|e| {
            let content = fs::read_to_string(e.path()).ok()?;
            serde_json::from_str(&content).ok()
        })
        .collect()
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…(已截断)", &s[..max])
    }
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

use crate::config::agent_cwd;

fn execute_routine(routine: &RoutineDefinition, app: &AppHandle) -> Result<(), String> {
    let id = routine.id.clone();
    let name = routine.name.clone();
    let prompt = routine.prompt.clone();
    let app = app.clone();

    // 与 runner 同一把执行锁：防「立即运行」与 cron 触发并发同跑；
    // 持锁至收尾，运行标记的存活判定（锁空闲=执行方已死）依赖它
    let lock_path = crate::routine_run::lock_path(config::data_dir(), &id);
    if let Some(parent) = lock_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let lock_file =
        fs::File::create(&lock_path).map_err(|e| format!("无法创建执行锁: {}", e))?;
    {
        use fs2::FileExt;
        if lock_file.try_lock_exclusive().is_err() {
            return Err("任务正在运行中".to_string());
        }
    }

    let started_at = Utc::now().to_rfc3339();
    let t0 = std::time::Instant::now();
    set_running(&id, Some(&started_at));

    // 开始事件：前端据此立即刷新出「运行中」状态
    let _ = app.emit("routine-started", serde_json::json!({
        "routineId": &id,
        "name": &name,
        "startedAt": &started_at,
    }));

    tauri::async_runtime::spawn_blocking(move || {
        let _lock = lock_file; // 持锁至本闭包结束（drop 释放）
        let data_dir = config::data_dir().clone();

        // 会话落盘（与 Agent 能力同一设置）：落盘时指定 session id 并记入执行日志，
        // 供事后在 agent_cwd 目录定位完整会话（可 claude --resume 追查）
        let persist = crate::channels::agent_session_persist();
        let session_id = uuid::Uuid::new_v4().to_string();

        // .app 环境 PATH 极简，裸命令名找不到 claude，必须走 locator 显式定位
        let output = match crate::claude_locator::locate() {
            Ok(located) => {
                let mut cmd = Command::new(&located.path);
                cmd.hide_console();
                // 先清掉父进程覆盖，再按 Monet 已解析的最终根重建 CLI 环境；
                // 避免 MONET_CLAUDE_ROOT 与继承的 CLAUDE_CONFIG_DIR 优先级交叉。
                cmd.env_remove("MONET_CLAUDE_ROOT");
                cmd.env_remove("CLAUDE_CONFIG_DIR");
                if let Some((k, v)) = config::claude_config_dir_env() {
                    cmd.env(k, v);
                }
                // claude 自立进程组（组 ID = claude PID）：stop_routine 组信号
                // 整树回收（含其 MCP 子进程），且不伤及主 App 自身
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    cmd.process_group(0);
                }
                cmd.arg("-p")
                    .arg(&prompt)
                    .arg("--output-format")
                    .arg("text")
                    .arg("--session-id")
                    .arg(&session_id);
                if !persist {
                    cmd.arg("--no-session-persistence");
                }
                cmd.env("PATH", crate::streaming::enhanced_path())
                    .current_dir(agent_cwd())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                match cmd.spawn() {
                    Ok(child) => {
                        // spawn 成功即写运行标记：终止能力与跨进程状态展示的事实源
                        crate::routine_run::write_marker(
                            &data_dir,
                            &id,
                            &crate::routine_run::RunningMarker {
                                pid: child.id(),
                                started_at: started_at.clone(),
                                source: "manual".to_string(),
                                cancelled: false,
                            },
                        );
                        child.wait_with_output()
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::NotFound, e)),
        };

        // 收尾前读取终止标志：stop_routine 杀进程前会置位 cancelled
        let cancelled = crate::routine_run::read_marker(&data_dir, &id)
            .is_some_and(|m| m.cancelled);
        crate::routine_run::remove_marker(&data_dir, &id);

        let finished_at = Utc::now().to_rfc3339();

        let log = match output {
            Ok(out) => RoutineExecutionLog {
                routine_id: id.clone(),
                started_at,
                finished_at: Some(finished_at),
                exit_code: out.status.code(),
                stdout: truncate_str(&String::from_utf8_lossy(&out.stdout), 10240),
                stderr: truncate_str(&String::from_utf8_lossy(&out.stderr), 4096),
                session_id: persist.then(|| session_id.clone()),
                cancelled: cancelled.then_some(true),
            },
            Err(e) => RoutineExecutionLog {
                routine_id: id.clone(),
                started_at,
                finished_at: Some(finished_at),
                exit_code: Some(-1),
                stdout: String::new(),
                stderr: format!("spawn failed: {}", e),
                session_id: None,
                cancelled: None,
            },
        };

        write_execution_log(&log);

        with_routines(|routines| {
            if let Some(r) = routines.iter_mut().find(|r| r.id == id) {
                r.last_run = Some(log.started_at.clone());
                r.next_run = compute_next_run_full(&r.cron_expression);
            }
            save_routines(routines);
        });

        set_running(&id, None);

        // 完成事件带结果概要：前端据此弹完成/失败/终止 toast，无需再查日志
        let _ = app.emit("routine-executed", serde_json::json!({
            "routineId": id,
            "name": name,
            "exitCode": log.exit_code,
            "durationMs": t0.elapsed().as_millis() as u64,
            "cancelled": cancelled,
        }));
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// System scheduler sync (replaces tick loop)
// ---------------------------------------------------------------------------

pub fn startup_sync() {
    // 独立 runner 看不到 App 启动时的 Claude 根覆盖，先把当前生效结果固化。
    // 写失败时撤下既有系统任务，禁止旧 runner 沿用陈旧快照继续执行。
    if let Err(e) = ensure_scheduler_environment() {
        log::warn!("{}; suspending schedules", e);
        suspend_machine_schedules_best_effort();
        return;
    }

    let runner_path = scheduler::runner_binary_path();

    // 无条件重算所有 enabled routine 的 next_run：cron 惯例迁移（vixie 收口）后，
    // 存量 next_run 字符串仍是旧 Quartz 语义算出的错日期，`is_none()` 门控永远
    // 不会自愈——直到该 routine 真正跑过一次 UI 才对上。一次性重算 O(N)，N 通
    // 常个位数，代价可忽略
    with_routines(|routines| {
        let mut changed = false;
        for r in routines.iter_mut() {
            if !r.enabled {
                continue;
            }
            let recomputed = compute_next_run_full(&r.cron_expression);
            if r.next_run != recomputed {
                r.next_run = recomputed;
                changed = true;
            }
        }
        if changed {
            save_routines(routines);
        }
    });

    // Sync with OS scheduler
    let routines_snapshot: Vec<RoutineDefinition> =
        with_routines(|routines| routines.clone());

    if let Err(e) = scheduler::sync_all(&routines_snapshot, &runner_path) {
        log::warn!("routine scheduler sync failed: {}", e);
    }

    // Sync wake schedule
    handle_wake_outcome(scheduler::sync_wake_schedule(
        &routines_snapshot,
        &wake_policy(),
    ));
}

fn sync_wake_if_active() {
    let policy = wake_policy();
    if policy == "active" {
        if let Err(e) = ensure_wake_scheduler_environment() {
            log::warn!("routine wake sync skipped: {}", e);
            suspend_machine_schedules_best_effort();
            return;
        }
        let snapshot: Vec<RoutineDefinition> = with_routines(|r| r.clone());
        handle_wake_outcome(scheduler::sync_wake_schedule(&snapshot, &policy));
    }
}

/// 授权丢失（用户手动删了 sudoers 规则等）时自动降级 passive——
/// 绝不在非交互路径弹系统授权框，设置页下次打开如实展示回落状态
fn handle_wake_outcome(outcome: crate::wake::SyncOutcome) {
    if outcome == crate::wake::SyncOutcome::NoAuthorization {
        log::warn!("wake authorization missing; downgrading policy to passive");
        write_app_setting(
            "routineWakePolicy",
            serde_json::Value::String("passive".to_string()),
        );
    }
}

// ---------------------------------------------------------------------------
// Tauri Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_routines() -> Result<Vec<RoutineRow>, String> {
    Ok(with_routines(|routines| {
        routines
            .iter()
            .map(|r| {
                let running_at = effective_running_started_at(&r.id);
                RoutineRow {
                    definition: r.clone(),
                    last_execution: read_latest_log(&r.id),
                    is_running: running_at.is_some(),
                    running_started_at: running_at,
                }
            })
            .collect()
    }))
}

#[tauri::command]
pub async fn create_routine(
    name: String,
    cron_expression: String,
    original_text: String,
    prompt: String,
    enabled: bool,
) -> Result<RoutineDefinition, String> {
    validate_cron(&cron_expression)?;

    let next_run = if enabled {
        compute_next_run_full(&cron_expression)
    } else {
        None
    };

    let routine = RoutineDefinition {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        cron_expression,
        original_text,
        prompt,
        enabled,
        created_at: Utc::now().to_rfc3339(),
        last_run: None,
        next_run,
        source: Some(RoutineSource::ui()),
    };

    with_routines(|routines| {
        routines.push(routine.clone());
        save_routines(routines);
    });

    if routine.enabled {
        if let Err(e) = ensure_scheduler_environment() {
            log::warn!("scheduler register skipped: {}", e);
            suspend_machine_schedules_best_effort();
        } else {
            let runner_path = scheduler::runner_binary_path();
            if let Err(e) = scheduler::register_routine(&routine, &runner_path) {
                log::warn!("scheduler register failed: {}", e);
            }
        }
    }

    sync_wake_if_active();
    Ok(routine)
}

#[tauri::command]
pub async fn update_routine(
    id: String,
    name: Option<String>,
    cron_expression: Option<String>,
    original_text: Option<String>,
    prompt: Option<String>,
    enabled: Option<bool>,
) -> Result<RoutineDefinition, String> {
    if let Some(ref expr) = cron_expression {
        validate_cron(expr)?;
    }

    let cron_changed = cron_expression.is_some();
    let enabled_changed = enabled.is_some();

    // 锁内只做数据变更与落盘；scheduler 同步必须在锁外——
    // sync_wake_if_active 会再取 ROUTINES 锁，锁内调用即重入死锁
    let result = with_routines(|routines| {
        let r = routines
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| format!("找不到任务: {}", id))?;

        if let Some(v) = name {
            r.name = v;
        }
        if let Some(v) = cron_expression {
            r.cron_expression = v;
        }
        if let Some(v) = original_text {
            r.original_text = v;
        }
        if let Some(v) = prompt {
            r.prompt = v;
        }
        if let Some(v) = enabled {
            r.enabled = v;
        }

        if cron_changed {
            r.last_run = None;
        }
        if (cron_changed || enabled_changed) && r.enabled {
            r.next_run = compute_next_run_full(&r.cron_expression);
        } else if !r.enabled {
            r.next_run = None;
        }

        let result = r.clone();
        save_routines(routines);
        Ok::<RoutineDefinition, String>(result)
    })?;

    // Sync system scheduler
    if cron_changed || enabled_changed {
        // dev 门控须跳过整对 unregister+register：register 在 dev 下是 no-op，
        // 只跑 unregister 会把正式 .app 装好的注册拆掉。改动落盘即可，
        // 下次正式启动 sync_all 按 needs_update 补正（runner 侧另有 enabled 兜底）
        if !scheduler::dev_build() {
            let _ = scheduler::unregister_routine(&result.id);
            if result.enabled {
                if let Err(e) = ensure_scheduler_environment() {
                    log::warn!("scheduler re-register skipped: {}", e);
                    suspend_machine_schedules_best_effort();
                } else {
                    let runner_path = scheduler::runner_binary_path();
                    if let Err(e) = scheduler::register_routine(&result, &runner_path) {
                        log::warn!("scheduler re-register failed: {}", e);
                    }
                }
            }
        }
        sync_wake_if_active();
    }

    Ok(result)
}

#[tauri::command]
pub async fn delete_routine(id: String) -> Result<(), String> {
    if let Err(e) = scheduler::unregister_routine(&id) {
        log::warn!("scheduler unregister failed: {}", e);
    }

    with_routines(|routines| {
        routines.retain(|r| r.id != id);
        save_routines(routines);
    });
    sync_wake_if_active();
    Ok(())
}

#[tauri::command]
pub fn get_routine_logs(id: String, limit: Option<usize>) -> Result<Vec<RoutineExecutionLog>, String> {
    Ok(read_logs(&id, limit.unwrap_or(20)))
}

#[tauri::command]
pub fn run_routine_now(id: String, app: AppHandle) -> Result<(), String> {
    // cron 触发的执行也要挡（marker 感知）；execute_routine 内抢锁是最终闸门
    if effective_running_started_at(&id).is_some() {
        return Err("任务正在运行中".to_string());
    }

    let routine = with_routines(|routines| {
        routines.iter().find(|r| r.id == id).cloned()
    })
    .ok_or_else(|| format!("找不到任务: {}", id))?;

    execute_routine(&routine, &app)
}

/// 终止正在执行的任务：置 cancelled 标志后对 claude 进程组发 TERM，
/// 3 秒未退（marker 未被执行方收尾清除）则 KILL 兜底。执行方（主 App
/// 闭包 / cron runner）的 wait 随即返回，正常落日志并清理状态。
#[tauri::command]
pub fn stop_routine(id: String) -> Result<(), String> {
    let data_dir = config::data_dir();
    let marker = crate::routine_run::mark_cancelled(data_dir, &id)
        .ok_or_else(|| "任务未在运行".to_string())?;

    signal_routine_group(marker.pid, "-TERM");

    let dir = data_dir.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if let Some(m) = crate::routine_run::read_marker(&dir, &id) {
            if m.pid == marker.pid {
                signal_routine_group(m.pid, "-KILL");
            }
        }
    });
    Ok(())
}

/// 与 streaming::signal_session 同模式：组信号整树回收（claude spawn 时
/// process_group(0) 自立门户），组不存在时退化单杀
#[cfg(unix)]
fn signal_routine_group(pid: u32, sig: &str) {
    let group_ok = Command::new("kill")
        .args([sig, "--", &format!("-{}", pid)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !group_ok {
        let _ = Command::new("kill").args([sig, &pid.to_string()]).output();
    }
}

/// Windows 无信号语义：一律 taskkill /T /F 整树强杀；
/// 已死进程上的第二发报 not found 被吞，与 unix 二段式击杀兼容
#[cfg(windows)]
fn signal_routine_group(pid: u32, _sig: &str) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .hide_console()
        .output();
}

#[tauri::command]
pub fn get_routine_wake_policy() -> String {
    wake_policy()
}

#[tauri::command]
pub async fn set_routine_wake_policy(policy: String) -> Result<(), String> {
    if policy != "passive" && policy != "active" {
        return Err("无效策略，支持: passive | active".to_string());
    }
    // 正常开启路径是 enable_wake_active（含授权引导）；此处兜住 MCP 等
    // 旁路直接 set active 且授权不在位的情况
    if policy == "active" && !crate::wake::authorization_present() {
        return Err("NEEDS_AUTHORIZATION".to_string());
    }
    if let Err(error) = ensure_wake_scheduler_environment() {
        suspend_machine_schedules_best_effort();
        return Err(error);
    }
    write_app_setting(
        "routineWakePolicy",
        serde_json::Value::String(policy.clone()),
    );

    let snapshot: Vec<RoutineDefinition> = with_routines(|r| r.clone());
    handle_wake_outcome(scheduler::sync_wake_schedule(&snapshot, &policy));

    Ok(())
}

/// sudoers 白名单授权是否在位（sudo -n pmset 探测）
#[tauri::command]
pub fn get_wake_authorization_status() -> bool {
    crate::wake::authorization_present()
}

/// 开启主动唤醒：授权不在位时先提权安装 sudoers 白名单（系统弹一次密码
/// 框，用户取消返回 Err("cancelled")），随后写入 policy 并静默同步计划
#[tauri::command]
pub async fn enable_wake_active() -> Result<(), String> {
    if !crate::wake::authorization_present() {
        crate::wake::install_authorization()?;
    }
    if let Err(error) = ensure_wake_scheduler_environment() {
        suspend_machine_schedules_best_effort();
        return Err(error);
    }
    write_app_setting(
        "routineWakePolicy",
        serde_json::Value::String("active".to_string()),
    );
    let snapshot: Vec<RoutineDefinition> = with_routines(|r| r.clone());
    handle_wake_outcome(scheduler::sync_wake_schedule(&snapshot, "active"));
    Ok(())
}

/// 移除系统授权：先趁规则还在静默清空唤醒计划并降级 passive，
/// 再提权删除 sudoers 文件（用户主动点击，弹窗在预期内）
#[tauri::command]
pub async fn remove_wake_authorization() -> Result<(), String> {
    if let Err(error) = ensure_wake_scheduler_environment() {
        suspend_machine_schedules_best_effort();
        return Err(error);
    }
    write_app_setting(
        "routineWakePolicy",
        serde_json::Value::String("passive".to_string()),
    );
    let snapshot: Vec<RoutineDefinition> = with_routines(|r| r.clone());
    let _ = scheduler::sync_wake_schedule(&snapshot, "passive");
    crate::wake::remove_authorization()
}
