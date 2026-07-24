//! 跑单 supervisor：进程管理 + 日志双通道 + 生命周期治理 + IPC commands。
//!
//! 每个跑单实例由独立 runner_id 标识，归属于一个会话 session_id。
//! 进程组整树回收保证子进程不残留；pid 文件 + meta 落盘 + 启动扫描兜底崩溃场景。
//!
//! 日志收敛为单 flusher 线程：stdout/stderr 读线程投 mpsc channel，
//! flusher 攒批 + seq 排序 + 按 runner 分组 emit，保证跨 stream 有序。

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc as std_mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::runner_store;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 前端可见的跑单快照（IPC 返回值 + runner-status 事件 payload）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerSnapshot {
    pub id: String,
    pub session_id: String,
    pub alias: Option<String>,
    pub cmd: String,
    pub cwd: String,
    pub status: RunnerStatus,
    /// epoch 毫秒
    pub started_at: i64,
    /// epoch 毫秒
    pub exited_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
    pub log_path: String,
}

/// 跑单状态（kebab-case 序列化以匹配前端契约）
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RunnerStatus {
    /// 保留：语义占位，未来可用于 spawn 前的瞬态
    #[allow(dead_code)]
    Starting,
    Running,
    Exited,
    Killed,
    Crashed,
    SpawnFailed,
}

/// 日志行（内存 ring buffer + IPC 返回值）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub seq: u64,
    pub ts: u64,
    pub stream: String,
    pub text: String,
}

/// supervisor 内部条目
struct RunnerEntry {
    snapshot: RunnerSnapshot,
    ring: VecDeque<LogLine>,
    seq_counter: u64,
    log_path: PathBuf,
    log_writer: Option<BufWriter<fs::File>>,
    log_bytes: u64,
    spec: RunnerSpec,
    stop_requested: bool,
    /// Windows: Job Object 句柄，KILL_ON_JOB_CLOSE 保底崩溃场景的子进程回收（未实机验证）
    #[cfg(windows)]
    _job_handle: Option<JobHandle>,
}

/// 重启用的原始参数
#[derive(Clone)]
struct RunnerSpec {
    session_id: String,
    cmd: String,
    cwd: String,
    alias: Option<String>,
    env_override: Option<HashMap<String, String>>,
    source_command_id: Option<String>,
}

/// 日志消息（读线程 → flusher 线程）
struct LogMessage {
    runner_id: String,
    line: LogLine,
}

// ---------------------------------------------------------------------------
// 全局状态
// ---------------------------------------------------------------------------

static SUPERVISOR: Mutex<Option<HashMap<String, RunnerEntry>>> = Mutex::new(None);

/// 日志聚合通道发送端（读线程向 flusher 投递）
static LOG_TX: OnceLock<std_mpsc::Sender<LogMessage>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Windows Job Object（未实机验证）
// ---------------------------------------------------------------------------

#[cfg(windows)]
struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
unsafe impl Send for JobHandle {}

/// 创建 Job Object 并将子进程分配至其中，KILL_ON_JOB_CLOSE 保证
/// 父进程崩溃时子进程被操作系统回收（未实机验证）
#[cfg(windows)]
fn create_job_for_child(pid: u32) -> Option<JobHandle> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Threading::*;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            CloseHandle(job);
            return None;
        }

        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if process.is_null() {
            CloseHandle(job);
            return None;
        }
        let assigned = AssignProcessToJobObject(job, process);
        CloseHandle(process);
        if assigned == 0 {
            CloseHandle(job);
            return None;
        }

        Some(JobHandle(job))
    }
}

// ---------------------------------------------------------------------------
// 信号工具
// ---------------------------------------------------------------------------

/// 向跑单进程组发信号。spawn 时 process_group(0) 自立门户（组 ID = PID），
/// 组信号整树回收子进程；组不存在时退化单杀
#[cfg(unix)]
fn signal_runner_group(pid: u32, sig: &str) {
    let group_ok = Command::new("kill")
        .args([sig, "--", &format!("-{}", pid)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !group_ok {
        let _ = Command::new("kill").args([sig, &pid.to_string()]).output();
    }
}

/// Windows 无信号语义：一律 taskkill /T /F 整树强杀
#[cfg(windows)]
fn signal_runner_group(pid: u32, _sig: &str) {
    use crate::proc_ext::HideConsole;
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .hide_console()
        .output();
}

/// 检查进程是否存活
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        let _ = pid;
        false
    }
}

