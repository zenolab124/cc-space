//! 多渠道(profile)配置域:`~/.monet/`
//!
//! - `settings.json`        应用设置:默认会话/Agent 渠道 + 渠道展示元数据
//! - `channels/<id>.json`   纯净 Claude Code settings 格式(顶层 env 块等),
//!   终端可直接 `claude --settings <路径>` 复用同一渠道
//! - `runtime/<sid>-<ns>.json` per-spawn 合成产物(渠道内容 + 防御空值 + ultracode),
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
    pub available_models: Vec<String>,
    pub agent_model: Option<String>,
}

pub(crate) fn read_channel_ext(id: &str) -> Option<ChannelExt> {
    let text = fs::read_to_string(channel_file_path(id)).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    root.get("_ccSpace")
        .and_then(|v| serde_json::from_value::<ChannelExt>(v.clone()).ok())
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
    pub default_session_channel: Option<String>,
    pub default_agent_channel: Option<String>,
    pub default_agent_model: Option<String>,
    pub channels: BTreeMap<String, ChannelMeta>,
    pub agent_toggles: BTreeMap<String, bool>,
    pub agent_preferences: BTreeMap<String, AgentFeaturePrefs>,
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
    if settings.default_agent_channel.as_deref() == Some(id) {
        settings.default_agent_channel = None;
        settings.default_agent_model = None;
        changed = true;
    }
    for prefs in settings.agent_preferences.values_mut() {
        if prefs.preferred_channel.as_deref() == Some(id) {
            prefs.preferred_channel = None;
            prefs.preferred_model = None;
            changed = true;
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

    // 迁移：旧 defaultChannelId → default_session_channel
    if let Some(old_id) = settings.default_channel_id.take() {
        if old_id != OFFICIAL_ID && settings.default_session_channel.is_none() {
            settings.default_session_channel = Some(old_id);
        }
        migrated = true;
    }

    // 迁移：旧 session_chain → default_session_channel
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

    if settings.default_agent_channel.is_none() && settings.default_agent_model.take().is_some() {
        migrated = true;
    }
    for prefs in settings.agent_preferences.values_mut() {
        if prefs.preferred_channel.is_none() && prefs.preferred_model.take().is_some() {
            migrated = true;
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
}

pub struct AgentChannelResolveError {
    pub channel_id: String,
    pub message: String,
}

/// 解析会话默认渠道凭据，供智能搜索等跟随会话默认值的功能使用。
pub fn resolve_session_credentials(
    requested_model: &str,
) -> Result<Option<AgentChannelCredentials>, AgentChannelResolveError> {
    let settings = load_app_settings();
    let Some(channel_id) = settings.default_session_channel.as_deref() else {
        return Ok(None);
    };
    let model = resolve_session_model(channel_id, requested_model);
    resolve_channel_credentials_checked(channel_id, &settings, model).map(Some)
}

pub fn resolve_agent_for_feature_logged(
    key: &str,
) -> Result<Option<AgentChannelCredentials>, AgentChannelResolveError> {
    let settings = load_app_settings();
    let (channel_id, model) = settings
        .agent_preferences
        .get(key)
        .and_then(|prefs| {
            prefs
                .preferred_channel
                .as_deref()
                .map(|channel| (channel, prefs.preferred_model.clone()))
        })
        .or_else(|| {
            settings
                .default_agent_channel
                .as_deref()
                .map(|channel| (channel, settings.default_agent_model.clone()))
        })
        .map_or((None, None), |(channel, model)| (Some(channel), model));

    let Some(channel_id) = channel_id else {
        return Ok(None);
    };
    resolve_channel_credentials_checked(channel_id, &settings, model).map(Some)
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

fn resolve_session_model(channel_id: &str, requested_model: &str) -> Option<String> {
    let requested_model = requested_model.trim();
    if requested_model.is_empty() {
        return fallback_agent_model(channel_id);
    }
    if channel_id == APPLE_FM_ID {
        return Some("system".to_string());
    }
    if channel_id == OFFICIAL_ID || channel_id == OFFICIAL_DIRECT_ID {
        return Some(requested_model.to_string());
    }

    let parsed: Value = serde_json::from_str(
        &fs::read_to_string(channel_file_path(channel_id)).ok()?,
    )
    .ok()?;
    let env = parsed.get("env").and_then(Value::as_object);
    let requested_lower = requested_model.to_ascii_lowercase();
    let role = ["fable", "opus", "sonnet", "haiku"]
        .into_iter()
        .find(|role| requested_lower.contains(role));

    if let Some(role) = role {
        let key = format!("ANTHROPIC_DEFAULT_{}_MODEL", role.to_ascii_uppercase());
        if let Some(model) = env
            .and_then(|values| values.get(&key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            return Some(model.to_string());
        }
    }

    let models = read_channel_ext(channel_id)
        .map(|ext| ext.available_models)
        .unwrap_or_default();
    if let Some(model) = models.iter().find(|model| {
        model.eq_ignore_ascii_case(requested_model)
            || role.is_some_and(|role| model.to_ascii_lowercase().contains(role))
    }) {
        return Some(model.clone());
    }

    env.and_then(|values| values.get("ANTHROPIC_MODEL"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(String::from)
        .or_else(|| models.first().cloned())
        .or_else(|| Some(requested_model.to_string()))
}

/// Resolve single agent credentials from default settings
pub fn resolve_agent_credentials() -> Option<AgentChannelCredentials> {
    let settings = load_app_settings();
    let channel_id = settings.default_agent_channel.as_deref()?;
    let model = settings.default_agent_model.clone();
    resolve_channel_credentials(channel_id, &settings, model)
}

/// Resolve agent credentials for a specific feature, with fallback to default
pub fn resolve_agent_for_feature(key: &str) -> Option<AgentChannelCredentials> {
    let settings = load_app_settings();
    // Per-feature override
    if let Some(prefs) = settings.agent_preferences.get(key) {
        if let Some(ch) = prefs.preferred_channel.as_deref() {
            return resolve_channel_credentials(ch, &settings, prefs.preferred_model.clone());
        }
    }
    // Fall back to default agent
    let channel_id = settings.default_agent_channel.as_deref()?;
    let model = settings.default_agent_model.clone();
    resolve_channel_credentials(channel_id, &settings, model)
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
        });
    }
    // 官方直连:同走官方 CLI 直调,spawn 时额外注入压制 settings(见 agent.rs)
    if channel_id == OFFICIAL_DIRECT_ID {
        return Some(AgentChannelCredentials {
            id: OFFICIAL_DIRECT_ID.to_string(), is_official: true,
            base_url: None, token: None,
            protocol: "anthropic".to_string(),
            agent_model: None,
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
        });
    }
    let protocol = meta.map_or("anthropic", |m| m.protocol()).to_string();
    let (base_url, token) = read_channel_credentials(channel_id)?;
    let agent_model = model_override
        .or_else(|| read_channel_ext(channel_id).and_then(|e| e.agent_model))
        .or_else(|| fallback_agent_model(channel_id));
    Some(AgentChannelCredentials {
        id: channel_id.to_string(), is_official: false,
        base_url: Some(base_url), token: Some(token),
        protocol, agent_model,
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

pub(crate) fn read_channel_credentials(id: &str) -> Option<(String, String)> {
    let text = fs::read_to_string(channel_file_path(id)).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    let env = root.get("env")?.as_object()?;
    // Anthropic keys
    if let Some(base_url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
        let token = env
            .get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Some((base_url.to_string(), token.to_string()));
    }
    // OpenAI keys
    if let Some(base_url) = env.get("OPENAI_BASE_URL").and_then(|v| v.as_str()) {
        let token = env.get("OPENAI_API_KEY").and_then(|v| v.as_str()).unwrap_or("");
        return Some((base_url.to_string(), token.to_string()));
    }
    None
}


// ---- 前端命令 ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
    pub base_url: Option<String>,
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelListResult {
    pub channels: Vec<ChannelView>,
    pub default_session_channel: Option<String>,
    pub default_agent_channel: Option<String>,
    pub default_agent_model: Option<String>,
}

fn build_channel_view(id: &str, meta: &ChannelMeta) -> ChannelView {
    if id == OFFICIAL_ID {
        return ChannelView {
            id: OFFICIAL_ID.to_string(),
            name: meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Official".to_string()),
            note: meta.note.clone().filter(|s| !s.is_empty()),
            base_url: None,
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
        };
    }
    if id == OFFICIAL_DIRECT_ID {
        return ChannelView {
            id: OFFICIAL_DIRECT_ID.to_string(),
            name: meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Official Direct".to_string()),
            note: meta.note.clone().filter(|s| !s.is_empty()),
            base_url: Some(OFFICIAL_BASE_URL.to_string()),
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
        };
    }
    if id == APPLE_FM_ID {
        return ChannelView {
            id: APPLE_FM_ID.to_string(),
            name: meta.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Apple FM".to_string()),
            note: meta.note.clone().filter(|s| !s.is_empty()),
            base_url: Some(format!("http://localhost:{}", APPLE_FM_PORT)),
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
    let (url_key, token_key) = if is_openai {
        ("OPENAI_BASE_URL", "OPENAI_API_KEY")
    } else {
        ("ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN")
    };
    let base_url = env
        .and_then(|e| e.get(url_key))
        .and_then(|v| v.as_str())
        .map(String::from);
    let token = env
        .and_then(|e| e.get(token_key))
        .and_then(|v| v.as_str())
        .filter(|t| !t.is_empty());
    let hidden_keys: &[&str] = if is_openai {
        &["OPENAI_BASE_URL", "OPENAI_API_KEY"]
    } else {
        &["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN"]
    };
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
    }
}

#[tauri::command]
pub fn list_channels() -> ChannelListResult {
    let settings = load_app_settings();
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

    ChannelListResult {
        channels,
        default_session_channel: settings.default_session_channel,
        default_agent_channel: settings.default_agent_channel,
        default_agent_model: settings.default_agent_model,
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

#[allow(clippy::too_many_arguments)] // Tauri command 参数由前端调用签名决定
#[tauri::command]
pub fn save_channel(
    id: String,
    name: String,
    base_url: String,
    auth_token: Option<String>,
    note: Option<String>,
    protocol: Option<String>,
    scope: Option<String>,
    agent_model: Option<String>,
    available_models: Option<Vec<String>>,
    model_env: Option<std::collections::HashMap<String, String>>,
    default_effort: Option<String>,
) -> Result<(), String> {
    validate_id(&id)?;
    let is_virtual = id == APPLE_FM_ID;
    let base_url = base_url.trim().to_string();
    if !is_virtual && base_url.is_empty() {
        return Err("Base URL 不能为空".to_string());
    }
    let is_openai = protocol.as_deref() == Some("openai");

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
        let env = obj.entry("env").or_insert_with(|| json!({}));
        let env_obj = env.as_object_mut().ok_or("渠道文件 env 字段不是对象")?;
        let token = auth_token.as_deref().map(str::trim).filter(|t| !t.is_empty());
        if is_openai {
            env_obj.insert("OPENAI_BASE_URL".to_string(), json!(base_url));
            if let Some(t) = token {
                env_obj.insert("OPENAI_API_KEY".to_string(), json!(t));
            }
        } else {
            env_obj.insert("ANTHROPIC_BASE_URL".to_string(), json!(base_url));
            if let Some(t) = token {
                env_obj.insert("ANTHROPIC_AUTH_TOKEN".to_string(), json!(t));
            } else if env_obj
                .get("ANTHROPIC_AUTH_TOKEN")
                .and_then(|v| v.as_str())
                .filter(|t| !t.is_empty())
                .is_none()
            {
                return Err("新建渠道必须提供 Auth Token".to_string());
            }
        }

        // 模型角色映射:替换语义只作用于 UI 管理键。
        // Some(map)=先移除 UI 管理键再写入 map 中的非空值;None=完全不动(向后兼容)。
        // _DESCRIPTION/_CAPABILITIES 等无 UI 键不在替换范围,手编值保留
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

        // 渠道默认思考强度:替换语义(Some=按值重写,None=不动,向后兼容)。
        // 原生 settings 字段承载:五档写顶层 effortLevel;"ultracode" 写顶层 ultracode=true——
        // 终端 `claude --settings <渠道文件>` 吃到同一默认
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

        let am = agent_model.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let av = available_models.as_deref().unwrap_or(&[]);
        if am.is_some() || !av.is_empty() {
            obj.insert("_ccSpace".to_string(), json!({
                "agentModel": am,
                "availableModels": av,
            }));
        }

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
pub fn set_default_session_channel(id: Option<String>) -> Result<(), String> {
    let mut settings = load_app_settings();
    let channel = id.filter(|s| !s.is_empty() && s != OFFICIAL_ID);
    if channel
        .as_deref()
        .is_some_and(|id| !is_channel_enabled(&settings, id))
    {
        return Err("已禁用的渠道不能设为会话默认渠道".to_string());
    }
    settings.default_session_channel = channel;
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
pub fn set_default_agent_model(channel: Option<String>, model: Option<String>) -> Result<(), String> {
    let mut settings = load_app_settings();
    let channel = channel.filter(|s| !s.is_empty() && s != OFFICIAL_ID);
    if channel
        .as_deref()
        .is_some_and(|id| !is_channel_enabled(&settings, id))
    {
        return Err("已禁用的渠道不能设为 Agent 默认渠道".to_string());
    }
    settings.default_agent_model = if channel.is_some() {
        model.filter(|s| !s.is_empty())
    } else {
        None
    };
    settings.default_agent_channel = channel;
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
    load_app_settings().agent_preferences
}

#[tauri::command]
pub fn set_agent_feature_model(key: String, channel: Option<String>, model: Option<String>) -> Result<(), String> {
    let mut settings = load_app_settings();
    let channel = channel.filter(|s| !s.is_empty());
    if channel
        .as_deref()
        .is_some_and(|id| !is_channel_enabled(&settings, id))
    {
        return Err("已禁用的渠道不能设为功能偏好".to_string());
    }
    let prefs = settings.agent_preferences.entry(key).or_default();
    prefs.preferred_model = if channel.is_some() {
        model.filter(|s| !s.is_empty())
    } else {
        None
    };
    prefs.preferred_channel = channel;
    save_app_settings(&settings)
}

#[tauri::command]
pub fn get_channel_token(id: String) -> Result<Option<String>, String> {
    if id == OFFICIAL_ID || id == APPLE_FM_ID {
        return Ok(None);
    }
    validate_id(&id)?;
    let path = channel_file_path(&id);
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let root: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let env = root.get("env").and_then(|e| e.as_object());
    Ok(env
        .and_then(|e| {
            e.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| e.get("OPENAI_API_KEY"))
        })
        .and_then(|v| v.as_str())
        .filter(|t| !t.is_empty())
        .map(String::from))
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

pub fn prepare_injection(
    channel_id: Option<&str>,
    session_id: &str,
    ultracode: bool,
    advisor: bool,
) -> Result<Option<ChannelInjection>, String> {
    if channel_id.is_none() && !ultracode && !advisor {
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
}

#[tauri::command]
pub async fn probe_channel(
    id: String,
    // 表单值直探(新建未保存渠道的「获取模型列表」):三者齐传时绕过渠道文件
    base_url: Option<String>,
    token: Option<String>,
    protocol: Option<String>,
) -> Result<ProbeResult, String> {
    // 表单值直探路径:不读文件、不校验 id 存在性
    if let Some(url) = base_url.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        let token = token.unwrap_or_default();
        let protocol = protocol.unwrap_or_else(|| "anthropic".to_string());
        return tauri::async_runtime::spawn_blocking(move || {
            probe_channel_blocking(&url, &token, &protocol)
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
        });
    }

    let settings = load_app_settings();
    let protocol = if id == APPLE_FM_ID {
        "openai".to_string()
    } else {
        settings.channels.get(&id).map_or("anthropic", |m| m.protocol()).to_string()
    };

    let is_apple_fm = id == APPLE_FM_ID;
    let (base_url, token) = if is_apple_fm {
        (format!("http://localhost:{}", APPLE_FM_PORT), String::new())
    } else {
        read_channel_credentials(&id)
            .ok_or_else(|| format!("渠道 {} 凭据不可读", id))?
    };

    tauri::async_runtime::spawn_blocking(move || {
        if is_apple_fm {
            let _ = ensure_fm_serve_running();
        }
        probe_channel_blocking(&base_url, &token, &protocol)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn probe_channel_blocking(base_url: &str, token: &str, protocol: &str) -> Result<ProbeResult, String> {
    let client = probe_client()?;
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let start = std::time::Instant::now();

    let mut req = client.get(&url);
    if protocol == "openai" {
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
    } else {
        req = req.header("x-api-key", token);
        req = req.header("anthropic-version", "2023-06-01");
    }
    let resp = req.send();

    match resp {
        Ok(r) => {
            let latency = start.elapsed().as_millis() as u64;
            let status_code = r.status().as_u16();

            if status_code == 401 || status_code == 403 {
                return Ok(ProbeResult {
                    online: false,
                    status: "auth_error".to_string(),
                    models: vec![],
                    latency_ms: latency,
                });
            }

            let mut models = Vec::new();
            if r.status().is_success() {
                if let Ok(body) = r.json::<Value>() {
                    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                        for item in data {
                            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                                models.push(id.to_string());
                            }
                        }
                    }
                }
            }

            Ok(ProbeResult {
                online: true,
                status: format!("{}", status_code),
                models,
                latency_ms: latency,
            })
        }
        Err(e) => {
            let latency = start.elapsed().as_millis() as u64;
            Ok(ProbeResult {
                online: false,
                status: if e.is_timeout() { "timeout".to_string() } else { "offline".to_string() },
                models: vec![],
                latency_ms: latency,
            })
        }
    }
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

