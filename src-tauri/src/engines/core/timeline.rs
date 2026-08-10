use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::{EngineError, EngineErrorKind, EngineResult, ProjectRef, SessionRef};

const SOURCE_METADATA_MAX_KEYS: usize = 32;
const SOURCE_METADATA_MAX_BYTES: usize = 16 * 1024;
const SEGMENT_TEXT_MAX_CHARS: usize = 2_000_000;
const SEGMENT_VALUE_MAX_BYTES: usize = 256 * 1024;

pub fn bounded_segment_text(mut value: String) -> String {
    if let Some((boundary, _)) = value.char_indices().nth(SEGMENT_TEXT_MAX_CHARS) {
        value.truncate(boundary);
        value.push('…');
    }
    value
}

pub fn bounded_segment_value(value: Value) -> Value {
    if serde_json::to_vec(&value)
        .map(|encoded| encoded.len() <= SEGMENT_VALUE_MAX_BYTES)
        .unwrap_or(false)
    {
        value
    } else {
        serde_json::json!({ "truncated": true })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationRole {
    User,
    Assistant,
    System,
    Tool,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReasoningVisibility {
    Visible,
    Summary,
    Redacted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Declined,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePatch {
    pub path: String,
    pub kind: String,
    pub diff: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    pub session: SessionRef,
    pub native_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Segment {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<TextPhase>,
    },
    Reasoning {
        text: String,
        visibility: ReasoningVisibility,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presentation: Option<ToolCallPresentation>,
    },
    ToolResult {
        call_id: String,
        content: Value,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<ToolResultAttachment>,
    },
    CommandExecution {
        id: String,
        command: String,
        cwd: Option<String>,
        output: Option<String>,
        status: ItemStatus,
    },
    FileChange {
        id: String,
        changes: Vec<FilePatch>,
        status: ItemStatus,
    },
    Attachment {
        asset: AssetRef,
        media_type: String,
        title: Option<String>,
    },
    Unknown {
        type_name: String,
        summary: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallPresentation {
    Orchestration,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultAttachment {
    pub asset: AssetRef,
    pub media_type: String,
    pub title: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextPhase {
    Progress,
    Final,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SourceMetadata(BTreeMap<String, Value>);

impl SourceMetadata {
    pub fn new(values: BTreeMap<String, Value>) -> EngineResult<Self> {
        if values.len() > SOURCE_METADATA_MAX_KEYS {
            return Err(EngineError::new(
                EngineErrorKind::Protocol,
                "source metadata contains too many keys",
            ));
        }
        let encoded_len = serde_json::to_vec(&values)
            .map_err(|_| {
                EngineError::new(
                    EngineErrorKind::Protocol,
                    "source metadata cannot be encoded",
                )
            })?
            .len();
        if encoded_len > SOURCE_METADATA_MAX_BYTES {
            return Err(EngineError::new(
                EngineErrorKind::Protocol,
                "source metadata exceeds the size limit",
            ));
        }
        Ok(Self(values))
    }

    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SourceMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeMap::<String, Value>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecord {
    pub id: String,
    pub session: SessionRef,
    pub turn_id: Option<String>,
    pub parent_id: Option<String>,
    pub role: ConversationRole,
    pub timestamp: Option<String>,
    pub segments: Vec<Segment>,
    pub usage: Option<Usage>,
    pub source_meta: SourceMetadata,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreProject {
    pub reference: ProjectRef,
    pub display_name: String,
    pub display_path: Option<String>,
    pub session_count: usize,
    pub last_active: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSessionSummary {
    pub reference: SessionRef,
    pub project: ProjectRef,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub usage: Option<Usage>,
    pub source_meta: SourceMetadata,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPage {
    pub records: Vec<ConversationRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAsset {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_metadata_rejects_unbounded_payloads() {
        let mut values = BTreeMap::new();
        values.insert("large".into(), Value::String("x".repeat(20 * 1024)));

        assert_eq!(
            SourceMetadata::new(values).unwrap_err().kind,
            EngineErrorKind::Protocol
        );
    }

    #[test]
    fn segment_payloads_are_bounded_on_utf8_boundaries() {
        let text = bounded_segment_text("界".repeat(SEGMENT_TEXT_MAX_CHARS + 1));
        assert_eq!(text.chars().count(), SEGMENT_TEXT_MAX_CHARS + 1);
        assert!(text.ends_with('…'));
        assert_eq!(
            bounded_segment_value(serde_json::json!({
                "value": "x".repeat(SEGMENT_VALUE_MAX_BYTES)
            })),
            serde_json::json!({ "truncated": true })
        );
    }
}
