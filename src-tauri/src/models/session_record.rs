use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ContentBlock, TokenUsage};

/// JSONL 行的顶层记录，按 type 字段区分
#[allow(clippy::large_enum_variant)] // User 变体较大但 Box 会改变所有 match 分支，不值得
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionRecord {
    User(UserRecord),
    Assistant(AssistantRecord),
    System(SystemRecord),
    TaskNotification(TaskNotificationRecord),
    AiTitle(AiTitleRecord),
    QueueOperation(QueueOperationRecord),
    FileHistorySnapshot(FileHistorySnapshotRecord),
    Unknown { raw_type: String },
}

/// CLI 注入的异步任务状态通知。
///
/// 不同 CLI 版本会把同一份 `<task-notification>` 放在 user、queue-operation
/// 或 attachment 记录中；解析层统一成这一变体，避免协议载体变化渗入前端账本。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNotificationRecord {
    pub timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    pub uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub timestamp: Option<String>,
    pub cwd: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
    #[serde(rename = "isSidechain")]
    pub is_sidechain: Option<bool>,
    #[serde(rename = "userType")]
    pub user_type: Option<String>,
    #[serde(rename = "permissionMode")]
    pub permission_mode: Option<String>,
    pub message: Option<UserMessage>,
    /// 顶层 toolUseResult 的异步任务精简提取（from_json_owned 回填，不直接反序列化，
    /// 避免透传前台 Agent 完整报告等大体积字段）
    #[serde(skip_deserializing)]
    pub async_meta: Option<AsyncMeta>,
    /// 顶层 origin.kind（"task-notification" = harness 注入的异步任务终态通知）
    #[serde(skip_deserializing)]
    pub origin_kind: Option<String>,
}

/// 异步任务账本所需的 toolUseResult 精简字段集。
/// 各物种占位/回执的字段并集，全部可缺省；任一字段命中才挂载（见 extract_async_meta）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AsyncMeta {
    /// 后台 Bash：任务 ID（b 前缀 base36），终态通知 <task-id> 与之相等
    pub background_task_id: Option<String>,
    /// "async_launched"（后台占位）/ "completed"（前台 Agent 完成）
    pub status: Option<String>,
    pub is_async: Option<bool>,
    /// Agent：子 agent ID（hex），即 subagents/agent-<id>.jsonl 文件名
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub resolved_model: Option<String>,
    pub description: Option<String>,
    /// Workflow/Monitor：任务 ID（w/b 前缀）；通知 <task-id> 用它而非 run_id
    pub task_id: Option<String>,
    /// "local_workflow" / "local_bash"
    pub task_type: Option<String>,
    pub workflow_name: Option<String>,
    /// Workflow：wf_ 前缀，= subagents/workflows/<run_id>/ 目录名
    pub run_id: Option<String>,
    pub summary: Option<String>,
    pub output_file: Option<String>,
    /// ScheduleWakeup：精确触发时刻 epoch_ms
    pub scheduled_for: Option<f64>,
    /// Monitor：超时与持续监视标记
    pub timeout_ms: Option<u64>,
    pub persistent: Option<bool>,
    /// SendMessage：续聊目标 agent
    pub resumed_agent_id: Option<String>,
}

/// 从顶层 toolUseResult Value 提取异步精简字段；无任何命中返回 None
fn extract_async_meta(v: &Value) -> Option<AsyncMeta> {
    let s = |key: &str| v.get(key).and_then(Value::as_str).map(String::from);
    let meta = AsyncMeta {
        background_task_id: s("backgroundTaskId"),
        status: s("status"),
        is_async: v.get("isAsync").and_then(Value::as_bool),
        agent_id: s("agentId"),
        agent_type: s("agentType"),
        resolved_model: s("resolvedModel"),
        description: s("description"),
        task_id: s("taskId"),
        task_type: s("taskType"),
        workflow_name: s("workflowName"),
        run_id: s("runId"),
        summary: s("summary"),
        output_file: s("outputFile"),
        scheduled_for: v.get("scheduledFor").and_then(Value::as_f64),
        timeout_ms: v.get("timeoutMs").and_then(Value::as_u64),
        persistent: v.get("persistent").and_then(Value::as_bool),
        resumed_agent_id: s("resumedAgentId"),
    };
    // 只认异步相关回执：普通工具（Read/Edit 等）的 toolUseResult 无这些锚点字段。
    // status=="async_launched" 单独算锚点——未知新物种即便 ID 字段名不认识也能靠它兜底入账
    let anchored = meta.background_task_id.is_some()
        || meta.agent_id.is_some()
        || meta.run_id.is_some()
        || meta.task_id.is_some()
        || meta.scheduled_for.is_some()
        || meta.resumed_agent_id.is_some()
        || meta.status.as_deref() == Some("async_launched");
    anchored.then_some(meta)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub role: Option<String>,
    pub content: MessageContent,
}

