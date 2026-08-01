use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::models::ContentBlock;
use crate::permission::PermissionService;

/// 按 Claude Code 优先级链读取 permissions.defaultMode
/// Local (.claude/settings.local.json) > Project (.claude/settings.json) > User (~/.claude/settings.local.json) > User (~/.claude/settings.json)
pub fn resolve_default_permission_mode(cwd: &str) -> Option<String> {
    let read_mode = |path: PathBuf| -> Option<String> {
        let text = std::fs::read_to_string(&path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&text).ok()?;
        json.get("permissions")?.get("defaultMode")?.as_str().map(String::from)
    };
    let cwd_path = std::path::Path::new(cwd);
    let candidates = [
        cwd_path.join(".claude/settings.local.json"),
        cwd_path.join(".claude/settings.json"),
        crate::config::claude_root().join("settings.local.json"),
        crate::config::claude_root().join("settings.json"),
    ];
    candidates.into_iter().find_map(read_mode)
}

/// 长活进程会话实例（进程跨轮复用，不杀不重启）
struct SessionProcess {
    child: Child,
    stdin: ChildStdin,
    request_counter: u64,
    channel: Option<String>,
    effort: Option<String>,
    /// 进程当前生效的模型(spawn --model / set_model 下达值;None = CLI 默认)。
    /// 复用判定用:意图回落 None 而进程钉着旧值时必须重启,set_model 无法"切回默认"
    model: Option<String>,
    /// spawn 时的顾问开关(advisor 经 --settings 注入,变更只能重启生效)
    advisor: bool,
    /// spawn 时的 Chrome 集成开关(--chrome 是启动参数,变更只能重启生效)
    chrome: bool,
    /// spawn 时的用户自定义 CLI 参数原始串(逃生舱,变更只能重启生效)
    extra_args: String,
    /// 进程启动墙钟时刻(epoch ms)。异步账本的进程代际判据:启动早于本时刻的
    /// 无终态任务属于已死的前代进程,终态通知永远不会来,不得再判"进行中"
    started_at_ms: i64,
}

/// 仅用于 SessionProcess 尚未接管 Child 的启动阶段错误路径。
fn terminate_and_reap_startup_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// 自定义参数黑名单:会打爆 Monet↔CLI 通信的协议级参数(会话身份/流式格式/权限桥/渠道注入)。
/// 冲突级参数(--model/--effort/--chrome 等)刻意放行——追加在 Monet 参数之后,CLI 后者
/// 覆盖语义让用户能强行改写,这正是逃生舱的意义
const EXTRA_ARGS_DENYLIST: &[&str] = &[
    "-p", "--print", "-c", "--continue",
    "--output-format", "--input-format", "--session-id", "--resume", "--fork-session",
    "--mcp-config", "--permission-prompt-tool", "--settings",
];

/// 解析会话自定义 CLI 参数:shell 式分词(引号值支持),黑名单项连其值一并剔除。
/// 解析失败(未闭合引号等)返回空——宁可不加参数也不给 CLI 传半截
fn parse_extra_args(raw: &str) -> Vec<String> {
    let tokens = shell_words::split(raw).unwrap_or_default();
    let mut out = Vec::new();
    let mut skip_value = false;
    for t in tokens {
        if skip_value {
            skip_value = false;
            // 非 - 开头 = 黑名单 flag 的值,一并剔除;- 开头则不是值,落回正常处理
            if !t.starts_with('-') {
                continue;
            }
        }
        let flag_part = t.split('=').next().unwrap_or(&t);
        if EXTRA_ARGS_DENYLIST.contains(&flag_part) {
            // --flag value 形式:标记跳过下一个值 token(--flag=value 形式无值可跳)
            skip_value = !t.contains('=');
            continue;
        }
        out.push(t);
    }
    out
}

#[cfg(test)]
mod extra_args_tests {
    use super::parse_extra_args;

    #[test]
    fn keeps_normal_args() {
        assert_eq!(parse_extra_args("--betas foo --fallback-model opus"),
            vec!["--betas", "foo", "--fallback-model", "opus"]);
    }

    #[test]
    fn strips_denied_flag_with_value() {
        assert_eq!(parse_extra_args("--output-format json --betas foo"),
            vec!["--betas", "foo"]);
    }

    #[test]
    fn strips_denied_eq_form() {
        assert_eq!(parse_extra_args("--session-id=abc --betas foo"),
            vec!["--betas", "foo"]);
    }

    #[test]
    fn boolean_denied_flag_does_not_eat_next_flag() {
        // --print 是布尔 flag,其后的 --betas 不能被当作值误剔
        assert_eq!(parse_extra_args("--print --betas foo"), vec!["--betas", "foo"]);
    }

