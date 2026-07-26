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

#[cfg(feature = "voice")]
pub mod voice;

// W7 Plan 4 Task 2: Windows UIA adapter — opt-in via `uia` feature, Windows-only.
// `cfg(all(windows, feature = "uia"))` (rather than just `feature = "uia"`) so
// that enabling `--all-features` on Linux/macOS doesn't try to compile the
// `uiautomation` crate (which is Windows-only). See Task 1 reviewer note.
#[cfg(all(windows, feature = "uia"))]
pub mod uiautomation;
