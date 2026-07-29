//! W9 Plan 1: 加密原语模块。
//!
//! 仅当 `stronghold` feature 启用时,`stronghold` 子模块可见。
//! `stronghold` 子模块封装 `tauri-plugin-stronghold`,提供 `StrongholdVault`
//! 抽象,管理 reverse_payload 加密 / 解密 / 密钥派生 / 降级模式(spec §2.1)。

#[cfg(feature = "stronghold")]
pub mod stronghold;
