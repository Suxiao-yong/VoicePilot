//! VoicePilot Trust Kernel — the single trust boundary.
//!
//! W1 scope: SQLite persistence, audit logging, task state machine.
//! Voice / MCP / Policy / Action Gateway land in W2+.

pub mod error;
pub mod state;
pub mod db;
pub mod audit;
pub mod repo;
pub mod kernel;
pub mod policy;
pub mod gateway;
