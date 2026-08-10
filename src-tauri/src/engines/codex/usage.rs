use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Local, NaiveDate};
use rayon::prelude::*;
use serde_json::Value;

use crate::cache::{self, CachedContrib, CachedUsage};
use crate::models::{TokenUsage, UsageSnapshot};
use crate::usage_stats::{EngineUsageContribution, EngineUsageData, SessionActivity};

use super::file_source;

const ENGINE_ID: &str = "codex";

pub(crate) fn collect_local_usage() -> EngineUsageData {
    let paths = file_source::session_paths();
    let source_available = file_source::codex_home().is_some_and(|home| {
        [home.join("sessions"), home.join("archived_sessions")]
            .iter()
            .any(|directory| std::fs::read_dir(directory).is_ok())
    });

    let parts: Vec<_> = paths
        .par_iter()
        .map(|path| {
            let summary = file_source::read_thread_summary(path);
            if summary
                .as_ref()
                .is_some_and(|thread| file_source::is_agent_cwd(&thread.cwd))
            {
                return (Vec::new(), None);
            }
            let contributions = scan_file_cached(path);
            let session = summary.map(|thread| SessionActivity {
                engine_id: ENGINE_ID.to_string(),
                session_id: thread.id,
                project_path: (!thread.cwd.trim().is_empty()).then_some(thread.cwd),
                updated_at: thread.updated_at.max(0) as u64,
            });
            (contributions, session)
        })
        .collect();

    let mut data = EngineUsageData {
        source_available,
        ..EngineUsageData::default()
    };
    for (contributions, session) in parts {
        data.contributions.extend(contributions);
        if let Some(session) = session {
            data.sessions.push(session);
        }
    }
    data
}

fn scan_file_cached(path: &Path) -> Vec<EngineUsageContribution> {
    if let Some(cached) = cache::get_usage(path) {
        return cached
            .by_id
            .into_iter()
            .filter_map(|(id, contribution)| cached_contribution(id, contribution))
            .chain(
                cached
                    .anon
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, contribution)| {
                        cached_contribution(format!("codex-anon-{index}"), contribution)
                    }),
            )
            .collect();
    }

    let contributions = scan_file(path);
    cache::set_usage(
        path,
        CachedUsage {
            by_id: contributions
                .iter()
                .map(|contribution| {
                    (
                        contribution.id.clone(),
                        CachedContrib {
                            date: contribution.date.format("%Y-%m-%d").to_string(),
                            model: contribution.model.clone(),
                            snapshot: UsageSnapshot::new(
                                contribution.usage.clone(),
                                Some("complete"),
                                Some(contribution.timestamp.clone()),
                                contribution.sequence,
                            ),
                        },
                    )
                })
                .collect(),
            anon: Vec::new(),
        },
    );
    contributions
}

fn cached_contribution(
    id: String,
    contribution: CachedContrib,
) -> Option<EngineUsageContribution> {
    let date = NaiveDate::parse_from_str(&contribution.date, "%Y-%m-%d").ok()?;
    let timestamp = contribution.snapshot.timestamp?;
    Some(EngineUsageContribution {
        id,
        engine_id: ENGINE_ID.to_string(),
        date,
        timestamp,
        model: contribution.model,
        usage: contribution.snapshot.usage,
        sequence: contribution.snapshot.sequence,
    })
}

fn scan_file(path: &Path) -> Vec<EngineUsageContribution> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut model = None;
    let mut session_kind_resolved = false;
    let mut is_subagent_rollout = false;
    let mut reached_turn_context = false;
    let mut contributions = Vec::new();
    for (sequence, line) in BufReader::with_capacity(64 * 1024, file)
        .lines()
        .enumerate()
    {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") if !session_kind_resolved => {
                is_subagent_rollout = is_subagent_session_meta(&value);
                session_kind_resolved = true;
            }
            Some("turn_context") => {
                reached_turn_context = true;
                model = value
                    .get("payload")
                    .unwrap_or(&value)
                    .get("model")
                    .and_then(Value::as_str)
                    .filter(|model| !model.is_empty())
                    .map(String::from);
            }
            Some("event_msg") => {
                let Some((timestamp, date, usage)) = usage_contribution(&value) else {
                    continue;
                };
                // legacy 子 Agent rollout 会在自身首个 turn_context 前复制父任务历史。
                // 这些 token_count 是继承上下文，不是子 Agent 新产生的用量。
                if is_subagent_rollout && !reached_turn_context {
                    continue;
                }
                let id = format!(
                    "codex:{timestamp}:{}:{}:{}:{}:{}",
                    model.as_deref().unwrap_or("unknown"),
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_creation_input_tokens,
                    usage.cache_read_input_tokens,
                );
                contributions.push(EngineUsageContribution {
                    id,
                    engine_id: ENGINE_ID.to_string(),
                    date,
                    timestamp,
                    model: model.clone(),
                    usage,
                    sequence: sequence as u64,
                });
            }
            _ => {}
        }
    }
    contributions
}

fn is_subagent_session_meta(value: &Value) -> bool {
    let payload = value.get("payload").unwrap_or(value);
    let source_marks_subagent = payload
        .get("source")
        .and_then(Value::as_object)
        .is_some_and(|source| source.contains_key("subagent"));
    let has_subagent_identity = payload
        .get("parent_thread_id")
        .is_some_and(|value| !value.is_null())
        && payload
            .get("agent_path")
            .is_some_and(|value| !value.is_null());
    source_marks_subagent || has_subagent_identity
}

