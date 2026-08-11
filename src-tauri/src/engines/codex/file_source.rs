use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::super::core::{EngineError, EngineErrorKind, EngineResult};
use super::{CodexThread, CodexTurn};

const PREVIEW_MAX_CHARS: usize = 4_000;
const SUMMARY_SCAN_LIMIT: usize = 512 * 1024;
const SUMMARY_CANONICAL_LOOKAHEAD_ROWS: usize = 32;

pub(super) fn list_threads() -> EngineResult<Vec<CodexThread>> {
    let paths = session_paths();
    let mut threads = Vec::with_capacity(paths.len());
    for path in paths {
        if let Some(thread) = read_thread_summary(&path) {
            threads.push(thread);
        }
    }
    threads.sort_by_key(|thread| std::cmp::Reverse(thread.updated_at));
    Ok(threads)
}

/// 内置 Agent/Routine 的专属 cwd 不进入普通档案、搜索或用量统计。
pub(super) fn is_agent_cwd(cwd: &str) -> bool {
    if cwd.trim().is_empty() {
        return false;
    }
    let candidate = PathBuf::from(cwd);
    let agent = crate::config::agent_cwd();
    let candidate = candidate.canonicalize().unwrap_or(candidate);
    let agent = agent.canonicalize().unwrap_or(agent);
    if cfg!(windows) {
        candidate
            .to_string_lossy()
            .eq_ignore_ascii_case(agent.to_string_lossy().as_ref())
    } else {
        candidate == agent
    }
}

pub(super) fn read_thread(id: &str) -> EngineResult<CodexThread> {
    let Some(path) = thread_path(id) else {
        return Err(EngineError::new(
            EngineErrorKind::NotFound,
            "Codex local session was not found",
        ));
    };
    read_thread_file(&path)
}

pub(super) fn thread_path(id: &str) -> Option<PathBuf> {
    for path in session_paths() {
        let Some(summary) = read_thread_summary(&path) else {
            continue;
        };
        if summary.id == id || path.file_stem().and_then(|value| value.to_str()) == Some(id) {
            return Some(path);
        }
    }
    None
}

/// 从 rollout 的 MCP 完成事件中按调用 ID 读取单个结果块。
/// 这条路径不依赖 App Server，供历史图片按需恢复使用。
pub(super) fn read_mcp_result_content_item(
    thread_id: &str,
    call_id: &str,
    content_index: usize,
) -> Option<Value> {
    let path = thread_path(thread_id)?;
    read_mcp_result_content_item_from_path(&path, call_id, content_index)
}

fn read_mcp_result_content_item_from_path(
    path: &Path,
    call_id: &str,
    content_index: usize,
) -> Option<Value> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let payload = value.get("payload").unwrap_or(&value);
        if payload.get("type").and_then(Value::as_str) != Some("mcp_tool_call_end")
            || payload.get("call_id").and_then(Value::as_str) != Some(call_id)
        {
            continue;
        }
        let result = payload.get("result")?;
        let result = result.get("Ok").unwrap_or(result);
        return result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.get(content_index))
            .cloned();
    }
    None
}

pub(super) fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path == Path::new("~") || path.starts_with("~/") {
                dirs::home_dir()
                    .map(|home| home.join(path.strip_prefix("~/").unwrap_or(Path::new(""))))
                    .unwrap_or(path)
            } else {
                path
            }
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

pub(super) fn session_paths() -> Vec<PathBuf> {
    let Some(home) = codex_home() else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for directory in [home.join("sessions"), home.join("archived_sessions")] {
        collect_jsonl_paths(&directory, &mut paths);
    }
    paths.sort();
    paths.dedup();
    paths
}

fn collect_jsonl_paths(directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_jsonl_paths(&path, paths);
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            paths.push(path);
        }
    }
}

