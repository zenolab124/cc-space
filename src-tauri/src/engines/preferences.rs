use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::core::{EngineError, EngineErrorKind, EngineInstanceId, EngineResult};

const SCHEMA_VERSION: u32 = 1;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnginePreferences {
    schema_version: u32,
    enabled: BTreeMap<String, bool>,
}

impl EnginePreferences {
    fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            enabled: BTreeMap::new(),
        }
    }
}

pub fn is_enabled(instance: &EngineInstanceId) -> bool {
    match read() {
        Ok(preferences) => preferences
            .enabled
            .get(&instance.storage_key())
            .copied()
            .unwrap_or(true),
        Err(error) => {
            log::warn!("engine preferences could not be read: {error}");
            true
        }
    }
}

pub fn set_enabled(instance: &EngineInstanceId, enabled: bool) -> EngineResult<()> {
    let mut preferences = read()?;
    preferences.enabled.insert(instance.storage_key(), enabled);
    let content = serde_json::to_string_pretty(&preferences)
        .map_err(|error| EngineError::new(EngineErrorKind::Internal, error.to_string()))?;
    let path = path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))?;
    }
    crate::config::atomic_write(&path, &content)
        .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))
}

fn read() -> EngineResult<EnginePreferences> {
    let path = path();
    if !path.is_file() {
        return Ok(EnginePreferences::new());
    }
    let content = fs::read_to_string(path)
        .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))?;
    let preferences: EnginePreferences = serde_json::from_str(&content)
        .map_err(|error| EngineError::new(EngineErrorKind::Protocol, error.to_string()))?;
    if preferences.schema_version != SCHEMA_VERSION {
        return Err(EngineError::new(
            EngineErrorKind::Protocol,
            "unsupported engine preferences schema version",
        ));
    }
    Ok(preferences)
}

fn path() -> PathBuf {
    crate::config::data_dir().join("engines.json")
}
