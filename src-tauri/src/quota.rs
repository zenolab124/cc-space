use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

mod claude;
mod codex;
mod tray_title;

pub use claude::{ExtraUsage, ModelQuota, QuotaInfo, QuotaWindow};
pub use tray_title::{TrayTitleConfig, TrayTitleConfigV2, TrayTitleSlot, TrayTitleSlotV2};

const PROVIDER_CACHE_TTL_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshIntent {
    Normal,
    Immediate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBundle {
    pub schema_version: u32,
    pub providers: Vec<ProviderQuota>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuota {
    pub id: String,
    pub display_name: String,
    pub visible: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub groups: Vec<QuotaGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub stale: bool,
    pub in_flight: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProviderQuotaError>,
}

impl ProviderQuota {
    fn unavailable(id: &str, display_name: &str, kind: &str, message: &str) -> Self {
        Self {
            id: id.to_string(),
            display_name: display_name.to_string(),
            visible: id == "claude",
            available: false,
            account_label: None,
            plan: None,
            groups: vec![],
            updated_at: None,
            stale: false,
            in_flight: false,
            retry_after_secs: None,
            error: Some(ProviderQuotaError {
                kind: kind.to_string(),
                message: message.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaGroup {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits: Option<QuotaCredits>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reached_type: Option<String>,
    pub items: Vec<QuotaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaItem {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_duration_mins: Option<i64>,
    pub kind: QuotaItemKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaItemKind {
    FiveHour,
    Weekly,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCredits {
    pub has_credits: bool,
    pub unlimited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuotaError {
    pub kind: String,
    pub message: String,
}

trait QuotaProvider: Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn gate(&self) -> &'static Mutex<()>;
    fn load_unlocked(&self, intent: RefreshIntent) -> ProviderQuota;
    fn peek(&self) -> Option<ProviderQuota>;
    fn watch_paths(&self) -> Vec<PathBuf>;
}

struct ClaudeProvider;
struct CodexProvider;

static CLAUDE_PROVIDER: ClaudeProvider = ClaudeProvider;
static CODEX_PROVIDER: CodexProvider = CodexProvider;
static CLAUDE_GATE: Mutex<()> = Mutex::new(());
static CODEX_GATE: Mutex<()> = Mutex::new(());

static PROVIDERS: [&dyn QuotaProvider; 2] = [&CLAUDE_PROVIDER, &CODEX_PROVIDER];

impl QuotaProvider for ClaudeProvider {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn gate(&self) -> &'static Mutex<()> {
        &CLAUDE_GATE
    }

    fn load_unlocked(&self, intent: RefreshIntent) -> ProviderQuota {
        let info = match intent {
            RefreshIntent::Normal => claude::get_quota(),
            RefreshIntent::Immediate => claude::refresh_quota(),
        };
        claude_provider_quota(info)
    }

    fn peek(&self) -> Option<ProviderQuota> {
        claude::peek_cached_quota().map(claude_provider_quota)
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        vec![claude::disk_cache_path(), claude::backoff_path()]
    }
}

impl QuotaProvider for CodexProvider {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn gate(&self) -> &'static Mutex<()> {
        &CODEX_GATE
    }

    fn load_unlocked(&self, intent: RefreshIntent) -> ProviderQuota {
        codex::get_quota(intent)
    }

    fn peek(&self) -> Option<ProviderQuota> {
        codex::peek_provider_quota()
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        vec![codex::disk_cache_path(), codex::backoff_path()]
    }
}

fn try_with_provider_lock<T>(
    provider: &dyn QuotaProvider,
    action: impl FnOnce() -> T,
) -> Option<T> {
    let _guard = provider.gate().try_lock().ok()?;

    let lock_path = crate::config::data_dir().join(format!("quota-refresh-{}.lock", provider.id()));
    let lock_file = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
    {
        Ok(file) => file,
        Err(_) => return Some(action()),
    };
    if lock_file.try_lock_exclusive().is_err() {
        return None;
    }

    let result = action();
    let _ = FileExt::unlock(&lock_file);
    Some(result)
}

fn load_provider(provider: &dyn QuotaProvider, intent: RefreshIntent) -> ProviderQuota {
    try_with_provider_lock(provider, || provider.load_unlocked(intent))
        .unwrap_or_else(|| in_flight_snapshot(provider))
}

fn in_flight_snapshot(provider: &dyn QuotaProvider) -> ProviderQuota {
    let mut snapshot = provider.peek().unwrap_or_else(|| {
        ProviderQuota::unavailable(
            provider.id(),
            provider.display_name(),
            "refresh_in_progress",
            "Refresh already in progress",
        )
    });
    snapshot.in_flight = true;
    snapshot
}

fn load_bundle(intent: RefreshIntent) -> QuotaBundle {
    let providers = std::thread::scope(|scope| {
        let handles: Vec<_> = PROVIDERS
            .iter()
            .map(|provider| scope.spawn(move || load_provider(*provider, intent)))
            .collect();
        handles
            .into_iter()
            .enumerate()
            .map(|(index, handle)| {
                handle.join().unwrap_or_else(|_| {
                    let provider = PROVIDERS[index];
                    ProviderQuota::unavailable(
                        provider.id(),
                        provider.display_name(),
                        "internal",
                        "Quota provider failed",
                    )
                })
            })
            .collect()
    });

    QuotaBundle {
        schema_version: 1,
        providers,
        generated_at: Utc::now().to_rfc3339(),
    }
}

#[tauri::command]
pub fn get_quota_bundle() -> QuotaBundle {
    load_bundle(RefreshIntent::Normal)
}

#[tauri::command]
pub fn refresh_quota_bundle() -> QuotaBundle {
    load_bundle(RefreshIntent::Immediate)
}

pub fn peek_quota_bundle() -> QuotaBundle {
    QuotaBundle {
        schema_version: 1,
        providers: PROVIDERS
            .iter()
            .map(|provider| {
                provider.peek().unwrap_or_else(|| {
                    ProviderQuota::unavailable(
                        provider.id(),
                        provider.display_name(),
                        "unavailable",
                        "No quota data available",
                    )
                })
            })
            .collect(),
        generated_at: Utc::now().to_rfc3339(),
    }
}

pub fn provider_watch_paths() -> Vec<PathBuf> {
    PROVIDERS
        .iter()
        .flat_map(|provider| provider.watch_paths())
        .collect()
}

fn claude_refresh_in_progress() -> QuotaInfo {
    QuotaInfo {
        session: None,
        weekly: None,
        weekly_models: vec![],
        extra_usage: None,
        plan: None,
        account_email: None,
        updated_at: Utc::now().to_rfc3339(),
        error: Some("Refresh already in progress".into()),
        error_kind: Some("refresh_in_progress".into()),
    }
}

#[tauri::command]
pub fn get_quota() -> QuotaInfo {
    try_with_provider_lock(&CLAUDE_PROVIDER, claude::get_quota)
        .or_else(claude::peek_cached_quota)
        .unwrap_or_else(claude_refresh_in_progress)
}

#[tauri::command]
pub fn refresh_quota() -> QuotaInfo {
    try_with_provider_lock(&CLAUDE_PROVIDER, claude::refresh_quota)
        .or_else(claude::peek_cached_quota)
        .unwrap_or_else(claude_refresh_in_progress)
}

#[tauri::command]
pub fn quota_available() -> bool {
    claude::quota_available()
}

pub fn notify_session_activity() {
    claude::notify_session_activity(|now_ms| {
        let _ = try_with_provider_lock(&CLAUDE_PROVIDER, || {
            claude::refresh_for_activity(now_ms);
        });
    });
}

pub fn backoff_remaining_secs() -> Option<i64> {
    claude::backoff_remaining_secs()
}

pub fn peek_cached_quota() -> Option<QuotaInfo> {
    claude::peek_cached_quota()
}

pub fn secs_until(resets_at: Option<&str>) -> Option<i64> {
    claude::secs_until(resets_at)
}

pub fn read_tray_title_config() -> TrayTitleConfig {
    tray_title::read_v1()
}

pub fn read_tray_title_config_v2() -> TrayTitleConfigV2 {
    tray_title::read_v2()
}

pub fn tray_title_config_path() -> PathBuf {
    tray_title::config_path()
}

#[tauri::command]
pub fn get_tray_title_config() -> TrayTitleConfig {
    tray_title::read_v1()
}

#[tauri::command]
pub fn set_tray_title_config(slots: Vec<TrayTitleSlot>) -> Result<(), String> {
    tray_title::set_v1(slots)
}

#[tauri::command]
pub fn get_tray_title_config_v2() -> TrayTitleConfigV2 {
    tray_title::read_v2()
}

#[tauri::command]
pub fn set_tray_title_config_v2(slots: Vec<TrayTitleSlotV2>) -> Result<(), String> {
    tray_title::set_v2(slots)
}

pub fn format_tray_title(info: &QuotaInfo) -> Option<String> {
    tray_title::format_v1(info)
}

pub fn format_bundle_title(bundle: &QuotaBundle) -> Option<String> {
    tray_title::format_bundle(bundle)
}

pub fn tray_title_references_provider(provider: &str) -> bool {
    tray_title::references_provider(provider)
}

pub fn format_tray_tooltip(info: &QuotaInfo) -> String {
    claude::format_tray_tooltip(info)
}

fn claude_provider_quota(info: QuotaInfo) -> ProviderQuota {
    let mut items = Vec::new();
    if let Some(window) = &info.session {
        items.push(QuotaItem {
            id: "default/session".into(),
            label: "Session".into(),
            used_percent: Some(window.used_percent),
            resets_at: window.resets_at.clone(),
            window_duration_mins: Some(300),
            kind: QuotaItemKind::FiveHour,
            scope: None,
        });
    }
    if let Some(window) = &info.weekly {
        items.push(QuotaItem {
            id: "default/weekly".into(),
            label: "Weekly".into(),
            used_percent: Some(window.used_percent),
            resets_at: window.resets_at.clone(),
            window_duration_mins: Some(10_080),
            kind: QuotaItemKind::Weekly,
            scope: None,
        });
    }
    for model in &info.weekly_models {
        let name = model.display_name.as_deref().unwrap_or(&model.model);
        items.push(QuotaItem {
            id: format!("default/model:{}", name.to_ascii_lowercase()),
            label: name.to_string(),
            used_percent: Some(model.used_percent),
            resets_at: model.resets_at.clone(),
            window_duration_mins: Some(10_080),
            kind: QuotaItemKind::Weekly,
            scope: Some(model.model.clone()),
        });
    }

    let error = info.error.as_ref().map(|message| ProviderQuotaError {
        kind: info.error_kind.clone().unwrap_or_else(|| "api".into()),
        message: message.clone(),
    });
    let has_data = !items.is_empty();
    ProviderQuota {
        id: "claude".into(),
        display_name: "Claude Code".into(),
        visible: true,
        available: claude::quota_available() || has_data,
        account_label: info.account_email.clone(),
        plan: info.plan.clone(),
        groups: if has_data {
            vec![QuotaGroup {
                id: "default".into(),
                label: "Claude Code".into(),
                plan: info.plan.clone(),
                credits: None,
                reached_type: None,
                items,
            }]
        } else {
            vec![]
        },
        updated_at: Some(info.updated_at.clone()),
        stale: error.is_some() && has_data,
        in_flight: false,
        retry_after_secs: claude::backoff_remaining_secs(),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_order_is_stable() {
        assert_eq!(
            PROVIDERS.iter().map(|p| p.id()).collect::<Vec<_>>(),
            ["claude", "codex"]
        );
    }

    #[test]
    fn legacy_quota_json_stays_provider_free() {
        let info = QuotaInfo {
            session: None,
            weekly: None,
            weekly_models: vec![],
            extra_usage: None,
            plan: None,
            account_email: None,
            updated_at: "2026-08-01T00:00:00Z".into(),
            error: None,
            error_kind: None,
        };
        let value = serde_json::to_value(info).unwrap();
        assert!(value.get("provider").is_none());
        assert!(value.get("weeklyModels").is_some());
        assert!(value.get("updatedAt").is_some());
    }

    #[test]
    fn claude_maps_to_generic_items() {
        let info = QuotaInfo {
            session: Some(QuotaWindow {
                used_percent: 42.0,
                resets_at: None,
                resets_in_secs: None,
            }),
            weekly: None,
            weekly_models: vec![],
            extra_usage: None,
            plan: Some("Max".into()),
            account_email: None,
            updated_at: "2026-08-01T00:00:00Z".into(),
            error: None,
            error_kind: None,
        };
        let mapped = claude_provider_quota(info);
        assert_eq!(mapped.groups[0].items[0].id, "default/session");
        assert_eq!(mapped.groups[0].items[0].kind, QuotaItemKind::FiveHour);
    }

    #[test]
    fn provider_lock_skips_second_refresh_action() {
        let guard = CLAUDE_GATE.lock().unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let result = try_with_provider_lock(&CLAUDE_PROVIDER, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        drop(guard);

        assert!(result.is_none());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn provider_cache_ttl_is_five_minutes() {
        assert_eq!(PROVIDER_CACHE_TTL_SECS, 300);
    }
}
