//! 权限请求服务
//!
//! 在主进程内启动 TCP loopback server（127.0.0.1 随机端口），作为 monet-mcp 子进程与前端的桥梁。
//! 曾用 Unix socket，为 Windows 同构统一改为 TCP：接口一致、仅本机回环可达，
//! 请求需携带一次性 token（经环境变量注入 monet-mcp），防止本机其他进程伪造请求。
//! v2.1.0 起按会话隔离：每个流式会话一个 PermissionService 实例（工作台多列并行）。
//! 主流程：
//! 1. `PermissionService::start(app, session_id)` 绑定 127.0.0.1:0（随机端口）
//! 2. 接受连接（每条连接对应 monet-mcp 转发的一次权限请求）
//! 3. emit Tauri Event `permission-request` 给前端（payload 带 sessionId）
//! 4. 前端通过 `respond_permission` Tauri Command 回写决策（allow/deny）
//! 5. service 把决策写回连接，monet-mcp 据此返回 claude CLI

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// accept 失败的错误退避步长（防 fd 耗尽等错误风暴下 accept 立即返错空转）
const POLL_INTERVAL_MS: u64 = 50;

/// 一次 pending 请求的状态
struct PendingRequest {
    /// 收到响应后写到这里
    decision: Mutex<Option<Decision>>,
    /// 决策就绪通知：respond/shutdown 写入后唤醒 wait_decision，
    /// 等待方零轮询（仅留长间隔超时兜底 Drop 等只置 stop_flag 不写决策的路径）
    ready: Condvar,
}

/// 用户决策
#[derive(Debug, Clone)]
enum Decision {
    /// 携带可选 updatedInput（交互工具经此回传用户答案，None 时 monet-mcp 回填原始 input）
    /// 与可选 updatedPermissions（「始终允许」时让 CLI 自行把规则写进 settings 并即刻生效）
    Allow(Option<Value>, Option<Value>),
    Deny(String),
}

/// 权限服务（每个流式会话一个实例，会话归属由 SERVICES 的 key 承载）
pub struct PermissionService {
    /// 监听地址（"127.0.0.1:<port>"，注入 monet-mcp 环境变量，同时充当实例身份）
    endpoint: String,
    /// 连接鉴权 token（随实例生成，经环境变量注入 monet-mcp）
    token: String,
    /// 关闭信号
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
    /// 共享的 pending 请求表
    pending: Arc<Mutex<HashMap<String, Arc<PendingRequest>>>>,
}

/// 会话 → 服务实例表
static SERVICES: Mutex<Option<HashMap<String, Arc<PermissionService>>>> = Mutex::new(None);

/// requestId 自增（全进程唯一，respond 时跨实例查找无歧义）
static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);
static OBSERVER_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub enum PermissionObserverEvent {
    Requested {
        request_id: String,
        session_id: String,
        tool_name: String,
        input: Value,
    },
}

type PermissionObserver = Arc<dyn Fn(PermissionObserverEvent) + Send + Sync>;
static OBSERVERS: Mutex<Vec<(u64, PermissionObserver)>> = Mutex::new(Vec::new());

pub struct PermissionObserverHandle(u64);

impl Drop for PermissionObserverHandle {
    fn drop(&mut self) {
        OBSERVERS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(id, _)| *id != self.0);
    }
}

pub fn subscribe_observer(observer: PermissionObserver) -> PermissionObserverHandle {
    let id = OBSERVER_COUNTER.fetch_add(1, Ordering::Relaxed);
    OBSERVERS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push((id, observer));
    PermissionObserverHandle(id)
}

fn notify_observers(event: PermissionObserverEvent) {
    let observers: Vec<_> = OBSERVERS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .map(|(_, observer)| Arc::clone(observer))
        .collect();
    for observer in observers {
        observer(event.clone());
    }
}

/// 推送给前端的事件
#[derive(Debug, Clone, Serialize)]
struct PermissionRequestPayload {
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "toolName")]
    tool_name: String,
    input: Value,
    /// Unix 毫秒
    timestamp: u128,
}

impl PermissionService {
    /// 启动指定会话的权限服务，返回实例（地址与 token 注入 monet-mcp 子进程环境变量）
    pub fn start(app: AppHandle, session_id: &str) -> Result<Arc<Self>, String> {
        // 同会话旧实例先停（同会话重发场景）
        Self::stop_for(session_id);

        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|e| format!("绑定权限服务端口失败：{}", e))?;
        let endpoint = listener
            .local_addr()
            .map_err(|e| format!("读取权限服务地址失败：{}", e))?
            .to_string();

        let token = make_token();
        let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pending: Arc<Mutex<HashMap<String, Arc<PendingRequest>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let service = Arc::new(PermissionService {
            endpoint,
            token: token.clone(),
            stop_flag: stop_flag.clone(),
            pending: pending.clone(),
        });