pub(super) fn read_thread_summary(path: &Path) -> Option<CodexThread> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut meta = None;
    let mut preview = None;
    let mut fallback_preview = None;
    let mut fallback_row = None;
    let mut scanned_bytes = 0usize;
    let mut scanned_rows = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        scanned_bytes = scanned_bytes.saturating_add(line.len());
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            if scanned_bytes >= SUMMARY_SCAN_LIMIT {
                break;
            }
            continue;
        };
        scanned_rows += 1;
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            meta = value.get("payload").cloned();
        }
        let canonical_user_message = value.get("type").and_then(Value::as_str) == Some("event_msg")
            && value
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(Value::as_str)
                == Some("user_message");
        if canonical_user_message {
            preview = preview_from_value(&value);
        } else if fallback_preview.is_none() {
            fallback_preview = preview_from_value(&value);
            if fallback_preview.is_some() {
                fallback_row = Some(scanned_rows);
            }
        }
        let canonical_lookahead_exhausted = fallback_row.is_some_and(|row| {
            scanned_rows.saturating_sub(row) >= SUMMARY_CANONICAL_LOOKAHEAD_ROWS
        });
        if (meta.is_some() && (preview.is_some() || canonical_lookahead_exhausted))
            || scanned_bytes >= SUMMARY_SCAN_LIMIT
        {
            break;
        }
    }

    let metadata = fs::metadata(path).ok()?;
    let modified_at = modified_epoch(&metadata);
    let payload = meta.unwrap_or_default();
    let id = string_field(&payload, &["id", "session_id"]).or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .map(String::from)
    })?;
    let created_at = payload
        .get("timestamp")
        .and_then(timestamp_value)
        .unwrap_or(modified_at);
    Some(CodexThread {
        id,
        name: None,
        preview: preview.or(fallback_preview).unwrap_or_default(),
        cwd: string_field(&payload, &["cwd"]).unwrap_or_default(),
        model_provider: string_field(&payload, &["model_provider"]).unwrap_or_default(),
        path: Some(path.to_string_lossy().into_owned()),
        created_at,
        updated_at: modified_at.max(created_at),
        turns: Vec::new(),
    })
}

fn read_thread_file(path: &Path) -> EngineResult<CodexThread> {
    let file = File::open(path)
        .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut builder = ThreadBuilder::new(path);
    let mut line_number = 0usize;

    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))?
            == 0
        {
            break;
        }
        line_number += 1;
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        builder.consume(&value, line_number);
    }

    builder.finish()
}

struct ThreadBuilder {
    path: PathBuf,
    id: Option<String>,
    cwd: String,
    model_provider: String,
    created_at: Option<i64>,
    updated_at: i64,
    preview: String,
    turns: Vec<CodexTurn>,
    turn_indexes: HashMap<String, usize>,
    current_turn_id: Option<String>,
}

