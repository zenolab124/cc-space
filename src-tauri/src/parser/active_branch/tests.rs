use super::*;
use crate::models::{ContentBlock, MessageContent};

fn select(lines: &[&str]) -> Vec<SessionRecord> {
    let records = lines
        .iter()
        .enumerate()
        .map(|(order, line)| {
            let value: Value = serde_json::from_str(line).unwrap();
            let meta = BranchMeta::from_json(&value, order);
            let mut record = SessionRecord::from_json_owned(value).unwrap();
            super::super::inject_image_indices(&mut record);
            BranchRecord::new(record, meta)
        })
        .collect();
    select_active_branch(records)
}

fn uuids(records: &[SessionRecord]) -> Vec<&str> {
    records
        .iter()
        .filter_map(|record| match record {
            SessionRecord::User(record) => record.uuid.as_deref(),
            SessionRecord::Assistant(record) => record.uuid.as_deref(),
            SessionRecord::System(record) => record.uuid.as_deref(),
            _ => None,
        })
        .collect()
}

fn user(uuid: &str, parent: Option<&str>, timestamp: &str) -> String {
    let parent = parent
        .map(|id| format!(",\"parentUuid\":\"{id}\""))
        .unwrap_or_default();
    format!(
        r#"{{"type":"user","uuid":"{uuid}"{parent},"timestamp":"{timestamp}","message":{{"role":"user","content":"{uuid}"}}}}"#
    )
}

fn assistant(uuid: &str, parent: &str, timestamp: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","parentUuid":"{parent}","timestamp":"{timestamp}","message":{{"role":"assistant","content":[{{"type":"text","text":"{uuid}"}}]}}}}"#
    )
}

fn refs(lines: &[String]) -> Vec<&str> {
    lines.iter().map(String::as_str).collect()
}

#[test]
fn linear_history_is_unchanged() {
    let lines = [
        user("u1", None, "2026-01-01T00:00:00Z"),
        assistant("a1", "u1", "2026-01-01T00:00:01Z"),
        user("u2", Some("a1"), "2026-01-01T00:00:02Z"),
    ];
    assert_eq!(uuids(&select(&refs(&lines))), ["u1", "a1", "u2"]);
}

#[test]
fn selects_newer_rewind_branch() {
    let lines = [
        user("root", None, "2026-01-01T00:00:00Z"),
        user("old", Some("root"), "2026-01-01T00:00:01Z"),
        user("active", Some("root"), "2026-01-01T00:00:02Z"),
    ];
    assert_eq!(uuids(&select(&refs(&lines))), ["root", "active"]);
}

#[test]
fn last_prompt_selects_active_leaf_across_rewind_roots() {
    let lines = [
        user("old-root", None, "2026-01-01T00:00:00Z"),
        assistant("old-leaf", "old-root", "2026-01-01T00:00:01Z"),
        user("active-root", None, "2026-01-01T00:00:02Z"),
        assistant("active-leaf", "active-root", "2026-01-01T00:00:03Z"),
        r#"{"type":"last-prompt","leafUuid":"active-leaf","sessionId":"session-1"}"#.to_string(),
    ];
    let records = select(&refs(&lines));
    assert_eq!(uuids(&records), ["active-root", "active-leaf"]);
    assert_eq!(
        records.len(),
        3,
        "last-prompt control record must be retained"
    );
}

#[test]
fn unknown_last_prompt_leaf_fails_open() {
    let lines = [
        user("old-root", None, "2026-01-01T00:00:00Z"),
        user("new-root", None, "2026-01-01T00:00:01Z"),
        r#"{"type":"last-prompt","leafUuid":"missing","sessionId":"session-1"}"#.to_string(),
    ];
    assert_eq!(select(&refs(&lines)).len(), 3);
}

#[test]
fn leaf_uuid_on_other_record_types_is_ignored() {
    let lines = [
        user("old-root", None, "2026-01-01T00:00:00Z"),
        user("new-root", None, "2026-01-01T00:00:01Z"),
        r#"{"type":"progress","leafUuid":"new-root","sessionId":"session-1"}"#.to_string(),
    ];
    assert_eq!(select(&refs(&lines)).len(), 3);
}

#[test]
fn file_order_breaks_retry_timestamp_tie() {
    let lines = [
        user("u1", None, "2026-01-01T00:00:00Z"),
        assistant("retry-old", "u1", "2026-01-01T00:00:01Z"),
        assistant("retry-active", "u1", "2026-01-01T00:00:01Z"),
    ];
    assert_eq!(uuids(&select(&refs(&lines))), ["u1", "retry-active"]);
}

#[test]
fn file_order_wins_when_leaf_timestamp_is_missing() {
    let lines = [
        user("u1", None, "2026-01-01T00:00:00Z"),
        assistant("dated", "u1", "2026-01-01T00:00:02Z"),
        assistant("undated", "u1", "not-a-timestamp"),
    ];
    assert_eq!(uuids(&select(&refs(&lines))), ["u1", "undated"]);
}

