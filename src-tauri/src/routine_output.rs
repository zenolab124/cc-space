//! Routine CLI 输出归一化。
//!
//! Claude Code 的 text 输出可直接展示；Codex `exec --json` 输出 JSONL，日志只
//! 展示最后一条完成的 agent message，避免把协议事件或正常进度误当成警告。

use serde_json::Value;

use crate::routine_types::RoutineEngine;

pub fn normalize_routine_stdout(engine: &RoutineEngine, stdout: &[u8]) -> String {
    let raw = String::from_utf8_lossy(stdout);
    if !engine.is_codex() {
        return raw.into_owned();
    }

    let last_message = raw
        .lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            if value.get("type").and_then(Value::as_str) != Some("item.completed") {
                return None;
            }
            let item = value.get("item")?;
            if item.get("type").and_then(Value::as_str) != Some("agent_message") {
                return None;
            }
            item.get("text").and_then(Value::as_str).map(str::to_string)
        })
        .next_back();

    last_message.unwrap_or_else(|| raw.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_final_codex_agent_message() {
        let stdout = br#"{"type":"thread.started","thread_id":"thread-1"}
{"type":"item.completed","item":{"id":"one","type":"agent_message","text":"First"}}
{"type":"item.completed","item":{"id":"two","type":"agent_message","text":"Final answer"}}
{"type":"turn.completed","usage":{"input_tokens":1}}
"#;
        assert_eq!(
            normalize_routine_stdout(&RoutineEngine::codex(), stdout),
            "Final answer"
        );
    }

    #[test]
    fn preserves_raw_output_when_codex_has_no_final_message() {
        let stdout = br#"{"type":"turn.failed","error":{"message":"failed"}}"#;
        assert_eq!(
            normalize_routine_stdout(&RoutineEngine::codex(), stdout),
            String::from_utf8_lossy(stdout)
        );
    }

    #[test]
    fn leaves_claude_text_unchanged() {
        assert_eq!(
            normalize_routine_stdout(&RoutineEngine::claude_code(), b"Summary"),
            "Summary"
        );
    }
}
