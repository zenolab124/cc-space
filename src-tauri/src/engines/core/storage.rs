use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{EngineInstanceId, SessionRef};

const METADATA_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags_manual: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_manual: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl SessionMetadata {
    pub fn apply(&mut self, patch: Self) {
        if let Some(value) = patch.title {
            self.title = Some(value);
        }
        if let Some(value) = patch.deleted {
            self.deleted = Some(value);
        }
        if let Some(value) = patch.deleted_at {
            self.deleted_at = Some(value);
        }
        if let Some(value) = patch.tags {
            self.tags = Some(value);
        }
        if let Some(value) = patch.starred {
            self.starred = Some(value);
        }
        if let Some(value) = patch.tags_manual {
            self.tags_manual = Some(value);
        }
        if let Some(value) = patch.title_manual {
            self.title_manual = Some(value);
        }
        if let Some(value) = patch.summary {
            self.summary = Some(value);
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadataEntry {
    pub session: SessionRef,
    pub metadata: SessionMetadata,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceMetadata {
    sessions: BTreeMap<String, SessionMetadata>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetadataDocument {
    schema_version: u32,
    instances: BTreeMap<String, InstanceMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    migration_warnings: Vec<String>,
}

impl Default for MetadataDocument {
    fn default() -> Self {
        Self {
            schema_version: METADATA_SCHEMA_VERSION,
            instances: BTreeMap::new(),
            migration_warnings: Vec::new(),
        }
    }
}

pub struct MetadataStore {
    path: PathBuf,
    document: MetadataDocument,
}

impl MetadataStore {
    pub fn open(data_dir: &Path, legacy_instance: &EngineInstanceId) -> Result<Self, String> {
        let path = data_dir.join("metadata-v2.json");
        if path.is_file() {
            let document = read_document(&path)?;
            return Ok(Self { path, document });
        }

        let legacy_path = data_dir.join("metadata.json");
        let mut document = MetadataDocument::default();
        if legacy_path.is_file() {
            let content = fs::read_to_string(&legacy_path).map_err(|error| error.to_string())?;
            let legacy: HashMap<String, SessionMetadata> = serde_json::from_str(&content)
                .map_err(|error| format!("legacy metadata could not be migrated: {error}"))?;
            document.instances.insert(
                legacy_instance.storage_key(),
                InstanceMetadata {
                    sessions: legacy.into_iter().collect(),
                },
            );
        }

        let store = Self { path, document };
        store.save_and_validate(&store.document)?;
        Ok(store)
    }

    pub fn all(&self) -> Result<Vec<SessionMetadataEntry>, String> {
        let mut entries = Vec::new();
        for (instance_key, instance) in &self.document.instances {
            let engine = EngineInstanceId::from_storage_key(instance_key)
                .map_err(|error| error.to_string())?;
            for (native_id, metadata) in &instance.sessions {
                entries.push(SessionMetadataEntry {
                    session: SessionRef::new(engine.clone(), native_id)
                        .map_err(|error| error.to_string())?,
                    metadata: metadata.clone(),
                });
            }
        }
        Ok(entries)
    }

    pub fn all_for_instance(
        &self,
        instance: &EngineInstanceId,
    ) -> HashMap<String, SessionMetadata> {
        self.document
            .instances
            .get(&instance.storage_key())
            .map(|metadata| metadata.sessions.clone().into_iter().collect())
            .unwrap_or_default()
    }

    pub fn get(&self, session: &SessionRef) -> Option<&SessionMetadata> {
        self.document
            .instances
            .get(&session.engine().storage_key())
            .and_then(|instance| instance.sessions.get(session.native_id()))
    }

    pub fn update(
        &mut self,
        session: &SessionRef,
        patch: SessionMetadata,
    ) -> Result<SessionMetadata, String> {
        let mut next = self.document.clone();
        let entry = next
            .instances
            .entry(session.engine().storage_key())
            .or_default()
            .sessions
            .entry(session.native_id().to_string())
            .or_default();
        entry.apply(patch);
        let result = entry.clone();
        self.save_and_validate(&next)?;
        self.document = next;
        Ok(result)
    }

    pub fn update_tags(
        &mut self,
        session: &SessionRef,
        tags: Vec<String>,
        manual: bool,
    ) -> Result<SessionMetadata, String> {
        self.update(
            session,
            SessionMetadata {
                tags: Some(tags),
                tags_manual: manual.then_some(true),
                ..Default::default()
            },
        )
    }

    pub fn normalize_all_tags(&mut self) -> Result<bool, String> {
        let mut next = self.document.clone();
        let mut changed = false;
        for instance in next.instances.values_mut() {
            for metadata in instance.sessions.values_mut() {
                let Some(tags) = metadata.tags.as_ref() else {
                    continue;
                };
                let normalized = normalize_tag_values(tags);
                if &normalized != tags {
                    metadata.tags = Some(normalized);
                    changed = true;
                }
            }
        }
        if changed {
            self.save_and_validate(&next)?;
            self.document = next;
        }
        Ok(changed)
    }

    pub fn all_tags(&self) -> BTreeSet<String> {
        self.document
            .instances
            .values()
            .flat_map(|instance| instance.sessions.values())
            .flat_map(|metadata| metadata.tags.iter().flatten())
            .cloned()
            .collect()
    }

    pub fn tag_usage_counts(&self, include_deleted: bool) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for metadata in self
            .document
            .instances
            .values()
            .flat_map(|instance| instance.sessions.values())
        {
            if !include_deleted && metadata.deleted.unwrap_or(false) {
                continue;
            }
            for tag in metadata.tags.iter().flatten() {
                *counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    pub fn replace_tag(&mut self, source: &str, target: &str) -> Result<(), String> {
        if source == target {
            return Ok(());
        }
        let mut next = self.document.clone();
        let mut changed = false;
        for metadata in next
            .instances
            .values_mut()
            .flat_map(|instance| instance.sessions.values_mut())
        {
            let Some(tags) = metadata.tags.as_mut() else {
                continue;
            };
            if !tags.iter().any(|tag| tag == source) {
                continue;
            }
            let replaced = tags
                .iter()
                .map(|tag| {
                    if tag == source {
                        target.to_string()
                    } else {
                        tag.clone()
                    }
                })
                .collect::<Vec<_>>();
            *tags = normalize_tag_values(&replaced);
            changed = true;
        }
        if changed {
            self.save_and_validate(&next)?;
            self.document = next;
        }
        Ok(())
    }

    pub fn remove_tag(&mut self, name: &str) -> Result<(), String> {
        let mut next = self.document.clone();
        let mut changed = false;
        for metadata in next
            .instances
            .values_mut()
            .flat_map(|instance| instance.sessions.values_mut())
        {
            let Some(tags) = metadata.tags.as_mut() else {
                continue;
            };
            let before = tags.len();
            tags.retain(|tag| tag != name);
            changed |= tags.len() != before;
        }
        if changed {
            self.save_and_validate(&next)?;
            self.document = next;
        }
        Ok(())
    }

    fn save_and_validate(&self, document: &MetadataDocument) -> Result<(), String> {
        let json = serde_json::to_string_pretty(document).map_err(|error| error.to_string())?;
        crate::config::atomic_write(&self.path, &json).map_err(|error| error.to_string())?;
        let reloaded = read_document(&self.path)?;
        if &reloaded != document {
            return Err("metadata v2 validation failed after write".into());
        }
        Ok(())
    }
}

pub fn normalize_tag_values(values: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for value in values {
        for part in value.split(['，', ',', '、', ';', '；']) {
            let tag = part.trim();
            if !tag.is_empty() && seen.insert(tag.to_string()) {
                normalized.push(tag.to_string());
            }
        }
    }
    normalized
}

fn read_document(path: &Path) -> Result<MetadataDocument, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let document: MetadataDocument =
        serde_json::from_str(&content).map_err(|error| error.to_string())?;
    if document.schema_version != METADATA_SCHEMA_VERSION {
        return Err(format!(
            "unsupported metadata schema version: {}",
            document.schema_version
        ));
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("monet-metadata-v2-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn instance(engine: &str) -> EngineInstanceId {
        EngineInstanceId::new(engine, "default").unwrap()
    }

    #[test]
    fn initializes_an_empty_store_idempotently() {
        let root = TestDir::new();
        let first = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();
        assert!(first.all().unwrap().is_empty());

        let second = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();
        assert!(second.all().unwrap().is_empty());
    }

    #[test]
    fn ignores_an_interrupted_temporary_write_and_converges() {
        let root = TestDir::new();
        fs::write(root.0.join("metadata-v2.tmp4242"), "partial").unwrap();
        fs::write(
            root.0.join("metadata.json"),
            r#"{"legacy-id":{"title":"Legacy"}}"#,
        )
        .unwrap();

        let store = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();
        let session = SessionRef::new(instance("claude-code"), "legacy-id").unwrap();
        assert_eq!(
            store
                .get(&session)
                .and_then(|metadata| metadata.title.as_deref()),
            Some("Legacy")
        );
        assert_eq!(
            fs::read_to_string(root.0.join("metadata-v2.tmp4242")).unwrap(),
            "partial"
        );
    }

    #[test]
    fn migrates_legacy_once_without_modifying_legacy_file() {
        let root = TestDir::new();
        let legacy_path = root.0.join("metadata.json");
        let legacy = r#"{"same-id":{"title":"Legacy title","starred":true}}"#;
        fs::write(&legacy_path, legacy).unwrap();

        let mut first = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();
        let claude_ref = SessionRef::new(instance("claude-code"), "same-id").unwrap();
        first
            .update(
                &claude_ref,
                SessionMetadata {
                    title: Some("New title".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let reopened = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();

        assert_eq!(
            reopened
                .get(&claude_ref)
                .and_then(|metadata| metadata.title.as_deref()),
            Some("New title")
        );
        assert_eq!(fs::read_to_string(legacy_path).unwrap(), legacy);
    }

    #[test]
    fn same_native_id_is_isolated_across_engines() {
        let root = TestDir::new();
        let mut store = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();
        let claude_ref = SessionRef::new(instance("claude-code"), "same-id").unwrap();
        let codex_ref = SessionRef::new(instance("codex"), "same-id").unwrap();
        store
            .update(
                &claude_ref,
                SessionMetadata {
                    title: Some("Claude".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .update(
                &codex_ref,
                SessionMetadata {
                    title: Some("Codex".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(
            store.get(&claude_ref).unwrap().title.as_deref(),
            Some("Claude")
        );
        assert_eq!(
            store.get(&codex_ref).unwrap().title.as_deref(),
            Some("Codex")
        );
    }

    #[test]
    fn normalizes_all_supported_tag_separators_in_stable_order() {
        let values = vec![
            " 代码生成、插件开发, 文档 ".to_string(),
            "文档；测试;代码生成".to_string(),
        ];
        assert_eq!(
            normalize_tag_values(&values),
            vec!["代码生成", "插件开发", "文档", "测试"]
        );
    }

    #[test]
    fn failed_update_does_not_commit_the_in_memory_document() {
        let root = TestDir::new();
        let mut store = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();
        let session = SessionRef::new(instance("claude-code"), "session").unwrap();
        store
            .update(
                &session,
                SessionMetadata {
                    title: Some("Before".into()),
                    ..Default::default()
                },
            )
            .unwrap();

        let blocked_path = root.0.join("blocked");
        fs::create_dir(&blocked_path).unwrap();
        store.path = blocked_path;
        let result = store.update(
            &session,
            SessionMetadata {
                title: Some("After".into()),
                ..Default::default()
            },
        );

        assert!(result.is_err());
        assert_eq!(
            store.get(&session).unwrap().title.as_deref(),
            Some("Before")
        );
        let reopened = MetadataStore::open(&root.0, &instance("claude-code")).unwrap();
        assert_eq!(
            reopened.get(&session).unwrap().title.as_deref(),
            Some("Before")
        );
    }

    #[test]
    fn corrupt_v2_is_never_overwritten_by_migration() {
        let root = TestDir::new();
        fs::write(root.0.join("metadata.json"), r#"{"id":{"title":"legacy"}}"#).unwrap();
        fs::write(root.0.join("metadata-v2.json"), "not-json").unwrap();

        assert!(MetadataStore::open(&root.0, &instance("claude-code")).is_err());
        assert_eq!(
            fs::read_to_string(root.0.join("metadata-v2.json")).unwrap(),
            "not-json"
        );
    }
}
