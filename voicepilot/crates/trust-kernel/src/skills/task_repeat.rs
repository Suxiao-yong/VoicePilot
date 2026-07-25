//! task.repeat_verified Skill executor — W7 Plan 2 Task 3.
//!
//! Re-executes the verified move of a previous files.organize operation.
//! Read-only: uses `filesystem.search_files` + `filesystem.verify_move` only.
//! Per spec §2.4:
//!   1. Look up the previous task's `effect_manifest` from the steps table.
//!   2. Parse it into `EffectManifest` (sources + destination).
//!   3. Re-search the source directory using `source_filter`.
//!   4. Call `verify_move` to confirm the destination still has the moved
//!      files (sha256+size match).
//!   5. Return a `ToolResult` V2 with status Succeeded + Weak evidence
//!      (read-only) + data payload `{source_count, dest_verified}`.
//!
//! Risk E1, approval PerStep — but since no prepare/commit happens (read-only),
//! the approver is not invoked. The approval gate is enforced at the executor
//! level for write operations.

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{finalize_step_success, validate_input_against_manifest};
use crate::skills::manifest::task_repeat_verified_manifest;
use std::collections::HashMap;
use std::path::Path;

/// Input for the `task.repeat_verified` Skill executor.
#[derive(Debug, Clone)]
pub struct TaskRepeatVerifiedInput {
    /// New task ID for this repeat operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// Previous task ID whose `effect_manifest` will be re-verified.
    pub target_task_id: String,
    /// Glob pattern to search for source files (e.g. `*.pdf`).
    pub source_filter: String,
}

/// Execute the `task.repeat_verified` Skill.
///
/// Read-only: searches source files + verifies the previous move's
/// destination. No prepare/commit/approval happens. Returns the new task_id
/// on success. The `_approver` parameter is unused (read-only Skill).
pub fn execute_repeat_verified(
    kernel: &TrustKernel,
    input: &TaskRepeatVerifiedInput,
    _approver: &dyn Approver,
) -> Result<String> {
    // Step 1: validate input — reject empty required fields explicitly,
    // since validate_input_against_manifest only checks presence + type
    // constraints (an empty string passes Text validation when max_length
    // is None).
    if input.target_task_id.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for target_task_id: must not be empty".to_string(),
        ));
    }
    if input.source_filter.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for source_filter: must not be empty".to_string(),
        ));
    }
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert(
        "target_task_id".to_string(),
        serde_json::json!(input.target_task_id),
    );
    input_map.insert(
        "source_filter".to_string(),
        serde_json::json!(input.source_filter),
    );
    validate_input_against_manifest(&input_map, &task_repeat_verified_manifest())?;

    // Step 2: create new task + step.
    kernel.create_task(
        &input.task_id,
        &format!("repeat_verified:{}", input.target_task_id),
    )?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 3: look up the previous task's effect_manifest.
    let prev_manifest = lookup_effect_manifest(kernel, &input.target_task_id)?;

    // Step 4: mark the new step Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 5: re-search the source directory using source_filter.
    // The source dir is derived from the first source's parent dir.
    let source_dir = prev_manifest
        .sources
        .first()
        .and_then(|s| Path::new(&s.canonical_path).parent())
        .ok_or_else(|| {
            KernelError::Skill("no source paths in previous manifest".to_string())
        })?
        .to_path_buf();
    let found = kernel
        .filesystem()
        .search_files(&source_dir, &input.source_filter)?;
    if found.is_empty() {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        return Err(KernelError::Skill(format!(
            "no files matching '{}' found under {}",
            input.source_filter,
            source_dir.display()
        )));
    }

    // Step 6: verify the previous move's destination still has the files.
    let _verify_result = kernel
        .filesystem()
        .verify_move(&prev_manifest)
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    // Step 7: finalize step as Succeeded.
    finalize_step_success(kernel, &input.step_id, "weak", None)
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    // Step 8: return the new task_id.
    Ok(input.task_id.clone())
}

