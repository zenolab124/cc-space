use serde::{Deserialize, Serialize};

const BUNDLE_SCHEMA_VERSION: u8 = 1;
const MONET_MCP_PROFILE: &str = "monet_stdio_permission_bridge_v1";

const HTML_VISUAL_PROMPT: &str = r#"当前客户端为 Monet，支持在 Markdown 中渲染内嵌 HTML。请在以下场景主动使用 HTML 增强表达，替代纯 Markdown 的垂直流式输出：

触发场景：
1. 横向对比：方案优劣、参数矩阵、多维对照 → flex 并排卡片
2. 信息卡片：多字段聚合、视觉分组的密集信息 → 带边框 div 分区
3. 折叠内容：长日志、补充细节、非关键信息 → <details>/<summary>
4. 结构图：简单流程、架构关系、时间线 → HTML+CSS 或内嵌 SVG

标签用法：
- 直接用，客户端已有样式：<details>+<summary>、<table>、<mark>、<kbd>、<abbr title="...">
- 布局用内联 style：flex 并排(display:flex;gap:12px)、多列(columns:2)、卡片边框(padding:12px;border:1px solid var(--hv-border);border-radius:6px)
- 对比卡片必须用不同背景区分立场（如暖色 var(--hv-warm) vs 冷色 var(--hv-cool)，或红调 var(--hv-red) vs 绿调 var(--hv-green)），不要用纯白或纯黑底

禁止：<script>、on* 事件属性、<style> 标签、class 属性、完整 HTML 页面框架。这些会被过滤，输出即浪费 token。

原则：Markdown 优先，HTML 穿插增强，每个片段服务于具体表达需求。"#;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionCapabilityId {
    HtmlVisual,
}

impl SessionCapabilityId {
    fn prompt(self) -> &'static str {
        match self {
            Self::HtmlVisual => HTML_VISUAL_PROMPT,
        }
    }
}

#[derive(Debug, Serialize)]
struct Fingerprint<'a> {
    schema_version: u8,
    ids: &'a [SessionCapabilityId],
    append_system_prompt: Option<&'a str>,
    monet_mcp_profile: &'static str,
}

#[derive(Clone, Debug)]
pub struct SessionCapabilityBundle {
    append_system_prompt: Option<String>,
    fingerprint: String,
}

impl SessionCapabilityBundle {
    pub fn new(mut ids: Vec<SessionCapabilityId>) -> Self {
        ids.sort_unstable();
        ids.dedup();

        let prompt = (!ids.is_empty()).then(|| {
            ids.iter()
                .map(|id| id.prompt())
                .collect::<Vec<_>>()
                .join("\n\n")
        });
        let fingerprint = serde_json::to_string(&Fingerprint {
            schema_version: BUNDLE_SCHEMA_VERSION,
            ids: &ids,
            append_system_prompt: prompt.as_deref(),
            monet_mcp_profile: MONET_MCP_PROFILE,
        })
        .expect("session capability fingerprint is serializable");

        Self {
            append_system_prompt: prompt,
            fingerprint,
        }
    }

    pub fn append_system_prompt(&self) -> Option<&str> {
        self.append_system_prompt.as_deref()
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

pub fn needs_restart(current_fingerprint: &str, target: &SessionCapabilityBundle) -> bool {
    current_fingerprint != target.fingerprint()
}

#[cfg(test)]
mod tests {
    use super::{needs_restart, SessionCapabilityBundle, SessionCapabilityId};

    #[test]
    fn normalizes_order_and_duplicates() {
        let first = SessionCapabilityBundle::new(vec![
            SessionCapabilityId::HtmlVisual,
            SessionCapabilityId::HtmlVisual,
        ]);
        let second = SessionCapabilityBundle::new(vec![SessionCapabilityId::HtmlVisual]);

        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.append_system_prompt(), second.append_system_prompt());
    }

    #[test]
    fn empty_bundle_has_no_prompt_and_differs_from_html_visual() {
        let empty = SessionCapabilityBundle::new(vec![]);
        let html = SessionCapabilityBundle::new(vec![SessionCapabilityId::HtmlVisual]);

        assert_eq!(empty.append_system_prompt(), None);
        assert!(html.append_system_prompt().is_some());
        assert_ne!(empty.fingerprint(), html.fingerprint());
    }

    #[test]
    fn fingerprint_contains_only_static_runtime_identity() {
        let bundle = SessionCapabilityBundle::new(vec![SessionCapabilityId::HtmlVisual]);

        assert!(bundle
            .fingerprint()
            .contains("monet_stdio_permission_bridge_v1"));
        assert!(!bundle.fingerprint().contains("MONET_PERMISSION_ADDR"));
        assert!(!bundle.fingerprint().contains("MONET_PERMISSION_TOKEN"));
        assert!(!bundle.fingerprint().contains("127.0.0.1"));
    }

    #[test]
    fn restart_depends_on_fingerprint_equality() {
        let empty = SessionCapabilityBundle::new(vec![]);
        let html = SessionCapabilityBundle::new(vec![SessionCapabilityId::HtmlVisual]);

        assert!(!needs_restart(empty.fingerprint(), &empty));
        assert!(needs_restart(empty.fingerprint(), &html));
    }
}
