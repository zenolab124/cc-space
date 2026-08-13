//! Routine 默认渠道解析的单一事实源。
//!
//! 主 App 的立即运行与独立 runner 都在每次执行前读取当前 settings.json，
//! 因而任务只保存引擎，渠道动态继承该引擎的 Monet 默认会话渠道。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::routine_types::RoutineEngine;

const OFFICIAL_ID: &str = "official";
const OFFICIAL_DIRECT_ID: &str = "official-direct";
const OFFICIAL_BASE_URL: &str = "https://api.anthropic.com";
const DEFENSE_ENV_KEYS: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RoutineSettings {
    default_session_channel: Option<String>,
    default_session_channels: BTreeMap<String, Option<String>>,
    channels: BTreeMap<String, ChannelMeta>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChannelMeta {
    enabled: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChannelExt {
    engine_support: Option<Vec<String>>,
    codex: Option<CodexChannel>,
}

impl ChannelExt {
    fn supports_engine(&self, engine_id: &str) -> bool {
        self.engine_support
            .as_ref()
            .map_or(engine_id == "claude-code", |engines| {
                engines.iter().any(|engine| engine == engine_id)
            })
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct CodexChannel {
    mode: String,
    provider_id: String,
    base_url: Option<String>,
    auth_mode: String,
    auth_token: Option<String>,
    default_model: Option<String>,
    default_effort: Option<String>,
}

pub struct RoutineChannel {
    pub claude_settings: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub clear_env: Vec<String>,
    pub codex_config: Vec<(String, String)>,
    runtime_path: Option<PathBuf>,
}

impl RoutineChannel {
    pub fn empty() -> Self {
        Self {
            claude_settings: None,
            env: Vec::new(),
            clear_env: Vec::new(),
            codex_config: Vec::new(),
            runtime_path: None,
        }
    }

    pub fn cleanup(&self) {
        if let Some(path) = &self.runtime_path {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn resolve(
    data_dir: &Path,
    engine: &RoutineEngine,
    execution_id: &str,
    auth_executable: &Path,
) -> Result<RoutineChannel, String> {
    let settings_path = data_dir.join("settings.json");
    let settings: RoutineSettings = match fs::read_to_string(&settings_path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("应用设置 JSON 解析失败: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => RoutineSettings::default(),
        Err(error) => return Err(format!("应用设置不可读: {error}")),
    };
    let channel_id = settings
        .default_session_channels
        .get(&engine.engine_id)
        .and_then(Option::as_deref)
        .or_else(|| {
            engine
                .is_claude_code()
                .then_some(settings.default_session_channel.as_deref())
                .flatten()
        });
    let Some(channel_id) = channel_id.filter(|id| !id.is_empty() && *id != OFFICIAL_ID) else {
        return Ok(RoutineChannel::empty());
    };
    if settings
        .channels
        .get(channel_id)
        .and_then(|meta| meta.enabled)
        == Some(false)
    {
        return Err(format!("默认渠道已禁用: {channel_id}"));
    }

    if engine.is_claude_code() {
        resolve_claude(data_dir, channel_id, execution_id)
    } else if engine.is_codex() {
        resolve_codex(data_dir, channel_id, auth_executable)
    } else {
        Err(format!(
            "unsupported routine engine: {}/{}",
            engine.engine_id, engine.instance_id
        ))
    }
}

fn resolve_claude(
    data_dir: &Path,
    channel_id: &str,
    execution_id: &str,
) -> Result<RoutineChannel, String> {
    let mut root = if channel_id == OFFICIAL_DIRECT_ID {
        json!({ "env": { "ANTHROPIC_BASE_URL": OFFICIAL_BASE_URL } })
    } else {
        read_channel(data_dir, channel_id)?
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| format!("渠道配置顶层不是 JSON 对象: {channel_id}"))?;
    if channel_id != OFFICIAL_DIRECT_ID {
        let extension = channel_ext(object);
        if !extension.supports_engine("claude-code") {
            return Err(format!("渠道 {channel_id} 未启用 Claude Code"));
        }
    }
    object.remove("_ccSpace");
    let env = object.entry("env").or_insert_with(|| json!({}));
    let env = env
        .as_object_mut()
        .ok_or_else(|| format!("渠道 {channel_id} 的 env 字段不是对象"))?;
    let mut env_pairs = env
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect::<Vec<_>>();
    let mut clear_env = DEFENSE_ENV_KEYS
        .iter()
        .map(|key| key.to_string())
        .collect::<Vec<_>>();
    let has_token = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty());
    if !has_token {
        clear_env.push("ANTHROPIC_AUTH_TOKEN".to_string());
    }
    for key in &clear_env {
        env.entry(key.clone()).or_insert_with(|| json!(""));
    }
    env_pairs.sort_by(|left, right| left.0.cmp(&right.0));

    let runtime_dir = data_dir.join("runtime");
    fs::create_dir_all(&runtime_dir).map_err(|error| error.to_string())?;
    let path = runtime_dir.join(format!("routine-{execution_id}.json"));
    write_json_0600(&path, &root)?;
    Ok(RoutineChannel {
        claude_settings: Some(path.clone()),
        env: env_pairs,
        clear_env,
        codex_config: Vec::new(),
        runtime_path: Some(path),
    })
}

fn resolve_codex(
    data_dir: &Path,
    channel_id: &str,
    auth_executable: &Path,
) -> Result<RoutineChannel, String> {
    if channel_id == OFFICIAL_DIRECT_ID {
        return Err("official-direct 不支持 Codex".to_string());
    }
    let root = read_channel(data_dir, channel_id)?;
    let object = root
        .as_object()
        .ok_or_else(|| format!("渠道配置顶层不是 JSON 对象: {channel_id}"))?;
    let extension = channel_ext(object);
    if !extension.supports_engine("codex") {
        return Err(format!("渠道 {channel_id} 未启用 Codex"));
    }
    let channel = extension
        .codex
        .ok_or_else(|| format!("渠道 {channel_id} 缺少 Codex Provider 配置"))?;
    let provider_id = channel.provider_id.trim();
    validate_id(provider_id)?;
    let mut config = vec![("model_provider".to_string(), toml_string(provider_id))];
    if channel.mode == "managed" {
        if matches!(provider_id, "openai" | "ollama" | "lmstudio") {
            return Err(format!(
                "Codex 内置 Provider ID {provider_id} 不能被自定义渠道覆盖"
            ));
        }
        let base_url = channel
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("渠道 {channel_id} 缺少 Codex Base URL"))?;
        let prefix = format!("model_providers.{provider_id}");
        config.extend([
            (format!("{prefix}.name"), toml_string(channel_id)),
            (format!("{prefix}.base_url"), toml_string(base_url)),
            (format!("{prefix}.wire_api"), toml_string("responses")),
        ]);
        match channel.auth_mode.as_str() {
            "openai" => config.push((format!("{prefix}.requires_openai_auth"), "true".to_string())),
            "none" => {}
            "bearer" | "" => {
                if channel.auth_token.as_deref().map_or(true, str::is_empty) {
                    return Err(format!("渠道 {channel_id} 缺少 Codex Bearer Token"));
                }
                config.extend([
                    (
                        format!("{prefix}.auth.command"),
                        toml_string(&auth_executable.to_string_lossy()),
                    ),
                    (
                        format!("{prefix}.auth.args"),
                        toml_array(&["--monet-codex-channel-token", channel_id]),
                    ),
                    (format!("{prefix}.auth.timeout_ms"), "5000".to_string()),
                    (
                        format!("{prefix}.auth.refresh_interval_ms"),
                        "300000".to_string(),
                    ),
                ]);
            }
            value => return Err(format!("渠道 {channel_id} 的 Codex 认证方式无效: {value}")),
        }
    } else if channel.mode != "external" {
        return Err(format!("渠道 {channel_id} 的 Codex 接入方式无效"));
    }
    if let Some(model) = nonempty(channel.default_model) {
        config.push(("model".to_string(), toml_string(&model)));
    }
    if let Some(effort) = nonempty(channel.default_effort) {
        config.push(("model_reasoning_effort".to_string(), toml_string(&effort)));
    }
    Ok(RoutineChannel {
        claude_settings: None,
        env: Vec::new(),
        clear_env: Vec::new(),
        codex_config: config,
        runtime_path: None,
    })
}

#[allow(dead_code)] // 主 App 走 channels.rs；独立 runner 直接复用本文件。
pub fn channel_token(data_dir: &Path, channel_id: &str) -> Result<String, String> {
    validate_id(channel_id)?;
    let root = read_channel(data_dir, channel_id)?;
    let extension = root
        .as_object()
        .map(channel_ext)
        .and_then(|extension| extension.codex)
        .ok_or_else(|| "Codex 渠道配置不可用".to_string())?;
    extension
        .auth_token
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Codex 渠道凭据不可用".to_string())
}

fn read_channel(data_dir: &Path, id: &str) -> Result<Value, String> {
    validate_id(id)?;
    let path = data_dir.join("channels").join(format!("{id}.json"));
    let text = fs::read_to_string(path).map_err(|_| format!("渠道配置不存在或不可读: {id}"))?;
    serde_json::from_str(&text).map_err(|error| format!("渠道配置 JSON 解析失败({id}): {error}"))
}

fn channel_ext(object: &Map<String, Value>) -> ChannelExt {
    object
        .get("_ccSpace")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err(format!("无效的渠道 ID: {id}"));
    }
    Ok(())
}

fn nonempty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn toml_array(values: &[&str]) -> String {
    toml::Value::Array(
        values
            .iter()
            .map(|value| toml::Value::String((*value).to_string()))
            .collect(),
    )
    .to_string()
}

fn write_json_0600(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "monet-routine-channel-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn write_fixture(root: &Path, relative: &str, value: Value) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }

    #[test]
    fn resolves_each_engine_from_its_own_default_channel_slot() {
        let directory = test_dir("engine-slots");
        write_fixture(
            &directory,
            "settings.json",
            json!({
                "defaultSessionChannels": {
                    "claude-code": "claude-proxy",
                    "codex": "codex-proxy"
                }
            }),
        );
        write_fixture(
            &directory,
            "channels/claude-proxy.json",
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://claude.example",
                    "ANTHROPIC_AUTH_TOKEN": "claude-token"
                },
                "_ccSpace": { "engineSupport": ["claude-code"] }
            }),
        );
        write_fixture(
            &directory,
            "channels/codex-proxy.json",
            json!({
                "_ccSpace": {
                    "engineSupport": ["codex"],
                    "codex": {
                        "mode": "managed",
                        "providerId": "routine-proxy",
                        "baseUrl": "https://codex.example/v1",
                        "authMode": "bearer",
                        "authToken": "codex-token",
                        "defaultModel": "gpt-test",
                        "defaultEffort": "high"
                    }
                }
            }),
        );

        let claude = resolve(
            &directory,
            &RoutineEngine::claude_code(),
            "claude-execution",
            Path::new("/bin/monet"),
        )
        .unwrap();
        assert!(claude.claude_settings.is_some());
        assert!(claude.codex_config.is_empty());
        assert!(claude.env.iter().any(|(key, value)| {
            key == "ANTHROPIC_BASE_URL" && value == "https://claude.example"
        }));

        let codex = resolve(
            &directory,
            &RoutineEngine::codex(),
            "codex-execution",
            Path::new("/bin/monet"),
        )
        .unwrap();
        assert!(codex.claude_settings.is_none());
        assert!(codex.env.is_empty());
        assert!(codex.codex_config.contains(&(
            "model_provider".to_string(),
            "\"routine-proxy\"".to_string()
        )));
        assert!(codex.codex_config.contains(&(
            "model_providers.routine-proxy.base_url".to_string(),
            "\"https://codex.example/v1\"".to_string()
        )));
        assert!(codex
            .codex_config
            .iter()
            .any(|(key, value)| key == "model" && value == "\"gpt-test\""));

        claude.cleanup();
        assert!(!directory
            .join("runtime/routine-claude-execution.json")
            .exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn follows_cli_only_when_the_engine_default_is_unset() {
        let directory = test_dir("unset");
        write_fixture(&directory, "settings.json", json!({}));

        for engine in [RoutineEngine::claude_code(), RoutineEngine::codex()] {
            let channel =
                resolve(&directory, &engine, "execution", Path::new("/bin/monet")).unwrap();
            assert!(channel.claude_settings.is_none());
            assert!(channel.env.is_empty());
            assert!(channel.codex_config.is_empty());
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_disabled_or_engine_incompatible_defaults() {
        let directory = test_dir("invalid-defaults");
        write_fixture(
            &directory,
            "settings.json",
            json!({
                "defaultSessionChannels": {
                    "claude-code": "disabled",
                    "codex": "claude-only"
                },
                "channels": { "disabled": { "enabled": false } }
            }),
        );
        write_fixture(
            &directory,
            "channels/claude-only.json",
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://claude.example",
                    "ANTHROPIC_AUTH_TOKEN": "token"
                },
                "_ccSpace": { "engineSupport": ["claude-code"] }
            }),
        );

        assert!(resolve(
            &directory,
            &RoutineEngine::claude_code(),
            "claude-execution",
            Path::new("/bin/monet"),
        )
        .err()
        .unwrap()
        .contains("默认渠道已禁用"));
        assert!(resolve(
            &directory,
            &RoutineEngine::codex(),
            "codex-execution",
            Path::new("/bin/monet"),
        )
        .err()
        .unwrap()
        .contains("未启用 Codex"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_corrupt_settings_instead_of_falling_back_to_cli() {
        let directory = test_dir("corrupt-settings");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("settings.json"), "{").unwrap();

        let error = resolve(
            &directory,
            &RoutineEngine::claude_code(),
            "execution",
            Path::new("/bin/monet"),
        )
        .err()
        .unwrap();
        assert!(error.contains("应用设置 JSON 解析失败"));
        fs::remove_dir_all(directory).unwrap();
    }
}
