use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    AssetRef, ConversationPage, CoreProject, CoreSessionSummary, EngineResult, ProjectRef,
    ResolvedAsset, SessionActions, SessionRef,
};

pub type EngineFuture<'a, T> = Pin<Box<dyn Future<Output = EngineResult<T>> + Send + 'a>>;

pub trait EngineSubscription: Send + Sync {}

impl<T: Send + Sync> EngineSubscription for T {}

pub type SubscriptionHandle = Box<dyn EngineSubscription>;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPage {
    pub projects: Vec<CoreProject>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    pub sessions: Vec<CoreSessionSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePage {
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Default for TimelinePage {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceChangeKind {
    ProjectsChanged,
    SessionChanged,
    SessionRemoved,
    FullRefresh,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceChange {
    pub kind: SourceChangeKind,
    pub project: Option<ProjectRef>,
    pub session: Option<SessionRef>,
}

pub type SourceChangeSink = Arc<dyn Fn(SourceChange) + Send + Sync>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchDocument {
    pub session: SessionRef,
    pub title: Option<String>,
    pub text: String,
}

pub trait SessionSource: Send + Sync {
    fn list_projects(&self, query: ProjectQuery) -> EngineFuture<'_, ProjectPage>;

    fn list_sessions(
        &self,
        project: ProjectRef,
        query: SessionQuery,
    ) -> EngineFuture<'_, SessionPage>;

    fn load_timeline(
        &self,
        session: SessionRef,
        page: TimelinePage,
    ) -> EngineFuture<'_, ConversationPage>;

    fn subscribe_changes(&self, sink: SourceChangeSink) -> EngineResult<SubscriptionHandle>;

    fn build_search_document(&self, session: SessionRef) -> EngineFuture<'_, SearchDocument>;

    fn resolve_asset(&self, asset: AssetRef) -> EngineFuture<'_, ResolvedAsset>;

    fn session_actions(&self, session: SessionRef) -> EngineFuture<'_, SessionActions>;
}
