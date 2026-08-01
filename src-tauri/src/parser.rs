use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::*;

/// 轻量结构体：仅提取 assistant 消息的 id/usage，跳过 content 反序列化
#[derive(Deserialize)]
struct UsageExtractor {
    #[serde(rename = "type")]
    record_type: Option<String>,
    timestamp: Option<String>,
    message: Option<UsageMessage>,
}

#[derive(Deserialize)]
struct UsageMessage {
    id: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageLedger {
    pub by_id: HashMap<String, UsageSnapshot>,
    pub anonymous: Vec<UsageSnapshot>,
}

impl UsageLedger {
    pub fn insert(&mut self, id: Option<String>, snapshot: UsageSnapshot) {
        match id {
            Some(id) => match self.by_id.get_mut(&id) {
                Some(current) if snapshot.is_better_than(current) => *current = snapshot,
                Some(_) => {}
                None => {
                    self.by_id.insert(id, snapshot);
                }
            },
            None => self.anonymous.push(snapshot),
        }
    }

    pub fn merge(&mut self, other: UsageLedger) {
        for (id, snapshot) in other.by_id {
            self.insert(Some(id), snapshot);
        }
        self.anonymous.extend(other.anonymous);
    }

    pub fn total(&self) -> TokenUsage {
        let mut total = TokenUsage::default();
        for snapshot in self.by_id.values().chain(self.anonymous.iter()) {
            total.accumulate(&snapshot.usage);
        }
        total
    }
}

/// 解析对话消息与异步任务通知，跳过 file-history-snapshot 等大型记录
/// 避免 Value 中间层，直接反序列化到目标类型
pub fn parse_messages(path: &Path) -> Vec<SessionRecord> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };
    let reader = BufReader::with_capacity(64 * 1024, file);
    let mut results = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        // 快速字符串检测，跳过非消息类型（避免解析巨大的 snapshot 等）
        if should_skip_message_line(&line) {
            continue;
        }

        // 直接反序列化到目标类型，不经过 Value 中间层
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(mut record) = SessionRecord::from_json_owned(value) {
            // 为每个 image block 注入深度优先序号（ccimg 协议按此 img_index 反查 base64）
            inject_image_indices(&mut record);
            results.push(record);
        }
    }

    results
}

/// queue-operation 通常不是消息，但新版本 CLI 会借它投递
/// `<task-notification>`；只为这类小记录打开解析通道。attachment 原本就会进入
/// typed 解析，因此其通知由 SessionRecord 直接识别。
fn should_skip_message_line(line: &str) -> bool {
    if line.contains("\"file-history-snapshot\"") || line.contains("\"ai-title\"") {
        return true;
    }
    line.contains("\"queue-operation\"") && !line.contains("<task-notification>")
}

// ============================================================================
// image block 深度优先遍历 —— img_index 的单一权威定义
// ----------------------------------------------------------------------------
// img_index = record 内第 N 个 image block（0 起）。遍历顺序（深度优先）：
//   顶层 message.content 数组按序遍历；遇到 tool_result 且其 content 为 Blocks
//   时，先递归遍历其内嵌 blocks，再继续外层。
//
// 这套顺序是 Rust parser（注入序号）与 ccimg 协议 handler（按序号反查 base64）
// 的共同契约。parser 走 typed 结构注入，handler 走 raw JSON 提取——两条路径必须
// 产出完全一致的序号。计数口径 = 「type == "image" 即计数」：typed 侧靠 ImageSource
// 全字段 default 保证畸形块（缺 media_type / 缺 source）也进 Image 变体不落 Unknown。
// 交叉验证测试：image_protocol::tests::traversal_order_matches_typed_injection。
// ============================================================================

/// 给一条记录内所有 image block 按深度优先序注入 img_index（typed 路径）。
/// 仅 User / Assistant 记录携带 message.content，其余记录无 image，跳过。
fn inject_image_indices(record: &mut SessionRecord) {
    let counter = &mut 0u32;
    match record {
        SessionRecord::User(u) => {
            if let Some(msg) = u.message.as_mut() {
                if let MessageContent::Blocks(blocks) = &mut msg.content {
                    walk_blocks_assign(blocks, counter);
                }
            }
        }
        SessionRecord::Assistant(a) => {
            if let Some(msg) = a.message.as_mut() {
                walk_blocks_assign(&mut msg.content, counter);
            }
        }
        _ => {}
    }
}

