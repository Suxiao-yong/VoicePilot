//! Tauri app builder + command 注册。

use crate::error::UiResult;
use crate::state::AppState;

pub fn run(kernel: trust_kernel::kernel::TrustKernel) -> UiResult<()> {
    let state = AppState::new(kernel);
    let builder = tauri::Builder::default().manage(state);

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
