use std::sync::{Arc, Mutex};

use app_lib::quota::{self, QuotaInfo};

fn main() {
    #[cfg(target_os = "macos")]
    macos_main();

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("monet-tray is macOS-only");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
const TAB_STOP_PT: f64 = 220.0;

#[cfg(target_os = "macos")]
fn macos_main() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().expect("must run on main thread");
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // 原生 NSStatusItem + statusItem.menu：点击与菜单弹出全权交给 AppKit
    // （与主流菜单栏应用同路径）。此前经 tray-icon 的自定义 view 拦截鼠标事件
    // 再 performClick 编程弹出，属模仿 AppKit 私有行为的非标准路径，
    // 在新系统上偶发菜单错位弹到屏幕左上角
    let status_item = create_status_item(mtm);
    // 菜单对象常驻：NSMenu 只创建并挂载一次，此后每次数据刷新原位清空重填内容。
    // 此前每次刷新都新建 NSMenu 再 setMenu 替换，属非常规用法——主流菜单栏应用的
    // 菜单对象终身不变；新系统上原生路径仍偶发弹出错位（菜单弹到屏幕左上角），
    // 收敛掉「statusItem 与菜单的关联被高频重建」这一残余变量
    let menu = muda::Menu::new();
    fill_menu(&menu, None);
    attach_menu(&status_item, &menu);
    set_tooltip(&status_item, "Monet");

    let pending: Arc<Mutex<Option<QuotaInfo>>> = Arc::new(Mutex::new(None));
    let fetching = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut state = TrayState {
        // 计时一律用墙钟毫秒：Instant 在 macOS 睡眠期间暂停，合盖过夜后
        // elapsed 仍是睡前值，周期刷新会白等一整个间隔
        last_refresh_ms: now_ms() - REFRESH_INTERVAL_MS,
        last_render_ms: now_ms(),
        title_config_mtime: tray_title_mtime(),
        cache_mtime: quota_cache_mtime(),
        last_info: None,
        in_backoff: quota::backoff_remaining_secs().is_some(),
    };

    // 冷启动先用磁盘缓存渲染一帧（含数据年龄行），不等首次 fetch
    if let Some(info) = quota::peek_cached_quota() {
        apply_to_tray(&status_item, &info, &menu);
        state.last_info = Some(info);
    }

    // 周期逻辑挂 NSTimer（default mode），事件循环交给标准 NSApp.run()。
    // 此前裸 NSRunLoop 每秒轮询（无 NSApp.run）是旧 tray-icon 时代继承的非标准结构，
    // AppKit 的菜单定位与事件路由假设标准事件循环——与主流菜单栏应用彻底同构。
    // 菜单打开期间 runloop 在 tracking mode，default-mode timer 不触发，
    // 重填不会撞上正在显示的菜单
    let state = std::cell::RefCell::new(state);
    let tick = block2::RcBlock::new(move |_timer: std::ptr::NonNull<objc2_foundation::NSTimer>| {
        tray_tick(&status_item, &menu, &mut state.borrow_mut(), &pending, &fetching);
    });
    let _timer = unsafe {
        objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0, true, &tick)
    };
    ns_app.run();
}

#[cfg(target_os = "macos")]
struct TrayState {
    last_refresh_ms: i64,
    last_render_ms: i64,
    title_config_mtime: Option<std::time::SystemTime>,
    cache_mtime: Option<std::time::SystemTime>,
    /// 进程内记住最新一帧数据（含 error 标注），周期重渲染倒计时/数据年龄用
    last_info: Option<QuotaInfo>,
    in_backoff: bool,
}

