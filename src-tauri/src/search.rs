//! 全局搜索引擎 —— 文本提取 + 落盘缓存 + 内存子串匹配
//!
//! 方案定调（2026-07-07 讨论）：可搜文本仅占 JSONL 总量约 6%（~85MB），
//! 此量级下暴力子串匹配毫秒级返回，任何倒排索引都是负优化；
//! 子串匹配对中文零分词障碍（FTS5 trigram 两字词硬伤 / jieba 召回黑洞均不存在）。
//!
//! 架构：
//! - 提取：user/assistant 的 text 块 + ai-title/custom-title，剔除 tool_result、
//!   thinking、sidechain 消息与 <system-reminder> 等注入噪音
//! - 缓存：内存 HashMap 为查询主体；落盘按项目分片存 data_dir()/search/v1/，
//!   mtime+size 判失效；写盘走 atomic_write（主 App 与 MCP 进程并发写安全）
//! - 增量：watcher 报变更只标 dirty，查询时懒重提取（天然去抖）
//! - 双进程：主 App 与 monet_mcp 二进制共用本模块（bin 侧 #[path] 引入），
//!   MCP 拉起时 warm() 对账 mtime 自补增量，不依赖主 App 存活
//!
//! 查询语义：空格分词多词 AND（会话级——每词在会话任意处出现即可），
//! 大小写不敏感；片段选包含命中的消息生成 KWIC，前端负责高亮。

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use base64::Engine as _;
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

// 主 App 与 monet_mcp bin 双端编译：bin 侧在其 crate root 以
// #[path = "../config.rs"] mod config; 提供同名模块，crate::config 两边都成立
use crate::config;

const CACHE_VERSION: u32 = 1;
/// 单会话返回的片段上限
const MAX_SNIPPETS: usize = 3;
/// 命中会话返回上限
const MAX_HITS: usize = 50;
/// KWIC 片段命中词前后的字符数
const KWIC_CONTEXT_CHARS: usize = 40;

// ── 缓存数据结构 ──────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct SearchMessage {
    pub uuid: Option<String>,
    /// 0 = user, 1 = assistant
    pub role: u8,
    pub timestamp: Option<String>,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub session_id: String,
    pub project_id: String,
    /// JSONL 内标题（custom-title 优先于 ai-title，同 parser::parse_summary 口径）
    pub title: Option<String>,
    pub mtime_secs: u64,
    pub mtime_nsec: u32,
    pub size: u64,
    /// 最后修改（秒，Unix epoch）——时间过滤与排序用
    pub last_modified: f64,
    pub messages: Vec<SearchMessage>,
}

/// 落盘分片：一个项目一个文件
#[derive(Serialize, Deserialize)]
struct ShardFile {
    version: u32,
    entries: HashMap<String, SessionEntry>,
}

// ── 增值元数据（轻量读，不依赖 metadata.rs 避免拖进 tauri）──

#[derive(Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaLite {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub deleted: Option<bool>,
}

#[derive(Default, Deserialize)]
struct MetadataV2Lite {
    #[serde(default)]
    instances: HashMap<String, InstanceMetaLite>,
}

#[derive(Default, Deserialize)]
struct InstanceMetaLite {
    #[serde(default)]
    sessions: HashMap<String, MetaLite>,
}

fn load_meta() -> HashMap<String, MetaLite> {
    let data_dir = config::data_dir();
    let v2_path = data_dir.join("metadata-v2.json");
    if v2_path.is_file() {
        let instance_key = format!(
            "ei1.{}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("claude-code"),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("default"),
        );
        return fs::read_to_string(v2_path)
            .ok()
            .and_then(|content| serde_json::from_str::<MetadataV2Lite>(&content).ok())
            .and_then(|mut document| document.instances.remove(&instance_key))
            .map(|instance| instance.sessions)
            .unwrap_or_default();
    }
    fs::read_to_string(data_dir.join("metadata.json"))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

// ── 引擎状态 ─────────────────────────────────────────────

#[derive(Default)]
struct EngineState {
    /// key = 会话 JSONL 绝对路径
    entries: HashMap<PathBuf, SessionEntry>,
    /// watcher 报来的待重提取文件（懒失效）
    dirty: HashSet<PathBuf>,
    /// 落盘分片是否有未写变更（按 project_id）
    dirty_shards: HashSet<String>,
    /// 首次 warm 是否完成
    ready: bool,
}

static ENGINE: Mutex<Option<EngineState>> = Mutex::new(None);

fn with_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut EngineState) -> R,
{
    let mut guard = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(EngineState::default);
    f(state)
}

