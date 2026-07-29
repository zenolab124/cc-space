use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use crate::discovery;
use crate::models::*;
use crate::parser;
use crate::permission::PermissionService;
use crate::probe;
use crate::search;
use crate::streaming;
use crate::usage_stats;

/// 获取所有项目（含会话摘要）。async：全量扫描属重活，避免阻塞主线程排队其他 IPC
#[tauri::command]
pub async fn get_projects() -> Vec<Project> {
    discovery::discover_all()
}

/// 性能监视 HUD 数据源：app 相关进程的真实内存足迹
#[tauri::command]
pub async fn get_perf_stats() -> crate::perf::PerfStats {
    crate::perf::collect()
}

/// 获取会话消息记录（仅 user/assistant，跳过 snapshot 等大型记录）。async 同上
#[tauri::command]
pub async fn get_session_records(project_id: String, session_id: String) -> Vec<SessionRecord> {
    let path = session_path(&project_id, &session_id);
    let t0 = std::time::Instant::now();
    let records = parser::parse_messages(&path);
    if cfg!(debug_assertions) {
        // dev 埋点:与前端 [perf] 会话加载报告的 invoke 段对照,差值即 IPC 序列化成本
        eprintln!(
            "[perf] parse_messages {}: {} records · {:.1}ms",
            &session_id[..session_id.len().min(8)],
            records.len(),
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }
    records
}

/// 获取单个会话的摘要信息。走摘要缓存：projects-changed 增量路径高频调用，
/// 顺带热缓存使下次全量扫描免于重复解析
#[tauri::command]
pub async fn get_session_summary(
    project_id: String,
    session_id: String,
) -> Option<SessionSummary> {
    let path = session_path(&project_id, &session_id);
    crate::cache::get_summary(&path)
}

/// 软删除会话：写 metadata 删除标记，不动 jsonl（只读铁律）。
/// 前端列表/扫描侧过滤 deleted=true 的条目。
#[tauri::command]
pub fn delete_session(_project_id: String, session_id: String) -> Result<(), String> {
    crate::metadata::update_meta(
        session_id,
        crate::metadata::SessionMeta {
            deleted: Some(true),
            deleted_at: Some(chrono::Utc::now().to_rfc3339()),
            ..Default::default()
        },
    )?;
    Ok(())
}

/// 在终端中恢复会话。channel 非空时经 `--settings <渠道文件>` 带上会话渠道——
/// 终端用渠道原文件(非 runtime 合成):与「终端可直接复用渠道文件」的设计一致,
/// 终端环境的变量残留属用户自己的 shell 管辖,不做防御注入
#[tauri::command]
pub async fn resume_in_terminal(
    cwd: String,
    session_id: String,
    channel: Option<String>,
) -> Result<(), String> {
    let settings_part = match channel
        .as_deref()
        .filter(|c| !c.is_empty() && *c != crate::channels::OFFICIAL_ID)
    {
        Some(ch) => {
            crate::channels::validate_id(ch)?;
            let path = crate::channels::channel_file_path(ch);
            if !path.is_file() {
                return Err(format!("渠道配置不存在: {}", ch));
            }
            format!(" --settings \\\"{}\\\"", path.display())
        }
        None => String::new(),
    };
    let escaped_cwd = cwd.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"tell application "Terminal"
            activate
            do script "cd \"{}\" && claude{} --resume {}"
        end tell"#,
        escaped_cwd, settings_part, session_id
    );
    run_terminal_script(script).await
}

/// 在终端中执行 GUI 内无法运行的交互式斜杠命令（/login /logout /vim 等）：
/// 打开 Terminal 于会话 cwd，把命令作为 claude 的首条输入
#[tauri::command]
pub async fn run_slash_in_terminal(cwd: String, command: String) -> Result<(), String> {
    // 命令名只允许小写字母与连字符，防 AppleScript/shell 注入
    if command.is_empty() || !command.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        return Err(format!("非法命令名: {}", command));
    }
    let escaped_cwd = cwd.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"tell application "Terminal"
            activate
            do script "cd \"{}\" && claude \"/{}\""
        end tell"#,
        escaped_cwd, command
    );
    run_terminal_script(script).await
}

