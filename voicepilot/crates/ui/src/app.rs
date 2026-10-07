//! Tauri app builder + command 注册。

use crate::error::UiResult;
use crate::state::AppState;

pub fn run(kernel: trust_kernel::kernel::TrustKernel) -> UiResult<()> {
    let state = AppState::new(kernel);
    // Task 0: 启动即从持久化配置重建 LLM client（此前只在设置保存时重建，重启后失效）。
    #[cfg(feature = "llm")]
    crate::settings_commands::startup_rebuild_llm(&state);
    // W7 Plan 3: 注册 dialog plugin,前端用 `@tauri-apps/plugin-dialog` 的
    // `open()` 选择本地 .md 文件导入为用户自定义 Skill。
    let builder = tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_dialog::init());

    // 桌宠化改造:main 关闭 ≠ 退出 —— 阻止关闭并隐藏到后台,进程随 pet 窗口
    // 常驻;唯一退出路径是桌宠右键菜单的 exit_app 命令。
    // (顺带约束了此前偶发的"静默退出"疑虑:生命周期现在只有一条显式出口。)
    let builder = builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if window.label() == "main" {
                api.prevent_close();
                let _ = window.hide();
                eprintln!("[voice] main close requested -> hidden to background");
            }
        } else if window.label() == "main" {
            // 诊断:主窗口尺寸/焦点事件留痕(定位"启动即最小化"怪象)
            if let tauri::WindowEvent::Resized(size) = event {
                eprintln!(
                    "[voice] main resized to {}x{} (minimized stub ≈160x28)",
                    size.width, size.height
                );
            }
        }
    });

    // 注册 global-shortcut plugin（voice feature 才需要 Push-to-talk）。
    #[cfg(feature = "voice")]
    let builder = {
        use tauri::Emitter;
        use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
        // 两个全局热键（Siri 式交互）：
        // - Ctrl+Alt+Space：按住说话 push-to-talk（既有行为，不变）
        // - Ctrl+Alt+V   ：展开/收起悬浮球 toggle（对应 Siri 的 ⌘+⌥ 唤起；避开 Windows 系统 Alt+Space）
        let ptt = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
        let toggle = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV);
        let toggle_id = toggle.id();
        let ptt_id = ptt.id();
        eprintln!(
            "[voice] registering shortcuts ptt_id={} toggle_id={}",
            ptt_id, toggle_id
        );
        // 批1修复:不再 `.expect()`。with_shortcut(s) 的 Result 只是定义解析校验
        // (硬编码合法组合不会失败),真正的 RegisterHotKey 在插件 setup 阶段执行,
        // 冲突时(典型:上一实例未退出仍占用热键)插件只打内部日志、不 panic ——
        // 此前用 expect 反而制造了一条"定义非法即静默退出"的路径。
        // 是否真的注册成功由下方 setup 钩子的 is_registered 诊断输出确认。
        let gs_builder = tauri_plugin_global_shortcut::Builder::new().with_handler(
            move |app, shortcut, event| {
                // 调试：每次热键事件都打日志，定位"V 无反应"是注册失败还是前端监听失败
                eprintln!(
                    "[voice] shortcut event id={} state={:?} is_toggle={} is_ptt={}",
                    shortcut.id(),
                    event.state,
                    shortcut.id() == toggle_id,
                    shortcut.id() == ptt_id
                );
                if shortcut.id() == toggle_id {
                    // 桌宠化改造:Ctrl+Alt+V = 显示/隐藏主界面。
                    // Rust 直控窗口显隐,不再 emit 给前端(悬浮球时代的
                    // toggle-main-window 前端链路已随双窗口架构移除)。
                    if event.state == ShortcutState::Pressed {
                        use tauri::Manager;
                        if let Some(win) = app.get_webview_window("main") {
                            let visible = win.is_visible().unwrap_or(false);
                            if visible {
                                eprintln!("[voice] Ctrl+Alt+V -> hide main");
                                let _ = win.hide();
                            } else {
                                eprintln!("[voice] Ctrl+Alt+V -> show main");
                                let _ = win.unminimize();
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    }
                    return;
                }
                if shortcut.id() == ptt_id {
                    // Ctrl+Alt+Space:按住说话（既有行为）
                    if event.state == ShortcutState::Pressed {
                        if let Err(e) = app.emit("push-to-talk-start", ()) {
                            eprintln!("[voice] emit push-to-talk-start failed: {}", e);
                        }
                    } else if event.state == ShortcutState::Released {
                        if let Err(e) = app.emit("push-to-talk-stop", ()) {
                            eprintln!("[voice] emit push-to-talk-stop failed: {}", e);
                        }
                    }
                    return;
                }
                eprintln!("[voice] unknown shortcut id={}", shortcut.id());
            },
        );
        let gs_builder = match gs_builder
            .with_shortcut(ptt)
            .and_then(|b| b.with_shortcut(toggle))
        {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[voice] shortcut definition rejected: {}", e);
                tauri_plugin_global_shortcut::Builder::new()
            }
        };
        builder.plugin(gs_builder.build())
    };

    // 根据 voice feature 选择 handler 注册函数。
    // voice feature on 时注册 voice_listen_command,否则只注册基础 commands。
    #[cfg(feature = "voice")]
    let builder = crate::commands::register_handlers_with_voice(builder);
    #[cfg(not(feature = "voice"))]
    let builder = crate::commands::register_handlers(builder);

    builder
        .setup(|app| {
            // 桌宠化改造:启动鼠标穿透看门狗(pet 透明区放行桌面点击,
            // 光标进入兽身/气泡热区时才恢复交互)
            crate::pet_commands::spawn_pet_cursor_watchdog(app.handle().clone());

            // Phase C: 启动进程内后台作业调度器。完成经 Tauri 事件
            // "agent-job-done" 投递（现有通知通道，R4 核 5）；历史落
            // agent_job_runs 表（job.history 可溯）。
            {
                use tauri::{Emitter, Manager};
                let state: tauri::State<AppState> = app.state();
                let kernel = state.kernel.clone_arc();
                let handle = app.handle().clone();
                let notify: trust_kernel::scheduler::NotifyFn =
                    std::sync::Arc::new(move |job, run| {
                        let payload = serde_json::json!({
                            "job_id": job.job_id,
                            "prompt": job.prompt,
                            "outcome": run.outcome,
                            "result": run.result,
                            "finished_at_ms": run.finished_at_ms,
                        });
                        if let Err(e) = handle.emit("agent-job-done", payload) {
                            eprintln!("[scheduler] emit agent-job-done failed: {}", e);
                        }
                    });
                let _ = trust_kernel::scheduler::spawn_scheduler(kernel, notify);
                eprintln!(
                    "[scheduler] background scheduler started (tick={}s)",
                    trust_kernel::scheduler::TICK_SECS
                );
            }

            // 系统托盘(用户反馈:关掉主窗口后后台不可见、无图标)。
            // 托盘 = 生命周期第二入口:打开主界面 / 退出程序。
            {
                use tauri::{
                    Manager,
                    menu::{Menu, MenuItem},
                    tray::TrayIconBuilder,
                };
                let open = MenuItem::with_id(app, "open-main", "打开主界面", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "exit-app", "退出程序", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &quit])?;
                let mut tray = TrayIconBuilder::with_id("voicepilot-tray")
                    .menu(&menu)
                    .show_menu_on_left_click(true)
                    .tooltip("VoicePilot —— 后台运行中");
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                tray.on_menu_event(|app, event| match event.id.as_ref() {
                    "open-main" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.unminimize();
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "exit-app" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
                eprintln!("[voice] tray icon created (open-main / exit-app)");
            }
            // 批1诊断:启动即打印主窗口物理尺寸 / DPI 缩放 / 热键注册状态,
            // 定位「窗口 135×93 尺寸异常」与「Ctrl+Alt+V 无反应」。
            #[cfg(feature = "voice")]
            {
                use tauri::Manager;
                use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
                if let Some(win) = app.get_webview_window("main") {
                    let size = win
                        .outer_size()
                        .map(|s| format!("{}x{}", s.width, s.height))
                        .unwrap_or_else(|e| format!("query failed: {}", e));
                    let scale = win.scale_factor().unwrap_or(f64::NAN);
                    eprintln!(
                        "[voice] main window outer_size={} scale_factor={}",
                        size, scale
                    );
                } else {
                    eprintln!("[voice] main window not found at setup");
                }
                let ptt_again =
                    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
                let toggle_again =
                    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV);
                eprintln!(
                    "[voice] hotkey registered check: ptt={} toggle={}",
                    app.global_shortcut().is_registered(ptt_again),
                    app.global_shortcut().is_registered(toggle_again)
                );
            }
            {
                // 桌宠化诊断:确认 pet 窗口已创建 + 看门狗已接管穿透
                use tauri::Manager;
                if let Some(pet) = app.get_webview_window("pet") {
                    // 桌宠出生位:主屏工作区右下角(默认级联位置会被居中的
                    // 主窗口盖住,用户以为桌宠没渲染)
                    if let Ok(Some(monitor)) = app.primary_monitor() {
                        let sz = monitor.size();
                        let pos = monitor.position();
                        let pet_sz = pet
                            .outer_size()
                            .unwrap_or(tauri::PhysicalSize::new(450, 540));
                        let x = pos.x + sz.width as i32 - pet_sz.width as i32 - 24;
                        let y = pos.y + sz.height as i32 - pet_sz.height as i32 - 48;
                        let _ = pet.set_position(tauri::PhysicalPosition::new(x, y));
                        eprintln!("[voice] pet moved to bottom-right ({}, {})", x, y);
                    }
                    eprintln!("[voice] pet window ready (cursor watchdog active)");
                } else {
                    eprintln!("[voice] pet window NOT found at setup");
                }
            }
            let _ = app; // voice off 时消除 unused 警告
            // W6b:按需通过 app.get_webview_window("approval") 打开 Approval 窗口
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| crate::error::UiError::Tauri(e.to_string()))?;
    Ok(())
}
