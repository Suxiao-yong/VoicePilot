//! task.compensate Skill executor — W7 Plan 2 Task 5.
//!
//! Reverses a previous operation by executing `auto_reverse` on the
//! compensation record for the given `target_step_id`. Per spec §2.4:
//! risk E2 (write — modifies filesystem), approval PerStep,
//! tools=`compensation.auto_reverse`.
//!
//! This is the first Skill in Plan 2 that exercises the full approval
//! flow (E2 + PerStep) and actually mutates state.
//!
//! Pipeline:
//!   1. Validate `target_step_id` input.
//!   2. Create new task + step.
//!   3. Look up the active CompensationRecord for `target_step_id`.
//!   4. Build an EffectManifest describing the reverse move (for the
//!      approval prompt).
//!   5. Update step → Running.
//!   6. Record approval decision (E2 + PerStep). Branch on Allow/Deny/Modify.
//!   7. Execute `auto_reverse_move` (the "commit" phase).
//!   8. Mark the compensation record status as "reversed".
//!   9. Finalize step as Succeeded with Strong evidence.
//!
//! Reversing a reverse is out of scope — `auto_reverse_move` is idempotent
//! enough; if it fails, files are rolled back by `auto_reverse_move` itself.

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::compensation::executor::auto_reverse_move;
use crate::compensation::types::CompensationRecord;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    finalize_step_success, record_approval_decision, validate_input_against_manifest,
    ApprovalContext,
};
use crate::skills::manifest::task_compensate_manifest;
use crate::skills::verifiers::{verify_task_compensate, VerificationContext, VerificationOutcome};
use crate::tools::fs_paths::canonicalize;
use crate::tools::fs_snapshot::snapshot_file;
// W9 Plan 2 Task 4: 解密 snapshot_encrypted 需要 EncryptedPayload 类型。
#[cfg(feature = "stronghold")]
use crate::crypto::stronghold::EncryptedPayload;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

/// Input for the `task.compensate` Skill executor.
#[derive(Debug, Clone)]
pub struct TaskCompensateInput {
    /// New task ID for this compensate operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// Previous step ID whose compensation record will be reversed.
    pub target_step_id: String,
}