    #[test]
    fn quoted_values_survive() {
        assert_eq!(parse_extra_args(r#"--append-system-prompt "hello world""#),
            vec!["--append-system-prompt", "hello world"]);
    }

    #[test]
    fn unclosed_quote_yields_empty() {
        assert!(parse_extra_args(r#"--betas "unclosed"#).is_empty());
    }
}

/// 活跃的长活进程表（sessionId → SessionProcess）
static ACTIVE_PROCESSES: Mutex<Option<HashMap<String, Arc<Mutex<SessionProcess>>>>> =
    Mutex::new(None);

/// 获取 Monet 为指定会话持有的长活进程 PID（check_session_running 排除自有进程用）
pub fn get_own_pid(session_id: &str) -> Option<u32> {
    ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(session_id))
        .map(|sp| sp.lock().unwrap().child.id())
}

/// 自有长活进程的启动时刻(epoch ms)；无进程返回 None
pub fn get_own_started_at_ms(session_id: &str) -> Option<i64> {
    ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(session_id))
        .map(|sp| sp.lock().unwrap().started_at_ms)
}

/// 前端接收的流式事件（全部携带 session_id，前端按会话分发）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    /// 助手消息内容更新（终态快照，作为字符级 delta 的兜底校正）
    AssistantMessage {
        session_id: String,
        message_id: String,
        content: Vec<ContentBlock>,
        /// 本轮实际运行的模型(message.model 真值)
        model: Option<String>,
        /// 该 API message 的 token 用量(快照自带,message 完成即有)——
        /// 前端借此在每段回复结束时即时显示块级 usage,不必等整轮落账
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
    },
    /// 字符级增量到达——content_block_start：某个 index 上出现新块
    BlockStart {
        session_id: String,
        message_id: String,
        index: usize,
        content_block: ContentBlock,
        /// 本轮实际运行的模型(message_start 事件 message.model 真值,随首块带出)
        model: Option<String>,
    },
    /// 字符级增量到达——content_block_delta：某个 index 上的块字段增长
    /// delta 原样透传给前端，由前端按 delta.type 派发：
    /// - text_delta { type, text }
    /// - input_json_delta { type, partial_json }
    /// - thinking_delta { type, thinking }
    /// - signature_delta { type, signature }
    BlockDelta {
        session_id: String,
        message_id: String,
        index: usize,
        delta: Value,
    },
    /// 字符级增量到达——content_block_stop：某个 index 上的块结束
    BlockStop {
        session_id: String,
        message_id: String,
        index: usize,
    },
    /// 流式完成（携带 modelUsage 真值：上下文容量 + token 用量）
    Result {
        session_id: String,
        text: String,
        cost_usd: f64,
        context_window: Option<u64>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    /// 错误
    Error {
        session_id: String,
        message: String,
    },
}

/// 流式链路累计计数，供开发者性能 HUD 对比入口事件率与实际 IPC 批次率。
/// 计数只增不减；前端按相邻采样差值换算每秒速率。
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct StreamPerfCounters {
    pub input_events: u64,
    pub emitted_batches: u64,
    pub emitted_events: u64,
    pub merged_deltas: u64,
}

static STREAM_INPUT_EVENTS: AtomicU64 = AtomicU64::new(0);
static STREAM_EMITTED_BATCHES: AtomicU64 = AtomicU64::new(0);
static STREAM_EMITTED_EVENTS: AtomicU64 = AtomicU64::new(0);
static STREAM_MERGED_DELTAS: AtomicU64 = AtomicU64::new(0);
static STREAM_EVENT_PUMP: OnceLock<SyncSender<StreamDispatch>> = OnceLock::new();

const STREAM_BATCH_WINDOW: Duration = Duration::from_millis(32);
const STREAM_BATCH_CAPACITY: usize = 256;
const STREAM_QUEUE_CAPACITY: usize = 8192;

#[derive(Debug)]
enum StreamDispatch {
    Event(StreamEvent),
    Signal { name: &'static str, payload: Value },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeltaKey {
    message_id: String,
    index: usize,
    delta_type: String,
}

#[derive(Default)]
struct StreamBatch {
    events: Vec<StreamEvent>,
    /// 每个会话只允许与它自己的最后一个 delta 合并；其他会话的事件不构成屏障。
    last_delta_by_session: HashMap<String, (DeltaKey, usize)>,
}

impl StreamBatch {
    fn push(&mut self, event: StreamEvent) -> bool {
        let session_id = event.session_id().to_string();
        if let Some(key) = event.delta_key() {
            if let Some((last_key, event_index)) = self.last_delta_by_session.get(&session_id) {
                if *last_key == key && merge_delta(&mut self.events[*event_index], &event) {
                    return true;
                }
            }
            let event_index = self.events.len();
            self.events.push(event);
            self.last_delta_by_session.insert(session_id, (key, event_index));
        } else {
            // 同会话的语义事件是严格屏障；不同会话仍可继续各自合并。
            self.last_delta_by_session.remove(&session_id);
            self.events.push(event);
        }
        false
    }

    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn len(&self) -> usize {
        self.events.len()
    }
}

impl StreamEvent {
    fn session_id(&self) -> &str {
        match self {
            Self::AssistantMessage { session_id, .. }
            | Self::BlockStart { session_id, .. }
            | Self::BlockDelta { session_id, .. }
            | Self::BlockStop { session_id, .. }
            | Self::Result { session_id, .. }
            | Self::Error { session_id, .. } => session_id,
        }
    }

    fn delta_key(&self) -> Option<DeltaKey> {
        let Self::BlockDelta { message_id, index, delta, .. } = self else {
            return None;
        };
        Some(DeltaKey {
            message_id: message_id.clone(),
            index: *index,
            delta_type: delta.get("type")?.as_str()?.to_string(),
        })
    }

    fn is_delta(&self) -> bool {
        matches!(self, Self::BlockDelta { .. })
    }
}

fn delta_text_field(delta: &Value) -> Option<(&'static str, &str)> {
    let field = match delta.get("type")?.as_str()? {
        "text_delta" => "text",
        "thinking_delta" => "thinking",
        "input_json_delta" => "partial_json",
        "signature_delta" => "signature",
        _ => return None,
    };
    Some((field, delta.get(field)?.as_str()?))
}

/// 合并同一会话、message、block、delta 类型的字符串负载。
fn merge_delta(target: &mut StreamEvent, incoming: &StreamEvent) -> bool {
    let (
        StreamEvent::BlockDelta { delta: target_delta, .. },
        StreamEvent::BlockDelta { delta: incoming_delta, .. },
    ) = (target, incoming)
    else {
        return false;
    };
    let Some((incoming_field, incoming_text)) = delta_text_field(incoming_delta) else {
        return false;
    };
    let Some((target_field, target_text)) = delta_text_field(target_delta) else {
        return false;
    };
    if target_field != incoming_field {
        return false;
    }
    let mut merged = String::with_capacity(target_text.len() + incoming_text.len());
    merged.push_str(target_text);
    merged.push_str(incoming_text);
    let Some(slot) = target_delta.get_mut(target_field) else {
        return false;
    };
    *slot = Value::String(merged);
    true
}

#[cfg(test)]
mod stream_batch_tests {
    use super::*;

    fn text_delta(session: &str, text: &str) -> StreamEvent {
        StreamEvent::BlockDelta {
            session_id: session.to_string(),
            message_id: format!("message-{session}"),
            index: 0,
            delta: json!({ "type": "text_delta", "text": text }),
        }
    }

    fn delta_text(event: &StreamEvent) -> &str {
        let StreamEvent::BlockDelta { delta, .. } = event else {
            panic!("expected block delta");
        };
        delta.get("text").and_then(Value::as_str).unwrap()
    }

    #[test]
    fn coalesces_interleaved_sessions_independently() {
        let mut batch = StreamBatch::default();
        let mut merged = 0;
        for _ in 0..100 {
            for sid in 0..7 {
                merged += usize::from(batch.push(text_delta(&format!("s{sid}"), "x")));
            }
        }

        assert_eq!(batch.len(), 7);
        assert_eq!(merged, 693);
        assert!(batch.events.iter().all(|event| delta_text(event).len() == 100));
    }

    #[test]
    fn semantic_event_is_a_per_session_ordering_barrier() {
        let mut batch = StreamBatch::default();
        assert!(!batch.push(text_delta("s1", "a")));
        assert!(!batch.push(StreamEvent::BlockStop {
            session_id: "s1".to_string(),
            message_id: "message-s1".to_string(),
            index: 0,
        }));
        assert!(!batch.push(text_delta("s1", "b")));

        assert_eq!(batch.len(), 3);
        assert_eq!(delta_text(&batch.events[0]), "a");
        assert!(matches!(batch.events[1], StreamEvent::BlockStop { .. }));
        assert_eq!(delta_text(&batch.events[2]), "b");
    }

