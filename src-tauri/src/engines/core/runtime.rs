use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    EngineFuture, EngineResult, InteractionKind, ItemStatus, ProjectRef, Segment, SessionRef,
    SubscriptionHandle,
};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RuntimeId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRef {
    pub session: SessionRef,
    pub runtime_id: RuntimeId,
    pub native_turn_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InputItem {
    Text { text: String },
    Image { media_type: String, data: String },
    File { path: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub project: ProjectRef,
    pub cwd: Option<String>,
    pub options: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkSessionRequest {
    pub session: SessionRef,
    pub last_turn_id: Option<String>,
    pub options: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachOptions {
    pub options: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRequest {
    pub input: Vec<InputItem>,
    pub options: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSession {
    pub session: SessionRef,
    pub runtime_id: RuntimeId,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnHandle {
    pub reference: TurnRef,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRef {
    pub session: SessionRef,
    pub runtime_id: RuntimeId,
    pub request_id: String,
    pub turn_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionOption {
    pub id: String,
    pub label: String,
    pub dangerous: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    pub reference: InteractionRef,
    pub kind: InteractionKind,
    pub title: Option<String>,
    pub payload: Value,
    pub options: Vec<InteractionOption>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponse {
    pub decision: String,
    pub payload: Option<Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NormalizedRuntimeEvent {
    SessionAttached,
    SessionDetached,
    TurnStarted {
        turn_id: String,
    },
    ItemStarted {
        turn_id: String,
        item_id: String,
        status: ItemStatus,
    },
    ItemDelta {
        turn_id: String,
        item_id: String,
        segment: Segment,
    },
    ItemCompleted {
        turn_id: String,
        item_id: String,
        status: ItemStatus,
    },
    InteractionRequested {
        request: InteractionRequest,
    },
    InteractionResolved {
        reference: InteractionRef,
        decision: String,
    },
    TurnCompleted {
        turn_id: String,
        status: TurnStatus,
        error: Option<String>,
    },
    RuntimeError {
        message: String,
        retryable: bool,
    },
    RuntimeExited,
    CapabilitiesChanged,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEventEnvelope {
    pub session: SessionRef,
    pub runtime_id: RuntimeId,
    pub generation: u64,
    pub sequence: u64,
    pub timestamp: String,
    pub event: NormalizedRuntimeEvent,
}

pub type RuntimeEventSink = Arc<dyn Fn(RuntimeEventEnvelope) + Send + Sync>;

pub trait AgentRuntime: Send + Sync {
    fn create_session(&self, request: CreateSessionRequest) -> EngineFuture<'_, RuntimeSession>;

    fn fork_session(&self, _request: ForkSessionRequest) -> EngineFuture<'_, RuntimeSession> {
        Box::pin(async move {
            Err(super::EngineError::new(
                super::EngineErrorKind::Unsupported,
                "engine runtime does not support session fork",
            ))
        })
    }

    fn attach_session(
        &self,
        session: SessionRef,
        options: AttachOptions,
    ) -> EngineFuture<'_, RuntimeSession>;

    fn start_turn(&self, session: SessionRef, request: TurnRequest)
        -> EngineFuture<'_, TurnHandle>;

    fn send_input_while_running(
        &self,
        turn: TurnRef,
        input: Vec<InputItem>,
    ) -> EngineFuture<'_, ()>;

    fn interrupt_turn(&self, turn: TurnRef) -> EngineFuture<'_, ()>;

    fn respond(
        &self,
        request: InteractionRef,
        response: InteractionResponse,
    ) -> EngineFuture<'_, ()>;

    fn close_session(&self, session: SessionRef) -> EngineFuture<'_, ()>;

    fn subscribe_events(&self, sink: RuntimeEventSink) -> EngineResult<SubscriptionHandle>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_event_fields_follow_the_frontend_camel_case_contract() {
        let event = NormalizedRuntimeEvent::ItemDelta {
            turn_id: "turn-1".into(),
            item_id: "item-1".into(),
            segment: Segment::Text {
                text: "done".into(),
                phase: None,
            },
        };

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "kind": "itemDelta",
                "turnId": "turn-1",
                "itemId": "item-1",
                "segment": {
                    "kind": "text",
                    "text": "done",
                },
            }),
        );
    }
}
