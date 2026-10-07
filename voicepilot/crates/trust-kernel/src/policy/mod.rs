//! Policy Engine — V1.1 §4.
//! Layered: hard-deny → Cedar → Rust Constraint → E×D risk → egress → transaction.

pub mod cedar_engine;
pub mod constraint_engine;
pub mod egress;
pub mod risk_matrix;
pub mod transaction;
pub mod types;
// W9 Plan 3: taints 表 CRUD + 值级污点追踪(spec §2.3 + §6.2)。
pub mod taint_repo;