#[test]
fn sidechain_cannot_become_active_leaf() {
    let side = user("side", Some("root"), "2026-01-01T00:00:02Z")
        .replace("\"timestamp\"", "\"isSidechain\":true,\"timestamp\"");
    let lines = [
        user("root", None, "2026-01-01T00:00:00Z"),
        user("main", Some("root"), "2026-01-01T00:00:01Z"),
        side,
    ];
    assert_eq!(uuids(&select(&refs(&lines))), ["root", "main"]);
}

#[test]
fn logical_parent_bridges_missing_compact_parent() {
    let compact = r#"{"type":"system","subtype":"compact_boundary","uuid":"compact","parentUuid":"missing","logicalParentUuid":"u1","timestamp":"2026-01-01T00:00:01Z"}"#.to_string();
    let lines = [
        user("u1", None, "2026-01-01T00:00:00Z"),
        user("old", Some("u1"), "2026-01-01T00:00:00.500Z"),
        compact,
        user("u2", Some("compact"), "2026-01-01T00:00:02Z"),
    ];
    assert_eq!(uuids(&select(&refs(&lines))), ["u1", "compact", "u2"]);
}

#[test]
fn keeps_parallel_tool_results_referenced_by_active_call() {
    let root = user("u1", None, "2026-01-01T00:00:00Z");
    let call = r#"{"type":"assistant","uuid":"call","parentUuid":"u1","timestamp":"2026-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}},{"type":"tool_use","id":"t2","name":"Read","input":{}}]}}"#;
    let stale = r#"{"type":"user","uuid":"stale","parentUuid":"call","timestamp":"2026-01-01T00:00:01.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"unrelated","content":"stale"}]}}"#;
    let result1 = r#"{"type":"user","uuid":"r1","parentUuid":"call","timestamp":"2026-01-01T00:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"one"}]}}"#;
    let result2 = r#"{"type":"user","uuid":"r2","parentUuid":"call","timestamp":"2026-01-01T00:00:03Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"two"}]}}"#;
    assert_eq!(
        uuids(&select(&[&root, call, stale, result1, result2])),
        ["u1", "call", "r1", "r2"]
    );
}

#[test]
fn malformed_graphs_fail_open() {
    let duplicate = [
        user("same", None, "2026-01-01T00:00:00Z"),
        user("same", None, "2026-01-01T00:00:01Z"),
    ];
    assert_eq!(select(&refs(&duplicate)).len(), 2);
    let cycle = [
        user("root", None, "2026-01-01T00:00:02Z"),
        user("a", Some("b"), "2026-01-01T00:00:00Z"),
        user("b", Some("a"), "2026-01-01T00:00:01Z"),
    ];
    assert_eq!(select(&refs(&cycle)).len(), 3);
    let broken = [
        user("root", None, "2026-01-01T00:00:02Z"),
        user("broken", Some("missing"), "2026-01-01T00:00:01Z"),
    ];
    assert_eq!(select(&refs(&broken)).len(), 2);
    let multiple_roots = [
        user("root", None, "2026-01-01T00:00:00Z"),
        r#"{"type":"system","subtype":"interruption","uuid":"detached","timestamp":"2026-01-01T00:00:01Z"}"#.to_string(),
    ];
    assert_eq!(select(&refs(&multiple_roots)).len(), 2);
}

#[test]
fn keeps_uuidless_control_records_and_image_indices() {
    let image = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png"}},{"type":"image","source":{"type":"base64","media_type":"image/jpeg"}}]}}"#;
    let control =
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-01-01T00:00:01Z"}"#;
    let records = select(&[image, control]);
    assert_eq!(records.len(), 2);
    let SessionRecord::User(user) = &records[0] else {
        panic!()
    };
    let MessageContent::Blocks(blocks) = &user.message.as_ref().unwrap().content else {
        panic!()
    };
    let indices: Vec<u32> = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image { source } => Some(source.img_index),
            _ => None,
        })
        .collect();
    assert_eq!(indices, [0, 1]);
}

#[test]
fn parse_messages_filters_fixture_without_writing_it() {
    let path =
        std::env::temp_dir().join(format!("monet-active-branch-{}.jsonl", std::process::id()));
    let lines = [
        user("root", None, "2026-01-01T00:00:00Z"),
        user("old", Some("root"), "2026-01-01T00:00:01Z"),
        r#"{"type":"system","subtype":"interruption","uuid":"sys","parentUuid":"root","timestamp":"2026-01-01T00:00:02Z"}"#.to_string(),
        user("active", Some("sys"), "2026-01-01T00:00:03Z"),
        r#"{"type":"queue-operation","content":"<task-notification>done</task-notification>","timestamp":"2026-01-01T00:00:04Z"}"#.to_string(),
    ];
    std::fs::write(&path, lines.join("\n")).unwrap();
    let before = std::fs::read(&path).unwrap();

    let records = super::super::parse_messages(&path);

    assert_eq!(uuids(&records), ["root", "sys", "active"]);
    assert_eq!(records.len(), 4);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).ok();
}
