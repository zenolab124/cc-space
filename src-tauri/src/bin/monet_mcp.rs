//! Monet MCP server
//!
//! stdio JSON-RPC 2.0 server，双职能：
//! 1. 权限审批（--permission-prompt-tool）：通过 TCP loopback 与主进程通信（跨平台统一）
//! 2. 通用工具：直接读写数据目录，暴露 routine 管理等能力
//!
//! tool 列表根据环境动态组装：
//! - MONET_PERMISSION_ADDR 存在 → 包含 approve_tool_use
//! - 始终包含 routine / search / runner / theme preview 工具

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};

const SERVER_NAME: &str = "monet";
const SERVER_VERSION: &str = "0.3.0";
const PROTOCOL_VERSION: &str = "2024-11-05";

// ---------------------------------------------------------------------------
// 共享模块（与主 App 单一事实源）
// ---------------------------------------------------------------------------

#[path = "../config.rs"]
#[allow(dead_code)]
mod config;
/// 搜索引擎：独立进程直读落盘缓存 + mtime 对账自补增量，不依赖主 App 存活
#[path = "../search.rs"]
mod search;
#[path = "../theme_domain.rs"]
mod theme_domain;

use config::data_dir;

// ---------------------------------------------------------------------------
// Routine data structures（与主 App / runner 共享单一事实源）
// ---------------------------------------------------------------------------

#[path = "../routine_types.rs"]
#[allow(dead_code)]
mod routine_types;
use routine_types::{RoutineDefinition, RoutineEngine, RoutineSource};

// Cron 表达式单一入口：存储用 vixie 惯例（1=Mon），cron crate 用 Quartz
// （1=Sun），本模块负责映射后交给 cron crate。主 App / runner / MCP 共享
#[path = "../cron_expr.rs"]
#[allow(dead_code)]
mod cron_expr;

/// 跑单候选清单 + pid 文件 + 磁盘日志回读（与主 App 共享单一事实源）
#[path = "../runner_store.rs"]
#[allow(dead_code)]
mod runner_store;

/// initialize 握手记录的 MCP 客户端标识（如 claude-code 2.1.187）
static CLIENT_INFO: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

// ---------------------------------------------------------------------------
// Routine file operations
// ---------------------------------------------------------------------------

fn routines_path() -> PathBuf {
    data_dir().join("routines.json")
}

fn load_routines() -> Vec<RoutineDefinition> {
    let path = routines_path();
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_routines(data: &[RoutineDefinition]) {
    let path = routines_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 原子写：主 App / runner 可能并发读同一文件
    if let Ok(json_str) = serde_json::to_string_pretty(data) {
        let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
        if std::fs::write(&tmp, json_str).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn validate_cron(cron_expr: &str) -> Result<(), String> {
    use cron::Schedule;
    use std::str::FromStr;
    let full = crate::cron_expr::to_quartz_full(cron_expr);
    Schedule::from_str(&full).map_err(|e| format!("无效的 cron 表达式: {}", e))?;
    Ok(())
}

fn compute_next_run(cron_expr: &str) -> Option<String> {
    use cron::Schedule;
    use std::str::FromStr;
    let full = crate::cron_expr::to_quartz_full(cron_expr);
    let schedule = Schedule::from_str(&full).ok()?;
    // 必须用 Local：cron 表达式的 hour/minute 是本地时，launchd 也按本地时触发；
    // 用 UTC 会把 "9 点" 解释成 UTC 9:00 落盘，前端渲染时按本地时区偏移显示错时刻
    schedule.upcoming(chrono::Local).next().map(|t| t.to_rfc3339())
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout_lock = stdout.lock();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                let resp = error_response(Value::Null, -32700, "Parse error");
                write_response(&mut stdout_lock, &resp);
                continue;
            }
        };

        let id = req.get("id").cloned();
        let is_notification = id.is_none() || id.as_ref().is_some_and(Value::is_null);
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");

        let response = match method {
            "initialize" => handle_initialize(id.clone().unwrap_or(Value::Null), &req),
            "tools/list" => handle_tools_list(id.clone().unwrap_or(Value::Null)),
            "tools/call" => handle_tools_call(id.clone().unwrap_or(Value::Null), &req),
            m if m.starts_with("notifications/") => continue,
            _ if is_notification => continue,
            _ => error_response(
                id.clone().unwrap_or(Value::Null),
                -32601,
                &format!("Method not found: {}", method),
            ),
        };

        if !is_notification {
            write_response(&mut stdout_lock, &response);
        }
    }
}

