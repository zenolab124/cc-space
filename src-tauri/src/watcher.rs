use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, RecursiveMode, Watcher};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// 启动文件监控，检测 ~/.claude/projects/ 下的变化
/// - 变更按 (project, session) 聚合，节流 1 秒发送 "projects-changed" 事件，
///   payload 携带变更集合供前端增量更新；项目目录级事件（新建/删除/重命名）
///   置 full=true 让前端全量兜底。节流窗口内积压的末批变更由 500ms tick 补发，
///   不丢失；tick 仅在有积压时运转，空闲态纯阻塞零唤醒
/// - 增量探测会话 jsonl 新增的 api_error 记录，发送 "session-api-error" 事件
///   （FR-010 外部会话出错兜底；是否属于工作台由前端判定过滤）
/// - 监控 data_dir/routines.json 变化，发送 "routines-changed" 事件（MCP 外部写入感知）
pub fn start(app: &AppHandle) {
    let watch_claude_sessions = crate::engines::claude::default_instance()
        .ok()
        .and_then(|instance| {
            crate::engines::system::get()
                .ok()
                .map(|system| system.registry().is_available(&instance))
        })
        .unwrap_or(false);
    let root = crate::config::projects_dir();

    let data_dir = crate::config::data_dir().to_path_buf();
    let routines_file = data_dir.join("routines.json");
    let runner_commands_file = data_dir.join("runner-commands.json");
    // 运行标记目录：cron 触发的 routine 执行由独立 runner 进程承载，
    // 主 App 只能靠 marker 文件的增删感知「开始/结束」
    let running_dir = crate::routine_run::running_dir(&data_dir);
    let _ = std::fs::create_dir_all(&running_dir);

    let handle = app.clone();
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel::<Event>();

        let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to create file watcher: {}", e);
                return;
            }
        };

        if watch_claude_sessions && root.is_dir() {
            if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
                log::error!("Failed to watch {:?}: {}", root, e);
                return;
            }
        }

        if data_dir.is_dir() {
            if let Err(e) = watcher.watch(&data_dir, RecursiveMode::NonRecursive) {
                log::warn!("Failed to watch data dir {:?}: {}", data_dir, e);
            }
        }
        if running_dir.is_dir() {
            if let Err(e) = watcher.watch(&running_dir, RecursiveMode::NonRecursive) {
                log::warn!("Failed to watch running dir {:?}: {}", running_dir, e);
            }
        }

        if watch_claude_sessions {
            log::info!("Claude session watcher started on {:?}", root);
        }

        // 启动时预记录全部会话文件 size：只对"启动之后新增"的内容做 api_error 探测，
        // 避免把历史错误当新事件误报
        let mut file_sizes = if watch_claude_sessions {
            snapshot_sizes(&root)
        } else {
            HashMap::new()
        };

        // 内置 Agent 工作目录：变化全部静音——Agent 落盘会话不进档案/搜索，
        // 其写盘若触发 projects-changed，前端会因"未知项目"回退全量重扫（打穿 P0-2 增量优化）
        let agent_dirs: HashSet<String> = crate::config::agent_project_dirs().into_iter().collect();

        let mut last_emit = Instant::now() - Duration::from_secs(10);
        let mut last_routines_emit = Instant::now() - Duration::from_secs(10);
        // 节流窗口内累积的会话变更 (project_id, session_id)；full 表示需要全量刷新
        let mut pending_changes: HashSet<(String, String)> = HashSet::new();
        let mut pending_full = false;
        // routines 侧积压：file = routines.json 有写入（需失效缓存+同步调度器），
        // marker = 运行标记增删（只需通知前端刷新）。带补发——marker 的「结束」
        // 事件若被节流吞掉且无后续事件，UI 会永久卡在「运行中」
        let mut pending_routine_file = false;
        let mut pending_routine_marker = false;
        let mut pending_runner_commands = false;
        let mut last_runner_commands_emit = Instant::now() - Duration::from_secs(10);

        loop {
            // 无积压时纯阻塞等事件（空闲零唤醒）；有积压时 500ms 超时兼作补发节拍。
            // 积压只在 emit 的 1s 节流窗口内短暂存在，稳态空闲不产生任何 tick
            let received = if pending_changes.is_empty()
                && !pending_full
                && !pending_routine_file
                && !pending_routine_marker
                && !pending_runner_commands
            {
                match rx.recv() {
                    Ok(event) => Some(event),
                    Err(_) => break,
                }
            } else {
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(event) => Some(event),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            };

            match received {
                Some(event) => {
                    let is_routine = event.paths.iter().any(|p| p == &routines_file);
                    let is_runner_commands = event.paths.iter().any(|p| p == &runner_commands_file);
                    let is_marker = !is_routine
                        && !is_runner_commands
                        && event
                            .paths
                            .iter()
                            .any(|p| p.parent() == Some(running_dir.as_path()));

                    if is_routine {
                        pending_routine_file = true;
                    }
                    if is_marker {
                        pending_routine_marker = true;
                    }
                    if is_runner_commands {
                        pending_runner_commands = true;
                    }
                    flush_routines_emit(
                        &handle,
                        &mut pending_routine_file,
                        &mut pending_routine_marker,
                        &mut last_routines_emit,
                    );
                    flush_runner_commands_emit(
                        &handle,
                        &mut pending_runner_commands,
                        &mut last_runner_commands_emit,
                    );

                    // api_error 增量探测：每个事件都处理（需要 paths，不能合并丢弃）
                    // Agent 目录跳过——Agent 调用失败有自己的 fallback 链和日志，不弹用户通知
                    if watch_claude_sessions {
                        for path in &event.paths {
                            if let Some((sid, pid)) = session_file_ids(&root, path) {
                                if !agent_dirs.contains(&pid) {
                                    probe_api_errors(&handle, path, &sid, &pid, &mut file_sizes);
                                }
                            }
                        }
                    }

                    if watch_claude_sessions && !is_routine {
                        for path in &event.paths {
                            if let Some((sid, pid)) = session_file_ids(&root, path) {
                                // 与 discovery 的会话定义保持一致：排除 agent- 前缀与 Agent 目录，
                                // 否则增量路径会把全量扫描不认的文件 push 成幽灵会话
                                if !sid.starts_with("agent-") && !agent_dirs.contains(&pid) {
                                    // 搜索缓存懒失效：只标 dirty，查询时才重提取
                                    crate::search::invalidate_file(path);
                                    pending_changes.insert((pid, sid));
                                }
                            } else if path.parent() == Some(root.as_path()) {
                                // 项目目录本身的创建/删除/重命名：无法增量定位；
                                // Agent 目录自身的出现/消失除外（对项目列表不可见）
                                let is_agent_dir = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .is_some_and(|n| agent_dirs.contains(n));
                                if !is_agent_dir {
                                    pending_full = true;
                                }
                            }
                            // 其余路径（subagents 深层文件、data_dir 内缓存写入等）
                            // 不影响项目列表，不触发事件
                        }
                    }

                    emit_pending_changes(
                        &handle,
                        &mut pending_changes,
                        &mut pending_full,
                        &mut last_emit,
                    );
                    flush_runner_commands_emit(
                        &handle,
                        &mut pending_runner_commands,
                        &mut last_runner_commands_emit,
                    );
                }
                None => {
                    emit_pending_changes(
                        &handle,
                        &mut pending_changes,
                        &mut pending_full,
                        &mut last_emit,
                    );
                    flush_routines_emit(
                        &handle,
                        &mut pending_routine_file,
                        &mut pending_routine_marker,
                        &mut last_routines_emit,
                    );
                    flush_runner_commands_emit(
                        &handle,
                        &mut pending_runner_commands,
                        &mut last_runner_commands_emit,
                    );
                }
            }
        }
    });
}

