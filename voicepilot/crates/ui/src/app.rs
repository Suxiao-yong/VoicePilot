//! Tauri app builder + command 注册。

use crate::error::UiResult;
use crate::state::AppState;

pub fn run(kernel: trust_kernel::kernel::TrustKernel) -> UiResult<()> {
    let state = AppState::new(kernel);
    let builder = tauri::Builder::default().manage(state);

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
                        let _ = app.emit("push-to-talk-start", ());
                    } else if event.state == ShortcutState::Released {
                        let _ = app.emit("push-to-talk-stop", ());
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