    #[test]
    fn preserves_signature_and_partial_json_chunks() {
        let mut batch = StreamBatch::default();
        for (delta_type, field, left, right) in [
            ("signature_delta", "signature", "sig-", "tail"),
            ("input_json_delta", "partial_json", "{\"a\":", "1}"),
        ] {
            let make = |text: &str| {
                let mut delta = json!({ "type": delta_type });
                delta[field] = json!(text);
                StreamEvent::BlockDelta {
                    session_id: delta_type.to_string(),
                    message_id: "m".to_string(),
                    index: 0,
                    delta,
                }
            };
            assert!(!batch.push(make(left)));
            assert!(batch.push(make(right)));
        }

        let serialized = serde_json::to_value(&batch.events).unwrap();
        assert_eq!(serialized[0]["delta"]["signature"], "sig-tail");
        assert_eq!(serialized[1]["delta"]["partial_json"], "{\"a\":1}");
    }
}

pub fn stream_perf_counters() -> StreamPerfCounters {
    StreamPerfCounters {
        input_events: STREAM_INPUT_EVENTS.load(Ordering::Relaxed),
        emitted_batches: STREAM_EMITTED_BATCHES.load(Ordering::Relaxed),
        emitted_events: STREAM_EMITTED_EVENTS.load(Ordering::Relaxed),
        merged_deltas: STREAM_MERGED_DELTAS.load(Ordering::Relaxed),
    }
}

fn emit_stream_batch(app: &AppHandle, batch: StreamBatch) {
    if batch.is_empty() {
        return;
    }
    STREAM_EMITTED_BATCHES.fetch_add(1, Ordering::Relaxed);
    STREAM_EMITTED_EVENTS.fetch_add(batch.len() as u64, Ordering::Relaxed);
    let _ = app.emit("stream-events", batch.events);
}

fn run_stream_event_pump(app: AppHandle, receiver: Receiver<StreamDispatch>) {
    while let Ok(first) = receiver.recv() {
        match first {
            StreamDispatch::Signal { name, payload } => {
                let _ = app.emit(name, payload);
            }
            StreamDispatch::Event(event) if !event.is_delta() => {
                let mut batch = StreamBatch::default();
                batch.push(event);
                emit_stream_batch(&app, batch);
            }
            StreamDispatch::Event(event) => {
                let mut batch = StreamBatch::default();
                batch.push(event);
                let deadline = Instant::now() + STREAM_BATCH_WINDOW;
                loop {
                    let now = Instant::now();
                    if now >= deadline || batch.len() >= STREAM_BATCH_CAPACITY {
                        emit_stream_batch(&app, batch);
                        break;
                    }
                    match receiver.recv_timeout(deadline.saturating_duration_since(now)) {
                        Ok(StreamDispatch::Event(next)) => {
                            let barrier = !next.is_delta();
                            if batch.push(next) {
                                STREAM_MERGED_DELTAS.fetch_add(1, Ordering::Relaxed);
                            }
                            if barrier || batch.len() >= STREAM_BATCH_CAPACITY {
                                emit_stream_batch(&app, batch);
                                break;
                            }
                        }
                        Ok(StreamDispatch::Signal { name, payload }) => {
                            emit_stream_batch(&app, batch);
                            let _ = app.emit(name, payload);
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            emit_stream_batch(&app, batch);
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            emit_stream_batch(&app, batch);
                            return;
                        }
                    }
                }
            }
        }
    }
}

fn stream_event_sender(app: &AppHandle) -> &'static SyncSender<StreamDispatch> {
    STREAM_EVENT_PUMP.get_or_init(|| {
        let (sender, receiver) = sync_channel(STREAM_QUEUE_CAPACITY);
        let worker_app = app.clone();
        std::thread::Builder::new()
            .name("stream-event-pump".to_string())
            .spawn(move || run_stream_event_pump(worker_app, receiver))
            .expect("stream event pump thread must start");
        sender
    })
}

/// 所有 WebView 流式消息的统一入口：先进入有界队列，再由单一线程跨会话合批发射。
fn dispatch_stream_event(app: &AppHandle, event: StreamEvent) {
    STREAM_INPUT_EVENTS.fetch_add(1, Ordering::Relaxed);
    let _ = stream_event_sender(app).send(StreamDispatch::Event(event));
}

/// 与流内容存在先后关系的生命周期信号也走同一队列，保证 result/error 已先抵达前端。
fn dispatch_stream_signal(app: &AppHandle, name: &'static str, payload: Value) {
    let _ = stream_event_sender(app).send(StreamDispatch::Signal { name, payload });
}

/// 查找 claude 可执行文件路径（薄 wrapper，事实源在 claude_locator）。
/// 探测全败时保留 env fallback 兜底，行为不比旧版倒退；
/// 需要结构化错误的消费方（定时任务、设置页）直接用 claude_locator。
pub fn find_claude() -> (String, Vec<String>) {
    match crate::claude_locator::locate() {
        Ok(l) => (l.path.to_string_lossy().into_owned(), vec![]),
        Err(e) => {
            eprintln!("[claude-locator] {}", e);
            ("/usr/bin/env".to_string(), vec!["claude".to_string()])
        }
    }
}

// 增强 PATH 实现已抽至 path_env（runner 二进制经 #[path] 共享同一份源），
// 此处 re-export 保持既有调用点不变
pub use crate::path_env::enhanced_path;

/// 向长活进程 stdin 写入一行 JSON
fn write_stdin(stdin: &mut ChildStdin, msg: &Value) -> Result<(), String> {
    let line = serde_json::to_string(msg).map_err(|e| format!("JSON 序列化失败: {}", e))?;
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("stdin 写入失败: {}", e))
}

