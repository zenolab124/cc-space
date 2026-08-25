use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    ProviderQuota, ProviderQuotaError, QuotaCredits, QuotaGroup, QuotaItem, QuotaItemKind,
    RefreshIntent, PROVIDER_CACHE_TTL_SECS,
};
use crate::engines::codex::app_server::{
    AppServerClient, AppServerError, AppServerErrorKind,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(24);
const DEFAULT_BACKOFF_SECS: i64 = 60;

#[derive(Serialize, Deserialize)]
struct DiskCache {
    info: ProviderQuota,
    fetched_at_ms: i64,
}

#[derive(Serialize, Deserialize)]
struct BackoffState {
    until_ms: i64,
}

#[derive(Debug)]
struct CodexError {
    kind: &'static str,
    message: &'static str,
    retry_after_secs: Option<i64>,
}

impl CodexError {
    fn new(kind: &'static str, message: &'static str) -> Self {
        Self {
            kind,
            message,
            retry_after_secs: None,
        }
    }

    fn with_retry(mut self, seconds: i64) -> Self {
        self.retry_after_secs = Some(seconds.clamp(1, 3600));
        self
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountReadResult {
    account: Option<AccountInfo>,
    #[serde(default)]
    requires_openai_auth: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum AccountInfo {
    Chatgpt {
        email: Option<String>,
        #[serde(rename = "planType")]
        plan_type: Option<String>,
    },
    ApiKey,
    AmazonBedrock,
    #[serde(other)]
    Other,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsReadResult {
    rate_limits: Option<RateLimitSnapshot>,
    #[serde(default)]
    rate_limits_by_limit_id: BTreeMap<String, RateLimitSnapshot>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitSnapshot {
    limit_id: Option<String>,
    limit_name: Option<String>,
    plan_type: Option<String>,
    primary: Option<RateLimitWindow>,
    secondary: Option<RateLimitWindow>,
    credits: Option<RawCredits>,
    individual_limit: Option<String>,
    rate_limit_reached_type: Option<String>,
    spend_control_reached: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindow {
    used_percent: Option<f64>,
    resets_at: Option<i64>,
    window_duration_mins: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCredits {
    #[serde(default)]
    has_credits: bool,
    #[serde(default)]
    unlimited: bool,
    balance: Option<Value>,
}

pub(super) fn disk_cache_path() -> PathBuf {
    crate::config::data_dir().join("quota-cache-codex.json")
}

pub(super) fn backoff_path() -> PathBuf {
    crate::config::data_dir().join("quota-backoff-codex.json")
}

fn read_disk_cache() -> Option<DiskCache> {
    let content = std::fs::read_to_string(disk_cache_path()).ok()?;
    let mut cache: DiskCache = serde_json::from_str(&content).ok()?;
    cache
        .info
        .groups
        .retain(|group| !should_hide_label(&group.label));
    Some(cache)
}

fn write_disk_cache(info: &ProviderQuota) {
    let cache = DiskCache {
        info: info.clone(),
        fetched_at_ms: Utc::now().timestamp_millis(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = crate::config::atomic_write(&disk_cache_path(), &json);
    }
}

fn backoff_remaining_secs() -> Option<i64> {
    let content = std::fs::read_to_string(backoff_path()).ok()?;
    let state: BackoffState = serde_json::from_str(&content).ok()?;
    let remaining = (state.until_ms - Utc::now().timestamp_millis() + 999) / 1000;
    (remaining > 0).then_some(remaining)
}

fn write_backoff(seconds: i64) {
    let state = BackoffState {
        until_ms: Utc::now().timestamp_millis() + seconds.clamp(1, 3600) * 1000,
    };
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = crate::config::atomic_write(&backoff_path(), &json);
    }
}

pub(super) fn get_quota(intent: RefreshIntent) -> ProviderQuota {
    if let Some(remaining) = backoff_remaining_secs() {
        return stale_or_error(
            CodexError::new("rate_limited", "Codex quota is temporarily unavailable")
                .with_retry(remaining),
        );
    }

    if intent == RefreshIntent::Normal {
        if let Some(cache) = read_disk_cache() {
            let age_secs = (Utc::now().timestamp_millis() - cache.fetched_at_ms) / 1000;
            if age_secs < PROVIDER_CACHE_TTL_SECS {
                return cache.info;
            }
        }
    }

    if !crate::codex_locator::is_available() {
        return stale_or_error(CodexError::new(
            "cli_not_found",
            "Codex CLI is not installed",
        ));
    }

    match fetch_quota() {
        Ok(info) => {
            write_disk_cache(&info);
            info
        }
        Err(error) => {
            if let Some(seconds) = error.retry_after_secs {
                write_backoff(seconds.max(DEFAULT_BACKOFF_SECS));
            }
            stale_or_error(error)
        }
    }
}

pub(super) fn peek_provider_quota() -> Option<ProviderQuota> {
    peek_cached_quota().or_else(|| {
        let visible =
            crate::codex_locator::is_available() || super::tray_title_references_provider("codex");
        visible.then(|| {
            let mut quota = ProviderQuota::unavailable(
                "codex",
                "Codex",
                "unavailable",
                "Codex quota has not been loaded yet",
            );
            quota.visible = true;
            quota
        })
    })
}

pub(super) fn peek_cached_quota() -> Option<ProviderQuota> {
    read_disk_cache().map(|mut cache| {
        cache.info.retry_after_secs = backoff_remaining_secs();
        cache.info
    })
}

fn stale_or_error(error: CodexError) -> ProviderQuota {
    if let Some(mut cached) = peek_cached_quota() {
        cached.stale = true;
        cached.retry_after_secs = error.retry_after_secs.or_else(backoff_remaining_secs);
        cached.error = Some(ProviderQuotaError {
            kind: error.kind.into(),
            message: error.message.into(),
        });
        return cached;
    }

    let mut unavailable = ProviderQuota::unavailable("codex", "Codex", error.kind, error.message);
    unavailable.visible = crate::codex_locator::is_available()
        || disk_cache_path().is_file()
        || super::tray_title_references_provider("codex");
    unavailable.retry_after_secs = error.retry_after_secs.or_else(backoff_remaining_secs);
    unavailable
}

fn fetch_quota() -> Result<ProviderQuota, CodexError> {
    let path = crate::codex_locator::locate()
        .map_err(|_| CodexError::new("cli_not_found", "Codex CLI is not installed"))?;
    let mut server = AppServerClient::spawn(&path).map_err(map_app_server_error)?;
    let deadline = Instant::now() + TOTAL_TIMEOUT;

    server.request(
        0,
        "initialize",
        json!({
            "clientInfo": {
                "name": "monet",
                "title": "Monet",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
        deadline,
        REQUEST_TIMEOUT,
    )
    .map_err(map_app_server_error)?;
    server
        .notify("initialized", json!({}))
        .map_err(map_app_server_error)?;

    let account_value = server.request(
        1,
        "account/read",
        json!({ "refreshToken": false }),
        deadline,
        REQUEST_TIMEOUT,
    )
    .map_err(map_app_server_error)?;
    let account: AccountReadResult = serde_json::from_value(account_value)
        .map_err(|_| CodexError::new("protocol", "Codex returned invalid account data"))?;
    if account.requires_openai_auth && account.account.is_none() {
        return Err(CodexError::new(
            "not_logged_in",
            "Sign in with the Codex CLI to view quota",
        ));
    }

    let limits_value = server
        .request(
            2,
            "account/rateLimits/read",
            json!({}),
            deadline,
            REQUEST_TIMEOUT,
        )
        .map_err(map_app_server_error)?;
    let limits: RateLimitsReadResult = serde_json::from_value(limits_value)
        .map_err(|_| CodexError::new("protocol", "Codex returned invalid quota data"))?;

    map_quota(account, limits)
}

fn map_app_server_error(error: AppServerError) -> CodexError {
    let mut mapped = match error.kind() {
        AppServerErrorKind::Spawn => {
            CodexError::new("spawn", "Codex app-server could not be started")
        }
        AppServerErrorKind::Protocol => {
            CodexError::new("protocol", "Codex app-server returned an invalid response")
        }
        AppServerErrorKind::MessageTooLarge => CodexError::new(
            "protocol",
            "Codex app-server response exceeded Monet's safety limit",
        ),
        AppServerErrorKind::Timeout => {
            CodexError::new("timeout", "Codex quota request timed out")
        }
        AppServerErrorKind::Io => {
            CodexError::new("io", "Codex app-server connection was closed")
        }
        AppServerErrorKind::Eof => {
            CodexError::new("eof", "Codex app-server stopped unexpectedly")
        }
        AppServerErrorKind::Rpc => {
            CodexError::new("rpc", "Codex app-server rejected the quota request")
        }
    };
    if let Some(seconds) = error
        .rpc_error()
        .and_then(|rpc_error| rpc_error.data.as_ref())
        .and_then(explicit_retry_after)
    {
        mapped = mapped.with_retry(seconds);
    }
    mapped
}

fn explicit_retry_after(value: &Value) -> Option<i64> {
    for key in ["retryAfterSecs", "retryAfter", "retry_after_secs"] {
        if let Some(seconds) = value.get(key).and_then(Value::as_i64) {
            return Some(seconds);
        }
    }
    value.get("data").and_then(explicit_retry_after)
}

fn should_hide_label(label: &str) -> bool {
    let normalized = label.to_ascii_lowercase();
    normalized.contains("codex-spark") || normalized.contains("codex spark")
}

fn should_hide_limit(snapshot: &RateLimitSnapshot) -> bool {
    snapshot
        .limit_name
        .as_deref()
        .or(snapshot.individual_limit.as_deref())
        .is_some_and(should_hide_label)
}

fn map_quota(
    account: AccountReadResult,
    limits: RateLimitsReadResult,
) -> Result<ProviderQuota, CodexError> {
    let (account_label, account_plan) = match account.account {
        Some(AccountInfo::Chatgpt { email, plan_type }) => (email, plan_type),
        Some(AccountInfo::ApiKey) => (None, Some("API key".into())),
        Some(AccountInfo::AmazonBedrock) => (None, Some("Amazon Bedrock".into())),
        Some(AccountInfo::Other) | None => (None, None),
    };

    let mut snapshots = Vec::new();
    if let Some(snapshot) = limits.rate_limits {
        if !should_hide_limit(&snapshot) {
            snapshots.push(("default".to_string(), snapshot));
        }
    }
    for (id, snapshot) in limits.rate_limits_by_limit_id {
        if should_hide_limit(&snapshot) {
            continue;
        }
        if snapshots
            .iter()
            .any(|(_, existing)| existing.limit_id.as_deref() == Some(id.as_str()))
        {
            continue;
        }
        snapshots.push((id, snapshot));
    }
    if snapshots.is_empty() {
        return Err(CodexError::new(
            "unavailable",
            "Codex did not provide subscription quota",
        ));
    }

    let groups: Vec<_> = snapshots
        .into_iter()
        .map(|(fallback_id, snapshot)| map_group(fallback_id, snapshot))
        .collect();
    let plan = account_plan.or_else(|| groups.iter().find_map(|group| group.plan.clone()));

    Ok(ProviderQuota {
        id: "codex".into(),
        display_name: "Codex".into(),
        visible: true,
        available: true,
        account_label,
        plan,
        groups,
        updated_at: Some(Utc::now().to_rfc3339()),
        stale: false,
        in_flight: false,
        retry_after_secs: None,
        error: None,
    })
}

fn map_group(fallback_id: String, snapshot: RateLimitSnapshot) -> QuotaGroup {
    let group_id = snapshot.limit_id.clone().unwrap_or(fallback_id);
    let mut items = Vec::new();
    if let Some(window) = snapshot.primary {
        items.push(map_window(&group_id, "primary", window));
    }
    if let Some(window) = snapshot.secondary {
        items.push(map_window(&group_id, "secondary", window));
    }

    let credits = snapshot.credits.map(|credits| QuotaCredits {
        has_credits: credits.has_credits,
        unlimited: credits.unlimited,
        balance: credits.balance.and_then(balance_string),
    });
    let reached_type = snapshot.rate_limit_reached_type.or_else(|| {
        snapshot
            .spend_control_reached
            .unwrap_or(false)
            .then(|| "spendControl".to_string())
    });

    QuotaGroup {
        id: group_id,
        label: snapshot
            .limit_name
            .or(snapshot.individual_limit)
            .unwrap_or_else(|| "Codex".into()),
        plan: snapshot.plan_type,
        credits,
        reached_type,
        items,
    }
}

fn balance_string(value: Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn map_window(group_id: &str, position: &str, window: RateLimitWindow) -> QuotaItem {
    let kind = match window.window_duration_mins {
        Some(300) => QuotaItemKind::FiveHour,
        Some(10_080) => QuotaItemKind::Weekly,
        _ => QuotaItemKind::Other,
    };
    let label = match (kind, window.window_duration_mins) {
        (QuotaItemKind::FiveHour, _) => "5 hours".into(),
        (QuotaItemKind::Weekly, _) => "Weekly".into(),
        (_, Some(minutes)) => format!("{minutes} minutes"),
        _ => "Usage window".into(),
    };
    QuotaItem {
        id: format!("{group_id}/{position}"),
        label,
        used_percent: window.used_percent,
        resets_at: window
            .resets_at
            .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single())
            .map(|date| date.to_rfc3339()),
        window_duration_mins: window.window_duration_mins,
        kind,
        scope: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_limits(value: Value) -> RateLimitsReadResult {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn window_semantics_come_from_duration_not_position() {
        let limits = parse_limits(json!({
            "rateLimits": {
                "primary": { "usedPercent": 12.5, "windowDurationMins": 10080 },
                "secondary": { "usedPercent": 4.0, "windowDurationMins": 300 }
            }
        }));
        let quota = map_quota(AccountReadResult::default(), limits).unwrap();
        let items = &quota.groups[0].items;
        assert_eq!(items[0].kind, QuotaItemKind::Weekly);
        assert_eq!(items[1].kind, QuotaItemKind::FiveHour);
    }

    #[test]
    fn weekly_only_does_not_fabricate_five_hour_window() {
        let limits = parse_limits(json!({
            "rateLimits": {
                "secondary": { "usedPercent": 44.0, "windowDurationMins": 10080 }
            }
        }));
        let quota = map_quota(AccountReadResult::default(), limits).unwrap();
        assert_eq!(quota.groups[0].items.len(), 1);
        assert_eq!(quota.groups[0].items[0].kind, QuotaItemKind::Weekly);
    }

    #[test]
    fn limit_groups_are_sorted_and_credits_keep_string_precision() {
        let limits = parse_limits(json!({
            "rateLimitsByLimitId": {
                "zeta": {
                    "limitName": "Zeta",
                    "credits": { "hasCredits": true, "unlimited": false, "balance": "12.3400" }
                },
                "alpha": { "limitName": "Alpha", "spendControlReached": null }
            }
        }));
        let quota = map_quota(AccountReadResult::default(), limits).unwrap();
        assert_eq!(quota.groups[0].id, "alpha");
        assert_eq!(quota.groups[1].id, "zeta");
        assert_eq!(
            quota.groups[1].credits.as_ref().unwrap().balance.as_deref(),
            Some("12.3400")
        );
    }

    #[test]
    fn codex_spark_limit_is_not_exposed() {
        let limits = parse_limits(json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "primary": { "usedPercent": 18.0, "windowDurationMins": 10080 }
                },
                "codex_fast": {
                    "limitId": "codex_fast",
                    "limitName": "GPT-5.3-Codex-Spark",
                    "primary": { "usedPercent": 36.0, "windowDurationMins": 10080 }
                }
            }
        }));
        let quota = map_quota(AccountReadResult::default(), limits).unwrap();
        assert_eq!(quota.groups.len(), 1);
        assert_eq!(quota.groups[0].id, "codex");
    }

    #[test]
    fn retry_after_requires_explicit_field() {
        assert_eq!(
            explicit_retry_after(&json!({ "message": "retry after 30 seconds" })),
            None
        );
        assert_eq!(
            explicit_retry_after(&json!({ "data": { "retryAfterSecs": 90 } })),
            Some(90)
        );
    }

    #[test]
    fn account_read_never_requests_refresh() {
        let params = json!({ "refreshToken": false });
        assert_eq!(params.get("refreshToken"), Some(&Value::Bool(false)));
    }

    #[test]
    #[ignore = "requires an installed, signed-in Codex CLI and explicit MONET_CODEX_QUOTA_SMOKE=1"]
    fn real_app_server_smoke() {
        assert_eq!(
            std::env::var("MONET_CODEX_QUOTA_SMOKE").as_deref(),
            Ok("1"),
            "set MONET_CODEX_QUOTA_SMOKE=1 to run the read-only Codex quota smoke test"
        );
        let quota = fetch_quota().expect("Codex app-server quota request should succeed");
        assert_eq!(quota.id, "codex");
        assert!(quota.available);
        assert!(!quota.groups.is_empty());
    }
}