/// Execute the `task.compensate` Skill.
///
/// Reverses the operation identified by `target_step_id` by executing
/// `auto_reverse_move` on its active CompensationRecord. Risk E2 + PerStep
/// approval: the approver sees an EffectManifest describing the reverse
/// move and must Allow before the reversal is committed.
///
/// Returns the new task_id on success. On user denial, returns
/// `Err(KernelError::Skill(...))` and marks the step Cancelled. On any
/// other failure (auto_reverse failure, compensation update failure,
/// finalization failure), returns Err and marks the step Failed.
pub fn execute_compensate(
    kernel: &TrustKernel,
    input: &TaskCompensateInput,
    approver: &dyn Approver,
) -> Result<String> {
    // Step 1: validate input — reject empty target_step_id explicitly,
    // since validate_input_against_manifest only checks presence + type
    // constraints (an empty string passes Text validation when max_length
    // is None).
    if input.target_step_id.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for target_step_id: must not be empty".to_string(),
        ));
    }
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert(
        "target_step_id".to_string(),
        serde_json::json!(input.target_step_id),
    );
    validate_input_against_manifest(&input_map, &task_compensate_manifest())?;

    // Step 2: create new task + step.
    kernel.create_task(
        &input.task_id,
        &format!("task.compensate:{}", input.target_step_id),
    )?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 3: look up the active CompensationRecord for target_step_id.
    // list_active_compensations returns all 'active' records; we filter
    // by step_id in Rust (compensation repo has no list_for_step method).
    // If multiple records exist for the same step, take the first.
    let raw_comp = kernel
        .list_active_compensations()?
        .into_iter()
        .find(|c| c.step_id == input.target_step_id)
        .ok_or_else(|| {
            KernelError::Skill(format!(
                "no active compensation record found for step {}",
                input.target_step_id
            ))
        })?;

    // W9 Plan 2 Task 4: 若 snapshot_encrypted 非空,先解密还原 reverse_payload 明文,
    // 供后续 build_reverse_effect_manifest / preconditions_hash / auto_reverse_move 使用。
    // 解密失败(vault 锁定 / bincode 损坏 / AEAD 校验失败)→ 审计 + 返回 Err。
    //
    // stronghold feature 未启用时:raw_comp.snapshot_encrypted 恒为 None,
    // 走明文 PoC 路径,W3a-W8 既有测试不回归。
    //
    // 审计 task_id 用 input.task_id(已在 line 90 create_task 创建,FK 保证),
    // 不用 task_id_for_step(raw_comp.step_id) 避免原始 task 已删除的边界 case。
    #[cfg(feature = "stronghold")]
    let target_comp = decrypt_compensation_if_needed(kernel, raw_comp, &input.task_id, &input.step_id)?;
    #[cfg(not(feature = "stronghold"))]
    let target_comp = raw_comp;

    // Step 4: build EffectManifest for the approval prompt.
    let effect_manifest = build_reverse_effect_manifest(&target_comp)?;

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: record approval decision (E2 + PerStep). The approver sees
    // the effect_manifest describing what will be reversed. The
    // preconditions_hash binds this approval to the exact reverse_payload
    // (the {moves: [...]} list) so post-hoc audit can verify what the user
    // actually approved — prevents TOCTOU between approve and commit.
    let preconditions_hash = {
        let mut hasher = Sha256::new();
        hasher.update(target_comp.reverse_payload.as_bytes());
        format!("{:x}", hasher.finalize())
    };
    let ctx = ApprovalContext {
        task_id: &input.task_id,
        step_id: &input.step_id,
        destination: &effect_manifest.destination,
        preconditions_hash: &preconditions_hash,
        e_level: ELevel::E2,
        d_level: DLevel::D2,
        approval_scope: ApprovalScope::Single,
    };
    let approval = record_approval_decision(kernel, approver, &effect_manifest, &ctx)?;

    // Branch on user_decision: Allow → proceed; Deny/Modify → cancel.
    match approval.user_decision {
        ApprovalDecision::Deny => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill("user denied compensation".to_string()));
        }
        ApprovalDecision::Modify => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill(
                "modify not supported for compensation".to_string(),
            ));
        }
        ApprovalDecision::Allow => { /* proceed to commit */ }
    }

    // Step 7: execute auto_reverse (the "commit" phase). On failure, mark
    // step Failed — auto_reverse_move itself rolls back any partial
    // reversals, so the filesystem is left in a consistent state.
    auto_reverse_move(&target_comp).inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Step 8: mark compensation record as "reversed".
    kernel
        .mark_compensation_status(&target_comp.comp_id, "reversed")
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    // Step 9: W10 Plan 1 — 调用真实 verify_task_compensate 重读 compensations 表,
    // 验证 status="reversed" + reverse_payload 非空(spec §6.3 Strong Verifier)。
    // 通过 → finalize with strong evidence + comp_id 作为 compensation_ref(向后兼容);
    // 失败 → step Failed + 返回错误(auto_reverse 虽执行但 DB 记录不一致,说明 commit 阶段出错)。
    let verify_ctx = VerificationContext {
        kernel,
        step_id: &input.step_id,
    };
    let outcome = verify_task_compensate(&verify_ctx, &input.target_step_id)?;
    match outcome {
        VerificationOutcome::Strong { .. } => {
            // finalize_step_success 第 4 参数是 compensation_ref(&str)而非 evidence JSON,
            // 保持原行为:用 comp_id 作为 compensation_ref,evidence_strength="strong"。
            finalize_step_success(kernel, &input.step_id, "strong", Some(&target_comp.comp_id))
                .inspect_err(|_e| {
                    let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                })?;
        }
        VerificationOutcome::Failed { reason } => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_task_compensate failed: {}",
                reason
            )));
        }
        _ => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_task_compensate returned unexpected outcome: {:?}",
                outcome
            )));
        }
    }

    Ok(input.task_id.clone())
}

