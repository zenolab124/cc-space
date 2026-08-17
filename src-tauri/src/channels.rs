//! 多渠道(profile)配置域:`~/.monet/`
//!
//! - `settings.json`        应用设置:默认会话/Agent 渠道 + 渠道展示元数据
//! - `channels/<id>.json`   Monet 渠道配置；`_ccSpace.connection` 存共享连接，
//!   Claude/Codex 适配器可按需覆盖，实际 API 地址在运行时统一解析
//! - `runtime/<sid>-<ns>.json` per-spawn 合成产物(渠道内容 + 连接凭据 + 防御空值 + 会话覆盖),
//!   进程结束即删,应用启动兜底清空
//!
//! 红线:authToken 等敏感值不回传前端(list 仅给掩码)、不进 argv(经 --settings 文件
//! 路径 + spawn env 注入)。所有读取用时重读,不做进程级缓存(同 settings.json 活文件教训)。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::config;
use crate::proc_ext::HideConsole;

/// 保留 id:语义为「跟随 CLI/零注入」。参与链排序,不对应 channels/ 下的文件
pub const OFFICIAL_ID: &str = "official";

/// 保留 id:语义为「官方直连」——合成压制 settings 挤掉 CLI 配置里的第三方
/// 认证/路由键,强制走 Anthropic 官方端点 + OAuth 登录态。无渠道文件。
/// 与 OFFICIAL_ID(零注入直通,CLI 配置什么样就什么样)语义互补。
pub const OFFICIAL_DIRECT_ID: &str = "official-direct";

/// 官方直连的强制端点(实测:空串 token 被 CLI 视为未设,回落 OAuth)
const OFFICIAL_BASE_URL: &str = "https://api.anthropic.com";

/// Apple Foundation Models 虚拟渠道 id
pub const APPLE_FM_ID: &str = "apple-fm";
const APPLE_FM_PORT: u16 = 39175;

fn is_builtin_channel(id: &str) -> bool {
    id == OFFICIAL_ID || id == OFFICIAL_DIRECT_ID || id == APPLE_FM_ID
}

/// 注入渠道时无条件压制的认证/路由残留键
pub const DEFENSE_ENV_KEYS: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// Monet 托管的「模型角色映射」env 键(共 21 个),存于 channels/<id>.json 顶层 env 块。
/// 四角色 FABLE/OPUS/SONNET/HAIKU 各 4 键(重定向落点/显示名/描述/能力)+ 自定义第五槽 4 键 + 兜底 1 键。
/// save_channel(model_env=Some) 时整命名空间替换:先移除全部 21 键再写入非空值;
/// ChannelView.model_env 回传这些键的当前值(模型 ID 非敏感,明文回传)。
pub const MODEL_ENV_KEYS: &[&str] = &[
    // FABLE
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_SUPPORTED_CAPABILITIES",
    // OPUS
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_SUPPORTED_CAPABILITIES",
    // SONNET
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_SUPPORTED_CAPABILITIES",
    // HAIKU
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_SUPPORTED_CAPABILITIES",
    // 自定义第五槽
    "ANTHROPIC_CUSTOM_MODEL_OPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES",
    // 兜底
    "ANTHROPIC_MODEL",
];

/// UI 实际管理的模型映射键子集(角色 _MODEL/_NAME + 自定义槽 + 默认模型)。
/// save_channel 的替换语义只作用于这些键——_DESCRIPTION/_CAPABILITIES 等
/// v1 无 UI 的手编键在重新保存映射时原样保留,不被整命名空间替换吞掉
pub const UI_MANAGED_MODEL_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_CUSTOM_MODEL_OPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
    "ANTHROPIC_MODEL",
];

fn data_dir() -> PathBuf {
    config::data_dir().to_path_buf()
}

fn channels_dir() -> PathBuf {
    data_dir().join("channels")
}

fn runtime_dir() -> PathBuf {
    data_dir().join("runtime")
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn channel_file_path(id: &str) -> PathBuf {
    channels_dir().join(format!("{}.json", id))
}

pub fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 {
        return Err("渠道 ID 须为 1-64 个字符".to_string());
    }
    if id == OFFICIAL_ID || id == OFFICIAL_DIRECT_ID {
        return Err(format!("{} 为保留 ID", id));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("渠道 ID 仅允许字母、数字、- 和 _".to_string());
    }
    Ok(())
}

// ---- 应用设置(settings.json) ----

