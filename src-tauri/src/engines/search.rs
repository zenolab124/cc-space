use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::core::{
    CoreSessionSummary, EngineError, EngineErrorKind, EngineInstanceId, EngineResult, ProjectRef,
    SearchDocument, SessionRef, SessionSource, SourceChange, SourceChangeKind,
};

const CACHE_VERSION: u32 = 2;
const MAX_ENTRIES: usize = 10_000;
const MAX_DOCUMENT_CHARS: usize = 2_000_000;
const MAX_TOTAL_CHARS: usize = 128 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CachedDocument {
    stamp: Option<String>,
    document: SearchDocument,
    #[serde(skip)]
    fresh: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShardFile {
    version: u32,
    project_key: String,
    entries: HashMap<String, CachedDocument>,
}

#[derive(Default)]
struct SearchCache {
    documents: HashMap<SessionRef, CachedDocument>,
    order: VecDeque<SessionRef>,
    loaded_projects: HashSet<ProjectRef>,
    total_chars: usize,
}

static CACHE: Mutex<Option<SearchCache>> = Mutex::new(None);

pub async fn documents_for_project(
    source: &dyn SessionSource,
    project: ProjectRef,
    sessions: Vec<CoreSessionSummary>,
) -> EngineResult<Vec<SearchDocument>> {
    load_project_shard(&project);
    let mut documents = Vec::with_capacity(sessions.len());
    let mut shard_entries = HashMap::with_capacity(sessions.len());
    let mut changed = false;

    for summary in sessions {
        let session = summary.reference;
        let stamp = summary.updated_at;
        let entry = match cached_entry(&session) {
            Some(entry) if cache_entry_matches(&entry, stamp.as_deref()) => entry,
            _ => {
                let mut document = source.build_search_document(session.clone()).await?;
                if document.text.chars().count() > MAX_DOCUMENT_CHARS {
                    document.text = document.text.chars().take(MAX_DOCUMENT_CHARS).collect();
                }
                changed = true;
                CachedDocument {
                    stamp: stamp.clone(),
                    document,
                    fresh: true,
                }
            }
        };
        insert(session.clone(), entry.clone());
        documents.push(entry.document.clone());
        shard_entries.insert(session.storage_key(), entry);
    }

    if changed || project_members_changed(&project, &shard_entries) {
        persist_project_shard(&project, shard_entries)?;
    }
    Ok(documents)
}

pub fn invalidate(instance: &EngineInstanceId, change: &SourceChange) {
    let mut guard = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    let cache = guard.get_or_insert_with(SearchCache::default);
    match change.kind {
        SourceChangeKind::SessionChanged | SourceChangeKind::SessionRemoved => {
            if let Some(session) = change.session.as_ref() {
                remove(cache, session);
            } else {
                clear_instance(cache, instance);
            }
        }
        SourceChangeKind::ProjectsChanged | SourceChangeKind::FullRefresh => {
            clear_instance(cache, instance)
        }
    }
}

fn cache_entry_matches(entry: &CachedDocument, stamp: Option<&str>) -> bool {
    match stamp {
        Some(stamp) => entry.stamp.as_deref() == Some(stamp),
        None => entry.fresh,
    }
}

fn cached_entry(session: &SessionRef) -> Option<CachedDocument> {
    CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .and_then(|cache| cache.documents.get(session))
        .cloned()
}

fn insert(session: SessionRef, entry: CachedDocument) {
    let mut guard = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    let cache = guard.get_or_insert_with(SearchCache::default);
    if let Some(previous) = cache.documents.insert(session.clone(), entry.clone()) {
        cache.total_chars = cache
            .total_chars
            .saturating_sub(previous.document.text.len());
        cache.order.retain(|candidate| candidate != &session);
    }
    cache.total_chars = cache.total_chars.saturating_add(entry.document.text.len());
    cache.order.push_back(session);
    while cache.documents.len() > MAX_ENTRIES || cache.total_chars > MAX_TOTAL_CHARS {
        if let Some(oldest) = cache.order.pop_front() {
            remove(cache, &oldest);
        } else {
            break;
        }
    }
}

fn remove(cache: &mut SearchCache, session: &SessionRef) {
    if let Some(entry) = cache.documents.remove(session) {
        cache.total_chars = cache.total_chars.saturating_sub(entry.document.text.len());
    }
    cache.order.retain(|candidate| candidate != session);
}

fn load_project_shard(project: &ProjectRef) {
    {
        let mut guard = CACHE.lock().unwrap_or_else(|error| error.into_inner());
        let cache = guard.get_or_insert_with(SearchCache::default);
        if !cache.loaded_projects.insert(project.clone()) {
            return;
        }
    }
    let Some(shard) = fs::read_to_string(shard_path(project))
        .ok()
        .and_then(|content| serde_json::from_str::<ShardFile>(&content).ok())
        .filter(|shard| {
            shard.version == CACHE_VERSION && shard.project_key == project.storage_key()
        })
    else {
        return;
    };
    for (_, mut entry) in shard.entries {
        if entry.document.session.engine() != project.engine() {
            continue;
        }
        entry.fresh = false;
        insert(entry.document.session.clone(), entry);
    }
}

fn project_members_changed(
    project: &ProjectRef,
    current: &HashMap<String, CachedDocument>,
) -> bool {
    fs::read_to_string(shard_path(project))
        .ok()
        .and_then(|content| serde_json::from_str::<ShardFile>(&content).ok())
        .map(|shard| {
            shard.version != CACHE_VERSION
                || shard.project_key != project.storage_key()
                || shard.entries.keys().collect::<HashSet<_>>()
                    != current.keys().collect::<HashSet<_>>()
        })
        .unwrap_or(true)
}

fn persist_project_shard(
    project: &ProjectRef,
    mut entries: HashMap<String, CachedDocument>,
) -> EngineResult<()> {
    for entry in entries.values_mut() {
        entry.fresh = false;
    }
    let content = serde_json::to_string(&ShardFile {
        version: CACHE_VERSION,
        project_key: project.storage_key(),
        entries,
    })
    .map_err(|error| EngineError::new(EngineErrorKind::Internal, error.to_string()))?;
    crate::config::atomic_write(&shard_path(project), &content)
        .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))
}

