use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde::Serialize;

use crate::config;
use crate::engines::core::{
    normalize_tag_values, ConversationRecord, ConversationRole, EngineInstanceId, MetadataStore,
    Segment, SessionMetadataEntry, SessionRef, TimelinePage,
};
use crate::engines::system;
use crate::tag_registry::{validate_new_tag_name, TagRegistryStore};

pub use crate::engines::core::SessionMetadata as SessionMeta;

static STORE: Mutex<Option<MetadataStore>> = Mutex::new(None);
static TAG_STORE: Mutex<Option<TagRegistryStore>> = Mutex::new(None);

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
        let mut store = MetadataStore::open(config::data_dir(), &claude_instance())?;
        store.normalize_all_tags()?;
        *guard = Some(store);
    }
    let store = guard.as_mut().expect("metadata store was initialized");
    f(store)
}

fn with_tag_state<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut MetadataStore, &mut TagRegistryStore) -> Result<R, String>,
{
    let mut metadata_guard = STORE.lock().unwrap_or_else(|error| error.into_inner());
    if metadata_guard.is_none() {
        let mut store = MetadataStore::open(config::data_dir(), &claude_instance())?;
        store.normalize_all_tags()?;
        *metadata_guard = Some(store);
    }
    let metadata = metadata_guard
        .as_mut()
        .expect("metadata store was initialized");

    let mut tag_guard = TAG_STORE.lock().unwrap_or_else(|error| error.into_inner());
    if tag_guard.is_none() {
        *tag_guard = Some(TagRegistryStore::open(
            config::data_dir(),
            &metadata.all_tags(),
        )?);
    }
    let registry = tag_guard.as_mut().expect("tag registry was initialized");
    registry.ensure_tags(metadata.all_tags())?;
    f(metadata, registry)
}

fn update_manual_meta(session: &SessionRef, mut patch: SessionMeta) -> Result<SessionMeta, String> {
    let Some(raw_tags) = patch.tags.take() else {
        return with_store(|store| store.update(session, patch));
    };
    with_tag_state(|metadata, registry| {
        let tags = normalize_tag_values(&raw_tags);
        for tag in &tags {
            if !registry.contains(tag) {
                validate_new_tag_name(tag)?;
            }
        }
        registry.ensure_tags(tags.iter().cloned())?;
        patch.tags = Some(tags);
        patch.tags_manual = Some(true);
        metadata.update(session, patch)
    })
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
    update_manual_meta(&session, patch)
}

#[tauri::command]
pub fn update_meta_v2(session: SessionRef, patch: SessionMeta) -> Result<SessionMeta, String> {
    update_manual_meta(&session, patch)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagView {
    name: String,
    color: String,
    created_at: String,
    usage_count: usize,
    total_usage_count: usize,
}

fn tag_views(metadata: &MetadataStore, registry: &TagRegistryStore) -> Vec<TagView> {
    let visible_counts = metadata.tag_usage_counts(false);
    let total_counts = metadata.tag_usage_counts(true);
    let mut tags = registry
        .definitions()
        .iter()
        .map(|tag| TagView {
            name: tag.name.clone(),
            color: tag.color.clone(),
            created_at: tag.created_at.clone(),
            usage_count: visible_counts.get(&tag.name).copied().unwrap_or_default(),
            total_usage_count: total_counts.get(&tag.name).copied().unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    tags.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| left.name.cmp(&right.name))
    });
    tags
}

#[tauri::command]
pub fn get_tag_registry() -> Result<Vec<TagView>, String> {
    with_tag_state(|metadata, registry| Ok(tag_views(metadata, registry)))
}

#[tauri::command]
pub fn update_session_tags(session: SessionRef, tags: Vec<String>) -> Result<SessionMeta, String> {
    update_manual_meta(
        &session,
        SessionMeta {
            tags: Some(tags),
            ..Default::default()
        },
    )
}

#[tauri::command]
pub fn rename_tag(source: String, target: String) -> Result<Vec<TagView>, String> {
    with_tag_state(|metadata, registry| {
        if !registry.contains(&source) {
            return Err("标签不存在".to_string());
        }
        let target = validate_new_tag_name(&target)?;
        if source == target {
            return Ok(tag_views(metadata, registry));
        }
        registry.ensure_renamed_target(&source, &target)?;
        metadata.replace_tag(&source, &target)?;
        registry.remove(&source)?;
        Ok(tag_views(metadata, registry))
    })
}

