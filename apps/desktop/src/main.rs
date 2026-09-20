//! Tacet 桌面应用入口。
//!
//! ## 启动顺序（每一步都有理由）
//!
//! ```text
//!   ⓪ 初始化日志      —— 必须最先，否则前面的失败没人记得下来
//!   ① 单实例检查      —— 两个实例会重复计时、重复提醒
//!   ② 打开数据库      —— 失败要明确报错，不能带着坏库继续跑
//!   ③ 探测平台能力    —— 缺能力只记日志，不弹窗（原则 7）
//!   ④ 建托盘          —— 菜单栏图标是产品唯一的常驻视觉元素
//!   ⑤ 启动调度线程    —— 从这里开始真正计时
//! ```
//!
//! ## 关于第 ⓪ 步
//!
//! 日志放在最前面是有原因的：这个应用没有主窗口，用户从 Finder 双击启动，
//! stderr 直接掉进虚空。如果日志初始化晚于数据库，那么「数据库打不开」
//! 这个最需要被记录的故障反而写不进日志 —— 正好把最有用的信息漏掉了。
//!
//! ## 为什么没有「主窗口」
//!
//! Tacet 是一个菜单栏应用：它启动后**不显示任何窗口**，
//! 只在菜单栏留一个图标。所有界面都是按需打开的。
//!
//! 这也是为什么这里调了 `app.prevent_exit(true)`？—— 不，没有这回事。
//! Tauri 在窗口全关后会退出，所以我们要阻止它：把 `RunEvent::ExitRequested`
//! 的 `api.prevent_exit()` 用上。否则用户关掉设置窗口，整个应用就退出了。

// Windows 上不要弹控制台窗口（虽然是 macOS 优先，但保持习惯）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use tauri::{Manager, RunEvent, WindowEvent};

use tacet_desktop_lib::commands;
use tacet_desktop_lib::logging;
use tacet_desktop_lib::scheduler;
use tacet_desktop_lib::state::{self as app_state, AppState};
use tacet_desktop_lib::windows;