/// 检查进程组是否仍有存活成员（pgid = 组长 pid）
#[cfg(unix)]
fn is_group_alive(pgid: u32) -> bool {
    Command::new("kill")
        .args(["-0", "--", &format!("-{}", pgid)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 取进程的 ppid（孤儿治理用：ppid==1 说明已被 launchd 收养）
#[cfg(unix)]
fn get_ppid(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .ok()
}

// ---------------------------------------------------------------------------
// 日志滚动
// ---------------------------------------------------------------------------

fn rotated_path(base: &Path, n: u32) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(format!(".{}", n));
    PathBuf::from(s)
}

/// 日志文件滚动：.log→.log.1→.log.2→.log.3（最多 3 份旧日志）
fn rotate_log(entry: &mut RunnerEntry) {
    // 刷盘并关闭当前文件
    if let Some(mut writer) = entry.log_writer.take() {
        let _ = writer.flush();
    }

    let base = &entry.log_path;
    let _ = fs::remove_file(rotated_path(base, 3));
    let _ = fs::rename(rotated_path(base, 2), rotated_path(base, 3));
    let _ = fs::rename(rotated_path(base, 1), rotated_path(base, 2));
    let _ = fs::rename(base, rotated_path(base, 1));

    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(base)
    {
        Ok(f) => {
            entry.log_writer = Some(BufWriter::new(f));
            entry.log_bytes = 0;
        }
        Err(e) => {
            // 降级：追加到刚刚滚动的 .log.1
            log::warn!("日志滚动后创建新文件失败，降级追加: {}", e);
            if let Ok(f) = fs::OpenOptions::new()
                .append(true)
                .open(rotated_path(base, 1))
            {
                entry.log_writer = Some(BufWriter::new(f));
            }
        }
    }
}

fn epoch_ms_now() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

// ---------------------------------------------------------------------------
// 日志 flusher（单发射通道，保证跨 stdout/stderr seq 有序）
// ---------------------------------------------------------------------------

/// 启动日志聚合发射线程。读线程将日志行投入 channel，
/// flusher 攒批 + seq 排序 + 按 runner 分组 emit
fn init_log_flusher(app: AppHandle) {
    let (tx, rx) = std_mpsc::channel::<LogMessage>();
    LOG_TX.set(tx).ok();

    std::thread::spawn(move || {
        let mut batch: Vec<LogMessage> = Vec::new();

        loop {
            // 无积压时纯阻塞（零唤醒）；有积压时 100ms 超时驱动发射
            let got_msg = if batch.is_empty() {
                match rx.recv() {
                    Ok(m) => {
                        batch.push(m);
                        true
                    }
                    Err(_) => break,
                }
            } else {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(m) => {
                        batch.push(m);
                        true
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => false,
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                        flush_log_batch(&app, &mut batch);
                        break;
                    }
                }
            };

            // 满 32 行或超时：发射 + 刷盘
            if batch.len() >= 32 || (!got_msg && !batch.is_empty()) {
                flush_log_batch(&app, &mut batch);
            }
        }
    });
}

/// 排序→分组→emit→刷盘。emit 在 SUPERVISOR 锁外
fn flush_log_batch(app: &AppHandle, batch: &mut Vec<LogMessage>) {
    if batch.is_empty() {
        return;
    }

    // 按 seq 排序（跨 stdout/stderr 归并有序）
    batch.sort_by_key(|m| m.line.seq);

    // 按 runner_id 分组 emit（emit 不持 SUPERVISOR 锁）
    let mut groups: HashMap<String, Vec<&LogLine>> = HashMap::new();
    for m in batch.iter() {
        groups
            .entry(m.runner_id.clone())
            .or_default()
            .push(&m.line);
    }
    for (runner_id, lines) in &groups {
        let _ = app.emit(
            "runner-log",
            json!({ "runnerId": runner_id, "lines": lines }),
        );
    }

    // 刷盘（锁内，但 emit 已完成）
    {
        let mut guard = SUPERVISOR.lock().unwrap();
        if let Some(map) = guard.as_mut() {
            for rid in groups.keys() {
                if let Some(entry) = map.get_mut(rid.as_str()) {
                    if let Some(ref mut w) = entry.log_writer {
                        let _ = w.flush();
                    }
                }
            }
        }
    }

    batch.clear();
}

// ---------------------------------------------------------------------------
// Snapshot → Meta 转换
// ---------------------------------------------------------------------------

fn snapshot_to_meta(snap: &RunnerSnapshot) -> runner_store::RunnerMeta {
    // RunnerStatus 用 serde kebab-case 序列化为字符串
    let status_str = serde_json::to_value(&snap.status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "unknown".to_string());
    runner_store::RunnerMeta {
        runner_id: snap.id.clone(),
        session_id: snap.session_id.clone(),
        alias: snap.alias.clone(),
        cmd: snap.cmd.clone(),
        cwd: snap.cwd.clone(),
        status: status_str,
        started_at: snap.started_at,
        exited_at: snap.exited_at,
        exit_code: snap.exit_code,
        log_path: snap.log_path.clone(),
    }
}

// ---------------------------------------------------------------------------
// spawn 内部逻辑
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn do_spawn(
    app: &AppHandle,
    session_id: &str,
    cmd_str: &str,
    cwd: &str,
    alias: Option<&str>,
    env_override: Option<&HashMap<String, String>>,
    source_command_id: Option<&str>,
    reuse_runner_id: Option<&str>,
) -> Result<RunnerSnapshot, String> {
    // 1. 前置校验
    if !Path::new(cwd).is_dir() {
        return Err(format!("工作目录不存在: {}", cwd));
    }

    let limit = crate::config::read_app_setting("runner.perSessionLimit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    {
        let guard = SUPERVISOR.lock().unwrap();
        if let Some(map) = guard.as_ref() {
            let active = map
                .values()
                .filter(|e| {
                    e.snapshot.session_id == session_id
                        && matches!(
                            e.snapshot.status,
                            RunnerStatus::Starting | RunnerStatus::Running
                        )
                })
                .count();
            if active >= limit {
                return Err(format!(
                    "会话已有 {} 个运行中跑单（上限 {}）",
                    active, limit
                ));
            }
        }
    }

    // 2. 解析命令
    let argv = shell_words::split(cmd_str)
        .map_err(|e| format!("命令解析失败: {}", e))?;
    if argv.is_empty() {
        return Err("命令为空".to_string());
    }

    // 3. 日志目录与文件
    let runner_id = reuse_runner_id
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let log_dir = runner_store::proc_logs_dir().join(session_id);
    let _ = fs::create_dir_all(&log_dir);
    let log_path = log_dir.join(format!("{}.log", runner_id));

    // 4. 构建 Command
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .env("PATH", crate::path_env::enhanced_path())
        // pipe 下子进程探测不到 TTY 会关闭彩色输出，强制着色（用户 env_override 可覆盖）
        .env("FORCE_COLOR", "1")
        .env("CLICOLOR_FORCE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // 清除 Monet 内部环境变量，子进程不应继承
    for (k, _) in std::env::vars() {
        if k.starts_with("MONET_") {
            command.env_remove(&k);
        }
    }

    // 用户环境变量覆盖（空值 = 删除该键）
    if let Some(env) = env_override {
        for (k, v) in env {
            if v.is_empty() {
                command.env_remove(k);
            } else {
                command.env(k, v);
            }
        }
    }

    // 进程组自立门户（组 ID = PID），整树信号回收
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use crate::proc_ext::HideConsole;
        command.hide_console();
    }

    // 5. spawn
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn 失败: {}", e))?;

    let pid = child.id();
    let started_at = chrono::Utc::now();
    let started_at_ms = started_at.timestamp_millis();

    // 6. 写 pid 文件
    runner_store::write_pid(
        session_id,
        &runner_id,
        &runner_store::PidInfo {
            pid,
            pgid: Some(pid), // process_group(0) → pgid = pid
            start_time_epoch_ms: started_at_ms as u64,
            cmd: cmd_str.to_string(),
            session_id: session_id.to_string(),
        },
    );

    // 7. Windows Job Object 保底
    #[cfg(windows)]
    let job_handle = create_job_for_child(pid);

    // 8. 打开日志文件（append 模式：restart 时续写）
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path);
    let (log_writer, log_bytes) = match log_file {
        Ok(f) => {
            let bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
            (Some(BufWriter::new(f)), bytes)
        }
        Err(e) => {
            log::warn!("日志文件打开失败，降级无落盘: {}", e);
            (None, 0)
        }
    };

    // 9. 取 stdout/stderr
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    // 10. 构建 entry
    let snapshot = RunnerSnapshot {
        id: runner_id.clone(),
        session_id: session_id.to_string(),
        alias: alias.map(String::from),
        cmd: cmd_str.to_string(),
        cwd: cwd.to_string(),
        status: RunnerStatus::Running,
        started_at: started_at_ms,
        exited_at: None,
        exit_code: None,
        pid: Some(pid),
        log_path: log_path.to_string_lossy().into_owned(),
    };

    let spec = RunnerSpec {
        session_id: session_id.to_string(),
        cmd: cmd_str.to_string(),
        cwd: cwd.to_string(),
        alias: alias.map(String::from),
        env_override: env_override.cloned(),
        source_command_id: source_command_id.map(String::from),
    };

    {
        let mut guard = SUPERVISOR.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            runner_id.clone(),
            RunnerEntry {
                snapshot: snapshot.clone(),
                ring: VecDeque::new(),
                seq_counter: 0,
                log_path: log_path.clone(),
                log_writer,
                log_bytes,
                spec,
                stop_requested: false,
                #[cfg(windows)]
                _job_handle: job_handle,
            },
        );
    }

    // 11. 写 meta 文件（spawn 时初始状态）
    runner_store::write_meta(session_id, &runner_id, &snapshot_to_meta(&snapshot));

    // 12. 启动读线程（投 channel，flusher 统一发射）
    if let Some(pipe) = stdout_pipe {
        start_reader(runner_id.clone(), pipe, "stdout");
    }
    if let Some(pipe) = stderr_pipe {
        start_reader(runner_id.clone(), pipe, "stderr");
    }

    // 13. 启动 wait 线程
    let app_wait = app.clone();
    let rid_wait = runner_id.clone();
    let sid_wait = session_id.to_string();
    std::thread::spawn(move || {
        let status = child.wait();
        // 留一点时间给 reader 线程处理最后的输出
        std::thread::sleep(Duration::from_millis(50));

        let snap = {
            let mut guard = SUPERVISOR.lock().unwrap();
            let Some(map) = guard.as_mut() else { return };
            let Some(entry) = map.get_mut(&rid_wait) else {
                return;
            };
            // 已被新 spawn 替换（restart 场景）：此条目的 pid 已变，不覆盖
            if entry.snapshot.pid != Some(pid) {
                return;
            }

            let exited_at = chrono::Utc::now().timestamp_millis();
            entry.snapshot.exited_at = Some(exited_at);

            match status {
                Ok(es) => {
                    let code = es.code();
                    entry.snapshot.exit_code = code;

                    if entry.stop_requested {
                        entry.snapshot.status = RunnerStatus::Killed;
                    } else if code == Some(0) {
                        entry.snapshot.status = RunnerStatus::Exited;
                    } else {
                        entry.snapshot.status = RunnerStatus::Crashed;
                    }
                }
                Err(_) => {
                    entry.snapshot.status = RunnerStatus::Crashed;
                }
            }

            // 刷盘
            if let Some(ref mut w) = entry.log_writer {
                let _ = w.flush();
            }

            entry.snapshot.clone()
        };

        // 组残存清扫：组长已退但子进程可能残留（pgid = pid）
        #[cfg(unix)]
        {
            if is_group_alive(pid) {
                signal_runner_group(pid, "-KILL");
            }
        }

        // 删 pid 文件 + 更新 meta 终态
        runner_store::remove_pid(&sid_wait, &rid_wait);
        runner_store::write_meta(&sid_wait, &rid_wait, &snapshot_to_meta(&snap));

        // 裁决 1：runner-status payload = 裸 RunnerSnapshot
        let _ = app_wait.emit("runner-status", &snap);
    });

    // 14. emit 状态（裸 snapshot）
    let _ = app.emit("runner-status", &snapshot);

    // 15. 沉淀候选清单
    runner_store::settle_command(
        cwd,
        cmd_str,
        None,
        alias,
        source_command_id,
    );

    Ok(snapshot)
}