/// osascript 会阻塞在系统自动化授权弹窗上：放 blocking 线程等待结果，
/// 授权被拒（-1743）时返回前端可识别的错误标记
async fn run_terminal_script(script: String) -> Result<(), String> {
    let output = tauri::async_runtime::spawn_blocking(move || {
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-1743") || stderr.contains("Not authorized") {
            return Err(format!("AUTOMATION_DENIED: {}", stderr.trim()));
        }
        let msg = stderr.trim().to_string();
        return Err(if msg.is_empty() {
            format!("osascript exited with {}", output.status)
        } else {
            msg
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 权限体检（设置页）：主 app 与 runner 两本 TCC 账，见 tcc.rs
// ---------------------------------------------------------------------------

/// 静默检测主 app 账本的各项系统权限（零弹窗）
#[tauri::command]
pub fn check_system_permissions() -> serde_json::Value {
    serde_json::json!({
        "automationTerminal": crate::tcc::check_automation("com.apple.Terminal", false),
        "accessibility": crate::tcc::check_accessibility(),
        "screenCapture": crate::tcc::check_screen_capture(),
        "fullDiskAccess": crate::tcc::check_full_disk_access(),
        "localNetwork": probe_local_network(),
    })
}

/// 主动触发系统授权弹窗（仅用户点击驱动），返回请求后的最新状态。
/// denied 记录系统不会再弹：先 tccutil reset 清掉本 app 的旧记录（等价
/// 在系统设置里删除条目——频繁构建时代的旧 DR 记录靠这个自愈），再触发
#[tauri::command]
pub async fn request_system_permission(kind: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let reset_if_denied = |service: &str, currently: &str| {
            if currently == "denied" {
                let _ = std::process::Command::new("tccutil")
                    .args(["reset", service, "io.github.zenolab124.monet"])
                    .output();
            }
        };
        match kind.as_str() {
            "automationTerminal" => {
                reset_if_denied(
                    "AppleEvents",
                    crate::tcc::check_automation("com.apple.Terminal", false),
                );
                // 发一个无害真实事件：目标未运行会先拉起，未决则弹授权窗
                let _ = std::process::Command::new("osascript")
                    .args(["-e", r#"tell application "Terminal" to count windows"#])
                    .output();
                Ok(crate::tcc::check_automation("com.apple.Terminal", false).to_string())
            }
            "screenCapture" => {
                reset_if_denied("ScreenCapture", crate::tcc::check_screen_capture());
                Ok(crate::tcc::request_screen_capture().to_string())
            }
            "accessibility" => {
                reset_if_denied("Accessibility", crate::tcc::check_accessibility());
                Ok(crate::tcc::prompt_accessibility().to_string())
            }
            "localNetwork" => {
                Ok(probe_local_network().to_string())
            }
            _ => Err(format!("unsupported permission kind: {}", kind)),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn probe_local_network() -> &'static str {
    crate::tcc::check_local_network()
}

/// 打开系统设置对应隐私面板（白名单锚点）
#[tauri::command]
pub fn open_privacy_settings(panel: String) -> Result<(), String> {
    use crate::proc_ext::SpawnAndReap;
    let anchor = match panel.as_str() {
        "automation" => "Privacy_Automation",
        "accessibility" => "Privacy_Accessibility",
        "screenRecording" => "Privacy_ScreenCapture",
        "allFiles" => "Privacy_AllFiles",
        "filesAndFolders" => "Privacy_FilesAndFolders",
        "localNetwork" => "Privacy_LocalNetwork",
        _ => return Err(format!("unknown panel: {}", panel)),
    };
    std::process::Command::new("open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{}",
            anchor
        ))
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// runner 账本与主 app 分离（TCC 按 responsible process 记账），必须经
/// launchd 启动才是真实语境——主 app 直接 spawn 的话归因会挂到主 app 头上
#[tauri::command]
pub async fn run_runner_health_check(
    prompt_kind: Option<String>,
) -> Result<serde_json::Value, String> {
    if cfg!(not(target_os = "macos")) {
        return Err("macOS only".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        crate::scheduler::install_runner_binary()?;
        // dev 门控使 install 短路：健康检查是显式触发的瞬态 job（/tmp plist，
        // 不产生登录项），dev 下直接用 target 目录产物保持体检可用
        let runner = if crate::scheduler::dev_build() {
            crate::scheduler::bundled_runner_path()
        } else {
            crate::scheduler::runner_binary_path()
        };
        let result_path = runner_health_result_path();
        let _ = std::fs::remove_file(&result_path);

        // 与真实 routine 完全一致的启动机制（plist + bootstrap 到 gui 域）。
        // 不用 launchctl submit：submit 的 job 挂在提交者会话下，会继承
        // 主 app 的 TCC 语境，检测结果失真
        let label = "io.github.zenolab124.monet.health-check";
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "501".to_string());
        let domain = format!("gui/{}", uid);
        let service_target = format!("{}/{}", domain, label);

        // prompt 模式：runner 对指定权限发起请求式调用（弹系统授权窗）。
        // kind 走白名单防 plist 注入
        let prompt_args = match prompt_kind.as_deref() {
            Some(k @ ("automationSystemEvents" | "accessibility" | "screenCapture" | "localNetwork")) => {
                format!("\t\t<string>--prompt</string>\n\t\t<string>{}</string>\n", k)
            }
            Some(other) => return Err(format!("unsupported prompt kind: {}", other)),
            None => String::new(),
        };
        let plist_path = std::env::temp_dir().join("io.github.zenolab124.monet.health-check.plist");
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key><string>{}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{}</string>
		<string>--health-check</string>
{}	</array>
	<key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
            label,
            runner.display(),
            prompt_args
        );
        std::fs::write(&plist_path, &plist).map_err(|e| e.to_string())?;

        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &service_target])
            .output();
        let bootstrap = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain, &plist_path.to_string_lossy()])
            .output()
            .map_err(|e| e.to_string())?;
        if !bootstrap.status.success() {
            let _ = std::fs::remove_file(&plist_path);
            return Err(format!(
                "launchctl bootstrap failed: {}",
                String::from_utf8_lossy(&bootstrap.stderr).trim()
            ));
        }

        // prompt 模式会阻塞在系统授权窗上，等待时长给足用户反应时间
        let max_polls = if prompt_kind.is_some() { 600 } else { 50 };
        let mut content = None;
        for _ in 0..max_polls {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if result_path.exists() {
                // 结果文件出现后稍等写完
                std::thread::sleep(std::time::Duration::from_millis(150));
                content = std::fs::read_to_string(&result_path).ok();
                break;
            }
        }
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &service_target])
            .output();
        let _ = std::fs::remove_file(&plist_path);

        let Some(text) = content else {
            return Err("health check timed out".to_string());
        };
        serde_json::from_str(&text).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 读取上次 runner 健康检查结果（不触发新检测）
#[tauri::command]
pub fn get_runner_health_snapshot() -> Option<serde_json::Value> {
    std::fs::read_to_string(runner_health_result_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// 与 runner 侧硬编码一致：launchd 语境没有 MONET_DATA_DIR，
/// 双侧统一走默认家目录，避免 dev 环境变量导致读写错位
fn runner_health_result_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".monet")
        .join("permissions-runner.json")
}

/// 在 VSCode 中打开项目目录
#[tauri::command]
pub fn resume_in_vscode(cwd: String) -> Result<(), String> {
    use crate::proc_ext::SpawnAndReap;
    let url = format!("vscode://file{}", cwd);
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&url)
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    {
        use crate::proc_ext::HideConsole;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .hide_console() // cmd 是控制台程序，不抑制会闪黑窗
            .spawn_and_reap()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDir {
    /// projects 下的目录编码名（get_session_records 的 projectId）
    pub dir_name: String,
    /// 落盘目录绝对路径（Finder 打开用）
    pub path: String,
    /// 目录是否已存在（从未落盘时为 false，前端据此提示而非静默打开失败）
    pub exists: bool,
}

/// 内置 Agent / routine 落盘会话所在目录——「查看会话」与「打开目录」共用
#[tauri::command]
pub fn get_agent_session_dir() -> AgentSessionDir {
    let dir_name = crate::config::agent_project_dirs()
        .into_iter()
        .next()
        .unwrap_or_default();
    let dir = crate::config::projects_dir().join(&dir_name);
    let exists = dir.is_dir();
    AgentSessionDir {
        dir_name,
        path: dir.to_string_lossy().to_string(),
        exists,
    }
}

/// Windows 专属：原生标题栏亮暗跟随 app 主题。
/// 不用 window.set_theme——它会 override WebView 的 prefers-color-scheme，而前端
/// 亮暗双槽机制以该信号为输入，回授成环（实测切主题无限闪烁）。直调 DWM
/// 属性只影响标题栏，不触碰 WebView。非 Windows 平台 no-op（macOS 标题栏自绘）。
#[tauri::command]
pub fn set_titlebar_dark(window: tauri::WebviewWindow, dark: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE,
        };
        if let Ok(hwnd) = window.hwnd() {
            let value: i32 = if dark { 1 } else { 0 };
            unsafe {
                DwmSetWindowAttribute(
                    hwnd.0 as _,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    &value as *const i32 as *const core::ffi::c_void,
                    std::mem::size_of::<i32>() as u32,
                );
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (window, dark);
}

/// 在系统文件管理器中打开目录（macOS: Finder / Windows: 资源管理器 / Linux: 默认）
#[tauri::command]
pub fn open_in_finder(path: String) -> Result<(), String> {
    use crate::proc_ext::SpawnAndReap;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&path)
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&path)
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&path)
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 在系统文件管理器中显示并高亮文件（macOS: open -R / Windows: explorer /select,）。
/// Linux 无跨桌面的 reveal 协议，退化为打开所在目录。
#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    use crate::proc_ext::SpawnAndReap;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn_and_reap()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // /select, 与路径须作为一个参数整体传入,std 的引号封装会破坏该格式,用 raw_arg
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path))
            .spawn_and_reap()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        let dir = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path.clone());
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn_and_reap()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 发送消息（长活进程：自动 open + stdin 写入；替代旧 per-message spawn）。
/// async + spawn_blocking：open_session 的初始化握手是阻塞 I/O，不能卡 IPC 主线程
#[allow(clippy::too_many_arguments)] // Tauri command 参数由前端调用签名决定
#[tauri::command]
pub async fn start_streaming(
    app: tauri::AppHandle,
    session_id: String,
    cwd: String,
    message: String,
    model: Option<String>,
    effort: Option<String>,
    channel: Option<String>,
    advisor: bool,
    chrome: Option<bool>,
    fork_source: Option<String>,
    extra_args: Option<String>,
    images: Option<Vec<serde_json::Value>>,
    permission_mode: Option<String>,
    append_system_prompt: Option<String>,
    force_new: Option<bool>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        streaming::send_message(
            &app,
            &session_id,
            &cwd,
            &message,
            model.as_deref(),
            effort.as_deref(),
            channel.as_deref(),
            advisor,
            chrome.unwrap_or(false),
            fork_source.as_deref(),
            extra_args.as_deref(),
            images.as_deref(),
            permission_mode.as_deref(),
            append_system_prompt.as_deref(),
            force_new.unwrap_or(false),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 中断当前回复（发 interrupt 控制消息，不杀进程）
#[tauri::command]
pub fn stop_streaming(session_id: String) -> Result<(), String> {
    streaming::interrupt_session(&session_id)
}

/// 运行时切换权限模式
#[tauri::command]
pub fn set_permission_mode(session_id: String, mode: String) -> Result<(), String> {
    streaming::set_permission_mode(&session_id, &mode)
}

/// 开关 Remote Control（进程未启动时自动连接）
#[allow(clippy::too_many_arguments)] // Tauri command 参数由前端调用签名决定
#[tauri::command]
pub async fn toggle_remote_control(
    app: tauri::AppHandle,
    session_id: String,
    cwd: String,
    model: Option<String>,
    effort: Option<String>,
    channel: Option<String>,
    advisor: bool,
    chrome: Option<bool>,
    fork_source: Option<String>,
    extra_args: Option<String>,
    enabled: bool,
    permission_mode: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        streaming::toggle_remote_control(
            &app,
            &session_id,
            &cwd,
            model.as_deref(),
            effort.as_deref(),
            channel.as_deref(),
            advisor,
            chrome.unwrap_or(false),
            fork_source.as_deref(),
            extra_args.as_deref(),
            enabled,
            permission_mode.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 关闭会话进程（SIGTERM → 5s → SIGKILL）
#[tauri::command]
pub fn close_session(session_id: String) {
    streaming::close_session(&session_id);
}

/// 前端响应权限请求
///
/// `allow=true` 时透传给 claude CLI `{"behavior":"allow","updatedInput"?:...}`，
/// updated_input 用于交互工具（AskUserQuestion 答案注入等），缺省由 mcp 回填原 input
/// `allow=false` 时返回 `{"behavior":"deny","message":...}`，message 缺省为「用户拒绝」
#[tauri::command]
pub fn respond_permission(
    request_id: String,
    allow: bool,
    message: Option<String>,
    updated_input: Option<serde_json::Value>,
    updated_permissions: Option<serde_json::Value>,
) -> Result<(), String> {
    let ok = PermissionService::respond(&request_id, allow, message, updated_input, updated_permissions);
    if ok {
        Ok(())
    } else {
        Err(format!(
            "未找到 pending 权限请求，可能已超时或已被处理：{}",
            request_id
        ))
    }
}

/// CLI 全局配置摘要(~/.claude/settings.json):顶栏「默认」项展示真值用
#[derive(serde::Serialize)]
pub struct CliSettings {
    pub model: Option<String>,
    pub effort_level: Option<String>,
    pub ultracode: bool,
    pub permission_mode: Option<String>,
}

/// 读取 CLI settings.json 的模型/努力默认值。
/// settings.json 是活文件(CLI 内 /effort 等实时改写),每次调用现读现解析、
/// 绝不进程级缓存,见 docs/knowledge/pitfalls/cli-settings-live-rewrite.md
///
/// cwd 可选：传入时 permission_mode 按 Claude Code 优先级链读取
/// (Local > Project > User)，不传时只读 ~/.claude/settings.json
#[tauri::command]
pub fn get_cli_settings(cwd: Option<String>) -> CliSettings {
    let path = crate::config::claude_root().join("settings.json");
    let json: Option<serde_json::Value> = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let get_str = |key: &str| {
        json.as_ref()
            .and_then(|j| j.get(key))
            .and_then(|v| v.as_str())
            .map(String::from)
    };
    let perm_mode = cwd.as_deref()
        .and_then(crate::streaming::resolve_default_permission_mode)
        .or_else(|| {
            json.as_ref()
                .and_then(|j| j.get("permissions"))
                .and_then(|p| p.get("defaultMode"))
                .and_then(|v| v.as_str())
                .map(String::from)
        });
    CliSettings {
        model: get_str("model"),
        effort_level: get_str("effortLevel"),
        ultracode: json
            .as_ref()
            .and_then(|j| j.get("ultracode"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        permission_mode: perm_mode,
    }
}

/// 外部会话进程信息：是否在跑 + 归属应用（父进程链解析），横幅与停止确认共用
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSessionInfo {
    pub running: bool,
    pub pid: Option<u32>,
    pub owner: Option<String>,
}

/// 判断一条 ps 命令行是否是「跑着指定会话的 claude 进程」本体。双精确条件防误伤：
/// 1. 存在 basename 恰为 "claude" 的 token（可执行本体，而非 `~/.claude/...` 路径巧合）
/// 2. session_id 以独立 token 出现（而非 `<sid>.jsonl` 之类的子串）
///
/// 因此 `tail -f ~/.claude/projects/x/<sid>.jsonl`、`vim <sid>.jsonl`、`grep <sid> ~/.claude/…` 均不命中。
fn command_matches_claude_session(cmd: &str, session_id: &str) -> bool {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let has_claude_bin = tokens.iter().any(|t| t.rsplit('/').next().unwrap_or(t) == "claude");
    if !has_claude_bin {
        return false;
    }
    // --fork-session 时真实身份是 --session-id 的值，--resume 的值是源会话不算归属
    if tokens.contains(&"--fork-session") {
        tokens.windows(2).any(|w| w[0] == "--session-id" && w[1] == session_id)
    } else {
        tokens.contains(&session_id)
    }
}

/// 扫描外部 claude 进程：命中匹配且非 Monet 自持进程的 pid 列表。
/// 交互式 REPL（命令行不带 session-id）检测不到，属已知边界。
/// 数据源走 proc_scan（macOS 纯 syscall / 其他平台 ps 降级），本函数在秒级
/// 轮询节拍下常驻调用，禁止在此重新引入 spawn。
/// fresh：kill 等落点动作传 true 绕过短缓存（防 pid 复用误伤）；轮询探测传 false
/// 让多个探测方共享同窗口的一次全量扫描
fn scan_external_claude(session_id: &str, fresh: bool) -> Vec<u32> {
    let own_pid = crate::streaming::get_own_pid(session_id);
    let commands = if fresh {
        crate::proc_scan::list_commands()
    } else {
        crate::proc_scan::list_commands_cached()
    };
    commands
        .into_iter()
        .filter_map(|(pid, cmd)| {
            if own_pid == Some(pid) || !command_matches_claude_session(&cmd, session_id) {
                return None;
            }
            Some(pid)
        })
        .collect()
}

/// 沿父进程链解析归属应用名：跳过 shell/login 中转层，取第一个真实应用。
/// 例：claude ← zsh ← login ← Terminal 解析为 "Terminal"；
///     claude ← SomeApp 直挂时解析为 "SomeApp"。
fn resolve_owner(pid: u32, table: &std::collections::HashMap<u32, (u32, String)>) -> Option<String> {
    const SHELLS: [&str; 6] = ["sh", "zsh", "bash", "fish", "dash", "login"];
    let mut cur = pid;
    for _ in 0..6 {
        let (ppid, _) = table.get(&cur)?;
        if *ppid <= 1 {
            return None;
        }
        let (_, parent_name) = table.get(ppid)?;
        let clean = parent_name.trim_start_matches('-');
        if SHELLS.contains(&clean) {
            cur = *ppid;
            continue;
        }
        if clean.is_empty() {
            return None;
        }
        return Some(clean.to_string());
    }
    None
}

/// 检测 Monet 自身是否为该会话持有长活进程（webview 刷新后 processAlive 状态校准用）。
/// 与 check_session_running 互补：那边只看外部进程，这边只看自有进程。
/// 返回进程启动时刻 epoch ms（None = 无进程）——异步账本按它判进程代际。
#[tauri::command]
pub fn has_own_process(session_id: String) -> Option<i64> {
    crate::streaming::get_own_started_at_ms(&session_id)
}

/// 检测某会话是否有**外部** claude CLI 进程在运行，并解析归属方。
/// 排除 Monet 自身持有的长活进程，只报告终端 / VS Code / 其他会话管理器等外部进程。
#[tauri::command]
pub fn check_session_running(session_id: String) -> ExternalSessionInfo {
    match scan_external_claude(&session_id, false).first() {
        Some(&pid) => {
            let owner = resolve_owner(pid, &crate::proc_scan::process_table_cached());
            ExternalSessionInfo { running: true, pid: Some(pid), owner }
        }
        None => ExternalSessionInfo { running: false, pid: None, owner: None },
    }
}

/// 终止指定会话的外部 CLI 进程（用户在 Monet 点"停止"并确认后调用）。
/// SIGTERM 温和终止；排除 Monet 自身持有的长活进程。
#[tauri::command]
pub fn kill_external_session(session_id: String) -> bool {
    let pids = scan_external_claude(&session_id, true);
    for pid in &pids {
        use crate::proc_ext::HideConsole;
        let _ = std::process::Command::new("kill").arg(pid.to_string()).hide_console().output();
    }
    !pids.is_empty()
}

#[cfg(test)]
mod external_session_tests {
    use super::*;

    const SID: &str = "0ebe7955-ab04-4f5b-b5b9-8362886222aa";

    #[test]
    fn matches_real_claude_invocations() {
        assert!(command_matches_claude_session(&format!("claude --resume {SID}"), SID));
        assert!(command_matches_claude_session(
            &format!("/Users/x/.local/bin/claude --print --resume {SID}"),
            SID
        ));
        // node 包装：第二个 token 是 claude 本体
        assert!(command_matches_claude_session(
            &format!("node /usr/local/bin/claude --session-id {SID}"),
            SID
        ));
    }

    #[test]
    fn rejects_path_coincidences() {
        // 查看/编辑 JSONL 的无辜进程：路径含 .claude 与 sid 子串，但无 claude 二进制 token、sid 非独立 token
        assert!(!command_matches_claude_session(
            &format!("tail -f /Users/x/.claude/projects/p/{SID}.jsonl"),
            SID
        ));
        assert!(!command_matches_claude_session(&format!("vim {SID}.jsonl"), SID));
        assert!(!command_matches_claude_session(
            &format!("grep {SID} /Users/x/.claude/projects"),
            SID
        ));
        // sid 独立 token 但没有 claude 二进制
        assert!(!command_matches_claude_session(&format!("echo {SID}"), SID));
    }

    #[test]
    fn fork_not_counted_as_source_session() {
        const SRC: &str = "aaaa1111-0000-0000-0000-000000000000";
        const FORKED: &str = "bbbb2222-0000-0000-0000-000000000000";
        let cmd = format!("claude --resume {SRC} --fork-session --session-id {FORKED}");
        // 源会话不应命中
        assert!(!command_matches_claude_session(&cmd, SRC));
        // 分叉新会话才命中
        assert!(command_matches_claude_session(&cmd, FORKED));
    }

    #[test]
    fn owner_resolution_skips_shells() {
        let mut t = std::collections::HashMap::new();
        // 终端链:claude(100) ← zsh(90) ← login(80) ← Terminal(70) ← launchd(1)
        t.insert(100u32, (90u32, "claude".to_string()));
        t.insert(90, (80, "zsh".to_string()));
        t.insert(80, (70, "login".to_string()));
        t.insert(70, (1, "Terminal".to_string()));
        assert_eq!(resolve_owner(100, &t).as_deref(), Some("Terminal"));

        // GUI 直挂:claude(200) ← SomeApp(150) ← launchd(1)
        t.insert(200, (150, "claude".to_string()));
        t.insert(150, (1, "SomeApp".to_string()));
        assert_eq!(resolve_owner(200, &t).as_deref(), Some("SomeApp"));

        // 父链到顶仍是 shell → None
        let mut o = std::collections::HashMap::new();
        o.insert(300u32, (1u32, "claude".to_string()));
        assert_eq!(resolve_owner(300, &o), None);
    }
}

/// 全项目 token 用量聚合（v2.2.0 FR-001）：首页 Token 卡 / 活跃热力图数据源。
/// 全量扫描秒级耗时，丢 blocking 线程池跑，不占 IPC 主线程
#[tauri::command]
pub async fn get_usage_stats() -> Result<usage_stats::UsageStats, String> {
    tauri::async_runtime::spawn_blocking(usage_stats::collect_usage_stats)
        .await
        .map_err(|e| e.to_string())?
}

/// 全局搜索：多词 AND 匹配缓存文本。首查触发建缓存 + dirty 懒重提取，
/// 属重活，丢 blocking 线程池
#[tauri::command]
pub async fn search_query(
    query: String,
    filter: search::SearchFilter,
) -> Result<search::SearchResult, String> {
    tauri::async_runtime::spawn_blocking(move || search::query(&query, &filter))
        .await
        .map_err(|e| e.to_string())
}

/// 搜索索引状态（就绪与否 / 已索引会话数）
#[tauri::command]
pub fn search_status() -> search::SearchStatus {
    search::status()
}

/// 语义搜索结果（在 SearchResult 基础上附带 Agent 关键词组 + 归纳摘要）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartSearchResult {
    #[serde(flatten)]
    pub search: search::SearchResult,
    pub term_groups: Vec<String>,
    pub summary: Option<String>,
}

/// 语义搜索：Agent 翻译自然语言 → 多组关键词 → 逐组搜索 → 合并去重
#[tauri::command]
pub async fn smart_search(
    question: String,
    filter: search::SearchFilter,
    model: Option<String>,
    effort: Option<String>,
) -> Result<SmartSearchResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let m = model.as_deref().unwrap_or("sonnet");
        let e = effort.as_deref().unwrap_or("low");
        let term_groups = crate::agent::extract_search_terms(&question, m, e)?;
        eprintln!("[smart-search] 关键词组: {:?}", term_groups);
        let t0 = std::time::Instant::now();
        let mut seen = std::collections::HashSet::new();
        let mut merged = Vec::new();
        for terms in &term_groups {
            let r = search::query(terms, &filter);
            for hit in r.hits {
                if seen.insert(hit.session_id.clone()) {
                    merged.push(hit);
                }
            }
        }
        merged.sort_unstable_by(|a, b| {
            b.last_modified.partial_cmp(&a.last_modified).unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = merged.len();
        merged.truncate(50);

        // 把前几条命中的完整上下文（±3 消息窗口，与 recall 等价）喂给 Agent 归纳
        let all_terms = search::compile_terms(&term_groups.join(" "));
        let summary = if merged.is_empty() {
            None
        } else {
            let mut ctx = String::new();
            for (i, hit) in merged.iter().take(6).enumerate() {
                ctx.push_str(&format!("━━ 会话 {} ━━\n标题: {}\n\n",
                    i + 1,
                    hit.title.as_deref().unwrap_or("(无标题)")
                ));
                let messages = search::get_hit_context(
                    &hit.session_id, &all_terms, 3, 3000,
                );
                for (label, text) in &messages {
                    if label == "---" {
                        ctx.push_str("  ┄\n");
                    } else {
                        ctx.push_str(&format!("{} {}\n", label, text));
                    }
                }
                ctx.push('\n');
            }
            match crate::agent::summarize_search_hits(&question, &ctx, m, e) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("[smart-search] 归纳失败: {}", e);
                    None
                }
            }
        };

        Ok(SmartSearchResult {
            search: search::SearchResult {
                hits: merged,
                total_hits: total,
                elapsed_ms: t0.elapsed().as_millis() as u64,
            },
            term_groups,
            summary,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// schema-probe 全量扫描（v2.2.0 FR-004）：首页兼容性诊断卡数据源。
/// 返回结构与 CLI `--json` 输出同构（既有契约）
#[tauri::command]
pub async fn get_schema_diagnosis() -> Result<probe::Report, String> {
    tauri::async_runtime::spawn_blocking(|| probe::run_probe(None))
        .await
        .map_err(|e| e.to_string())?
}

/// 用系统默认应用打开文件(macOS: open / Windows: cmd start / Linux: xdg-open)
#[tauri::command]
pub fn open_in_default_app(path: String) -> Result<(), String> {
    use crate::proc_ext::SpawnAndReap;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn_and_reap()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        use crate::proc_ext::HideConsole;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path])
            .hide_console() // cmd 是控制台程序，不抑制会闪黑窗
            .spawn_and_reap()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn_and_reap()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico"];
const MAX_IMAGE_SIZE: u64 = 50 * 1024 * 1024;

#[derive(serde::Serialize)]
pub struct LocalImage {
    pub data: String,
    pub mime_type: String,
}

#[tauri::command]
pub fn read_local_image(path: String) -> Result<LocalImage, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("文件不存在: {}", path));
    }

    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("不支持的图片格式: {}", ext));
    }

    let meta = fs::metadata(&p).map_err(|e| e.to_string())?;
    if meta.len() > MAX_IMAGE_SIZE {
        return Err(format!(
            "图片过大: {:.1}MB（上限 {}MB）",
            meta.len() as f64 / 1_048_576.0,
            MAX_IMAGE_SIZE / 1_048_576
        ));
    }

    let mime_type = match ext.as_str() {
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
    .to_string();

    let bytes = fs::read(&p).map_err(|e| e.to_string())?;
    let data = STANDARD.encode(&bytes);

    Ok(LocalImage { data, mime_type })
}

fn session_path(project_id: &str, session_id: &str) -> PathBuf {
    crate::config::projects_dir()
        .join(project_id)
        .join(format!("{}.jsonl", session_id))
}

// ---------- 子 Agent ----------

#[derive(serde::Serialize)]
pub struct SubAgentMeta {
    pub agent_id: String,
    pub tool_use_id: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub workflow_id: Option<String>,
}

#[tauri::command]
pub fn list_subagents(project_id: String, session_id: String) -> Vec<SubAgentMeta> {
    let dir = crate::config::projects_dir()
        .join(&project_id)
        .join(&session_id)
        .join("subagents");
    let mut result = Vec::new();

    // Helper: parse agent-*.meta.json entries from a directory, tagging with workflow_id
    let scan_dir = |dir: &std::path::Path, workflow_id: Option<String>, out: &mut Vec<SubAgentMeta>| {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.ends_with(".meta.json") {
                continue;
            }
            let agent_id = name
                .strip_prefix("agent-")
                .and_then(|s| s.strip_suffix(".meta.json"))
                .unwrap_or("")
                .to_string();
            if agent_id.is_empty() {
                continue;
            }
            if let Ok(bytes) = fs::read(entry.path()) {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    out.push(SubAgentMeta {
                        agent_id,
                        tool_use_id: v["toolUseId"].as_str().unwrap_or("").to_string(),
                        agent_type: v["agentType"].as_str().map(String::from),
                        description: v["description"].as_str().map(String::from),
                        workflow_id: workflow_id.clone(),
                    });
                }
            }
        }
    };

    // 1) Direct subagents: subagents/agent-*.meta.json
    scan_dir(&dir, None, &mut result);

    // 2) Workflow subagents: subagents/workflows/*/agent-*.meta.json
    let workflows_dir = dir.join("workflows");
    if let Ok(wf_entries) = fs::read_dir(&workflows_dir) {
        for wf_entry in wf_entries.flatten() {
            if wf_entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                let wf_id = wf_entry.file_name().to_string_lossy().to_string();
                scan_dir(&wf_entry.path(), Some(wf_id), &mut result);
            }
        }
    }

    result
}

