//! 全项目 token 用量聚合（v2.2.0 FR-001）—— 首页 Token 卡与活跃热力图的数据源
//!
//! 聚合口径（PRD docs/prd/v2.2.0-home-dashboard.md FR-001）：
//! - 仅 assistant 记录的 message.usage，四类 token 求和（计费口径）
//! - 按 message.id 归并：优先终结非零快照，其次未终结非零，最后零值占位；
//!   id 缺失的行按行独立计
//! - `<synthetic>`（CLI 本地合成占位）与 timestamp 缺失的记录不进任何桶
//! - timestamp（ISO 8601 UTC）转本地时区后分天

use chrono::{DateTime, Datelike, Local, NaiveDate};
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cache::{self, CachedContrib, CachedUsage};
use crate::models::{TokenUsage, UsageSnapshot};
use crate::probe;

#[derive(Debug, Serialize)]
pub struct UsageStats {
    /// 所有可用引擎、所有历史日期的累计 token。
    pub total: u64,
    /// 近 16 周窗口（本周一往前 15 周起）内有数据的天，date 为本地 "YYYY-MM-DD"
    pub daily: Vec<DailyUsage>,
    /// 本地时区当前自然月
    pub month: MonthUsage,
    /// 同一聚合口径下的引擎分项，前端可解释总量来源。
    #[serde(rename = "byEngine")]
    pub by_engine: Vec<EngineUsageStats>,
    /// Widget 后台进程消费的会话活动，不进入 IPC JSON。
    #[serde(skip)]
    pub sessions: Vec<SessionActivity>,
    /// 去重后的秒级用量时间线，供 Widget 精确截取自定义日界线。
    #[serde(skip)]
    pub timeline: Vec<TimedUsage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineUsageStats {
    pub engine_id: String,
    pub total: u64,
    pub daily: Vec<DailyUsage>,
    pub month: MonthUsage,
}

#[derive(Debug, Serialize)]
pub struct DailyUsage {
    pub date: String,
    pub total: u64,
}

#[derive(Debug, Serialize)]
pub struct MonthUsage {
    pub total: u64,
    #[serde(rename = "byModel")]
    pub by_model: Vec<ModelUsage>,
    /// 按原始模型名的四类分量，供成本分价计算——归一化名会丢失匹配价目表
    /// 所需的原始信息（前缀/日期段），故独立成桶；前端不消费，不进 IPC
    #[serde(skip)]
    pub by_raw_model: Vec<RawModelUsage>,
}

#[derive(Debug, Serialize)]
pub struct ModelUsage {
    pub model: String,
    pub total: u64,
}

#[derive(Debug, Clone)]
pub struct RawModelUsage {
    pub model: String,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone)]
pub struct TimedUsage {
    pub timestamp: i64,
    pub model: String,
    pub raw_model: String,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone)]
pub struct SessionActivity {
    pub engine_id: String,
    pub session_id: String,
    pub project_path: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug)]
pub(crate) struct EngineUsageContribution {
    pub id: String,
    pub engine_id: String,
    pub date: NaiveDate,
    pub timestamp: String,
    pub model: Option<String>,
    pub usage: TokenUsage,
    pub sequence: u64,
}

#[derive(Debug, Default)]
pub(crate) struct EngineUsageData {
    pub source_available: bool,
    pub contributions: Vec<EngineUsageContribution>,
    pub sessions: Vec<SessionActivity>,
}

/// 单条 assistant 行的贡献；model 存原始串，归一化延后到聚合阶段
/// （distinct 模型数有限，避免在 10 万级热路径上跑 regex）
struct Contribution {
    engine_id: String,
    date: NaiveDate,
    model: Option<String>,
    snapshot: UsageSnapshot,
}

/// 文件局部聚合容器，rayon map-reduce 合并。
/// by_id 的去重跨文件生效（resume/fork 复制历史行时同 id 也只计一次）
#[derive(Default)]
struct Buckets {
    by_id: HashMap<String, Contribution>,
    anon: Vec<Contribution>,
}

