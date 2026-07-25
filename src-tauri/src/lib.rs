use tauri::Manager;

mod cache;
mod channels;
/// pub：定位逻辑同时被 monet-routine-runner 以 #[path] 方式复用
pub mod claude_locator;
pub mod config;
mod commands;
mod discovery;
mod image_protocol;
pub mod models;
mod parser;
mod permission;
pub mod pricing;
pub mod proc_ext;
mod proc_scan;
/// pub：schema-probe bin 复用扫描与 diff 核心
pub mod probe;
mod cron_expr;
mod path_env;
mod streaming;
mod menu;
pub mod usage_stats;
mod perf;
mod watcher;
mod agent;
mod automation;
mod metadata;
mod routines;
pub mod routine_run;
/// pub：结构定义同时被 monet-mcp / monet-routine-runner 以 #[path] 方式复用
pub mod routine_types;
/// pub：搜索引擎同时被 monet-mcp 以 #[path] 方式复用（search_sessions 工具）
pub mod search;
mod scheduler;
/// pub：唤醒计划逻辑同时被 monet-routine-runner 以 #[path] 方式复用
pub mod wake;
#[cfg(target_os = "macos")]
mod signing;
mod tcc;
mod workshop;
mod cli_env;
mod cli_settings;
/// pub：类型+获取逻辑同时被 monet-tray 独立二进制复用
pub mod quota;
mod tray_agent;
mod translate;
mod turn_signal;
mod widget;
/// pub：候选清单+pid+日志回读，MCP bin 经 #[path] 共享
pub mod runner_store;
mod runners;