/// 距上次发射满 1 秒且有 routines 侧积压时，发送 routines-changed 并清空积压；
/// routines.json 本体变更同时失效缓存并同步调度器（marker 增删不需要）
fn flush_routines_emit(
    app: &AppHandle,
    pending_file: &mut bool,
    pending_marker: &mut bool,
    last_emit: &mut Instant,
) {
    if !*pending_file && !*pending_marker {
        return;
    }
    let now = Instant::now();
    if now.duration_since(*last_emit) < Duration::from_secs(1) {
        return;
    }
    *last_emit = now;
    if *pending_file {
        crate::routines::invalidate_cache();
        crate::routines::sync_scheduler();
    }
    *pending_file = false;
    *pending_marker = false;
    let _ = app.emit("routines-changed", ());
}

/// 距上次发射满 1 秒且有积压时，发送 runner-commands-changed（MCP 写入后前端实时感知）
fn flush_runner_commands_emit(app: &AppHandle, pending: &mut bool, last_emit: &mut Instant) {
    if !*pending {
        return;
    }
    let now = Instant::now();
    if now.duration_since(*last_emit) < Duration::from_secs(1) {
        return;
    }
    *last_emit = now;
    *pending = false;
    let _ = app.emit("runner-commands-changed", ());
}

/// 距上次发射满 1 秒且有积压变更时，发送 projects-changed 并清空积压
fn emit_pending_changes(
    app: &AppHandle,
    pending: &mut HashSet<(String, String)>,
    full: &mut bool,
    last_emit: &mut Instant,
) {
    if pending.is_empty() && !*full {
        return;
    }
    let now = Instant::now();
    if now.duration_since(*last_emit) < Duration::from_secs(1) {
        return;
    }
    *last_emit = now;
    let changed_ids: Vec<(String, String)> = pending.drain().collect();
    let changes: Vec<Value> = changed_ids
        .iter()
        .map(|(pid, sid)| json!({ "projectId": pid, "sessionId": sid }))
        .collect();
    let payload = json!({ "full": *full, "changes": changes });
    if let Ok(instance) = crate::engines::claude::default_instance() {
        if *full {
            crate::engines::system::notify_source_change(
                &instance,
                crate::engines::core::SourceChange {
                    kind: crate::engines::core::SourceChangeKind::FullRefresh,
                    project: None,
                    session: None,
                },
            );
        }
        for (project_id, session_id) in &changed_ids {
            crate::engines::system::notify_source_change(
                &instance,
                crate::engines::core::SourceChange {
                    kind: crate::engines::core::SourceChangeKind::SessionChanged,
                    project: crate::engines::core::ProjectRef::new(instance.clone(), project_id)
                        .ok(),
                    session: crate::engines::core::SessionRef::new(instance.clone(), session_id)
                        .ok(),
                },
            );
        }
    }
    *full = false;
    let _ = app.emit("projects-changed", payload);
    // 会话有真实变更 = 额度正在被消耗：节流触发一次后台额度刷新（内部 90s
    // 闸门 + in-flight 防重），tray 经 quota-cache.json 的 mtime 侦测自动跟进
    crate::quota::notify_session_activity();
}

