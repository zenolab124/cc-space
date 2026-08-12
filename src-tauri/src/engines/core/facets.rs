use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{EngineFuture, SessionRef};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetItem {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub data: Value,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetQuery {
    pub kind: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetPage {
    pub items: Vec<FacetItem>,
    pub next_cursor: Option<String>,
}

pub trait AssetProvider: Send + Sync {
    fn list_assets(&self, query: FacetQuery) -> EngineFuture<'_, FacetPage>;
}

pub trait QuotaProvider: Send + Sync {
    fn read_quota(&self, force_refresh: bool) -> EngineFuture<'_, Value>;
}

pub trait RuntimeCommandProvider: Send + Sync {
    fn list_commands(&self, session: SessionRef) -> EngineFuture<'_, Vec<FacetItem>>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEffortOption {
    pub id: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelServiceTier {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    pub id: String,
    pub model: String,
    pub display_name: String,
    pub description: Option<String>,
    pub is_default: bool,
    pub hidden: bool,
    pub default_effort: Option<String>,
    pub efforts: Vec<ModelEffortOption>,
    pub default_service_tier: Option<String>,
    pub service_tiers: Vec<ModelServiceTier>,
}

pub trait ModelCatalogProvider: Send + Sync {
    fn list_models(&self) -> EngineFuture<'_, Vec<ModelDescriptor>>;
}