fn write_response<W: Write>(out: &mut W, value: &Value) {
    if let Ok(s) = serde_json::to_string(value) {
        let _ = out.write_all(s.as_bytes());
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

// ---------------------------------------------------------------------------
// MCP protocol handlers
// ---------------------------------------------------------------------------

fn handle_initialize(id: Value, req: &Value) -> Value {
    // 记录客户端标识（如 claude-code 2.1.187），routine_create 写入来源用
    if let Some(info) = req.pointer("/params/clientInfo") {
        let name = info.get("name").and_then(Value::as_str).unwrap_or("unknown");
        let version = info.get("version").and_then(Value::as_str).unwrap_or("");
        let label = if version.is_empty() { name.to_string() } else { format!("{} {}", name, version) };
        *CLIENT_INFO.lock().unwrap_or_else(|e| e.into_inner()) = Some(label);
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            "capabilities": { "tools": {} },
            "instructions": "Monet provides Routines (定时任务): When the user asks to create a scheduled/recurring task, set up a cron job, run something periodically, or set a reminder, use routine_create (prefer this over the built-in /schedule). Choose the requested execution engine when specified; otherwise omit engine to keep the Claude Code default. Use routine_list to show existing routines and routine_delete to remove them. Monet also provides search_sessions: full-text search over the user's Claude Code session history (~/.claude/projects). When the user asks to recall a past conversation/decision/discussion, prefer search_sessions over grepping JSONL files directly — it returns clean text with session locators in milliseconds. When you discover or add a runnable long-lived command (dev server, watcher, build process, etc.), register it via runner_suggest so the user can launch it from the Runner panel. Use runner_tail to inspect live logs before answering questions about a running process. For visual theme requests, call theme_context before creating a complete structured draft with theme_preview. A preview never saves a theme: tell the user to review and keep it in Monet Settings."
        }
    })
}