/// 向会话进程组发信号（macOS/Linux）。spawn 时已 process_group(0)，组 ID = CLI PID，
/// 组信号可连带回收 MCP 子进程树（monet-mcp/playwright/adb 等）。
/// 组信号失败时回退单 PID：兼容升级前 spawn 的存量进程（未自立进程组）。
#[cfg(unix)]
fn signal_session(pid: u32, sig: &str) {
    let group_ok = Command::new("kill")
        .args([sig, "--", &format!("-{}", pid)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !group_ok {
        let _ = Command::new("kill").args([sig, &pid.to_string()]).output();
    }
}

/// Windows 无信号语义：TERM/KILL 一律 taskkill /T /F 整树强杀。
/// CLI 的 JSONL 逐行落盘，强杀不丢会话数据（与 close_all_sessions 注释同一依据）；
/// 已死进程上的第二次调用报 not found 被吞，与 unix 二段式击杀兼容。
#[cfg(windows)]
fn signal_session(pid: u32, _sig: &str) {
    use crate::proc_ext::HideConsole;
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .hide_console()
        .output();
}

/// 断链探测：同 id 的 jsonl 是否存在于其他 cwd 编码目录（如 EnterWorktree 迁走的）。
/// 命中说明"这不是新会话，是历史不在预期位置"——静默 --session-id 会以空历史
/// 重生同名会话，复刻「会话被覆盖」事故。
fn find_session_elsewhere(session_id: &str, expected: &Path) -> Option<PathBuf> {
    let root = crate::config::projects_dir();
    let name = format!("{}.jsonl", session_id);
    for entry in std::fs::read_dir(&root).ok()?.filter_map(|e| e.ok()) {
        let candidate = entry.path().join(&name);
        if candidate != expected && candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 打开会话进程：spawn 长活 CLI + 初始化握手 + 启动 stdout 读取线程
#[allow(clippy::too_many_arguments)] // 会话参数透传，拆结构体收益不大
fn open_session(
    app: &AppHandle,
    session_id: &str,
    cwd: &str,
    model: Option<&str>,
    effort: Option<&str>,
    channel: Option<&str>,
    advisor: bool,
    chrome: bool,
    fork_source: Option<&str>,
    extra_args: Option<&str>,
    permission_mode: Option<&str>,
    append_system_prompt: Option<&str>,
    force_new: bool,
) -> Result<(), String> {
    if !std::path::Path::new(cwd).is_dir() {
        return Err(format!("工作目录不存在: {}", cwd));
    }

    // 1. 权限服务（per-session 生命周期：open 时 start，进程退出时 stop）
    let permission_service = PermissionService::start(app.clone(), session_id)
        .map_err(|e| format!("权限服务启动失败：{}", e))?;
    let perm_endpoint = permission_service.endpoint().to_string();
    let perm_token = permission_service.token().to_string();

    // 2. MCP 二进制
    let mcp_binary = match find_mcp_binary() {
        Some(p) => p,
        None => {
            PermissionService::stop_for(session_id);
            return Err("未找到 monet-mcp 二进制，无法启动权限服务".to_string());
        }
    };

    // 3. MCP 配置
    let mcp_config = json!({
        "mcpServers": {
            "monet": {
                "type": "stdio",
                "command": mcp_binary.to_string_lossy().into_owned(),
                "args": [],
                "env": {
                    "MONET_PERMISSION_ADDR": perm_endpoint.clone(),
                    "MONET_PERMISSION_TOKEN": perm_token
                }
            }
        }
    })
    .to_string();

    // 4. CLI 参数（不带 --print，不传消息文本；加 --input-format stream-json）
    let (executable, prefix_args) = find_claude();
    let mut args = prefix_args;
    // cwd 先 canonicalize 再编码:CLI 落盘前对 cwd 做 realpath(mac 的 /tmp → /private/tmp,
    // Windows 的 junction/subst/8.3 短名/磁盘大小写校正),用原始 cwd 编码会对着
    // 不存在的目录做落盘判断,resume/fork 三态失灵。Windows canonicalize 产生的
    // \\?\ verbatim 前缀由 encode_project_dir 剥离,无需分平台
    let cwd_real = std::fs::canonicalize(cwd)
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| cwd.to_string());
    // 编码必须与 CLI 同规则(非字母数字一律 →'-')：仅替换 '/' 会漏掉 '.'/'_'
    // 以及 Windows 的 ':'/'\\'，致 resume/fork 三态对着错误目录判定
    let project_dir = crate::config::projects_dir()
        .join(crate::config::encode_project_dir(std::path::Path::new(&cwd_real)));
    let session_file = project_dir.join(format!("{}.jsonl", session_id));
    // 会话身份参数三态:已落盘 → resume 自己;分叉意图且源已落盘 → CLI 原生 fork(resume 源 +
    // --fork-session + 指定新 ID,历史行 sessionId 由 CLI 重写,替代旧 fs::copy 仿造);
    // 否则全新会话。文件已存在时分叉意图残留无害,走正常 resume 忽略之;
    // 源未落盘(草稿源分叉)时意图同样无害,退化走新建分支——与前者对称
    let fork_src = fork_source
        .filter(|s| !s.is_empty())
        .filter(|src| project_dir.join(format!("{}.jsonl", src)).is_file());
    let (session_args, resumed) = if session_file.is_file() {
        (vec!["--resume".to_string(), session_id.to_string()], true)
    } else if let Some(src) = fork_src {
        (
            vec![
                "--resume".to_string(),
                src.to_string(),
                "--fork-session".to_string(),
                "--session-id".to_string(),
                session_id.to_string(),
            ],
            true,
        )
    } else {
        if !force_new {
            // jsonl 不在预期位置——全局探测：是迁走了还是真的新会话？
            if let Some(actual) = find_session_elsewhere(session_id, &session_file) {
                return Err(format!(
                    "SESSION_ELSEWHERE:{}",
                    actual.to_string_lossy()
                ));
            }
        }
        (vec!["--session-id".to_string(), session_id.to_string()], false)
    };
    args.extend(session_args);
    args.extend([
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        // 真流式:CLI 2.1.177+ 默认只发 assistant 快照(整块,可能多次),该 flag 恢复
        // content_block_delta 字符级增量(实测 thinking/text 平均十几字符一个事件)。
        // 首字延迟从"整条消息生成完"缩到 API 首 token;快照仍会兜底到达,
        // 前端 feedSnapshotText 按已知长度差量对账,双路径幂等不双写
        "--include-partial-messages".to_string(),
        "--thinking-display".to_string(),
        "summarized".to_string(),
        "--mcp-config".to_string(),
        mcp_config,
        "--permission-prompt-tool".to_string(),
        "mcp__monet__approve_tool_use".to_string(),
    ]);

    // Chrome 集成(Claude in Chrome 扩展 + Native Messaging):按需开启,
    // 浏览器工具常驻吃上下文,默认不传;<2.1.211 的 CLI 在 Chrome 未运行时可能挂起
    if chrome {
        args.push("--chrome".to_string());
    }

    let ultracode = effort == Some("ultracode");

    // 5. 渠道/ultracode/顾问注入（合成 --settings）
    let channel_opt = channel.filter(|s| !s.is_empty() && *s != crate::channels::OFFICIAL_ID);
    let channel_injection =
        match crate::channels::prepare_injection(channel_opt, session_id, ultracode, advisor) {
            Ok(Some(inj)) => {
                args.push("--settings".to_string());
                args.push(inj.settings_arg.clone());
                Some(inj)
            }
            Ok(None) => None,
            Err(e) => {
                PermissionService::stop_for(session_id);
                return Err(format!("会话配置加载失败:{}", e));
            }
        };

    if let Some(m) = model.filter(|s| !s.is_empty()) {
        args.push("--model".to_string());
        args.push(m.to_string());
    }
    if let Some(e) = effort.filter(|s| !s.is_empty() && *s != "ultracode") {
        args.push("--effort".to_string());
        args.push(e.to_string());
    }
    let effective_mode = permission_mode
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| resolve_default_permission_mode(cwd));
    if let Some(ref mode) = effective_mode {
        args.push("--permission-mode".to_string());
        args.push(mode.clone());
    }
    if let Some(prompt) = append_system_prompt.filter(|s| !s.is_empty()) {
        args.push("--append-system-prompt".to_string());
        args.push(prompt.to_string());
    }
    // 用户自定义 CLI 参数(逃生舱):置于全部 Monet 参数之后,冲突项按 CLI 后者覆盖生效
    if let Some(raw) = extra_args.filter(|s| !s.trim().is_empty()) {
        args.extend(parse_extra_args(raw));
    }

    // 6. Spawn（stdin/stdout/stderr 全 piped）
    let mut command = Command::new(&executable);
    command
        .args(&args)
        .current_dir(cwd)
        .env("PATH", enhanced_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some((k, v)) = crate::config::claude_config_dir_env() {
        command.env(k, v);
    }
    // CLI 自立进程组（组 ID = CLI PID）：MCP 子进程随组，signal_session 组信号可整树回收
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    // Windows 进程树由 taskkill /T 按父子关系回收，无需进程组；只抑制控制台闪窗
    #[cfg(windows)]
    {
        use crate::proc_ext::HideConsole;
        command.hide_console();
    }
    if let Some(inj) = &channel_injection {
        for key in &inj.clear_env {
            command.env_remove(key);
        }
        for (k, v) in &inj.env {
            command.env(k, v);
        }
    }
    let mut child = command.spawn().map_err(|e| {
        if let Some(inj) = &channel_injection {
            crate::channels::cleanup_runtime_file(&inj.runtime_path);
        }
        PermissionService::stop_for(session_id);
        format!("启动 claude 失败: {}", e)
    })?;

    // 7. Take handles
    let Some(mut stdin) = child.stdin.take() else {
        terminate_and_reap_startup_child(&mut child);
        return Err("无法获取 stdin".to_string());
    };
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap_startup_child(&mut child);
        return Err("无法获取 stdout".to_string());
    };
    let stderr = child.stderr.take();

    // 8. 初始化握手（没有这个握手，进程不处理任何消息）
    let init_msg = json!({
        "type": "control_request",
        "request_id": "init-1",
        "request": {"subtype": "initialize"}
    });
    if let Err(e) = write_stdin(&mut stdin, &init_msg) {
        PermissionService::stop_for(session_id);
        if let Some(inj) = &channel_injection {
            crate::channels::cleanup_runtime_file(&inj.runtime_path);
        }
        terminate_and_reap_startup_child(&mut child);
        return Err(format!("初始化握手写入失败: {}", e));
    }

    let mut reader = BufReader::new(stdout);
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let bytes_read = match reader.read_line(&mut line_buf) {
            Ok(bytes_read) => bytes_read,
            Err(e) => {
                PermissionService::stop_for(session_id);
                if let Some(inj) = &channel_injection {
                    crate::channels::cleanup_runtime_file(&inj.runtime_path);
                }
                terminate_and_reap_startup_child(&mut child);
                return Err(format!("初始化握手读取失败: {}", e));
            }
        };
        if bytes_read == 0 {
            PermissionService::stop_for(session_id);
            terminate_and_reap_startup_child(&mut child);
            let stderr_msg = stderr
                .map(|s| {
                    let mut r = BufReader::new(s);
                    let mut buf = String::new();
                    let _ = std::io::Read::read_to_string(&mut r, &mut buf);
                    buf
                })
                .unwrap_or_default();
            if let Some(inj) = &channel_injection {
                crate::channels::cleanup_runtime_file(&inj.runtime_path);
            }
            return Err(format!(
                "进程在初始化阶段退出{}",
                if stderr_msg.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", stderr_msg.trim())
                }
            ));
        }
        if let Ok(v) = serde_json::from_str::<Value>(&line_buf) {
            let t = v.get("type").and_then(|t| t.as_str());
            if t == Some("control_response") {
                break;
            }
            // 握手阶段也可能收到 hook 事件（SessionStart hooks 在 initialize 前执行）
            if t == Some("system") {
                if let Some(sub) = v.get("subtype").and_then(|s| s.as_str()) {
                    if sub == "hook_started" || sub == "hook_response" {
                        let mut payload = v;
                        payload.as_object_mut().map(|o| o.insert("session_id".to_string(), json!(session_id)));
                        let _ = app.emit("session-hook", &payload);
                    }
                }
            }
        }
    }

    // 9. 默认开启 Remote Control（进程活着就有 RC，关进程自动断）
    let rc_msg = json!({
        "type": "control_request",
        "request_id": "rc-init",
        "request": {"subtype": "remote_control", "enabled": true}
    });
    let _ = write_stdin(&mut stdin, &rc_msg);

    // 10. 存入 ACTIVE_PROCESSES
    let pid = child.id();
    eprintln!("[long-lived] 新建进程 PID={} 会话={}", pid, &session_id[..session_id.len().min(8)]);
    let started_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    // started_at_ms 随 connected 事件下发：warm 重启（needs_restart）时旧进程 EOF
    // 因 superseded 不发 exited，前端 processAlive 无边沿，此事件是代际锚点
    // 唯一不依赖竞态方向的刷新通道
    let _ = app.emit("session-connected", json!({
        "session_id": session_id,
        "resumed": resumed,
        "cwd": cwd,
        "started_at_ms": started_at_ms,
    }));
    let sp_arc = Arc::new(Mutex::new(SessionProcess {
        child,
        stdin,
        request_counter: 1,
        channel: channel.map(|s| s.to_string()),
        effort: effort.map(|s| s.to_string()),
        model: model.filter(|s| !s.is_empty()).map(|s| s.to_string()),
        advisor,
        chrome,
        extra_args: extra_args.unwrap_or("").to_string(),
        started_at_ms,
    }));
    ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(session_id.to_string(), sp_arc.clone());

    // 10a. 启动 stdout 读取线程（reader 已消费掉握手响应，后续全是流式事件）
    let handle = app.clone();
    let sid = session_id.to_string();
    let runtime_file = channel_injection.map(|inj| inj.runtime_path);
    std::thread::spawn(move || {
        read_stream(reader, stderr, sp_arc.clone(), &handle, &sid);
        PermissionService::stop_if_endpoint(&sid, &perm_endpoint);
        if let Some(p) = runtime_file {
            crate::channels::cleanup_runtime_file(&p);
        }
    });

    Ok(())
}

