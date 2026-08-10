use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::app_server::{
    AppServerClient, AppServerError, AppServerErrorKind, IncomingMessage, RequestId, RpcError,
};
use crate::engines::core::{EngineError, EngineErrorKind, EngineResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub type CodexProtocolSink = Arc<dyn Fn(IncomingMessage) + Send + Sync>;

pub struct CodexSupervisor {
    connection: Mutex<Option<Connection>>,
    sinks: Arc<Mutex<Vec<CodexProtocolSink>>>,
    next_request_id: AtomicU64,
}

struct Connection {
    commands: SyncSender<ActorCommand>,
}

enum ActorCommand {
    Request {
        id: u64,
        method: String,
        params: Value,
        response: Sender<EngineResult<Value>>,
    },
    Respond {
        id: RequestId,
        result: Value,
        response: Sender<EngineResult<()>>,
    },
    Shutdown,
}

impl CodexSupervisor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            connection: Mutex::new(None),
            sinks: Arc::new(Mutex::new(Vec::new())),
            next_request_id: AtomicU64::new(10),
        })
    }

    pub fn subscribe(&self, sink: CodexProtocolSink) {
        self.sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sink);
    }

    pub fn request(&self, method: &str, params: Value) -> EngineResult<Value> {
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        self.request_once(id, method, params.clone())
            .or_else(|error| {
                if !error.retryable {
                    return Err(error);
                }
                self.disconnect();
                std::thread::sleep(retry_delay(id, 0));
                self.request_once(id, method, params)
            })
    }

    pub fn respond(&self, id: RequestId, result: Value) -> EngineResult<()> {
        let commands = self.connection()?;
        let (response_tx, response_rx) = mpsc::channel();
        commands
            .send(ActorCommand::Respond {
                id,
                result,
                response: response_tx,
            })
            .map_err(|_| unavailable("Codex app-server command channel closed"))?;
        response_rx
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| unavailable("Codex app-server response timed out"))?
    }

    pub fn disconnect(&self) {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(connection) = connection {
            let _ = connection.commands.send(ActorCommand::Shutdown);
        }
    }

    fn request_once(&self, id: u64, method: &str, params: Value) -> EngineResult<Value> {
        let commands = self.connection()?;
        let (response_tx, response_rx) = mpsc::channel();
        commands
            .send(ActorCommand::Request {
                id,
                method: method.to_string(),
                params,
                response: response_tx,
            })
            .map_err(|_| unavailable("Codex app-server command channel closed"))?;
        response_rx
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| unavailable("Codex app-server request timed out"))?
    }

    fn connection(&self) -> EngineResult<SyncSender<ActorCommand>> {
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(connection) = guard.as_ref() {
            return Ok(connection.commands.clone());
        }

        let binary = crate::codex_locator::locate().map_err(|_| {
            EngineError::new(EngineErrorKind::Unavailable, "Codex CLI is not installed")
        })?;
        let mut client = AppServerClient::spawn(&binary).map_err(map_transport_error)?;
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        client
            .request(
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
                CONNECT_TIMEOUT,
            )
            .map_err(map_transport_error)?;
        client
            .notify("initialized", json!({}))
            .map_err(map_transport_error)?;

        let (commands, receiver) = mpsc::sync_channel(256);
        let sinks = Arc::clone(&self.sinks);
        std::thread::spawn(move || actor_loop(client, receiver, sinks));
        *guard = Some(Connection {
            commands: commands.clone(),
        });
        Ok(commands)
    }
}

impl Drop for CodexSupervisor {
    fn drop(&mut self) {
        if let Some(connection) = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = connection.commands.send(ActorCommand::Shutdown);
        }
    }
}

