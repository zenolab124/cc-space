use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::models::SessionSummary;
use crate::parser::{self, UsageLedger};

// v2: CachedContrib 从 tokens 总量改为四类分量（成本分价计算需要），旧缓存整体重扫
// v3: SessionSummary 增加 subagent_tokens 分项且 total_tokens 口径改为含子 Agent，旧缓存整体重扫
// v4: usage 按 message.id 选择最终完整快照，子 Agent 缓存保留账本以支持跨文件去重
// v5: Codex 子 Agent 的继承历史前缀不再重复计入用量，旧缓存整体重扫
const CACHE_VERSION: u32 = 5;

// ── Disk format ──────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct DiskCache {
    version: u32,
    summaries: HashMap<String, DiskSummaryEntry>,
    #[serde(default)]
    usages: HashMap<String, DiskUsageEntry>,
    /// 子 Agent 转录文件 → token 用量。子 Agent 跑完后文件不再变，命中率极高
    #[serde(default)]
    subs: HashMap<String, DiskSubEntry>,
}

#[derive(Serialize, Deserialize)]
struct DiskSummaryEntry {
    mtime_secs: u64,
    mtime_nsec: u32,
    size: u64,
    summary: SessionSummary,
}

#[derive(Serialize, Deserialize)]
struct DiskUsageEntry {
    mtime_secs: u64,
    mtime_nsec: u32,
    size: u64,
    usage: CachedUsage,
}

#[derive(Serialize, Deserialize)]
struct DiskSubEntry {
    mtime_secs: u64,
    mtime_nsec: u32,
    size: u64,
    ledger: UsageLedger,
}

// ── Public types (usage_stats 消费) ──────────────────────

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CachedUsage {
    pub by_id: Vec<(String, CachedContrib)>,
    pub anon: Vec<CachedContrib>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedContrib {
    pub date: String,
    pub model: Option<String>,
    pub snapshot: crate::models::UsageSnapshot,
}

// ── In-memory state ──────────────────────────────────────

struct SummaryEntry {
    mtime_secs: u64,
    mtime_nsec: u32,
    size: u64,
    summary: SessionSummary,
}

struct UsageEntry {
    mtime_secs: u64,
    mtime_nsec: u32,
    size: u64,
    usage: CachedUsage,
}

struct SubEntry {
    mtime_secs: u64,
    mtime_nsec: u32,
    size: u64,
    ledger: UsageLedger,
}

struct CacheState {
    summaries: HashMap<PathBuf, SummaryEntry>,
    usages: HashMap<PathBuf, UsageEntry>,
    subs: HashMap<PathBuf, SubEntry>,
    paths: HashMap<String, String>,
    dirty: bool,
}

static CACHE: OnceLock<Mutex<CacheState>> = OnceLock::new();

// ── Private helpers ──────────────────────────────────────

fn cache_file_path() -> Option<PathBuf> {
    Some(
        crate::config::claude_root()
            .join("cc-space")
            .join("cache")
            .join("sessions.json"),
    )
}

fn file_stamp(meta: &fs::Metadata) -> (u64, u32, u64) {
    let d = meta
        .modified()
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_secs(), d.subsec_nanos(), meta.len())
}

fn load_from_disk() -> CacheState {
    let (summaries, usages, subs) = cache_file_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|data| serde_json::from_str::<DiskCache>(&data).ok())
        .filter(|dc| dc.version == CACHE_VERSION)
        .map(|dc| {
            let summaries = dc
                .summaries
                .into_iter()
                .map(|(k, v)| {
                    (
                        PathBuf::from(k),
                        SummaryEntry {
                            mtime_secs: v.mtime_secs,
                            mtime_nsec: v.mtime_nsec,
                            size: v.size,
                            summary: v.summary,
                        },
                    )
                })
                .collect();
            let usages = dc
                .usages
                .into_iter()
                .map(|(k, v)| {
                    (
                        PathBuf::from(k),
                        UsageEntry {
                            mtime_secs: v.mtime_secs,
                            mtime_nsec: v.mtime_nsec,
                            size: v.size,
                            usage: v.usage,
                        },
                    )
                })
                .collect();
            let subs = dc
                .subs
                .into_iter()
                .map(|(k, v)| {
                    (
                        PathBuf::from(k),
                        SubEntry {
                            mtime_secs: v.mtime_secs,
                            mtime_nsec: v.mtime_nsec,
                            size: v.size,
                            ledger: v.ledger,
                        },
                    )
                })
                .collect();
            (summaries, usages, subs)
        })
        .unwrap_or_default();

    CacheState {
        summaries,
        usages,
        subs,
        paths: HashMap::new(),
        dirty: false,
    }
}