/// 发送消息（自动 open 会话进程；替代旧 start_streaming）
#[allow(clippy::too_many_arguments)] // 会话参数透传，拆结构体收益不大
pub fn send_message(
    app: &AppHandle,
    session_id: &str,
    cwd: &str,
    message: &str,
    model: Option<&str>,
    effort: Option<&str>,
    channel: Option<&str>,
    advisor: bool,
    chrome: bool,
    fork_source: Option<&str>,
    extra_args: Option<&str>,
    images: Option<&[serde_json::Value]>,
    permission_mode: Option<&str>,
    append_system_prompt: Option<&str>,
    force_new: bool,
) -> Result<(), String> {
    let mut exists = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|m| m.contains_key(session_id));

    if exists {
        let needs_restart = ACTIVE_PROCESSES
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|m| m.get(session_id).cloned())
            .is_some_and(|arc| {
                let sp = arc.lock().unwrap();
                sp.channel.as_deref() != channel
                    || sp.effort.as_deref() != effort
                    // advisor 经 --settings 注入,变更只能重启生效(否则只有主模型锁定生效、顾问没挂上)
                    || sp.advisor != advisor
                    // 模型意图回落默认(None)而进程钉着上次 set_model 的具体值:
                    // set_model 无法"切回默认",不重启就是旧模型粘滞(界面默认、实跑旧值)
                    || (model.is_none() && sp.model.is_some())
                    // --chrome 是启动参数,开关变更(含关闭卸载浏览器工具省上下文)只能重启生效
                    || sp.chrome != chrome
                    // 自定义 CLI 参数同为启动参数,变更只能重启生效
                    || sp.extra_args != extra_args.unwrap_or("")
            });
        if needs_restart {
            eprintln!("[long-lived] 渠道/effort/advisor/chrome/模型回落变更，重启进程 会话={}", &session_id[..session_id.len().min(8)]);
            close_session(session_id);
            exists = false;
        }
    }

    if !exists {
        open_session(app, session_id, cwd, model, effort, channel, advisor, chrome, fork_source, extra_args, permission_mode, append_system_prompt, force_new)?;
    }

    let process = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(session_id).cloned())
        .ok_or("会话进程不存在")?;

    let mut sp = process.lock().unwrap();
    if exists {
        eprintln!("[long-lived] 复用进程 PID={} 会话={}", sp.child.id(), &session_id[..session_id.len().min(8)]);
        if let Some(m) = model.filter(|s| !s.is_empty()) {
            sp.request_counter += 1;
            let req_id = format!("set-model-{}", sp.request_counter);
            let ctrl = json!({
                "type": "control_request",
                "request_id": req_id,
                "request": {"subtype": "set_model", "model": m}
            });
            write_stdin(&mut sp.stdin, &ctrl)?;
            sp.model = Some(m.to_string());
        }
    }

    if let Some(mode) = permission_mode.filter(|m| !m.is_empty()) {
        sp.request_counter += 1;
        let req_id = format!("set-perm-mode-{}", sp.request_counter);
        let ctrl = json!({
            "type": "control_request",
            "request_id": req_id,
            "request": {"subtype": "set_permission_mode", "permission_mode": mode}
        });
        let _ = write_stdin(&mut sp.stdin, &ctrl);
    }
    let mut content: Vec<serde_json::Value> = Vec::new();
    if let Some(imgs) = images {
        content.extend_from_slice(imgs);
    }
    content.push(json!({"type": "text", "text": message}));
    let msg = json!({
        "type": "user",
        "session_id": "",
        "message": {
            "role": "user",
            "content": content
        },
        "parent_tool_use_id": null
    });
    write_stdin(&mut sp.stdin, &msg)
}

