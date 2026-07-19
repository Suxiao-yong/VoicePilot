//! ToolResult V2 — V1.1 §6.3.
//!
//! Full V2 fields per spec. W3a populates the subset produced by FilesystemTool;
//! other fields (egress_performed, error_code, retryable) are filled by future
//! MCP wrappers and the Verifier pipeline.

use crate::compensation::types::CompensationLevel;
use crate::policy::types::DLevel;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceStrength {
    Strong,
    Medium,
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Succeeded,
    Failed,
    Cancelled,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub data: serde_json::Value,

    // V1.1 transaction + evidence
    pub evidence_strength: EvidenceStrength,
    pub compensation_ref: Option<String>,
    pub compensation_level: CompensationLevel,
    pub preconditions_hash: Option<String>,
    pub idempotency_key: String,

    // V1.1 data classification
    pub egress_performed: bool,
    pub data_classification: DLevel,

    // V1.0 error handling
    pub error_code: Option<String>,
    pub retryable: bool,
    pub safe_to_retry: bool,

    // V1.1 audit timestamps
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}