#[tauri::command]
pub fn delete_tag(name: String) -> Result<Vec<TagView>, String> {
    with_tag_state(|metadata, registry| {
        if !registry.contains(&name) {
            return Err("标签不存在".to_string());
        }
        metadata.remove_tag(&name)?;
        registry.remove(&name)?;
        Ok(tag_views(metadata, registry))
    })
}

#[tauri::command]
pub fn set_tag_color(name: String, color: String) -> Result<Vec<TagView>, String> {
    with_tag_state(|metadata, registry| {
        registry.set_color(&name, &color)?;
        Ok(tag_views(metadata, registry))
    })
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

pub fn metadata_for_ref(session: &SessionRef) -> Option<SessionMeta> {
    with_store(|store| Ok(store.get(session).cloned()))
        .ok()
        .flatten()
}

const TIMELINE_PAGE_SIZE: usize = 200;
const MAX_TIMELINE_PAGES: usize = 10_000;
const MAX_SNIPPET_LINE_CHARS: usize = 1_500;
const MAX_SNIPPET_CHARS: usize = 24_000;

fn snippet_from_records(records: &[ConversationRecord]) -> Option<(String, usize)> {
    let mut lines = VecDeque::new();
    let mut total_chars = 0_usize;
    let mut user_turns = 0_usize;

    for record in records {
        let (role, is_user) = match record.role {
            ConversationRole::User => ("用户", true),
            ConversationRole::Assistant => ("Agent", false),
            _ => continue,
        };
        let text = record
            .segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Text { text, .. } => Some(text.trim()),
                _ => None,
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            continue;
        }
        if is_user {
            user_turns += 1;
        }
        let body: String = text.chars().take(MAX_SNIPPET_LINE_CHARS).collect();
        let line = format!("{role}：{body}");
        total_chars = total_chars.saturating_add(line.chars().count() + 1);
        lines.push_back(line);
        while total_chars > MAX_SNIPPET_CHARS && lines.len() > 1 {
            if let Some(removed) = lines.pop_front() {
                total_chars = total_chars.saturating_sub(removed.chars().count() + 1);
            }
        }
    }

    (!lines.is_empty()).then(|| (lines.into_iter().collect::<Vec<_>>().join("\n"), user_turns))
}

