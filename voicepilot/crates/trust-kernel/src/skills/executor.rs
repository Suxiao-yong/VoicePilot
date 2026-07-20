//! files.organize Skill executor — V1.1 §5.2 + §6.2.
//!
//! Orchestrates the V1.1 §6.2 prepare → approve → commit → verify → compensate
//! pipeline for the files.organize Skill (V1.1 §5.2). The executor is UI-agnostic:
//! it calls the `Approver` trait between prepare and commit.
//!
//! Steps:
//!   1. Search sources via filesystem.search_files (V1.1 §6.1).
//!   2. Prepare move via filesystem.prepare_move (V1.1 §6.2 prepare).
//!      Persist prepare_token + preconditions_hash + effect_manifest on the step.
//!   3. Prompt the Approver with the effect_manifest (V1.1 §6.2 approve).
//!      Persist the approval decision. If Deny → cancel without commit.
//!   4. Commit move via filesystem.commit_move (V1.1 §6.2 commit).
//!   5. Verify via filesystem.verify_move (V1.1 §7.1 Strong Verifier).
//!   6. Create CompensationRecord (V1.1 §7.2 strong + auto_reverse ready).
//!      Persist compensation_ref on the step.
//!   7. Return ToolResult V2 (V1.1 §6.3) with status + evidence_strength +
//!      compensation_ref + idempotency_key.

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use crate::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::StepStatus;
use crate::toolresult::{EvidenceStrength, ToolResult, ToolStatus};
use crate::tools::fs_paths::canonicalize;
use chrono::Utc;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FilesOrganizeInput {
    pub task_id: String,
    pub step_id: String,
    pub source: PathBuf,
    pub filter: String,
    pub destination: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SkillExecution {
    pub tool_result: ToolResult,
    pub moved_paths: Vec<(PathBuf, PathBuf)>,
}

pub struct FilesOrganizeSkill;

impl Default for FilesOrganizeSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesOrganizeSkill {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(
        &self,
        kernel: &TrustKernel,
        input: &FilesOrganizeInput,
        approver: &dyn Approver,
    ) -> Result<SkillExecution> {
        let started_at = Utc::now();
        let idempotency_key = format!("idem-{}", uuid::Uuid::new_v4());

        // Step 1: search.
        kernel.update_step_status(&input.step_id, StepStatus::Running)?;
        let found = kernel
            .filesystem()
            .search_files(&input.source, &input.filter)?;
        if found.is_empty() {
            kernel.update_step_status(&input.step_id, StepStatus::Failed)?;
            return Err(KernelError::Skill(format!(
                "no files matching '{}' found under {}",
                input.filter,
                input.source.display()
            )));
        }
        let src_refs: Vec<&std::path::Path> = found.iter().map(|p| p.as_path()).collect();

        // Step 2: prepare.
        let prepared = kernel.filesystem().prepare_move(
            &input.task_id,
            &input.step_id,
            &src_refs,
            &input.destination,
            kernel.transaction_manager(),
        )?;
        let manifest_json = serde_json::to_value(&prepared.manifest)?;
        kernel.update_step_prepare_state(
            &input.step_id,
            &prepared.token.token,
            &prepared.preconditions_hash,
            &manifest_json,
        )?;

        // Step 3: prompt + record approval.
        let decision = approver.prompt(&prepared.manifest);
        let approval_id = format!("appr-{}", uuid::Uuid::new_v4());
        let approval_rec = ApprovalRecord {
            approval_id: approval_id.clone(),
            task_id: input.task_id.clone(),
            step_id: Some(input.step_id.clone()),
            risk_level: "E2".to_string(),
            args_hash: prepared.preconditions_hash.clone(),
            user_decision: decision,
            decided_at: Utc::now().to_rfc3339(),
            e_level: ELevel::E2,
            d_level: DLevel::D2,
            destination: canonicalize(&input.destination.to_string_lossy()),
            egress_approved: false,
            approval_scope: ApprovalScope::Single, // W3b always Single; W7 enables batch.
            policy_bundle_hash: kernel.gateway().bundle_hash().to_string(),
        };
        kernel.record_approval(&approval_rec)?;

        if decision == ApprovalDecision::Deny {
            // User denied — cancel step, return Cancelled ToolResult.
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            let finished_at = Utc::now();
            return Ok(SkillExecution {
                tool_result: ToolResult {
                    status: ToolStatus::Cancelled,
                    data: serde_json::json!({
                        "reason": "user_denied",
                        "approval_id": approval_id,
                    }),
                    evidence_strength: EvidenceStrength::Weak,
                    compensation_ref: None,
                    compensation_level: CompensationLevel::None,
                    preconditions_hash: Some(prepared.preconditions_hash.clone()),
                    idempotency_key,
                    egress_performed: false,
                    data_classification: DLevel::D2,
                    error_code: None,
                    retryable: false,
                    safe_to_retry: true,
                    started_at,
                    finished_at,
                },
                moved_paths: vec![],
            });
        }

        // Step 4: commit.
        let committed = kernel.filesystem().commit_move(
            &prepared.token,
            &prepared.manifest,
            kernel.transaction_manager(),
        )?;

        // Step 5: verify (Strong Verifier).
        let verify_result = kernel.filesystem().verify_move(&prepared.manifest)?;

        // Step 6: create CompensationRecord (strong + auto_reverse ready).
        let comp_id = format!("comp-{}", uuid::Uuid::new_v4());
        let reverse_payload = serde_json::json!({
            "moves": committed.moved_paths.iter().map(|(orig, curr)| {
                serde_json::json!({
                    "from": orig.to_string_lossy().replace('\\', "/"),
                    "to": curr.to_string_lossy().replace('\\', "/"),
                })
            }).collect::<Vec<_>>()
        })
        .to_string();
        let comp_rec = CompensationRecord {
            comp_id: comp_id.clone(),
            step_id: input.step_id.clone(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None,
            ttl_expires: (Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339(),
            status: "active".to_string(),
            snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "filesystem.reverse_move".to_string(),
            reverse_payload,
        };
        kernel.create_compensation(&comp_rec)?;

        // Step 7: persist post-commit state + assemble ToolResult.
        kernel.update_step_post_commit(
            &input.step_id,
            "strong",
            Some(&comp_id),
        )?;
        kernel.update_step_status(&input.step_id, StepStatus::Succeeded)?;
        let finished_at = Utc::now();

        let tool_result = ToolResult {
            status: ToolStatus::Succeeded,
            data: serde_json::json!({
                "moved_count": committed.moved_paths.len(),
                "destination": prepared.manifest.destination,
                "approval_id": approval_id,
            }),
            evidence_strength: verify_result.evidence_strength,
            compensation_ref: Some(comp_id),
            compensation_level: CompensationLevel::Strong,
            preconditions_hash: Some(prepared.preconditions_hash.clone()),
            idempotency_key,
            egress_performed: false,
            data_classification: DLevel::D2,
            error_code: None,
            retryable: false,
            safe_to_retry: false,
            started_at,
            finished_at,
        };

        Ok(SkillExecution {
            tool_result,
            moved_paths: committed.moved_paths,
        })
    }
}