fn actor_loop(
    mut client: AppServerClient,
    commands: Receiver<ActorCommand>,
    sinks: Arc<Mutex<Vec<CodexProtocolSink>>>,
) {
    let mut pending: BTreeMap<RequestId, Sender<EngineResult<Value>>> = BTreeMap::new();
    let mut active_turn = false;
    loop {
        let command_wait = if active_turn || !pending.is_empty() {
            Duration::from_millis(20)
        } else {
            Duration::from_secs(1)
        };
        match commands.recv_timeout(command_wait) {
            Ok(ActorCommand::Request {
                id,
                method,
                params,
                response,
            }) => {
                if method == "turn/start" {
                    active_turn = true;
                }
                match client.send_request(id, &method, params) {
                    Ok(()) => {
                        pending.insert(RequestId::Number(id), response);
                    }
                    Err(error) => {
                        let _ = response.send(Err(map_transport_error(error)));
                    }
                }
            }
            Ok(ActorCommand::Respond {
                id,
                result,
                response,
            }) => {
                let result = client.respond(id, result).map_err(map_transport_error);
                let _ = response.send(result);
            }
            Ok(ActorCommand::Shutdown) => break,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        loop {
            match client.receive(Duration::from_millis(1)) {
                Ok(IncomingMessage::Response { id, result }) => {
                    if let Some(response) = pending.remove(&id) {
                        let _ = response.send(Ok(result));
                    }
                }
                Ok(IncomingMessage::ErrorResponse { id, error }) => {
                    if let Some(response) = pending.remove(&id) {
                        let _ = response.send(Err(map_rpc_error(error)));
                    }
                }
                Ok(message) => {
                    if matches!(
                        &message,
                        IncomingMessage::Notification { method, .. } if method == "turn/completed"
                    ) {
                        active_turn = false;
                    }
                    let callbacks = sinks
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    for callback in &callbacks {
                        callback(message.clone());
                    }
                }
                Err(error) if error.kind() == AppServerErrorKind::Timeout => break,
                Err(error) => {
                    let failure = map_transport_error(error);
                    for (_, response) in std::mem::take(&mut pending) {
                        let _ = response.send(Err(failure.clone()));
                    }
                    return;
                }
            }
        }
    }
}

fn map_rpc_error(error: RpcError) -> EngineError {
    if error.message.contains("already has an active writer") {
        return EngineError::new(
            EngineErrorKind::Conflict,
            "Codex thread is active in another client",
        );
    }
    EngineError::new(
        EngineErrorKind::Protocol,
        format!("Codex request failed: {}", error.message),
    )
}

fn map_transport_error(error: AppServerError) -> EngineError {
    let kind = match error.kind() {
        AppServerErrorKind::Spawn | AppServerErrorKind::Eof => EngineErrorKind::Unavailable,
        AppServerErrorKind::Protocol | AppServerErrorKind::Rpc => EngineErrorKind::Protocol,
        AppServerErrorKind::Timeout | AppServerErrorKind::Io => EngineErrorKind::Io,
    };
    let mut mapped = EngineError::new(kind, error.to_string());
    if matches!(
        error.kind(),
        AppServerErrorKind::Spawn
            | AppServerErrorKind::Eof
            | AppServerErrorKind::Timeout
            | AppServerErrorKind::Io
    ) {
        mapped = mapped.retryable();
    }
    mapped
}

fn unavailable(message: &str) -> EngineError {
    EngineError::new(EngineErrorKind::Unavailable, message).retryable()
}

fn retry_delay(request_id: u64, attempt: u32) -> Duration {
    let exponential_ms = 50_u64.saturating_mul(1_u64 << attempt.min(4));
    let jitter_ms = request_id
        .wrapping_mul(37)
        .wrapping_add(attempt as u64 * 17)
        % 50;
    Duration::from_millis((exponential_ms + jitter_ms).min(1_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failures_keep_retry_policy_structured() {
        let error = map_transport_error(AppServerError::test_error(AppServerErrorKind::Eof));
        assert_eq!(error.kind, EngineErrorKind::Unavailable);
        assert!(error.retryable);
    }

    #[test]
    fn active_writer_rpc_error_is_a_non_automatic_conflict() {
        let error = map_rpc_error(RpcError {
            code: -32000,
            message: "thread example already has an active writer".into(),
            data: None,
        });
        assert_eq!(error.kind, EngineErrorKind::Conflict);
        assert_eq!(error.message, "Codex thread is active in another client");
        assert!(!error.retryable);
    }

    #[test]
    fn unrelated_rpc_error_remains_a_protocol_failure() {
        let error = map_rpc_error(RpcError {
            code: -32602,
            message: "invalid params".into(),
            data: None,
        });
        assert_eq!(error.kind, EngineErrorKind::Protocol);
        assert_eq!(error.message, "Codex request failed: invalid params");
    }

    #[test]
    fn retry_delay_is_bounded_and_jittered() {
        assert_ne!(retry_delay(1, 0), retry_delay(2, 0));
        assert!(retry_delay(42, 20) <= Duration::from_secs(1));
    }
}