impl ThreadBuilder {
    fn new(path: &Path) -> Self {
        let updated_at = fs::metadata(path)
            .ok()
            .map(|metadata| modified_epoch(&metadata))
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            id: None,
            cwd: String::new(),
            model_provider: String::new(),
            created_at: None,
            updated_at,
            preview: String::new(),
            turns: Vec::new(),
            turn_indexes: HashMap::new(),
            current_turn_id: None,
        }
    }

    fn consume(&mut self, value: &Value, line_number: usize) {
        let top_level_type = value.get("type").and_then(Value::as_str);
        match top_level_type {
            Some("session_meta") => {
                let payload = value.get("payload").unwrap_or(value);
                self.id = string_field(payload, &["id", "session_id"]);
                self.cwd = string_field(payload, &["cwd"]).unwrap_or_default();
                self.model_provider =
                    string_field(payload, &["model_provider"]).unwrap_or_default();
                self.created_at = payload.get("timestamp").and_then(timestamp_value);
            }
            Some("turn_context") => {
                let payload = value.get("payload").unwrap_or(value);
                if let Some(turn_id) = string_field(payload, &["turn_id", "turnId"]) {
                    self.current_turn_id = Some(turn_id.clone());
                    self.ensure_turn(&turn_id, None);
                }
            }
            Some("event_msg") => self.consume_event(value, line_number),
            Some("response_item") => {
                let payload = value.get("payload").unwrap_or(value);
                if let Some(item) = normalize_response_item(payload, line_number) {
                    let turn_id = self.current_turn_id();
                    self.add_item(&turn_id, item);
                }
            }
            _ => {}
        }
    }

    fn consume_event(&mut self, value: &Value, line_number: usize) {
        let payload = value.get("payload").unwrap_or(value);
        let event_type = payload.get("type").and_then(Value::as_str);
        let turn_id = string_field(payload, &["turn_id", "turnId"]);
        if let Some(turn_id) = &turn_id {
            self.current_turn_id = Some(turn_id.clone());
        }
        match event_type {
            Some("task_started") => {
                let id = turn_id.unwrap_or_else(|| self.current_turn_id());
                let started_at = payload.get("started_at").and_then(timestamp_value);
                self.ensure_turn(&id, started_at);
            }
            Some("task_complete") => {
                let id = turn_id.unwrap_or_else(|| self.current_turn_id());
                self.ensure_turn(&id, None);
                if let Some(index) = self.turn_indexes.get(&id).copied() {
                    self.turns[index].completed_at = payload
                        .get("completed_at")
                        .and_then(timestamp_value)
                        .or(self.turns[index].completed_at);
                }
            }
            Some("user_message") => {
                let text = payload
                    .get("message")
                    .and_then(text_value)
                    .or_else(|| payload.get("text").and_then(text_value));
                if let Some(text) = text.filter(|text| !text.is_empty()) {
                    let id = format!("event-user-{line_number}");
                    let item = json!({
                        "id": id,
                        "type": "userMessage",
                        "content": [{"type": "text", "text": text}],
                        "canonicalUserMessage": true
                    });
                    let turn_id = self.current_turn_id();
                    self.add_item(&turn_id, item);
                }
            }
            Some("agent_message") => {
                let text = payload
                    .get("message")
                    .and_then(text_value)
                    .or_else(|| payload.get("text").and_then(text_value));
                if let Some(text) = text.filter(|text| !text.is_empty()) {
                    let id = format!("event-agent-{line_number}");
                    let item = json!({
                        "id": id,
                        "type": "agentMessage",
                        "text": text,
                        "phase": payload.get("phase").cloned().unwrap_or(Value::Null),
                        "eventAgentMessage": true
                    });
                    let turn_id = self.current_turn_id();
                    self.add_item_if_text_is_new(&turn_id, item, &text);
                }
            }
            _ => {}
        }
    }

    fn current_turn_id(&mut self) -> String {
        self.current_turn_id.clone().unwrap_or_else(|| {
            let id = format!("turn-{}", self.turns.len() + 1);
            self.current_turn_id = Some(id.clone());
            id
        })
    }

    fn ensure_turn(&mut self, id: &str, started_at: Option<i64>) {
        if let Some(index) = self.turn_indexes.get(id).copied() {
            if self.turns[index].started_at.is_none() {
                self.turns[index].started_at = started_at;
            }
            return;
        }
        self.turn_indexes.insert(id.to_string(), self.turns.len());
        self.turns.push(CodexTurn {
            id: id.to_string(),
            items: Vec::new(),
            started_at,
            completed_at: None,
        });
    }

    fn add_item(&mut self, turn_id: &str, item: Value) {
        self.ensure_turn(turn_id, None);
        let index = self.turn_indexes[turn_id];
        let canonical_user_message = item
            .get("canonicalUserMessage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if item.get("type").and_then(Value::as_str) == Some("userMessage")
            && (self.preview.is_empty() || canonical_user_message)
        {
            self.preview = preview_from_value(&item).unwrap_or_default();
        }
        self.turns[index].items.push(item);
    }

    fn add_item_if_text_is_new(&mut self, turn_id: &str, item: Value, text: &str) {
        self.ensure_turn(turn_id, None);
        let index = self.turn_indexes[turn_id];
        let already_present = self.turns[index].items.iter().any(|existing| {
            existing.get("type").and_then(Value::as_str) == item.get("type").and_then(Value::as_str)
                && preview_from_value(existing).as_deref() == Some(text)
        });
        if !already_present {
            self.add_item(turn_id, item);
        }
    }

    fn finish(mut self) -> EngineResult<CodexThread> {
        for turn in &mut self.turns {
            canonicalize_user_messages(&mut turn.items);
            deduplicate_event_agent_messages(&mut turn.items);
            clear_internal_message_markers(&mut turn.items);
        }
        if let Some(preview) = self
            .turns
            .iter()
            .flat_map(|turn| &turn.items)
            .find(|item| item.get("type").and_then(Value::as_str) == Some("userMessage"))
            .and_then(preview_from_value)
        {
            self.preview = preview;
        }
        let id = self
            .id
            .or_else(|| {
                self.path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .map(String::from)
            })
            .ok_or_else(|| {
                EngineError::new(EngineErrorKind::Protocol, "Codex session has no id")
            })?;
        let created_at = self.created_at.unwrap_or(self.updated_at);
        Ok(CodexThread {
            id,
            name: None,
            preview: self.preview,
            cwd: self.cwd,
            model_provider: self.model_provider,
            path: Some(self.path.to_string_lossy().into_owned()),
            created_at,
            updated_at: self.updated_at.max(created_at),
            turns: self.turns,
        })
    }
}

