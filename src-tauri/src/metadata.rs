use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::config;
use crate::config::projects_dir;
use crate::engines::core::{EngineInstanceId, MetadataStore, SessionMetadataEntry, SessionRef};
use crate::models::{MessageContent, SessionRecord};
use crate::parser;

pub use crate::engines::core::SessionMetadata as SessionMeta;

static STORE: Mutex<Option<MetadataStore>> = Mutex::new(None);

fn claude_instance() -> EngineInstanceId {
    EngineInstanceId::new("claude-code", "default").expect("static Claude engine id is valid")
}

fn claude_session(session_id: impl Into<String>) -> Result<SessionRef, String> {
    SessionRef::new(claude_instance(), session_id.into()).map_err(|error| error.to_string())
}

fn with_store<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut MetadataStore) -> Result<R, String>,
{
    let mut guard = STORE.lock().unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
        *guard = Some(MetadataStore::open(config::data_dir(), &claude_instance())?);
    }
    let store = guard.as_mut().expect("metadata store was initialized");
    f(store)
}

#[tauri::command]
pub fn get_all_meta() -> HashMap<String, SessionMeta> {
    with_store(|store| Ok(store.all_for_instance(&claude_instance()))).unwrap_or_default()
}

#[tauri::command]
pub fn get_all_meta_v2() -> Result<Vec<SessionMetadataEntry>, String> {
    with_store(|store| store.all())
}

#[tauri::command]
pub fn update_meta(session_id: String, patch: SessionMeta) -> Result<SessionMeta, String> {
    let session = claude_session(session_id)?;
    with_store(|store| store.update(&session, patch))
}

#[tauri::command]
pub fn update_meta_v2(session: SessionRef, patch: SessionMeta) -> Result<SessionMeta, String> {
    with_store(|store| store.update(&session, patch))
}

/// 查询某会话是否已被软删除（discovery 过滤用）
pub fn is_deleted(session_id: &str) -> bool {
    let Ok(session) = claude_session(session_id) else {
        return false;
    };
    with_store(|store| {
        Ok(store
            .get(&session)
            .and_then(|metadata| metadata.deleted)
            .unwrap_or(false))
    })
    .unwrap_or(false)
}

fn metadata_for(session_id: &str) -> Option<SessionMeta> {
    let session = claude_session(session_id).ok()?;
    with_store(|store| Ok(store.get(&session).cloned()))
        .ok()
        .flatten()
}

pub fn metadata_for_ref(session: &SessionRef) -> Option<SessionMeta> {
    with_store(|store| Ok(store.get(session).cloned()))
        .ok()
        .flatten()
}

fn extract_conversation_snippet(project_id: &str, session_id: &str) -> Option<(String, usize)> {
    let path = projects_dir()
        .join(project_id)
        .join(format!("{}.jsonl", session_id));
    let records = parser::parse_messages(&path);

    let mut lines = Vec::new();
    for r in &records {
        if let SessionRecord::User(u) = r {
            if let Some(msg) = &u.message {
                let text = match &msg.content {
                    MessageContent::Text(s) => s.clone(),
                    MessageContent::Blocks(blocks) => {
                        use crate::models::ContentBlock;
                        blocks
                            .iter()
                            .filter_map(|b| match b {
                                ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    }
                };
                if !text.is_empty() {
                    let truncated: String = text.chars().take(500).collect();
                    lines.push(truncated);
                }
            }
        }
    }

    if lines.is_empty() {
        return None;
    }
    let count = lines.len();
    Some((lines.join("\n"), count))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleResult {
    pub title: String,
    pub turn_count: usize,
}

#[tauri::command]
pub async fn generate_title(project_id: String, session_id: String) -> Result<TitleResult, String> {
    if !crate::channels::is_agent_enabled("title") {
        return Err("agent.title 已禁用".to_string());
    }
    let sid = session_id.clone();
    let pid = project_id.clone();
    let current_title = metadata_for(&sid).and_then(|metadata| metadata.title);

    let (title, turn_count) = tauri::async_runtime::spawn_blocking(move || {
        let (snippet, count) =
            extract_conversation_snippet(&pid, &sid).ok_or_else(|| "会话无内容".to_string())?;
        let title = crate::agent::generate_title(&snippet, current_title.as_deref())?;
        Ok::<_, String>((title, count))
    })
    .await
    .map_err(|e| e.to_string())??;

    update_meta(
        session_id,
        SessionMeta {
            title: Some(title.clone()),
            ..Default::default()
        },
    )?;

    Ok(TitleResult { title, turn_count })
}

#[tauri::command]
pub async fn generate_permission_hint(
    tool_name: String,
    input_json: String,
) -> Result<String, String> {
    if !crate::channels::is_agent_enabled("permission_hint") {
        return Err("agent.permission_hint 已禁用".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        crate::agent::permission_hint(&tool_name, &input_json)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn translate_settings_fields(fields_json: String) -> Result<String, String> {
    if !crate::channels::is_agent_enabled("settings_explain") {
        return Err("agent.settings_explain 已禁用".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || crate::agent::translate_settings(&fields_json))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn extract_settings_defaults(fields_json: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cli_settings::extract_defaults_from_binary(&fields_json)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn generate_tags(project_id: String, session_id: String) -> Result<Vec<String>, String> {
    if !crate::channels::is_agent_enabled("tags") {
        return Err("agent.tags 已禁用".to_string());
    }
    let sid = session_id.clone();
    let pid = project_id.clone();
    let current_tags = metadata_for(&sid).and_then(|metadata| metadata.tags);

    let tags = tauri::async_runtime::spawn_blocking(move || {
        let (snippet, _) =
            extract_conversation_snippet(&pid, &sid).ok_or_else(|| "会话无内容".to_string())?;
        let raw = crate::agent::generate_tags(&snippet, current_tags.as_deref())?;
        let tags: Vec<String> = raw
            .split(['，', ','])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok::<_, String>(tags)
    })
    .await
    .map_err(|e| e.to_string())??;

    update_meta(
        session_id,
        SessionMeta {
            tags: Some(tags.clone()),
            ..Default::default()
        },
    )?;

    Ok(tags)
}

#[tauri::command]
pub async fn generate_summary(project_id: String, session_id: String) -> Result<String, String> {
    if !crate::channels::is_agent_enabled("summary") {
        return Err("agent.summary 已禁用".to_string());
    }
    let sid = session_id.clone();
    let pid = project_id.clone();
    let current_summary = metadata_for(&sid).and_then(|metadata| metadata.summary);

    let summary = tauri::async_runtime::spawn_blocking(move || {
        let (snippet, _) =
            extract_conversation_snippet(&pid, &sid).ok_or_else(|| "会话无内容".to_string())?;
        crate::agent::generate_summary(&snippet, current_summary.as_deref())
    })
    .await
    .map_err(|e| e.to_string())??;

    update_meta(
        session_id,
        SessionMeta {
            summary: Some(summary.clone()),
            ..Default::default()
        },
    )?;

    Ok(summary)
}

#[tauri::command]
pub fn set_agent_locale(locale: String) {
    crate::agent::set_locale(&locale);
}

#[tauri::command]
pub async fn parse_natural_schedule(text: String) -> Result<String, String> {
    if !crate::channels::is_agent_enabled("cron_parse") {
        return Err("agent.cron_parse 已禁用".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || crate::agent::parse_cron(&text))
        .await
        .map_err(|e| e.to_string())?
}
