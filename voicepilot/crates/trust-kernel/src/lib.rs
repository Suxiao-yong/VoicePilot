//! VoicePilot Trust Kernel — the single trust boundary.
//!
//! W1: SQLite persistence, audit logging, task state machine.
//! W2: Policy engine skeleton (types, E×D risk matrix, stubs for Cedar/Constraint/Egress/Transaction/Gateway).
//! Voice / MCP / full Action Gateway land in W3+.

pub mod error;
pub mod state;
pub mod db;
pub mod audit;
pub mod repo;
pub mod kernel;
pub mod policy;
pub mod gateway;
