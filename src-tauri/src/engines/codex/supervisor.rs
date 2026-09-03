use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use serde_json::{json, Value};

use super::app_server::{
    AppServerClient, AppServerError, AppServerErrorKind, IncomingMessage, RequestId, RpcError,
};
use crate::engines::core::{EngineError, EngineErrorKind, EngineResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const READINESS_RETRY_COOLDOWN: Duration = Duration::from_secs(5);

pub const CODEX_READINESS_EVENT: &str = "codex-readiness-changed";

pub type CodexProtocolSink = Arc<dyn Fn(IncomingMessage) + Send + Sync>;
pub type CodexReadinessSink = Arc<dyn Fn(CodexReadinessSnapshot) + Send + Sync>;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexReadinessPhase {
    Warming,
    Ready,
    Degraded,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexReadinessSnapshot {
    pub phase: CodexReadinessPhase,
    pub error: Option<String>,
}

pub struct CodexSupervisor {
    connection: Mutex<Option<Connection>>,
    sinks: Arc<Mutex<Vec<CodexProtocolSink>>>,
    readiness: Mutex<ReadinessState>,
    readiness_changed: Condvar,
    readiness_sinks: Mutex<Vec<CodexReadinessSink>>,
    next_request_id: AtomicU64,
    next_connection_epoch: AtomicU64,
}

enum ReadinessState {
    Idle,
    Warming,
    Ready {
        runtime_version: Option<String>,
    },
    Degraded {
        error: EngineError,
        attempted_at: Instant,
    },
}

#[derive(Clone)]
struct Connection {
    commands: SyncSender<ActorCommand>,
    epoch: u64,
    alive: Arc<AtomicBool>,
    active_turns: Arc<AtomicU64>,
    runtime_identity: Option<RuntimeIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeIdentity {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

struct ConnectionLiveness(Arc<AtomicBool>);

impl Drop for ConnectionLiveness {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
            readiness: Mutex::new(ReadinessState::Idle),
            readiness_changed: Condvar::new(),
            readiness_sinks: Mutex::new(Vec::new()),
            next_request_id: AtomicU64::new(10),
            next_connection_epoch: AtomicU64::new(1),
        })
    }

    pub fn subscribe(&self, sink: CodexProtocolSink) {
        self.sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sink);
    }

    pub fn subscribe_readiness(&self, sink: CodexReadinessSink) {
        self.readiness_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sink);
    }

    pub fn prewarm(self: &Arc<Self>) {
        let supervisor = Arc::clone(self);
        std::thread::spawn(move || {
            if let Err(error) = supervisor.ensure_ready() {
                log::warn!("Codex App Server readiness prewarm failed: {error}");
            }
        });
    }

    pub fn ensure_ready(&self) -> EngineResult<()> {
        let mut restart_connection = false;
        loop {
            let mut readiness = self
                .readiness
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match &*readiness {
                ReadinessState::Idle => {
                    *readiness = ReadinessState::Warming;
                    break;
                }
                ReadinessState::Warming => {
                    readiness = self
                        .readiness_changed
                        .wait(readiness)
                        .unwrap_or_else(|error| error.into_inner());
                    drop(readiness);
                }
                ReadinessState::Ready { runtime_version } => {
                    let runtime_current = self.connection_uses_current_runtime();
                    if runtime_current
                        && crate::codex_env::cache_matches_version(runtime_version.as_deref())
                    {
                        return Ok(());
                    }
                    // 不打断正在输出的轮次；轮次结束后的下一次 readiness 检查会换代。
                    if !runtime_current && self.connection_has_active_turns() {
                        return Ok(());
                    }
                    restart_connection = !runtime_current;
                    *readiness = ReadinessState::Warming;
                    break;
                }
                ReadinessState::Degraded {
                    error,
                    attempted_at,
                } if attempted_at.elapsed() < READINESS_RETRY_COOLDOWN => {
                    return Err(error.clone());
                }
                ReadinessState::Degraded { .. } => {
                    *readiness = ReadinessState::Warming;
                    break;
                }
            }
        }

        self.emit_readiness(CodexReadinessSnapshot {
            phase: CodexReadinessPhase::Warming,
            error: None,
        });
        if restart_connection {
            log::info!("Codex runtime changed on disk; reconnecting App Server");
            self.disconnect();
        }
        let runtime_version = crate::codex_env::current_runtime_version();
        let result = self
            .request("model/list", json!({ "cursor": null, "limit": 1 }))
            .and_then(|response| {
                response
                    .get("data")
                    .and_then(Value::as_array)
                    .map(|_| ())
                    .ok_or_else(|| {
                        EngineError::new(
                            EngineErrorKind::Protocol,
                            "Codex returned an invalid model list during readiness check",
                        )
                    })
            });

        let snapshot = match &result {
            Ok(()) => {
                *self
                    .readiness
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) =
                    ReadinessState::Ready { runtime_version };
                CodexReadinessSnapshot {
                    phase: CodexReadinessPhase::Ready,
                    error: None,
                }
            }
            Err(error) => {
                *self
                    .readiness
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = ReadinessState::Degraded {
                    error: error.clone(),
                    attempted_at: Instant::now(),
                };
                CodexReadinessSnapshot {
                    phase: CodexReadinessPhase::Degraded,
                    error: Some(error.message.clone()),
                }
            }
        };
        self.readiness_changed.notify_all();
        self.emit_readiness(snapshot);
        result
    }

    pub fn request(&self, method: &str, params: Value) -> EngineResult<Value> {
        self.request_with_epoch(method, params)
            .map(|(response, _)| response)
    }

    pub fn request_with_epoch(&self, method: &str, params: Value) -> EngineResult<(Value, u64)> {
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

    pub fn current_connection_epoch(&self) -> Option<u64> {
        self.connection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|connection| connection.alive.load(Ordering::Acquire))
            .map(|connection| connection.epoch)
    }

    pub fn respond(&self, id: RequestId, result: Value) -> EngineResult<()> {
        let connection = self.connection()?;
        let (response_tx, response_rx) = mpsc::channel();
        connection
            .commands
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

    fn connection_uses_current_runtime(&self) -> bool {
        let current = crate::codex_locator::locate()
            .ok()
            .as_deref()
            .and_then(runtime_identity);
        let guard = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(connection) = guard
            .as_ref()
            .filter(|connection| connection.alive.load(Ordering::Acquire))
        else {
            return false;
        };
        matches!(
            (&connection.runtime_identity, current),
            (Some(existing), Some(current)) if existing == &current
        )
    }

    fn connection_has_active_turns(&self) -> bool {
        self.connection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|connection| connection.alive.load(Ordering::Acquire))
            .is_some_and(|connection| connection.active_turns.load(Ordering::Acquire) > 0)
    }

    fn request_once(&self, id: u64, method: &str, params: Value) -> EngineResult<(Value, u64)> {
        let connection = self.connection()?;
        let (response_tx, response_rx) = mpsc::channel();
        connection
            .commands
            .send(ActorCommand::Request {
                id,
                method: method.to_string(),
                params,
                response: response_tx,
            })
            .map_err(|_| unavailable("Codex app-server command channel closed"))?;
        let response = response_rx
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| unavailable("Codex app-server request timed out"))??;
        Ok((response, connection.epoch))
    }

    fn emit_readiness(&self, snapshot: CodexReadinessSnapshot) {
        let sinks = self
            .readiness_sinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        for sink in sinks {
            sink(snapshot.clone());
        }
    }

    fn connection(&self) -> EngineResult<Connection> {
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(connection) = guard.as_ref() {
            if connection.alive.load(Ordering::Acquire) {
                return Ok(connection.clone());
            }
            *guard = None;
        }

        let binary = crate::codex_locator::locate().map_err(|_| {
            EngineError::new(EngineErrorKind::Unavailable, "Codex CLI is not installed")
        })?;
        let runtime_identity = runtime_identity(&binary);
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
        let alive = Arc::new(AtomicBool::new(true));
        let actor_alive = Arc::clone(&alive);
        let active_turns = Arc::new(AtomicU64::new(0));
        let actor_active_turns = Arc::clone(&active_turns);
        std::thread::spawn(move || {
            let _liveness = ConnectionLiveness(actor_alive);
            actor_loop(client, receiver, sinks, actor_active_turns);
        });
        let connection = Connection {
            commands,
            epoch: self.next_connection_epoch.fetch_add(1, Ordering::Relaxed),
            alive,
            active_turns,
            runtime_identity,
        };
        *guard = Some(connection.clone());
        Ok(connection)
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
    active_turns: Arc<AtomicU64>,
) {
    let mut pending: BTreeMap<RequestId, Sender<EngineResult<Value>>> = BTreeMap::new();
    let mut pending_turn_starts = BTreeSet::new();
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
                    active_turns.fetch_add(1, Ordering::AcqRel);
                    pending_turn_starts.insert(RequestId::Number(id));
                }
                match client.send_request(id, &method, params) {
                    Ok(()) => {
                        pending.insert(RequestId::Number(id), response);
                    }
                    Err(error) => {
                        if pending_turn_starts.remove(&RequestId::Number(id)) {
                            decrement_active_turns(&active_turns);
                        }
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
                    pending_turn_starts.remove(&id);
                    if let Some(response) = pending.remove(&id) {
                        let _ = response.send(Ok(result));
                    }
                }
                Ok(IncomingMessage::ErrorResponse { id, error }) => {
                    if pending_turn_starts.remove(&id) {
                        decrement_active_turns(&active_turns);
                    }
                    if let Some(response) = pending.remove(&id) {
                        let _ = response.send(Err(map_rpc_error(error)));
                    }
                }
                Ok(message) => {
                    if let IncomingMessage::Notification { method, .. } = &message {
                        match method.as_str() {
                            "turn/completed" => {
                                active_turn = false;
                                decrement_active_turns(&active_turns);
                            }
                            _ => {}
                        }
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

fn decrement_active_turns(active_turns: &AtomicU64) {
    let _ = active_turns.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
        Some(count.saturating_sub(1))
    });
}

fn runtime_identity(path: &Path) -> Option<RuntimeIdentity> {
    let path = path.canonicalize().ok()?;
    let metadata = std::fs::metadata(&path).ok()?;
    Some(RuntimeIdentity {
        path,
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn map_rpc_error(error: RpcError) -> EngineError {
    if error.message.contains("already has an active writer") {
        return EngineError::new(
            EngineErrorKind::Conflict,
            "Codex thread is active in another client",
        );
    }
    if error
        .message
        .to_ascii_lowercase()
        .contains("thread not found")
    {
        return EngineError::new(
            EngineErrorKind::NotFound,
            format!("Codex request failed: {}", error.message),
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
        AppServerErrorKind::Protocol
        | AppServerErrorKind::MessageTooLarge
        | AppServerErrorKind::Rpc => EngineErrorKind::Protocol,
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
    fn missing_thread_rpc_error_is_structured_for_runtime_recovery() {
        let error = map_rpc_error(RpcError {
            code: -32000,
            message: "thread not found: example".into(),
            data: None,
        });
        assert_eq!(error.kind, EngineErrorKind::NotFound);
        assert_eq!(
            error.message,
            "Codex request failed: thread not found: example"
        );
        assert!(!error.retryable);
    }

    #[test]
    fn retry_delay_is_bounded_and_jittered() {
        assert_ne!(retry_delay(1, 0), retry_delay(2, 0));
        assert!(retry_delay(42, 20) <= Duration::from_secs(1));
    }

    #[test]
    fn runtime_identity_detects_an_in_place_binary_change() {
        let path = std::env::temp_dir().join(format!(
            "monet-codex-runtime-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"old").unwrap();
        let before = runtime_identity(&path).unwrap();
        std::fs::write(&path, b"new-runtime").unwrap();
        let after = runtime_identity(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_ne!(before, after);
    }
}