/// 每秒 tick：消费菜单事件与后台刷新结果、侦测配置/缓存变化、周期重渲染与周期刷新
#[cfg(target_os = "macos")]
fn tray_tick(
    status_item: &objc2_app_kit::NSStatusItem,
    menu: &muda::Menu,
    st: &mut TrayState,
    pending: &Arc<Mutex<Option<QuotaInfo>>>,
    fetching: &Arc<std::sync::atomic::AtomicBool>,
) {
    while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
        match event.id.0.as_str() {
            "show" => open_main_app(),
            "refresh" => {
                // 手动刷新：强制打 API（跳过磁盘 TTL；限流冷却期内 lib 层会拦截）
                request_refresh(pending, fetching, true);
                st.last_refresh_ms = now_ms();
            }
            "quit" => {
                unregister_and_exit();
            }
            _ => {}
        }
    }

    // 主线程消费后台线程的 quota 结果。
    // 注意不在此处重采 cache_mtime：fetch 在途期间（最长 15s 网络超时）
    // 主应用可能写盘，现采会把那次写入标记为已消费、吞掉下方 mtime 分支
    // 用新数据清 error 的机会；tray 自己写盘引发的 mtime 分支多渲染一帧无害
    if let Ok(mut guard) = pending.try_lock() {
        if let Some(info) = guard.take() {
            apply_to_tray(status_item, &info, menu);
            st.last_info = Some(info);
            st.last_render_ms = now_ms();
        }
    }

    // 设置页改了菜单栏标题配置 → mtime 变化 → 用现有数据即时重渲染（不打 API）
    let mtime = tray_title_mtime();
    if mtime != st.title_config_mtime {
        st.title_config_mtime = mtime;
        if let Some(info) = st.last_info.clone().or_else(quota::peek_cached_quota) {
            apply_to_tray(status_item, &info, menu);
            st.last_render_ms = now_ms();
        }
    }

    // 磁盘缓存更新（主应用或 tray 自己刷新成功）→ mtime 变化 →
    // peek 按时间戳取内存/磁盘新者 → 采用新数据（同时清掉旧 error 标注）
    let cm = quota_cache_mtime();
    if cm != st.cache_mtime {
        st.cache_mtime = cm;
        if let Some(info) = quota::peek_cached_quota() {
            apply_to_tray(status_item, &info, menu);
            st.last_info = Some(info);
            st.last_render_ms = now_ms();
        }
    }

    // 周期重渲染：重置倒计时、数据年龄、限流剩余时间都是现算的，
    // 不重建菜单就会停在上一帧（曾因此显示 fetch 时刻算死的倒计时）
    if now_ms() - st.last_render_ms >= 30_000 {
        if let Some(info) = &st.last_info {
            apply_to_tray(status_item, info, menu);
        }
        st.last_render_ms = now_ms();
    }

    // 限流冷却结束的下降沿：立即补一次刷新，不等下个周期（最多 5 分钟）。
    // 边沿触发只发生一次，非 429 失败不会走到这里（不写 backoff），无风暴风险
    let backoff_now = quota::backoff_remaining_secs().is_some();
    if st.in_backoff && !backoff_now {
        if quota::quota_available() {
            request_refresh(pending, fetching, false);
        }
        st.last_refresh_ms = now_ms();
    }
    st.in_backoff = backoff_now;

    if now_ms() - st.last_refresh_ms >= REFRESH_INTERVAL_MS {
        if quota::quota_available() {
            // 周期刷新：共享磁盘缓存 TTL（主应用可能刚刷过，省 API 调用）
            request_refresh(pending, fetching, false);
        }
        st.last_refresh_ms = now_ms();
    }
}

/// 与 quota::CACHE_TTL 对齐：usage API 限流预算有限，120s 节奏曾把限流打爆
#[cfg(target_os = "macos")]
const REFRESH_INTERVAL_MS: i64 = 300_000;

#[cfg(target_os = "macos")]
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn tray_title_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(app_lib::config::data_dir().join("tray-title.json"))
        .ok()
        .and_then(|m| m.modified().ok())
}

fn quota_cache_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(app_lib::config::data_dir().join("quota-cache.json"))
        .ok()
        .and_then(|m| m.modified().ok())
}