/// rollout 会同时记录宿主上下文和真正的人类输入为 user message；只有
/// `event_msg.user_message` 与 App Server 的用户消息口径一致。若该权威事件存在，
/// 丢弃同一 turn 里的原始 user 快照，并从匹配快照补回图片内容。
fn canonicalize_user_messages(items: &mut Vec<Value>) {
    let canonical_indexes: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            item.get("canonicalUserMessage")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                .then_some(index)
        })
        .collect();
    if canonical_indexes.is_empty() {
        return;
    }

    let candidates = items.clone();
    for index in canonical_indexes {
        let Some(canonical_text) = items[index]
            .get("content")
            .map(content_text)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        let Some(source_content) = candidates.iter().find_map(|candidate| {
            let is_raw_user = candidate.get("type").and_then(Value::as_str) == Some("userMessage")
                && !candidate
                    .get("canonicalUserMessage")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            if !is_raw_user {
                return None;
            }
            candidate
                .get("content")
                .and_then(Value::as_array)
                .filter(|content| {
                    content.iter().any(|part| {
                        part.get("type").and_then(Value::as_str) == Some("text")
                            && part.get("text").and_then(Value::as_str)
                                == Some(canonical_text.as_str())
                    })
                })
        }) else {
            continue;
        };
        let images: Vec<_> = source_content
            .iter()
            .filter(|part| {
                matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("image" | "input_image")
                )
            })
            .cloned()
            .collect();
        if let Some(content) = items[index]
            .get_mut("content")
            .and_then(Value::as_array_mut)
        {
            content.extend(images);
        }
    }

    items.retain(|item| {
        item.get("type").and_then(Value::as_str) != Some("userMessage")
            || item
                .get("canonicalUserMessage")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    });
}

