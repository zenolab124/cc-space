use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Timelike,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[path = "../config.rs"]
#[allow(dead_code)]
mod config;
#[path = "../widget_storage.rs"]
mod widget_storage;

static SNAPSHOT_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WidgetSnapshot {
    today_sessions: u32,
    today_tokens: u64,
    models: Vec<String>,
    updated_at: String,
    // Streak
    current_streak: u32,
    longest_streak: u32,
    active_days: u32,
    // Monthly
    monthly_tokens: u64,
    last_month_tokens: u64,
    month_mode: String,
    monthly_models: Vec<ModelStat>,
    // Cost
    estimated_cost_usd: f64,
    /// 价目表匹配不到的模型 token 总量（Swift 端 Optional 消费，旧版本忽略即可）
    unpriced_tokens: u64,
    // Weekly (last 7 days)
    weekly_tokens: Vec<DayTokens>,
    // Projects
    active_projects_today: u32,
    top_projects: Vec<ProjectStat>,
    // Hourly distribution (24 entries)
    hourly_distribution: Vec<u32>,
    // Heatmap (last 28 days)
    daily_heatmap: Vec<DayTokens>,
    // Totals
    total_sessions: u32,
    total_tokens: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModelStat {
    model: String,
    count: u32,
    tokens: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DayTokens {
    date: String,
    tokens: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProjectStat {
    name: String,
    sessions: u32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WidgetConfig {
    #[serde(default)]
    day_start_hour: i8,
    #[serde(default)]
    month_mode: String,
}

fn write_snapshot(path: &std::path::Path, json: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("snapshot path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    let mut temporary_name = path.as_os_str().to_os_string();
    let sequence = SNAPSHOT_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
    let temporary_path = PathBuf::from(temporary_name);
    fs::write(&temporary_path, json)
        .map_err(|error| format!("write {}: {error}", temporary_path.display()))?;
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("replace {}: {error}", path.display()));
    }
    Ok(())
}

fn with_snapshot_lock<T>(operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let lock_path = config::data_dir().join("widget-data.lock");
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("open widget snapshot lock: {error}"))?;
    FileExt::lock_exclusive(&lock).map_err(|error| format!("lock widget snapshot: {error}"))?;
    let result = operation();
    let _ = FileExt::unlock(&lock);
    result
}

fn read_config() -> WidgetConfig {
    fs::read_to_string(config::data_dir().join("widget-config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn compute_day_boundary(day_start_hour: i8, now: DateTime<Local>) -> i64 {
    if day_start_hour < 0 {
        return (now - Duration::hours(24)).timestamp();
    }

    let hour = u32::try_from(day_start_hour)
        .ok()
        .filter(|hour| *hour < 24)
        .unwrap_or_default();
    let boundary_time = NaiveTime::from_hms_opt(hour, 0, 0).unwrap_or_default();
    let today = now.date_naive();
    let boundary_date = if now.naive_local().time() >= boundary_time {
        today
    } else {
        today - Duration::days(1)
    };
    let boundary = resolve_local_boundary(boundary_date.and_time(boundary_time))
        .unwrap_or_else(|| now - Duration::hours(24));

    boundary.timestamp()
}

fn resolve_local_boundary(mut naive: NaiveDateTime) -> Option<DateTime<Local>> {
    // DST 春季跳时可能让配置的本地时刻不存在：顺延到首个有效分钟。
    // 秋季回拨会出现两个同名时刻：取第一次，保持当天窗口完整。
    for _ in 0..=180 {
        match naive.and_local_timezone(Local) {
            LocalResult::Single(value) => return Some(value),
            LocalResult::Ambiguous(first, _) => return Some(first),
            LocalResult::None => naive += Duration::minutes(1),
        }
    }
    None
}

fn summarize_timeline(
    timeline: &[app_lib::usage_stats::TimedUsage],
    start_ts: i64,
    end_exclusive_ts: i64,
) -> (
    u64,
    Vec<ModelStat>,
    Vec<app_lib::usage_stats::RawModelUsage>,
) {
    let mut total = 0u64;
    let mut models: HashMap<String, u64> = HashMap::new();
    let mut raw_models: HashMap<String, app_lib::models::TokenUsage> = HashMap::new();
    for item in timeline
        .iter()
        .filter(|item| item.timestamp >= start_ts && item.timestamp < end_exclusive_ts)
    {
        let tokens = item.usage.total();
        total += tokens;
        *models.entry(item.model.clone()).or_default() += tokens;
        raw_models
            .entry(item.raw_model.clone())
            .or_default()
            .accumulate(&item.usage);
    }
    let mut models: Vec<ModelStat> = models
        .into_iter()
        .map(|(model, tokens)| ModelStat {
            model,
            count: 0,
            tokens,
        })
        .collect();
    models.sort_unstable_by_key(|item| std::cmp::Reverse(item.tokens));
    let raw_models = raw_models
        .into_iter()
        .map(|(model, usage)| app_lib::usage_stats::RawModelUsage { model, usage })
        .collect();
    (total, models, raw_models)
}

fn compute_streak(daily: &[app_lib::usage_stats::DailyUsage]) -> (u32, u32, u32) {
    let today = Local::now().date_naive();
    let active_dates: std::collections::HashSet<NaiveDate> = daily
        .iter()
        .filter(|d| d.total > 0)
        .filter_map(|d| NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok())
        .collect();

    let active_days = active_dates.len() as u32;

    let mut current = 0u32;
    let mut day = today;
    if !active_dates.contains(&day) {
        day -= Duration::days(1);
    }
    while active_dates.contains(&day) {
        current += 1;
        day -= Duration::days(1);
    }

    let mut longest = 0u32;
    let mut sorted: Vec<NaiveDate> = active_dates.into_iter().collect();
    sorted.sort();
    let mut streak = 0u32;
    for (i, d) in sorted.iter().enumerate() {
        if i == 0 || *d != sorted[i - 1] + Duration::days(1) {
            streak = 1;
        } else {
            streak += 1;
        }
        longest = longest.max(streak);
    }

    (current, longest, active_days)
}

/// 按模型四类 token 分价计算月度成本。返回 (成本 USD, 未计价 token 数)——
/// 价目表匹配不到的模型不套兜底价（第三方模型按官方价乱猜会差出数量级），
/// 计 0 并如实上报未计价量
fn estimate_cost(
    pricing: &app_lib::pricing::PricingTable,
    models: &[app_lib::usage_stats::RawModelUsage],
) -> (f64, u64) {
    let mut cost = 0.0;
    let mut unpriced: u64 = 0;
    for m in models {
        match pricing.cost_usd(&m.model, &m.usage) {
            Some(usd) => cost += usd,
            None => unpriced += m.usage.total(),
        }
    }
    ((cost * 100.0).round() / 100.0, unpriced)
}

fn project_display_name(project_path: Option<&str>) -> String {
    project_path
        .and_then(|path| path.rsplit(['/', '\\']).find(|part| !part.is_empty()))
        .filter(|name| !name.is_empty())
        .unwrap_or("Uncategorized")
        .to_string()
}

fn collect_project_stats(
    start_ts: i64,
    end_exclusive_ts: i64,
    sessions: &[app_lib::usage_stats::SessionActivity],
) -> (u32, Vec<ProjectStat>, u32, Vec<u32>) {
    let mut project_counts: HashMap<String, (String, u32)> = HashMap::new();
    let mut active_today = std::collections::HashSet::new();
    let mut hourly = vec![0u32; 24];

    for session in sessions {
        let project_key = session
            .project_path
            .clone()
            .unwrap_or_else(|| "uncategorized".into());
        let entry = project_counts
            .entry(project_key.clone())
            .or_insert_with(|| (project_display_name(session.project_path.as_deref()), 0));
        entry.1 += 1;
        if session.updated_at >= start_ts.max(0) as u64
            && session.updated_at < end_exclusive_ts.max(0) as u64
        {
            active_today.insert(project_key);
        }
        if let Some(datetime) = chrono::DateTime::from_timestamp(session.updated_at as i64, 0) {
            let local = datetime.with_timezone(&Local);
            hourly[local.hour() as usize] += 1;
        }
    }

    let mut top: Vec<ProjectStat> = project_counts
        .into_iter()
        .map(|(_, (name, sessions))| ProjectStat { name, sessions })
        .collect();
    top.sort_by_key(|p| std::cmp::Reverse(p.sessions));
    top.truncate(8);

    (
        active_today.len() as u32,
        top,
        sessions.len() as u32,
        hourly,
    )
}

fn main() {
    let cfg = read_config();
    let now = Local::now();
    let end_exclusive_ts = now.timestamp().saturating_add(1);
    let start_ts = compute_day_boundary(cfg.day_start_hour, now);

    let stats = app_lib::usage_stats::collect_widget_usage_stats().ok();
    let today_sessions = stats
        .as_ref()
        .map(|stats| {
            stats
                .sessions
                .iter()
                .filter(|session| {
                    session.updated_at >= start_ts.max(0) as u64
                        && session.updated_at < end_exclusive_ts.max(0) as u64
                })
                .count() as u32
        })
        .unwrap_or_default();

    let (
        today_tokens,
        models,
        monthly_tokens,
        last_month_tokens,
        monthly_models,
        estimated_cost,
        unpriced_tokens,
        weekly_tokens,
        daily_heatmap,
        current_streak,
        longest_streak,
        active_days,
        total_tokens,
    ) = if let Some(stats) = stats.as_ref() {
        let today_date = now.date_naive();

        // Today tokens
        let tt = summarize_timeline(&stats.timeline, start_ts, end_exclusive_ts).0;

        // Last month tokens
        let lm = if now.month() == 1 {
            12
        } else {
            now.month() - 1
        };
        let ly = if now.month() == 1 {
            now.year() - 1
        } else {
            now.year()
        };
        let natural_last_month: u64 = stats
            .daily
            .iter()
            .filter(|d| {
                if let Ok(nd) = NaiveDate::parse_from_str(&d.date, "%Y-%m-%d") {
                    nd.year() == ly && nd.month() == lm
                } else {
                    false
                }
            })
            .map(|d| d.total)
            .sum();

        let is_rolling = cfg.month_mode == "rolling";

        // Rolling mode is an exact moving window, with the immediately preceding
        // 30 days as its comparison period.
        let (monthly_t, previous_monthly_t, mm, raw_models) = if is_rolling {
            let current_start = (now - Duration::days(30)).timestamp();
            let previous_start = (now - Duration::days(60)).timestamp();
            let (total, models, raw_models) =
                summarize_timeline(&stats.timeline, current_start, end_exclusive_ts);
            let previous_total =
                summarize_timeline(&stats.timeline, previous_start, current_start).0;
            (total, previous_total, models, raw_models)
        } else {
            let models = stats
                .month
                .by_model
                .iter()
                .map(|m| ModelStat {
                    model: m.model.clone(),
                    count: 0,
                    tokens: m.total,
                })
                .collect();
            (
                stats.month.total,
                natural_last_month,
                models,
                stats.month.by_raw_model.clone(),
            )
        };

        // 计价用原始模型名桶：归一化名匹配不上价目表。
        let pricing = app_lib::pricing::load();
        let (cost, unpriced) = estimate_cost(&pricing, &raw_models);
        let models_list: Vec<String> = mm.iter().map(|m| m.model.clone()).collect();

        // Weekly (last 7 days)
        let weekly: Vec<DayTokens> = (0..7)
            .rev()
            .map(|i| {
                let d = today_date - Duration::days(i);
                let ds = d.format("%Y-%m-%d").to_string();
                let t = stats
                    .daily
                    .iter()
                    .find(|x| x.date == ds)
                    .map(|x| x.total)
                    .unwrap_or(0);
                DayTokens {
                    date: ds,
                    tokens: t,
                }
            })
            .collect();

        // Heatmap
        let heatmap: Vec<DayTokens> = if is_rolling {
            (0..30)
                .rev()
                .map(|i| {
                    let d = today_date - Duration::days(i);
                    let ds = d.format("%Y-%m-%d").to_string();
                    let t = stats
                        .daily
                        .iter()
                        .find(|x| x.date == ds)
                        .map(|x| x.total)
                        .unwrap_or(0);
                    DayTokens {
                        date: ds,
                        tokens: t,
                    }
                })
                .collect()
        } else {
            let month_start =
                NaiveDate::from_ymd_opt(today_date.year(), today_date.month(), 1).unwrap();
            let next_month = if today_date.month() == 12 {
                NaiveDate::from_ymd_opt(today_date.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(today_date.year(), today_date.month() + 1, 1).unwrap()
            };
            let days_in_month = (next_month - month_start).num_days();
            (0..days_in_month)
                .map(|i| {
                    let d = month_start + Duration::days(i);
                    let ds = d.format("%Y-%m-%d").to_string();
                    let t = stats
                        .daily
                        .iter()
                        .find(|x| x.date == ds)
                        .map(|x| x.total)
                        .unwrap_or(0);
                    DayTokens {
                        date: ds,
                        tokens: t,
                    }
                })
                .collect()
        };

        let (cs, ls, ad) = compute_streak(&stats.daily);
        let total_t = stats.total;

        (
            tt,
            models_list,
            monthly_t,
            previous_monthly_t,
            mm,
            cost,
            unpriced,
            weekly,
            heatmap,
            cs,
            ls,
            ad,
            total_t,
        )
    } else {
        (
            0,
            Vec::new(),
            0,
            0,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            0,
            0,
            0,
            0,
        )
    };

    let (active_projects, top_projects, total_sessions, hourly) = stats
        .as_ref()
        .map(|stats| collect_project_stats(start_ts, end_exclusive_ts, &stats.sessions))
        .unwrap_or_else(|| (0, Vec::new(), 0, vec![0; 24]));

    let snap = WidgetSnapshot {
        today_sessions,
        today_tokens,
        models,
        updated_at: Local::now().to_rfc3339(),
        current_streak,
        longest_streak,
        active_days,
        monthly_tokens,
        last_month_tokens,
        month_mode: cfg.month_mode,
        monthly_models,
        estimated_cost_usd: estimated_cost,
        unpriced_tokens,
        weekly_tokens,
        active_projects_today: active_projects,
        top_projects,
        hourly_distribution: hourly,
        daily_heatmap,
        total_sessions,
        total_tokens,
    };

    let json = serde_json::to_string_pretty(&snap).unwrap_or_default();

    let result = with_snapshot_lock(|| {
        let bp = config::data_dir().join("widget-data.json");
        let mut failures = Vec::new();
        match widget_storage::shared_snapshot_path() {
            Ok(Some(path)) => {
                if let Err(error) = write_snapshot(&path, &json) {
                    failures.push(error);
                }
            }
            Ok(None) => {}
            Err(error) => failures.push(error),
        }
        if let Err(error) = write_snapshot(&bp, &json) {
            failures.push(error);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n"))
        }
    });
    if let Err(error) = result {
        eprintln!("widget snapshot update failed:\n{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local_time(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, 30, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn rolling_day_boundary_is_exactly_24_hours() {
        let now = local_time(2026, 8, 12, 9);
        assert_eq!(
            compute_day_boundary(-1, now),
            local_time(2026, 8, 11, 9).timestamp()
        );
    }

    #[test]
    fn fixed_day_boundary_uses_configured_local_hour() {
        let after_boundary = local_time(2026, 8, 12, 9);
        let before_boundary = local_time(2026, 8, 12, 3);
        assert_eq!(
            compute_day_boundary(5, after_boundary),
            Local
                .with_ymd_and_hms(2026, 8, 12, 5, 0, 0)
                .single()
                .unwrap()
                .timestamp()
        );
        assert_eq!(
            compute_day_boundary(5, before_boundary),
            Local
                .with_ymd_and_hms(2026, 8, 11, 5, 0, 0)
                .single()
                .unwrap()
                .timestamp()
        );
    }

    #[test]
    fn invalid_fixed_hour_falls_back_to_midnight() {
        let now = local_time(2026, 8, 12, 9);
        assert_eq!(
            compute_day_boundary(42, now),
            Local
                .with_ymd_and_hms(2026, 8, 12, 0, 0, 0)
                .single()
                .unwrap()
                .timestamp()
        );
    }

    #[test]
    fn timeline_summary_respects_exact_window() {
        let usage = |timestamp: i64, tokens: u64| app_lib::usage_stats::TimedUsage {
            timestamp,
            model: "gpt-test".into(),
            raw_model: "gpt-test".into(),
            usage: app_lib::models::TokenUsage {
                input_tokens: tokens,
                ..Default::default()
            },
        };
        let timeline = vec![
            usage(99, 100),
            usage(100, 200),
            usage(199, 300),
            usage(200, 400),
        ];

        let (total, models, raw_models) = summarize_timeline(&timeline, 100, 200);

        assert_eq!(total, 500);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].tokens, 500);
        assert_eq!(raw_models.len(), 1);
        assert_eq!(raw_models[0].usage.total(), 500);
    }
}