/// 发起后台 quota 刷新（不阻塞主线程）。force=true 跳过磁盘缓存 TTL。
fn request_refresh(
    pending: &Arc<Mutex<Option<QuotaInfo>>>,
    fetching: &Arc<std::sync::atomic::AtomicBool>,
    force: bool,
) {
    if fetching.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let pending = Arc::clone(pending);
    let fetching = Arc::clone(fetching);
    std::thread::spawn(move || {
        let info = if force { quota::refresh_quota() } else { quota::get_quota() };
        if let Ok(mut guard) = pending.lock() {
            *guard = Some(info);
        }
        fetching.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/// 创建原生 NSStatusItem：template 图标（随亮暗菜单栏自动配色）+ 变长宽度
#[cfg(target_os = "macos")]
fn create_status_item(
    mtm: objc2_foundation::MainThreadMarker,
) -> objc2::rc::Retained<objc2_app_kit::NSStatusItem> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSCellImagePosition, NSImage, NSStatusBar, NSVariableStatusItemLength};
    use objc2_foundation::{NSData, NSSize};

    let bar = NSStatusBar::systemStatusBar();
    let item = bar.statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        let data = NSData::with_bytes(include_bytes!("../../icons/tray-template.png"));
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
            let size = image.size();
            if size.height > 0.0 {
                // 菜单栏标准图标高 18pt，等比缩放
                let h = 18.0;
                image.setSize(NSSize::new(size.width * h / size.height, h));
            }
            image.setTemplate(true);
            button.setImage(Some(&image));
            // 额度标题显示在图标右侧
            button.setImagePosition(NSCellImagePosition::ImageLeft);
        }
    }
    item
}

/// 把常驻菜单挂载到 status item（仅启动时调用一次，此后关联终身不变）
#[cfg(target_os = "macos")]
fn attach_menu(status_item: &objc2_app_kit::NSStatusItem, menu: &muda::Menu) {
    use muda::ContextMenu;
    unsafe {
        let ns_menu = menu.ns_menu().cast::<objc2_app_kit::NSMenu>();
        status_item.setMenu(ns_menu.as_ref());
    }
}

#[cfg(target_os = "macos")]
fn set_tooltip(status_item: &objc2_app_kit::NSStatusItem, tooltip: &str) {
    use objc2_foundation::{MainThreadMarker, NSString};
    let Some(mtm) = MainThreadMarker::new() else { return };
    if let Some(button) = status_item.button(mtm) {
        button.setToolTip(Some(&NSString::from_str(tooltip)));
    }
}

#[cfg(target_os = "macos")]
fn set_button_title(status_item: &objc2_app_kit::NSStatusItem, title: &str) {
    use objc2_foundation::{MainThreadMarker, NSString};
    let Some(mtm) = MainThreadMarker::new() else { return };
    if let Some(button) = status_item.button(mtm) {
        // 空串真正清空标题（此前 tray-icon 的 set_title(None) 清不掉，一并修复）
        button.setTitle(&NSString::from_str(title));
    }
}

/// 主线程上应用 quota 数据到 tray（原位重填菜单 + patch 样式 + tooltip + 标题）
#[cfg(target_os = "macos")]
fn apply_to_tray(
    status_item: &objc2_app_kit::NSStatusItem,
    info: &QuotaInfo,
    menu: &muda::Menu,
) {
    fill_menu(menu, Some(info));
    patch_menu_styles(menu);
    set_tooltip(status_item, &quota::format_tray_tooltip(info));
    set_button_title(
        status_item,
        quota::format_tray_title(info).as_deref().unwrap_or(""),
    );
}

