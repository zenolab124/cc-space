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
    let mut scanned_bytes = 0usize;
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
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            meta = value.get("payload").cloned();
        }
        if preview.is_none() {
            preview = preview_from_value(&value);
        }
        if (meta.is_some() && preview.is_some()) || scanned_bytes >= SUMMARY_SCAN_LIMIT {
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
        preview: preview.unwrap_or_default(),
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
                        "content": [{"type": "text", "text": text}]
                    });
                    let turn_id = self.current_turn_id();
                    self.add_item_if_text_is_new(&turn_id, item, &text);
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
                        "phase": payload.get("phase").cloned().unwrap_or(Value::Null)
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
        if item.get("type").and_then(Value::as_str) == Some("userMessage")
            && self.preview.is_empty()
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

    fn finish(self) -> EngineResult<CodexThread> {
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
                    "id": "call-1",
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
        assert_eq!(thread.turns[0].items[1]["type"], "dynamicToolCall");
        assert_eq!(thread.turns[0].items[2]["type"], "toolResult");
        assert_eq!(thread.turns[0].items[3]["text"], "done");
        assert_eq!(thread.turns[0].items[3]["phase"], "final_answer");

        let _ = fs::remove_file(path);
    }
}
