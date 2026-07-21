//! VoicePilot UI crate —— Tauri 2 桌面应用。
//!
//! W6a 范围:
//! - Tauri command 桥接到 `trust-kernel`
//! - `TauriApprover` 实现(基于 IPC 的 Approver)
//! - Approval 窗口(React + TypeScript)
//! - 端到端冒烟测试
//!
//! Feature 门控:`default = []` 保持 crate 纯 Rust(无 Tauri 也能编译)。
//! `tauri` feature 开启桌面 app 二进制 + voice feature。

pub mod error;
pub mod state;

#[cfg(feature = "tauri")]
pub mod approver;

#[cfg(feature = "tauri")]
pub mod commands;

#[cfg(feature = "tauri")]
pub mod settings_commands;

#[cfg(feature = "tauri")]
pub mod audit_commands;

#[cfg(feature = "tauri")]
pub mod trust_center_commands;

#[cfg(feature = "tauri")]
pub mod skills_commands;

#[cfg(feature = "tauri")]
pub mod app;

#[cfg(feature = "voice")]
pub mod voice_commands;

pub use error::UiError;
pub use state::AppState;