/// 原位重填常驻菜单：先清空再按当前数据追加，不替换 NSMenu 对象本身
fn fill_menu(menu: &muda::Menu, info: Option<&QuotaInfo>) {
    use muda::{MenuItem, PredefinedMenuItem};

    while menu.remove_at(0).is_some() {}
    let zh = is_chinese();

    if let Some(qi) = info {
        // 刷新失败时旧数据仍有参考价值：照常展示额度块，
        // 由下方状态行（限流/失败 + 数据年龄）说明它有多旧
        let has_data = qi.session.is_some() || qi.weekly.is_some() || !qi.weekly_models.is_empty();
        if has_data {
            let plan = qi.plan.as_deref().unwrap_or("");
            let title = if plan.is_empty() {
                "Claude Code".to_string()
            } else {
                format!("Claude Code · {plan}")
            };
            let _ = menu.append(&MenuItem::with_id("title", title, true, None::<muda::accelerator::Accelerator>));
            let _ = menu.append(&PredefinedMenuItem::separator());

            if let Some(s) = &qi.session {
                let label = format_quota_line(
                    if zh { "本轮" } else { "Session" },
                    s.used_percent,
                    // 倒计时从绝对重置时刻现算——缓存里的 resets_in_secs 是
                    // fetch 时刻的快照，会随缓存年龄漂移
                    quota::secs_until(s.resets_at.as_deref()),
                    zh,
                );
                let _ = menu.append(&MenuItem::with_id("s", label, true, None::<muda::accelerator::Accelerator>));
            }
            if let Some(w) = &qi.weekly {
                let label = format_quota_line(
                    if zh { "每周" } else { "Weekly" },
                    w.used_percent,
                    quota::secs_until(w.resets_at.as_deref()),
                    zh,
                );
                let _ = menu.append(&MenuItem::with_id("w", label, true, None::<muda::accelerator::Accelerator>));
            }
            for (i, m) in qi.weekly_models.iter().enumerate() {
                let name = m.display_name.as_deref().unwrap_or(&m.model);
                let label = format!("{name}  {:.0}%", m.used_percent);
                let _ = menu.append(&MenuItem::with_id(
                    format!("m{i}"),
                    label,
                    true,
                    None::<muda::accelerator::Accelerator>,
                ));
            }
        }

        // 状态行：限流 > 一般失败，加数据年龄；disabled 灰字
        if let Some(remain) = quota::backoff_remaining_secs() {
            let mins = (remain + 59) / 60;
            let label = if zh {
                format!("接口限流中 · {mins} 分钟后自动恢复")
            } else {
                format!("Rate limited · resumes in {mins}m")
            };
            let _ = menu.append(&MenuItem::with_id("status", label, false, None::<muda::accelerator::Accelerator>));
        } else if qi.error.is_some() {
            // 按错误分类给行动引导：过期点刷新即触发委托续期（quota.rs 手动路径），
            // 笼统的「刷新失败」只留给无法归类的残余
            let label = match qi.error_kind.as_deref() {
                Some("token_expired") => {
                    if zh { "凭据已过期 · 点击下方刷新恢复" } else { "Credentials expired · click Refresh below" }
                }
                Some("no_credentials") => {
                    if zh { "未检测到 Claude 登录凭据" } else { "No Claude credentials found" }
                }
                Some("network") => {
                    if zh { "网络错误 · 将自动重试" } else { "Network error · will retry" }
                }
                _ => {
                    if zh { "刷新失败" } else { "Refresh failed" }
                }
            };
            let _ = menu.append(&MenuItem::with_id("status", label, false, None::<muda::accelerator::Accelerator>));
        }
        if has_data {
            if let Some(age) = format_age(&qi.updated_at, zh) {
                let _ = menu.append(&MenuItem::with_id("age", age, false, None::<muda::accelerator::Accelerator>));
            }
            let _ = menu.append(&PredefinedMenuItem::separator());
        } else if qi.error.is_some() {
            let _ = menu.append(&PredefinedMenuItem::separator());
        }
    }

    let show_label = if zh { "打开 Monet" } else { "Open Monet" };
    let refresh_label = if zh { "刷新额度" } else { "Refresh Quota" };
    let quit_label = if zh { "退出菜单栏" } else { "Quit Menu Bar" };

    let _ = menu.append(&MenuItem::with_id("show", show_label, true, None::<muda::accelerator::Accelerator>));
    let _ = menu.append(&MenuItem::with_id("refresh", refresh_label, true, None::<muda::accelerator::Accelerator>));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", quit_label, true, None::<muda::accelerator::Accelerator>));
}