/// W9 Plan 2 Task 4: 若 `raw_comp.snapshot_encrypted` 非空,用 Stronghold vault 解密
/// 还原 `reverse_payload` 明文,返回新的 CompensationRecord(reverse_payload 已替换)。
///
/// 若 `snapshot_encrypted` 为 None,直接返回 raw_comp(降级模式 / feature 未启用 / W3a PoC)。
///
/// 解密失败(vault 未注入 / 未解锁 / bincode 损坏 / AEAD 校验失败)时:
/// 1. 审计 `stronghold_snapshot_decrypt_failed` 事件,details.error 为错误变体名
///    (不含密钥 / 密文 / 密码,spec §6.4 隐私处理)。
/// 2. 返回 Err(KernelError::Compensation(...))。
///
/// 审计 task_id 用调用方传入的 `audit_task_id`(已在 tasks 表创建,FK 保证),
/// step_id 用 `audit_step_id`(新 task 的 step,展示"哪个补偿操作触发了失败")。
#[cfg(feature = "stronghold")]
fn decrypt_compensation_if_needed(
    kernel: &TrustKernel,
    raw_comp: CompensationRecord,
    audit_task_id: &str,
    audit_step_id: &str,
) -> Result<CompensationRecord> {
    use serde_json::json;

    // snapshot_encrypted 为 None:无需解密,直接返回原 record
    if raw_comp.snapshot_encrypted.is_none() {
        return Ok(raw_comp);
    }

    let comp_id = raw_comp.comp_id.clone();
    let step_id_for_audit = raw_comp.step_id.clone();

    // 取 vault:未注入或未解锁 → 审计 + Err
    let vault_opt = kernel.stronghold_vault();
    let vault = match vault_opt {
        Some(v) if v.is_unlocked() => v,
        _ => {
            kernel.audit_append_external(
                audit_task_id,
                Some(audit_step_id),
                "stronghold_snapshot_decrypt_failed",
                json!({
                    "compensation_id": comp_id,
                    "target_step_id": step_id_for_audit,
                    "error": "NotUnlocked",
                }),
            )?;
            return Err(KernelError::Compensation(
                "stronghold vault not unlocked, cannot decrypt reverse_payload".into(),
            ));
        }
    };

    // bincode 反序列化 snapshot_encrypted → EncryptedPayload
    let payload_bytes = raw_comp.snapshot_encrypted.as_ref().unwrap();
    let payload: EncryptedPayload = match bincode::deserialize(payload_bytes) {
        Ok(p) => p,
        Err(e) => {
            kernel.audit_append_external(
                audit_task_id,
                Some(audit_step_id),
                "stronghold_snapshot_decrypt_failed",
                json!({
                    "compensation_id": comp_id,
                    "target_step_id": step_id_for_audit,
                    "error": format!("BincodeDecodeFailed: {}", e),
                }),
            )?;
            return Err(KernelError::Compensation(format!(
                "stronghold bincode decode failed: {}",
                e
            )));
        }
    };

    // AEAD 解密
    let plaintext = match vault.decrypt(&payload) {
        Ok(p) => p,
        Err(e) => {
            // error 字段仅记变体名(NotUnlocked / DecryptionFailed / VaultCorrupted),
            // 不含密钥 / 密文 / 密码(spec §6.4)
            let error_str = format!("{:?}", e);
            kernel.audit_append_external(
                audit_task_id,
                Some(audit_step_id),
                "stronghold_snapshot_decrypt_failed",
                json!({
                    "compensation_id": comp_id,
                    "target_step_id": step_id_for_audit,
                    "error": error_str,
                }),
            )?;
            return Err(KernelError::Compensation(format!(
                "stronghold decrypt failed: {}",
                e
            )));
        }
    };

    // plaintext → UTF-8 字符串
    let plaintext_str = String::from_utf8(plaintext).map_err(|e| {
        KernelError::Compensation(format!("plaintext not UTF-8: {}", e))
    })?;

    // 替换 reverse_payload,返回新 record
    let mut decrypted = raw_comp;
    decrypted.reverse_payload = plaintext_str;
    Ok(decrypted)
}