// ── 路径与文件收集 ────────────────────────────────────────

fn projects_root() -> PathBuf {
    crate::config::projects_dir()
}

fn shards_dir() -> PathBuf {
    config::data_dir().join("search").join("v1")
}

fn file_stamp(meta: &fs::Metadata) -> (u64, u32, u64, f64) {
    let d = meta
        .modified()
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_secs(), d.subsec_nanos(), meta.len(), d.as_secs_f64())
}

/// 收集主会话 JSONL：projects/<pid>/<sid>.jsonl 两层结构，
/// 排除 agent- 前缀（与 watcher::session_file_ids 口径一致），不含 subagents 深层
fn collect_main_sessions() -> Vec<(PathBuf, String, String)> {
    let mut out = Vec::new();
    let root = projects_root();
    if !root.is_dir() {
        return out;
    }
    let Ok(projects) = fs::read_dir(&root) else {
        return out;
    };
    // 内置 Agent 工作目录不入索引（防旧版残留污染搜索）
    let agent_dirs = crate::config::agent_project_dirs();
    for project in projects.filter_map(|e| e.ok()) {
        let dir = project.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(pid) = dir.file_name().and_then(|n| n.to_str()).map(String::from) else {
            continue;
        };
        if agent_dirs.contains(&pid) {
            continue;
        }
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        for file in files.filter_map(|e| e.ok()) {
            let path = file.path();
            // map_or 而非 is_none_or:后者 1.82 才稳定,项目 MSRV 1.77.2
            if path.extension().map_or(true, |e| e != "jsonl") {
                continue;
            }
            let Some(sid) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else {
                continue;
            };
            if sid.starts_with("agent-") {
                continue;
            }
            out.push((path, pid.clone(), sid));
        }
    }
    out
}

// ── 文本提取 ─────────────────────────────────────────────

/// 轻量行解析：只反序列化搜索需要的字段
#[derive(Deserialize)]
struct LineExtract {
    #[serde(rename = "type")]
    record_type: Option<String>,
    uuid: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    message: Option<MsgExtract>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
}

