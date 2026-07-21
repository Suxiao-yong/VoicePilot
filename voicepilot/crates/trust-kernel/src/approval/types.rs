//! Approval types — V1.1 §8.1 approvals table + §6.2 approve phase.

use crate::policy::types::{DLevel, ELevel};
use serde::{Deserialize, Serialize};

/// User's decision on a prepare→approve→commit prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Allow,
    Deny,
    Modify,
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalDecision::Allow => "allow",
            ApprovalDecision::Deny => "deny",
            ApprovalDecision::Modify => "modify",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "allow" => Some(ApprovalDecision::Allow),
            "deny" => Some(ApprovalDecision::Deny),
            "modify" => Some(ApprovalDecision::Modify),
            _ => None,
        }
    }
}

/// Whether this approval covers a single step or a batch of N steps.
/// V1.1 §8.1: batch requires Skill manifest `approval.mode: batch_once`
/// + same args_hash + same policy_bundle_hash + within max_approval_scope.
///
/// W3b always returns Single; W7 enables batch logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalScope {
    Single,
    Batch,
}

impl ApprovalScope {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalScope::Single => "single",
            ApprovalScope::Batch => "batch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "single" => Some(ApprovalScope::Single),
            "batch" => Some(ApprovalScope::Batch),
            _ => None,
        }
    }
}

/// Persisted approval record. Mirrors V1.1 §8.1 `approvals` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approval_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    /// V1.0 legacy column — kept for backward compat. Typically "E{e_level}".
    pub risk_level: String,
    pub args_hash: String,
    pub user_decision: ApprovalDecision,
    pub decided_at: String, // RFC3339
    pub e_level: ELevel,
    pub d_level: DLevel,
    pub destination: String,
    pub egress_approved: bool,
    pub approval_scope: ApprovalScope,
    pub policy_bundle_hash: String,
}