        // accept loop 单独线程
        {
            let pending = pending.clone();
            let stop_flag = stop_flag.clone();
            let sid = session_id.to_string();
            thread::spawn(move || {
                accept_loop(listener, app, sid, token, pending, stop_flag);
            });
        }

        let mut services = SERVICES.lock().unwrap();
        services
            .get_or_insert_with(HashMap::new)
            .insert(session_id.to_string(), service.clone());
        Ok(service)
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// 提交一个用户决策（requestId 全进程唯一，跨实例查找）。返回是否找到对应的 pending 请求
    pub fn respond(
        request_id: &str,
        allow: bool,
        message: Option<String>,
        updated_input: Option<Value>,
        updated_permissions: Option<Value>,
    ) -> bool {
        let services: Vec<Arc<PermissionService>> = SERVICES
            .lock()
            .unwrap()
            .as_ref()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default();
        for service in services {
            let pending = service.pending.lock().unwrap();
            if let Some(req) = pending.get(request_id) {
                let mut slot = req.decision.lock().unwrap();
                if slot.is_none() {
                    *slot = Some(if allow {
                        Decision::Allow(updated_input, updated_permissions)
                    } else {
                        Decision::Deny(message.unwrap_or_else(|| "用户拒绝".to_string()))
                    });
                    req.ready.notify_all();
                    return true;
                }
            }
        }
        false
    }

    /// 停掉指定会话的服务（关闭监听、pending 请求统一按 deny 收尾）
    pub fn stop_for(session_id: &str) {
        let service = SERVICES
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|m| m.remove(session_id));
        if let Some(service) = service {
            service.shutdown();
        }
    }

    /// 仅当该会话当前注册的服务正是 `endpoint` 这一实例时才停止。
    ///
    /// 同会话连发场景下，每个 streaming turn 新建一个监听端口。旧 turn 的 read_stream
    /// 线程退出时若按 session_id 盲调 stop_for，会误删新 turn 刚注册的服务（监听被
    /// shutdown → 新 turn 的 monet-mcp 连接 Connection refused / pending 被强制 deny）。
    /// 用监听地址做实例身份校验：被新 turn 接管后旧线程不再触碰。
    pub fn stop_if_endpoint(session_id: &str, endpoint: &str) {
        let service = {
            let mut guard = SERVICES.lock().unwrap();
            let is_current = guard
                .as_ref()
                .and_then(|m| m.get(session_id))
                .is_some_and(|s| s.endpoint == endpoint);
            if is_current {
                guard.as_mut().and_then(|m| m.remove(session_id))
            } else {
                None
            }
        };
        if let Some(service) = service {
            service.shutdown();
        }
    }

    /// 内部：置停止位 + 踢醒 accept 线程 + deny 全部 pending
    fn shutdown(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        Self::kick_accept(&self.endpoint);
        let pending = self.pending.lock().unwrap();
        for req in pending.values() {
            let mut slot = req.decision.lock().unwrap();
            if slot.is_none() {
                *slot = Some(Decision::Deny("流式中断，自动拒绝".to_string()));
            }
            req.ready.notify_all();
        }
    }

    /// self-connect 踢醒阻塞中的 accept 线程（须在 stop_flag 置位后调用）。
    /// 失败仅意味着该线程晚退（继续阻塞在 accept 上零开销，进程退出时回收），不影响正确性
    fn kick_accept(endpoint: &str) {
        if let Ok(addr) = endpoint.parse::<std::net::SocketAddr>() {
            let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(500));
        }
    }
}

impl Drop for PermissionService {
    fn drop(&mut self) {
        // 兜底未走 shutdown 的丢弃路径：同样踢醒，避免 accept 线程静默泄漏
        self.stop_flag.store(true, Ordering::SeqCst);
        Self::kick_accept(&self.endpoint);
    }
}

/// 生成连接鉴权 token。RandomState 自带进程级随机种子（std 唯一内置熵源），
/// 叠加时间与 pid，防本机其他进程伪造权限请求足够，无密码学强度诉求
fn make_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h1 = RandomState::new().build_hasher();
    h1.write_u128(nanos);
    h1.write_u32(std::process::id());
    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(h1.finish());
    format!("{:016x}{:016x}", h1.finish(), h2.finish())
}