fn usage_contribution(value: &Value) -> Option<(String, NaiveDate, TokenUsage)> {
    let payload = value.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let timestamp = value.get("timestamp")?.as_str()?.to_string();
    let date = DateTime::parse_from_rfc3339(&timestamp)
        .ok()?
        .with_timezone(&Local)
        .date_naive();
    let usage = payload.get("info")?.get("last_token_usage")?;
    let input_tokens = usage.get("input_tokens")?.as_u64()?;
    let cache_read_input_tokens = usage
        .get("cached_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let cache_creation_input_tokens = usage
        .get("cache_write_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let uncached_input_tokens = input_tokens
        .saturating_sub(cache_read_input_tokens)
        .saturating_sub(cache_creation_input_tokens);
    Some((
        timestamp,
        date,
        TokenUsage {
            input_tokens: uncached_input_tokens,
            output_tokens: usage.get("output_tokens")?.as_u64()?,
            cache_creation_input_tokens,
            cache_read_input_tokens,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_rollout_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "monet-codex-usage-{name}-{}-{unique}.jsonl",
            std::process::id()
        ))
    }

    fn token_event(timestamp: &str, input: u64, cached: u64, output: u64) -> Value {
        json!({
            "type": "event_msg",
            "timestamp": timestamp,
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": input,
                        "cached_input_tokens": cached,
                        "output_tokens": output
                    }
                }
            }
        })
    }

    #[test]
    fn maps_codex_increment_without_double_counting_cache() {
        let (timestamp, date, usage) = usage_contribution(&json!({
            "type": "event_msg",
            "timestamp": "2026-08-09T16:20:41.993Z",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": 1080,
                        "cached_input_tokens": 900,
                        "cache_write_input_tokens": 20,
                        "output_tokens": 70,
                        "total_tokens": 1150
                    }
                }
            }
        }))
        .unwrap();

        assert_eq!(timestamp, "2026-08-09T16:20:41.993Z");
        let expected_date = DateTime::parse_from_rfc3339(&timestamp)
            .unwrap()
            .with_timezone(&Local)
            .date_naive();
        assert_eq!(date, expected_date);
        assert_eq!(usage.input_tokens, 160);
        assert_eq!(usage.cache_read_input_tokens, 900);
        assert_eq!(usage.cache_creation_input_tokens, 20);
        assert_eq!(usage.output_tokens, 70);
        assert_eq!(usage.total(), 1150);
    }

    #[test]
    fn ignores_cumulative_total_and_uses_each_last_increment() {
        let value = json!({
            "type": "event_msg",
            "timestamp": "2026-08-09T08:00:00Z",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": { "input_tokens": 9999, "output_tokens": 999 },
                    "last_token_usage": { "input_tokens": 100, "output_tokens": 20 }
                }
            }
        });
        assert_eq!(usage_contribution(&value).unwrap().2.total(), 120);
    }

    #[test]
    fn skips_inherited_usage_before_subagent_turn_context() {
        let path = test_rollout_path("subagent-prefix");
        let mut file = File::create(&path).unwrap();
        let rows = [
            json!({
                "type": "session_meta",
                "timestamp": "2026-08-09T08:00:00Z",
                "payload": {
                    "source": { "subagent": { "kind": "spawn" } },
                    "parent_thread_id": "parent",
                    "agent_path": "agent/test",
                    "history_mode": "legacy"
                }
            }),
            token_event("2026-08-09T08:00:01Z", 1_000, 900, 50),
            json!({
                "type": "turn_context",
                "timestamp": "2026-08-09T08:00:02Z",
                "payload": { "turn_id": "child-turn", "model": "gpt-test" }
            }),
            token_event("2026-08-09T08:00:03Z", 200, 120, 30),
        ];
        for row in rows {
            writeln!(file, "{row}").unwrap();
        }
        drop(file);

        let contributions = scan_file(&path);
        assert_eq!(contributions.len(), 1);
        assert_eq!(contributions[0].model.as_deref(), Some("gpt-test"));
        assert_eq!(contributions[0].usage.total(), 230);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn keeps_pre_context_usage_for_regular_rollouts() {
        let path = test_rollout_path("regular-prefix");
        let mut file = File::create(&path).unwrap();
        let rows = [
            json!({
                "type": "session_meta",
                "timestamp": "2026-08-09T08:00:00Z",
                "payload": { "source": "vscode", "history_mode": "legacy" }
            }),
            token_event("2026-08-09T08:00:01Z", 100, 40, 10),
            json!({
                "type": "turn_context",
                "timestamp": "2026-08-09T08:00:02Z",
                "payload": { "turn_id": "turn", "model": "gpt-test" }
            }),
            token_event("2026-08-09T08:00:03Z", 200, 120, 30),
        ];
        for row in rows {
            writeln!(file, "{row}").unwrap();
        }
        drop(file);

        let contributions = scan_file(&path);
        assert_eq!(contributions.len(), 2);
        assert_eq!(contributions[0].model, None);
        assert_eq!(contributions[0].usage.total(), 110);
        assert_eq!(contributions[1].model.as_deref(), Some("gpt-test"));
        assert_eq!(contributions[1].usage.total(), 230);
        std::fs::remove_file(path).ok();
    }
}