#[derive(Deserialize)]
struct MsgExtract {
    content: Option<ContentExtract>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContentExtract {
    Text(String),
    Blocks(Vec<BlockExtract>),
}

#[derive(Deserialize)]
struct BlockExtract {
    #[serde(rename = "type")]
    block_type: Option<String>,
    text: Option<String>,
}

/// CLI 注入类噪音：这些块每会话重复出现，入索引会污染搜索信噪比
fn is_noise_text(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<system-reminder>")
        || t.starts_with("<command-name>")
        || t.starts_with("<local-command-stdout>")
        || t.starts_with("Caveat: The messages below were generated by the user")
}

/// 剥离 markdown 围栏代码块（```...```）：代码/CLI 输出/diff 对搜索价值极低，
/// 留着只会让"删除"命中 `rm -rf`、"修改"命中 `sed` 这类杂音。
/// 保留行内代码（`单反引号`）——通常是术语/变量名，有搜索价值。
fn strip_fenced_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// 从 message.content 拼接可搜文本（仅 text 块；string 形态直接取）
fn extract_text(content: &ContentExtract) -> String {
    match content {
        ContentExtract::Text(s) => {
            if is_noise_text(s) {
                String::new()
            } else {
                strip_fenced_code(s)
            }
        }
        ContentExtract::Blocks(blocks) => {
            let mut parts = Vec::new();
            for b in blocks {
                if b.block_type.as_deref() == Some("text") {
                    if let Some(t) = &b.text {
                        if !is_noise_text(t) {
                            parts.push(strip_fenced_code(t));
                        }
                    }
                }
            }
            parts.join("\n")
        }
    }
}

/// 提取单个会话文件为 SessionEntry。文件不可读时返回 None
fn extract_file(path: &Path, project_id: &str, session_id: &str) -> Option<SessionEntry> {
    let meta = fs::metadata(path).ok()?;
    let (mtime_secs, mtime_nsec, size, last_modified) = file_stamp(&meta);
    let file = File::open(path).ok()?;
    let reader = BufReader::with_capacity(64 * 1024, file);

    let mut messages = Vec::new();
    let mut ai_title: Option<String> = None;
    let mut custom_title: Option<String> = None;

    for line in reader.lines() {
        let Ok(line) = line else { break };
        // 预筛：非目标行直接跳过，省去大行（file-history-snapshot 等）的 JSON 解析
        let maybe_message =
            line.contains("\"type\":\"user\"") || line.contains("\"type\":\"assistant\"");
        let maybe_title = line.contains("\"ai-title\"") || line.contains("\"custom-title\"");
        if !maybe_message && !maybe_title {
            continue;
        }

        let Ok(ext) = serde_json::from_str::<LineExtract>(&line) else {
            continue;
        };
        match ext.record_type.as_deref() {
            Some("user") | Some("assistant") => {
                if ext.is_sidechain == Some(true) {
                    continue;
                }
                let Some(content) = ext.message.as_ref().and_then(|m| m.content.as_ref()) else {
                    continue;
                };
                let text = extract_text(content);
                if text.trim().is_empty() {
                    continue;
                }
                messages.push(SearchMessage {
                    uuid: ext.uuid,
                    role: if ext.record_type.as_deref() == Some("user") {
                        0
                    } else {
                        1
                    },
                    timestamp: ext.timestamp,
                    text,
                });
            }
            Some("ai-title") => ai_title = ext.ai_title.or(ai_title),
            Some("custom-title") => custom_title = ext.custom_title.or(custom_title),
            _ => {}
        }
    }

    Some(SessionEntry {
        session_id: session_id.to_string(),
        project_id: project_id.to_string(),
        title: custom_title.or(ai_title),
        mtime_secs,
        mtime_nsec,
        size,
        last_modified,
        messages,
    })
}

// ── 落盘分片 ─────────────────────────────────────────────

fn shard_path(project_id: &str) -> PathBuf {
    shards_dir().join(format!("{project_id}.json"))
}

fn load_shard(project_id: &str) -> HashMap<String, SessionEntry> {
    fs::read_to_string(shard_path(project_id))
        .ok()
        .and_then(|s| serde_json::from_str::<ShardFile>(&s).ok())
        .filter(|f| f.version == CACHE_VERSION)
        .map(|f| f.entries)
        .unwrap_or_default()
}

fn save_shard(project_id: &str, entries: &HashMap<String, SessionEntry>) {
    let shard = ShardFile {
        version: CACHE_VERSION,
        entries: entries.clone(),
    };
    if let Ok(json) = serde_json::to_string(&shard) {
        let _ = config::atomic_write(&shard_path(project_id), &json);
    }
}

/// 把 dirty 分片写盘（在锁外序列化会话数据的拷贝，避免长时间持锁）
fn flush_dirty_shards() {
    let to_write: Vec<(String, HashMap<String, SessionEntry>)> = with_state(|s| {
        let shard_ids: Vec<String> = s.dirty_shards.drain().collect();
        shard_ids
            .into_iter()
            .map(|pid| {
                let entries: HashMap<String, SessionEntry> = s
                    .entries
                    .values()
                    .filter(|e| e.project_id == pid)
                    .map(|e| (e.session_id.clone(), e.clone()))
                    .collect();
                (pid, entries)
            })
            .collect()
    });
    for (pid, entries) in to_write {
        save_shard(&pid, &entries);
    }
}

// ── Warm（启动构建/对账）─────────────────────────────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatus {
    /// building | ready
    pub state: String,
    pub indexed_sessions: usize,
    pub total_sessions: usize,
}