/// 开关 Remote Control（通过 control_request 动态启用/禁用；进程未启动时自动连接）
#[allow(clippy::too_many_arguments)] // 会话参数透传，拆结构体收益不大
pub fn toggle_remote_control(
    app: &AppHandle,
    session_id: &str,
    cwd: &str,
    model: Option<&str>,
    effort: Option<&str>,
    channel: Option<&str>,
    advisor: bool,
    chrome: bool,
    fork_source: Option<&str>,
    extra_args: Option<&str>,
    enabled: bool,
    permission_mode: Option<&str>,
) -> Result<(), String> {
    let exists = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|m| m.contains_key(session_id));

    if !exists {
        open_session(app, session_id, cwd, model, effort, channel, advisor, chrome, fork_source, extra_args, permission_mode, None, false)?;
    }

    let process = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(session_id).cloned())
        .ok_or("会话进程不存在")?;

    let mut sp = process.lock().unwrap();
    eprintln!("[long-lived] Remote Control enabled={} PID={} 会话={}", enabled, sp.child.id(), &session_id[..session_id.len().min(8)]);
    sp.request_counter += 1;
    // 语义编进 request_id：CLI 的 response 不含 enabled 值，read_stream 只能靠它还原请求意图
    let req_id = format!("rc-{}-{}", if enabled { "on" } else { "off" }, sp.request_counter);
    let msg = json!({
        "type": "control_request",
        "request_id": req_id,
        "request": {"subtype": "remote_control", "enabled": enabled}
    });
    write_stdin(&mut sp.stdin, &msg)
}

/// 中断当前回复（发 interrupt 控制消息，不杀进程）
pub fn interrupt_session(session_id: &str) -> Result<(), String> {
    let process = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(session_id).cloned());

    match process {
        Some(p) => {
            let mut sp = p.lock().unwrap();
            eprintln!("[long-lived] 中断进程 PID={} 会话={}", sp.child.id(), &session_id[..session_id.len().min(8)]);
            sp.request_counter += 1;
            let req_id = format!("interrupt-{}", sp.request_counter);
            let msg = json!({
                "type": "control_request",
                "request_id": req_id,
                "request": {"subtype": "interrupt"}
            });
            write_stdin(&mut sp.stdin, &msg)
        }
        None => Ok(()),
    }
}

/// 运行时切换权限模式
pub fn set_permission_mode(session_id: &str, mode: &str) -> Result<(), String> {
    let process = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(session_id).cloned());

    match process {
        Some(p) => {
            let mut sp = p.lock().unwrap();
            sp.request_counter += 1;
            let req_id = format!("set-perm-mode-{}", sp.request_counter);
            let msg = json!({
                "type": "control_request",
                "request_id": req_id,
                "request": {"subtype": "set_permission_mode", "permission_mode": mode}
            });
            write_stdin(&mut sp.stdin, &msg)
        }
        None => Err("会话进程不存在".to_string()),
    }
}

/// 关闭会话进程（SIGTERM → 5s → SIGKILL）
pub fn close_session(session_id: &str) {
    let process_arc = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|m| m.remove(session_id));

    if let Some(process_arc) = process_arc {
        let pid = {
            let mut sp = process_arc.lock().unwrap();
            graceful_shutdown(&mut sp);
            sp.child.id()
        };
        signal_session(pid, "-TERM");

        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if let Ok(mut sp) = process_arc.lock() {
                if sp.child.try_wait().ok().flatten().is_none() {
                    signal_session(pid, "-KILL");
                    let _ = sp.child.wait();
                }
            }
        });
    }

    PermissionService::stop_for(session_id);
}

/// 关闭所有活跃会话进程（应用退出时调用）。
/// 必须同步等待：主进程退出后无人兜底，close_session 那种 detached 线程在此场景随进程消亡。
/// SIGTERM 全体 → 轮询最多 400ms → 对未退者 SIGKILL 整组。窗口收窄的依据：进程组 SIGKILL
/// 本身就保证不留孤儿，JSONL 逐行落盘强杀不丢会话数据——优雅窗口只是让 CLI 少打错误日志，
/// 不值得让退出卡秒级（quit_app 同步调用本函数，等待时长 = 用户按 Cmd+Q 的延迟）。
pub fn close_all_sessions() {
    let processes: Vec<(String, Arc<Mutex<SessionProcess>>)> = ACTIVE_PROCESSES
        .lock()
        .unwrap()
        .as_mut()
        .map(|m| m.drain().collect())
        .unwrap_or_default();

    let mut pids: Vec<u32> = Vec::with_capacity(processes.len());
    for (sid, process_arc) in &processes {
        if let Ok(mut sp) = process_arc.lock() {
            graceful_shutdown(&mut sp);
            let pid = sp.child.id();
            signal_session(pid, "-TERM");
            pids.push(pid);
        }
        PermissionService::stop_for(sid);
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
    loop {
        let all_exited = processes.iter().all(|(_, arc)| {
            arc.lock()
                .map(|mut sp| sp.child.try_wait().ok().flatten().is_some())
                .unwrap_or(true)
        });
        if all_exited || std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    for (i, (_, arc)) in processes.iter().enumerate() {
        if let Ok(mut sp) = arc.lock() {
            if sp.child.try_wait().ok().flatten().is_none() {
                signal_session(pids[i], "-KILL");
                let _ = sp.child.wait();
            }
        }
    }
}

/// 定向发信号：group=true 发整组（pgid = pid），否则单 PID。清扫场景不做回退——
/// 老残留进程的 pgid 可能是终端 shell 组，组信号会误伤，必须按 pgid==pid 判定后精确选择。
#[cfg(unix)]
fn send_sig(pid: u32, sig: &str, group: bool) {
    if group {
        let _ = Command::new("kill")
            .args([sig, "--", &format!("-{}", pid)])
            .output();
    } else {
        let _ = Command::new("kill").args([sig, &pid.to_string()]).output();
    }
}

/// 启动残留清扫：上次崩溃/强杀/dev Ctrl+C 遗留的会话进程树。
/// 指纹：命令行含 `.monet/runtime/`（Monet 注入的 --settings 路径）或 `/monet-mcp`。
/// ppid==1 是必要条件（父进程已死被 launchd 收养）——其他 Monet 实例的活会话 ppid 是该实例，不会命中。
/// 须在本实例 spawn 任何会话之前调用（setup 阶段）。
pub fn cleanup_orphans() {
    #[cfg(unix)]
    {
        let output = match Command::new("ps")
            .args(["-axo", "pid=,ppid=,pgid=,command="])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return,
        };
        let text = String::from_utf8_lossy(&output.stdout);
        // (pid, 可整组杀)。--append-system-prompt 含真实换行会把命令行拆成多行，
        // 但指纹字段（--settings/--mcp-config）位于参数前段与 pid 同行，续行解析 pid 失败自然跳过
        let mut targets: Vec<(u32, bool)> = Vec::new();
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(pid), Some(ppid), Some(pgid)) = (
                parts.next().and_then(|s| s.parse::<u32>().ok()),
                parts.next().and_then(|s| s.parse::<u32>().ok()),
                parts.next().and_then(|s| s.parse::<u32>().ok()),
            ) else {
                continue;
            };
            if ppid != 1 {
                continue;
            }
            if !(line.contains(".monet/runtime/") || line.contains("/monet-mcp")) {
                continue;
            }
            targets.push((pid, pgid == pid));
        }
        if targets.is_empty() {
            return;
        }
        eprintln!(
            "[cleanup_orphans] 清理 {} 个残留会话进程: {:?}",
            targets.len(),
            targets.iter().map(|t| t.0).collect::<Vec<_>>()
        );
        for (pid, grouped) in &targets {
            send_sig(*pid, "-TERM", *grouped);
        }
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            for (pid, grouped) in &targets {
                let alive = Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if alive {
                    send_sig(*pid, "-KILL", *grouped);
                }
            }
        });
    }
}