/// 读取后台任务输出文件（/private/tmp/claude-<uid>/.../tasks/<id>.output）尾部。
/// 该目录是 CLI 的临时区、可能已被系统清理——不存在时返回 None 而非报错。
/// 路径白名单锁定在 claude 任务临时目录，防任意文件读取。
#[tauri::command]
pub fn read_task_output(path: String, max_bytes: Option<u64>) -> Option<String> {
    let p = std::path::Path::new(&path);
    // unix 固定形态 /tmp/claude-<uid>/…；Windows 无此形态，兜底认系统临时目录下的 claude-* 子树
    let is_claude_tmp = path.starts_with("/private/tmp/claude-")
        || path.starts_with("/tmp/claude-")
        || p
            .strip_prefix(std::env::temp_dir())
            .ok()
            .and_then(|rest| rest.components().next())
            .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("claude-"));
    if !is_claude_tmp || path.contains("..") {
        return None;
    }
    let len = fs::metadata(p).ok()?.len();
    let cap = max_bytes.unwrap_or(64 * 1024);
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut f = fs::File::open(p).ok()?;
    let mut truncated = false;
    if len > cap {
        f.seek(SeekFrom::Start(len - cap)).ok()?;
        truncated = true;
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    // 截断点可能落在多字节字符/行中间，去掉首个不完整行
    let text = if truncated {
        match text.find('\n') {
            Some(i) => format!("…{}", &text[i + 1..]),
            None => text.into_owned(),
        }
    } else {
        text.into_owned()
    };
    Some(text)
}