fn main() {
    // ⓪ 日志必须在所有初始化之前 —— 否则它之前的失败都记不下来。
    logging::init(app_state::local_offset());

    let app = tauri::Builder::default()
        // ------------------------------------------------ 插件
        //
        // 单实例必须放在最前面：它要在其它初始化之前就决定「是不是该退出」。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 用户再次打开应用时，把主面板调出来 —— 这是符合直觉的响应
            //（「我点了图标，应该看到点什么」）。
            let _ = windows::toggle_panel(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // ------------------------------------------------ 初始化
        .setup(|app| {
            logging::info(&format!("启动 Tacet {}", env!("CARGO_PKG_VERSION")));

            // ② 打开数据库 + 建立状态
            let platform = Box::new(platform_macos::MacPlatform::new());
            let state = match AppState::new(platform) {
                Ok(state) => state,
                Err(err) => {
                    // 数据库打不开是**真正的故障**（磁盘满、权限问题），
                    // 这时候唯一正确的做法是明确告诉用户，而不是静默降级 ——
                    // 一个不记录任何数据的健康工具是自相矛盾的。
                    logging::error(&format!("数据库初始化失败：{err}"));
                    return Err(Box::new(err) as Box<dyn std::error::Error>);
                }
            };

            logging::info(&format!("平台：{}", state.platform.name()));

            // ③ 能力探测：缺能力只记日志，绝不弹窗（原则 7 / ADR-011）
            let report = state.platform.capabilities();
            if report.meets_v01_requirements() {
                logging::info("平台能力检查通过");
            } else {
                let missing: Vec<&str> = report
                    .missing_required()
                    .iter()
                    .map(|c| c.display_name())
                    .collect();

                logging::warn(&format!(
                    "缺少 {} 项基础能力：{}。相关功能会自动跳过，不影响使用。",
                    missing.len(),
                    missing.join("、")
                ));
            }
            for (capability, reason) in &report.unavailable {
                logging::warn(&format!(
                    "能力不可用：{} —— {reason}",
                    capability.display_name()
                ));
            }

            let shared: windows::SharedState = Arc::new(Mutex::new(state));
            app.manage(Arc::clone(&shared));

            // ④ 托盘
            build_tray(app.handle())?;

            // ⑤ 调度线程
            scheduler::spawn(app.handle().clone(), shared);

            Ok(())
        })
        // ------------------------------------------------ 命令
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_today_summary,
            commands::get_preferences,
            commands::save_preferences,
            commands::log_water,
            commands::log_activity,
            commands::log_eye_rest,
            commands::start_break,
            commands::capture_intent,
            commands::end_break,
            commands::skip_break,
            commands::snooze_break,
            commands::dismiss_break,
            commands::set_do_not_disturb,
            commands::pause_tracking,
            commands::resume_tracking,
            commands::preview_reminder,
            commands::get_capabilities,
            commands::open_settings_window,
            commands::open_today_window,
            commands::close_current_window,
            commands::resize_panel,
        ])
        // ------------------------------------------------ 窗口事件
        .on_window_event(|window, event| {
            match event {
                // 面板失焦就收起：这是菜单栏应用的肌肉记忆。
                // 但**休息窗口例外** —— 它必须用户显式操作才会关，
                // 否则用户切个应用回来发现提醒没了，会以为提醒没发生。
                WindowEvent::Focused(false) => {
                    if window.label() == "panel" {
                        let _ = window.hide();
                    }
                }

                // 点关闭按钮时隐藏而不是销毁：窗口的创建成本比隐藏高，
                // 而且重新打开时能保留滚动位置等界面状态。
                WindowEvent::CloseRequested { api, .. } => {
                    if matches!(window.label(), "settings" | "today") {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }

                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("Tauri 应用应当能构建");

    // ------------------------------------------------ 事件循环
    app.run(|app_handle, event| match event {
        // 所有窗口都关掉后不要退出 —— 这是一个常驻菜单栏的应用。
        // 用户关掉设置窗口的意思是「我不想看设置了」，不是「我要退出 Tacet」。
        RunEvent::ExitRequested { api, code, .. } => {
            // code 为 None 表示这是「窗口全关了」触发的自动退出请求，
            // 而不是显式的 app.exit()
            if code.is_none() {
                api.prevent_exit();
            } else {
                // 真正要退出了。记一笔，这样日志能分辨「正常退出」与
                // 「进程被杀 / 崩溃」—— 后者在日志里是**开头有、结尾没有**。
                logging::info("退出 Tacet");
            }
        }

        // 系统休眠 / 唤醒 —— 计时准确性的关键（验收标准点名的一条）
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            // 用户点击 Dock 图标（如果有）：把面板调出来
            let _ = windows::toggle_panel(app_handle);
        }

        _ => {}
    });
}

/// 建立菜单栏托盘。
fn build_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    // 托盘的图标：一个内嵌的 PNG —— **乐谱的休止符**。
    //
    // ## 为什么选这个符号
    //
    // "tacet" 是乐谱记号，意思是「此处休止」。整个产品的立意
    //（该安静的时候安静）和这个符号是同一件事，不需要任何解释。
    //
    // 另外它还悄悄呼应了菜单栏本身：菜单栏就是屏幕顶部的一条横线，
    // 而休止符正好由两道横杠构成 —— 图标放进去像是原本就长在那里。
    //
    // ## 为什么必须是单色
    //
    // 为什么用模板图标（`icon_as_template(true)`）：
    // macOS 的菜单栏在深色/浅色模式下有不同的底色，模板图标交给系统去着色，
    // 才能自动适配。自己画一个带颜色的图标会在某种模式下看不清。
    //
    // 同理，源图必须是**纯黑 + 透明背景**（只有一个 RGB 值），
    // 否则系统反色时会得到一团模糊的灰。
    // 图标由 `icons/generate_tray_icons.py` 生成，不要手工替换。
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;

    let open_item = MenuItemBuilder::with_id("open", "打开面板").build(app)?;
    let today_item = MenuItemBuilder::with_id("today", "今天的记录").build(app)?;
    let preview_item = MenuItemBuilder::with_id("preview", "预览休息提醒").build(app)?;
    let settings_item = MenuItemBuilder::with_id("settings", "设置…").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出 Tacet").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .item(&today_item)
        .separator()
        .item(&preview_item)
        .item(&settings_item)
        .separator()
        .item(&quit_item)
        .build()?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Tacet —— 此刻，我知道该我安静了")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                let _ = windows::toggle_panel(app);
            }
            "today" => {
                let _ = windows::open_today(app);
            }
            "preview" => {
                let _ = windows::show_break_window(app);
            }
            "settings" => {
                let _ = windows::open_settings(app);
            }
            "quit" => {
                // 显式退出：这次要真的退出，不能被 ExitRequested 拦下来
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键点击 -> 切换面板显隐
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = windows::toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