/// message.content 可能是纯字符串或内容块数组
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl MessageContent {
    /// 提取第一个文本内容
    #[allow(dead_code)] // 预留给消息内容提取
    pub fn first_text(&self) -> Option<&str> {
        match self {
            MessageContent::Text(s) => Some(s.as_str()),
            MessageContent::Blocks(blocks) => blocks.iter().find_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantRecord {
    pub uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub timestamp: Option<String>,
    pub cwd: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
    #[serde(rename = "isSidechain")]
    pub is_sidechain: Option<bool>,
    #[serde(rename = "userType")]
    pub user_type: Option<String>,
    pub message: Option<AssistantMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub message_type: Option<String>,
    pub role: Option<String>,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub usage: Option<TokenUsage>,
}

/// system 类型记录：api_error（API 报错/重试）、compact_boundary（上下文压缩）等子类型
/// 字段取各 subtype 的并集，全部可缺省；error/compactMetadata 保持 Value 灵活透传
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemRecord {
    pub subtype: Option<String>,
    pub content: Option<String>,
    pub level: Option<String>,
    pub timestamp: Option<String>,
    pub uuid: Option<String>,
    pub error: Option<Value>,
    pub compact_metadata: Option<Value>,
    pub retry_attempt: Option<u32>,
    pub max_retries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTitleRecord {
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(rename = "aiTitle")]
    pub ai_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueOperationRecord {
    pub operation: Option<String>,
    pub timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistorySnapshotRecord {
    #[serde(rename = "messageId")]
    pub message_id: Option<String>,
    #[serde(rename = "isSnapshotUpdate")]
    pub is_snapshot_update: Option<bool>,
    pub snapshot: Option<Value>,
}

/// 手动反序列化 SessionRecord，因为 JSONL 中的 type 字段值与 Rust 枚举变体不一致
impl SessionRecord {
    /// 取所有权版本，避免 Value::clone 开销
    pub fn from_json_owned(value: Value) -> Option<Self> {
        let record_type = value.get("type")?.as_str()?.to_string();
        match record_type.as_str() {
            "user" => {
                let task_notification = value
                    .pointer("/origin/kind")
                    .and_then(Value::as_str)
                    .filter(|kind| *kind == "task-notification")
                    .and_then(|_| value.pointer("/message/content"))
                    .and_then(Value::as_str)
                    .filter(|content| content.contains("<task-notification>"))
                    .map(|content| TaskNotificationRecord {
                        timestamp: value
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(String::from),
                        session_id: value
                            .get("sessionId")
                            .and_then(Value::as_str)
                            .map(String::from),
                        content: content.to_string(),
                    });
                if let Some(notification) = task_notification {
                    return Some(SessionRecord::TaskNotification(notification));
                }
                let async_meta = value.get("toolUseResult").and_then(extract_async_meta);
                let origin_kind = value
                    .pointer("/origin/kind")
                    .and_then(Value::as_str)
                    .map(String::from);
                serde_json::from_value(value).ok().map(|mut u: UserRecord| {
                    u.async_meta = async_meta;
                    u.origin_kind = origin_kind;
                    SessionRecord::User(u)
                })
            }
            "assistant" => serde_json::from_value(value)
                .ok()
                .map(SessionRecord::Assistant),
            "system" => serde_json::from_value(value)
                .ok()
                .map(SessionRecord::System),
            "ai-title" => serde_json::from_value(value)
                .ok()
                .map(SessionRecord::AiTitle),
            "queue-operation" => {
                if let Some(content) = value
                    .get("content")
                    .and_then(Value::as_str)
                    .filter(|content| content.contains("<task-notification>"))
                {
                    return Some(SessionRecord::TaskNotification(TaskNotificationRecord {
                        timestamp: value
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(String::from),
                        session_id: value
                            .get("sessionId")
                            .and_then(Value::as_str)
                            .map(String::from),
                        content: content.to_string(),
                    }));
                }
                serde_json::from_value(value)
                    .ok()
                    .map(SessionRecord::QueueOperation)
            }
            "attachment" => value
                .pointer("/attachment/prompt")
                .and_then(Value::as_str)
                .filter(|content| content.contains("<task-notification>"))
                .map(|content| {
                    SessionRecord::TaskNotification(TaskNotificationRecord {
                        timestamp: value
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(String::from),
                        session_id: value
                            .get("sessionId")
                            .and_then(Value::as_str)
                            .map(String::from),
                        content: content.to_string(),
                    })
                })
                .or_else(|| {
                    Some(SessionRecord::Unknown {
                        raw_type: "attachment".to_string(),
                    })
                }),
            "file-history-snapshot" => serde_json::from_value(value)
                .ok()
                .map(SessionRecord::FileHistorySnapshot),
            other => Some(SessionRecord::Unknown {
                raw_type: other.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod async_meta_tests {
    use super::*;

    fn parse_user(line: &str) -> UserRecord {
        match SessionRecord::from_json_owned(serde_json::from_str(line).unwrap()) {
            Some(SessionRecord::User(u)) => u,
            other => panic!("expected user record, got {:?}", other.map(|_| "non-user")),
        }
    }

    /// 后台 Bash 占位回执：backgroundTaskId 是任务 ID 的唯一可靠来源
    #[test]
    fn extracts_background_bash_placeholder() {
        let u = parse_user(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"Command running in background with ID: b03o4t7yd.","is_error":false}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false,"backgroundTaskId":"b03o4t7yd"}}"#,
        );
        let meta = u.async_meta.expect("should anchor on backgroundTaskId");
        assert_eq!(meta.background_task_id.as_deref(), Some("b03o4t7yd"));
    }

    /// Workflow 启动占位：runId（目录名）与 taskId（通知配对键）都要拿到
    #[test]
    fn extracts_workflow_launch() {
        let u = parse_user(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_02","content":"Workflow launched in background. Task ID: wwqexlaf1","is_error":false}]},"toolUseResult":{"status":"async_launched","taskId":"wwqexlaf1","taskType":"local_workflow","workflowName":"sweep","runId":"wf_51eaa4ea-64a","summary":"调研","transcriptDir":"/x","scriptPath":"/y"}}"#,
        );
        let meta = u.async_meta.unwrap();
        assert_eq!(meta.run_id.as_deref(), Some("wf_51eaa4ea-64a"));
        assert_eq!(meta.task_id.as_deref(), Some("wwqexlaf1"));
        assert_eq!(meta.status.as_deref(), Some("async_launched"));
    }

    /// 旧版终态通知投递体也统一成独立记录，避免进入普通用户消息流。
    #[test]
    fn extracts_origin_kind() {
        let record = SessionRecord::from_json_owned(
            serde_json::from_str(
                r#"{"type":"user","timestamp":"2026-08-02T10:00:00Z","message":{"role":"user","content":"<task-notification>\n<task-id>b03o4t7yd</task-id>\n<status>completed</status>\n</task-notification>"},"origin":{"kind":"task-notification"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        match record {
            SessionRecord::TaskNotification(n) => {
                assert!(n.content.contains("<task-id>b03o4t7yd</task-id>"));
                assert_eq!(n.timestamp.as_deref(), Some("2026-08-02T10:00:00Z"));
            }
            other => panic!("expected task notification, got {other:?}"),
        }
    }

    #[test]
    fn extracts_queue_operation_notification() {
        let record = SessionRecord::from_json_owned(
            serde_json::from_str(
                r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-08-02T10:01:00Z","sessionId":"session-1","content":"<task-notification><task-id>agent-1</task-id><status>completed</status></task-notification>"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        match record {
            SessionRecord::TaskNotification(n) => {
                assert_eq!(n.session_id.as_deref(), Some("session-1"));
                assert!(n.content.contains("<status>completed</status>"));
            }
            other => panic!("expected task notification, got {other:?}"),
        }
    }

    #[test]
    fn extracts_attachment_notification() {
        let record = SessionRecord::from_json_owned(
            serde_json::from_str(
                r#"{"type":"attachment","timestamp":"2026-08-02T10:02:00Z","attachment":{"type":"queued_command","prompt":"<task-notification><task-id>agent-2</task-id><status>failed</status></task-notification>"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        match record {
            SessionRecord::TaskNotification(n) => {
                assert!(n.content.contains("<task-id>agent-2</task-id>"));
            }
            other => panic!("expected task notification, got {other:?}"),
        }
    }

    /// 普通工具回执（Read/Edit 等）不挂 async_meta——锚点字段全缺
    #[test]
    fn ignores_plain_tool_results() {
        let u = parse_user(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_03","content":"file content","is_error":false}]},"toolUseResult":{"stdout":"ok","stderr":"","interrupted":false}}"#,
        );
        assert!(u.async_meta.is_none());
        assert!(u.origin_kind.is_none());
    }

    /// ScheduleWakeup 回执：scheduledFor 精确触发时刻（倒计时数据源）
    #[test]
    fn extracts_wakeup_schedule() {
        let u = parse_user(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_04","content":"Next wakeup scheduled","is_error":false}]},"toolUseResult":{"scheduledFor":1779006960000,"clampedDelaySeconds":90,"wasClamped":false}}"#,
        );
        let meta = u.async_meta.unwrap();
        assert_eq!(meta.scheduled_for, Some(1779006960000.0));
    }
}