impl Buckets {
    fn merge(mut self, other: Buckets) -> Buckets {
        for (id, candidate) in other.by_id {
            match self.by_id.get_mut(&id) {
                Some(current) if candidate.snapshot.is_better_than(&current.snapshot) => {
                    *current = candidate;
                }
                Some(_) => {}
                None => {
                    self.by_id.insert(id, candidate);
                }
            }
        }
        self.anon.extend(other.anon);
        self
    }
}

/// 轻量行解析：只取去重/分桶必需字段，跳过 content 反序列化
#[derive(Deserialize)]
struct LineExtract {
    #[serde(rename = "type")]
    record_type: Option<String>,
    timestamp: Option<String>,
    message: Option<MsgExtract>,
}

#[derive(Deserialize)]
struct MsgExtract {
    id: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    usage: Option<TokenUsage>,
}

fn scan_file(path: &Path) -> Buckets {
    let mut out = Buckets::default();
    let Ok(file) = File::open(path) else { return out };
    let reader = BufReader::with_capacity(64 * 1024, file);

    // 先按行序收集，EOF 后统一做缓存写推断再入桶（推断需要整个文件的全局判断）
    let mut rows: Vec<(Option<String>, Contribution)> = Vec::new();
    for (sequence, line) in reader.lines().enumerate() {
        let Ok(line) = line else { break };
        if !line.contains("\"assistant\"") || !line.contains("\"usage\"") {
            continue;
        }
        let Ok(ext) = serde_json::from_str::<LineExtract>(&line) else { continue };
        if ext.record_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(msg) = ext.message else { continue };
        if msg.model.as_deref() == Some("<synthetic>") {
            continue;
        }
        let Some(usage) = msg.usage else { continue };
        let Some(date) = ext
            .timestamp
            .as_deref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Local).date_naive())
        else {
            continue;
        };

        rows.push((
            msg.id,
            Contribution {
                engine_id: "claude-code".into(),
                date,
                model: msg.model,
                snapshot: UsageSnapshot::new(
                    usage,
                    msg.stop_reason.as_deref(),
                    ext.timestamp,
                    sequence as u64,
                ),
            },
        ));
    }

    for (id, contribution) in rows {
        match id {
            Some(id) => match out.by_id.get_mut(&id) {
                Some(current) if contribution.snapshot.is_better_than(&current.snapshot) => {
                    *current = contribution;
                }
                Some(_) => {}
                None => {
                    out.by_id.insert(id, contribution);
                }
            },
            None => out.anon.push(contribution),
        }
    }
    infer_cache_creation(&mut out);
    out
}

fn is_openai_model(model: Option<&str>) -> bool {
    let Some(model) = model else { return false };
    let model = model.to_ascii_lowercase();
    let bare = model.strip_prefix("openai/").unwrap_or(&model);
    model.starts_with("openai/")
        || bare.contains("gpt")
        || bare.contains("codex")
        || matches!(bare, "o1" | "o3" | "o4")
        || bare.starts_with("o1-")
        || bare.starts_with("o3-")
        || bare.starts_with("o4-")
}

/// 第三方兼容层的已知缺口：部分 Anthropic 渠道只上报 cache_read 不报
/// cache_creation。OpenAI/GPT 的缓存语义不同，不参与这项启发式推断。
fn infer_cache_creation(buckets: &mut Buckets) {
    let mut rows: Vec<&mut Contribution> = buckets
        .by_id
        .values_mut()
        .chain(buckets.anon.iter_mut())
        .filter(|c| !is_openai_model(c.model.as_deref()))
        .collect();
    rows.sort_unstable_by(|a, b| {
        a.snapshot
            .timestamp
            .cmp(&b.snapshot.timestamp)
            .then(a.snapshot.sequence.cmp(&b.snapshot.sequence))
    });

    let has_creation = rows
        .iter()
        .any(|c| c.snapshot.usage.cache_creation_input_tokens > 0);
    let has_read = rows
        .iter()
        .any(|c| c.snapshot.usage.cache_read_input_tokens > 0);
    if has_creation || !has_read {
        return;
    }
    let mut max_read: u64 = 0;
    for c in rows {
        let read = c.snapshot.usage.cache_read_input_tokens;
        if read > max_read {
            c.snapshot.usage.cache_creation_input_tokens = read - max_read;
            c.snapshot.usage.cache_read_input_tokens = max_read;
            max_read = read;
        }
    }
}