async fn extract_conversation_snippet(session: &SessionRef) -> Result<(String, usize), String> {
    let source = system::get()
        .map_err(|error| error.to_string())?
        .registry()
        .source_for(session)
        .map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    let mut cursor = None;
    for _ in 0..MAX_TIMELINE_PAGES {
        let page = source
            .load_timeline(
                session.clone(),
                TimelinePage {
                    cursor: cursor.clone(),
                    limit: TIMELINE_PAGE_SIZE,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        records.extend(page.records);
        match page.next_cursor {
            Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
            _ => return snippet_from_records(&records).ok_or_else(|| "会话无内容".to_string()),
        }
    }
    Err("会话时间线分页超过安全上限".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleResult {
    pub title: String,
    pub turn_count: usize,
}

#[tauri::command]
pub async fn generate_title(session: SessionRef) -> Result<TitleResult, String> {
    if !crate::channels::is_agent_enabled("title") {
        return Err("agent.title 已禁用".to_string());
    }
    let current_title = metadata_for_ref(&session).and_then(|metadata| metadata.title);
    let (snippet, turn_count) = extract_conversation_snippet(&session).await?;

    let title = tauri::async_runtime::spawn_blocking(move || {
        let title = crate::agent::generate_title(&snippet, current_title.as_deref())?;
        Ok::<_, String>(title)
    })
    .await
    .map_err(|e| e.to_string())??;

    let title = with_store(|store| {
        let latest = store.get(&session).cloned().unwrap_or_default();
        if latest.title_manual.unwrap_or(false) {
            return Ok(latest.title.unwrap_or(title.clone()));
        }
        store.update(
            &session,
            SessionMeta {
                title: Some(title.clone()),
                ..Default::default()
            },
        )?;
        Ok(title.clone())
    })?;

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagsResult {
    tags: Vec<String>,
    skipped: bool,
}

#[tauri::command]
pub async fn generate_tags(session: SessionRef) -> Result<TagsResult, String> {
    if !crate::channels::is_agent_enabled("tags") {
        return Err("agent.tags 已禁用".to_string());
    }
    let (current_tags, preferred_tags, manual) = with_tag_state(|metadata, registry| {
        let current = metadata.get(&session).cloned().unwrap_or_default();
        let counts = metadata.tag_usage_counts(false);
        let mut preferred = registry
            .definitions()
            .iter()
            .map(|tag| {
                (
                    tag.name.clone(),
                    counts.get(&tag.name).copied().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        preferred.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        Ok((
            current.tags.unwrap_or_default(),
            preferred
                .into_iter()
                .take(30)
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            current.tags_manual.unwrap_or(false),
        ))
    })?;
    if manual {
        return Ok(TagsResult {
            tags: current_tags,
            skipped: true,
        });
    }
    let (snippet, _) = extract_conversation_snippet(&session).await?;

    let input_tags = current_tags.clone();
    let tags = tauri::async_runtime::spawn_blocking(move || {
        let raw = crate::agent::generate_tags(&snippet, Some(&input_tags), &preferred_tags)?;
        let tags = normalize_tag_values(&[raw]);
        Ok::<_, String>(tags)
    })
    .await
    .map_err(|e| e.to_string())??;

    let tags = if tags.is_empty() { current_tags } else { tags };
    with_tag_state(|metadata, registry| {
        let latest = metadata.get(&session).cloned().unwrap_or_default();
        if latest.tags_manual.unwrap_or(false) {
            return Ok(TagsResult {
                tags: latest.tags.unwrap_or_default(),
                skipped: true,
            });
        }
        registry.ensure_tags(tags.iter().cloned())?;
        metadata.update_tags(&session, tags.clone(), false)?;
        Ok(TagsResult {
            tags: tags.clone(),
            skipped: false,
        })
    })
}

#[tauri::command]
pub async fn generate_summary(session: SessionRef) -> Result<String, String> {
    if !crate::channels::is_agent_enabled("summary") {
        return Err("agent.summary 已禁用".to_string());
    }
    let current_summary = metadata_for_ref(&session).and_then(|metadata| metadata.summary);
    let (snippet, _) = extract_conversation_snippet(&session).await?;

    let summary = tauri::async_runtime::spawn_blocking(move || {
        crate::agent::generate_summary(&snippet, current_summary.as_deref())
    })
    .await
    .map_err(|e| e.to_string())??;

    with_store(|store| {
        store.update(
            &session,
            SessionMeta {
                summary: Some(summary.clone()),
                ..Default::default()
            },
        )
    })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::core::{SourceMetadata, TextPhase};

    fn record(role: ConversationRole, segments: Vec<Segment>) -> ConversationRecord {
        ConversationRecord {
            id: format!("record-{role:?}"),
            session: SessionRef::new(claude_instance(), "session").unwrap(),
            turn_id: None,
            parent_id: None,
            role,
            timestamp: None,
            segments,
            usage: None,
            source_meta: SourceMetadata::default(),
        }
    }

    #[test]
    fn normalized_snippet_keeps_user_and_assistant_text_only() {
        let records = vec![
            record(
                ConversationRole::System,
                vec![Segment::Text {
                    text: "system instructions".into(),
                    phase: None,
                }],
            ),
            record(
                ConversationRole::User,
                vec![Segment::Text {
                    text: "实现多引擎摘要".into(),
                    phase: None,
                }],
            ),
            record(
                ConversationRole::Assistant,
                vec![
                    Segment::Reasoning {
                        text: "private reasoning".into(),
                        visibility: crate::engines::core::ReasoningVisibility::Visible,
                    },
                    Segment::Text {
                        text: "已经完成".into(),
                        phase: Some(TextPhase::Final),
                    },
                ],
            ),
        ];

        let (snippet, turns) = snippet_from_records(&records).unwrap();
        assert_eq!(turns, 1);
        assert_eq!(snippet, "用户：实现多引擎摘要\nAgent：已经完成");
        assert!(!snippet.contains("system instructions"));
        assert!(!snippet.contains("private reasoning"));
    }
}