/// 启动时记录所有项目目录直接子级 .jsonl 的当前大小
fn snapshot_sizes(root: &Path) -> HashMap<PathBuf, u64> {
    let mut sizes = HashMap::new();
    let Ok(projects) = std::fs::read_dir(root) else {
        return sizes;
    };
    for project in projects.filter_map(|e| e.ok()) {
        let dir = project.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.filter_map(|e| e.ok()) {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Ok(meta) = file.metadata() {
                    sizes.insert(path, meta.len());
                }
            }
        }
    }
    sizes
}

/// 判定路径是否为「项目目录直接子级的会话 jsonl」（排除 subagents 等深层文件），
/// 返回 (session_id, project_id)
fn session_file_ids(root: &Path, path: &Path) -> Option<(String, String)> {
    if path.extension()? != "jsonl" {
        return None;
    }
    let project_dir = path.parent()?;
    // 直接子级：父目录的父目录必须是 projects root
    if project_dir.parent()? != root {
        return None;
    }
    let sid = path.file_stem()?.to_str()?.to_string();
    let pid = project_dir.file_name()?.to_str()?.to_string();
    Some((sid, pid))
}

/// 读取文件自上次记录以来的增量内容，扫描新增的 api_error 记录并 emit
fn probe_api_errors(
    app: &AppHandle,
    path: &Path,
    session_id: &str,
    project_id: &str,
    file_sizes: &mut HashMap<PathBuf, u64>,
) {
    let Ok(meta) = std::fs::metadata(path) else {
        // 文件被删：清掉记录
        file_sizes.remove(path);
        return;
    };
    let new_size = meta.len();
    let old_size = match file_sizes.get(path) {
        Some(&s) => s,
        None => {
            // 启动后新出现的文件：从头算（新会话的首条 api_error 也要捕获）
            0
        }
    };
    file_sizes.insert(path.to_path_buf(), new_size);

    if new_size <= old_size {
        // 截断或无增长（重写场景下放弃本轮，下轮以新 size 为基线）
        return;
    }

    let Ok(mut file) = std::fs::File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(old_size)).is_err() {
        return;
    }
    // 单轮增量上限 4MB：防御性，正常追加远小于此
    let len = (new_size - old_size).min(4 * 1024 * 1024) as usize;
    let mut buf = vec![0u8; len];
    if file.read_exact(&mut buf).is_err() {
        return;
    }
    let text = String::from_utf8_lossy(&buf);

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || !line.contains("api_error") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let is_api_error = value.get("type").and_then(Value::as_str) == Some("system")
            && value.get("subtype").and_then(Value::as_str) == Some("api_error");
        if !is_api_error {
            continue;
        }
        let retry_attempt = value.get("retryAttempt").and_then(Value::as_u64);
        let max_retries = value.get("maxRetries").and_then(Value::as_u64);
        if let (Some(attempt), Some(max)) = (retry_attempt, max_retries) {
            if attempt < max {
                continue;
            }
        }
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("API 错误")
            .to_string();
        let _ = app.emit(
            "session-api-error",
            json!({
                "sessionId": session_id,
                "projectId": project_id,
                "content": content,
                "retryAttempt": retry_attempt,
            }),
        );
    }
}