#[tauri::command]
pub fn get_subagent_records(
    project_id: String,
    session_id: String,
    agent_id: String,
) -> Vec<SessionRecord> {
    let subagents_dir = crate::config::projects_dir()
        .join(&project_id)
        .join(&session_id)
        .join("subagents");
    let filename = format!("agent-{}.jsonl", agent_id);

    // 1) Try direct path: subagents/agent-{id}.jsonl
    let direct = subagents_dir.join(&filename);
    if direct.exists() {
        return parser::parse_messages(&direct);
    }

    // 2) Fallback: search subagents/workflows/*/agent-{id}.jsonl
    let workflows_dir = subagents_dir.join("workflows");
    if let Ok(wf_entries) = fs::read_dir(&workflows_dir) {
        for wf_entry in wf_entries.flatten() {
            let candidate = wf_entry.path().join(&filename);
            if candidate.exists() {
                return parser::parse_messages(&candidate);
            }
        }
    }

    vec![]
}

// ---- 工作区 git 只读快照(PRD v2.6.0 FR-004) ----

#[derive(serde::Serialize)]
pub struct GitSnapshotEntry {
    /// porcelain XY 两字符状态码(" M"/"??"/"A "等)
    pub status: String,
    pub path: String,
}

#[derive(serde::Serialize)]
pub struct GitWorktreeSnapshot {
    pub is_repo: bool,
    pub entries: Vec<GitSnapshotEntry>,
    pub truncated: bool,
}