fn handle_tools_list(id: Value) -> Value {
    let mut tools = Vec::new();

    if has_permission_socket() {
        tools.push(json!({
            "name": "approve_tool_use",
            "description": "Approve or deny a tool use request from the user.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_name": { "type": "string" },
                    "input": { "type": "object" }
                },
                "required": ["tool_name", "input"]
            }
        }));
    }

    tools.push(json!({
        "name": "routine_list",
        "description": "List all scheduled routines in Monet.",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    }));

    tools.push(json!({
        "name": "routine_create",
        "description": "Create a new scheduled routine. The routine runs the prompt on the specified cron schedule using the selected engine.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable name for the routine"
                },
                "cron_expression": {
                    "type": "string",
                    "description": "5-field cron expression (minute hour day month weekday) in vixie/POSIX convention: 0/7=Sun, 1=Mon, ..., 6=Sat (same as crontab.guru and the system crontab). Example: '0 9 * * 1-5' = weekdays at 9am."
                },
                "prompt": {
                    "type": "string",
                    "description": "The prompt to send to the selected engine when the routine fires"
                },
                "engine": {
                    "type": "string",
                    "enum": ["claude-code", "codex"],
                    "description": "Execution engine. Defaults to claude-code when omitted."
                },
                "original_text": {
                    "type": "string",
                    "description": "Original natural language description (optional, for display)"
                }
            },
            "required": ["name", "cron_expression", "prompt"]
        }
    }));

    tools.push(json!({
        "name": "routine_delete",
        "description": "Delete a scheduled routine by ID.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The routine ID to delete"
                }
            },
            "required": ["id"]
        }
    }));

    tools.push(json!({
        "name": "search_sessions",
        "description": "Full-text search over the user's Claude Code session history (all projects). Space-separated terms are ANDed at session level, case-insensitive substring match (works for CJK). Returns matching sessions with title, project, KWIC snippets, message uuids and the JSONL path — read the JSONL directly for full context around a hit.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search terms, space-separated (AND semantics across the session)"
                },
                "project_id": {
                    "type": "string",
                    "description": "Restrict to one project (encoded dir name under ~/.claude/projects)"
                },
                "days": {
                    "type": "integer",
                    "description": "Only sessions modified within the last N days"
                },
                "title_only": {
                    "type": "boolean",
                    "description": "Match only titles/tags/summary, skip message bodies"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max sessions to return (default 20, max 50)"
                }
            },
            "required": ["query"]
        }
    }));

    // Runner 工具
    tools.push(json!({
        "name": "runner_list",
        "description": "List Monet runners (auxiliary processes attached to a session). Use `runner_tail` to fetch recent log output.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Filter by session ID (optional)"
                }
            }
        }
    }));

    tools.push(json!({
        "name": "runner_tail",
        "description": "Tail the last N lines of a Monet runner's log. Defaults to 200 lines with ANSI stripped.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "runner_id": {
                    "type": "string",
                    "description": "Runner ID (from runner_list)"
                },
                "lines": {
                    "type": "integer",
                    "description": "Number of lines to return (default 200, max 2000)"
                },
                "grep": {
                    "type": "string",
                    "description": "Regex pattern to filter lines before taking the tail"
                },
                "strip_ansi": {
                    "type": "boolean",
                    "description": "Strip ANSI escape sequences (default true)"
                }
            },
            "required": ["runner_id"]
        }
    }));

    tools.push(json!({
        "name": "runner_suggest",
        "description": "Register a runnable command suggestion for the current project (e.g. dev server, watch build). It appears in Monet's Runner panel for the user to launch with one click. Use when you discover or create a long-running command worth keeping at hand.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "cmd": {
                    "type": "string",
                    "description": "The command to run (e.g. 'pnpm dev', 'cargo watch -x check')"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory for the command. Defaults to the project root (current working directory). Pass an explicit path for monorepo sub-packages."
                },
                "alias": {
                    "type": "string",
                    "description": "Short display name (e.g. 'dev', 'storybook')"
                },
                "note": {
                    "type": "string",
                    "description": "Brief description of what the command does"
                }
            },
            "required": ["cmd"]
        }
    }));

    tools.push(json!({
        "name": "theme_context",
        "description": "Read Monet's safe theme schema, constraints, saved local themes, and pending previews. Call this before designing or adjusting a theme.",
        "inputSchema": { "type": "object", "properties": {} }
    }));

    tools.push(json!({
        "name": "theme_preview",
        "description": "Create or replace a structured theme preview for the user to review in Monet Settings. This tool cannot save or activate a theme.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "theme": {
                    "type": "object",
                    "description": "Complete theme object matching theme_context exactly; unknown fields are rejected."
                },
                "preview_id": {
                    "type": "string",
                    "description": "Existing preview ID to replace when continuing an adjustment."
                },
                "base_theme_id": {
                    "type": "string",
                    "description": "Saved local theme ID being adjusted, if any."
                }
            },
            "required": ["theme"]
        }
    }));

    tools.push(json!({
        "name": "theme_preview_get",
        "description": "Read one pending theme preview by ID before continuing an adjustment.",
        "inputSchema": {
            "type": "object",
            "properties": { "preview_id": { "type": "string" } },
            "required": ["preview_id"]
        }
    }));

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "tools": tools }
    })
}

fn handle_tools_call(id: Value, req: &Value) -> Value {
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    let result = match name {
        "approve_tool_use" => handle_approve_tool_use(&arguments),
        "routine_list" => handle_routine_list(),
        "routine_create" => handle_routine_create(&arguments),
        "routine_delete" => handle_routine_delete(&arguments),
        "search_sessions" => handle_search_sessions(&arguments),
        "runner_list" => handle_runner_list(&arguments),
        "runner_tail" => handle_runner_tail(&arguments),
        "runner_suggest" => handle_runner_suggest(&arguments),
        "theme_context" => handle_theme_context(),
        "theme_preview" => handle_theme_preview(&arguments),
        "theme_preview_get" => handle_theme_preview_get(&arguments),
        _ => Err(format!("Unknown tool: {}", name)),
    };

    match result {
        Ok(text) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": text }]
            }
        }),
        Err(msg) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": msg }],
                "isError": true
            }
        }),
    }
}

