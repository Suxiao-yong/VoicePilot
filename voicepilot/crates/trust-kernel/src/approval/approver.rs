//! Approver trait — V1.1 §6.2 approve phase.
//!
//! UI-agnostic: CLI implements it with stdin; Tauri implements it with
//! an IPC call to the approval window. The Skill executor calls
//! `approver.prompt(manifest)` between prepare and commit.

use crate::approval::types::ApprovalDecision;
use crate::policy::transaction::EffectManifest;

/// Callback the Skill executor invokes between prepare and commit.
pub trait Approver: Send + Sync {
    /// Show the effect_manifest to the user and return their decision.
    /// May block (CLI stdin) or return immediately (auto-approve / auto-deny).
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision;
}

/// Auto-approver for tests and headless runs. Always returns Allow.
pub struct AutoApprover;

impl Approver for AutoApprover {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Allow
    }
}

/// Auto-denier for negative-path tests.
pub struct AutoDenier;

impl Approver for AutoDenier {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Deny
    }
}