/// agent_message 事件与紧随其后的 assistant response_item 是同一条回复的两种落盘形态。
/// 优先保留带稳定原生 ID 的 response_item；仅在快照缺失时保留事件兜底。
fn deduplicate_event_agent_messages(items: &mut Vec<Value>) {
    let response_messages: Vec<_> = items
        .iter()
        .filter(|item| {
            item.get("type").and_then(Value::as_str) == Some("agentMessage")
                && !item
                    .get("eventAgentMessage")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .filter_map(|item| {
            item.get("text").and_then(Value::as_str).map(|text| {
                (
                    text.to_string(),
                    item.get("phase").and_then(Value::as_str).map(String::from),
                )
            })
        })
        .collect();
    if response_messages.is_empty() {
        return;
    }
    items.retain(|item| {
        let event_message = item
            .get("eventAgentMessage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text = item.get("text").and_then(Value::as_str);
        let phase = item.get("phase").and_then(Value::as_str);
        !event_message
            || !text.is_some_and(|text| {
                response_messages
                    .iter()
                    .any(|(response_text, response_phase)| {
                        response_text == text && response_phase.as_deref() == phase
                    })
            })
    });
}

fn clear_internal_message_markers(items: &mut [Value]) {
    for item in items {
        if let Some(object) = item.as_object_mut() {
            object.remove("canonicalUserMessage");
            object.remove("eventAgentMessage");
        }
    }
}

fn normalize_response_item(payload: &Value, line_number: usize) -> Option<Value> {
    let kind = payload.get("type").and_then(Value::as_str)?;
    let id = string_field(payload, &["id", "call_id"])
        .unwrap_or_else(|| format!("response-item-{line_number}"));
    match kind {
        "message" => match payload.get("role").and_then(Value::as_str) {
            Some("user") => Some(json!({
                "id": id,
                "type": "userMessage",
                "content": normalized_content(payload.get("content")?)
            })),
            Some("assistant") => Some(json!({
                "id": id,
                "type": "agentMessage",
                "text": content_or_text(payload.get("content")?),
                "phase": payload.get("phase").cloned().unwrap_or(Value::Null)
            })),
            _ => None,
        },
        "agent_message" => Some(json!({
            "id": id,
            "type": "agentMessage",
            "text": payload
                .get("content")
                .map(content_or_text)
                .unwrap_or_default(),
            "phase": payload.get("phase").cloned().unwrap_or(Value::Null)
        })),
        "reasoning" => Some(json!({
            "id": id,
            "type": "reasoning",
            "summary": payload.get("summary").cloned().or_else(|| payload.get("content").cloned()).unwrap_or_else(|| json!([]))
        })),
        "function_call" => Some(json!({
            "id": id,
            "type": "dynamicToolCall",
            "callId": payload.get("call_id").and_then(Value::as_str).unwrap_or(&id),
            "tool": payload.get("name").and_then(Value::as_str).unwrap_or("function_call"),
            "arguments": parse_json_value(payload.get("arguments").cloned().unwrap_or(Value::Null))
        })),
        "custom_tool_call" | "tool_search_call" => Some(json!({
            "id": id,
            "type": "mcpToolCall",
            "callId": payload.get("call_id").and_then(Value::as_str).unwrap_or(&id),
            "tool": payload.get("name").and_then(Value::as_str).unwrap_or("tool_search"),
            "arguments": parse_json_value(payload.get("input").cloned().or_else(|| payload.get("arguments").cloned()).unwrap_or(Value::Null))
        })),
        "function_call_output" | "custom_tool_call_output" | "tool_search_output" => Some(json!({
            "id": id,
            "type": "toolResult",
            "callId": payload.get("call_id").and_then(Value::as_str).unwrap_or(&id),
            "content": parse_json_value(payload.get("output").cloned().unwrap_or(Value::Null)),
            "isError": payload.get("status").and_then(Value::as_str).is_some_and(|status| matches!(status, "failed" | "error"))
        })),
        _ => None,
    }
}

fn normalized_content(value: &Value) -> Value {
    let Some(content) = value.as_array() else {
        return json!([{"type": "text", "text": text_value(value).unwrap_or_default()}]);
    };
    Value::Array(
        content
            .iter()
            .filter_map(|item| {
                let kind = item.get("type").and_then(Value::as_str)?;
                match kind {
                    "input_text" | "output_text" | "text" => Some(json!({
                        "type": "text",
                        "text": item.get("text").and_then(Value::as_str).unwrap_or_default()
                    })),
                    "image" | "input_image" => Some(json!({
                        "type": "image",
                        "url": item.get("url").cloned().or_else(|| item.get("image_url").cloned()).unwrap_or(Value::Null)
                    })),
                    other => Some(json!({"type": other})),
                }
            })
            .collect(),
    )
}

fn content_text(value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn content_or_text(value: &Value) -> String {
    if value.is_array() {
        content_text(value)
    } else {
        text_value(value).unwrap_or_default()
    }
}

fn preview_from_value(value: &Value) -> Option<String> {
    let payload = value.get("payload").unwrap_or(value);
    if value.get("type").and_then(Value::as_str) == Some("event_msg")
        && payload.get("type").and_then(Value::as_str) != Some("user_message")
    {
        return None;
    }
    if value.get("type").and_then(Value::as_str) == Some("userMessage") {
        return value
            .get("content")
            .map(content_text)
            .filter(|text| !text.is_empty())
            .map(bounded_preview);
    }
    if value.get("type").and_then(Value::as_str) == Some("agentMessage") {
        return value
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| bounded_preview(text.to_string()));
    }
    if value.get("type").and_then(Value::as_str) == Some("response_item")
        && ((payload.get("type").and_then(Value::as_str) == Some("message")
            && payload.get("role").and_then(Value::as_str) != Some("user"))
            || payload.get("type").and_then(Value::as_str) == Some("agent_message"))
    {
        return None;
    }
    let text = if payload.get("type").and_then(Value::as_str) == Some("message") {
        payload
            .get("content")
            .map(content_or_text)
            .unwrap_or_default()
    } else {
        payload
            .get("message")
            .and_then(text_value)
            .or_else(|| payload.get("text").and_then(text_value))
            .unwrap_or_default()
    };
    (!text.is_empty()).then(|| bounded_preview(text))
}

fn text_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => object
            .get("text")
            .and_then(text_value)
            .or_else(|| object.get("message").and_then(text_value)),
        _ => None,
    }
}

fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(String::from)
}