/// 启动构建：读分片 → stat 全部主会话 → 重提取过期项 → 写回变化分片。
/// 同步阻塞，调用方负责丢后台线程。幂等，可重复调用（对账语义）
pub fn warm() -> SearchStatus {
    let files = collect_main_sessions();
    let total = files.len();

    // 读盘分片（仅首次——已 ready 时直接走内存对账）
    let loaded: HashMap<PathBuf, SessionEntry> = {
        let already_ready = with_state(|s| s.ready);
        if already_ready {
            HashMap::new()
        } else {
            let pids: HashSet<&String> = files.iter().map(|(_, pid, _)| pid).collect();
            let root = projects_root();
            pids.into_iter()
                .flat_map(|pid| {
                    load_shard(pid).into_values().map(|e| {
                        let path = root
                            .join(&e.project_id)
                            .join(format!("{}.jsonl", e.session_id));
                        (path, e)
                    })
                })
                .collect()
        }
    };
    with_state(|s| {
        for (path, entry) in loaded {
            s.entries.entry(path).or_insert(entry);
        }
    });

    // mtime+size 对账：找出需要重提取的文件
    let stale: Vec<(PathBuf, String, String)> = {
        let stamps: HashMap<PathBuf, (u64, u32, u64)> = with_state(|s| {
            s.entries
                .iter()
                .map(|(p, e)| (p.clone(), (e.mtime_secs, e.mtime_nsec, e.size)))
                .collect()
        });
        files
            .iter()
            .filter(|(path, _, _)| {
                let Ok(meta) = fs::metadata(path) else {
                    return false;
                };
                let (secs, nanos, size, _) = file_stamp(&meta);
                stamps.get(path) != Some(&(secs, nanos, size))
            })
            .cloned()
            .collect()
    };

    // 并行重提取
    let fresh: Vec<(PathBuf, SessionEntry)> = stale
        .par_iter()
        .filter_map(|(path, pid, sid)| extract_file(path, pid, sid).map(|e| (path.clone(), e)))
        .collect();

    // 删除已消失的会话（文件被删/项目移除）
    let live: HashSet<PathBuf> = files.iter().map(|(p, _, _)| p.clone()).collect();

    let status = with_state(|s| {
        for (path, entry) in fresh {
            s.dirty_shards.insert(entry.project_id.clone());
            s.entries.insert(path, entry);
        }
        let removed: Vec<PathBuf> = s
            .entries
            .keys()
            .filter(|p| !live.contains(*p))
            .cloned()
            .collect();
        for p in removed {
            if let Some(e) = s.entries.remove(&p) {
                s.dirty_shards.insert(e.project_id);
            }
        }
        s.ready = true;
        SearchStatus {
            state: "ready".into(),
            indexed_sessions: s.entries.len(),
            total_sessions: total,
        }
    });

    flush_dirty_shards();
    status
}

#[allow(dead_code)] // lib 侧使用，部分 bin 编译时未引用
pub fn status() -> SearchStatus {
    with_state(|s| SearchStatus {
        state: if s.ready { "ready" } else { "building" }.into(),
        indexed_sessions: s.entries.len(),
        total_sessions: 0,
    })
}

/// 获取会话命中消息的完整文本 + 前后 context_n 条上下文（喂给归纳 Agent）。
/// 等价于 recall 的 ±3 上下文窗口。返回 (role_label, full_text) 列表。
#[allow(dead_code)] // lib 侧使用，部分 bin 编译时未引用
pub fn get_hit_context(
    session_id: &str,
    terms: &[Regex],
    context_n: usize,
    max_chars: usize,
) -> Vec<(String, String)> {
    with_state(|s| {
        let entry = s.entries.values().find(|e| e.session_id == session_id);
        let entry = match entry {
            Some(e) => e,
            None => return vec![],
        };

        let msgs = &entry.messages;
        let mut hit_indices: Vec<usize> = Vec::new();
        for (i, msg) in msgs.iter().enumerate() {
            if terms.iter().any(|t| t.is_match(&msg.text)) {
                hit_indices.push(i);
            }
        }

        let mut included = std::collections::BTreeSet::new();
        for &hi in &hit_indices {
            let lo = hi.saturating_sub(context_n);
            let hi_end = (hi + context_n + 1).min(msgs.len());
            for idx in lo..hi_end {
                included.insert(idx);
            }
        }

        let mut result = Vec::new();
        let mut total_chars = 0;
        let mut prev_idx: Option<usize> = None;
        for idx in &included {
            if let Some(p) = prev_idx {
                if *idx > p + 1 {
                    result.push(("---".to_string(), "".to_string()));
                }
            }
            prev_idx = Some(*idx);

            let msg = &msgs[*idx];
            let role = if msg.role == 0 { "用户" } else { "助手" };
            let is_hit = hit_indices.contains(idx);
            let marker = if is_hit { "▸ " } else { "  " };
            let label = format!("{marker}[{role}]");
            let text = msg.text.clone();
            total_chars += text.len();
            result.push((label, text));
            if total_chars > max_chars {
                break;
            }
        }
        result
    })
}