fn scan_file_cached(path: &Path) -> Buckets {
    if let Some(cached) = cache::get_usage(path) {
        return cached_to_buckets(cached);
    }
    let buckets = scan_file(path);
    cache::set_usage(path, buckets_to_cached(&buckets));
    buckets
}

fn cached_to_buckets(cached: CachedUsage) -> Buckets {
    let by_id = cached
        .by_id
        .into_iter()
        .filter_map(|(id, c)| {
            NaiveDate::parse_from_str(&c.date, "%Y-%m-%d")
                .ok()
                .map(|date| {
                    (
                        id,
                        Contribution {
                            engine_id: "claude-code".into(),
                            date,
                            model: c.model,
                            snapshot: c.snapshot,
                        },
                    )
                })
        })
        .collect();
    let anon = cached
        .anon
        .into_iter()
        .filter_map(|c| {
            NaiveDate::parse_from_str(&c.date, "%Y-%m-%d")
                .ok()
                .map(|date| Contribution {
                    engine_id: "claude-code".into(),
                    date,
                    model: c.model,
                    snapshot: c.snapshot,
                })
        })
        .collect();
    Buckets { by_id, anon }
}

fn buckets_to_cached(buckets: &Buckets) -> CachedUsage {
    CachedUsage {
        by_id: buckets
            .by_id
            .iter()
            .map(|(id, c)| {
                (
                    id.clone(),
                    CachedContrib {
                        date: c.date.format("%Y-%m-%d").to_string(),
                        model: c.model.clone(),
                        snapshot: c.snapshot.clone(),
                    },
                )
            })
            .collect(),
        anon: buckets
            .anon
            .iter()
            .map(|c| CachedContrib {
                date: c.date.format("%Y-%m-%d").to_string(),
                model: c.model.clone(),
                snapshot: c.snapshot.clone(),
            })
            .collect(),
    }
}

/// 模型名归一化（PRD FR-001 规则 5，五步顺序执行）
fn normalize_model(raw: &str, date_suffix: &Regex, version_tail: &Regex) -> String {
    // ① 去方括号后缀（如 "opus-4.6 [1m]"）与首尾空白
    let s = raw.split('[').next().unwrap_or(raw).trim();
    // ② 去前缀 claude-
    let s = s.strip_prefix("claude-").unwrap_or(s);
    // ③ 去尾部 -YYYYMMDD 八位日期后缀
    let s = date_suffix.replace(s, "");
    // ④ 尾部 -数字-数字 → -数字.数字（opus-4-8 → opus-4.8）
    let s = version_tail.replace(&s, "-$1.$2");
    // ⑤ 其余原样保留
    s.into_owned()
}

