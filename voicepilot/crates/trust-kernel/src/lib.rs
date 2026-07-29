//! VoicePilot Trust Kernel — the single trust boundary.
//!
//! W1: SQLite persistence, audit logging, task state machine.
//! W2: Policy engine — types, E×D risk matrix, Cedar, Constraint, Egress, Transaction, Action Gateway.
//! Voice / MCP / full ToolResult V2 land in W3+.

pub mod error;
pub mod state;
pub mod db;
pub mod audit;
pub mod repo;
pub mod kernel;
pub mod policy;
pub mod gateway;
pub mod tools;
pub mod compensation;
pub mod toolresult;
pub mod approval;
pub mod allowed_paths;
pub mod mcp;
pub mod skills;
pub mod llm;
// W9 Plan 1: 加密原语模块(stronghold 子模块内部 #[cfg(feature = "stronghold")] 门控)
pub mod crypto;

#[cfg(feature = "voice")]
pub mod voice;

// W7 Plan 4 Task 2: Windows UIA adapter — opt-in via `uia` feature.
// Project is Windows-only (user decision 2026-07-26); the `all(windows, ...)`
// gate is retained as a compile-time guard so a stray `--features uia` on a
// non-Windows target fails fast instead of trying to compile the
// `uiautomation` crate.
#[cfg(all(windows, feature = "uia"))]
pub mod uiautomation;