fn state() -> &'static Mutex<CacheState> {
    CACHE.get_or_init(|| Mutex::new(load_from_disk()))
}

// ── Public API: summary ──────────────────────────────────

pub fn get_summary(path: &Path) -> Option<SessionSummary> {
    let meta = fs::metadata(path).ok()?;
    let (secs, nanos, size) = file_stamp(&meta);

    if let Ok(cache) = state().lock() {
        if let Some(e) = cache.summaries.get(path) {
            if e.mtime_secs == secs && e.mtime_nsec == nanos && e.size == size {
                return Some(e.summary.clone());
            }
        }
    }

    let (mut summary, mut total_ledger) = parser::parse_summary_with_usage(path, 50)?;

    // 主会话与子 Agent/工作流先按 message.id 合并，再求和，避免转发或复制记录重复计数。
    let sub_ledger = collect_subagent_usage(path);
    let subagent_tokens = sub_ledger.total();
    total_ledger.merge(sub_ledger);
    summary.total_tokens = total_ledger.total();
    summary.subagent_tokens = subagent_tokens;

    if let Ok(mut cache) = state().lock() {
        cache.summaries.insert(
            path.to_path_buf(),
            SummaryEntry {
                mtime_secs: secs,
                mtime_nsec: nanos,
                size,
                summary: summary.clone(),
            },
        );
        cache.dirty = true;
    }
    Some(summary)
}

/// 累计一个会话全部子 Agent 转录的 token 用量。
/// 覆盖直接子 Agent（subagents/agent-*.jsonl）与工作流
/// （subagents/workflows/<run>/agent-*.jsonl），逐文件走 (mtime, size) 缓存。
fn collect_subagent_usage(main_path: &Path) -> UsageLedger {
    let mut ledger = UsageLedger::default();
    let Some(stem) = main_path.file_stem() else {
        return ledger;
    };
    let Some(parent) = main_path.parent() else {
        return ledger;
    };
    let subagents_dir = parent.join(stem).join("subagents");

    let mut scan = |dir: &Path| {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("agent-") && name.ends_with(".jsonl") {
                ledger.merge(sub_file_usage(&path));
            }
        }
    };

    scan(&subagents_dir);

    let workflows_dir = subagents_dir.join("workflows");
    if let Ok(wf_entries) = fs::read_dir(&workflows_dir) {
        for wf_entry in wf_entries.flatten() {
            if wf_entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                scan(&wf_entry.path());
            }
        }
    }

    ledger
}

/// 单个子 Agent 文件的 usage 账本，(mtime, size) 命中则免解析
fn sub_file_usage(path: &Path) -> UsageLedger {
    let Ok(meta) = fs::metadata(path) else {
        return UsageLedger::default();
    };
    let (secs, nanos, size) = file_stamp(&meta);

    if let Ok(cache) = state().lock() {
        if let Some(e) = cache.subs.get(path) {
            if e.mtime_secs == secs && e.mtime_nsec == nanos && e.size == size {
                return e.ledger.clone();
            }
        }
    }

    let ledger = parser::parse_usage_ledger(path);
    if let Ok(mut cache) = state().lock() {
        cache.subs.insert(
            path.to_path_buf(),
            SubEntry {
                mtime_secs: secs,
                mtime_nsec: nanos,
                size,
                ledger: ledger.clone(),
            },
        );
        cache.dirty = true;
    }
    ledger
}

// ── Public API: usage ────────────────────────────────────

pub fn get_usage(path: &Path) -> Option<CachedUsage> {
    let meta = fs::metadata(path).ok()?;
    let (secs, nanos, size) = file_stamp(&meta);

    if let Ok(cache) = state().lock() {
        if let Some(e) = cache.usages.get(path) {
            if e.mtime_secs == secs && e.mtime_nsec == nanos && e.size == size {
                return Some(e.usage.clone());
            }
        }
    }
    None
}