#[cfg(target_os = "macos")]
extern "C" {
    /// src/native/high_refresh.m：swizzle WKWebView 初始化器解锁 ProMotion 高刷。
    /// 必须先于任何 WKWebView 创建执行——feature 仅在创建时刻读取
    fn monet_install_high_refresh_unlock();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "macos")]
    unsafe {
        monet_install_high_refresh_unlock();
    }
    tauri::Builder::default()
        // ccimg 自定义协议：历史区图片按需取（base64 已从 records 剥离）。
        // 异步 responder：读 JSONL/decode 走 tauri 线程池，不阻塞主线程。
        .register_asynchronous_uri_scheme_protocol("ccimg", |_ctx, request, responder| {
            image_protocol::handle_request(request, responder);
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin({
            let mut ws = tauri_plugin_window_state::Builder::new();
            if cfg!(debug_assertions) {
                ws = ws.skip_initial_state("main");
            }
            ws.build()
        })
        .setup(|app| {
            // 原生菜单仅 macOS：Windows 的 win32 菜单条不跟 app 主题且文案不走 i18n，
            // 快捷键等价由前端 keydown 承载（useShortcuts），Linux 同理
            #[cfg(target_os = "macos")]
            {
                app.set_menu(menu::create(app.handle())?)?;
                app.on_menu_event(|app, event| menu::handle_event(app, &event));
            }
            // 日志双档：dev 全量 Info；release 只落 WARN+ 到 LogDir，线上问题可查。
            // 时间戳用本地时区（默认 UTC 与系统日志/文件 mtime 对不上，排查时易错位 8 小时）
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(if cfg!(debug_assertions) {
                        log::LevelFilter::Info
                    } else {
                        log::LevelFilter::Warn
                    })
                    .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                    .max_file_size(5 * 1024 * 1024)
                    .build(),
            )?;

            if cfg!(debug_assertions) {
                // 开发模式：窗口移到非主显示器（扩展屏）
                move_to_secondary_monitor(app);
            }
            // 启动文件监控
            watcher::start(app.handle());
            // 会话状态跟踪扩展：已安装则恢复信号监听
            turn_signal::start_listener_if_installed(app.handle().clone());
            // 渠道 runtime 残留清理(上次异常退出可能留下含 token 的合成文件)
            channels::cleanup_runtime_dir();
            // 会话进程残留清扫(上次崩溃/强杀遗留的 CLI+MCP 孤儿树,须在任何会话 spawn 前)
            streaming::cleanup_orphans();
            // 跑单 supervisor 初始化 + 日志 flusher + 孤儿清扫
            // 候选清单变更感知已迁入 watcher.rs（data_dir 监听 runner-commands.json）
            runners::init(app.handle());
            // AgentService 常驻进程
            agent::init();
            // Apple FM 自动检测
            channels::register_apple_fm_if_available();
            // 系统级定时任务调度器同步（launchctl/pmset 耗时且可能弹授权框，不得阻塞主线程）
            tauri::async_runtime::spawn_blocking(routines::startup_sync);
            // 后台刷新 CLI settings schema
            cli_settings::refresh_settings_schema();
            // MCP 二进制启动自愈（存量 adhoc 安装收敛到稳定签名）
            cli_settings::startup_sync_mcp();
            // Widget LaunchAgent 自动安装
            widget::ensure_launch_agent();
            // Tray LaunchAgent 自动安装（独立 menubar 进程）。
            // launchctl bootout/bootstrap 有 IO 开销，不阻塞主线程
            tauri::async_runtime::spawn_blocking(tray_agent::ensure_launch_agent);
            // 搜索缓存预热：延迟避开启动高峰，后台建/对账文本缓存
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(3));
                search::warm();
            });

            // WebView 原生底色钉成主题色:窗口出现到 HTML 内联底色接管之间(~130ms)
            // 的空窗期不再是 WKWebView 默认白底,而是与主题同色系——消灭启动白闪的
            // 最后一层(HTML 后由 index.html 内联脚本按 localStorage 精确接管)
            if let Some(w) = app.get_webview_window("main") {
                let dark = matches!(w.theme(), Ok(tauri::Theme::Dark));
                let color = if dark {
                    tauri::webview::Color(30, 30, 30, 255) // ink #1e1e1e
                } else {
                    tauri::webview::Color(242, 235, 220, 255) // paper #F2EBDC
                };
                let _ = w.set_background_color(Some(color));
            }

            // 关窗=收起（macOS 标准文档型行为）：进程与 Dock 图标保留，
            // 点 Dock/Reopen 恢复；真退出走 Cmd+Q（quit_app）。
            // 其他平台无 Dock 恢复入口，放行默认关闭（Destroyed 里做会话清理）
            let handle = app.handle().clone();
            if let Some(window) = handle.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                let w = window.clone();
                window.on_window_event(move |event| {
                    match event {
                        #[cfg(target_os = "macos")]
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            api.prevent_close();
                            let _ = w.hide();
                        }
                        tauri::WindowEvent::Destroyed => {
                            streaming::close_all_sessions();
                            runners::shutdown_all();
                            agent::shutdown();
                            channels::shutdown_fm_serve();
                        }
                        _ => {}
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_projects,
            commands::get_app_setting,
            commands::set_app_setting,
            commands::get_session_records,
            commands::get_session_summary,
            commands::get_perf_stats,
            commands::delete_session,
            commands::resume_in_terminal,
            commands::run_slash_in_terminal,
            commands::check_system_permissions,
            commands::request_system_permission,
            commands::open_privacy_settings,
            commands::run_runner_health_check,
            commands::get_runner_health_snapshot,
            commands::resume_in_vscode,
            commands::open_in_finder,
            commands::reveal_in_finder,
            commands::set_titlebar_dark,
            commands::get_agent_session_dir,
            commands::start_streaming,
            commands::stop_streaming,
            commands::close_session,
            commands::toggle_remote_control,
            commands::respond_permission,
            commands::set_permission_mode,
            commands::get_cli_settings,
            commands::check_session_running,
            commands::has_own_process,
            commands::kill_external_session,
            commands::list_subagents,
            commands::get_subagent_records,
            commands::read_task_output,
            commands::git_worktree_snapshot,
            commands::get_usage_stats,
            commands::get_schema_diagnosis,
            commands::search_query,
            commands::search_status,
            commands::smart_search,
            workshop::get_workshop_assets,
            workshop::probe_mcp_server,
            workshop::open_workshop_dir,
            workshop::get_asset_detail,
            workshop::open_asset_file,
            workshop::get_memory_raw,
            workshop::open_memory_index,
            workshop::mcp_add,
            workshop::mcp_remove,
            workshop::mcp_reset_project_choices,
            workshop::get_hooks_overview,
            workshop::get_memory_overview,
            workshop::get_memory_detail,
            workshop::save_memory,
            workshop::delete_memory,
            automation::get_hooks_config,
            automation::get_hooks_stats,
            automation::open_hooks_config,
            turn_signal::turn_signal_status,
            turn_signal::turn_signal_install,
            turn_signal::turn_signal_uninstall,
            cli_env::claude_env_check,
            cli_env::claude_env_upgrade,
            cli_env::claude_env_install,
            cli_env::claude_env_diagnose,
            channels::list_channels,
            channels::get_cli_env_target,
            channels::save_channel,
            channels::set_official_defaults,
            channels::delete_channel,
            channels::set_channel_enabled,
            channels::set_default_session_channel,
            channels::set_default_agent_model,
            channels::get_channel_token,
            channels::get_agent_toggles,
            channels::set_agent_toggle,
            channels::get_agent_session_persist,
            channels::set_agent_session_persist,
            channels::get_agent_preferences,
            channels::set_agent_feature_model,
            channels::reveal_channels_dir,
            channels::probe_channel,
            channels::scan_cc_switch,
            channels::import_cc_switch,
            commands::open_in_default_app,
            commands::read_local_image,
            metadata::get_all_meta,
            metadata::update_meta,
            metadata::generate_title,
            metadata::generate_tags,
            metadata::generate_summary,
            metadata::generate_permission_hint,
            metadata::set_agent_locale,
            metadata::parse_natural_schedule,
            metadata::translate_settings_fields,
            metadata::extract_settings_defaults,
            agent::test_agent_channel,
            agent::get_agent_logs,
            agent::clear_agent_logs,
            routines::get_routines,
            routines::create_routine,
            routines::update_routine,
            routines::delete_routine,
            routines::get_routine_logs,
            routines::run_routine_now,
            routines::stop_routine,
            routines::get_routine_wake_policy,
            routines::set_routine_wake_policy,
            routines::get_wake_authorization_status,
            routines::enable_wake_active,
            routines::remove_wake_authorization,
            cli_settings::get_settings_schema,
            cli_settings::get_full_cli_settings,
            cli_settings::update_cli_settings,
            cli_settings::refresh_settings_schema,
            cli_settings::get_mcp_status,
            cli_settings::register_mcp,
            cli_settings::unregister_mcp,
            cli_settings::get_claude_binary_info,
            cli_settings::set_claude_binary_path,
            cli_settings::redetect_claude_binary,
            cli_settings::get_claude_root_info,
            cli_settings::set_claude_root,
            translate::translate_locale,
            translate::parse_language_intent,
            translate::list_external_locales,
            translate::delete_external_locale,
            widget::update_widget,
            widget::get_widget_config,
            widget::set_widget_config,
            quota::get_quota,
            quota::refresh_quota,
            quota::quota_available,
            quota::get_tray_title_config,
            quota::set_tray_title_config,
            tray_agent::get_tray_enabled,
            tray_agent::set_tray_enabled,
            menu::hide_main_window,
            menu::quit_app,
            runners::runner_spawn,
            runners::runner_stop,
            runners::runner_restart,
            runners::runner_list,
            runners::runner_tail,
            runners::runner_commands_list,
            runners::runner_command_remove,
            runners::runner_session_stop_all,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // 点 Dock 图标恢复被隐藏/最小化的窗口（Reopen 是 macOS 独有事件）
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app_handle, event);
        });
}

/// 开发时把窗口移到扩展显示器居中
fn move_to_secondary_monitor(app: &tauri::App) {

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let monitors: Vec<_> = window.available_monitors().unwrap_or_default();
    // 找非主显示器
    let secondary = monitors.iter().find(|m| {
        let pos = m.position();
        // 主显示器通常在 (0,0)
        pos.x != 0 || pos.y != 0
    });

    if let Some(monitor) = secondary {
        let pos = monitor.position();
        let size = monitor.size();
        let scale = monitor.scale_factor();

        // 窗口居中于该显示器
        let win_w = 1200.0;
        let win_h = 800.0;
        let x = pos.x as f64 + (size.width as f64 - win_w * scale) / 2.0;
        let y = pos.y as f64 + (size.height as f64 - win_h * scale) / 2.0;

        let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
    }
}