/// watcher 增量入口：只标 dirty，查询时懒重提取
#[allow(dead_code)] // lib 侧使用，部分 bin 编译时未引用
pub fn invalidate_file(path: &Path) {
    with_state(|s| {
        s.dirty.insert(path.to_path_buf());
    });
}

/// 查询前处理 dirty 集：重提取（或移除已删文件）
fn settle_dirty() {
    let dirty: Vec<PathBuf> = with_state(|s| s.dirty.drain().collect());
    if dirty.is_empty() {
        return;
    }
    let root = projects_root();
    let refreshed: Vec<(PathBuf, Option<SessionEntry>)> = dirty
        .par_iter()
        .filter_map(|path| {
            let pid = path.parent()?.file_name()?.to_str()?.to_string();
            let sid = path.file_stem()?.to_str()?.to_string();
            // 防御：只接受 projects root 直接子级结构
            if path.parent()?.parent()? != root {
                return None;
            }
            if path.exists() {
                Some((path.clone(), extract_file(path, &pid, &sid)))
            } else {
                Some((path.clone(), None))
            }
        })
        .collect();

    with_state(|s| {
        for (path, entry) in refreshed {
            match entry {
                Some(e) => {
                    s.dirty_shards.insert(e.project_id.clone());
                    s.entries.insert(path, e);
                }
                None => {
                    if let Some(old) = s.entries.remove(&path) {
                        s.dirty_shards.insert(old.project_id);
                    }
                }
            }
        }
    });
    flush_dirty_shards();
}

// ── 查询 ─────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilter {
    /// 限定项目（encoded project_id）
    #[serde(default)]
    pub project_id: Option<String>,
    /// 只看近 N 天（按会话 last_modified）
    #[serde(default)]
    pub days: Option<u32>,
    /// 只搜标题（含增值 title/tags/summary）
    #[serde(default)]
    pub title_only: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSnippet {
    pub uuid: Option<String>,
    pub role: u8,
    pub timestamp: Option<String>,
    pub text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub session_id: String,
    pub project_id: String,
    /// 解析后的展示标题：meta.title > JSONL title > 首条消息截断
    pub title: Option<String>,
    pub last_modified: f64,
    /// 命中来源：title / tags / summary / messages
    pub matched_in: Vec<String>,
    pub total_matches: usize,
    pub snippets: Vec<SearchSnippet>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total_hits: usize,
    pub elapsed_ms: u64,
}

/// 编译查询：空格分词 → 每词一个 CI regex（字面量转义）
pub fn compile_terms(query: &str) -> Vec<Regex> {
    query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .filter_map(|t| {
            regex::RegexBuilder::new(&regex::escape(t))
                .case_insensitive(true)
                .build()
                .ok()
        })
        .collect()
}

/// UTF-8 安全 KWIC：以首个命中位置为中心截前后文，边界对齐 char boundary
fn make_kwic(text: &str, first_match: std::ops::Range<usize>) -> String {
    let mut start = first_match.start;
    let mut taken = 0;
    while start > 0 && taken < KWIC_CONTEXT_CHARS {
        start -= 1;
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        taken += 1;
    }
    let mut end = first_match.end.min(text.len());
    let mut taken = 0;
    while end < text.len() && taken < KWIC_CONTEXT_CHARS * 2 {
        end += 1;
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }
        taken += 1;
    }
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    // 压掉换行，片段单行展示
    out.push_str(&text[start..end].replace('\n', " "));
    if end < text.len() {
        out.push('…');
    }
    out
}