pub fn set_usage(path: &Path, usage: CachedUsage) {
    let Ok(meta) = fs::metadata(path) else { return };
    let (secs, nanos, size) = file_stamp(&meta);

    if let Ok(mut cache) = state().lock() {
        cache.usages.insert(
            path.to_path_buf(),
            UsageEntry {
                mtime_secs: secs,
                mtime_nsec: nanos,
                size,
                usage,
            },
        );
        cache.dirty = true;
    }
}

// ── Public API: flush to disk ────────────────────────────

pub fn flush() {
    let json = {
        let mut cache = match state().lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        if !cache.dirty {
            return;
        }
        cache.dirty = false;

        let disk = DiskCache {
            version: CACHE_VERSION,
            summaries: cache
                .summaries
                .iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        DiskSummaryEntry {
                            mtime_secs: v.mtime_secs,
                            mtime_nsec: v.mtime_nsec,
                            size: v.size,
                            summary: v.summary.clone(),
                        },
                    )
                })
                .collect(),
            usages: cache
                .usages
                .iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        DiskUsageEntry {
                            mtime_secs: v.mtime_secs,
                            mtime_nsec: v.mtime_nsec,
                            size: v.size,
                            usage: v.usage.clone(),
                        },
                    )
                })
                .collect(),
            subs: cache
                .subs
                .iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        DiskSubEntry {
                            mtime_secs: v.mtime_secs,
                            mtime_nsec: v.mtime_nsec,
                            size: v.size,
                            ledger: v.ledger.clone(),
                        },
                    )
                })
                .collect(),
        };

        match serde_json::to_string(&disk) {
            Ok(s) => Some(s),
            Err(_) => {
                cache.dirty = true;
                None
            }
        }
    };

    if let Some(json) = json {
        if let Some(path) = cache_file_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, json);
        }
    }
}

// ── Public API: path decode cache ────────────────────────

pub fn get_decoded_path(encoded: &str, decode_fn: impl FnOnce(&str) -> String) -> String {
    if let Ok(cache) = state().lock() {
        if let Some(decoded) = cache.paths.get(encoded) {
            return decoded.clone();
        }
    }

    let decoded = decode_fn(encoded);
    if let Ok(mut cache) = state().lock() {
        cache.paths.insert(encoded.to_string(), decoded.clone());
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn assistant_line(id: &str, output_tokens: u64) -> String {
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"2026-06-11T10:00:00.000Z\",\"message\":{{\"id\":\"{id}\",\"model\":\"claude-fable-5\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":{output_tokens},\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}}}}"
        )
    }

    /// 会话总消耗口径：total_tokens = 主转录 + 直接子 Agent + 工作流子 Agent，
    /// subagent_tokens 为子 Agent 分项
    #[test]
    fn summary_includes_subagent_tokens() {
        let root = std::env::temp_dir().join("monet-test-sub-cache");
        let session_dir = root.join("sess-1").join("subagents");
        let wf_dir = session_dir.join("workflows").join("wf_abc123");
        fs::create_dir_all(&wf_dir).unwrap();

        let main_path = root.join("sess-1.jsonl");
        let mut f = fs::File::create(&main_path).unwrap();
        writeln!(f, "{}", assistant_line("msg_main", 90)).unwrap();
        drop(f);

        let mut f = fs::File::create(session_dir.join("agent-a1.jsonl")).unwrap();
        writeln!(f, "{}", assistant_line("msg_sub_a", 190)).unwrap();
        drop(f);
        // meta.json 与非 agent 前缀文件不参与累计
        fs::write(session_dir.join("agent-a1.meta.json"), "{}").unwrap();

        let mut f = fs::File::create(wf_dir.join("agent-b2.jsonl")).unwrap();
        writeln!(f, "{}", assistant_line("msg_sub_b", 40)).unwrap();
        drop(f);

        let summary = get_summary(&main_path).unwrap();
        // 主(100) + 直接子(200) + 工作流子(50)
        assert_eq!(summary.total_tokens.total(), 100 + 200 + 50);
        assert_eq!(summary.subagent_tokens.total(), 200 + 50);

        fs::remove_dir_all(&root).ok();
    }
}