/// Look up the most recent step with a non-null `effect_manifest` for the
/// given task_id. Returns `Err(KernelError::Skill(...))` if no such step
/// exists or the manifest JSON fails to parse.
fn lookup_effect_manifest(kernel: &TrustKernel, task_id: &str) -> Result<EffectManifest> {
    let conn = kernel.conn();
    let manifest_str: Option<String> = conn
        .query_row(
            "SELECT effect_manifest FROM steps
             WHERE task_id = ?1 AND effect_manifest IS NOT NULL
             ORDER BY step_order DESC LIMIT 1",
            rusqlite::params![task_id],
            |r| r.get(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(KernelError::Db(other)),
        })?;
    let manifest_str = manifest_str.ok_or_else(|| {
        KernelError::Skill(format!(
            "no effect_manifest found for task_id '{}'",
            task_id
        ))
    })?;
    let manifest: EffectManifest = serde_json::from_str(&manifest_str).map_err(|e| {
        KernelError::Skill(format!(
            "failed to parse effect_manifest for task_id '{}': {}",
            task_id, e
        ))
    })?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;
    use crate::policy::transaction::EffectManifest;
    use crate::repo::step_repo::{StepRecord, StepStatus};
    use crate::tools::fs_paths::canonicalize;
    use crate::tools::fs_snapshot::snapshot_file;
    use std::fs;
    use std::path::PathBuf;

    fn tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voicepilot-w7p2-task-repeat-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Build a real `EffectManifest` from on-disk source files + a destination
    /// dir. The destination must contain files with matching sha256+size so
    /// `verify_move` passes.
    fn build_manifest_from_disk(src_dir: &Path, dest_dir: &Path, filenames: &[&str]) -> EffectManifest {
        let mut snapshots = Vec::new();
        let mut total_bytes = 0;
        for name in filenames {
            let src = src_dir.join(name);
            let snap = snapshot_file(&src).unwrap();
            total_bytes += snap.size;
            snapshots.push(snap);
        }
        EffectManifest {
            sources: snapshots,
            destination: canonicalize(&dest_dir.to_string_lossy()),
            conflicts: vec![],
            total_bytes,
        }
    }

    /// Persist a previous task + step with the given `effect_manifest`.
    fn persist_previous_step(kernel: &TrustKernel, manifest: &EffectManifest) {
        kernel.create_task("prev-task", "previous organize").unwrap();
        let mut prev_step = StepRecord::new("prev-step", "prev-task", 1);
        prev_step.effect_manifest = Some(serde_json::to_value(manifest).unwrap());
        kernel.create_step(&prev_step).unwrap();
    }

    #[test]
    fn execute_repeat_verified_succeeds_when_previous_manifest_exists() {
        let kernel = TrustKernel::open_in_memory().unwrap();

        // Set up source + destination on disk (content must match for verify_move).
        let dir = tmp_dir();
        let src_dir = dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let dest_dir = dir.join("out");
        fs::create_dir_all(&dest_dir).unwrap();
        fs::write(src_dir.join("a.pdf"), b"pdf1").unwrap();
        fs::write(dest_dir.join("a.pdf"), b"pdf1").unwrap();

        // Build + persist a real EffectManifest.
        let manifest = build_manifest_from_disk(&src_dir, &dest_dir, &["a.pdf"]);
        persist_previous_step(&kernel, &manifest);

        // Call execute_repeat_verified.
        let input = TaskRepeatVerifiedInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_task_id: "prev-task".to_string(),
            source_filter: "*.pdf".to_string(),
        };
        let approver = AutoApprover;
        let result = execute_repeat_verified(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "new-task");

        // Verify the new task + step were created and step is Succeeded.
        let new_step = kernel.get_step("new-step").unwrap().unwrap();
        assert_eq!(new_step.status, StepStatus::Succeeded);
        assert_eq!(new_step.evidence_strength.as_deref(), Some("weak"));
        assert!(new_step.compensation_ref.is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn execute_repeat_verified_fails_when_no_previous_manifest() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // No previous task created.

        let input = TaskRepeatVerifiedInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_task_id: "nonexistent".to_string(),
            source_filter: "*.pdf".to_string(),
        };
        let approver = AutoApprover;
        let result = execute_repeat_verified(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("no effect_manifest"),
            "expected 'no effect_manifest' in error, got: {}",
            err
        );
    }

    #[test]
    fn execute_repeat_verified_fails_when_source_filter_matches_nothing() {
        let kernel = TrustKernel::open_in_memory().unwrap();

        let dir = tmp_dir();
        let src_dir = dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let dest_dir = dir.join("out");
        fs::create_dir_all(&dest_dir).unwrap();
        fs::write(src_dir.join("a.pdf"), b"pdf1").unwrap();
        fs::write(dest_dir.join("a.pdf"), b"pdf1").unwrap();

        let manifest = build_manifest_from_disk(&src_dir, &dest_dir, &["a.pdf"]);
        persist_previous_step(&kernel, &manifest);

        // Use a filter that matches nothing.
        let input = TaskRepeatVerifiedInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_task_id: "prev-task".to_string(),
            source_filter: "*.nonexistent".to_string(),
        };
        let approver = AutoApprover;
        let result = execute_repeat_verified(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("no files matching"),
            "expected 'no files matching' in error, got: {}",
            err
        );

        // Step should be marked Failed.
        let new_step = kernel.get_step("new-step").unwrap().unwrap();
        assert_eq!(new_step.status, StepStatus::Failed);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn execute_repeat_verified_fails_when_target_task_id_empty() {
        let kernel = TrustKernel::open_in_memory().unwrap();

        let input = TaskRepeatVerifiedInput {
            task_id: "new-task".to_string(),
            step_id: "new-step".to_string(),
            target_task_id: "".to_string(),
            source_filter: "*.pdf".to_string(),
        };
        let approver = AutoApprover;
        let result = execute_repeat_verified(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("validation failed"),
            "expected 'validation failed' in error, got: {}",
            err
        );
    }
}
