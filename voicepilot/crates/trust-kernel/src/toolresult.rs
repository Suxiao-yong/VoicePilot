//! ToolResult V2 — V1.1 §6.3, §7.1.
//!
//! W3a implements the subset needed for filesystem verification:
//!   - EvidenceStrength enum
//!   - ToolResult struct (status + data + evidence + compensation_ref)
//! Full V2 fields (egress_performed, idempotency_key, retryable) land in W3b.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceStrength {
    /// System-level state directly read (filesystem sha256+size match, PID exists).
    Strong,
    /// Structured but indirect (UIA tree contains expected node, accessibility snapshot).
    Medium,
    /// Tool's natural-language "succeeded" with no system-level check.
    Weak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub data: serde_json::Value,
    pub evidence_strength: EvidenceStrength,
    pub compensation_ref: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Succeeded,
    Failed,
    Cancelled,
    Partial,
}
