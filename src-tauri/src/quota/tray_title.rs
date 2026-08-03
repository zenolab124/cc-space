use serde::{Deserialize, Serialize};

use super::{QuotaBundle, QuotaInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayTitleConfig {
    pub slots: Vec<TrayTitleSlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrayTitleSlot {
    Session,
    Weekly,
    Model(String),
}

impl Default for TrayTitleConfig {
    fn default() -> Self {
        Self {
            slots: vec![TrayTitleSlot::Session],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrayTitleConfigV2 {
    pub version: u32,
    pub slots: Vec<TrayTitleSlotV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrayTitleSlotV2 {
    pub provider: String,
    pub item: String,
}

impl Default for TrayTitleConfigV2 {
    fn default() -> Self {
        Self {
            version: 2,
            slots: vec![TrayTitleSlotV2 {
                provider: "claude".into(),
                item: "default/session".into(),
            }],
        }
    }
}

pub fn config_path() -> std::path::PathBuf {
    crate::config::data_dir().join("tray-title.json")
}

pub fn read_v1() -> TrayTitleConfig {
    let Ok(content) = std::fs::read_to_string(config_path()) else {
        return TrayTitleConfig::default();
    };
    if let Ok(config) = serde_json::from_str::<TrayTitleConfig>(&content) {
        return config;
    }
    serde_json::from_str::<TrayTitleConfigV2>(&content)
        .map(v2_to_v1)
        .unwrap_or_default()
}

pub fn read_v2() -> TrayTitleConfigV2 {
    let Ok(content) = std::fs::read_to_string(config_path()) else {
        return TrayTitleConfigV2::default();
    };
    if let Ok(config) = serde_json::from_str::<TrayTitleConfigV2>(&content) {
        if config.version == 2 {
            return config;
        }
    }
    serde_json::from_str::<TrayTitleConfig>(&content)
        .map(v1_to_v2)
        .unwrap_or_default()
}

pub fn set_v1(slots: Vec<TrayTitleSlot>) -> Result<(), String> {
    let existing = read_v2();
    let mut migrated = v1_to_v2(TrayTitleConfig { slots });
    migrated.slots.extend(
        existing
            .slots
            .into_iter()
            .filter(|slot| slot.provider != "claude"),
    );
    write_v2(&migrated)
}

pub fn set_v2(slots: Vec<TrayTitleSlotV2>) -> Result<(), String> {
    write_v2(&TrayTitleConfigV2 { version: 2, slots })
}

fn write_v2(config: &TrayTitleConfigV2) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
    crate::config::atomic_write(&config_path(), &json).map_err(|error| error.to_string())
}

fn v1_to_v2(config: TrayTitleConfig) -> TrayTitleConfigV2 {
    let slots = config
        .slots
        .into_iter()
        .map(|slot| TrayTitleSlotV2 {
            provider: "claude".into(),
            item: match slot {
                TrayTitleSlot::Session => "default/session".into(),
                TrayTitleSlot::Weekly => "default/weekly".into(),
                TrayTitleSlot::Model(name) => {
                    format!("default/model:{}", name.to_ascii_lowercase())
                }
            },
        })
        .collect();
    TrayTitleConfigV2 { version: 2, slots }
}

fn v2_to_v1(config: TrayTitleConfigV2) -> TrayTitleConfig {
    let slots = config
        .slots
        .into_iter()
        .filter_map(|slot| {
            if slot.provider != "claude" {
                return None;
            }
            match slot.item.as_str() {
                "default/session" => Some(TrayTitleSlot::Session),
                "default/weekly" => Some(TrayTitleSlot::Weekly),
                item => item
                    .strip_prefix("default/model:")
                    .map(|name| TrayTitleSlot::Model(title_case(name))),
            }
        })
        .collect();
    TrayTitleConfig { slots }
}

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

pub fn format_v1(info: &QuotaInfo) -> Option<String> {
    let config = read_v1();
    let parts: Vec<_> = config
        .slots
        .iter()
        .filter_map(|slot| match slot {
            TrayTitleSlot::Session => info
                .session
                .as_ref()
                .map(|window| format!("{:.0}%", window.used_percent)),
            TrayTitleSlot::Weekly => info
                .weekly
                .as_ref()
                .map(|window| format!("{:.0}%", window.used_percent)),
            TrayTitleSlot::Model(name) => info
                .weekly_models
                .iter()
                .find(|model| {
                    model.display_name.as_deref() == Some(name.as_str())
                        || model.model.eq_ignore_ascii_case(name)
                })
                .map(|model| format!("{:.0}%", model.used_percent)),
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" | "))
}

pub fn format_bundle(bundle: &QuotaBundle) -> Option<String> {
    let config = read_v2();
    let multi_provider = {
        let mut providers: Vec<&str> = config
            .slots
            .iter()
            .map(|slot| slot.provider.as_str())
            .collect();
        providers.sort_unstable();
        providers.dedup();
        providers.len() > 1
    };

    let parts: Vec<_> = config
        .slots
        .iter()
        .filter_map(|slot| {
            let provider = bundle
                .providers
                .iter()
                .find(|item| item.id == slot.provider)?;
            let item = provider
                .groups
                .iter()
                .flat_map(|group| &group.items)
                .find(|item| item.id == slot.item)?;
            let percent = item.used_percent?;
            Some(if multi_provider {
                format!("{} {:.0}%", provider.display_name, percent)
            } else {
                format!("{percent:.0}%")
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" | "))
}

pub fn references_provider(provider: &str) -> bool {
    read_v2().slots.iter().any(|slot| slot.provider == provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_slots_migrate_to_claude_without_writing() {
        let config = v1_to_v2(TrayTitleConfig {
            slots: vec![TrayTitleSlot::Session, TrayTitleSlot::Model("Fable".into())],
        });
        assert_eq!(config.version, 2);
        assert_eq!(config.slots[0].provider, "claude");
        assert_eq!(config.slots[1].item, "default/model:fable");
    }

    #[test]
    fn v2_to_v1_ignores_non_claude_slots() {
        let config = v2_to_v1(TrayTitleConfigV2 {
            version: 2,
            slots: vec![
                TrayTitleSlotV2 {
                    provider: "codex".into(),
                    item: "default/primary".into(),
                },
                TrayTitleSlotV2 {
                    provider: "claude".into(),
                    item: "default/weekly".into(),
                },
            ],
        });
        assert_eq!(config.slots, vec![TrayTitleSlot::Weekly]);
    }
}