/// 渠道文件(channels/<id>.json)扩展元数据。持久化键名固定为 `_ccSpace`——
/// 存量渠道文件的既定磁盘格式,读取兼容不可改(改键名会丢已存渠道的模型清单)。
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ChannelExt {
    /// 共享连接。旧渠道缺省时从 Claude env / Codex 配置兼容读取。
    pub connection: Option<ChannelConnectionExt>,
    /// Claude Code 固定使用 Anthropic Messages；这里只保存可选连接覆盖。
    pub claude: Option<EngineConnectionExt>,
    pub available_models: Vec<String>,
    pub agent_model: Option<String>,
    /// None 表示旧渠道，按兼容规则仅支持 Claude Code。
    pub engine_support: Option<Vec<String>>,
    pub codex: Option<CodexChannelExt>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ChannelConnectionExt {
    pub base_url: String,
    /// bearer:共享 API Key；none:本地或无需认证的网关。
    pub auth_mode: String,
    pub auth_token: Option<String>,
}

/// 引擎适配器连接覆盖。空字段表示继承 `_ccSpace.connection`。
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineConnectionExt {
    pub base_url: Option<String>,
    pub auth_mode: Option<String>,
    pub auth_token: Option<String>,
    pub resolved_connection: Option<ResolvedConnectionExt>,
    /// 新版渠道启用 URL 规则补全；旧渠道缺省为 false，保持原地址不变。
    pub auto_resolve: bool,
}

/// 自动探测得到的派生地址缓存。只有 source_base_url 与当前适配器输入完全一致时才使用，
/// 避免用户手改渠道文件后继续命中旧解析结果。
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ResolvedConnectionExt {
    pub source_base_url: String,
    pub base_url: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct CodexChannelExt {
    /// external:引用用户 Codex 配置中的 provider；managed:由 Monet 注入 provider 定义。
    pub mode: String,
    pub provider_id: String,
    pub base_url: Option<String>,
    /// bearer:渠道令牌；openai:复用 Codex 登录；none:无认证。
    pub auth_mode: String,
    pub auth_token: Option<String>,
    pub default_model: Option<String>,
    pub default_effort: Option<String>,
    pub available_models: Vec<String>,
    pub resolved_connection: Option<ResolvedConnectionExt>,
    /// 新版托管 Provider 启用 URL 规则补全；旧配置缺省为 false。
    pub auto_resolve: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChannelAdapter {
    Claude,
    Codex,
    OpenAiChat,
}

impl ChannelAdapter {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "claude-code" | "anthropic" => Some(Self::Claude),
            "codex" | "responses" => Some(Self::Codex),
            "openai" | "openai-chat" => Some(Self::OpenAiChat),
            _ => None,
        }
    }
}

impl ChannelExt {
    fn supports_engine(&self, engine_id: &str) -> bool {
        self.engine_support.as_ref().map_or(engine_id == "claude-code", |engines| {
            engines.iter().any(|engine| engine == engine_id)
        })
    }
}

pub(crate) fn read_channel_ext(id: &str) -> Option<ChannelExt> {
    let text = fs::read_to_string(channel_file_path(id)).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    root.get("_ccSpace")
        .and_then(|v| serde_json::from_value::<ChannelExt>(v.clone()).ok())
}

fn channel_supports_engine(id: &str, engine_id: &str) -> bool {
    match id {
        OFFICIAL_ID => matches!(engine_id, "claude-code" | "codex"),
        OFFICIAL_DIRECT_ID => engine_id == "claude-code",
        APPLE_FM_ID => false,
        _ => read_channel_ext(id).is_some_and(|extension| extension.supports_engine(engine_id)),
    }
}

fn normalize_base_url(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    (!value.is_empty()).then(|| value.to_string())
}

fn url_path_ends_with(value: &str, suffix: &str) -> bool {
    reqwest::Url::parse(value).ok().is_some_and(|url| {
        url.path().trim_end_matches('/').to_ascii_lowercase().ends_with(suffix)
    })
}

fn replace_url_path(value: &str, transform: impl FnOnce(&str) -> String) -> Option<String> {
    let mut url = reqwest::Url::parse(value).ok()?;
    let path = transform(url.path().trim_end_matches('/'));
    url.set_path(if path.is_empty() { "/" } else { &path });
    let rendered = url.to_string();
    Some(if url.path() == "/" && url.query().is_none() && url.fragment().is_none() {
        rendered.trim_end_matches('/').to_string()
    } else {
        rendered
    })
}

fn append_url_path(value: &str, suffix: &str) -> Option<String> {
    replace_url_path(value, |path| format!("{}{}", path, suffix))
}

fn strip_url_path_suffix(value: &str, suffix: &str) -> Option<String> {
    replace_url_path(value, |path| {
        let keep = path.len().saturating_sub(suffix.len());
        path[..keep].trim_end_matches('/').to_string()
    })
}

/// 适配器接收的是 endpoint 前缀，不是完整 endpoint：
/// Claude 在 base 后请求 `/v1/messages`；Codex 在 base 后请求 `/responses`。
/// 候选顺序代表无探测结果时的安全默认，明确 override 也保留同样的容错探测。
fn adapter_base_url_candidates(value: &str, adapter: ChannelAdapter) -> Vec<String> {
    let Some(mut base) = normalize_base_url(value) else { return vec![] };
    match adapter {
        ChannelAdapter::Claude => {
            if url_path_ends_with(&base, "/v1/messages") {
                if let Some(stripped) = strip_url_path_suffix(&base, "/v1/messages") {
                    base = stripped;
                }
            }
            let mut candidates = Vec::new();
            if url_path_ends_with(&base, "/v1") {
                if let Some(stripped) = strip_url_path_suffix(&base, "/v1") {
                    candidates.push(stripped);
                }
            }
            if !candidates.contains(&base) {
                candidates.push(base);
            }
            candidates
        }
        ChannelAdapter::Codex => {
            if url_path_ends_with(&base, "/responses") {
                if let Some(stripped) = strip_url_path_suffix(&base, "/responses") {
                    base = stripped;
                }
            }
            let mut candidates = if url_path_ends_with(&base, "/v1") {
                vec![base.clone(), strip_url_path_suffix(&base, "/v1").unwrap_or(base)]
            } else {
                vec![append_url_path(&base, "/v1").unwrap_or_else(|| format!("{base}/v1")), base]
            };
            candidates.dedup();
            candidates
        }
        ChannelAdapter::OpenAiChat => vec![base],
    }
}

fn adapter_endpoint_url(base_url: &str, adapter: ChannelAdapter) -> String {
    let suffix = match adapter {
        ChannelAdapter::Claude => "/v1/messages",
        ChannelAdapter::Codex => "/responses",
        ChannelAdapter::OpenAiChat => "/v1/chat/completions",
    };
    append_url_path(base_url, suffix).unwrap_or_else(|| {
        format!("{}{}", base_url.trim_end_matches('/'), suffix)
    })
}

fn adapter_models_url(base_url: &str, adapter: ChannelAdapter) -> String {
    let suffix = if adapter == ChannelAdapter::Codex { "/models" } else { "/v1/models" };
    append_url_path(base_url, suffix).unwrap_or_else(|| {
        format!("{}{}", base_url.trim_end_matches('/'), suffix)
    })
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ChannelMeta {
    pub name: Option<String>,
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub protocol: Option<String>,
    pub scope: Option<String>,
    pub agent_model: Option<String>,
    /// 渠道默认模型/思考强度——仅 official 用此存储(无渠道文件可写);
    /// 第三方渠道的默认存渠道文件本身(env.ANTHROPIC_MODEL / 顶层 effortLevel),
    /// 终端 `claude --settings <渠道文件>` 可复用同一默认
    pub default_model: Option<String>,
    pub default_effort: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ChannelMeta {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
    pub fn protocol(&self) -> &str {
        self.protocol.as_deref().unwrap_or("anthropic")
    }
    pub fn scope(&self) -> &str {
        self.scope.as_deref().unwrap_or("full")
    }
    #[allow(dead_code)] // 预留给渠道过滤逻辑
    pub fn is_agent_only(&self) -> bool {
        self.scope() == "agent-only"
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentFeaturePrefs {
    pub preferred_channel: Option<String>,
    pub preferred_model: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    #[serde(skip_serializing)]
    pub default_channel_id: Option<String>,
    #[serde(skip_serializing)]
    pub session_chain: Vec<String>,
    #[serde(skip_serializing)]
    pub agent_chain: Vec<String>,
    /// 旧版单一会话默认渠道。只读用于迁移，不再写回 settings.json。
    #[serde(skip_serializing)]
    pub default_session_channel: Option<String>,
    /// 按引擎保存会话默认渠道；缺省值表示使用该引擎的官方配置。
    pub default_session_channels: BTreeMap<String, Option<String>>,
    /// 按引擎保存会话默认模型/思考强度；键存在且值为 null 表示显式跟随引擎默认。
    pub default_session_models: BTreeMap<String, Option<String>>,
    pub default_session_efforts: BTreeMap<String, Option<String>>,
    /// 旧版智能增强单引擎默认值。只读用于迁移，不再写回 settings.json。
    #[serde(skip_serializing)]
    pub default_agent_channel: Option<String>,
    #[serde(skip_serializing)]
    pub default_agent_model: Option<String>,
    #[serde(skip_serializing)]
    pub default_agent_effort: Option<String>,
    /// 智能增强当前使用的引擎；旧设置缺省时迁移为 Claude Code。
    pub default_agent_engine: Option<String>,
    /// 智能增强默认连接、模型和思考强度按引擎分槽，切换引擎不覆盖另一套配置。
    pub default_agent_channels: BTreeMap<String, Option<String>>,
    pub default_agent_models: BTreeMap<String, Option<String>>,
    pub default_agent_efforts: BTreeMap<String, Option<String>>,
    pub channels: BTreeMap<String, ChannelMeta>,
    pub agent_toggles: BTreeMap<String, bool>,
    /// 旧版单层功能偏好。只读迁移到 Claude Code 分槽。
    #[serde(skip_serializing)]
    pub agent_preferences: BTreeMap<String, AgentFeaturePrefs>,
    pub agent_preferences_by_engine: BTreeMap<String, BTreeMap<String, AgentFeaturePrefs>>,
    /// 内置 Agent 走官方 CLI 时是否保留会话落盘（可追溯）。None = 默认落盘
    pub agent_session_persist: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn is_channel_enabled(settings: &AppSettings, id: &str) -> bool {
    is_builtin_channel(id)
        || settings
            .channels
            .get(id)
            .map_or(true, ChannelMeta::is_enabled)
}

fn clear_channel_references(settings: &mut AppSettings, id: &str) -> bool {
    let mut changed = false;
    if settings.default_session_channel.as_deref() == Some(id) {
        settings.default_session_channel = None;
        changed = true;
    }
    let affected_engines: Vec<String> = settings.default_session_channels.iter()
        .filter(|(_, channel)| channel.as_deref() == Some(id))
        .map(|(engine, _)| engine.clone())
        .collect();
    for engine in affected_engines {
        settings.default_session_channels.insert(engine.clone(), None);
        settings.default_session_models.insert(engine.clone(), None);
        settings.default_session_efforts.insert(engine, None);
        changed = true;
    }
    let affected_agent_engines: Vec<String> = settings.default_agent_channels.iter()
        .filter(|(_, channel)| channel.as_deref() == Some(id))
        .map(|(engine, _)| engine.clone())
        .collect();
    for engine in affected_agent_engines {
        settings.default_agent_channels.insert(engine.clone(), None);
        settings.default_agent_models.insert(engine.clone(), None);
        settings.default_agent_efforts.insert(engine, None);
        changed = true;
    }
    for preferences in settings.agent_preferences_by_engine.values_mut() {
        for prefs in preferences.values_mut() {
            if prefs.preferred_channel.as_deref() == Some(id) {
                prefs.preferred_channel = None;
                prefs.preferred_model = None;
                changed = true;
            }
        }
    }
    changed
}

pub(crate) fn load_app_settings() -> AppSettings {
    let mut settings: AppSettings = fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let mut migrated = false;

    // 迁移：旧 defaultChannelId → Claude Code 的会话默认渠道
    if let Some(old_id) = settings.default_channel_id.take() {
        if old_id != OFFICIAL_ID && settings.default_session_channel.is_none() {
            settings.default_session_channel = Some(old_id);
        }
        migrated = true;
    }

    // 迁移：旧 session_chain → Claude Code 的会话默认渠道
    if !settings.session_chain.is_empty() {
        if settings.default_session_channel.is_none() {
            if let Some(first) = settings.session_chain.iter().find(|id| *id != OFFICIAL_ID) {
                settings.default_session_channel = Some(first.clone());
            }
        }
        if settings.default_agent_channel.is_none() {
            if let Some(first) = settings.agent_chain.iter().find(|id| *id != OFFICIAL_ID) {
                settings.default_agent_channel = Some(first.clone());
            }
        }
        settings.session_chain.clear();
        settings.agent_chain.clear();
        migrated = true;
    }

    // 迁移现有单一默认值；Codex 没有历史默认值时保持官方配置。
    if !settings.default_session_channels.contains_key("claude-code") {
        if let Some(channel) = settings.default_session_channel.clone() {
            settings
                .default_session_channels
                .insert("claude-code".to_string(), Some(channel));
            migrated = true;
        }
    }

    // 迁移智能增强：旧版只有 Claude Code 一套默认值和功能偏好。
    let agent_engine = settings
        .default_agent_engine
        .as_deref()
        .filter(|engine| matches!(*engine, "claude-code" | "codex"))
        .unwrap_or("claude-code")
        .to_string();
    if settings.default_agent_engine.as_deref() != Some(agent_engine.as_str()) {
        settings.default_agent_engine = Some(agent_engine);
        migrated = true;
    }
    if !settings.default_agent_channels.contains_key("claude-code") {
        if let Some(channel) = settings.default_agent_channel.clone() {
            settings.default_agent_channels.insert("claude-code".into(), Some(channel));
            migrated = true;
        }
    }
    if !settings.default_agent_models.contains_key("claude-code")
        && settings.default_agent_model.is_some()
    {
        settings.default_agent_models.insert("claude-code".into(), settings.default_agent_model.clone());
        migrated = true;
    }
    if !settings.default_agent_efforts.contains_key("claude-code")
        && settings.default_agent_effort.is_some()
    {
        settings.default_agent_efforts.insert("claude-code".into(), settings.default_agent_effort.clone());
        migrated = true;
    }
    if !settings.agent_preferences.is_empty()
        && !settings.agent_preferences_by_engine.contains_key("claude-code")
    {
        settings.agent_preferences_by_engine.insert(
            "claude-code".into(),
            settings.agent_preferences.clone(),
        );
        migrated = true;
    }

    // 内置渠道由运行能力决定，不参与普通渠道的启停生命周期。
    for id in [OFFICIAL_ID, OFFICIAL_DIRECT_ID, APPLE_FM_ID] {
        if settings
            .channels
            .get_mut(id)
            .is_some_and(|meta| meta.enabled.take().is_some())
        {
            migrated = true;
        }
    }

    // 兼容旧版本留下的矛盾状态：禁用的第三方不能继续作为任何默认项或功能偏好。
    let disabled_ids: Vec<String> = settings
        .channels
        .iter()
        .filter(|(id, meta)| !is_builtin_channel(id) && !meta.is_enabled())
        .map(|(id, _)| id.clone())
        .collect();
    for id in disabled_ids {
        migrated |= clear_channel_references(&mut settings, &id);
    }

    for preferences in settings.agent_preferences_by_engine.values_mut() {
        for prefs in preferences.values_mut() {
            if prefs.preferred_channel.is_none() && prefs.preferred_model.take().is_some() {
                migrated = true;
            }
        }
    }

    if migrated {
        let _ = save_app_settings(&settings);
    }

    settings
}

fn scan_channel_ids() -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(entries) = fs::read_dir(channels_dir()) {
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for path in files {
            if let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(String::from) {
                if validate_id(&id).is_ok() {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

fn save_app_settings(settings: &AppSettings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    crate::config::atomic_write(&settings_path(), &text).map_err(|e| e.to_string())
}

fn write_json_0600(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() >= 12 {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{}…{}", head, tail)
    } else {
        "•••".to_string()
    }
}

// ---- 渠道解析(内部 API) ----

pub struct AgentChannelCredentials {
    pub id: String,
    pub is_official: bool,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub protocol: String,
    pub agent_model: Option<String>,
    pub agent_effort: Option<String>,
}

pub struct AgentChannelResolveError {
    pub channel_id: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct AgentRuntimeSelection {
    pub engine_id: String,
    pub channel_id: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

fn configured_agent_engine(settings: &AppSettings) -> &str {
    settings
        .default_agent_engine
        .as_deref()
        .filter(|engine| matches!(*engine, "claude-code" | "codex"))
        .unwrap_or("claude-code")
}

pub fn default_agent_engine() -> String {
    configured_agent_engine(&load_app_settings()).to_string()
}

pub fn resolve_agent_runtime_selection(
    key: &str,
) -> Result<AgentRuntimeSelection, AgentChannelResolveError> {
    let settings = load_app_settings();
    let engine_id = configured_agent_engine(&settings).to_string();
    let preferences = settings
        .agent_preferences_by_engine
        .get(&engine_id)
        .and_then(|features| features.get(key));
    let channel_id = preferences
        .and_then(|prefs| prefs.preferred_channel.clone())
        .or_else(|| settings.default_agent_channels.get(&engine_id).cloned().flatten())
        .unwrap_or_else(|| OFFICIAL_ID.to_string());
    let model = preferences
        .and_then(|prefs| prefs.preferred_model.clone())
        .or_else(|| settings.default_agent_models.get(&engine_id).cloned().flatten());
    let effort = settings.default_agent_efforts.get(&engine_id).cloned().flatten();

    if !is_channel_enabled(&settings, &channel_id) {
        return Err(AgentChannelResolveError {
            channel_id,
            message: "渠道已禁用".to_string(),
        });
    }
    if engine_id == "codex" && !channel_supports_engine(&channel_id, "codex") {
        return Err(AgentChannelResolveError {
            channel_id,
            message: "该渠道未启用 Codex".to_string(),
        });
    }
    Ok(AgentRuntimeSelection { engine_id, channel_id, model, effort })
}

pub fn resolve_agent_for_feature_logged(
    key: &str,
) -> Result<Option<AgentChannelCredentials>, AgentChannelResolveError> {
    let selection = resolve_agent_runtime_selection(key)?;
    if selection.engine_id != "claude-code" {
        return Ok(None);
    }
    let settings = load_app_settings();
    resolve_channel_credentials_checked(&selection.channel_id, &settings, selection.model)
        .map(|mut credentials| {
            credentials.agent_effort = selection.effort;
            credentials
        })
        .map(Some)
}

fn resolve_channel_credentials_checked(
    channel_id: &str,
    settings: &AppSettings,
    model: Option<String>,
) -> Result<AgentChannelCredentials, AgentChannelResolveError> {
    if !is_channel_enabled(settings, channel_id) {
        return Err(AgentChannelResolveError {
            channel_id: channel_id.to_string(),
            message: "渠道已禁用".to_string(),
        });
    }

    resolve_channel_credentials(channel_id, settings, model).ok_or_else(|| {
        AgentChannelResolveError {
            channel_id: channel_id.to_string(),
            message: "渠道配置或凭据不可用".to_string(),
        }
    })
}

/// Resolve single agent credentials from default settings
pub fn resolve_agent_credentials() -> Option<AgentChannelCredentials> {
    let settings = load_app_settings();
    if configured_agent_engine(&settings) != "claude-code" {
        return None;
    }
    let channel_id = settings.default_agent_channels
        .get("claude-code").and_then(Option::as_deref).unwrap_or(OFFICIAL_ID);
    let model = settings.default_agent_models.get("claude-code").cloned().flatten();
    resolve_channel_credentials(channel_id, &settings, model).map(|mut credentials| {
        credentials.agent_effort = settings.default_agent_efforts.get("claude-code").cloned().flatten();
        credentials
    })
}

/// Resolve agent credentials for a specific feature, with fallback to default
pub fn resolve_agent_for_feature(key: &str) -> Option<AgentChannelCredentials> {
    let selection = resolve_agent_runtime_selection(key).ok()?;
    if selection.engine_id != "claude-code" {
        return None;
    }
    let settings = load_app_settings();
    resolve_channel_credentials(&selection.channel_id, &settings, selection.model).map(|mut credentials| {
        credentials.agent_effort = selection.effort;
        credentials
    })
}

fn resolve_channel_credentials(channel_id: &str, settings: &AppSettings, model_override: Option<String>) -> Option<AgentChannelCredentials> {
    if !is_channel_enabled(settings, channel_id) { return None; }
    let meta = settings.channels.get(channel_id);

    if channel_id == OFFICIAL_ID {
        return Some(AgentChannelCredentials {
            id: OFFICIAL_ID.to_string(), is_official: true,
            base_url: None, token: None,
            protocol: "anthropic".to_string(),
            agent_model: None,
            agent_effort: None,
        });
    }
    // 官方直连:同走官方 CLI 直调,spawn 时额外注入压制 settings(见 agent.rs)
    if channel_id == OFFICIAL_DIRECT_ID {
        return Some(AgentChannelCredentials {
            id: OFFICIAL_DIRECT_ID.to_string(), is_official: true,
            base_url: None, token: None,
            protocol: "anthropic".to_string(),
            agent_model: None,
            agent_effort: None,
        });
    }
    if channel_id == APPLE_FM_ID {
        let agent_model = model_override.or_else(|| meta.and_then(|m| m.agent_model.clone()));
        return Some(AgentChannelCredentials {
            id: APPLE_FM_ID.to_string(), is_official: false,
            base_url: Some(format!("http://localhost:{}", APPLE_FM_PORT)),
            token: Some(String::new()),
            protocol: "openai".to_string(),
            agent_model,
            agent_effort: None,
        });
    }
    // 协议选择已从新渠道移除；仅为尚未保存迁移的历史 OpenAI 直调渠道保留旧行为。
    let legacy_openai = meta.and_then(|value| value.protocol.as_deref()) == Some("openai");
    let adapter = if legacy_openai { ChannelAdapter::OpenAiChat } else { ChannelAdapter::Claude };
    let connection = read_channel_connection(channel_id, adapter)?;
    let agent_model = model_override
        .or_else(|| read_channel_ext(channel_id).and_then(|e| e.agent_model))
        .or_else(|| fallback_agent_model(channel_id));
    Some(AgentChannelCredentials {
        id: channel_id.to_string(), is_official: false,
        base_url: Some(connection.base_url), token: Some(connection.token),
        protocol: if legacy_openai { "openai" } else { "anthropic" }.to_string(), agent_model,
        agent_effort: None,
    })
}

/// 渠道未显式配置 agent 模型时的兜底:并非所有第三方渠道都提供 Haiku,
/// 写死 Haiku ID 会在这类渠道上 404。优先渠道默认模型(env.ANTHROPIC_MODEL,
/// 用户已验证可用),次选模型清单中的轻量款(含 haiku 名者),再次清单首项;
/// 全无线索则返回 None,由调用方决定最终写死值
pub(crate) fn fallback_agent_model(channel_id: &str) -> Option<String> {
    let path = channel_file_path(channel_id);
    let parsed: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    if let Some(m) = parsed
        .get("env")
        .and_then(|e| e.get("ANTHROPIC_MODEL"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(m.to_string());
    }
    let models = read_channel_ext(channel_id)?.available_models;
    models
        .iter()
        .find(|m| m.to_lowercase().contains("haiku"))
        .or_else(|| models.first())
        .cloned()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChannelConnectionCredentials {
    base_url: String,
    auth_mode: String,
    token: String,
}

fn read_channel_connection(id: &str, adapter: ChannelAdapter) -> Option<ChannelConnectionCredentials> {
    let text = fs::read_to_string(channel_file_path(id)).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    channel_connection_from_root(&root, adapter)
}

fn legacy_channel_token_from_root(root: &Value, prefer_openai: bool) -> Option<String> {
    let env = root.get("env").and_then(Value::as_object);
    let anthropic = || env.and_then(|values| {
        values.get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| values.get("ANTHROPIC_API_KEY"))
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(String::from)
    });
    let openai = || env.and_then(|values| {
        values.get("OPENAI_API_KEY")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(String::from)
    });
    let env_token = if prefer_openai {
        openai().or_else(anthropic)
    } else {
        anthropic().or_else(openai)
    };
    env_token.or_else(|| {
        root.get("_ccSpace")
            .and_then(|value| serde_json::from_value::<ChannelExt>(value.clone()).ok())
            .and_then(|extension| extension.codex)
            .and_then(|codex| codex.auth_token)
            .filter(|token| !token.is_empty())
    })
}

fn read_legacy_channel_token(id: &str, prefer_openai: bool) -> Option<String> {
    let text = fs::read_to_string(channel_file_path(id)).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    legacy_channel_token_from_root(&root, prefer_openai)
}

fn channel_connection_from_root(root: &Value, adapter: ChannelAdapter) -> Option<ChannelConnectionCredentials> {
    let extension = root.get("_ccSpace")
        .and_then(|value| serde_json::from_value::<ChannelExt>(value.clone()).ok());
    let env = root.get("env").and_then(Value::as_object);

    let legacy_anthropic = || {
        let base_url = env?.get("ANTHROPIC_BASE_URL")?.as_str()?.to_string();
        let token = env.and_then(|values| values.get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| values.get("ANTHROPIC_API_KEY")))
            .and_then(Value::as_str).unwrap_or("").to_string();
        Some(ChannelConnectionCredentials {
            base_url,
            auth_mode: if token.is_empty() { "none" } else { "bearer" }.to_string(),
            token,
        })
    };
    let legacy_openai = || {
        let base_url = env?.get("OPENAI_BASE_URL")?.as_str()?.to_string();
        let token = env.and_then(|values| values.get("OPENAI_API_KEY"))
            .and_then(Value::as_str).unwrap_or("").to_string();
        Some(ChannelConnectionCredentials {
            base_url,
            auth_mode: if token.is_empty() { "none" } else { "bearer" }.to_string(),
            token,
        })
    };

    let shared = extension.as_ref().and_then(|ext| ext.connection.as_ref()).and_then(|connection| {
        normalize_base_url(&connection.base_url).map(|base_url| ChannelConnectionCredentials {
            base_url,
            auth_mode: if connection.auth_mode.is_empty() {
                if connection.auth_token.as_deref().is_some_and(|token| !token.is_empty()) { "bearer" } else { "none" }
            } else {
                connection.auth_mode.as_str()
            }.to_string(),
            token: connection.auth_token.clone().unwrap_or_default(),
        })
    });

    let mut connection = shared.or_else(|| match adapter {
        ChannelAdapter::Claude => legacy_anthropic().or_else(legacy_openai),
        ChannelAdapter::Codex => extension.as_ref().and_then(|ext| ext.codex.as_ref()).and_then(|codex| {
            normalize_base_url(codex.base_url.as_deref()?).map(|base_url| ChannelConnectionCredentials {
                base_url,
                auth_mode: if codex.auth_mode.is_empty() {
                    if codex.auth_token.as_deref().is_some_and(|token| !token.is_empty()) { "bearer" } else { "none" }
                } else {
                    codex.auth_mode.as_str()
                }.to_string(),
                token: codex.auth_token.clone().unwrap_or_default(),
            })
        }).or_else(legacy_openai).or_else(legacy_anthropic),
        ChannelAdapter::OpenAiChat => legacy_openai().or_else(legacy_anthropic),
    })?;

    let (resolved, auto_resolve) = match adapter {
        ChannelAdapter::Claude => extension.as_ref().and_then(|ext| ext.claude.as_ref())
            .map_or((None, false), |config| (config.resolved_connection.as_ref(), config.auto_resolve)),
        ChannelAdapter::Codex => extension.as_ref().and_then(|ext| ext.codex.as_ref())
            .map_or((None, false), |config| (config.resolved_connection.as_ref(), config.auto_resolve)),
        ChannelAdapter::OpenAiChat => (None, false),
    };

    // Codex 兼容结构不能借用临时引用，单独覆盖；Claude 走共享结构。
    match adapter {
        ChannelAdapter::Claude => {
            if let Some(config) = extension.as_ref().and_then(|ext| ext.claude.as_ref()) {
                if let Some(base_url) = config.base_url.as_deref().and_then(normalize_base_url) {
                    connection.base_url = base_url;
                }
                if let Some(auth_mode) = config.auth_mode.as_deref().filter(|value| !value.is_empty()) {
                    connection.auth_mode = auth_mode.to_string();
                }
                if let Some(token) = config.auth_token.as_ref() {
                    connection.token = token.clone();
                }
            }
        }
        ChannelAdapter::Codex => {
            if let Some(config) = extension.as_ref().and_then(|ext| ext.codex.as_ref()) {
                if let Some(base_url) = config.base_url.as_deref().and_then(normalize_base_url) {
                    connection.base_url = base_url;
                }
                if !config.auth_mode.is_empty() {
                    connection.auth_mode.clone_from(&config.auth_mode);
                }
                if let Some(token) = config.auth_token.as_ref() {
                    connection.token = token.clone();
                }
            }
        }
        ChannelAdapter::OpenAiChat => {}
    }

    // 显式关闭当前适配器鉴权时，不能继续沿用通用连接里的凭据。
    if matches!(connection.auth_mode.as_str(), "none" | "openai") {
        connection.token.clear();
    }

    let source_base_url = connection.base_url.clone();
    if let Some(resolved) = resolved.filter(|cached| cached.source_base_url == source_base_url) {
        connection.base_url.clone_from(&resolved.base_url);
    } else if auto_resolve {
        if let Some(preferred) = adapter_base_url_candidates(&source_base_url, adapter).into_iter().next() {
            connection.base_url = preferred;
        }
    }
    Some(connection)
}


// ---- 前端命令 ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
    pub base_url: Option<String>,
    pub auth_mode: String,
    pub auth_token_masked: Option<String>,
    pub extra_env_keys: Vec<String>,
    pub valid: bool,
    pub enabled: bool,
    pub protocol: String,
    pub scope: String,
    pub agent_model: Option<String>,
    pub available_models: Vec<String>,
    /// Monet 托管的模型角色映射键当前值(MODEL_ENV_KEYS 过滤自 env 块,明文回传)
    pub model_env: BTreeMap<String, String>,
    /// 渠道默认模型(official 读 meta;第三方读文件 env.ANTHROPIC_MODEL)
    pub default_model: Option<String>,
    /// 渠道默认思考强度:五档 | "ultracode"(official 读 meta;第三方读文件顶层 ultracode/effortLevel)
    pub default_effort: Option<String>,
    /// 渠道在统一引擎系统中的可用范围。旧渠道自动为 ["claude-code"]。
    pub engine_support: Vec<String>,
    pub claude: Option<EngineConnectionView>,
    pub codex: Option<CodexChannelView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConnectionView {
    /// 仅返回显式覆盖；None 表示继承共享连接。
    pub base_url: Option<String>,
    pub auth_mode: Option<String>,
    pub auth_token_masked: Option<String>,
    /// 当前缓存或规则解析出的适配器 Base URL，用于最终地址预览。
    pub resolved_base_url: Option<String>,
    /// 仅返回与当前源地址匹配的真实探测缓存，保存时不得用规则推导值替代。
    pub cached_base_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexChannelView {
    pub mode: String,
    pub provider_id: String,
    pub base_url: Option<String>,
    pub auth_mode: String,
    pub auth_token_masked: Option<String>,
    pub default_model: Option<String>,
    pub default_effort: Option<String>,
    pub available_models: Vec<String>,
    pub resolved_base_url: Option<String>,
    pub cached_base_url: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderView {
    pub id: String,
    pub name: String,
    pub base_url: Option<String>,
    /// builtin:Codex 内置 Provider;config:用户 config.toml 中的 Provider。
    pub source: String,
}

const CODEX_BUILTIN_PROVIDERS: &[(&str, &str)] = &[
    ("openai", "OpenAI"),
    ("ollama", "Ollama"),
    ("lmstudio", "LM Studio"),
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelListResult {
    pub channels: Vec<ChannelView>,
    pub default_session_channels: BTreeMap<String, Option<String>>,
    pub default_session_models: BTreeMap<String, Option<String>>,
    pub default_session_efforts: BTreeMap<String, Option<String>>,
    /// 兼容旧前端；新前端使用 defaultSessionChannels。
    pub default_session_channel: Option<String>,
    pub default_agent_engine: String,
    pub default_agent_channels: BTreeMap<String, Option<String>>,
    pub default_agent_models: BTreeMap<String, Option<String>>,
    pub default_agent_efforts: BTreeMap<String, Option<String>>,
    /// 兼容旧前端；值取当前智能增强引擎的分槽。
    pub default_agent_channel: Option<String>,
    pub default_agent_model: Option<String>,
    pub default_agent_effort: Option<String>,
}

fn matching_cached_base_url(
    resolved: Option<&ResolvedConnectionExt>,
    source_base_url: Option<&str>,
) -> Option<String> {
    let source_base_url = source_base_url.and_then(normalize_base_url)?;
    resolved
        .filter(|cached| cached.source_base_url == source_base_url)
        .map(|cached| cached.base_url.clone())
}

fn build_channel_view(id: &str, meta: &ChannelMeta) -> ChannelView {
    if id == OFFICIAL_ID {
        return ChannelView {
            id: OFFICIAL_ID.to_string(),
            name: meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Official".to_string()),
            note: meta.note.clone().filter(|s| !s.is_empty()),
            base_url: None,
            auth_mode: "none".to_string(),
            auth_token_masked: None,
            extra_env_keys: vec![],
            valid: true,
            enabled: true,
            protocol: "anthropic".to_string(),
            scope: "full".to_string(),
            agent_model: None,
            available_models: vec![],
            model_env: BTreeMap::new(),
            default_model: meta.default_model.clone().filter(|s| !s.is_empty()),
            default_effort: meta.default_effort.clone().filter(|s| !s.is_empty()),
            engine_support: vec!["claude-code".to_string(), "codex".to_string()],
            claude: None,
            codex: None,
        };
    }
    if id == OFFICIAL_DIRECT_ID {
        return ChannelView {
            id: OFFICIAL_DIRECT_ID.to_string(),
            name: meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Official Direct".to_string()),
            note: meta.note.clone().filter(|s| !s.is_empty()),
            base_url: Some(OFFICIAL_BASE_URL.to_string()),
            auth_mode: "none".to_string(),
            auth_token_masked: None,
            extra_env_keys: vec![],
            valid: true,
            enabled: true,
            protocol: "anthropic".to_string(),
            scope: "full".to_string(),
            agent_model: None,
            available_models: vec![],
            model_env: BTreeMap::new(),
            default_model: meta.default_model.clone().filter(|s| !s.is_empty()),
            default_effort: meta.default_effort.clone().filter(|s| !s.is_empty()),
            engine_support: vec!["claude-code".to_string()],
            claude: None,
            codex: None,
        };
    }
    if id == APPLE_FM_ID {
        return ChannelView {
            id: APPLE_FM_ID.to_string(),
            name: meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Apple FM".to_string()),
            note: meta.note.clone().filter(|s| !s.is_empty()),
            base_url: Some(format!("http://localhost:{}", APPLE_FM_PORT)),
            auth_mode: "none".to_string(),
            auth_token_masked: None,
            extra_env_keys: vec![],
            valid: true,
            enabled: true,
            protocol: "openai".to_string(),
            scope: "agent-only".to_string(),
            agent_model: meta.agent_model.clone(),
            available_models: vec![],
            model_env: BTreeMap::new(),
            default_model: None,
            default_effort: None,
            engine_support: vec![],
            claude: None,
            codex: None,
        };
    }
    let path = channel_file_path(id);
    let parsed: Option<Value> = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let valid = parsed.is_some();
    let env = parsed
        .as_ref()
        .and_then(|v| v.get("env"))
        .and_then(|v| v.as_object());
    let is_openai = meta.protocol() == "openai";
    let hidden_keys: &[&str] = &[
        "ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY",
        "OPENAI_BASE_URL", "OPENAI_API_KEY",
    ];
    let extra_env_keys = env
        .map(|e| {
            e.keys()
                .filter(|k| !hidden_keys.contains(&k.as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let cc_ext = parsed.as_ref()
        .and_then(|v| v.get("_ccSpace"))
        .and_then(|v| serde_json::from_value::<ChannelExt>(v.clone()).ok())
        .unwrap_or_default();
    let legacy_codex = cc_ext.codex.as_ref();
    let base_url = cc_ext.connection.as_ref()
        .map(|connection| connection.base_url.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let key = if is_openai { "OPENAI_BASE_URL" } else { "ANTHROPIC_BASE_URL" };
            env.and_then(|values| values.get(key)).and_then(Value::as_str).map(String::from)
        })
        .or_else(|| legacy_codex.and_then(|config| config.base_url.clone()));
    let token = cc_ext.connection.as_ref()
        .and_then(|connection| connection.auth_token.as_deref())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let key = if is_openai { "OPENAI_API_KEY" } else { "ANTHROPIC_AUTH_TOKEN" };
            env.and_then(|values| values.get(key)).and_then(Value::as_str).filter(|value| !value.is_empty())
        })
        .or_else(|| legacy_codex.and_then(|config| config.auth_token.as_deref()).filter(|value| !value.is_empty()));
    let auth_mode = cc_ext.connection.as_ref()
        .map(|connection| connection.auth_mode.as_str()).filter(|value| !value.is_empty())
        .or_else(|| legacy_codex.and_then(|config| match config.auth_mode.as_str() {
            "bearer" => Some("bearer"),
            "openai" | "none" => Some("none"),
            _ => None,
        }))
        .unwrap_or(if token.is_some() { "bearer" } else { "none" }).to_string();
    let engine_support = if meta.is_agent_only() {
        vec![]
    } else {
        cc_ext.engine_support.clone().unwrap_or_else(|| vec!["claude-code".to_string()])
    };
    let codex = cc_ext.codex.as_ref().map(|config| {
        let cached_base_url = matching_cached_base_url(
            config.resolved_connection.as_ref(),
            config.base_url.as_deref().or(base_url.as_deref()),
        );
        CodexChannelView {
            mode: if config.mode.is_empty() { "managed".to_string() } else { config.mode.clone() },
            provider_id: config.provider_id.clone(),
            // 这里只回传显式覆盖；共享值由 ChannelView 顶层字段承载。
            base_url: config.base_url.clone().filter(|value| !value.trim().is_empty()),
            auth_mode: if config.auth_mode.is_empty() { "inherit".to_string() } else { config.auth_mode.clone() },
            auth_token_masked: config.auth_token.as_deref().filter(|value| !value.is_empty()).map(mask_token),
            // 仅供历史默认会话配置迁移；新保存不再写这些字段。
            default_model: config.default_model.clone().filter(|value| !value.is_empty()),
            default_effort: config.default_effort.clone().filter(|value| !value.is_empty()),
            available_models: cc_ext.available_models.clone(),
            resolved_base_url: parsed.as_ref()
                .and_then(|root| channel_connection_from_root(root, ChannelAdapter::Codex))
                .map(|connection| connection.base_url),
            cached_base_url,
        }
    });
    let claude = engine_support.iter().any(|engine| engine == "claude-code").then(|| {
        let config = cc_ext.claude.as_ref();
        let cached_base_url = matching_cached_base_url(
            config.and_then(|value| value.resolved_connection.as_ref()),
            config.and_then(|value| value.base_url.as_deref()).or(base_url.as_deref()),
        );
        EngineConnectionView {
            base_url: config.and_then(|value| value.base_url.clone()).filter(|value| !value.trim().is_empty()),
            auth_mode: config.and_then(|value| value.auth_mode.clone()).filter(|value| !value.is_empty()),
            auth_token_masked: config.and_then(|value| value.auth_token.as_deref())
                .filter(|value| !value.is_empty()).map(mask_token),
            resolved_base_url: parsed.as_ref()
                .and_then(|root| channel_connection_from_root(root, ChannelAdapter::Claude))
                .map(|connection| connection.base_url),
            cached_base_url,
        }
    });
    // 从 env 块过滤出 Monet 托管的模型角色映射键(明文回传)
    let model_env = env
        .map(|e| {
            MODEL_ENV_KEYS
                .iter()
                .filter_map(|k| {
                    e.get(*k)
                        .and_then(|v| v.as_str())
                        .map(|s| (k.to_string(), s.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    // 渠道默认模型/思考强度:全部读自渠道文件本身(原生 settings 语义,终端 --settings 同样生效)。
    // 默认模型 = env.ANTHROPIC_MODEL;默认思考强度 = 顶层 ultracode(true 优先) / effortLevel
    let default_model = env
        .and_then(|e| e.get("ANTHROPIC_MODEL"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let default_effort = parsed.as_ref().and_then(|root| {
        if root.get("ultracode").and_then(|v| v.as_bool()) == Some(true) {
            return Some("ultracode".to_string());
        }
        root.get("effortLevel")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    });
    ChannelView {
        name: meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| id.to_string()),
        note: meta.note.clone().filter(|s| !s.is_empty()),
        base_url,
        auth_mode,
        auth_token_masked: token.map(mask_token),
        extra_env_keys,
        valid,
        enabled: meta.is_enabled(),
        id: id.to_string(),
        protocol: meta.protocol().to_string(),
        scope: meta.scope().to_string(),
        agent_model: cc_ext.agent_model,
        available_models: cc_ext.available_models,
        model_env,
        default_model,
        default_effort,
        engine_support,
        claude,
        codex,
    }
}

#[tauri::command]
pub fn list_channels() -> ChannelListResult {
    let settings = load_app_settings();
    let agent_engine = configured_agent_engine(&settings).to_string();
    let file_ids = scan_channel_ids();
    let mut channels = Vec::new();

    // Official always first
    let official_meta = settings.channels.get(OFFICIAL_ID).cloned().unwrap_or_default();
    channels.push(build_channel_view(OFFICIAL_ID, &official_meta));

    // Official-direct second(虚拟渠道:强制官方,无文件)
    let od_meta = settings.channels.get(OFFICIAL_DIRECT_ID).cloned().unwrap_or_default();
    channels.push(build_channel_view(OFFICIAL_DIRECT_ID, &od_meta));

    // Apple FM if registered
    if settings.channels.contains_key(APPLE_FM_ID) {
        let meta = settings.channels.get(APPLE_FM_ID).cloned().unwrap_or_default();
        channels.push(build_channel_view(APPLE_FM_ID, &meta));
    }

    // File channels sorted
    for id in &file_ids {
        let meta = settings.channels.get(id).cloned().unwrap_or_default();
        channels.push(build_channel_view(id, &meta));
    }

    let mut default_session_models = settings.default_session_models.clone();
    let mut default_session_efforts = settings.default_session_efforts.clone();
    for engine in ["claude-code", "codex"] {
        if default_session_models.contains_key(engine) && default_session_efforts.contains_key(engine) {
            continue;
        }
        let selected_id = settings.default_session_channels.get(engine)
            .and_then(Option::as_deref).unwrap_or(OFFICIAL_ID);
        let selected = channels.iter().find(|channel| channel.id == selected_id);
        if !default_session_models.contains_key(engine) {
            let legacy_model = if engine == "codex" {
                selected.and_then(|channel| channel.codex.as_ref()).and_then(|codex| codex.default_model.clone())
            } else {
                selected.and_then(|channel| channel.default_model.clone())
            };
            default_session_models.insert(engine.to_string(), legacy_model);
        }
        if !default_session_efforts.contains_key(engine) {
            let legacy_effort = if engine == "codex" {
                selected.and_then(|channel| channel.codex.as_ref()).and_then(|codex| codex.default_effort.clone())
            } else {
                selected.and_then(|channel| channel.default_effort.clone())
            };
            default_session_efforts.insert(engine.to_string(), legacy_effort);
        }
    }

    ChannelListResult {
        channels,
        default_session_channels: settings.default_session_channels.clone(),
        default_session_models,
        default_session_efforts,
        default_session_channel: settings
            .default_session_channels
            .get("claude-code")
            .cloned()
            .flatten()
            .or(settings.default_session_channel),
        default_agent_engine: agent_engine.clone(),
        default_agent_channels: settings.default_agent_channels.clone(),
        default_agent_models: settings.default_agent_models.clone(),
        default_agent_efforts: settings.default_agent_efforts.clone(),
        default_agent_channel: settings.default_agent_channels.get(&agent_engine).cloned().flatten(),
        default_agent_model: settings.default_agent_models.get(&agent_engine).cloned().flatten(),
        default_agent_effort: settings.default_agent_efforts.get(&agent_engine).cloned().flatten(),
    }
}

fn codex_home_dir() -> Option<PathBuf> {
    let configured = std::env::var_os("CODEX_HOME")
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
        });
    configured.or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

fn sanitized_provider_base_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    // Base URL 可能带查询参数；列表只用于选择 Provider，不应把潜在的查询凭据回传前端。
    Some(
        value
            .split(['?', '#'])
            .next()
            .unwrap_or(value)
            .trim_end_matches('/')
            .to_string(),
    )
}

fn parse_codex_providers(content: &str) -> Result<Vec<CodexProviderView>, String> {
    let root: toml::Value =
        toml::from_str(content).map_err(|error| format!("Codex config.toml 解析失败: {error}"))?;
    let mut providers = BTreeMap::new();

    for (id, name) in CODEX_BUILTIN_PROVIDERS {
        providers.insert(
            (*id).to_string(),
            CodexProviderView {
                id: (*id).to_string(),
                name: (*name).to_string(),
                base_url: None,
                source: "builtin".to_string(),
            },
        );
    }

    if let Some(table) = root.get("model_providers").and_then(toml::Value::as_table) {
        for (id, value) in table {
            let provider = value.as_table();
            let name = provider
                .and_then(|table| table.get("name"))
                .and_then(toml::Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id)
                .to_string();
            let base_url = provider
                .and_then(|table| table.get("base_url"))
                .and_then(toml::Value::as_str)
                .and_then(sanitized_provider_base_url);
            providers.insert(
                id.clone(),
                CodexProviderView {
                    id: id.clone(),
                    name,
                    base_url,
                    source: "config".to_string(),
                },
            );
        }
    }

    Ok(providers.into_values().collect())
}

/// 列出 Codex 可用于 modelProvider 的内置及用户配置 Provider。
/// 只回传路由元数据，不读取认证 Token。
#[tauri::command]
pub fn list_codex_providers() -> Result<Vec<CodexProviderView>, String> {
    let Some(home) = codex_home_dir() else {
        return parse_codex_providers("");
    };
    let path = home.join("config.toml");
    match fs::read_to_string(&path) {
        Ok(content) => parse_codex_providers(&content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => parse_codex_providers(""),
        Err(error) => Err(format!("读取 Codex 配置失败: {}", error)),
    }
}

/// 渠道默认思考强度的合法值(五档 + ultracode 超档)
const VALID_EFFORT_VALUES: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultracode"];

fn validate_effort_value(effort: &str) -> Result<(), String> {
    if VALID_EFFORT_VALUES.contains(&effort) {
        Ok(())
    } else {
        Err(format!("无效的思考强度值: {}(允许 low/medium/high/xhigh/max/ultracode)", effort))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveEngineConnection {
    pub base_url: Option<String>,
    pub auth_mode: Option<String>,
    pub auth_token: Option<String>,
    pub resolved_base_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCodexChannel {
    pub mode: String,
    pub provider_id: String,
    pub base_url: Option<String>,
    pub auth_mode: Option<String>,
    pub auth_token: Option<String>,
    pub resolved_base_url: Option<String>,
}

fn normalize_engine_support(engines: &[String]) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for engine in engines {
        if engine != "claude-code" && engine != "codex" {
            return Err(format!("不支持的渠道引擎: {engine}"));
        }
        if !normalized.contains(engine) {
            normalized.push(engine.clone());
        }
    }
    Ok(normalized)
}

fn normalize_engine_connection(input: SaveEngineConnection, shared_base_url: &str) -> Result<Option<EngineConnectionExt>, String> {
    let base_url = input.base_url.as_deref().and_then(normalize_base_url);
    let auth_mode = input.auth_mode.as_deref().map(str::trim).filter(|value| !value.is_empty() && *value != "inherit")
        .map(String::from);
    if auth_mode.as_deref().is_some_and(|mode| !matches!(mode, "bearer" | "none")) {
        return Err("引擎连接覆盖的认证方式无效".to_string());
    }
    let mut auth_token = input.auth_token.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    if auth_mode.as_deref() == Some("none") {
        auth_token = None;
    }
    let source_base_url = base_url.clone().or_else(|| normalize_base_url(shared_base_url));
    let resolved_connection = match (source_base_url, input.resolved_base_url.as_deref().and_then(normalize_base_url)) {
        (Some(source_base_url), Some(base_url)) => Some(ResolvedConnectionExt { source_base_url, base_url }),
        _ => None,
    };
    Ok(Some(EngineConnectionExt {
        base_url,
        auth_mode,
        auth_token,
        resolved_connection,
        auto_resolve: true,
    }))
}

fn normalize_codex_channel(input: SaveCodexChannel, shared_base_url: &str) -> Result<CodexChannelExt, String> {
    let mode = input.mode.trim();
    if !matches!(mode, "managed" | "external") {
        return Err("Codex 渠道接入方式无效".to_string());
    }
    let provider_id = input.provider_id.trim();
    validate_id(provider_id)?;
    if mode == "external" {
        return Ok(CodexChannelExt {
            mode: "external".to_string(),
            provider_id: provider_id.to_string(),
            ..CodexChannelExt::default()
        });
    }
    if mode == "managed" && matches!(provider_id, "openai" | "ollama" | "lmstudio") {
        return Err(format!("{provider_id} 为 Codex 内置 Provider ID，请换一个自定义 ID"));
    }
    let base_url = input.base_url.as_deref().and_then(normalize_base_url);
    let auth_mode = input.auth_mode.as_deref().map(str::trim).filter(|value| !value.is_empty() && *value != "inherit")
        .unwrap_or("");
    if !auth_mode.is_empty() && !matches!(auth_mode, "bearer" | "openai" | "none") {
        return Err("Codex 连接覆盖的认证方式无效".to_string());
    }
    let mut auth_token = input.auth_token.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    if matches!(auth_mode, "openai" | "none") {
        auth_token = None;
    }
    let source_base_url = base_url.clone().or_else(|| normalize_base_url(shared_base_url));
    let resolved_connection = match (source_base_url, input.resolved_base_url.as_deref().and_then(normalize_base_url)) {
        (Some(source_base_url), Some(base_url)) => Some(ResolvedConnectionExt { source_base_url, base_url }),
        _ => None,
    };
    Ok(CodexChannelExt {
        mode: "managed".to_string(),
        provider_id: provider_id.to_string(),
        base_url,
        auth_mode: auth_mode.to_string(),
        auth_token,
        default_model: None,
        default_effort: None,
        available_models: vec![],
        resolved_connection,
        auto_resolve: true,
    })
}

#[allow(clippy::too_many_arguments)] // Tauri command 参数由前端调用签名决定
#[tauri::command]
pub fn save_channel(
    id: String,
    name: String,
    base_url: String,
    auth_token: Option<String>,
    auth_mode: Option<String>,
    note: Option<String>,
    protocol: Option<String>,
    scope: Option<String>,
    agent_model: Option<String>,
    available_models: Option<Vec<String>>,
    model_env: Option<std::collections::HashMap<String, String>>,
    default_effort: Option<String>,
    engine_support: Option<Vec<String>>,
    claude: Option<SaveEngineConnection>,
    codex: Option<SaveCodexChannel>,
) -> Result<(), String> {
    validate_id(&id)?;
    let is_virtual = id == APPLE_FM_ID;
    let normalized_engine_support = engine_support
        .as_deref()
        .map(normalize_engine_support)
        .transpose()?;
    let supports_claude = normalized_engine_support
        .as_ref()
        .map_or(true, |engines| engines.iter().any(|engine| engine == "claude-code"));
    let supports_codex = normalized_engine_support
        .as_ref()
        .is_some_and(|engines| engines.iter().any(|engine| engine == "codex"));
    let external_only = !supports_claude && supports_codex
        && codex.as_ref().is_some_and(|config| config.mode.trim() == "external");
    let uses_shared_connection = !external_only;
    let base_url = base_url.trim().to_string();
    if !is_virtual && uses_shared_connection && base_url.is_empty() {
        return Err("Base URL 不能为空".to_string());
    }
    let auth_mode = if uses_shared_connection {
        auth_mode.unwrap_or_else(|| "bearer".to_string())
    } else {
        "none".to_string()
    };
    if uses_shared_connection && !matches!(auth_mode.as_str(), "bearer" | "none") {
        return Err("渠道认证方式无效".to_string());
    }
    let existing_extension = fs::read_to_string(channel_file_path(&id)).ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|root| root.get("_ccSpace").cloned())
        .and_then(|value| serde_json::from_value::<ChannelExt>(value).ok());
    let existing_shared_token = existing_extension.as_ref()
        .and_then(|extension| extension.connection.as_ref())
        .and_then(|connection| connection.auth_token.as_ref())
        .cloned()
        .or_else(|| {
            existing_extension.as_ref().and_then(|extension| extension.connection.as_ref()).is_none()
                .then(|| {
                    let prefer_openai = load_app_settings().channels.get(&id)
                        .and_then(|meta| meta.protocol.as_deref()) == Some("openai");
                    read_legacy_channel_token(&id, prefer_openai)
                })
                .flatten()
        });
    let shared_token = auth_token.as_deref().map(str::trim).filter(|token| !token.is_empty())
        .map(String::from).or(existing_shared_token).filter(|token| !token.is_empty());
    if !is_virtual && uses_shared_connection && auth_mode == "bearer" && shared_token.is_none() {
        return Err("新建渠道必须提供 API Key".to_string());
    }

    if !is_virtual {
        fs::create_dir_all(channels_dir()).map_err(|e| e.to_string())?;
        let path = channel_file_path(&id);

        let mut root: Value = match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| {
                format!("渠道文件已存在但 JSON 解析失败,请先手动修复后重试({}): {}", path.display(), e)
            })?,
            Err(_) => json!({}),
        };
        let obj = root
            .as_object_mut()
            .ok_or("渠道文件顶层不是 JSON 对象,请手动修复后重试")?;
        if supports_claude {
            let env = obj.entry("env").or_insert_with(|| json!({}));
            let env_obj = env.as_object_mut().ok_or("渠道文件 env 字段不是对象")?;
            // 模型角色映射:替换语义只作用于 UI 管理键。
            if let Some(map) = model_env.as_ref() {
                for k in UI_MANAGED_MODEL_ENV_KEYS {
                    env_obj.remove(*k);
                }
                for k in UI_MANAGED_MODEL_ENV_KEYS {
                    if let Some(v) = map.get(*k) {
                        let v = v.trim();
                        if !v.is_empty() {
                            env_obj.insert(k.to_string(), json!(v));
                        }
                    }
                }
            }
        }
        // v2:连接凭据不再挂在任一引擎 env 下，旧键在成功保存时归一化移除。
        if let Some(env_obj) = obj.get_mut("env").and_then(Value::as_object_mut) {
            for key in ["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "OPENAI_BASE_URL", "OPENAI_API_KEY"] {
                env_obj.remove(key);
            }
        }

        // 渠道默认思考强度:替换语义(Some=按值重写,None=不动,向后兼容)。
        // 原生 settings 字段承载:五档写顶层 effortLevel;"ultracode" 写顶层 ultracode=true——
        // 终端 `claude --settings <渠道文件>` 吃到同一默认
        if supports_claude {
            if let Some(effort) = default_effort.as_deref() {
                let effort = effort.trim();
                obj.remove("effortLevel");
                obj.remove("ultracode");
                if !effort.is_empty() {
                    validate_effort_value(effort)?;
                    if effort == "ultracode" {
                        obj.insert("ultracode".to_string(), json!(true));
                    } else {
                        obj.insert("effortLevel".to_string(), json!(effort));
                    }
                }
            }
        }

        let mut extension = obj
            .get("_ccSpace")
            .cloned()
            .and_then(|value| serde_json::from_value::<ChannelExt>(value).ok())
            .unwrap_or_default();
        extension.connection = uses_shared_connection.then(|| ChannelConnectionExt {
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_mode: auth_mode.clone(),
            auth_token: if auth_mode == "bearer" { shared_token } else { None },
        });
        extension.agent_model = agent_model.as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(String::from);
        if let Some(models) = available_models {
            extension.available_models = models;
        }
        if let Some(engines) = normalized_engine_support.clone() {
            extension.engine_support = Some(engines);
        }
        if let Some(input) = claude {
            extension.claude = normalize_engine_connection(input, &base_url)?;
        }
        if let Some(input) = codex {
            extension.codex = Some(normalize_codex_channel(input, &base_url)?);
        } else if supports_codex && extension.codex.is_none() {
            return Err("启用 Codex 前需要配置 Provider".to_string());
        }
        let shared_has_token = extension.connection.as_ref()
            .and_then(|connection| connection.auth_token.as_deref())
            .is_some_and(|token| !token.is_empty());
        let claude_bearer_without_token = extension.claude.as_ref().is_some_and(|connection| {
            connection.auth_mode.as_deref() == Some("bearer")
                && connection
                    .auth_token
                    .as_deref()
                    .map_or(true, str::is_empty)
                && !shared_has_token
        });
        if supports_claude && claude_bearer_without_token {
            return Err("Claude Code 使用 Bearer 认证时必须提供 API Key".to_string());
        }
        let codex_bearer_without_token = extension.codex.as_ref().is_some_and(|connection| {
            connection.auth_mode == "bearer"
                && connection
                    .auth_token
                    .as_deref()
                    .map_or(true, str::is_empty)
                && !shared_has_token
        });
        if supports_codex && codex_bearer_without_token {
            return Err("Codex 使用 Bearer 认证时必须提供 API Key".to_string());
        }
        obj.insert(
            "_ccSpace".to_string(),
            serde_json::to_value(extension).map_err(|error| error.to_string())?,
        );

        write_json_0600(&path, &root)?;
    }

    let mut settings = load_app_settings();
    let meta = settings.channels.entry(id.clone()).or_default();
    meta.name = Some(name.trim().to_string()).filter(|s| !s.is_empty());
    meta.note = note.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    meta.protocol = protocol;
    meta.scope = scope;
    meta.agent_model = agent_model.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    save_app_settings(&settings)
}

/// official 渠道的默认模型/思考强度(无渠道文件,存 settings.json 的渠道元数据)。
/// 全量替换语义:两参数均传当前表单值,空/None = 清除该字段
#[tauri::command]
pub fn set_official_defaults(model: Option<String>, effort: Option<String>) -> Result<(), String> {
    let model = model.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let effort = effort.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(e) = effort.as_deref() {
        validate_effort_value(e)?;
    }
    let mut settings = load_app_settings();
    let meta = settings.channels.entry(OFFICIAL_ID.to_string()).or_default();
    meta.default_model = model;
    meta.default_effort = effort;
    save_app_settings(&settings)
}

#[tauri::command]
pub fn delete_channel(id: String) -> Result<(), String> {
    if is_builtin_channel(&id) {
        return Err("内置渠道不能删除".to_string());
    }
    validate_id(&id)?;
    let path = channel_file_path(&id);
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    let mut settings = load_app_settings();
    settings.channels.remove(&id);
    clear_channel_references(&mut settings, &id);
    save_app_settings(&settings)
}

#[tauri::command]
pub fn set_channel_enabled(id: String, enabled: bool) -> Result<(), String> {
    if is_builtin_channel(&id) {
        return Ok(());
    }
    validate_id(&id)?;
    let mut settings = load_app_settings();
    let meta = settings.channels.entry(id.clone()).or_default();
    meta.enabled = Some(enabled);
    if !enabled {
        clear_channel_references(&mut settings, &id);
    }
    save_app_settings(&settings)
}

#[tauri::command]
pub fn set_default_session_channel(engine: String, id: Option<String>) -> Result<(), String> {
    if engine != "claude-code" && engine != "codex" {
        return Err("不支持的会话引擎".to_string());
    }
    let mut settings = load_app_settings();
    let channel = id.filter(|s| !s.is_empty() && s != OFFICIAL_ID);
    if channel
        .as_deref()
        .is_some_and(|id| !is_channel_enabled(&settings, id))
    {
        return Err("已禁用的渠道不能设为会话默认渠道".to_string());
    }
    if channel
        .as_deref()
        .is_some_and(|id| !channel_supports_engine(id, &engine))
    {
        return Err(format!("该渠道未启用 {}，不能设为会话默认渠道", engine));
    }
    settings.default_session_models.insert(engine.clone(), None);
    settings.default_session_efforts.insert(engine.clone(), None);
    if let Some(channel) = channel {
        settings.default_session_channels.insert(engine, Some(channel));
    } else {
        settings.default_session_channels.remove(&engine);
    }
    save_app_settings(&settings)
}

#[tauri::command]
pub fn set_default_session_runtime(
    engine: String,
    model: Option<String>,
    effort: Option<String>,
) -> Result<(), String> {
    if engine != "claude-code" && engine != "codex" {
        return Err("不支持的会话引擎".to_string());
    }
    let model = model.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    let effort = effort.map(|value| value.trim().to_ascii_lowercase()).filter(|value| !value.is_empty());
    if let Some(value) = effort.as_deref() {
        validate_effort_value(value)?;
    }
    let mut settings = load_app_settings();
    settings.default_session_models.insert(engine.clone(), model);
    settings.default_session_efforts.insert(engine, effort);
    save_app_settings(&settings)
}

/// 官方直连的压制 settings 文件(固定路径,内容无敏感信息,幂等重写)。
/// 供 Agent CLI 直调等无 per-session 注入机制的 spawn 场景复用;
/// 会话 spawn 走 prepare_injection 的 per-session 合成,不用此文件
pub fn official_direct_settings_path() -> Result<PathBuf, String> {
    fs::create_dir_all(runtime_dir()).map_err(|e| e.to_string())?;
    let path = runtime_dir().join("official-direct.json");
    let mut env = serde_json::Map::new();
    env.insert("ANTHROPIC_BASE_URL".into(), json!(OFFICIAL_BASE_URL));
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(""));
    for key in DEFENSE_ENV_KEYS {
        env.insert(key.to_string(), json!(""));
    }
    write_json_0600(&path, &json!({ "env": env }))?;
    Ok(path)
}

/// 「跟随 CLI」当前实际指向:读 CLI user 级 settings 的 env 段(只读,不追 project/local 覆盖——
/// 副文案提示性质,取 user 级已覆盖绝大多数第三方配置场景)
#[tauri::command]
pub fn get_cli_env_target() -> Value {
    let host = fs::read_to_string(crate::config::claude_root().join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            let url = v.get("env")?.get("ANTHROPIC_BASE_URL")?.as_str()?;
            if url.is_empty() {
                return None;
            }
            let stripped = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(url);
            Some(stripped.split('/').next().unwrap_or(stripped).to_string())
        });
    json!({ "kind": if host.is_some() { "third-party" } else { "official" }, "host": host })
}

#[tauri::command]
pub fn set_default_agent_engine(engine: String) -> Result<(), String> {
    if !matches!(engine.as_str(), "claude-code" | "codex") {
        return Err("不支持的智能增强引擎".to_string());
    }
    let mut settings = load_app_settings();
    settings.default_agent_engine = Some(engine);
    save_app_settings(&settings)
}

#[tauri::command]
pub fn set_default_agent_model(
    engine: String,
    channel: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    if !matches!(engine.as_str(), "claude-code" | "codex") {
        return Err("不支持的智能增强引擎".to_string());
    }
    let mut settings = load_app_settings();
    let channel = channel.filter(|s| !s.is_empty());
    if channel
        .as_deref()
        .is_some_and(|id| !is_channel_enabled(&settings, id))
    {
        return Err("已禁用的渠道不能设为 Agent 默认渠道".to_string());
    }
    if engine == "codex"
        && channel.as_deref().is_some_and(|id| !channel_supports_engine(id, "codex"))
    {
        return Err("该渠道未启用 Codex，不能设为智能增强默认渠道".to_string());
    }
    settings.default_agent_models.insert(
        engine.clone(),
        model.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()),
    );
    settings.default_agent_channels.insert(engine, channel);
    save_app_settings(&settings)
}

#[tauri::command]
pub fn set_default_agent_effort(engine: String, effort: Option<String>) -> Result<(), String> {
    if !matches!(engine.as_str(), "claude-code" | "codex") {
        return Err("不支持的智能增强引擎".to_string());
    }
    let mut settings = load_app_settings();
    let effort = effort
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if let Some(value) = effort.as_deref() {
        validate_effort_value(value)?;
    }
    settings.default_agent_efforts.insert(engine, effort);
    save_app_settings(&settings)
}

#[tauri::command]
pub fn get_agent_toggles() -> BTreeMap<String, bool> {
    load_app_settings().agent_toggles
}

#[tauri::command]
pub fn set_agent_toggle(key: String, enabled: bool) -> Result<(), String> {
    let mut settings = load_app_settings();
    settings.agent_toggles.insert(key, enabled);
    save_app_settings(&settings)
}

pub fn is_agent_enabled(key: &str) -> bool {
    load_app_settings().agent_toggles.get(key).copied().unwrap_or(false)
}

/// Agent 会话是否落盘（默认 true）。false 时 spawn CLI 附加 --no-session-persistence
pub(crate) fn agent_session_persist() -> bool {
    load_app_settings().agent_session_persist.unwrap_or(true)
}

#[tauri::command]
pub fn get_agent_session_persist() -> bool {
    agent_session_persist()
}

#[tauri::command]
pub fn set_agent_session_persist(enabled: bool) -> Result<(), String> {
    let mut settings = load_app_settings();
    settings.agent_session_persist = Some(enabled);
    save_app_settings(&settings)
}

#[tauri::command]
pub fn get_agent_preferences() -> BTreeMap<String, AgentFeaturePrefs> {
    let settings = load_app_settings();
    settings.agent_preferences_by_engine
        .get(configured_agent_engine(&settings))
        .cloned()
        .unwrap_or_default()
}

#[tauri::command]
pub fn set_agent_feature_model(
    engine: String,
    key: String,
    channel: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    if !matches!(engine.as_str(), "claude-code" | "codex") {
        return Err("不支持的智能增强引擎".to_string());
    }
    let mut settings = load_app_settings();
    let channel = channel.filter(|s| !s.is_empty());
    if channel
        .as_deref()
        .is_some_and(|id| !is_channel_enabled(&settings, id))
    {
        return Err("已禁用的渠道不能设为功能偏好".to_string());
    }
    if engine == "codex"
        && channel.as_deref().is_some_and(|id| !channel_supports_engine(id, "codex"))
    {
        return Err("该渠道未启用 Codex，不能设为功能偏好".to_string());
    }
    let prefs = settings.agent_preferences_by_engine
        .entry(engine).or_default().entry(key).or_default();
    prefs.preferred_model = if channel.is_some() {
        model.filter(|s| !s.is_empty())
    } else {
        None
    };
    prefs.preferred_channel = channel;
    save_app_settings(&settings)
}

#[tauri::command]
pub fn get_channel_token(id: String, engine: Option<String>) -> Result<Option<String>, String> {
    if id == OFFICIAL_ID || id == APPLE_FM_ID {
        return Ok(None);
    }
    validate_id(&id)?;
    let extension = read_channel_ext(&id);
    let token = match engine.as_deref().unwrap_or("shared") {
        "shared" => {
            if let Some(connection) = extension.as_ref().and_then(|value| value.connection.as_ref()) {
                connection.auth_token.clone()
            } else {
                let prefer_openai = load_app_settings().channels.get(&id)
                    .and_then(|meta| meta.protocol.as_deref()) == Some("openai");
                read_legacy_channel_token(&id, prefer_openai)
            }
        }
        "claude-code" => extension.as_ref().and_then(|value| value.claude.as_ref())
            .and_then(|value| value.auth_token.clone()),
        "codex" => extension.as_ref().and_then(|value| value.codex.as_ref())
            .and_then(|value| value.auth_token.clone()),
        _ => return Err("不支持的渠道凭据类型".to_string()),
    };
    Ok(token.filter(|token| !token.is_empty()))
}

pub(crate) fn codex_channel_token(id: &str) -> Result<String, String> {
    validate_id(id)?;
    read_channel_connection(id, ChannelAdapter::Codex)
        .map(|connection| connection.token)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Codex 渠道凭据不可用".to_string())
}

/// 将统一渠道绑定转换为 Codex App Server 的 thread/* 运行参数。
/// 托管 Provider 的凭据不进入 App Server 请求或 argv，使用短命令按需读取渠道令牌。
pub(crate) fn codex_runtime_channel_options(id: &str) -> Result<Map<String, Value>, String> {
    if id == OFFICIAL_ID {
        return Ok(Map::new());
    }
    validate_id(id)?;
    let extension = read_channel_ext(id)
        .ok_or_else(|| format!("渠道配置不存在或不可读: {id}"))?;
    if !extension.supports_engine("codex") {
        return Err(format!("渠道 {id} 未启用 Codex"));
    }
    let channel = extension.codex.as_ref()
        .ok_or_else(|| format!("渠道 {id} 缺少 Codex Provider 配置"))?;
    let provider_id = channel.provider_id.trim();
    if provider_id.is_empty() {
        return Err(format!("渠道 {id} 缺少 Codex Provider ID"));
    }
    let mut options = Map::new();
    options.insert("modelProvider".to_string(), Value::String(provider_id.to_string()));
    // external 始终只引用 Codex 自有 Provider，不由 Monet 注入连接覆盖。
    if channel.mode == "external" {
        return Ok(options);
    }
    if matches!(provider_id, "openai" | "ollama" | "lmstudio") {
        return Err(format!("Codex 内置 Provider ID {provider_id} 不能被自定义渠道覆盖"));
    }
    let connection = read_channel_connection(id, ChannelAdapter::Codex)
        .ok_or_else(|| format!("渠道 {id} 缺少共享连接配置"))?;
    let auth_mode = connection.auth_mode.as_str();
    let mut provider = Map::from_iter([
        ("name".to_string(), Value::String(id.to_string())),
        ("base_url".to_string(), Value::String(connection.base_url.clone())),
        ("wire_api".to_string(), Value::String("responses".to_string())),
    ]);
    match auth_mode {
        "openai" => {
            provider.insert("requires_openai_auth".to_string(), Value::Bool(true));
        }
        "none" => {}
        "bearer" | "" => {
            if connection.token.is_empty() {
                return Err(format!("渠道 {id} 缺少 Codex Bearer Token"));
            }
            let executable = std::env::current_exe()
                .map_err(|error| format!("无法定位 Monet 可执行文件: {error}"))?;
            provider.insert(
                "auth".to_string(),
                json!({
                    "command": executable.to_string_lossy(),
                    "args": ["--monet-codex-channel-token", id],
                    "timeout_ms": 5000,
                    "refresh_interval_ms": 300000
                }),
            );
        }
        value => return Err(format!("渠道 {id} 的共享认证方式无效: {value}")),
    }
    let mut providers = Map::new();
    providers.insert(provider_id.to_string(), Value::Object(provider));
    options.insert(
        "config".to_string(),
        json!({ "model_providers": Value::Object(providers) }),
    );
    Ok(options)
}

#[tauri::command]
pub fn reveal_channels_dir() -> Result<(), String> {
    use crate::proc_ext::SpawnAndReap;
    let dir = channels_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---- spawn 注入(streaming.rs 消费) ----

pub struct ChannelInjection {
    pub settings_arg: String,
    pub env: Vec<(String, String)>,
    pub clear_env: Vec<String>,
    pub runtime_path: PathBuf,
}

const ADVISOR_MODEL: &str = "claude-fable-5";
const ADVISOR_ENABLE_ENV: &str = "CLAUDE_CODE_ENABLE_EXPERIMENTAL_ADVISOR_TOOL";

fn apply_fast_mode_override(obj: &mut Map<String, Value>, fast_mode: Option<bool>) {
    if let Some(enabled) = fast_mode {
        obj.insert("fastMode".to_string(), json!(enabled));
    }
}

pub fn prepare_injection(
    channel_id: Option<&str>,
    session_id: &str,
    ultracode: bool,
    advisor: bool,
    fast_mode: Option<bool>,
) -> Result<Option<ChannelInjection>, String> {
    if channel_id.is_none() && !ultracode && !advisor && fast_mode.is_none() {
        return Ok(None);
    }

    let mut root: Value = match channel_id {
        // 官方直连:无渠道文件,合成压制 settings——显式官方端点 + 下方防御清扫
        // 挤掉 CLI 用户配置里的第三方认证/路由键(空串 token 实测被 CLI 视为未设,回落 OAuth)
        Some(OFFICIAL_DIRECT_ID) => json!({ "env": { "ANTHROPIC_BASE_URL": OFFICIAL_BASE_URL } }),
        Some(id) => {
            validate_id(id)?;
            let text = fs::read_to_string(channel_file_path(id))
                .map_err(|_| format!("渠道配置不存在或不可读: {}", id))?;
            serde_json::from_str(&text)
                .map_err(|e| format!("渠道配置 JSON 解析失败({}): {}", id, e))?
        }
        None => json!({}),
    };
    let obj = root.as_object_mut().ok_or_else(|| match channel_id {
        Some(id) => format!("渠道配置顶层不是 JSON 对象: {}", id),
        None => "注入配置顶层不是 JSON 对象".to_string(),
    })?;

    let mut env_pairs = Vec::new();
    let mut clear_env: Vec<String> = Vec::new();
    {
        let env = obj.entry("env").or_insert_with(|| json!({}));
        let env_obj = env.as_object_mut().ok_or("注入配置 env 字段不是对象")?;
        if let Some(id) = channel_id.filter(|id| *id != OFFICIAL_DIRECT_ID) {
            let connection = read_channel_connection(id, ChannelAdapter::Claude)
                .ok_or_else(|| format!("渠道 {} 的 Claude Code 连接配置不可读", id))?;
            env_obj.insert("ANTHROPIC_BASE_URL".to_string(), json!(connection.base_url));
            env_obj.insert("ANTHROPIC_AUTH_TOKEN".to_string(), json!(connection.token));
        }
        if advisor {
            env_obj.insert(ADVISOR_ENABLE_ENV.to_string(), json!("1"));
        }
        for (k, v) in env_obj.iter() {
            if let Some(s) = v.as_str() {
                env_pairs.push((k.clone(), s.to_string()));
            }
        }
        if channel_id.is_some() {
            clear_env.extend(DEFENSE_ENV_KEYS.iter().map(|s| s.to_string()));
            let channel_has_token = env_obj
                .get("ANTHROPIC_AUTH_TOKEN")
                .and_then(|v| v.as_str())
                .is_some_and(|t| !t.is_empty());
            if !channel_has_token {
                clear_env.push("ANTHROPIC_AUTH_TOKEN".to_string());
            }
            for key in &clear_env {
                env_obj.entry(key.clone()).or_insert_with(|| json!(""));
            }
        }
    }
    if advisor {
        obj.insert("advisorModel".to_string(), json!(ADVISOR_MODEL));
    }
    if ultracode {
        obj.insert("ultracode".to_string(), json!(true));
    } else {
        // ultracode 开关以调用方解析结果为准:会话覆盖了五档时,
        // 渠道文件自带的 ultracode=true 不放行(否则超档压过会话选择)
        obj.remove("ultracode");
    }
    apply_fast_mode_override(obj, fast_mode);
    obj.remove("_ccSpace");

    fs::create_dir_all(runtime_dir()).map_err(|e| e.to_string())?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let runtime_path = runtime_dir().join(format!("{}-{}.json", session_id, nanos));
    write_json_0600(&runtime_path, &root)?;
    Ok(Some(ChannelInjection {
        settings_arg: runtime_path.to_string_lossy().into_owned(),
        env: env_pairs,
        clear_env,
        runtime_path,
    }))
}

pub fn cleanup_runtime_file(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn cleanup_runtime_dir() {
    if let Ok(entries) = fs::read_dir(runtime_dir()) {
        for entry in entries.filter_map(|e| e.ok()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

// ---- 渠道探活 + 模型发现 ----

use std::sync::OnceLock;
use std::time::Duration;

fn probe_client() -> Result<&'static reqwest::blocking::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| e.clone())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub online: bool,
    pub status: String,
    pub models: Vec<String>,
    pub latency_ms: u64,
    pub resolved_base_url: Option<String>,
    pub endpoint_url: Option<String>,
}

#[tauri::command]
pub async fn probe_channel(
    id: String,
    // 表单值直探(新建未保存渠道):齐传时绕过渠道文件并尝试带/不带 /v1 的候选。
    base_url: Option<String>,
    token: Option<String>,
    protocol: Option<String>,
    adapter: Option<String>,
) -> Result<ProbeResult, String> {
    let draft_adapter = adapter.as_deref().and_then(ChannelAdapter::parse)
        .or_else(|| protocol.as_deref().and_then(ChannelAdapter::parse));
    // 表单值直探路径:不读文件、不校验 id 存在性
    if let Some(url) = base_url.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        let token = token.unwrap_or_default();
        let adapter = draft_adapter.unwrap_or(ChannelAdapter::Claude);
        return tauri::async_runtime::spawn_blocking(move || {
            probe_channel_candidates(&url, &token, adapter, false)
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    }

    if id == OFFICIAL_ID {
        return Ok(ProbeResult {
            online: true,
            status: "official".to_string(),
            models: vec![],
            latency_ms: 0,
            resolved_base_url: None,
            endpoint_url: None,
        });
    }

    let is_apple_fm = id == APPLE_FM_ID;
    let adapter = if is_apple_fm {
        ChannelAdapter::OpenAiChat
    } else {
        draft_adapter.unwrap_or_else(|| {
            if channel_supports_engine(&id, "claude-code") { ChannelAdapter::Claude } else { ChannelAdapter::Codex }
        })
    };
    let connection = if is_apple_fm {
        ChannelConnectionCredentials {
            base_url: format!("http://localhost:{}", APPLE_FM_PORT),
            auth_mode: "none".to_string(),
            token: String::new(),
        }
    } else {
        read_channel_connection(&id, adapter)
            .ok_or_else(|| format!("渠道 {} 凭据不可读", id))?
    };

    tauri::async_runtime::spawn_blocking(move || {
        if is_apple_fm {
            let _ = ensure_fm_serve_running();
        }
        // 已保存渠道先探当前解析结果，不重新覆盖用户之前确认过的非 /v1 路径。
        probe_channel_candidates(&connection.base_url, &connection.token, adapter, true)
    })
    .await
    .map_err(|e| e.to_string())?
}

enum CandidateProbe {
    Selected(ProbeResult),
    Missing,
    Unreachable(ProbeResult),
}

fn apply_probe_auth(
    request: reqwest::blocking::RequestBuilder,
    token: &str,
    adapter: ChannelAdapter,
) -> reqwest::blocking::RequestBuilder {
    match adapter {
        ChannelAdapter::Claude => request
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01"),
        ChannelAdapter::Codex | ChannelAdapter::OpenAiChat if !token.is_empty() =>
            request.header("Authorization", format!("Bearer {token}")),
        _ => request,
    }
}

fn probe_candidate(base_url: &str, token: &str, adapter: ChannelAdapter) -> Result<CandidateProbe, String> {
    let client = probe_client()?;
    let models_url = adapter_models_url(base_url, adapter);
    let endpoint_url = adapter_endpoint_url(base_url, adapter);
    let start = std::time::Instant::now();
    let resp = apply_probe_auth(client.get(&models_url), token, adapter).send();
    let models = match resp {
        Ok(r) if r.status().is_success() => {
            let mut models = Vec::new();
            if let Ok(body) = r.json::<Value>() {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    for item in data {
                        if let Some(id) = item.get("id").and_then(Value::as_str) {
                            models.push(id.to_string());
                        }
                    }
                }
            }
            models
        }
        Ok(_) => vec![],
        Err(e) => {
            let latency = start.elapsed().as_millis() as u64;
            return Ok(CandidateProbe::Unreachable(ProbeResult {
                online: false,
                status: if e.is_timeout() { "timeout".to_string() } else { "offline".to_string() },
                models: vec![],
                latency_ms: latency,
                resolved_base_url: None,
                endpoint_url: Some(endpoint_url),
            }));
        }
    };

    // /models 的存在不能证明实际推理路由存在。用必然无效的空请求确认 endpoint；
    // 400/422 表示路由存在且不会触发模型调用或计费。
    let route_start = std::time::Instant::now();
    let route_response = apply_probe_auth(client.post(&endpoint_url), token, adapter)
        .header("content-type", "application/json")
        .json(&json!({}))
        .send();
    match route_response {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let latency = start.elapsed().as_millis() as u64;
            if matches!(status_code, 404 | 405) {
                return Ok(CandidateProbe::Missing);
            }
            Ok(CandidateProbe::Selected(ProbeResult {
                online: !matches!(status_code, 401 | 403 | 429 | 500..=599),
                status: if matches!(status_code, 401 | 403) { "auth_error".to_string() } else { status_code.to_string() },
                models,
                latency_ms: latency.max(route_start.elapsed().as_millis() as u64),
                resolved_base_url: Some(base_url.to_string()),
                endpoint_url: Some(endpoint_url),
            }))
        }
        Err(error) => Ok(CandidateProbe::Unreachable(ProbeResult {
            online: false,
            status: if error.is_timeout() { "timeout".to_string() } else { "offline".to_string() },
            models: vec![],
            latency_ms: start.elapsed().as_millis() as u64,
            resolved_base_url: None,
            endpoint_url: Some(endpoint_url),
        })),
    }
}

fn probe_channel_candidates(
    base_url: &str,
    token: &str,
    adapter: ChannelAdapter,
    exact_first: bool,
) -> Result<ProbeResult, String> {
    let candidates = if exact_first {
        vec![base_url.to_string()]
    } else {
        adapter_base_url_candidates(base_url, adapter)
    };
    let mut last_missing_endpoint = None;
    for candidate in candidates {
        match probe_candidate(&candidate, token, adapter)? {
            CandidateProbe::Selected(result) | CandidateProbe::Unreachable(result) => return Ok(result),
            CandidateProbe::Missing => last_missing_endpoint = Some(adapter_endpoint_url(&candidate, adapter)),
        }
    }
    Ok(ProbeResult {
        online: false,
        status: "route_not_found".to_string(),
        models: vec![],
        latency_ms: 0,
        resolved_base_url: None,
        endpoint_url: last_missing_endpoint,
    })
}

// ---- Apple FM 自动检测 & 进程管理 ----

use std::sync::Mutex;
use std::process::{Child, Command, Stdio};

static FM_PROCESS: Mutex<Option<Child>> = Mutex::new(None);

fn detect_apple_fm() -> bool {
    #[cfg(not(target_os = "macos"))]
    return false;

    // .app 环境 PATH 极简，which 依赖 PATH 查 fm，必须注入增强 PATH，
    // 否则 homebrew 等用户级安装的 fm 会静默检测失败
    #[cfg(target_os = "macos")]
    Command::new("which")
        .arg("fm")
        .env("PATH", crate::streaming::enhanced_path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn register_apple_fm_if_available() {
    if !detect_apple_fm() { return; }

    let mut settings = load_app_settings();
    if settings.channels.contains_key(APPLE_FM_ID) { return; }

    let meta = ChannelMeta {
        name: Some("Apple FM".to_string()),
        note: Some("Apple Foundation Models (local)".to_string()),
        protocol: Some("openai".to_string()),
        scope: Some("agent-only".to_string()),
        ..Default::default()
    };
    settings.channels.insert(APPLE_FM_ID.to_string(), meta);
    let _ = save_app_settings(&settings);
    eprintln!("[apple-fm] 检测到 fm 命令，已注册 Apple FM 渠道");
}

fn probe_port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(500),
    ).is_ok()
}

pub fn ensure_fm_serve_running() -> Result<(), String> {
    let mut guard = FM_PROCESS.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(ref mut child) = *guard {
        if child.try_wait().ok().flatten().is_none() && probe_port_open(APPLE_FM_PORT) {
            return Ok(());
        }
    }

    if probe_port_open(APPLE_FM_PORT) {
        return Ok(());
    }

    eprintln!("[apple-fm] 启动 fm serve --port {}", APPLE_FM_PORT);
    let child = Command::new("fm")
        .args(["serve", "--port", &APPLE_FM_PORT.to_string()])
        .env("PATH", crate::streaming::enhanced_path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("fm serve 启动失败: {}", e))?;

    *guard = Some(child);
    // 等待服务就绪
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if probe_port_open(APPLE_FM_PORT) {
            eprintln!("[apple-fm] fm serve 已就绪");
            return Ok(());
        }
    }
    Err("fm serve 启动超时".to_string())
}

pub fn shutdown_fm_serve() {
    let mut guard = FM_PROCESS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("[apple-fm] fm serve 已关闭");
    }
}

// ---- CC Switch 导入 ----

fn cc_switch_db_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".cc-switch").join("cc-switch.db");
    if path.is_file() { Some(path) } else { None }
}

fn cc_switch_channel_id(cc_switch_id: &str) -> String {
    let sanitized: String = cc_switch_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(59)
        .collect();
    format!("ccs-{}", sanitized)
}

fn query_cc_switch_providers() -> Result<Vec<Value>, String> {
    let db_path = cc_switch_db_path().ok_or("CC Switch 数据库不存在")?;
    let output = Command::new("sqlite3")
        .hide_console()
        .args([
            "-json",
            &db_path.to_string_lossy(),
            "SELECT id, name, settings_config, category, is_current, notes FROM providers WHERE app_type = 'claude' AND id NOT IN ('claude-official', 'claude-desktop-official')",
        ])
        .output()
        .map_err(|e| format!("sqlite3 执行失败: {}", e))?;
    if !output.status.success() {
        return Err(format!("sqlite3 查询失败: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(vec![]);
    }
    serde_json::from_str(&stdout).map_err(|e| format!("解析失败: {}", e))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchProvider {
    pub id: String,
    pub name: String,
    pub base_url: Option<String>,
    pub has_token: bool,
    pub category: Option<String>,
    pub is_current: bool,
    pub notes: Option<String>,
    pub already_imported: bool,
}

#[tauri::command]
pub fn scan_cc_switch() -> Result<Vec<CcSwitchProvider>, String> {
    let rows = query_cc_switch_providers()?;
    let existing = scan_channel_ids();
    let mut out = Vec::new();
    for row in &rows {
        let id = row.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let name = row.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let sc_str = row.get("settings_config").and_then(|v| v.as_str()).unwrap_or("{}");
        let sc: Value = serde_json::from_str(sc_str).unwrap_or(json!({}));
        let env = sc.get("env").and_then(|v| v.as_object());
        let base_url = env
            .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let has_token = env
            .and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN"))
            .and_then(|v| v.as_str())
            .is_some_and(|t| !t.is_empty());
        let channel_id = cc_switch_channel_id(id);
        out.push(CcSwitchProvider {
            id: id.to_string(),
            name: name.to_string(),
            base_url,
            has_token,
            category: row.get("category").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from),
            is_current: row.get("is_current").and_then(|v| v.as_i64()).unwrap_or(0) == 1,
            notes: row.get("notes").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from),
            already_imported: existing.contains(&channel_id),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn import_cc_switch(ids: Vec<String>) -> Result<u32, String> {
    let rows = query_cc_switch_providers()?;
    let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let mut settings = load_app_settings();
    let mut imported = 0u32;

    for row in &rows {
        let id = row.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if !id_set.contains(id) { continue; }

        let name = row.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let sc_str = row.get("settings_config").and_then(|v| v.as_str()).unwrap_or("{}");
        let sc: Value = serde_json::from_str(sc_str).unwrap_or(json!({}));
        let env = match sc.get("env").and_then(|v| v.as_object()) {
            Some(e) => e.clone(),
            None => continue,
        };

        let channel_id = cc_switch_channel_id(id);
        if validate_id(&channel_id).is_err() { continue; }

        fs::create_dir_all(channels_dir()).map_err(|e| e.to_string())?;
        write_json_0600(&channel_file_path(&channel_id), &json!({ "env": env }))?;

        let notes = row.get("notes").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
        let meta = settings.channels.entry(channel_id).or_default();
        meta.name = Some(name).filter(|s| !s.is_empty());
        meta.note = notes;
        meta.enabled = Some(true);
        meta.protocol = Some("anthropic".to_string());
        meta.scope = Some("full".to_string());
        imported += 1;
    }

    if imported > 0 {
        save_app_settings(&settings)?;
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_channel_support_defaults_to_claude_only() {
        let extension = ChannelExt::default();
        assert!(extension.supports_engine("claude-code"));
        assert!(!extension.supports_engine("codex"));
    }

    #[test]
    fn shared_connection_precedes_legacy_engine_credentials() {
        let root = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://legacy.example.com",
                "ANTHROPIC_AUTH_TOKEN": "legacy-token"
            },
            "_ccSpace": {
                "connection": {
                    "baseUrl": "https://shared.example.com",
                    "authMode": "bearer",
                    "authToken": "shared-token"
                }
            }
        });
        assert_eq!(
            channel_connection_from_root(&root, ChannelAdapter::Claude),
            Some(ChannelConnectionCredentials {
                base_url: "https://shared.example.com".into(),
                auth_mode: "bearer".into(),
                token: "shared-token".into(),
            })
        );
    }

    #[test]
    fn legacy_claude_credentials_remain_readable() {
        let root = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://legacy.example.com",
                "ANTHROPIC_AUTH_TOKEN": "legacy-token"
            }
        });
        assert_eq!(
            channel_connection_from_root(&root, ChannelAdapter::Claude),
            Some(ChannelConnectionCredentials {
                base_url: "https://legacy.example.com".into(),
                auth_mode: "bearer".into(),
                token: "legacy-token".into(),
            })
        );
    }

    #[test]
    fn adapter_urls_add_or_remove_v1_without_duplication() {
        assert_eq!(
            adapter_base_url_candidates("https://proxy.example.com", ChannelAdapter::Codex),
            vec!["https://proxy.example.com/v1", "https://proxy.example.com"]
        );
        assert_eq!(
            adapter_base_url_candidates("https://proxy.example.com/v1", ChannelAdapter::Claude),
            vec!["https://proxy.example.com", "https://proxy.example.com/v1"]
        );
        assert_eq!(
            adapter_endpoint_url("https://proxy.example.com/v1", ChannelAdapter::Codex),
            "https://proxy.example.com/v1/responses"
        );
    }

    #[test]
    fn engine_override_and_resolved_cache_are_applied_only_to_matching_source() {
        let root = json!({
            "_ccSpace": {
                "connection": {
                    "baseUrl": "https://shared.example.com",
                    "authMode": "bearer",
                    "authToken": "shared-token"
                },
                "claude": {
                    "baseUrl": "https://claude.example.com/v1",
                    "authMode": "none",
                    "resolvedConnection": {
                        "sourceBaseUrl": "https://claude.example.com/v1",
                        "baseUrl": "https://claude.example.com"
                    }
                }
            }
        });
        assert_eq!(
            channel_connection_from_root(&root, ChannelAdapter::Claude),
            Some(ChannelConnectionCredentials {
                base_url: "https://claude.example.com".into(),
                auth_mode: "none".into(),
                token: String::new(),
            })
        );
    }

    #[test]
    fn legacy_codex_connection_keeps_exact_url_and_infers_no_auth() {
        let root = json!({
            "_ccSpace": {
                "engineSupport": ["codex"],
                "codex": {
                    "mode": "managed",
                    "providerId": "legacy-proxy",
                    "baseUrl": "https://legacy.example.com",
                    "authMode": ""
                }
            }
        });
        assert_eq!(
            channel_connection_from_root(&root, ChannelAdapter::Codex),
            Some(ChannelConnectionCredentials {
                base_url: "https://legacy.example.com".into(),
                auth_mode: "none".into(),
                token: String::new(),
            })
        );
    }

    #[test]
    fn new_codex_connection_enables_url_completion_without_faking_cache() {
        let channel = normalize_codex_channel(SaveCodexChannel {
            mode: "managed".into(),
            provider_id: "work-proxy".into(),
            base_url: None,
            auth_mode: Some("inherit".into()),
            auth_token: None,
            resolved_base_url: None,
        }, "https://example.com").unwrap();
        assert!(channel.auto_resolve);
        assert!(channel.resolved_connection.is_none());

        let root = json!({
            "_ccSpace": {
                "connection": {
                    "baseUrl": "https://example.com",
                    "authMode": "none"
                },
                "engineSupport": ["codex"],
                "codex": channel
            }
        });
        assert_eq!(
            channel_connection_from_root(&root, ChannelAdapter::Codex)
                .map(|connection| connection.base_url),
            Some("https://example.com/v1".into())
        );
    }

    #[test]
    fn cached_base_url_is_exposed_only_for_its_original_source() {
        let cache = ResolvedConnectionExt {
            source_base_url: "https://example.com".into(),
            base_url: "https://example.com/v1".into(),
        };
        assert_eq!(
            matching_cached_base_url(Some(&cache), Some("https://example.com/")),
            Some("https://example.com/v1".into())
        );
        assert_eq!(
            matching_cached_base_url(Some(&cache), Some("https://other.example.com")),
            None
        );
        assert_eq!(matching_cached_base_url(None, Some("https://example.com")), None);
    }

    #[test]
    fn legacy_token_reveal_prefers_protocol_and_falls_back_to_codex() {
        let both = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "anthropic-token",
                "OPENAI_API_KEY": "openai-token"
            }
        });
        assert_eq!(legacy_channel_token_from_root(&both, false).as_deref(), Some("anthropic-token"));
        assert_eq!(legacy_channel_token_from_root(&both, true).as_deref(), Some("openai-token"));

        let codex_only = json!({
            "_ccSpace": { "codex": { "authToken": "codex-token" } }
        });
        assert_eq!(legacy_channel_token_from_root(&codex_only, false).as_deref(), Some("codex-token"));
    }

    #[test]
    fn external_codex_provider_remains_delegated() {
        let channel = normalize_codex_channel(SaveCodexChannel {
            mode: "external".into(),
            provider_id: "existing-provider".into(),
            base_url: None,
            auth_mode: None,
            auth_token: None,
            resolved_base_url: None,
        }, "").unwrap();
        assert_eq!(channel.mode, "external");
        assert!(!channel.auto_resolve);
        assert!(channel.base_url.is_none());
    }

    #[test]
    fn session_fast_mode_override_accepts_both_boolean_states() {
        let mut enabled = Map::new();
        enabled.insert("fastMode".into(), json!(true));
        apply_fast_mode_override(&mut enabled, Some(false));
        assert_eq!(enabled.get("fastMode"), Some(&json!(false)));

        let mut inherited = Map::new();
        inherited.insert("fastMode".into(), json!(true));
        apply_fast_mode_override(&mut inherited, None);
        assert_eq!(inherited.get("fastMode"), Some(&json!(true)));
    }

    #[test]
    fn codex_managed_provider_normalizes_runtime_fields() {
        let channel = normalize_codex_channel(SaveCodexChannel {
            mode: "managed".into(),
            provider_id: "work-proxy".into(),
            base_url: None,
            auth_mode: Some("inherit".into()),
            auth_token: None,
            resolved_base_url: Some("https://example.com/v1".into()),
        }, "https://example.com").unwrap();
        assert_eq!(channel.mode, "managed");
        assert_eq!(channel.provider_id, "work-proxy");
        assert!(channel.base_url.is_none());
        assert!(channel.auth_token.is_none());
        assert!(channel.available_models.is_empty());
        assert_eq!(channel.resolved_connection.as_ref().map(|value| value.base_url.as_str()), Some("https://example.com/v1"));
    }

    #[test]
    fn codex_provider_parser_lists_configured_and_builtin_providers() {
        let providers = parse_codex_providers(
            r#"
            [model_providers.work-proxy]
            name = "Work Proxy"
            base_url = "https://example.com/v1?query=redacted"
            "#,
        )
        .unwrap();

        let work_proxy = providers
            .iter()
            .find(|provider| provider.id == "work-proxy")
            .unwrap();
        assert_eq!(work_proxy.name, "Work Proxy");
        assert_eq!(
            work_proxy.base_url.as_deref(),
            Some("https://example.com/v1")
        );
        assert_eq!(work_proxy.source, "config");
        assert!(providers.iter().any(|provider| provider.id == "openai"));
    }

    #[test]
    fn agent_engine_settings_serialize_only_multi_engine_slots() {
        let mut settings = AppSettings {
            default_agent_channel: Some("legacy-channel".into()),
            default_agent_model: Some("legacy-model".into()),
            default_agent_effort: Some("low".into()),
            default_agent_engine: Some("codex".into()),
            ..AppSettings::default()
        };
        settings.default_agent_channels.insert("codex".into(), Some("official".into()));
        settings.default_agent_models.insert("codex".into(), Some("gpt-5.4".into()));
        settings.default_agent_efforts.insert("codex".into(), Some("high".into()));

        let value = serde_json::to_value(settings).unwrap();
        assert!(value.get("defaultAgentChannel").is_none());
        assert!(value.get("defaultAgentModel").is_none());
        assert!(value.get("defaultAgentEffort").is_none());
        assert_eq!(value["defaultAgentEngine"], "codex");
        assert_eq!(value["defaultAgentChannels"]["codex"], "official");
        assert_eq!(value["defaultAgentModels"]["codex"], "gpt-5.4");
        assert_eq!(value["defaultAgentEfforts"]["codex"], "high");
    }
}