/// 主查询入口。ready 前调用会先同步 warm（首查即建）
pub fn query(raw_query: &str, filter: &SearchFilter) -> SearchResult {
    let t0 = std::time::Instant::now();
    let terms = compile_terms(raw_query);
    if terms.is_empty() {
        return SearchResult {
            hits: vec![],
            total_hits: 0,
            elapsed_ms: 0,
        };
    }

    if !with_state(|s| s.ready) {
        warm();
    }
    settle_dirty();

    let meta_map = load_meta();
    let now_secs = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let cutoff = filter.days.map(|d| now_secs - d as f64 * 86400.0);

    // 快照会话列表引用做并行匹配（entries clone 代价高，锁内收集指针不安全，
    // 这里直接在锁内做匹配——匹配本身是纯 CPU，85MB 量级 <100ms，可接受）
    let mut hits: Vec<SearchHit> = with_state(|s| {
        s.entries
            .values()
            .collect::<Vec<_>>()
            .par_iter()
            .filter_map(|entry| {
                if let Some(pid) = &filter.project_id {
                    if &entry.project_id != pid {
                        return None;
                    }
                }
                if let Some(cut) = cutoff {
                    if entry.last_modified < cut {
                        return None;
                    }
                }
                let meta = meta_map.get(&entry.session_id);
                if meta.and_then(|m| m.deleted).unwrap_or(false) {
                    return None;
                }
                match_session(entry, meta, &terms, filter.title_only)
            })
            .collect()
    });

    hits.sort_unstable_by(|a, b| {
        b.last_modified
            .partial_cmp(&a.last_modified)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total_hits = hits.len();
    hits.truncate(MAX_HITS);

    SearchResult {
        hits,
        total_hits,
        elapsed_ms: t0.elapsed().as_millis() as u64,
    }
}

/// 会话级 AND 匹配：每个词须在会话任意可搜面出现（标题/tags/摘要/消息）。
/// 片段取包含命中的消息（优先包含更多词的），前 MAX_SNIPPETS 条
fn match_session(
    entry: &SessionEntry,
    meta: Option<&MetaLite>,
    terms: &[Regex],
    title_only: bool,
) -> Option<SearchHit> {
    let meta_title = meta.and_then(|m| m.title.as_deref());
    let tags_joined = meta
        .and_then(|m| m.tags.as_ref())
        .map(|t| t.join(" "))
        .unwrap_or_default();
    let summary = meta.and_then(|m| m.summary.as_deref()).unwrap_or("");
    let title = meta_title.or(entry.title.as_deref()).unwrap_or("");

    let mut matched_in: HashSet<&'static str> = HashSet::new();
    // 每个词的会话级命中标记
    let mut term_hit = vec![false; terms.len()];

    for (i, term) in terms.iter().enumerate() {
        if term.is_match(title) {
            term_hit[i] = true;
            matched_in.insert("title");
        }
        if term.is_match(&tags_joined) {
            term_hit[i] = true;
            matched_in.insert("tags");
        }
        if term.is_match(summary) {
            term_hit[i] = true;
            matched_in.insert("summary");
        }
    }

    // 消息面：统计每条消息命中的词数，并给会话级标记补位
    let mut msg_scores: Vec<(usize, usize, std::ops::Range<usize>)> = Vec::new(); // (msg_idx, n_terms, first_range)
    if !title_only {
        for (mi, msg) in entry.messages.iter().enumerate() {
            let mut n = 0;
            let mut first: Option<std::ops::Range<usize>> = None;
            for (i, term) in terms.iter().enumerate() {
                if let Some(m) = term.find(&msg.text) {
                    n += 1;
                    term_hit[i] = true;
                    if first.is_none() {
                        first = Some(m.range());
                    }
                }
            }
            if n > 0 {
                matched_in.insert("messages");
                msg_scores.push((mi, n, first.unwrap_or(0..0)));
            }
        }
    }

    if !term_hit.iter().all(|&h| h) {
        return None;
    }

    // 片段排序：user 消息加权（人的问题/需求比 AI 回复有更高搜索价值），
    // 再按命中词数、位置（靠后=更新）排序
    let user_bonus = |idx: usize| -> usize {
        if entry.messages[idx].role == 0 {
            100
        } else {
            0
        }
    };
    msg_scores.sort_unstable_by(|a, b| {
        (b.1 + user_bonus(b.0))
            .cmp(&(a.1 + user_bonus(a.0)))
            .then(b.0.cmp(&a.0))
    });
    let total_matches = msg_scores.len();
    let snippets: Vec<SearchSnippet> = msg_scores
        .into_iter()
        .take(MAX_SNIPPETS)
        .map(|(mi, _, range)| {
            let msg = &entry.messages[mi];
            SearchSnippet {
                uuid: msg.uuid.clone(),
                role: msg.role,
                timestamp: msg.timestamp.clone(),
                text: make_kwic(&msg.text, range),
            }
        })
        .collect();

    // 展示标题回退：首条 user 消息截断
    let display_title = if !title.is_empty() {
        Some(title.to_string())
    } else {
        entry.messages.iter().find(|m| m.role == 0).map(|m| {
            let mut t: String = m.text.chars().take(60).collect();
            if m.text.chars().count() > 60 {
                t.push('…');
            }
            t
        })
    };

    Some(SearchHit {
        session_id: entry.session_id.clone(),
        project_id: entry.project_id.clone(),
        title: display_title,
        last_modified: entry.last_modified,
        matched_in: matched_in.into_iter().map(String::from).collect(),
        total_matches,
        snippets,
    })
}

// ── 测试 ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_session(dir: &Path, sid: &str, lines: &[String]) -> PathBuf {
        let path = dir.join(format!("{sid}.jsonl"));
        let mut f = File::create(&path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        path
    }

    fn user_line(uuid: &str, text: &str) -> String {
        format!(
            "{{\"type\":\"user\",\"uuid\":\"{uuid}\",\"timestamp\":\"2026-07-01T10:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}"
        )
    }

    fn assistant_line(uuid: &str, text: &str) -> String {
        format!(
            "{{\"type\":\"assistant\",\"uuid\":\"{uuid}\",\"timestamp\":\"2026-07-01T10:00:05.000Z\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"{text}\"}},{{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{{}}}}]}}}}"
        )
    }

    /// 提取：user 字符串 content、assistant text 块、标题、噪音剔除、sidechain 剔除
    #[test]
    fn extract_basics() {
        let dir = std::env::temp_dir().join("monet-search-test-extract");
        fs::create_dir_all(&dir).unwrap();
        let lines = vec![
            user_line("u1", "修复流式渲染的死锁问题"),
            assistant_line("a1", "找到根因了，是持锁重入"),
            // tool_result 回传（user 行、blocks 无 text 块）→ 不入索引
            "{\"type\":\"user\",\"uuid\":\"u2\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"secret-tool-output\"}]}}".to_string(),
            // system-reminder 噪音 → 剔除
            user_line("u3", "<system-reminder>注入的上下文</system-reminder>"),
            // sidechain → 剔除
            "{\"type\":\"user\",\"uuid\":\"u4\",\"isSidechain\":true,\"message\":{\"role\":\"user\",\"content\":\"sidechain 内部消息\"}}".to_string(),
            "{\"type\":\"ai-title\",\"aiTitle\":\"流式死锁修复\"}".to_string(),
        ];
        let path = write_session(&dir, "s1", &lines);
        let entry = extract_file(&path, "p1", "s1").unwrap();

        assert_eq!(entry.title.as_deref(), Some("流式死锁修复"));
        assert_eq!(entry.messages.len(), 2);
        assert_eq!(entry.messages[0].uuid.as_deref(), Some("u1"));
        assert!(entry.messages[1].text.contains("持锁重入"));
        fs::remove_dir_all(&dir).ok();
    }

    /// KWIC：UTF-8 多字节边界安全 + 上下文截断
    #[test]
    fn kwic_utf8_safety() {
        let text = "前".repeat(100) + "命中词" + &"后".repeat(100);
        let pos = text.find("命中词").unwrap();
        let kwic = make_kwic(&text, pos..pos + "命中词".len());
        assert!(kwic.contains("命中词"));
        assert!(kwic.starts_with('…') && kwic.ends_with('…'));
        // 不 panic 即通过边界检查；长度约 40+3+80 字符量级
        assert!(kwic.chars().count() < 130);
    }

    /// 查询语义：会话级 AND（词分布在不同消息）、大小写不敏感、中文两字词
    #[test]
    fn query_semantics() {
        let entry = SessionEntry {
            session_id: "s1".into(),
            project_id: "p1".into(),
            title: None,
            mtime_secs: 0,
            mtime_nsec: 0,
            size: 0,
            last_modified: 0.0,
            messages: vec![
                SearchMessage {
                    uuid: Some("u1".into()),
                    role: 0,
                    timestamp: None,
                    text: "流式渲染出现闪烁".into(),
                },
                SearchMessage {
                    uuid: Some("a1".into()),
                    role: 1,
                    timestamp: None,
                    text: "定位到 useStreaming 的问题".into(),
                },
            ],
        };
        let terms = compile_terms("闪烁 USESTREAMING");
        let hit = match_session(&entry, None, &terms, false).expect("跨消息 AND 应命中");
        assert_eq!(hit.total_matches, 2);
        assert!(hit.matched_in.contains(&"messages".to_string()));

        // 缺词不命中
        let terms = compile_terms("闪烁 不存在的词");
        assert!(match_session(&entry, None, &terms, false).is_none());

        // title_only 时消息面不参与
        let terms = compile_terms("闪烁");
        assert!(match_session(&entry, None, &terms, true).is_none());
    }

    /// 联机 smoke：本机真实 ~/.claude/projects 全量建缓存 + 查询计时。
    /// 缓存写入重定向到临时目录。不进常规跑：cargo test -- --ignored --nocapture
    #[test]
    #[ignore]
    fn smoke_real_data() {
        let tmp = std::env::temp_dir().join("monet-search-smoke");
        std::env::set_var("MONET_DATA_DIR", &tmp);

        let t0 = std::time::Instant::now();
        let status = warm();
        let warm_cold = t0.elapsed();

        let t1 = std::time::Instant::now();
        warm();
        let warm_hot = t1.elapsed();

        let (n_msgs, text_bytes) = with_state(|s| {
            let n: usize = s.entries.values().map(|e| e.messages.len()).sum();
            let b: usize = s
                .entries
                .values()
                .flat_map(|e| &e.messages)
                .map(|m| m.text.len())
                .sum();
            (n, b)
        });

        let t2 = std::time::Instant::now();
        let r1 = query("死锁", &SearchFilter::default());
        let q1 = t2.elapsed();
        let t3 = std::time::Instant::now();
        let r2 = query("流式 闪烁", &SearchFilter::default());
        let q2 = t3.elapsed();

        println!(
            "warm(cold): {warm_cold:?} · warm(hot): {warm_hot:?} · sessions: {} · msgs: {n_msgs} · text: {:.1}MB",
            status.indexed_sessions,
            text_bytes as f64 / 1e6
        );
        println!(
            "query 死锁: {q1:?} → {} hits · query 流式+闪烁: {q2:?} → {} hits",
            r1.total_hits, r2.total_hits
        );
        fs::remove_dir_all(&tmp).ok();
    }

    /// 增值元数据：meta.title/tags 命中 + deleted 过滤在 query 层（此处验证 title 优先级）
    #[test]
    fn meta_title_and_tags() {
        let entry = SessionEntry {
            session_id: "s1".into(),
            project_id: "p1".into(),
            title: Some("JSONL 内标题".into()),
            mtime_secs: 0,
            mtime_nsec: 0,
            size: 0,
            last_modified: 0.0,
            messages: vec![],
        };
        let meta = MetaLite {
            title: Some("增值标题：搜索引擎设计".into()),
            tags: Some(vec!["架构".into(), "性能".into()]),
            summary: None,
            deleted: None,
        };
        let terms = compile_terms("搜索引擎");
        let hit = match_session(&entry, Some(&meta), &terms, false).unwrap();
        assert_eq!(hit.title.as_deref(), Some("增值标题：搜索引擎设计"));
        assert!(hit.matched_in.contains(&"title".to_string()));

        let terms = compile_terms("性能");
        let hit = match_session(&entry, Some(&meta), &terms, false).unwrap();
        assert!(hit.matched_in.contains(&"tags".to_string()));
    }
}
