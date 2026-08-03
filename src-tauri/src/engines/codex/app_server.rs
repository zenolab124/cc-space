use std::collections::{BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engines::core::{EngineError, EngineErrorKind, EngineResult};
use crate::proc_ext::HideConsole;

pub const CORE_CLIENT_METHODS: &[&str] = &[
    "initialize",
    "thread/list",
    "thread/read",
    "thread/start",
    "thread/resume",
    "turn/start",
    "turn/steer",
    "turn/interrupt",
    "model/list",
];

pub const CORE_SERVER_REQUESTS: &[&str] = &[
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
    "item/permissions/requestApproval",
];

pub const CORE_NOTIFICATIONS: &[&str] = &[
    "turn/started",
    "turn/completed",
    "item/started",
    "item/completed",
    "item/agentMessage/delta",
];

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    String(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientRequest {
    pub id: RequestId,
    pub method: String,
    pub params: Value,
}

impl ClientRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            id: RequestId::Number(id),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientNotification {
    pub method: String,
    pub params: Value,
}

impl ClientNotification {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IncomingMessage {
    Response {
        id: RequestId,
        result: Value,
    },
    ErrorResponse {
        id: RequestId,
        error: RpcError,
    },
    Notification {
        method: String,
        params: Value,
    },
    ServerRequest {
        id: RequestId,
        method: String,
        params: Value,
    },
}

impl IncomingMessage {
    pub fn parse(line: &str) -> EngineResult<Self> {
        let value: Value = serde_json::from_str(line).map_err(|_| {
            EngineError::new(
                EngineErrorKind::Protocol,
                "Codex app-server emitted invalid JSON",
            )
        })?;
        let object = value.as_object().ok_or_else(|| {
            EngineError::new(
                EngineErrorKind::Protocol,
                "Codex app-server message must be an object",
            )
        })?;
        let id = object
            .get("id")
            .cloned()
            .map(serde_json::from_value::<RequestId>)
            .transpose()
            .map_err(|_| {
                EngineError::new(
                    EngineErrorKind::Protocol,
                    "Codex app-server returned an unsupported request id",
                )
            })?;
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .map(String::from);
        let params = object.get("params").cloned().unwrap_or(Value::Null);

        match (id, method) {
            (Some(id), Some(method)) => Ok(Self::ServerRequest { id, method, params }),
            (None, Some(method)) => Ok(Self::Notification { method, params }),
            (Some(id), None) => {
                if let Some(error) = object.get("error") {
                    let error = serde_json::from_value(error.clone()).map_err(|_| {
                        EngineError::new(
                            EngineErrorKind::Protocol,
                            "Codex app-server returned an invalid error object",
                        )
                    })?;
                    return Ok(Self::ErrorResponse { id, error });
                }
                let result = object.get("result").cloned().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorKind::Protocol,
                        "Codex app-server response has no result or error",
                    )
                })?;
                Ok(Self::Response { id, result })
            }
            (None, None) => Err(EngineError::new(
                EngineErrorKind::Protocol,
                "Codex app-server message has no id or method",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppServerErrorKind {
    Spawn,
    Protocol,
    Timeout,
    Io,
    Eof,
    Rpc,
}

#[derive(Clone, Debug)]
pub struct AppServerError {
    kind: AppServerErrorKind,
    rpc_error: Option<RpcError>,
}

impl AppServerError {
    fn new(kind: AppServerErrorKind) -> Self {
        Self {
            kind,
            rpc_error: None,
        }
    }

    fn rpc(error: RpcError) -> Self {
        Self {
            kind: AppServerErrorKind::Rpc,
            rpc_error: Some(error),
        }
    }

    pub fn kind(&self) -> AppServerErrorKind {
        self.kind
    }

    pub fn rpc_error(&self) -> Option<&RpcError> {
        self.rpc_error.as_ref()
    }
}

impl std::fmt::Display for AppServerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self.kind {
            AppServerErrorKind::Spawn => "Codex app-server could not be started",
            AppServerErrorKind::Protocol => "Codex app-server protocol error",
            AppServerErrorKind::Timeout => "Codex app-server request timed out",
            AppServerErrorKind::Io => "Codex app-server connection was closed",
            AppServerErrorKind::Eof => "Codex app-server stopped unexpectedly",
            AppServerErrorKind::Rpc => "Codex app-server rejected the request",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AppServerError {}

pub struct AppServerClient {
    stdin: BufWriter<ChildStdin>,
    stdout_rx: Receiver<String>,
    pending: VecDeque<IncomingMessage>,
    _process: AppServerProcess,
}

impl AppServerClient {
    pub fn spawn(path: &Path) -> Result<Self, AppServerError> {
        let mut command = Command::new(path);
        command
            .args(["app-server", "--listen", "stdio://"])
            .env("PATH", crate::path_env::enhanced_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .hide_console();
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        let child = command
            .spawn()
            .map_err(|_| AppServerError::new(AppServerErrorKind::Spawn))?;
        let mut process = AppServerProcess::new(child);
        let stdin = process
            .child
            .stdin
            .take()
            .ok_or_else(|| AppServerError::new(AppServerErrorKind::Spawn))?;
        let stdout = process
            .child
            .stdout
            .take()
            .ok_or_else(|| AppServerError::new(AppServerErrorKind::Spawn))?;
        let stderr = process.child.stderr.take();

        let (stdout_tx, stdout_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if stdout_tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut sink = String::new();
                while reader.read_line(&mut sink).unwrap_or(0) > 0 {
                    if sink.len() > 16 * 1024 {
                        sink.drain(..8 * 1024);
                    }
                }
            });
        }

        Ok(Self {
            stdin: BufWriter::new(stdin),
            stdout_rx,
            pending: VecDeque::new(),
            _process: process,
        })
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), AppServerError> {
        self.write_message(&ClientNotification::new(method, params))
    }

    pub fn send_request(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
    ) -> Result<(), AppServerError> {
        self.write_message(&ClientRequest::new(id, method, params))
    }

    pub fn receive(&mut self, timeout: Duration) -> Result<IncomingMessage, AppServerError> {
        if let Some(message) = self.pending.pop_front() {
            return Ok(message);
        }
        self.receive_transport(timeout)
    }

    pub fn request(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        total_deadline: Instant,
        request_timeout: Duration,
    ) -> Result<Value, AppServerError> {
        self.send_request(id, method, params)?;
        let request_id = RequestId::Number(id);
        let request_deadline = (Instant::now() + request_timeout).min(total_deadline);

        loop {
            let remaining = request_deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| AppServerError::new(AppServerErrorKind::Timeout))?;
            match self.receive_transport(remaining)? {
                IncomingMessage::Response {
                    id: response_id,
                    result,
                } if response_id == request_id => return Ok(result),
                IncomingMessage::ErrorResponse {
                    id: response_id,
                    error,
                } if response_id == request_id => return Err(AppServerError::rpc(error)),
                message => self.pending.push_back(message),
            }
        }
    }

    fn receive_transport(&self, timeout: Duration) -> Result<IncomingMessage, AppServerError> {
        match self.stdout_rx.recv_timeout(timeout) {
            Ok(line) => IncomingMessage::parse(&line)
                .map_err(|_| AppServerError::new(AppServerErrorKind::Protocol)),
            Err(RecvTimeoutError::Timeout) => Err(AppServerError::new(AppServerErrorKind::Timeout)),
            Err(RecvTimeoutError::Disconnected) => {
                Err(AppServerError::new(AppServerErrorKind::Eof))
            }
        }
    }

    fn write_message(&mut self, value: &impl Serialize) -> Result<(), AppServerError> {
        serde_json::to_writer(&mut self.stdin, value)
            .map_err(|_| AppServerError::new(AppServerErrorKind::Protocol))?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|_| AppServerError::new(AppServerErrorKind::Io))
    }
}

struct AppServerProcess {
    child: Child,
    #[cfg(unix)]
    process_group_id: i32,
    #[cfg(windows)]
    job: Option<JobHandle>,
}

impl AppServerProcess {
    fn new(child: Child) -> Self {
        #[cfg(unix)]
        let process_group_id = child.id() as i32;
        #[cfg(windows)]
        let job = create_job_for_child(child.id());
        Self {
            child,
            #[cfg(unix)]
            process_group_id,
            #[cfg(windows)]
            job,
        }
    }
}

#[cfg(unix)]
fn process_group_exists(process_group_id: i32) -> bool {
    let result = unsafe { libc::kill(-process_group_id, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

impl Drop for AppServerProcess {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-self.process_group_id, libc::SIGTERM);
        }
        #[cfg(windows)]
        {
            let _ = self.child.kill();
            self.job.take();
        }

        let deadline = Instant::now() + Duration::from_millis(350);
        #[cfg(unix)]
        while process_group_exists(self.process_group_id) && Instant::now() < deadline {
            let _ = self.child.try_wait();
            std::thread::sleep(Duration::from_millis(20));
        }
        #[cfg(not(unix))]
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }

        #[cfg(unix)]
        if process_group_exists(self.process_group_id) {
            unsafe {
                libc::kill(-self.process_group_id, libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
unsafe impl Send for JobHandle {}

#[cfg(windows)]
fn create_job_for_child(pid: u32) -> Option<JobHandle> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Threading::*;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if configured == 0 {
            CloseHandle(job);
            return None;
        }
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if process.is_null() {
            CloseHandle(job);
            return None;
        }
        let assigned = AssignProcessToJobObject(job, process);
        CloseHandle(process);
        if assigned == 0 {
            CloseHandle(job);
            return None;
        }
        Some(JobHandle(job))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaContractReport {
    pub supported: Vec<String>,
    pub missing: Vec<String>,
}

impl SchemaContractReport {
    pub fn is_supported(&self) -> bool {
        self.missing.is_empty()
    }
}

pub fn validate_schema_contract<'a>(
    schemas: impl IntoIterator<Item = &'a Value>,
) -> SchemaContractReport {
    let mut literals = BTreeSet::new();
    for schema in schemas {
        collect_schema_literals(schema, &mut literals);
    }

    let mut supported = Vec::new();
    let mut missing = Vec::new();
    for method in CORE_CLIENT_METHODS
        .iter()
        .chain(CORE_SERVER_REQUESTS)
        .chain(CORE_NOTIFICATIONS)
    {
        if literals.contains(*method) {
            supported.push((*method).to_string());
        } else {
            missing.push((*method).to_string());
        }
    }
    SchemaContractReport { supported, missing }
}

fn collect_schema_literals(value: &Value, literals: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(value) = object.get("const").and_then(Value::as_str) {
                literals.insert(value.to_string());
            }
            if let Some(values) = object.get("enum").and_then(Value::as_array) {
                literals.extend(values.iter().filter_map(Value::as_str).map(String::from));
            }
            for child in object.values() {
                collect_schema_literals(child, literals);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_schema_literals(child, literals);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn parses_all_json_rpc_message_shapes() {
        assert!(matches!(
            IncomingMessage::parse(r#"{"id":1,"result":{"ok":true}}"#).unwrap(),
            IncomingMessage::Response { .. }
        ));
        assert!(matches!(
            IncomingMessage::parse(
                r#"{"id":2,"error":{"code":-1,"message":"failed","data":null}}"#
            )
            .unwrap(),
            IncomingMessage::ErrorResponse { .. }
        ));
        assert!(matches!(
            IncomingMessage::parse(r#"{"method":"turn/completed","params":{}}"#).unwrap(),
            IncomingMessage::Notification { .. }
        ));
        assert!(matches!(
            IncomingMessage::parse(
                r#"{"id":"approval-1","method":"item/commandExecution/requestApproval","params":{}}"#
            )
            .unwrap(),
            IncomingMessage::ServerRequest { .. }
        ));
    }

    #[test]
    fn outgoing_messages_omit_json_rpc_header() {
        let request =
            serde_json::to_value(ClientRequest::new(1, "thread/list", json!({ "limit": 1 })))
                .unwrap();
        let notification =
            serde_json::to_value(ClientNotification::new("initialized", json!({}))).unwrap();

        assert_eq!(
            request.get("method").and_then(Value::as_str),
            Some("thread/list")
        );
        assert!(request.get("jsonrpc").is_none());
        assert!(notification.get("jsonrpc").is_none());
    }

    #[test]
    fn schema_contract_reports_missing_methods() {
        let schema = json!({
            "properties": {
                "method": { "enum": ["initialize", "thread/list"] }
            }
        });
        let report = validate_schema_contract([&schema]);

        assert!(report.supported.contains(&"initialize".to_string()));
        assert!(report.missing.contains(&"turn/start".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn process_guard_kills_descendant_after_group_leader_exits() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;

        let output = Command::new("/bin/sh")
            .args([
                "-c",
                "trap '' TERM; while :; do sleep 1; done & child=$!; echo $child; trap 'exit 0' TERM; wait",
            ])
            .process_group(0)
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn process group fixture");
        let mut process = AppServerProcess::new(output);
        let stdout = process.child.stdout.take().expect("fixture stdout");
        let mut line = String::new();
        BufReader::new(stdout)
            .read_line(&mut line)
            .expect("read descendant pid");
        let descendant_pid = line.trim().parse::<i32>().expect("valid descendant pid");

        drop(process);

        let deadline = Instant::now() + Duration::from_secs(2);
        while unsafe { libc::kill(descendant_pid, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            unsafe { libc::kill(descendant_pid, 0) },
            -1,
            "descendant process survived AppServerProcess::drop"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    #[ignore = "requires an installed Codex CLI and explicit MONET_CODEX_APP_SERVER_READ_SMOKE=1"]
    fn installed_server_handles_read_only_handshake() {
        assert_eq!(
            std::env::var("MONET_CODEX_APP_SERVER_READ_SMOKE").as_deref(),
            Ok("1"),
            "set MONET_CODEX_APP_SERVER_READ_SMOKE=1 to run the read-only transport smoke test"
        );
        let binary = crate::codex_locator::locate().expect("Codex CLI should be installed");
        let mut server = AppServerClient::spawn(&binary).expect("Codex app-server should start");
        let deadline = Instant::now() + Duration::from_secs(20);
        let timeout = Duration::from_secs(8);

        server
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
                timeout,
            )
            .expect("initialize should succeed");
        server
            .notify("initialized", json!({}))
            .expect("initialized notification should be sent");
        let threads = server
            .request(1, "thread/list", json!({ "limit": 1 }), deadline, timeout)
            .expect("thread/list should succeed");

        assert!(threads.is_object(), "thread/list should return an object");
    }

    #[test]
    #[ignore = "requires an installed Codex CLI and explicit MONET_CODEX_APP_SERVER_SCHEMA_SMOKE=1"]
    fn installed_schema_supports_core_contract() {
        assert_eq!(
            std::env::var("MONET_CODEX_APP_SERVER_SCHEMA_SMOKE").as_deref(),
            Ok("1"),
            "set MONET_CODEX_APP_SERVER_SCHEMA_SMOKE=1 to run the read-only schema smoke test"
        );
        let binary = crate::codex_locator::locate().expect("Codex CLI should be installed");
        let root =
            std::env::temp_dir().join(format!("monet-codex-app-server-schema-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temporary schema directory should be created");
        let output = Command::new(binary)
            .args(["app-server", "generate-json-schema", "--out"])
            .arg(&root)
            .env("PATH", crate::path_env::enhanced_path())
            .output()
            .expect("Codex schema generator should start");
        assert!(
            output.status.success(),
            "Codex schema generator failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let schema_paths = [
            root.join("codex_app_server_protocol.v2.schemas.json"),
            root.join("codex_app_server_protocol.schemas.json"),
        ];
        let schemas: Vec<Value> = schema_paths
            .iter()
            .map(|path| {
                serde_json::from_slice(&fs::read(path).expect("schema bundle should exist"))
                    .expect("schema bundle should contain JSON")
            })
            .collect();
        let report = validate_schema_contract(&schemas);

        let temp_root = std::env::temp_dir();
        if root.starts_with(&temp_root)
            && root.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("monet-codex-app-server-schema-")
            })
        {
            let _ = fs::remove_dir_all(&root);
        }
        assert!(
            report.is_supported(),
            "Codex schema is missing core methods: {:?}",
            report.missing
        );
    }
}
