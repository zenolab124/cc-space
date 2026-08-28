use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;
pub const TAG_COLORS: [&str; 8] = [
    "sage", "clay", "ocean", "lavender", "coral", "sand", "slate", "ember",
];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagDefinition {
    pub name: String,
    pub color: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct TagRegistryDocument {
    schema_version: u32,
    tags: Vec<TagDefinition>,
}

impl Default for TagRegistryDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            tags: Vec::new(),
        }
    }
}

pub struct TagRegistryStore {
    path: PathBuf,
    document: TagRegistryDocument,
}

impl TagRegistryStore {
    pub fn open(data_dir: &Path, known_tags: &BTreeSet<String>) -> Result<Self, String> {
        let path = data_dir.join("tags-v1.json");
        let (document, existed) = match fs::read_to_string(&path) {
            Ok(content) => {
                let document: TagRegistryDocument = serde_json::from_str(&content)
                    .map_err(|error| format!("标签注册表损坏，已停止写入: {error}"))?;
                if document.schema_version != SCHEMA_VERSION {
                    return Err(format!(
                        "不支持的标签注册表版本: {}",
                        document.schema_version
                    ));
                }
                if document
                    .tags
                    .iter()
                    .any(|tag| !TAG_COLORS.contains(&tag.color.as_str()))
                {
                    return Err("标签注册表包含不支持的颜色，已停止写入".to_string());
                }
                let unique_names = document
                    .tags
                    .iter()
                    .map(|tag| tag.name.as_str())
                    .collect::<BTreeSet<_>>();
                if unique_names.len() != document.tags.len() {
                    return Err("标签注册表包含重复名称，已停止写入".to_string());
                }
                (document, true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (TagRegistryDocument::default(), false)
            }
            Err(error) => return Err(error.to_string()),
        };

        let mut store = Self { path, document };
        let changed = store.ensure_tags(known_tags.iter().cloned())?;
        if !changed && !existed {
            store.save()?;
        }
        Ok(store)
    }

    pub fn definitions(&self) -> &[TagDefinition] {
        &self.document.tags
    }

    pub fn contains(&self, name: &str) -> bool {
        self.document.tags.iter().any(|tag| tag.name == name)
    }

    pub fn ensure_tags(&mut self, tags: impl IntoIterator<Item = String>) -> Result<bool, String> {
        let mut next = self.document.clone();
        let mut changed = false;
        for name in tags {
            if name.is_empty() || next.tags.iter().any(|tag| tag.name == name) {
                continue;
            }
            let color = TAG_COLORS[next.tags.len() % TAG_COLORS.len()].to_string();
            next.tags.push(TagDefinition {
                name,
                color,
                created_at: Utc::now().to_rfc3339(),
            });
            changed = true;
        }
        if changed {
            self.save_document(&next)?;
            self.document = next;
        }
        Ok(changed)
    }

    pub fn ensure_renamed_target(&mut self, source: &str, target: &str) -> Result<(), String> {
        if self.contains(target) {
            return Ok(());
        }
        let mut next = self.document.clone();
        let color = next
            .tags
            .iter()
            .find(|tag| tag.name == source)
            .map(|tag| tag.color.clone())
            .unwrap_or_else(|| TAG_COLORS[next.tags.len() % TAG_COLORS.len()].to_string());
        next.tags.push(TagDefinition {
            name: target.to_string(),
            color,
            created_at: Utc::now().to_rfc3339(),
        });
        self.save_document(&next)?;
        self.document = next;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let mut next = self.document.clone();
        let before = next.tags.len();
        next.tags.retain(|tag| tag.name != name);
        if next.tags.len() != before {
            self.save_document(&next)?;
            self.document = next;
        }
        Ok(())
    }

    pub fn set_color(&mut self, name: &str, color: &str) -> Result<(), String> {
        if !TAG_COLORS.contains(&color) {
            return Err("不支持的标签颜色".to_string());
        }
        let mut next = self.document.clone();
        let tag = next
            .tags
            .iter_mut()
            .find(|tag| tag.name == name)
            .ok_or_else(|| "标签不存在".to_string())?;
        if tag.color != color {
            tag.color = color.to_string();
            self.save_document(&next)?;
            self.document = next;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_document(&self.document)
    }

    fn save_document(&self, document: &TagRegistryDocument) -> Result<(), String> {
        let json = serde_json::to_string_pretty(document).map_err(|error| error.to_string())?;
        crate::config::atomic_write(&self.path, &json).map_err(|error| error.to_string())?;
        let content = fs::read_to_string(&self.path).map_err(|error| error.to_string())?;
        let reloaded: TagRegistryDocument =
            serde_json::from_str(&content).map_err(|error| error.to_string())?;
        if &reloaded != document {
            return Err("tag registry validation failed after write".to_string());
        }
        Ok(())
    }
}

pub fn validate_new_tag_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("标签名不能为空".to_string());
    }
    if name.chars().count() > 24 {
        return Err("标签名不能超过 24 个字符".to_string());
    }
    if name
        .chars()
        .any(|character| ['，', ',', '、', ';', '；'].contains(&character))
    {
        return Err("标签名不能包含分隔符".to_string());
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("monet-tags-v1-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn imports_known_tags_in_stable_order_and_preserves_colors() {
        let root = TestDir::new();
        let known = BTreeSet::from(["文档".to_string(), "测试".to_string()]);
        let first = TagRegistryStore::open(&root.0, &known).unwrap();
        let first_tags = first.definitions().to_vec();
        assert_eq!(
            first_tags
                .iter()
                .map(|tag| tag.name.as_str())
                .collect::<Vec<_>>(),
            vec!["文档", "测试"]
        );

        let reopened = TagRegistryStore::open(&root.0, &known).unwrap();
        assert_eq!(reopened.definitions(), first_tags);
    }

    #[test]
    fn corrupt_registry_is_never_rebuilt() {
        let root = TestDir::new();
        let path = root.0.join("tags-v1.json");
        fs::write(&path, "not-json").unwrap();

        assert!(TagRegistryStore::open(&root.0, &BTreeSet::new()).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "not-json");
    }

    #[test]
    fn rejects_invalid_manual_names() {
        assert_eq!(validate_new_tag_name("  文档  ").unwrap(), "文档");
        assert!(validate_new_tag_name("文档,测试").is_err());
        assert!(validate_new_tag_name("").is_err());
        assert!(validate_new_tag_name(&"长".repeat(25)).is_err());
    }
}