fn collect_claude_usage() -> Result<(Buckets, Vec<SessionActivity>), String> {
    let root = crate::config::projects_dir();
    std::fs::read_dir(&root).map_err(|e| format!("会话数据目录不可读 {}: {e}", root.display()))?;

    let mut files = Vec::new();
    probe::collect_jsonl(&root, None, &mut files);

    // 内置 Agent 落盘会话不计入用量：与档案/搜索/watcher 的软屏蔽口径一致
    let agent_dirs: std::collections::HashSet<String> =
        crate::config::agent_project_dirs().into_iter().collect();
    files.retain(|p| {
        p.strip_prefix(&root)
            .ok()
            .and_then(|rel| rel.components().next())
            .map(|c| !agent_dirs.contains(c.as_os_str().to_string_lossy().as_ref()))
            .unwrap_or(true)
    });

    let sessions = files
        .iter()
        .filter(|path| {
            !path
                .components()
                .any(|component| component.as_os_str() == "subagents")
                && !path.file_name().is_some_and(|name| {
                    name.to_string_lossy().starts_with("agent-")
                })
        })
        .filter_map(|path| claude_session_activity(path, &root))
        .collect();

    let buckets = files
        .par_iter()
        .map(|p| scan_file_cached(p))
        .reduce(Buckets::default, Buckets::merge);

    Ok((buckets, sessions))
}

fn claude_session_activity(path: &Path, root: &Path) -> Option<SessionActivity> {
    let metadata = std::fs::metadata(path).ok()?;
    let updated_at = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    let mut project_path = None;
    if let Ok(file) = File::open(path) {
        for line in BufReader::new(file).lines().take(32).flatten() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            project_path = value
                .get("cwd")
                .and_then(serde_json::Value::as_str)
                .filter(|cwd| !cwd.trim().is_empty())
                .map(String::from);
            if project_path.is_some() {
                break;
            }
        }
    }
    if project_path.is_none() {
        project_path = path
            .strip_prefix(root)
            .ok()
            .and_then(|relative| relative.components().next())
            .map(|component| component.as_os_str().to_string_lossy())
            .map(|encoded| {
                encoded
                    .strip_prefix('-')
                    .map(|value| format!("/{}", value.replace('-', "/")))
                    .unwrap_or_else(|| encoded.replace('-', "/"))
            });
    }
    Some(SessionActivity {
        engine_id: "claude-code".into(),
        session_id,
        project_path,
        updated_at,
    })
}

#[derive(Default)]
struct UsageAccumulator {
    total: u64,
    daily: HashMap<NaiveDate, u64>,
    month_total: u64,
    by_model: HashMap<String, u64>,
    by_raw_model: HashMap<String, TokenUsage>,
}

impl UsageAccumulator {
    fn add(
        &mut self,
        contribution: &Contribution,
        today: NaiveDate,
        window_start: NaiveDate,
        date_suffix: &Regex,
        version_tail: &Regex,
    ) {
        let usage = &contribution.snapshot.usage;
        let tokens = usage.total();
        if contribution.date <= today {
            self.total += tokens;
        }
        if contribution.date >= window_start && contribution.date <= today {
            *self.daily.entry(contribution.date).or_default() += tokens;
        }
        if contribution.date.year() == today.year()
            && contribution.date.month() == today.month()
            && contribution.date <= today
        {
            self.month_total += tokens;
            let model = contribution
                .model
                .as_deref()
                .map(|model| normalize_model(model, date_suffix, version_tail))
                .unwrap_or_else(|| "未知".to_string());
            *self.by_model.entry(model).or_default() += tokens;
            self.by_raw_model
                .entry(contribution.model.clone().unwrap_or_default())
                .or_default()
                .accumulate(usage);
        }
    }

    fn finish(self) -> (u64, Vec<DailyUsage>, MonthUsage) {
        let mut daily: Vec<DailyUsage> = self
            .daily
            .into_iter()
            .map(|(date, total)| DailyUsage {
                date: date.format("%Y-%m-%d").to_string(),
                total,
            })
            .collect();
        daily.sort_unstable_by(|left, right| left.date.cmp(&right.date));

        let mut by_model: Vec<ModelUsage> = self
            .by_model
            .into_iter()
            .map(|(model, total)| ModelUsage { model, total })
            .collect();
        by_model.sort_unstable_by_key(|model| std::cmp::Reverse(model.total));

        let mut by_raw_model: Vec<RawModelUsage> = self
            .by_raw_model
            .into_iter()
            .map(|(model, usage)| RawModelUsage { model, usage })
            .collect();
        by_raw_model.sort_unstable_by_key(|model| std::cmp::Reverse(model.usage.total()));

        (
            self.total,
            daily,
            MonthUsage {
                total: self.month_total,
                by_model,
                by_raw_model,
            },
        )
    }
}