/// 深度优先遍历 typed blocks，为遇到的每个 Image 赋递增 img_index。
/// pub(crate)：image_protocol 的交叉验证测试直接调用，确保与 raw 遍历序号一致
pub(crate) fn walk_blocks_assign(blocks: &mut [ContentBlock], counter: &mut u32) {
    for block in blocks.iter_mut() {
        match block {
            ContentBlock::Image { source } => {
                source.img_index = *counter;
                *counter += 1;
            }
            ContentBlock::ToolResult {
                content: ToolResultContent::Blocks(inner),
                ..
            } => {
                walk_blocks_assign(inner, counter);
            }
            _ => {}
        }
    }
}

/// 懒解析：提取摘要信息，不加载完整对话
/// 前 max_lines 行完整解析提取元数据，后续行用轻量结构体仅提取 token usage
pub fn parse_summary_with_usage(
    path: &Path,
    max_lines: usize,
) -> Option<(SessionSummary, UsageLedger)> {
    let metadata = fs::metadata(path).ok()?;
    let file_size = metadata.len();
    let last_modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs_f64();

    let session_id = path.file_stem()?.to_str()?.to_string();

    let file = File::open(path).ok()?;
    let reader = BufReader::with_capacity(64 * 1024, file);

    let mut title: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut first_user_message: Option<String> = None;
    let mut model: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut version: Option<String> = None;
    let mut earliest_timestamp: Option<String> = None;
    let mut usage_ledger = UsageLedger::default();
    let mut message_count: u32 = 0;
    let mut context_window: Option<u64> = None;

    for (i, line) in reader.lines().enumerate() {
        let line = match line.ok() {
            Some(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        if i < max_lines {
            // 前 max_lines 行：完整解析提取所有元数据
            let value: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let record_type = value.get("type").and_then(|t| t.as_str());

            match record_type {
                Some("user") => {
                    message_count += 1;
                    if first_user_message.is_none() {
                        first_user_message = extract_first_text(&value);
                    }
                    if earliest_timestamp.is_none() {
                        earliest_timestamp =
                            value.get("timestamp").and_then(|t| t.as_str()).map(String::from);
                    }
                    if git_branch.is_none() {
                        git_branch = value
                            .get("gitBranch")
                            .and_then(|b| b.as_str())
                            .map(String::from);
                    }
                    if cwd.is_none() {
                        cwd = value.get("cwd").and_then(|c| c.as_str()).map(String::from);
                    }
                    if version.is_none() {
                        version = value
                            .get("version")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                }
                Some("assistant") => {
                    message_count += 1;
                    if let Some(msg) = value.get("message") {
                        if model.is_none() {
                            model = msg.get("model").and_then(|m| m.as_str()).map(String::from);
                        }
                        if let Some(usage) = msg.get("usage") {
                            let usage: TokenUsage =
                                serde_json::from_value(usage.clone()).unwrap_or_default();
                            usage_ledger.insert(
                                msg.get("id").and_then(|i| i.as_str()).map(String::from),
                                UsageSnapshot::new(
                                    usage,
                                    msg.get("stop_reason").and_then(|v| v.as_str()),
                                    value
                                        .get("timestamp")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    i as u64,
                                ),
                            );
                        }
                    }
                    if earliest_timestamp.is_none() {
                        earliest_timestamp =
                            value.get("timestamp").and_then(|t| t.as_str()).map(String::from);
                    }
                }
                Some("ai-title") => {
                    title = value
                        .get("aiTitle")
                        .and_then(|t| t.as_str())
                        .map(String::from);
                }
                Some("custom-title") => {
                    custom_title = value
                        .get("customTitle")
                        .and_then(|t| t.as_str())
                        .map(String::from);
                }
                Some("result") => {
                    if let Some(cw) = value
                        .get("modelUsage")
                        .and_then(|u| u.get("contextWindow"))
                        .and_then(|v| v.as_u64())
                    {
                        context_window = Some(cw);
                    }
                }
                _ => {}
            }
        } else {
            // 后续行：轻量路径，只提取 token 和计数
            // 快速字符串检测，跳过不相关行
            if line.contains("\"file-history-snapshot\"") || line.contains("\"queue-operation\"") {
                continue;
            }

            if line.contains("\"ai-title\"") {
                // 用轻量解析提取标题
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if value.get("type").and_then(|t| t.as_str()) == Some("ai-title") {
                        title = value
                            .get("aiTitle")
                            .and_then(|t| t.as_str())
                            .map(String::from);
                    }
                }
                continue;
            }

            if line.contains("\"custom-title\"") {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if value.get("type").and_then(|t| t.as_str()) == Some("custom-title") {
                        custom_title = value
                            .get("customTitle")
                            .and_then(|t| t.as_str())
                            .map(String::from);
                    }
                }
                continue;
            }

            if line.contains("\"user\"") && !line.contains("\"assistant\"") {
                message_count += 1;
                continue;
            }

            if line.contains("\"assistant\"") {
                message_count += 1;
                // 用轻量结构体只提取 usage，跳过 content 反序列化
                if line.contains("\"usage\"") {
                    if let Ok(ext) = serde_json::from_str::<UsageExtractor>(&line) {
                        if ext.record_type.as_deref() == Some("assistant") {
                            if let Some(msg) = ext.message {
                                if let Some(u) = msg.usage {
                                    usage_ledger.insert(
                                        msg.id,
                                        UsageSnapshot::new(
                                            u,
                                            msg.stop_reason.as_deref(),
                                            ext.timestamp,
                                            i as u64,
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                continue;
            }

            if line.contains("\"result\"") && line.contains("\"modelUsage\"") {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if value.get("type").and_then(|t| t.as_str()) == Some("result") {
                        if let Some(cw) = value
                            .get("modelUsage")
                            .and_then(|u| u.get("contextWindow"))
                            .and_then(|v| v.as_u64())
                        {
                            context_window = Some(cw);
                        }
                    }
                }
                continue;
            }
        }
    }

    // 如果前面没找到 ai-title，在文件尾部搜索
    if title.is_none() && custom_title.is_none() {
        title = search_tail_for_title(path, 4096);
    }

    // 用户手动标题（/title 命令写入的 custom-title）优先于 AI 生成标题
    let title = custom_title.or(title);

    let total_tokens = usage_ledger.total();
    let summary = SessionSummary {
        id: session_id,
        title,
        first_user_message,
        model,
        git_branch,
        cwd,
        version,
        timestamp: earliest_timestamp,
        last_modified,
        total_tokens,
        subagent_tokens: TokenUsage::default(),
        file_size,
        message_count,
        context_window,
    };
    Some((summary, usage_ledger))
}

#[cfg(test)]
pub fn parse_summary(path: &Path, max_lines: usize) -> Option<SessionSummary> {
    parse_summary_with_usage(path, max_lines).map(|(summary, _)| summary)
}

/// 提取单个 JSONL 文件的 token 用量账本。
/// 同一 message.id 可能先写零值占位、后写最终快照；账本保留质量最高的一份。
pub fn parse_usage_ledger(path: &Path) -> UsageLedger {
    let mut ledger = UsageLedger::default();
    let Ok(file) = File::open(path) else {
        return ledger;
    };
    let reader = BufReader::with_capacity(64 * 1024, file);

    for (sequence, line) in reader.lines().enumerate() {
        let Ok(line) = line else { continue };
        if !line.contains("\"assistant\"") || !line.contains("\"usage\"") {
            continue;
        }
        let Ok(ext) = serde_json::from_str::<UsageExtractor>(&line) else {
            continue;
        };
        if ext.record_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(msg) = ext.message else { continue };
        if msg.model.as_deref() == Some("<synthetic>") {
            continue;
        }
        let Some(usage) = msg.usage else { continue };
        ledger.insert(
            msg.id,
            UsageSnapshot::new(
                usage,
                msg.stop_reason.as_deref(),
                ext.timestamp,
                sequence as u64,
            ),
        );
    }
    ledger
}

#[cfg(test)]
pub fn parse_subagent_usage(path: &Path) -> TokenUsage {
    parse_usage_ledger(path).total()
}

/// 从文件尾部搜索 ai-title 记录
fn search_tail_for_title(path: &Path, tail_size: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let seek_pos = file_len.saturating_sub(tail_size);
    file.seek(SeekFrom::Start(seek_pos)).ok()?;

    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;

    // 从后往前查找 ai-title
    for line in buf.lines().rev() {
        if line.contains("\"ai-title\"") {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                if value.get("type").and_then(|t| t.as_str()) == Some("ai-title") {
                    return value
                        .get("aiTitle")
                        .and_then(|t| t.as_str())
                        .map(String::from);
                }
            }
        }
    }
    None
}

/// 从 JSONL value 中提取第一段用户文本（截断到 200 字符，降低 IPC 载荷）
fn extract_first_text(value: &Value) -> Option<String> {
    let message = value.get("message")?;
    let content = message.get("content")?;

    let raw = if let Some(text) = content.as_str() {
        text.to_string()
    } else {
        let blocks = content.as_array()?;
        let mut found = None;
        for block in blocks {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                found = block.get("text").and_then(|t| t.as_str()).map(String::from);
                if found.is_some() {
                    break;
                }
            }
        }
        found?
    };

    Some(truncate_chars(&raw, 200))
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut chars = s.char_indices();
    if chars.nth(max).is_some() {
        let byte_end = s.char_indices().nth(max).unwrap().0;
        format!("{}…", &s[..byte_end])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn assistant_line_with_stop(
        id: Option<&str>,
        input_tokens: u64,
        output_tokens: u64,
        stop_reason: Option<&str>,
    ) -> String {
        let id_part = id.map(|i| format!("\"id\":\"{i}\",")).unwrap_or_default();
        let stop_part = stop_reason
            .map(|s| format!("\"stop_reason\":\"{s}\","))
            .unwrap_or_default();
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"2026-06-11T10:00:00.000Z\",\"message\":{{{id_part}{stop_part}\"model\":\"claude-fable-5\",\"usage\":{{\"input_tokens\":{input_tokens},\"output_tokens\":{output_tokens},\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}}}}"
        )
    }

    fn assistant_line(id: Option<&str>, output_tokens: u64) -> String {
        assistant_line_with_stop(id, 10, output_tokens, None)
    }

    #[test]
    fn keeps_only_notification_control_records() {
        assert!(!should_skip_message_line(
            r#"{"type":"queue-operation","content":"<task-notification></task-notification>"}"#,
        ));
        assert!(!should_skip_message_line(
            r#"{"type":"attachment","attachment":{"prompt":"<task-notification></task-notification>"}}"#,
        ));
        assert!(should_skip_message_line(
            r#"{"type":"queue-operation","operation":"remove"}"#,
        ));
        assert!(!should_skip_message_line(
            r#"{"type":"attachment","attachment":{"prompt":"ordinary queued command"}}"#,
        ));
    }

    /// 同一 message.id 只计一次；后续最终快照替换零值占位。
    /// 同时覆盖完整路径（前 max_lines）与轻量路径（之后）共享账本。
    #[test]
    fn summary_selects_best_usage_snapshot() {
        let path = std::env::temp_dir().join("monet-test-dedup.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "{}", assistant_line_with_stop(Some("msg_a"), 0, 0, None)).unwrap();
        writeln!(f, "{}", assistant_line(None, 7)).unwrap();
        writeln!(f, "{}", assistant_line(Some("msg_b"), 50)).unwrap();
        writeln!(
            f,
            "{}",
            assistant_line_with_stop(Some("msg_a"), 100, 25, Some("tool_use"))
        )
        .unwrap();
        writeln!(f, "{}", assistant_line_with_stop(Some("msg_a"), 0, 0, None)).unwrap();
        drop(f);

        let summary = parse_summary(&path, 3).unwrap();
        assert_eq!(summary.total_tokens.total(), 125 + 17 + 60);
        assert_eq!(summary.message_count, 5);
        fs::remove_file(&path).ok();
    }

    /// 子 Agent 转录 usage 累计：同 id 去重、非 assistant 行忽略
    #[test]
    fn subagent_usage_accumulates_with_dedup() {
        let path = std::env::temp_dir().join("monet-test-subagent-usage.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "{{\"type\":\"user\",\"message\":{{\"content\":\"hi assistant usage\"}}}}").unwrap();
        writeln!(f, "{}", assistant_line_with_stop(Some("msg_x"), 0, 0, None)).unwrap();
        writeln!(
            f,
            "{}",
            assistant_line_with_stop(Some("msg_x"), 10, 200, Some("tool_use"))
        )
        .unwrap();
        writeln!(f, "{}", assistant_line(Some("msg_y"), 30)).unwrap();
        drop(f);

        let usage = parse_subagent_usage(&path);
        // msg_x 计一次(210) + msg_y(40)
        assert_eq!(usage.total(), 210 + 40);
        fs::remove_file(&path).ok();
    }
}