// ---------------------------------------------------------------------------
// Tools: safe theme previews (persistence remains UI-only)
// ---------------------------------------------------------------------------

fn handle_theme_context() -> Result<String, String> {
    let library = theme_domain::list_library();
    serde_json::to_string_pretty(&json!({
        "schema": theme_domain::schema_context(),
        "savedLocalThemes": library.themes,
        "pendingPreviews": library.previews,
        "invalidEntries": library.invalid_entries,
        "nextStep": "Create a complete draft with theme_preview, then ask the user to review it in Monet Settings. MCP cannot save themes."
    })).map_err(|error| error.to_string())
}

fn handle_theme_preview(arguments: &Value) -> Result<String, String> {
    let raw = arguments.get("theme").cloned().ok_or("theme is required")?;
    let theme: theme_domain::ThemeDefinition = serde_json::from_value(raw)
        .map_err(|error| format!("theme schema rejected: {error}"))?;
    let preview_id = arguments.get("preview_id").and_then(Value::as_str);
    let base_theme_id = arguments.get("base_theme_id").and_then(Value::as_str).map(str::to_string);
    let preview = theme_domain::put_preview(theme, preview_id, base_theme_id)?;
    serde_json::to_string_pretty(&json!({
        "preview": preview,
        "saved": false,
        "nextStep": "Ask the user to open Monet Settings > Appearance, review the isolated preview, and choose Keep Theme."
    })).map_err(|error| error.to_string())
}