fn aggregate_usage(
    buckets: Buckets,
    sessions: Vec<SessionActivity>,
    today: NaiveDate,
    include_timeline: bool,
) -> Result<UsageStats, String> {
    let days_from_monday = today.weekday().num_days_from_monday() as i64;
    let window_start = today - chrono::Duration::days(days_from_monday + 15 * 7);

    let date_suffix = Regex::new(r"-\d{8}$").map_err(|e| e.to_string())?;
    let version_tail = Regex::new(r"-(\d+)-(\d+)$").map_err(|e| e.to_string())?;

    let mut aggregate = UsageAccumulator::default();
    let mut engines: HashMap<String, UsageAccumulator> = HashMap::new();
    let mut timeline = Vec::new();
    for session in &sessions {
        engines.entry(session.engine_id.clone()).or_default();
    }
    for contribution in buckets.by_id.into_values().chain(buckets.anon) {
        if include_timeline {
            let timestamp = contribution
                .snapshot
                .timestamp
                .as_deref()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
            if let Some(timestamp) = timestamp {
                let raw_model = contribution.model.clone().unwrap_or_default();
                let model = contribution
                    .model
                    .as_deref()
                    .map(|value| normalize_model(value, &date_suffix, &version_tail))
                    .unwrap_or_else(|| "未知".to_string());
                timeline.push(TimedUsage {
                    timestamp: timestamp.timestamp(),
                    model,
                    raw_model,
                    usage: contribution.snapshot.usage.clone(),
                });
            }
        }
        aggregate.add(
            &contribution,
            today,
            window_start,
            &date_suffix,
            &version_tail,
        );
        engines.entry(contribution.engine_id.clone()).or_default().add(
            &contribution,
            today,
            window_start,
            &date_suffix,
            &version_tail,
        );
    }

    let (total, daily, month) = aggregate.finish();
    let mut by_engine: Vec<EngineUsageStats> = engines
        .into_iter()
        .map(|(engine_id, usage)| {
            let (total, daily, month) = usage.finish();
            EngineUsageStats {
                engine_id,
                total,
                daily,
                month,
            }
        })
        .collect();
    by_engine.sort_unstable_by(|left, right| left.engine_id.cmp(&right.engine_id));
    timeline.sort_unstable_by_key(|item| item.timestamp);

    Ok(UsageStats {
        total,
        daily,
        month,
        by_engine,
        sessions,
        timeline,
    })
}

/// 聚合所有可用引擎的本地用量。单个引擎数据源不可读时，其余引擎仍可工作；
/// 只有所有来源都不可用时才返回错误。
fn collect_usage_stats_inner(include_timeline: bool) -> Result<UsageStats, String> {
    let mut buckets = Buckets::default();
    let mut sessions = Vec::new();
    let mut source_available = false;
    let mut source_errors = Vec::new();

    match collect_claude_usage() {
        Ok((claude_buckets, claude_sessions)) => {
            source_available = true;
            buckets = buckets.merge(claude_buckets);
            sessions.extend(claude_sessions);
        }
        Err(error) => source_errors.push(error),
    }

    let codex = crate::engines::codex::collect_local_usage();
    source_available |= codex.source_available;
    let mut codex_buckets = Buckets::default();
    for contribution in codex.contributions {
        codex_buckets.by_id.insert(
            contribution.id,
            Contribution {
                engine_id: contribution.engine_id,
                date: contribution.date,
                model: contribution.model,
                snapshot: UsageSnapshot::new(
                    contribution.usage,
                    Some("complete"),
                    Some(contribution.timestamp),
                    contribution.sequence,
                ),
            },
        );
    }
    buckets = buckets.merge(codex_buckets);
    sessions.extend(codex.sessions);

    if !source_available {
        return Err(source_errors
            .into_iter()
            .next()
            .unwrap_or_else(|| "没有可读取的引擎会话数据目录".into()));
    }

    let mut unique_sessions: HashMap<(String, String), SessionActivity> = HashMap::new();
    for session in sessions {
        let key = (session.engine_id.clone(), session.session_id.clone());
        match unique_sessions.get(&key) {
            Some(current) if current.updated_at >= session.updated_at => {}
            _ => {
                unique_sessions.insert(key, session);
            }
        }
    }
    let sessions = unique_sessions.into_values().collect();
    let stats = aggregate_usage(
        buckets,
        sessions,
        Local::now().date_naive(),
        include_timeline,
    )?;
    cache::flush();
    Ok(stats)
}