/// Build an EffectManifest describing the reverse move for the approval
/// prompt.
///
/// Parses `rec.reverse_payload` JSON `{"moves": [{"from": orig, "to": curr}, ...]}`
/// and constructs an EffectManifest with:
///   - `sources`: FileSnapshots of the files at the "to" paths (current
///     locations).
///   - `destination`: the parent directory of the first "from" path
///     (where the files will be moved back to).
///   - `total_bytes`: sum of source file sizes.
///
/// Returns Err if the payload is malformed, has no moves, or any source
/// file cannot be snapshotted (e.g. missing on disk).
fn build_reverse_effect_manifest(rec: &CompensationRecord) -> Result<EffectManifest> {
    let payload: serde_json::Value = serde_json::from_str(&rec.reverse_payload)
        .map_err(|e| KernelError::Compensation(format!("invalid reverse_payload: {}", e)))?;

    let moves = payload
        .get("moves")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            KernelError::Compensation("reverse_payload missing 'moves' array".to_string())
        })?;

    if moves.is_empty() {
        return Err(KernelError::Compensation(
            "reverse_payload has empty moves array".to_string(),
        ));
    }

    let mut sources = Vec::with_capacity(moves.len());
    let mut total_bytes = 0;
    let mut first_destination: Option<String> = None;

    for m in moves {
        let from = m
            .get("from")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KernelError::Compensation("move entry missing 'from'".to_string()))?;
        let to = m
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KernelError::Compensation("move entry missing 'to'".to_string()))?;

        // Snapshot the file at the "to" path (current location).
        let snap = snapshot_file(Path::new(to))?;
        total_bytes += snap.size;
        sources.push(snap);

        // Use the first move's "from" parent dir as the destination.
        if first_destination.is_none() {
            first_destination = Path::new(from)
                .parent()
                .map(|p| canonicalize(&p.to_string_lossy()));
        }
    }

    let destination = first_destination.unwrap_or_else(|| "(reverse)".to_string());

    Ok(EffectManifest {
        sources,
        destination,
        conflicts: vec![],
        total_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::compensation::types::{CompensationLevel, ConflictPolicy};
    use crate::kernel::TrustKernel;
    use crate::repo::step_repo::{StepRecord, StepStatus};
    use crate::skills::common::create_post_commit_compensation;
    use std::fs;
    use std::path::PathBuf;

    fn tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voicepilot-w7p2-task-compensate-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Set up a previous task + step with an active CompensationRecord
    /// whose reverse_payload moves `curr` back to `orig`. Creates the file
    /// at `curr` on disk so auto_reverse can find it. Returns the comp_id.
    fn setup_previous_compensation(kernel: &TrustKernel, orig: &Path, curr: &Path) -> String {
        // Create parent dirs.
        if let Some(parent) = orig.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        if let Some(parent) = curr.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // Create the file at curr (current location).
        fs::write(curr, b"hello").unwrap();

        // Create previous task + step.
        kernel.create_task("prev-task", "previous organize").unwrap();
        kernel
            .create_step(&StepRecord::new("prev-step", "prev-task", 1))
            .unwrap();

        // Create compensation record: from=orig, to=curr.
        let moved: Vec<(PathBuf, PathBuf)> = vec![(orig.to_path_buf(), curr.to_path_buf())];
        create_post_commit_compensation(
            kernel,
            "prev-step",
            &moved,
            "filesystem.reverse_move",
            CompensationLevel::Strong,
            ConflictPolicy::AutoReverse,
            3600,
        )
        .unwrap()
    }

    #[test]
    fn execute_compensate_succeeds_and_reverses_move() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let dir = tmp_dir();
        let orig = dir.join("orig").join("file.txt");
        let curr = dir.join("curr").join("file.txt");

        let comp_id = setup_previous_compensation(&kernel, &orig, &curr);

        let input = TaskCompensateInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_step_id: "prev-step".to_string(),
        };
        let approver = AutoApprover;
        let result = execute_compensate(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "new-task");

        // New step is Succeeded with strong evidence + comp_ref.
        let new_step = kernel.get_step("new-step").unwrap().unwrap();
        assert_eq!(new_step.status, StepStatus::Succeeded);
        assert_eq!(new_step.evidence_strength.as_deref(), Some("strong"));
        assert_eq!(
            new_step.compensation_ref.as_deref(),
            Some(comp_id.as_str())
        );

        // File moved from curr back to orig.
        assert!(
            !curr.exists(),
            "file should have been moved away from curr"
        );
        assert!(orig.exists(), "file should exist at orig after reverse");
        assert_eq!(fs::read_to_string(&orig).unwrap(), "hello");

        // Compensation record status = "reversed".
        let comp = kernel.get_compensation(&comp_id).unwrap().unwrap();
        assert_eq!(comp.status, "reversed");

        // An approval record was persisted for the new task.
        let approvals = kernel.list_approvals_for_task("new-task").unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].user_decision, ApprovalDecision::Allow);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn execute_compensate_fails_when_user_denies() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let dir = tmp_dir();
        let orig = dir.join("orig").join("file.txt");
        let curr = dir.join("curr").join("file.txt");

        let comp_id = setup_previous_compensation(&kernel, &orig, &curr);

        let input = TaskCompensateInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_step_id: "prev-step".to_string(),
        };
        let approver = AutoDenier;
        let result = execute_compensate(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("denied"),
            "expected 'denied' in error, got: {}",
            err
        );

        // Step status = Cancelled.
        let new_step = kernel.get_step("new-step").unwrap().unwrap();
        assert_eq!(new_step.status, StepStatus::Cancelled);

        // File NOT moved — still at curr, not at orig.
        assert!(curr.exists(), "file should still be at curr");
        assert!(!orig.exists(), "file should NOT be at orig");

        // Compensation record status still "active".
        let comp = kernel.get_compensation(&comp_id).unwrap().unwrap();
        assert_eq!(comp.status, "active");

        // Approval record still persisted (Deny).
        let approvals = kernel.list_approvals_for_task("new-task").unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].user_decision, ApprovalDecision::Deny);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn execute_compensate_fails_when_no_active_compensation() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // No previous compensation record created.

        let input = TaskCompensateInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_step_id: "nonexistent".to_string(),
        };
        let approver = AutoApprover;
        let result = execute_compensate(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("no active compensation"),
            "expected 'no active compensation' in error, got: {}",
            err
        );
    }

    #[test]
    fn execute_compensate_fails_when_target_step_id_empty() {
        let kernel = TrustKernel::open_in_memory().unwrap();

        let input = TaskCompensateInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_step_id: "".to_string(),
        };
        let approver = AutoApprover;
        let result = execute_compensate(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("validation failed"),
            "expected 'validation failed' in error, got: {}",
            err
        );
    }
}