fn open_main_app() {
    // MonetTray.app 是独立 Helper App，位于 Monet.app/Contents/Library/LoginItems/ 下。
    // 当前二进制: MonetTray.app/Contents/MacOS/monet-tray
    // 主应用:     Monet.app（上 7 级）
    let app_path = std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent() // MacOS/
                .and_then(|d| d.parent()) // Contents/
                .and_then(|d| d.parent()) // MonetTray.app/
                .and_then(|d| d.parent()) // LoginItems/
                .and_then(|d| d.parent()) // Library/
                .and_then(|d| d.parent()) // Contents/
                .and_then(|d| d.parent()) // Monet.app
                .map(|app| app.to_path_buf())
        })
        // 防御：dev 直跑/旧布局下回溯结果不是 .app，避免 open 打开错误目录
        .filter(|p| p.extension().is_some_and(|e| e == "app"));
    if let Some(path) = app_path {
        let _ = std::process::Command::new("open").arg(path).spawn();
    } else {
        // 非标准布局回退 bundle id（Helper 与主应用 id 不同，无歧义）
        let _ = std::process::Command::new("open")
            .args(["-b", "io.github.zenolab124.monet"])
            .spawn();
    }
}

fn unregister_and_exit() {
    // 持久化「用户主动退出」意图：主应用下次启动看到标记就不再自动拉起。
    // 路径约定与 tray_agent::disabled_marker_path 一致。
    let marker = app_lib::config::data_dir().join("tray-disabled");
    let _ = std::fs::write(&marker, "");
    // KeepAlive > SuccessfulExit: false 保证 exit(0) 不触发 launchd 重启
    std::process::exit(0);
}

/// 系统语言进程生命周期内不变，缓存避免每次建菜单都 spawn `defaults` 子进程
fn is_chinese() -> bool {
    static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHED.get_or_init(detect_chinese)
}

fn detect_chinese() -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "NSGlobalDomain", "AppleLanguages"])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = s.lines().find(|l| l.contains('"')) {
                return first.contains("zh");
            }
        }
    }
    for key in ["LANG", "LC_ALL", "LC_MESSAGES"] {
        if let Ok(val) = std::env::var(key) {
            if val.starts_with("zh") {
                return true;
            }
        }
    }
    false
}

fn format_quota_line(label: &str, used: f64, resets: Option<i64>, zh: bool) -> String {
    let left = format!("{label}  {used:.0}%");
    let right = format_reset(resets, zh);
    if right.is_empty() {
        left
    } else {
        format!("{left}\t{right}")
    }
}

/// 数据年龄行：「更新于 X 前」，基于上次成功 fetch 的时间戳现算。
/// 刷新失败静默回退旧缓存时，这行是用户唯一能察觉数据陈旧的途径。
fn format_age(updated_at: &str, zh: bool) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(updated_at).ok()?;
    let secs = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    Some(if secs < 60 {
        if zh { "刚刚更新".into() } else { "Updated just now".into() }
    } else if secs < 3600 {
        let m = secs / 60;
        if zh { format!("更新于 {m} 分钟前") } else { format!("Updated {m}m ago") }
    } else if secs < 86_400 {
        let h = secs / 3600;
        if zh { format!("更新于 {h} 小时前") } else { format!("Updated {h}h ago") }
    } else {
        let d = secs / 86_400;
        if zh { format!("更新于 {d} 天前") } else { format!("Updated {d}d ago") }
    })
}