fn graceful_shutdown(sp: &mut SessionProcess) {
    sp.request_counter += 1;
    let req_id = format!("shutdown-{}", sp.request_counter);
    let _ = write_stdin(&mut sp.stdin, &json!({
        "type": "control_request",
        "request_id": req_id,
        "request": {"subtype": "remote_control", "enabled": false}
    }));
}

fn emit_error(app: &AppHandle, session_id: &str, message: String) {
    dispatch_stream_event(
        app,
        StreamEvent::Error { session_id: session_id.to_string(), message },
    );
    dispatch_stream_signal(app, "stream-done", json!({ "session_id": session_id }));
}

/// 定位 monet-mcp 二进制
///
/// 顺序：
/// 1. 环境变量 `MONET_MCP_BIN`（手动覆盖）
/// 2. 与当前进程同目录（生产打包后 sidecar 就在主程序旁）
/// 3. cargo target 目录（开发模式：current_exe 直接就是 target/{debug,release}/app）
fn find_mcp_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MONET_MCP_BIN") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let (bundled, dev) = if cfg!(windows) {
        ("monet-mcp.exe", "monet_mcp.exe")
    } else {
        ("monet-mcp", "monet_mcp")
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(bundled);
            if candidate.is_file() {
                return Some(candidate);
            }
            // dev: tauri 启动时 current_exe 在 target/debug，二进制名为 monet_mcp
            let candidate2 = dir.join(dev);
            if candidate2.is_file() {
                return Some(candidate2);
            }
        }
    }
    None
}

/// 读取长活进程 stdout——逐行解析流式事件，result 立即 emit + stream-done 标记轮次结束，
/// 循环仅在 stdout EOF（进程退出）时终止
fn read_stream(
    reader: BufReader<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    process: Arc<Mutex<SessionProcess>>,
    app: &AppHandle,
    session_id: &str,
) {
    // (message_id, model):message_start 写入,后续 content_block_* 读取;model 是本轮真值
    let mut current_message_id: Option<(String, Option<String>)> = None;

    for line in reader.lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let raw_type = value
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        if let Some(event) = decode_stream_event(&value, session_id, &mut current_message_id) {
            dispatch_stream_event(app, event);
        }

        // hook 事件转发给前端（system 类型的 hook_started / hook_response）
        if raw_type == "system" {
            if let Some(subtype) = value.get("subtype").and_then(|s| s.as_str()) {
                if subtype == "hook_started" || subtype == "hook_response" {
                    let mut payload = value.clone();
                    payload.as_object_mut().map(|o| o.insert("session_id".to_string(), json!(session_id)));
                    let _ = app.emit("session-hook", &payload);
                }
            }
        }

        // Remote Control 判决回报：success/error 都上报，前端 rcActive 完全由本事件驱动
        // （不能乐观置位——判决与 invoke 返回并发，先到的判决会被晚写的乐观值覆盖）。
        // 请求意图编码在 request_id：rc-init（连接自动开）/rc-on-N/rc-off-N（手动）。
        // 第三方渠道实测 CLI 回 error "Remote Control initialization failed"
        if raw_type == "control_response" {
            if let Some(resp) = value.get("response") {
                let req_id = resp.get("request_id").and_then(|v| v.as_str()).unwrap_or("");
                if req_id.starts_with("rc-") {
                    let is_err = resp.get("subtype").and_then(|v| v.as_str()) == Some("error");
                    let wanted_on = !req_id.starts_with("rc-off");
                    // 成功 = 达成所愿；失败 = 维持原状（开失败→仍关，关失败→仍开）
                    let active = if is_err { !wanted_on } else { wanted_on };
                    let err = resp.get("error").and_then(|v| v.as_str());
                    eprintln!(
                        "[long-lived] Remote Control 判决 req={} active={} 会话={}{}",
                        req_id, active, &session_id[..session_id.len().min(8)],
                        err.map(|e| format!(" err={}", e)).unwrap_or_default()
                    );
                    let _ = app.emit("rc-status", json!({
                        "session_id": session_id,
                        "active": active,
                        // 手动开关的判决才触发 toast；连接时自动开启的失败静默（防第三方渠道每次连接被骚扰）
                        "manual": req_id != "rc-init",
                        "error": if is_err { json!(err.unwrap_or("Remote Control unavailable")) } else { Value::Null },
                    }));
                }
            }
        }

        // "result" 标记一轮结束（进程继续活着等下一条 stdin 消息）。
        // 轮次归属：CLI 自发轮（task-notification 唤醒的后台任务收尾轮）的 result 带
        // origin 字段（如 {"kind":"task-notification"}），用户 send 的轮没有——实测确认。
        // 前端靠 initiator 区分：auto 轮不得冒领用户消息的 streaming 标志/落账收尾，
        // 否则后台任务期间发消息会出现轮次边界错乱（消息重复出现/消失）
        if raw_type == "result" {
            let is_auto = value.get("origin").is_some_and(|v| !v.is_null());
            dispatch_stream_signal(
                app,
                "stream-done",
                json!({
                    "session_id": session_id,
                    "initiator": if is_auto { "auto" } else { "user" },
                }),
            );
        }
    }

    // stdout EOF —— 进程已退出

    // 所有退出路径都在 reader 线程统一 wait。close_session / 热重启会先从活跃表移除，
    // 但 `Child` Drop 本身不会回收退出状态；只在“非预期退出”分支 wait 会让预期关闭
    // 与被新代际顶替的进程短暂或永久成为 zombie。
    let exit_status = {
        let mut sp = process.lock().unwrap();
        sp.child.wait().ok()
    };

    // 判断退出性质（close_session 会先从 map 移除再 SIGTERM）：
    // - map 中仍是本进程 = 非预期退出，需要上报错误
    // - map 中是别的 Arc = 已被新进程顶替（模型/渠道变更重启），会话进程实际还活着
    let (was_unexpected, superseded) = {
        let mut map = ACTIVE_PROCESSES.lock().unwrap();
        let is_self = map
            .as_ref()
            .and_then(|m| m.get(session_id))
            .is_some_and(|c| Arc::ptr_eq(c, &process));
        if is_self {
            if let Some(m) = map.as_mut() {
                m.remove(session_id);
            }
            (true, false)
        } else {
            let superseded = map.as_ref().is_some_and(|m| m.contains_key(session_id));
            (false, superseded)
        }
    };

    // 进程退出的独立信号（与 stream-done 的"轮次结束"语义分离）：
    // 前端靠它维护 processAlive——turn 结束后长活进程仍在跑后台任务（Workflow/子 agent）
    // 时，异步账本据此把无终态条目判为 running 而非 unknown。被顶替时不发，防止
    // 旧进程 EOF 迟到把新进程的活态翻掉。
    if !superseded {
        dispatch_stream_signal(
            app,
            "session-process-exited",
            json!({ "session_id": session_id }),
        );
    }

    if was_unexpected {
        if let Some(status) = exit_status {
            if !status.success() {
                let stderr_text = stderr.map(|stderr| {
                    let mut reader = BufReader::new(stderr);
                    let mut buf = String::new();
                    let _ = std::io::Read::read_to_string(&mut reader, &mut buf);
                    buf
                });
                let err_msg = stderr_text
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| t.trim().to_string())
                    .unwrap_or_else(|| format!("进程异常退出 (code: {})", status));
                emit_error(app, session_id, err_msg);
            } else {
                // 正常退出但未预期（CLI 自行决定退出），通知前端收尾
                dispatch_stream_signal(app, "stream-done", json!({ "session_id": session_id }));
            }
        } else {
            emit_error(app, session_id, "进程异常退出".to_string());
        }
    }
}