/// accept loop：阻塞 accept，每条连接 spawn 一个处理线程。
/// 空闲零唤醒；shutdown 置 stop_flag 后 self-connect 踢醒本循环退出
fn accept_loop(
    listener: TcpListener,
    app: AppHandle,
    session_id: String,
    token: String,
    pending: Arc<Mutex<HashMap<String, Arc<PendingRequest>>>>,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // 踢醒连接（或 stop 竞态窗口内的真实连接，服务已停本就该拒）：丢弃退出
                if stop_flag.load(Ordering::SeqCst) {
                    return;
                }
                let app = app.clone();
                let pending = pending.clone();
                let stop_flag = stop_flag.clone();
                let sid = session_id.clone();
                let token = token.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, app, sid, token, pending, stop_flag) {
                        log::warn!("权限请求处理失败：{}", e);
                    }
                });
            }
            Err(e) => {
                if stop_flag.load(Ordering::SeqCst) {
                    return;
                }
                log::warn!("权限服务 accept 失败：{}", e);
                // 防错误风暴（如 fd 耗尽时 accept 立即返错）
                thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
        }
    }
}

/// 处理单次连接：读一行请求（校验 token）→ emit event → 等用户响应 → 写回一行决策
fn handle_connection(
    stream: TcpStream,
    app: AppHandle,
    session_id: String,
    token: String,
    pending: Arc<Mutex<HashMap<String, Arc<PendingRequest>>>>,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut req_line = String::new();
    reader
        .read_line(&mut req_line)
        .map_err(|e| format!("读权限请求失败：{}", e))?;
    let req: Value = serde_json::from_str(req_line.trim())
        .map_err(|e| format!("解析权限请求 JSON 失败：{}", e))?;

    // token 不符：非 monet-mcp 的来路，直接断开不回话
    if req.get("token").and_then(Value::as_str) != Some(token.as_str()) {
        return Err("权限请求 token 校验失败，已拒绝连接".to_string());
    }

    let tool_name = req
        .get("toolName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let input = req.get("input").cloned().unwrap_or_else(|| json!({}));

    let request_id = format!("perm-{}", REQ_COUNTER.fetch_add(1, Ordering::Relaxed));
    let pending_req = Arc::new(PendingRequest {
        decision: Mutex::new(None),
        ready: Condvar::new(),
    });

    pending
        .lock()
        .unwrap()
        .insert(request_id.clone(), pending_req.clone());

    // emit 给前端
    let payload = PermissionRequestPayload {
        request_id: request_id.clone(),
        session_id: session_id.clone(),
        tool_name: tool_name.clone(),
        input: input.clone(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    };
    let _ = app.emit("permission-request", &payload);
    notify_observers(PermissionObserverEvent::Requested {
        request_id: request_id.clone(),
        session_id,
        tool_name,
        input,
    });

    // 永不超时:阻塞等用户响应或服务停止(中断收尾会唤醒并按 deny 写回)
    let final_decision = wait_decision(&pending_req, &stop_flag);

    // 移除 pending
    pending.lock().unwrap().remove(&request_id);

    // 写回响应
    let resp = match final_decision {
        Decision::Allow(updated_input, updated_permissions) => {
            let mut o = json!({ "behavior": "allow" });
            if let Some(u) = updated_input {
                o["updatedInput"] = u;
            }
            if let Some(p) = updated_permissions {
                o["updatedPermissions"] = p;
            }
            o
        }
        Decision::Deny(msg) => json!({ "behavior": "deny", "message": msg }),
    };
    let mut out = stream;
    let line = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
    out.write_all(line.as_bytes())
        .map_err(|e| format!("写响应失败：{}", e))?;
    out.write_all(b"\n").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// 阻塞等待决策（condvar 挂起，永不超时）：命中用户响应或服务停止两者之一返回。
/// 不自动拒绝——卡住等用户点。respond/shutdown 写入决策后 notify 即醒；
/// 1s 超时兜底只为 Drop 等仅置 stop_flag 不写决策的路径，正常路径零空转
fn wait_decision(req: &PendingRequest, stop_flag: &Arc<std::sync::atomic::AtomicBool>) -> Decision {
    let mut slot = req.decision.lock().unwrap();
    loop {
        if let Some(d) = slot.as_ref() {
            return d.clone();
        }
        if stop_flag.load(Ordering::SeqCst) {
            // 服务停止时按拒绝处理
            return Decision::Deny("流式中断，自动拒绝".to_string());
        }
        let (guard, _) = req
            .ready
            .wait_timeout(slot, Duration::from_secs(1))
            .unwrap();
        slot = guard;
    }
}

// ----- 单元测试 -----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_token_unique() {
        let a = make_token();
        let b = make_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn parse_permission_request_ok() {
        let line = r#"{"toolName":"Bash","input":{"command":"ls"}}"#;
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v.get("toolName").unwrap().as_str().unwrap(), "Bash");
        assert_eq!(
            v.get("input")
                .unwrap()
                .get("command")
                .unwrap()
                .as_str()
                .unwrap(),
            "ls"
        );
    }
}
