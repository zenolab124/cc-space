//! Routine 数据结构单一事实源。
//!
//! routines.json 有三个写者：主 App（routines.rs）、MCP server（monet-mcp）、
//! runner（monet-routine-runner），后两者会整文件重写——任何一方用缺字段的
//! 本地副本序列化，都会把其他方写入的字段抹掉。因此结构定义必须共享：
//! 本文件同时被 app_lib（mod routine_types）和两个 bin（#[path] mod）编译，
//! 只允许依赖 std / serde，禁止 use crate::* 引用宿主 crate 的其他模块。

use serde::{Deserialize, Serialize};

/// 任务来源（谁创建的）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineSource {
    /// 创建入口：ui | mcp
    pub kind: String,
    /// 发起会话的项目路径（MCP 场景：server 继承 claude CLI 的 cwd）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// MCP 客户端标识（initialize 握手的 clientInfo，如 claude-code 2.1.187）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

impl RoutineSource {
    pub fn ui() -> Self {
        Self {
            kind: "ui".to_string(),
            project: None,
            client: None,
        }
    }

    pub fn mcp(project: Option<String>, client: Option<String>) -> Self {
        Self {
            kind: "mcp".to_string(),
            project,
            client,
        }
    }
}

/// Routine 执行引擎。结构与 EngineInstanceId 的持久化形状一致，但本文件需被
/// 独立 runner/MCP 直接编译，因此保留 std + serde 的轻量契约。
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineEngine {
    pub engine_id: String,
    pub instance_id: String,
}

impl RoutineEngine {
    pub fn claude_code() -> Self {
        Self {
            engine_id: "claude-code".to_string(),
            instance_id: "default".to_string(),
        }
    }

    pub fn codex() -> Self {
        Self {
            engine_id: "codex".to_string(),
            instance_id: "default".to_string(),
        }
    }

    pub fn is_claude_code(&self) -> bool {
        self.engine_id == "claude-code" && self.instance_id == "default"
    }

    pub fn is_codex(&self) -> bool {
        self.engine_id == "codex" && self.instance_id == "default"
    }
}

impl Default for RoutineEngine {
    fn default() -> Self {
        Self::claude_code()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineDefinition {
    pub id: String,
    pub name: String,
    pub cron_expression: String,
    pub original_text: String,
    pub prompt: String,
    /// 执行该任务的默认引擎；旧任务未存此字段时保持 Claude Code 行为。
    #[serde(default)]
    pub engine: RoutineEngine,
    pub enabled: bool,
    pub created_at: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    /// 任务来源；旧数据无此字段，反序列化为 None（UI 显示「未知」）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RoutineSource>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_routine_defaults_to_claude_code() {
        let routine: RoutineDefinition = serde_json::from_str(
            r#"{
                "id":"legacy",
                "name":"Legacy",
                "cronExpression":"0 9 * * *",
                "originalText":"",
                "prompt":"Summarize",
                "enabled":true,
                "createdAt":"2026-08-10T00:00:00Z",
                "lastRun":null,
                "nextRun":null
            }"#,
        )
        .unwrap();

        assert_eq!(routine.engine, RoutineEngine::claude_code());
    }

    #[test]
    fn routine_engine_round_trips_as_engine_instance_shape() {
        let value = serde_json::to_value(RoutineEngine::codex()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "engineId": "codex", "instanceId": "default" })
        );
        assert_eq!(
            serde_json::from_value::<RoutineEngine>(value).unwrap(),
            RoutineEngine::codex()
        );
    }
}