/// 解析单行 stream-json 为 StreamEvent
///
/// `current_message_id` 由 `stream_event/message_start` 写入，后续 content_block_* 事件读取——
/// CLI 的 partial messages envelope 是 Anthropic Messages API 原生 SSE 的透传，
/// 单个 block 事件本身不带 message_id，需要靠跨行状态拼接。
fn decode_stream_event(
    value: &Value,
    session_id: &str,
    current_message_id: &mut Option<(String, Option<String>)>,
) -> Option<StreamEvent> {
    let event_type = value.get("type")?.as_str()?;
    let sid = session_id.to_string();

    // 子 agent 的转发事件：顶层 parent_tool_use_id 非 null（实测 v2.1.x 子 agent 的
    // assistant 快照带此标记，主对话恒 null）。不进主对话流——子 agent 内容由
    // 异步面板从 subagents 转录呈现，混入主流会把子 agent 输出渲染进主会话区
    if value
        .get("parent_tool_use_id")
        .is_some_and(|v| !v.is_null())
    {
        return None;
    }

    match event_type {
        "stream_event" => {
            // CLI envelope: { type: "stream_event", event: <Anthropic SSE event> }
            let inner = value.get("event")?;
            let inner_type = inner.get("type")?.as_str()?;
            match inner_type {
                "message_start" => {
                    // 仅更新跨行状态,不 emit:让 content_block_start 自带建 turn 能力。
                    // 顺带提取 message.model——本轮实际运行模型的最早真值来源
                    let msg = inner.get("message")?;
                    let id = msg.get("id")?.as_str()?.to_string();
                    let model = msg.get("model").and_then(|v| v.as_str()).map(String::from);
                    *current_message_id = Some((id, model));
                    None
                }
                "content_block_start" => {
                    let (mid, model) = current_message_id.as_ref()?.clone();
                    let index = inner.get("index")?.as_u64()? as usize;
                    let cb_value = inner.get("content_block")?.clone();
                    let content_block: ContentBlock =
                        serde_json::from_value(cb_value).ok()?;
                    Some(StreamEvent::BlockStart {
                        session_id: sid,
                        message_id: mid,
                        index,
                        content_block,
                        model,
                    })
                }
                "content_block_delta" => {
                    let mid = current_message_id.as_ref()?.0.clone();
                    let index = inner.get("index")?.as_u64()? as usize;
                    let delta = inner.get("delta")?.clone();
                    Some(StreamEvent::BlockDelta {
                        session_id: sid,
                        message_id: mid,
                        index,
                        delta,
                    })
                }
                "content_block_stop" => {
                    let mid = current_message_id.as_ref()?.0.clone();
                    let index = inner.get("index")?.as_u64()? as usize;
                    Some(StreamEvent::BlockStop {
                        session_id: sid,
                        message_id: mid,
                        index,
                    })
                }
                // message_delta / message_stop 忽略,终结由 result 事件兜底
                _ => None,
            }
        }
        "assistant" => {
            let msg = value.get("message")?;
            let message_id = msg
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| value.get("uuid").and_then(|v| v.as_str()))
                .unwrap_or("unknown")
                .to_string();
            let content: Vec<ContentBlock> = msg
                .get("content")
                .and_then(|c| serde_json::from_value(c.clone()).ok())
                .unwrap_or_default();
            let model = msg.get("model").and_then(|v| v.as_str()).map(String::from);
            let usage = msg.get("usage").cloned();
            let stop_reason = msg
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(StreamEvent::AssistantMessage {
                session_id: sid,
                message_id,
                content,
                model,
                usage,
                stop_reason,
            })
        }
        // "progress"（老版 CLI 的子任务进度转发容器）不再混入主流：
        // 子 agent 内容归异步面板，主对话只渲染自己的消息
        "result" => {
            let result_text = value
                .get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_error = value
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let is_interrupt = result_text.is_empty()
                || result_text.contains("interrupted by user");
            if is_error && !is_interrupt {
                Some(StreamEvent::Error {
                    session_id: sid,
                    message: result_text,
                })
            } else {
                let cost = value
                    .get("total_cost_usd")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let usage = value.get("modelUsage");
                Some(StreamEvent::Result {
                    session_id: sid,
                    text: result_text,
                    cost_usd: cost,
                    context_window: usage.and_then(|u| u.get("contextWindow")).and_then(|v| v.as_u64()),
                    input_tokens: usage.and_then(|u| u.get("inputTokens")).and_then(|v| v.as_u64()),
                    output_tokens: usage.and_then(|u| u.get("outputTokens")).and_then(|v| v.as_u64()),
                })
            }
        }
        _ => None,
    }
}