fn parse_json_value(value: Value) -> Value {
    match value {
        Value::String(value) => serde_json::from_str(&value).unwrap_or(Value::String(value)),
        value => value,
    }
}

fn timestamp_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Value::String(value) => chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|date| date.timestamp())
            .or_else(|| value.parse::<i64>().ok()),
        _ => None,
    }
}

fn modified_epoch(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or_default()
        })
}

fn bounded_preview(value: String) -> String {
    let mut value = value.trim().to_string();
    if let Some((boundary, _)) = value.char_indices().nth(PREVIEW_MAX_CHARS) {
        value.truncate(boundary);
        value.push('…');
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_content_preserves_user_image_data() {
        assert_eq!(
            normalized_content(&json!([{
                "type": "input_image",
                "image_url": "data:image/png;base64,aW1hZ2U="
            }])),
            json!([{
                "type": "image",
                "url": "data:image/png;base64,aW1hZ2U="
            }])
        );
    }

    #[test]
    fn reads_one_mcp_image_block_from_rollout_event() {
        let path = std::env::temp_dir().join(format!(
            "monet-codex-mcp-image-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(
            &path,
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "mcp_tool_call_end",
                    "call_id": "exec-image-1",
                    "result": {
                        "Ok": {
                            "content": [
                                { "type": "text", "text": "done" },
                                { "type": "image", "mimeType": "image/png", "data": "aW1hZ2U=" }
                            ]
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let image = read_mcp_result_content_item_from_path(&path, "exec-image-1", 1).unwrap();
        assert_eq!(image["mimeType"], "image/png");
        assert_eq!(image["data"], "aW1hZ2U=");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn custom_tool_calls_preserve_pairing_id_and_program_source() {
        let item = normalize_response_item(
            &json!({
                "type": "custom_tool_call",
                "id": "program-item-1",
                "call_id": "program-call-1",
                "name": "exec",
                "input": "const r = await tools.example({ title: \"检查界面\" });"
            }),
            1,
        )
        .unwrap();

        assert_eq!(item["id"], "program-item-1");
        assert_eq!(item["callId"], "program-call-1");
        assert_eq!(
            item["arguments"].as_str(),
            Some("const r = await tools.example({ title: \"检查界面\" });")
        );
    }

    #[test]
    fn reads_local_codex_jsonl_into_app_server_like_thread() {
        let path = std::env::temp_dir().join(format!(
            "monet-codex-file-source-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let rows = [
            json!({
                "type": "session_meta",
                "payload": {
                    "id": "session-1",
                    "cwd": "/tmp/project",
                    "model_provider": "openai",
                    "timestamp": "2026-08-06T00:00:00Z"
                }
            }),
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "task_started",
                    "turn_id": "turn-1",
                    "started_at": "2026-08-06T00:00:01Z"
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": "user-1",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "hello"}]
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "id": "function-item-1",
                    "call_id": "call-1",
                    "name": "shell",
                    "arguments": "{\"command\":\"pwd\"}"
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "call_id": "call-1",
                    "output": "ok"
                }
            }),
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "agent_message",
                    "phase": "final_answer",
                    "message": "done"
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": "assistant-1",
                    "role": "assistant",
                    "phase": "final_answer",
                    "content": [{"type": "output_text", "text": "done"}]
                }
            }),
        ];
        fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        let thread = read_thread_file(&path).unwrap();
        assert_eq!(thread.id, "session-1");
        assert_eq!(thread.preview, "hello");
        assert_eq!(thread.cwd, "/tmp/project");
        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.turns[0].items.len(), 4);
        assert_eq!(thread.turns[0].items[1]["type"], "dynamicToolCall");
        assert_eq!(thread.turns[0].items[1]["callId"], "call-1");
        assert_eq!(thread.turns[0].items[2]["type"], "toolResult");
        assert_eq!(thread.turns[0].items[3]["text"], "done");
        assert_eq!(thread.turns[0].items[3]["phase"], "final_answer");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn local_timeline_uses_canonical_user_events_instead_of_injected_context() {
        let path = std::env::temp_dir().join(format!(
            "monet-codex-canonical-user-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let rows = [
            json!({
                "type": "session_meta",
                "payload": {
                    "id": "session-1",
                    "cwd": "/tmp/project",
                    "timestamp": "2026-08-11T00:00:00Z"
                }
            }),
            json!({
                "type": "event_msg",
                "payload": { "type": "task_started", "turn_id": "turn-1" }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": "injected-context",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "<workspace_context>hidden</workspace_context>" }]
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": "user-1",
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "first prompt" },
                        { "type": "input_text", "text": "hidden image annotation" },
                        { "type": "input_image", "image_url": "data:image/png;base64,aW1hZ2U=" }
                    ]
                }
            }),
            json!({
                "type": "event_msg",
                "payload": { "type": "user_message", "message": "first prompt" }
            }),
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "agent_message",
                    "phase": "final_answer",
                    "message": "first reply"
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": "assistant-1",
                    "role": "assistant",
                    "phase": "final_answer",
                    "content": [{ "type": "output_text", "text": "first reply" }]
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": "user-2",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "second prompt" }]
                }
            }),
            json!({
                "type": "event_msg",
                "payload": { "type": "user_message", "message": "second prompt" }
            }),
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "agent_message",
                    "phase": "final_answer",
                    "message": "second reply"
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "id": "assistant-2",
                    "role": "assistant",
                    "phase": "final_answer",
                    "content": [{ "type": "output_text", "text": "second reply" }]
                }
            }),
        ];
        fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        let summary = read_thread_summary(&path).unwrap();
        assert_eq!(summary.preview, "first prompt");

        let thread = read_thread_file(&path).unwrap();
        assert_eq!(thread.preview, "first prompt");
        assert_eq!(thread.turns.len(), 1);
        let items = &thread.turns[0].items;
        assert_eq!(items.len(), 4);
        assert_eq!(content_text(&items[0]["content"]), "first prompt");
        assert_eq!(items[0]["content"].as_array().unwrap().len(), 2);
        assert_eq!(items[1]["text"], "first reply");
        assert_eq!(content_text(&items[2]["content"]), "second prompt");
        assert_eq!(items[3]["text"], "second reply");
        assert!(items
            .iter()
            .all(|item| !item.to_string().contains("hidden")));
        assert!(items.iter().all(|item| {
            item.get("canonicalUserMessage").is_none() && item.get("eventAgentMessage").is_none()
        }));

        let _ = fs::remove_file(path);
    }
}
