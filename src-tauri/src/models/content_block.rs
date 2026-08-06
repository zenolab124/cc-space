use serde::{Deserialize, Serialize};
use serde_json::Value;

fn empty_tool_input() -> Value {
    Value::Object(Default::default())
}

/// 消息内容块，对应 JSONL 中 message.content 数组元素
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        /// Anthropic redacted thinking 的加密签名,thinking 为空但 signature 存在时表示"已加密"。
        /// 流式期间需借此与"思考中"(尚未拿到 delta)区分。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        /// Anthropic 的流式 `content_block_start` 有时先只发 id/name，
        /// 参数在后续 input_json_delta 才到；首帧必须先能进入前端流。
        #[serde(default = "empty_tool_input")]
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },
    Image {
        // default：source 整个缺失的畸形块也必须进本变体（见 ImageSource 注释的计数口径）
        #[serde(default)]
        source: ImageSource,
    },
    /// PDF 等文档附件。source.data 的完整 base64 不在字段中声明，
    /// serde 反序列化时直接丢弃——防止大 payload 落进 Unknown(Value) 穿透 IPC
    Document {
        source: DocumentSource,
        #[serde(default)]
        title: Option<String>,
    },
    #[serde(untagged)]
    Unknown(Value),
}

/// tool_result 的 content 字段可能是字符串或内容块数组
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// document 块的来源描述，刻意不声明 data 字段（base64 原文不进内存/IPC）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
}

/// image 块的来源描述。学 DocumentSource 的做法：base64 原文（source.data）
/// 刻意不声明为字段——serde 反序列化时直接丢弃，避免几百 KB 的 base64 落进内存/穿透 IPC。
/// 历史区图片改由 ccimg:// 自定义协议按需取，前端凭 img_index 拼 URL。
/// 全字段 default：任何 `type=="image"` 的块（含缺 media_type / 缺 source 的畸形块）
/// 都必须进 Image 变体——与协议 handler raw 遍历「只看 type」的计数口径严格一致
/// （否则 img_index 序号两侧错位），同时防畸形块落 Unknown(Value) 携 base64 穿透 IPC。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type", default)]
    pub source_type: String,
    #[serde(default)]
    pub media_type: String,
    /// record 内第 N 个 image block（0 起，深度优先遍历序，见 parser::inject_image_indices）。
    /// 由 Rust 解析时注入，前端直接读取零计算，用于拼 ccimg:// 协议 URL 的 img_index 段。
    #[serde(default)]
    pub img_index: u32,
}