const GIT_SNAPSHOT_MAX_ENTRIES: usize = 200;

fn run_git_readonly_blocking(cwd: &str, args: &[&str]) -> Result<std::process::Output, String> {
    use crate::proc_ext::HideConsole;
    use std::process::{Command, Stdio};
    // git 位于 /usr/bin,系统命令允许裸调(spawn 铁律豁免清单);
    // 3s 超时防网络挂载盘等场景挂起(项目无 tokio 直依赖,try_wait 轮询实现)
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .hide_console()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => return child.wait_with_output().map_err(|e| e.to_string()),
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                return Err("git timeout".to_string());
            }
            None => std::thread::sleep(std::time::Duration::from_millis(30)),
        }
    }
}

/// 工作区 git 只读快照:是否仓库 + 未提交条目(porcelain v1 -z 解析)。
/// 白名单仅 rev-parse 与 status 两条只读子命令——本 command 永不扩展写操作
/// (NFR-002 代码审查口径:实现中不得出现任何修改工作区/暂存区的 git 子命令)。
#[tauri::command]
pub async fn git_worktree_snapshot(cwd: String) -> Result<GitWorktreeSnapshot, String> {
    let (probe, out) = tauri::async_runtime::spawn_blocking(move || {
        let probe = run_git_readonly_blocking(&cwd, &["rev-parse", "--is-inside-work-tree"])?;
        if !probe.status.success() || String::from_utf8_lossy(&probe.stdout).trim() != "true" {
            return Ok::<_, String>((probe, None));
        }
        let out = run_git_readonly_blocking(&cwd, &["status", "--porcelain=v1", "-z"])?;
        Ok((probe, Some(out)))
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(out) = out else {
        let _ = probe;
        return Ok(GitWorktreeSnapshot { is_repo: false, entries: vec![], truncated: false });
    };
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    // -z 输出:NUL 分隔,每条 "XY path";R/C 状态后额外跟一个源路径 token,须吞掉
    let raw = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut tokens = raw.split('\0').filter(|s| !s.is_empty());
    while let Some(tok) = tokens.next() {
        if tok.len() < 4 {
            continue;
        }
        let (xy, path) = tok.split_at(2);
        let path = path.trim_start_matches(' ');
        if xy.starts_with('R') || xy.starts_with('C') {
            let _ = tokens.next(); // 吞 rename/copy 的源路径
        }
        if entries.len() >= GIT_SNAPSHOT_MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(GitSnapshotEntry { status: xy.to_string(), path: path.to_string() });
    }
    Ok(GitWorktreeSnapshot { is_repo: true, entries, truncated })
}

/// 读取 ~/.monet/settings.json 单个顶层键（UI 偏好收编：theme/locale/zoomFactor 等）
#[tauri::command]
pub fn get_app_setting(key: String) -> Option<serde_json::Value> {
    crate::config::read_app_setting(&key)
}

/// 写入 ~/.monet/settings.json 单个顶层键；value 为 null 时删除该键
#[tauri::command]
pub fn set_app_setting(key: String, value: serde_json::Value) {
    crate::config::write_app_setting(&key, value);
}
