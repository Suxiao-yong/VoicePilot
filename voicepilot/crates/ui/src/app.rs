//! Tauri app builder + command 注册。

use crate::error::UiResult;
use crate::state::AppState;

pub fn run(kernel: trust_kernel::kernel::TrustKernel) -> UiResult<()> {
    let state = AppState::new(kernel);
    // W7 Plan 3: 注册 dialog plugin,前端用 `@tauri-apps/plugin-dialog` 的
    // `open()` 选择本地 .md 文件导入为用户自定义 Skill。
    let builder = tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_dialog::init());

    // 注册 global-shortcut plugin（voice feature 才需要 Push-to-talk）。
    #[cfg(feature = "voice")]
    let builder = {
        use tauri::Emitter;
        use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
        let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
        builder.plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcut(shortcut)
                .expect("hardcoded Ctrl+Alt+Space shortcut should always be valid")
                .with_handler(move |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        // W6c P2 #4:emit 失败时记录到 stderr,避免静默吞错。
                        // 不引入 tracing 依赖,保持简单(用户按快捷键无反馈时可在 stderr 排查)。
                        if let Err(e) = app.emit("push-to-talk-start", ()) {
                            eprintln!("[voice] emit push-to-talk-start failed: {}", e);
                        }
                    } else if event.state == ShortcutState::Released {
                        if let Err(e) = app.emit("push-to-talk-stop", ()) {
                            eprintln!("[voice] emit push-to-talk-stop failed: {}", e);
                        }
                    }
                })
                .build(),
        )
    };

    // 根据 voice feature 选择 handler 注册函数。
    // voice feature on 时注册 voice_listen_command,否则只注册基础 commands。
    #[cfg(feature = "voice")]
    let builder = crate::commands::register_handlers_with_voice(builder);
    #[cfg(not(feature = "voice"))]
    let builder = crate::commands::register_handlers(builder);

    builder
        .setup(|_app| {
            // W6b:按需通过 app.get_webview_window("approval") 打开 Approval 窗口
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| crate::error::UiError::Tauri(e.to_string()))?;
    Ok(())
}
