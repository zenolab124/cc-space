use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::engines::core::{ActionAvailability, SessionActions, SessionRef};

const SCHEMA_VERSION: u32 = 1;
const STORE_FILE: &str = "worktree-sessions-v1.json";
const CWD_UNAVAILABLE_REASON: &str = "worktreeSession.cwdUnavailable";

static STORE: Mutex<Option<WorkspaceStore>> = Mutex::new(None);
static CONTEXTS: Mutex<Option<HashMap<SessionRef, WorkspaceContext>>> = Mutex::new(None);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceKind {
    Primary,
    Linked,
    Legacy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceDetection {
    Git,
    Persisted,
    Convention,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContext {
    pub kind: WorkspaceKind,
    pub worktree_root: String,
    pub main_root: Option<String>,
    pub name: Option<String>,
    pub branch: Option<String>,
    pub available: bool,
    pub main_available: bool,
    pub detected_by: WorkspaceDetection,
    #[serde(skip)]
    common_dir: Option<String>,
}

impl WorkspaceContext {
    pub fn is_legacy_unavailable(&self) -> bool {
        self.kind == WorkspaceKind::Legacy && !self.available
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceResolveRequest {
    pub session: SessionRef,
    pub cwd: Option<String>,
    pub project_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceResolveResult {
    pub session: SessionRef,
    pub context: Option<WorkspaceContext>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedWorkspace {
    session: SessionRef,
    worktree_root: String,
    main_root: String,
    name: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    common_dir: Option<String>,
    last_seen_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceDocument {
    schema: u32,
    records: BTreeMap<String, PersistedWorkspace>,
}

struct WorkspaceStore {
    path: PathBuf,
    document: WorkspaceDocument,
    writable: bool,
    dirty: bool,
}

impl WorkspaceStore {
    fn open(data_dir: &Path) -> Result<Self, String> {
        let path = data_dir.join(STORE_FILE);
        if !path.exists() {
            return Ok(Self {
                path,
                document: WorkspaceDocument {
                    schema: SCHEMA_VERSION,
                    records: BTreeMap::new(),
                },
                writable: true,
                dirty: false,
            });
        }

        let raw = std::fs::read_to_string(&path)
            .map_err(|error| format!("Worktree 关系文件读取失败: {error}"))?;
        match serde_json::from_str::<WorkspaceDocument>(&raw) {
            Ok(document) if document.schema == SCHEMA_VERSION => Ok(Self {
                path,
                document,
                writable: true,
                dirty: false,
            }),
            Ok(_) | Err(_) => {
                // 损坏或未知版本时只允许活态解析，绝不以空文档覆盖原文件。
                Ok(Self {
                    path,
                    document: WorkspaceDocument {
                        schema: SCHEMA_VERSION,
                        records: BTreeMap::new(),
                    },
                    writable: false,
                    dirty: false,
                })
            }
        }
    }

    fn persisted(&self, session: &SessionRef) -> Option<&PersistedWorkspace> {
        self.document.records.get(&session.storage_key())
    }

    fn remember(&mut self, session: &SessionRef, context: &WorkspaceContext) {
        if !self.writable || context.kind != WorkspaceKind::Linked {
            return;
        }
        let Some(main_root) = &context.main_root else {
            return;
        };
        let key = session.storage_key();
        let now = chrono::Utc::now();
        if let Some(previous) = self.document.records.get(&key) {
            let same_relation = previous.session.eq(session)
                && previous.worktree_root == context.worktree_root
                && previous.main_root == main_root.as_str()
                && previous.name.as_deref() == context.name.as_deref()
                && previous.branch.as_deref() == context.branch.as_deref()
                && previous.common_dir.as_deref() == context.common_dir.as_deref();
            let recently_seen = chrono::DateTime::parse_from_rfc3339(&previous.last_seen_at)
                .ok()
                .is_some_and(|seen| now.signed_duration_since(seen.to_utc()).num_minutes() < 5);
            if same_relation && recently_seen {
                return;
            }
        }
        let record = PersistedWorkspace {
            session: session.clone(),
            worktree_root: context.worktree_root.clone(),
            main_root: main_root.clone(),
            name: context.name.clone(),
            branch: context.branch.clone(),
            common_dir: context.common_dir.clone(),
            last_seen_at: now.to_rfc3339(),
        };
        if self.document.records.get(&key) != Some(&record) {
            self.document.records.insert(key, record);
            self.dirty = true;
        }
    }

    fn commit(&mut self) -> Result<(), String> {
        if !self.writable || !self.dirty {
            return Ok(());
        }
        let json = serde_json::to_string_pretty(&self.document)
            .map_err(|error| format!("Worktree 关系序列化失败: {error}"))?;
        crate::config::atomic_write(&self.path, &json)
            .map_err(|error| format!("Worktree 关系写入失败: {error}"))?;
        let verified = std::fs::read_to_string(&self.path)
            .map_err(|error| format!("Worktree 关系回读失败: {error}"))?;
        let verified: WorkspaceDocument = serde_json::from_str(&verified)
            .map_err(|error| format!("Worktree 关系回读校验失败: {error}"))?;
        if verified != self.document {
            return Err("Worktree 关系写入校验不一致".to_string());
        }
        self.dirty = false;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct WorktreeEntry {
    root: PathBuf,
    branch: Option<String>,
}

#[tauri::command]
pub async fn resolve_workspace_contexts(
    requests: Vec<WorkspaceResolveRequest>,
) -> Result<Vec<WorkspaceResolveResult>, String> {
    tauri::async_runtime::spawn_blocking(move || resolve_batch(requests))
        .await
        .map_err(|error| error.to_string())?
}

fn resolve_batch(
    requests: Vec<WorkspaceResolveRequest>,
) -> Result<Vec<WorkspaceResolveResult>, String> {
    let mut store_guard = STORE.lock().unwrap_or_else(|error| error.into_inner());
    if store_guard.is_none() {
        *store_guard = Some(WorkspaceStore::open(crate::config::data_dir())?);
    }
    let store = store_guard.as_mut().expect("workspace store initialized");
    let mut cwd_cache = HashMap::<String, Option<WorkspaceContext>>::new();
    let mut contexts = HashMap::new();
    let mut results = Vec::with_capacity(requests.len());

    for request in requests {
        let context = resolve_request(&request, store, &mut cwd_cache);
        if let Some(value) = &context {
            store.remember(&request.session, value);
            contexts.insert(request.session.clone(), value.clone());
        }
        results.push(WorkspaceResolveResult {
            session: request.session,
            context,
        });
    }
    store.commit()?;
    *CONTEXTS.lock().unwrap_or_else(|error| error.into_inner()) = Some(contexts);
    Ok(results)
}

fn resolve_request(
    request: &WorkspaceResolveRequest,
    store: &WorkspaceStore,
    cwd_cache: &mut HashMap<String, Option<WorkspaceContext>>,
) -> Option<WorkspaceContext> {
    for candidate in [request.project_path.as_deref(), request.cwd.as_deref()]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
    {
        let cached = cwd_cache
            .entry(candidate.to_string())
            .or_insert_with(|| detect_live_workspace(Path::new(candidate)));
        if let Some(context) = cached.clone() {
            if let Some(persisted) = store.persisted(&request.session) {
                if let (Some(expected), Some(actual)) = (&persisted.common_dir, &context.common_dir)
                {
                    if !same_path(Path::new(expected), Path::new(actual)) {
                        continue;
                    }
                }
            }
            return Some(context);
        }
    }

    if let Some(record) = store.persisted(&request.session) {
        return Some(WorkspaceContext {
            kind: WorkspaceKind::Legacy,
            worktree_root: record.worktree_root.clone(),
            main_root: Some(record.main_root.clone()),
            name: record.name.clone(),
            branch: record.branch.clone(),
            available: false,
            main_available: Path::new(&record.main_root).is_dir(),
            detected_by: WorkspaceDetection::Persisted,
            common_dir: record.common_dir.clone(),
        });
    }

    request
        .project_path
        .as_deref()
        .and_then(detect_convention_workspace)
        .or_else(|| request.cwd.as_deref().and_then(detect_convention_workspace))
}

fn detect_live_workspace(cwd: &Path) -> Option<WorkspaceContext> {
    if !cwd.is_dir() {
        return None;
    }
    let top_level = git_path(cwd, &["rev-parse", "--show-toplevel"])?;
    let common_dir = git_path(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let entries = git_worktrees(&top_level)?;
    let primary = entries.first()?;
    let primary_common_dir = git_path(
        &primary.root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    if !same_path(&common_dir, &primary_common_dir) {
        return None;
    }
    let current = entries
        .iter()
        .find(|entry| same_path(&entry.root, &top_level))?;
    let linked = !same_path(&current.root, &primary.root);
    Some(WorkspaceContext {
        kind: if linked {
            WorkspaceKind::Linked
        } else {
            WorkspaceKind::Primary
        },
        worktree_root: display_path(&current.root),
        main_root: Some(display_path(&primary.root)),
        name: if linked {
            current
                .root
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        } else {
            None
        },
        branch: current.branch.clone(),
        available: true,
        main_available: primary.root.is_dir(),
        detected_by: WorkspaceDetection::Git,
        common_dir: Some(display_path(&common_dir)),
    })
}

fn git_path(cwd: &Path, args: &[&str]) -> Option<PathBuf> {
    let output = git_output(cwd, args)?;
    let raw = String::from_utf8(output).ok()?;
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    Some(normalize_existing_path(&path))
}

fn git_worktrees(cwd: &Path) -> Option<Vec<WorktreeEntry>> {
    let output = git_output(cwd, &["worktree", "list", "--porcelain", "-z"])?;
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for field in output.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }
        let field = String::from_utf8_lossy(field);
        if let Some(path) = field.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                root: normalize_existing_path(Path::new(path)),
                branch: None,
            });
        } else if let Some(branch) = field.strip_prefix("branch ") {
            if let Some(entry) = current.as_mut() {
                entry.branch = Some(
                    branch
                        .strip_prefix("refs/heads/")
                        .unwrap_or(branch)
                        .to_string(),
                );
            }
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    (!entries.is_empty()).then_some(entries)
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("PATH", crate::path_env::enhanced_path())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn detect_convention_workspace(cwd: &str) -> Option<WorkspaceContext> {
    let path = Path::new(cwd);
    let components = path.components().collect::<Vec<_>>();
    let marker = components.windows(2).position(|pair| {
        matches!(pair[0], Component::Normal(value) if value == ".claude")
            && matches!(pair[1], Component::Normal(value) if value == "worktrees")
    })?;
    let name = match components.get(marker + 2)? {
        Component::Normal(value) => value.to_string_lossy().to_string(),
        _ => return None,
    };
    let mut main_root = PathBuf::new();
    for component in &components[..marker] {
        main_root.push(component.as_os_str());
    }
    if !main_root.is_dir() {
        return None;
    }
    let mut worktree_root = main_root.clone();
    worktree_root.push(".claude");
    worktree_root.push("worktrees");
    worktree_root.push(&name);
    Some(WorkspaceContext {
        kind: WorkspaceKind::Legacy,
        worktree_root: display_path(&worktree_root),
        main_root: Some(display_path(&normalize_existing_path(&main_root))),
        name: Some(name),
        branch: None,
        available: false,
        main_available: true,
        detected_by: WorkspaceDetection::Convention,
        common_dir: None,
    })
}

fn normalize_existing_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(windows)]
fn same_path(left: &Path, right: &Path) -> bool {
    display_path(left).eq_ignore_ascii_case(&display_path(right))
}

#[cfg(not(windows))]
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

pub fn restrict_session_actions(session: &SessionRef, actions: &mut SessionActions) {
    let context = CONTEXTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .and_then(|contexts| contexts.get(session).cloned());
    if !context.is_some_and(|value| value.is_legacy_unavailable()) {
        return;
    }
    actions.resume = ActionAvailability::unavailable(CWD_UNAVAILABLE_REASON);
    actions.fork = ActionAvailability::unavailable(CWD_UNAVAILABLE_REASON);
    actions.send = ActionAvailability::unavailable(CWD_UNAVAILABLE_REASON);
    actions.send_while_running = ActionAvailability::unavailable(CWD_UNAVAILABLE_REASON);
    actions.open_cwd = ActionAvailability::unavailable(CWD_UNAVAILABLE_REASON);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convention_fallback_uses_path_components() {
        let root =
            std::env::temp_dir().join(format!("monet-workspace-convention-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let cwd = root.join(".claude/worktrees/topic/src");
        let context = detect_convention_workspace(&display_path(&cwd)).unwrap();
        let expected_main = display_path(&root.canonicalize().unwrap());
        assert_eq!(context.kind, WorkspaceKind::Legacy);
        assert_eq!(context.name.as_deref(), Some("topic"));
        assert_eq!(context.main_root.as_deref(), Some(expected_main.as_str()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupted_store_is_read_only() {
        let root =
            std::env::temp_dir().join(format!("monet-workspace-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(STORE_FILE), "not-json").unwrap();
        let store = WorkspaceStore::open(&root).unwrap();
        assert!(!store.writable);
        assert!(store.document.records.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