/// 读线程：从 pipe 按行读取，推 ring buffer + 落盘 + 投 channel。
/// 不直接 emit——由 flusher 线程统一攒批排序发射
fn start_reader<R: std::io::Read + Send + 'static>(
    runner_id: String,
    pipe: R,
    stream_name: &'static str,
) {
    std::thread::spawn(move || {
        let buf = BufReader::new(pipe);

        for line_result in buf.lines() {
            let Ok(text) = line_result else { break };
            let now_ms = epoch_ms_now();

            let log_line = {
                let mut guard = SUPERVISOR.lock().unwrap();
                let Some(map) = guard.as_mut() else { break };
                let Some(entry) = map.get_mut(&runner_id) else {
                    break;
                };

                entry.seq_counter += 1;
                let ll = LogLine {
                    seq: entry.seq_counter,
                    ts: now_ms,
                    stream: stream_name.to_string(),
                    text: text.clone(),
                };

                // ring buffer（上限 2000 行）
                entry.ring.push_back(ll.clone());
                if entry.ring.len() > 2000 {
                    entry.ring.pop_front();
                }

                // 落盘（不 flush，由 flusher 定时刷）
                if let Some(ref mut writer) = entry.log_writer {
                    let ts_iso = chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                    let _ = writeln!(writer, "[{}][{}] {}", ts_iso, stream_name, text);
                    // 近似字节数（含时间戳和分隔符）
                    entry.log_bytes += (text.len() + stream_name.len() + 32) as u64;

                    // 10MB 滚动
                    if entry.log_bytes >= 10 * 1024 * 1024 {
                        rotate_log(entry);
                    }
                }

                ll
            };

            // 投 channel（flusher 负责攒批 + seq 排序 + emit）
            if let Some(tx) = LOG_TX.get() {
                let _ = tx.send(LogMessage {
                    runner_id: runner_id.clone(),
                    line: log_line,
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// stop 内部逻辑
// ---------------------------------------------------------------------------

fn do_stop(_app: &AppHandle, runner_id: &str, graceful: bool) -> Result<(), String> {
    let pid = {
        let mut guard = SUPERVISOR.lock().unwrap();
        let map = guard.as_mut().ok_or("supervisor 未初始化")?;
        let entry = map.get_mut(runner_id).ok_or("runner not found")?;

        if !matches!(
            entry.snapshot.status,
            RunnerStatus::Starting | RunnerStatus::Running
        ) {
            return Ok(()); // 已终止
        }

        entry.stop_requested = true;
        entry.snapshot.pid.ok_or("runner 无 pid")?
    };

    if graceful {
        signal_runner_group(pid, "-TERM");

        // 3s 后检查是否仍存活，存活则 KILL 升级
        let rid = runner_id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            let should_escalate = {
                let guard = SUPERVISOR.lock().unwrap();
                guard
                    .as_ref()
                    .and_then(|m| m.get(&rid))
                    .map(|e| {
                        // pid 校验：防 restart 换代后误杀新进程
                        e.snapshot.pid == Some(pid)
                            && matches!(
                                e.snapshot.status,
                                RunnerStatus::Starting | RunnerStatus::Running
                            )
                    })
                    .unwrap_or(false)
            };
            if should_escalate {
                signal_runner_group(pid, "-KILL");
                // 裁决 1：KILL 升级线程不再 emit 事件（wait 线程发终态 snapshot）
            }
        });
    } else {
        signal_runner_group(pid, "-KILL");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// IPC commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn runner_spawn(
    app: AppHandle,
    session_id: String,
    cmd: String,
    cwd: String,
    alias: Option<String>,
    env: Option<HashMap<String, String>>,
    source_command_id: Option<String>,
) -> Result<RunnerSnapshot, String> {
    do_spawn(
        &app,
        &session_id,
        &cmd,
        &cwd,
        alias.as_deref(),
        env.as_ref(),
        source_command_id.as_deref(),
        None,
    )
}

#[tauri::command]
pub fn runner_stop(app: AppHandle, runner_id: String, graceful: bool) -> Result<(), String> {
    do_stop(&app, &runner_id, graceful)
}

#[tauri::command]
pub async fn runner_restart(app: AppHandle, runner_id: String) -> Result<RunnerSnapshot, String> {
    let (spec, old_seq) = {
        let guard = SUPERVISOR.lock().unwrap();
        let map = guard.as_ref().ok_or("supervisor 未初始化")?;
        let entry = map.get(&runner_id).ok_or("runner not found")?;
        (entry.spec.clone(), entry.seq_counter)
    };

    // 先停旧进程
    do_stop(&app, &runner_id, true)?;

    // 等待进程实际退出（最多 5s，spawn_blocking 不阻塞 async 线程池）
    let rid_poll = runner_id.clone();
    let exited = tauri::async_runtime::spawn_blocking(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            {
                let guard = SUPERVISOR.lock().unwrap();
                let still_active = guard
                    .as_ref()
                    .and_then(|m| m.get(&rid_poll))
                    .map(|e| {
                        matches!(
                            e.snapshot.status,
                            RunnerStatus::Starting | RunnerStatus::Running
                        )
                    })
                    .unwrap_or(false);
                if !still_active {
                    return true;
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })
    .await
    .unwrap_or(false);

    if !exited {
        return Err("等待进程退出超时".to_string());
    }

    // 以原参数重 spawn，复用 runner_id（do_spawn 内部 insert 覆盖旧条目）
    match do_spawn(
        &app,
        &spec.session_id,
        &spec.cmd,
        &spec.cwd,
        spec.alias.as_deref(),
        spec.env_override.as_ref(),
        spec.source_command_id.as_deref(),
        Some(&runner_id),
    ) {
        Ok(snap) => {
            // seq 续增：避免前端日志序号断裂
            {
                let mut guard = SUPERVISOR.lock().unwrap();
                if let Some(entry) = guard.as_mut().and_then(|m| m.get_mut(&runner_id)) {
                    entry.seq_counter = old_seq;
                }
            }
            Ok(snap)
        }
        Err(e) => {
            // spawn 失败：保留旧条目，标记 SpawnFailed
            let mut guard = SUPERVISOR.lock().unwrap();
            if let Some(entry) = guard.as_mut().and_then(|m| m.get_mut(&runner_id)) {
                entry.snapshot.status = RunnerStatus::SpawnFailed;
                entry.snapshot.exited_at = Some(chrono::Utc::now().timestamp_millis());
                entry.snapshot.pid = None;
                let snap = entry.snapshot.clone();
                let sid = entry.snapshot.session_id.clone();
                runner_store::write_meta(&sid, &runner_id, &snapshot_to_meta(&snap));
                drop(guard);
                let _ = app.emit("runner-status", &snap);
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub fn runner_list(session_id: Option<String>) -> Vec<RunnerSnapshot> {
    let guard = SUPERVISOR.lock().unwrap();
    let Some(map) = guard.as_ref() else {
        return Vec::new();
    };
    map.values()
        .filter(|e| match &session_id {
            Some(sid) => e.snapshot.session_id == *sid,
            None => true,
        })
        .map(|e| e.snapshot.clone())
        .collect()
}

/// 从内存 ring buffer 读取末尾 N 行（前端断连补齐用）
#[tauri::command]
pub fn runner_tail(runner_id: String, lines: usize) -> Result<Vec<LogLine>, String> {
    let guard = SUPERVISOR.lock().unwrap();
    let map = guard.as_ref().ok_or("supervisor 未初始化")?;
    let entry = map.get(&runner_id).ok_or("runner not found")?;
    let ring = &entry.ring;
    let start = ring.len().saturating_sub(lines);
    Ok(ring.iter().skip(start).cloned().collect())
}

#[tauri::command]
pub fn runner_commands_list(project_cwd: String) -> Vec<runner_store::RunnerCommand> {
    runner_store::list_commands(&project_cwd)
}

#[tauri::command]
pub fn runner_command_remove(project_cwd: String, id: String) -> Result<(), String> {
    runner_store::remove_command(&project_cwd, &id)
}

/// 停止指定会话的所有运行中跑单（会话归档/关闭时调）
#[tauri::command]
pub fn runner_session_stop_all(app: AppHandle, session_id: String) -> Result<(), String> {
    let runner_ids: Vec<String> = {
        let guard = SUPERVISOR.lock().unwrap();
        guard
            .as_ref()
            .map(|m| {
                m.iter()
                    .filter(|(_, e)| e.snapshot.session_id == session_id)
                    .filter(|(_, e)| {
                        matches!(
                            e.snapshot.status,
                            RunnerStatus::Starting | RunnerStatus::Running
                        )
                    })
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    for rid in runner_ids {
        let _ = do_stop(&app, &rid, true);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 生命周期
// ---------------------------------------------------------------------------

/// 初始化 supervisor + 日志 flusher + 孤儿清扫。
/// 必须在 setup 阶段、任何 command 可被调用之前执行
pub fn init(app: &AppHandle) {
    // 初始化 supervisor map
    {
        let mut guard = SUPERVISOR.lock().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
    }

    // 启动日志聚合发射线程
    init_log_flusher(app.clone());

    // 孤儿清扫
    startup_cleanup();
}

/// 启动孤儿扫描：扫 proc-logs/**/*.pid，探活 + start_time 校验 + ppid 校验。
/// 真孤儿（ppid==1，已被 launchd 收养）组杀 KILL；其余保守不杀
fn startup_cleanup() {
    let pids = runner_store::scan_all_pids();
    for (session_id, runner_id, info) in pids {
        if !is_pid_alive(info.pid) {
            // 进程已死——假孤儿，只清 pid 文件
            runner_store::remove_pid(&session_id, &runner_id);
            continue;
        }

        // 进程存活——校验 start_time 判断是否真孤儿
        match runner_store::verify_start_time(info.pid, info.start_time_epoch_ms) {
            Ok(true) => {
                // start_time 吻合 → 同一进程；再验 ppid==1 避免双实例误杀
                #[cfg(unix)]
                {
                    match get_ppid(info.pid) {
                        Some(1) => {
                            // 真孤儿：已被 launchd 收养
                            log::warn!(
                                "启动孤儿扫描：杀真孤儿 runner={} pid={}",
                                runner_id,
                                info.pid
                            );
                            signal_runner_group(info.pid, "-KILL");
                            runner_store::remove_pid(&session_id, &runner_id);
                        }
                        Some(_) => {
                            // ppid 非 1：可能属于另一实例，不杀不清 pid 文件
                        }
                        None => {
                            // ppid 取不到：保守不杀，保留 pid 文件
                        }
                    }
                }
                #[cfg(windows)]
                {
                    // Windows 维持现状（JobObject 兜底）
                    runner_store::remove_pid(&session_id, &runner_id);
                }
            }
            Ok(false) => {
                // pid 已被复用：不杀，只清 pid 文件
                runner_store::remove_pid(&session_id, &runner_id);
            }
            Err(_) => {
                // 无法校验：保守路径，只清 pid 文件不杀
                runner_store::remove_pid(&session_id, &runner_id);
            }
        }
    }
}

/// 退出时同步停止所有运行中跑单（总时限 5s：TERM → 轮询 → KILL）
pub fn shutdown_all() {
    let pids: Vec<(String, u32)> = {
        let mut guard = SUPERVISOR.lock().unwrap();
        let Some(map) = guard.as_mut() else {
            return;
        };
        map.iter_mut()
            .filter(|(_, e)| {
                matches!(
                    e.snapshot.status,
                    RunnerStatus::Starting | RunnerStatus::Running
                )
            })
            .filter_map(|(id, e)| {
                e.stop_requested = true;
                e.snapshot.pid.map(|p| (id.clone(), p))
            })
            .collect()
    };

    if pids.is_empty() {
        return;
    }

    // 全体发 TERM
    for (_, pid) in &pids {
        signal_runner_group(*pid, "-TERM");
    }

    // 轮询最多 3s，以进程组为准判断存活
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let all_dead = pids.iter().all(|(_, pid)| {
            #[cfg(unix)]
            {
                !is_group_alive(*pid)
            }
            #[cfg(not(unix))]
            {
                !is_pid_alive(*pid)
            }
        });
        if all_dead || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // KILL 残存者（以组为单位）
    for (_, pid) in &pids {
        #[cfg(unix)]
        {
            if is_group_alive(*pid) {
                signal_runner_group(*pid, "-KILL");
            }
        }
        #[cfg(not(unix))]
        {
            if is_pid_alive(*pid) {
                signal_runner_group(*pid, "-KILL");
            }
        }
    }

    // 清 pid 文件
    for (rid, _) in &pids {
        let guard = SUPERVISOR.lock().unwrap();
        if let Some(entry) = guard.as_ref().and_then(|m| m.get(rid)) {
            runner_store::remove_pid(&entry.snapshot.session_id, rid);
        }
    }
}
