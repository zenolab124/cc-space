use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::{EngineError, EngineErrorKind, EngineResult};

const INSTANCE_KEY_PREFIX: &str = "ei1";
const PROJECT_KEY_PREFIX: &str = "pr1";
const SESSION_KEY_PREFIX: &str = "sr1";

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", try_from = "RawEngineInstanceId")]
pub struct EngineInstanceId {
    engine_id: String,
    instance_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEngineInstanceId {
    engine_id: String,
    instance_id: String,
}

impl EngineInstanceId {
    pub fn new(engine_id: impl Into<String>, instance_id: impl Into<String>) -> EngineResult<Self> {
        let engine_id = engine_id.into();
        let instance_id = instance_id.into();
        validate_named_component("engineId", &engine_id)?;
        validate_named_component("instanceId", &instance_id)?;
        Ok(Self {
            engine_id,
            instance_id,
        })
    }

    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn storage_key(&self) -> String {
        encode_key(
            INSTANCE_KEY_PREFIX,
            [self.engine_id.as_str(), self.instance_id.as_str()],
        )
    }

    pub fn from_storage_key(value: &str) -> EngineResult<Self> {
        let parts = decode_key(value, INSTANCE_KEY_PREFIX, 2)?;
        Self::new(&parts[0], &parts[1])
    }
}

impl TryFrom<RawEngineInstanceId> for EngineInstanceId {
    type Error = EngineError;

    fn try_from(value: RawEngineInstanceId) -> Result<Self, Self::Error> {
        Self::new(value.engine_id, value.instance_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", try_from = "RawProjectRef")]
pub struct ProjectRef {
    engine: EngineInstanceId,
    native_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProjectRef {
    engine: EngineInstanceId,
    native_id: String,
}

impl ProjectRef {
    pub fn new(engine: EngineInstanceId, native_id: impl Into<String>) -> EngineResult<Self> {
        let native_id = native_id.into();
        validate_native_id(&native_id)?;
        Ok(Self { engine, native_id })
    }

    pub fn engine(&self) -> &EngineInstanceId {
        &self.engine
    }

    pub fn native_id(&self) -> &str {
        &self.native_id
    }

    pub fn storage_key(&self) -> String {
        encode_key(
            PROJECT_KEY_PREFIX,
            [
                self.engine.engine_id(),
                self.engine.instance_id(),
                &self.native_id,
            ],
        )
    }

    pub fn from_storage_key(value: &str) -> EngineResult<Self> {
        let parts = decode_key(value, PROJECT_KEY_PREFIX, 3)?;
        Self::new(EngineInstanceId::new(&parts[0], &parts[1])?, &parts[2])
    }
}

impl TryFrom<RawProjectRef> for ProjectRef {
    type Error = EngineError;

    fn try_from(value: RawProjectRef) -> Result<Self, Self::Error> {
        Self::new(value.engine, value.native_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", try_from = "RawSessionRef")]
pub struct SessionRef {
    engine: EngineInstanceId,
    native_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSessionRef {
    engine: EngineInstanceId,
    native_id: String,
}

impl SessionRef {
    pub fn new(engine: EngineInstanceId, native_id: impl Into<String>) -> EngineResult<Self> {
        let native_id = native_id.into();
        validate_native_id(&native_id)?;
        Ok(Self { engine, native_id })
    }

    pub fn engine(&self) -> &EngineInstanceId {
        &self.engine
    }

    pub fn native_id(&self) -> &str {
        &self.native_id
    }

    pub fn storage_key(&self) -> String {
        encode_key(
            SESSION_KEY_PREFIX,
            [
                self.engine.engine_id(),
                self.engine.instance_id(),
                &self.native_id,
            ],
        )
    }

    pub fn from_storage_key(value: &str) -> EngineResult<Self> {
        let parts = decode_key(value, SESSION_KEY_PREFIX, 3)?;
        Self::new(EngineInstanceId::new(&parts[0], &parts[1])?, &parts[2])
    }
}

impl TryFrom<RawSessionRef> for SessionRef {
    type Error = EngineError;

    fn try_from(value: RawSessionRef) -> Result<Self, Self::Error> {
        Self::new(value.engine, value.native_id)
    }
}

fn validate_named_component(name: &str, value: &str) -> EngineResult<()> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        });
    if valid {
        return Ok(());
    }
    Err(EngineError::new(
        EngineErrorKind::InvalidIdentity,
        format!("{name} must use 1-64 lowercase ASCII letters, digits, '-', '_' or '.'"),
    ))
}

fn validate_native_id(value: &str) -> EngineResult<()> {
    if !value.is_empty() && value.len() <= 4096 && !value.contains('\0') {
        return Ok(());
    }
    Err(EngineError::new(
        EngineErrorKind::InvalidIdentity,
        "nativeId must contain 1-4096 bytes and no NUL character",
    ))
}

fn encode_key<'a>(prefix: &str, values: impl IntoIterator<Item = &'a str>) -> String {
    std::iter::once(prefix.to_string())
        .chain(
            values
                .into_iter()
                .map(|value| URL_SAFE_NO_PAD.encode(value.as_bytes())),
        )
        .collect::<Vec<_>>()
        .join(".")
}

fn decode_key(value: &str, prefix: &str, component_count: usize) -> EngineResult<Vec<String>> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.first() != Some(&prefix) || parts.len() != component_count + 1 {
        return Err(EngineError::new(
            EngineErrorKind::InvalidIdentity,
            "invalid engine storage key",
        ));
    }

    parts[1..]
        .iter()
        .map(|part| {
            URL_SAFE_NO_PAD
                .decode(part)
                .map_err(|_| {
                    EngineError::new(
                        EngineErrorKind::InvalidIdentity,
                        "invalid engine storage key encoding",
                    )
                })
                .and_then(|bytes| {
                    String::from_utf8(bytes).map_err(|_| {
                        EngineError::new(
                            EngineErrorKind::InvalidIdentity,
                            "engine storage key is not UTF-8",
                        )
                    })
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_keys_round_trip_opaque_native_ids() {
        let engine = EngineInstanceId::new("sample-engine", "default").unwrap();
        let session = SessionRef::new(engine.clone(), "folder/id:with symbols").unwrap();
        let project = ProjectRef::new(engine.clone(), "project/一").unwrap();

        assert_eq!(
            EngineInstanceId::from_storage_key(&engine.storage_key()).unwrap(),
            engine
        );
        assert_eq!(
            SessionRef::from_storage_key(&session.storage_key()).unwrap(),
            session
        );
        assert_eq!(
            ProjectRef::from_storage_key(&project.storage_key()).unwrap(),
            project
        );
    }

    #[test]
    fn same_native_id_isolated_by_engine_instance() {
        let first = SessionRef::new(
            EngineInstanceId::new("engine-a", "default").unwrap(),
            "same-id",
        )
        .unwrap();
        let second = SessionRef::new(
            EngineInstanceId::new("engine-b", "default").unwrap(),
            "same-id",
        )
        .unwrap();

        assert_ne!(first, second);
        assert_ne!(first.storage_key(), second.storage_key());
    }

    #[test]
    fn serde_rejects_invalid_instance_names() {
        let error = serde_json::from_str::<EngineInstanceId>(
            r#"{"engineId":"Uppercase","instanceId":"default"}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("engineId"));
    }
}