fn format_reset(secs: Option<i64>, zh: bool) -> String {
    match secs {
        Some(s) if s > 0 => {
            let h = s / 3600;
            let m = (s % 3600) / 60;
            if zh {
                if h > 24 {
                    format!("{}天{}小时后重置", h / 24, h % 24)
                } else if h > 0 {
                    format!("{h}小时{m}分后重置")
                } else {
                    format!("{m}分后重置")
                }
            } else if h > 24 {
                format!("resets in {}d {}h", h / 24, h % 24)
            } else if h > 0 {
                format!("resets in {h}h {m}m")
            } else {
                format!("resets in {m}m")
            }
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// macOS: attributed string styling (bold title, tab-stop aligned reset times)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn patch_menu_styles(menu: &muda::Menu) {
    use muda::ContextMenu;
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_app_kit::{
        NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
        NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSTextAlignment, NSTextTab,
    };
    use objc2_core_foundation::CGFloat;
    use objc2_foundation::{
        MainThreadMarker, NSArray, NSDictionary, NSMutableAttributedString, NSRange, NSString,
    };

    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };

    let ns_menu_ptr = menu.ns_menu();
    let ns_menu: &objc2_app_kit::NSMenu =
        unsafe { &*(ns_menu_ptr as *const objc2_app_kit::NSMenu) };

    let para = NSMutableParagraphStyle::new();
    let empty_tabs = NSArray::<NSTextTab>::new();
    para.setTabStops(Some(&empty_tabs));
    let tab = unsafe {
        NSTextTab::initWithTextAlignment_location_options(
            NSTextTab::alloc(),
            NSTextAlignment::Right,
            TAB_STOP_PT as CGFloat,
            &NSDictionary::<NSString, AnyObject>::new(),
        )
    };
    para.addTabStop(&tab);

    let menu_font = NSFont::menuFontOfSize(0.0);
    let bold_font = NSFont::boldSystemFontOfSize(0.0);
    let gray = NSColor::secondaryLabelColor();

    let count = ns_menu.numberOfItems();
    for i in 0..count {
        let Some(item) = ns_menu.itemAtIndex(i) else {
            continue;
        };
        if item.isSeparatorItem() {
            continue;
        }

        let title_rs = item.title().to_string();

        if title_rs.starts_with("Claude Code") {
            let ns_str = NSString::from_str(&title_rs);
            let attr = NSMutableAttributedString::initWithString(
                NSMutableAttributedString::alloc(),
                &ns_str,
            );
            let full = NSRange {
                location: 0,
                length: ns_str.length(),
            };
            let bold_obj: &AnyObject = &bold_font;
            unsafe {
                attr.addAttribute_value_range(NSFontAttributeName, bold_obj, full);
            }
            item.setAttributedTitle(Some(&attr));
            continue;
        }

        if title_rs.contains('\t') {
            let ns_str = NSString::from_str(&title_rs);
            let attr = NSMutableAttributedString::initWithString(
                NSMutableAttributedString::alloc(),
                &ns_str,
            );
            let full = NSRange {
                location: 0,
                length: ns_str.length(),
            };

            let font_obj: &AnyObject = &menu_font;
            let para_obj: &AnyObject = &para;
            unsafe {
                attr.addAttribute_value_range(NSFontAttributeName, font_obj, full);
                attr.addAttribute_value_range(NSParagraphStyleAttributeName, para_obj, full);
            }

            if let Some(tab_byte) = title_rs.find('\t') {
                let tab_utf16 = title_rs[..tab_byte].encode_utf16().count() + 1;
                let rest_len = ns_str.length() - tab_utf16;
                if rest_len > 0 {
                    let rest_range = NSRange {
                        location: tab_utf16,
                        length: rest_len,
                    };
                    let gray_obj: &AnyObject = &gray;
                    unsafe {
                        attr.addAttribute_value_range(
                            NSForegroundColorAttributeName,
                            gray_obj,
                            rest_range,
                        );
                    }
                }
            }

            item.setAttributedTitle(Some(&attr));
            continue;
        }

        if title_rs.contains('%') {
            let ns_str = NSString::from_str(&title_rs);
            let attr = NSMutableAttributedString::initWithString(
                NSMutableAttributedString::alloc(),
                &ns_str,
            );
            let full = NSRange {
                location: 0,
                length: ns_str.length(),
            };
            let font_obj: &AnyObject = &menu_font;
            unsafe {
                attr.addAttribute_value_range(NSFontAttributeName, font_obj, full);
            }
            item.setAttributedTitle(Some(&attr));
        }
    }
}
