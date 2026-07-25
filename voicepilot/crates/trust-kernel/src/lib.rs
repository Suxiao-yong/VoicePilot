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