fn handle_theme_preview_get(arguments: &Value) -> Result<String, String> {
    let id = arguments.get("preview_id").and_then(Value::as_str).ok_or("preview_id is required")?;
    serde_json::to_string_pretty(&theme_domain::load_preview(id)?).map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// Tool: search_sessions
// ---------------------------------------------------------------------------

fn handle_search_sessions(arguments: &Value) -> Result<String, String> {
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| "query is required".to_string())?;

    let filter = search::SearchFilter {
        project_id: arguments
            .get("project_id")
            .and_then(Value::as_str)
            .map(String::from),
        days: arguments.get("days").and_then(Value::as_u64).map(|d| d as u32),
        title_only: arguments
            .get("title_only")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .min(50) as usize;

    // 每次调用都对账：stdio server 长驻期间会话文件在持续变化；
    // 冷建 <1s、热对账 <10ms，可承受且保证结果新鲜
    search::warm();
    let result = search::query(query, &filter);

    let projects_root = config::projects_dir();
    let sessions: Vec<Value> = result
        .hits
        .iter()
        .take(limit)
        .map(|h| {
            let iso = chrono::DateTime::from_timestamp(h.last_modified as i64, 0)
                .map(|t| t.to_rfc3339())
                .unwrap_or_default();
            json!({
                "sessionId": h.session_id,
                "projectId": h.project_id,
                "title": h.title,
                "lastModified": iso,
                "matchedIn": h.matched_in,
                "totalMatches": h.total_matches,
                "jsonlPath": projects_root
                    .join(&h.project_id)
                    .join(format!("{}.jsonl", h.session_id))
                    .to_string_lossy(),
                "snippets": h.snippets.iter().map(|s| json!({
                    "uuid": s.uuid,
                    "role": if s.role == 0 { "user" } else { "assistant" },
                    "timestamp": s.timestamp,
                    "text": s.text,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    serde_json::to_string(&json!({
        "totalHits": result.total_hits,
        "returned": sessions.len(),
        "elapsedMs": result.elapsed_ms,
        "sessions": sessions,
    }))
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tool: approve_tool_use
// ---------------------------------------------------------------------------

fn has_permission_socket() -> bool {
    std::env::var("MONET_PERMISSION_ADDR").is_ok()
}

fn handle_approve_tool_use(arguments: &Value) -> Result<String, String> {
    let tool_name = arguments
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let tool_input = arguments
        .get("input")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let decision = ask_main_process(&tool_name, &tool_input).unwrap_or_else(|err| {
        json!({
            "behavior": "deny",
            "message": format!("权限服务通信失败：{}", err)
        })
    });

    serde_json::to_string(&decision).map_err(|e| e.to_string())
}

fn ask_main_process(tool_name: &str, tool_input: &Value) -> Result<Value, String> {
    let addr = std::env::var("MONET_PERMISSION_ADDR")
        .map_err(|_| "环境变量 MONET_PERMISSION_ADDR 未设置".to_string())?;
    let token = std::env::var("MONET_PERMISSION_TOKEN").unwrap_or_default();

    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("连接 {} 失败：{}", addr, e))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;

    let req_line = serde_json::to_string(&json!({
        "toolName": tool_name,
        "input": tool_input,
        "token": token,
    }))
    .map_err(|e| e.to_string())?;
    stream
        .write_all(req_line.as_bytes())
        .map_err(|e| format!("发送请求失败：{}", e))?;
    stream
        .write_all(b"\n")
        .map_err(|e| format!("发送换行失败：{}", e))?;
    stream.flush().map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = reader
            .read(&mut tmp)
            .map_err(|e| format!("读响应失败：{}", e))?;
        if n == 0 {
            break;
        }
        buf.push_str(&String::from_utf8_lossy(&tmp[..n]));
        if buf.contains('\n') {
            break;
        }
    }

    let line = buf
        .lines()
        .next()
        .ok_or_else(|| "主进程未返回任何数据".to_string())?;
    let decision: Value =
        serde_json::from_str(line).map_err(|e| format!("解析主进程响应失败：{}", e))?;

    let behavior = decision
        .get("behavior")
        .and_then(Value::as_str)
        .unwrap_or("");
    match behavior {
        "allow" => {
            let mut out = decision.clone();
            if out.get("updatedInput").is_none() {
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("updatedInput".to_string(), tool_input.clone());
                }
            }
            Ok(out)
        }
        "deny" => Ok(decision),
        _ => Err(format!("主进程返回未知 behavior：{:?}", behavior)),
    }
}

// ---------------------------------------------------------------------------
// Tool: routine_list
// ---------------------------------------------------------------------------

fn handle_routine_list() -> Result<String, String> {
    let routines = load_routines();
    serde_json::to_string_pretty(&routines).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tool: routine_create
// ---------------------------------------------------------------------------

fn handle_routine_create(arguments: &Value) -> Result<String, String> {
    let name = arguments
        .get("name")
        .and_then(Value::as_str)
        .ok_or("缺少参数: name")?
        .to_string();
    let cron_expression = arguments
        .get("cron_expression")
        .and_then(Value::as_str)
        .ok_or("缺少参数: cron_expression")?
        .to_string();
    let prompt = arguments
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or("缺少参数: prompt")?
        .to_string();
    let original_text = arguments
        .get("original_text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let engine = match arguments
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("claude-code")
    {
        "claude-code" => RoutineEngine::claude_code(),
        "codex" => RoutineEngine::codex(),
        value => return Err(format!("不支持的定时任务引擎: {}", value)),
    };

    validate_cron(&cron_expression)?;

    // 来源：MCP server 继承 claude CLI 的 cwd，即发起会话的项目路径
    let project = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let client = CLIENT_INFO.lock().unwrap_or_else(|e| e.into_inner()).clone();

    let routine = RoutineDefinition {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        cron_expression: cron_expression.clone(),
        original_text,
        prompt,
        engine,
        enabled: true,
        created_at: Utc::now().to_rfc3339(),
        last_run: None,
        next_run: compute_next_run(&cron_expression),
        source: Some(RoutineSource::mcp(project, client)),
    };

    let mut routines = load_routines();
    routines.push(routine.clone());
    save_routines(&routines);

    serde_json::to_string_pretty(&routine).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tool: routine_delete
// ---------------------------------------------------------------------------

fn handle_routine_delete(arguments: &Value) -> Result<String, String> {
    let id = arguments
        .get("id")
        .and_then(Value::as_str)
        .ok_or("缺少参数: id")?;

    let mut routines = load_routines();
    let before = routines.len();
    routines.retain(|r| r.id != id);

    if routines.len() == before {
        return Err(format!("未找到 routine: {}", id));
    }

    save_routines(&routines);
    Ok(json!({ "deleted": id }).to_string())
}

// ---------------------------------------------------------------------------
// Tool: runner_list
// ---------------------------------------------------------------------------

fn handle_runner_list(arguments: &Value) -> Result<String, String> {
    let session_filter = arguments.get("session_id").and_then(Value::as_str);

    // meta.json 为主源：覆盖活跑单 + 已结束跑单
    let metas = runner_store::scan_all_metas();
    // pid 文件辅助探活：有 pid 且进程存活 → 覆盖 meta 状态为 running
    let pid_map: std::collections::HashMap<String, runner_store::PidInfo> =
        runner_store::scan_all_pids()
            .into_iter()
            .map(|(_, rid, info)| (rid, info))
            .collect();

    let mut runners = Vec::new();
    for (_session_id, runner_id, meta) in &metas {
        if let Some(filter) = session_filter {
            if meta.session_id != filter {
                continue;
            }
        }

        let (status, pid) = if let Some(pid_info) = pid_map.get(runner_id) {
            if is_process_alive(pid_info.pid) {
                ("running".to_string(), Some(pid_info.pid))
            } else {
                (meta.status.clone(), None)
            }
        } else {
            (meta.status.clone(), None)
        };

        runners.push(json!({
            "id": runner_id,
            "sessionId": meta.session_id,
            "alias": meta.alias,
            "cmd": meta.cmd,
            "cwd": meta.cwd,
            "status": status,
            "startedAt": meta.started_at,
            "exitedAt": meta.exited_at,
            "exitCode": meta.exit_code,
            "pid": pid,
            "logPath": meta.log_path,
        }));
    }
    serde_json::to_string(&json!({ "runners": runners })).map_err(|e| e.to_string())
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

// ---------------------------------------------------------------------------
// Tool: runner_tail
// ---------------------------------------------------------------------------

fn handle_runner_tail(arguments: &Value) -> Result<String, String> {
    let runner_id = arguments
        .get("runner_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "runner_id is required".to_string())?;

    let lines = arguments
        .get("lines")
        .and_then(Value::as_u64)
        .unwrap_or(200)
        .min(2000) as usize;

    let grep = arguments.get("grep").and_then(Value::as_str);

    let strip_ansi = arguments
        .get("strip_ansi")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let log_path = runner_store::find_log_path(runner_id)
        .ok_or_else(|| "runner not found".to_string())?;

    let result = runner_store::read_tail_from_disk(&log_path, lines, grep, strip_ansi)?;
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tool: runner_suggest
// ---------------------------------------------------------------------------

fn handle_runner_suggest(arguments: &Value) -> Result<String, String> {
    let cmd = arguments
        .get("cmd")
        .and_then(Value::as_str)
        .ok_or_else(|| "cmd is required".to_string())?;

    let cwd_arg = arguments.get("cwd").and_then(Value::as_str);
    let alias = arguments.get("alias").and_then(Value::as_str);
    let note = arguments.get("note").and_then(Value::as_str);

    // 桶 key 固定用项目根（MCP 进程 cwd = 会话项目路径），与 cwd 入参解耦
    let project_root = std::env::current_dir()
        .and_then(|p| p.canonicalize())
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| "could not determine current directory".to_string())?;

    // cwd 入参仅作条目运行目录：相对路径按项目根解析，与项目根相同存 None
    let resolved_cwd = cwd_arg.and_then(|c| {
        let resolved = if std::path::Path::new(c).is_relative() {
            std::path::Path::new(&project_root).join(c)
        } else {
            PathBuf::from(c)
        };
        let canonical = std::fs::canonicalize(&resolved).unwrap_or(resolved);
        if canonical.to_string_lossy() == project_root {
            None
        } else {
            Some(canonical.to_string_lossy().into_owned())
        }
    });

    let (project, total) =
        runner_store::suggest_command(&project_root, cmd, resolved_cwd.as_deref(), alias, note)?;

    serde_json::to_string(&json!({
        "ok": true,
        "project": project,
        "total": total,
    }))
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}