fn shard_path(project: &ProjectRef) -> PathBuf {
    crate::config::data_dir()
        .join("search")
        .join("v2")
        .join(project.engine().storage_key())
        .join(format!("{}.json", stable_key_hash(&project.storage_key())))
}

fn stable_key_hash(value: &str) -> String {
    fn fnv(bytes: impl Iterator<Item = u8>, seed: u64) -> u64 {
        bytes.fold(seed, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
    }
    let left = fnv(value.bytes(), 0xcbf29ce484222325);
    let right = fnv(value.bytes().rev(), 0x84222325cbf29ce4);
    format!("{left:016x}{right:016x}")
}

fn clear_instance(cache: &mut SearchCache, instance: &EngineInstanceId) {
    let sessions: Vec<_> = cache
        .documents
        .keys()
        .filter(|session| session.engine() == instance)
        .cloned()
        .collect();
    for session in sessions {
        remove(cache, &session);
    }
    cache
        .loaded_projects
        .retain(|project| project.engine() != instance);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(session: SessionRef, text: &str) -> CachedDocument {
        CachedDocument {
            stamp: Some("1".into()),
            document: SearchDocument {
                session,
                title: None,
                text: text.into(),
            },
            fresh: true,
        }
    }

    #[test]
    fn cache_keys_isolate_equal_native_ids() {
        let first =
            SessionRef::new(EngineInstanceId::new("one", "default").unwrap(), "same").unwrap();
        let second =
            SessionRef::new(EngineInstanceId::new("two", "default").unwrap(), "same").unwrap();
        insert(first.clone(), entry(first.clone(), "one"));
        insert(second.clone(), entry(second.clone(), "two"));

        assert_eq!(cached_entry(&first).unwrap().document.text, "one");
        assert_eq!(cached_entry(&second).unwrap().document.text, "two");
    }

    #[test]
    fn shard_names_are_stable_and_bounded() {
        let key = "project/".repeat(1_000);
        assert_eq!(stable_key_hash(&key), stable_key_hash(&key));
        assert_eq!(stable_key_hash(&key).len(), 32);
    }
}