pub fn collect_usage_stats() -> Result<UsageStats, String> {
    collect_usage_stats_inner(false)
}

/// Widget 需要按秒截取自定义时间窗口，只在这条路径保留去重后时间线。
pub fn collect_widget_usage_stats() -> Result<UsageStats, String> {
    collect_usage_stats_inner(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// FR-001 规则 5：模型名归一化五步
    #[test]
    fn model_normalization_rules() {
        let date_suffix = Regex::new(r"-\d{8}$").unwrap();
        let version_tail = Regex::new(r"-(\d+)-(\d+)$").unwrap();
        let norm = |s: &str| normalize_model(s, &date_suffix, &version_tail);

        assert_eq!(norm("claude-opus-4-8"), "opus-4.8");
        assert_eq!(norm("claude-fable-5"), "fable-5");
        assert_eq!(norm("claude-sonnet-4-5-20250929"), "sonnet-4.5");
        assert_eq!(norm("gpt-5.4"), "gpt-5.4");
        assert_eq!(norm("sonnet"), "sonnet");
        assert_eq!(norm("claude-opus-4-6 [1m]"), "opus-4.6");
    }

    /// FR-001 规则 2/3/4：id 去重进 map、synthetic 与缺 timestamp 不进桶、缺 cache 字段按 0
    #[test]
    fn scan_file_buckets() {
        let path = std::env::temp_dir().join("monet-test-usage.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        let mk = |id: &str, model: &str, ts: &str| {
            format!(
                "{{\"type\":\"assistant\",{ts}\"message\":{{\"id\":\"{id}\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":2}}}}}}"
            )
        };
        let ts = "\"timestamp\":\"2026-06-11T10:00:00.000Z\",";
        writeln!(f, "{}", mk("m1", "claude-fable-5", ts)).unwrap();
        writeln!(f, "{}", mk("m1", "claude-fable-5", ts)).unwrap(); // 同 id 重复
        writeln!(f, "{}", mk("m2", "<synthetic>", ts)).unwrap(); // synthetic 剔除
        writeln!(f, "{}", mk("m3", "claude-fable-5", "")).unwrap(); // 缺 timestamp 剔除
        drop(f);

        let buckets = scan_file(&path);
        assert_eq!(buckets.by_id.len(), 1);
        assert!(buckets.anon.is_empty());
        // 极简 usage（缺两个 cache 字段）按 0 计：1 + 2 = 3
        assert_eq!(
            buckets.by_id.get("m1").unwrap().snapshot.usage.total(),
            3
        );
        std::fs::remove_file(&path).ok();
    }

    /// Anthropic 兼容渠道保留 cache creation 推断；OpenAI/GPT 不套用该语义。
    #[test]
    fn cache_creation_inference_skips_openai_models() {
        let contribution = |model: &str, cc: u64, cr: u64, sequence: u64| Contribution {
            engine_id: "claude-code".into(),
            date: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
            model: Some(model.into()),
            snapshot: UsageSnapshot::new(
                TokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_creation_input_tokens: cc,
                    cache_read_input_tokens: cr,
                },
                Some("end_turn"),
                Some(format!("2026-07-01T00:00:{sequence:02}Z")),
                sequence,
            ),
        };

        let mut buckets = Buckets::default();
        buckets
            .by_id
            .insert("a0".into(), contribution("claude-fable-5", 0, 0, 0));
        buckets
            .by_id
            .insert("a1".into(), contribution("claude-fable-5", 0, 1000, 1));
        buckets
            .by_id
            .insert("a2".into(), contribution("claude-fable-5", 0, 3000, 2));
        buckets
            .by_id
            .insert("g1".into(), contribution("gpt-5.6-sol", 0, 4000, 3));

        infer_cache_creation(&mut buckets);
        let usage = |id: &str| &buckets.by_id.get(id).unwrap().snapshot.usage;
        assert_eq!(
            (
                usage("a1").cache_creation_input_tokens,
                usage("a1").cache_read_input_tokens,
            ),
            (1000, 0)
        );
        assert_eq!(
            (
                usage("a2").cache_creation_input_tokens,
                usage("a2").cache_read_input_tokens,
            ),
            (2000, 1000)
        );
        assert_eq!(
            (
                usage("g1").cache_creation_input_tokens,
                usage("g1").cache_read_input_tokens,
            ),
            (0, 4000),
            "GPT 缓存字段保持上游原值"
        );
    }

    #[test]
    fn aggregates_engines_while_preserving_breakdown() {
        let contribution = |engine_id: &str, model: &str, total: u64| Contribution {
            engine_id: engine_id.into(),
            date: NaiveDate::from_ymd_opt(2026, 8, 10).unwrap(),
            model: Some(model.into()),
            snapshot: UsageSnapshot::new(
                TokenUsage {
                    input_tokens: total,
                    ..TokenUsage::default()
                },
                Some("complete"),
                Some("2026-08-10T08:00:00Z".into()),
                total,
            ),
        };
        let mut buckets = Buckets::default();
        buckets.by_id.insert(
            "claude-message".into(),
            contribution("claude-code", "claude-opus-4-6", 10),
        );
        buckets.by_id.insert(
            "codex-event".into(),
            contribution("codex", "gpt-5.6-sol", 20),
        );
        let sessions = vec![
            SessionActivity {
                engine_id: "claude-code".into(),
                session_id: "claude-session".into(),
                project_path: Some("/workspace/project".into()),
                updated_at: 1,
            },
            SessionActivity {
                engine_id: "codex".into(),
                session_id: "codex-session".into(),
                project_path: Some("/workspace/project".into()),
                updated_at: 2,
            },
        ];

        let stats = aggregate_usage(
            buckets,
            sessions,
            NaiveDate::from_ymd_opt(2026, 8, 10).unwrap(),
            true,
        )
        .unwrap();

        assert_eq!(stats.month.total, 30);
        assert_eq!(stats.total, 30);
        assert_eq!(stats.daily[0].total, 30);
        assert_eq!(stats.sessions.len(), 2);
        assert_eq!(stats.timeline.len(), 2);
        assert_eq!(
            stats
                .by_engine
                .iter()
                .map(|engine| (engine.engine_id.as_str(), engine.month.total))
                .collect::<Vec<_>>(),
            vec![("claude-code", 10), ("codex", 20)]
        );
    }

    /// 联机 smoke：本机真实数据全量聚合的耗时与量级。
    /// 依赖本机 ~/.claude/projects/，不进常规跑：cargo test -- --ignored --nocapture
    #[test]
    #[ignore]
    fn smoke_full_aggregation() {
        let t0 = std::time::Instant::now();
        let stats = collect_usage_stats().unwrap();
        println!(
            "elapsed: {:?} · daily days: {} · month total: {} · by_model: {:?}",
            t0.elapsed(),
            stats.daily.len(),
            stats.month.total,
            stats
                .month
                .by_model
                .iter()
                .map(|m| (m.model.as_str(), m.total))
                .collect::<Vec<_>>()
        );
    }
}
