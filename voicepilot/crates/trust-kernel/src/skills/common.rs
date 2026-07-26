//! skills/common.rs — Shared helpers for Skill executors (W7 Plan 2 Task 1).
//!
//! Extracts the reusable pieces of the prepare → approve → commit → verify →
//! compensate pipeline from `executor.rs` (W3b files.organize). Future skill
//! executors (UIA, Playwright, etc.) call these helpers instead of
//! re-implementing the approval recording, compensation creation, and step
//! finalization logic.
//!
//! The `run_prepare_approve_commit` pipeline is composed from three utility
//! functions:
//!   1. `record_approval_decision` — prompt the approver + persist the record.
//!   2. `create_post_commit_compensation` — build + persist a CompensationRecord.
//!   3. `finalize_step_success` — mark the step Succeeded with evidence + comp_ref.
//!
//! Additionally, `validate_input_against_manifest` validates a caller-supplied
//! input map against a SkillManifest's `inputs` constraints.

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalRecord, ApprovalScope};
use crate::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::mcp::client::McpClient;
use crate::mcp::repo::McpServerRepo;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::StepStatus;
use crate::skills::manifest::{SkillInput, SkillInputType, SkillManifest};
use crate::tools::fs_paths::canonicalize;
use crate::toolresult::{EvidenceStrength, ToolResult, ToolStatus};
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;

// ===== run_prepare_approve_commit pipeline helpers =====

/// Caller-supplied context for `record_approval_decision`. Groups the
/// step-identifying and risk/scope fields so the helper signature stays
/// small. Future skill executors construct one of these per prepared step.
#[derive(Debug, Clone)]
pub struct ApprovalContext<'a> {
    pub task_id: &'a str,
    pub step_id: &'a str,
    pub destination: &'a str,
    pub preconditions_hash: &'a str,
    pub e_level: ELevel,
    pub d_level: DLevel,
    pub approval_scope: ApprovalScope,
}

/// Record the user's approval decision (Allow/Deny/Modify) for a prepared step.
///
/// Calls `approver.prompt(effect_manifest)` to obtain the user's decision,
/// then constructs an `ApprovalRecord` from `ctx` and persists it via
/// `kernel.record_approval()`. Returns the persisted record so the caller
/// can branch on `user_decision`:
/// - `Allow`  → proceed to commit
/// - `Deny`   → mark step Cancelled and return a Cancelled ToolResult
/// - `Modify` → (W7 future) re-prepare with modified args
pub fn record_approval_decision(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    effect_manifest: &EffectManifest,
    ctx: &ApprovalContext,
) -> Result<ApprovalRecord> {
    let decision = approver.prompt(effect_manifest);
    let approval_id = format!("appr-{}", uuid::Uuid::new_v4());
    let record = ApprovalRecord {
        approval_id,
        task_id: ctx.task_id.to_string(),
        step_id: Some(ctx.step_id.to_string()),
        risk_level: ctx.e_level.as_str().to_string(),
        args_hash: ctx.preconditions_hash.to_string(),
        user_decision: decision,
        decided_at: Utc::now().to_rfc3339(),
        e_level: ctx.e_level,
        d_level: ctx.d_level,
        destination: canonicalize(ctx.destination),
        egress_approved: false,
        approval_scope: ctx.approval_scope,
        policy_bundle_hash: kernel.gateway().bundle_hash().to_string(),
    };
    kernel.record_approval(&record)?;
    Ok(record)
}

/// Create a post-commit CompensationRecord for a list of moved paths.
///
/// Builds a JSON reverse_payload of `{"moves": [{"from": orig, "to": curr}, ...]}`
/// and persists a compensation record via `kernel.create_compensation()`.
/// Returns the new comp_id on success.
///
/// On failure, the caller MUST handle the uncompensatable state (the move is
/// already committed on disk) — typically by marking the step Failed and
/// surfacing the error.
pub fn create_post_commit_compensation(
    kernel: &TrustKernel,
    step_id: &str,
    moved_paths: &[(PathBuf, PathBuf)],
    compensate_fn: &str,
    level: CompensationLevel,
    conflict_policy: ConflictPolicy,
    ttl_seconds: i64,
) -> Result<String> {
    let comp_id = format!("comp-{}", uuid::Uuid::new_v4());
    let reverse_payload = serde_json::json!({
        "moves": moved_paths.iter().map(|(orig, curr)| {
            serde_json::json!({
                "from": orig.to_string_lossy().replace('\\', "/"),
                "to":   curr.to_string_lossy().replace('\\', "/"),
            })
        }).collect::<Vec<_>>()
    })
    .to_string();
    let record = CompensationRecord {
        comp_id: comp_id.clone(),
        step_id: step_id.to_string(),
        level,
        snapshot_encrypted: None,
        ttl_expires: (Utc::now() + chrono::Duration::seconds(ttl_seconds)).to_rfc3339(),
        status: "active".to_string(),
        snapshot_vault_ref: None,
        conflict_policy,
        compensate_fn: compensate_fn.to_string(),
        reverse_payload,
    };
    kernel.create_compensation(&record)?;
    Ok(comp_id)
}

/// Finalize a step as Succeeded with the given evidence strength and
/// optional compensation_ref.
///
/// Calls `update_step_post_commit` then `update_step_status(Succeeded)`.
/// Returns Err if either kernel call fails (the step is left in an
/// intermediate state — caller should surface the error).
pub fn finalize_step_success(
    kernel: &TrustKernel,
    step_id: &str,
    evidence_strength: &str,
    compensation_ref: Option<&str>,
) -> Result<()> {
    kernel.update_step_post_commit(step_id, evidence_strength, compensation_ref)?;
    kernel.update_step_status(step_id, StepStatus::Succeeded)?;
    Ok(())
}

// ===== MCP invocation =====

/// Invoke a tool on an external MCP server (e.g. Playwright MCP).
///
/// Looks up the server record by `server_id` in the `mcp_servers` table,
/// spawns the MCP subprocess via `McpClient::spawn`, performs the
/// `initialize` handshake, and calls `tools/call` with the given tool name
/// and arguments.
///
/// Returns `Ok(ToolResult { status: Succeeded, data: <parsed> })` on success.
/// Returns `Err(KernelError::Mcp(...))` on any failure (server not found,
/// server disabled, command missing, spawn failure, initialize failure,
/// invoke failure). The Skill executor catches the error and constructs a
/// `Failed` ToolResult with `error_code = "mcp_playwright_unavailable"`.
///
/// `idempotency_key` is generated as `mcp-{uuid}`. `started_at` / `finished_at`
/// bracket the spawn+invoke lifecycle. `data` is the JSON returned by
/// `McpClient::invoke_tool` (already parsed from `content[0].text`).
pub fn invoke_mcp_tool(
    kernel: &TrustKernel,
    server_id: &str,
    tool_name: &str,
    args: serde_json::Value,
) -> Result<ToolResult> {
    let started_at = Utc::now();

    // Look up the server config inside a block scope so the MutexGuard is
    // dropped before we spawn the subprocess (avoids holding the DB lock
    // across potentially slow MCP I/O).
    let (command, args_vec, env_json) = {
        let conn = kernel.conn();
        let rec = McpServerRepo::new()
            .get(&conn, server_id)?
            .ok_or_else(|| {
                KernelError::Mcp(format!(
                    "MCP server '{server_id}' not found in mcp_servers table"
                ))
            })?;
        if !rec.enabled {
            return Err(KernelError::Mcp(format!(
                "MCP server '{server_id}' is disabled"
            )));
        }
        let command = rec.command.clone().ok_or_else(|| {
            KernelError::Mcp(format!(
                "MCP server '{server_id}' missing command field"
            ))
        })?;
        let args_str = rec.args.clone().unwrap_or_else(|| "[]".to_string());
        let args_vec: Vec<String> = serde_json::from_str(&args_str).map_err(|e| {
            KernelError::Mcp(format!(
                "MCP server '{server_id}' args parse error: {e}"
            ))
        })?;
        let env_str = rec.env.clone().unwrap_or_else(|| "{}".to_string());
        let env_json: serde_json::Value = serde_json::from_str(&env_str).map_err(|e| {
            KernelError::Mcp(format!(
                "MCP server '{server_id}' env parse error: {e}"
            ))
        })?;
        (command, args_vec, env_json)
    };

    // Spawn the MCP subprocess, run the initialize handshake, and invoke
    // the tool. All errors here are already KernelError::Mcp(...).
    let mut client = McpClient::spawn(&command, &args_vec, &env_json)?;
    client.initialize()?;
    let result = client.invoke_tool(tool_name, args)?;

    let finished_at = Utc::now();
    let tool_result = ToolResult {
        status: ToolStatus::Succeeded,
        data: result,
        evidence_strength: EvidenceStrength::Weak,
        compensation_ref: None,
        compensation_level: CompensationLevel::None,
        preconditions_hash: None,
        idempotency_key: format!("mcp-{}", uuid::Uuid::new_v4()),
        egress_performed: false,
        data_classification: DLevel::D2,
        error_code: None,
        retryable: false,
        safe_to_retry: false,
        started_at,
        finished_at,
    };
    Ok(tool_result)
}

// ===== Input validation =====

/// Validate a caller-supplied input map against a SkillManifest's `inputs`
/// constraints. Returns Ok(()) if all inputs satisfy the manifest, or
/// Err(KernelError::Skill(...)) with a descriptive message on violation.
///
/// Rules per `SkillInputType`:
/// - `Directory` / `File`: if `allowed_roots` non-empty, the path must equal
///   or start with `{root}/` for some root in `allowed_roots`.
/// - `FileFilter`: must be a string-like value (no further constraints).
/// - `Text`: if `max_length` is `Some(n)`, the string's char count must be <= n.
/// - `Number`: value must parse as f64.
/// - `Enum`: if `allowed_values` non-empty, the value (as string) must be in
///   `allowed_values`.
/// - `Url`: value must start with `http://` or `https://`.
///
/// Required fields (`required: true`) must be present; optional fields are
/// skipped when absent.
pub fn validate_input_against_manifest(
    input: &HashMap<String, serde_json::Value>,
    manifest: &SkillManifest,
) -> Result<()> {
    for (name, spec) in &manifest.inputs {
        let value = match input.get(name) {
            Some(v) => v,
            None => {
                if spec.required {
                    return Err(KernelError::Skill(format!(
                        "validation failed for {name}: required field missing"
                    )));
                }
                continue;
            }
        };
        validate_single_input(name, spec, value)?;
    }
    Ok(())
}

fn validate_single_input(name: &str, spec: &SkillInput, value: &serde_json::Value) -> Result<()> {
    match spec.input_type {
        SkillInputType::Directory | SkillInputType::File => {
            let s = value_as_string(name, value)?;
            if !spec.allowed_roots.is_empty() {
                let normalized = s.replace('\\', "/");
                let matched = spec.allowed_roots.iter().any(|root| {
                    let root_norm = root.replace('\\', "/");
                    normalized == root_norm || normalized.starts_with(&format!("{root_norm}/"))
                });
                if !matched {
                    return Err(KernelError::Skill(format!(
                        "validation failed for {name}: path '{s}' not under any allowed_root"
                    )));
                }
            }
        }
        SkillInputType::FileFilter => {
            let _ = value_as_string(name, value)?;
        }
        SkillInputType::Text => {
            let s = value_as_string(name, value)?;
            if let Some(max) = spec.max_length {
                let len = s.chars().count() as u32;
                if len > max {
                    return Err(KernelError::Skill(format!(
                        "validation failed for {name}: length {len} exceeds max_length {max}"
                    )));
                }
            }
        }
        SkillInputType::Number => {
            let parsed = match value {
                serde_json::Value::Number(n) => n.as_f64(),
                serde_json::Value::String(s) => s.parse::<f64>().ok(),
                _ => None,
            };
            if parsed.is_none() {
                return Err(KernelError::Skill(format!(
                    "validation failed for {name}: value is not a number"
                )));
            }
        }
        SkillInputType::Enum => {
            let s = value_as_string(name, value)?;
            if !spec.allowed_values.is_empty()
                && !spec.allowed_values.iter().any(|v| v == &s)
            {
                return Err(KernelError::Skill(format!(
                    "validation failed for {name}: value '{s}' not in allowed_values"
                )));
            }
        }
        SkillInputType::Url => {
            let s = value_as_string(name, value)?;
            if !(s.starts_with("http://") || s.starts_with("https://")) {
                return Err(KernelError::Skill(format!(
                    "validation failed for {name}: value is not an http(s) url"
                )));
            }
        }
    }
    Ok(())
}

fn value_as_string(name: &str, value: &serde_json::Value) -> Result<String> {
    match value {
        serde_json::Value::String(s) => Ok(s.clone()),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
        _ => Err(KernelError::Skill(format!(
            "validation failed for {name}: expected string-like value, got {}",
            value_type_name(value)
        ))),
    }
}

fn value_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::approval::types::ApprovalDecision;
    use crate::kernel::TrustKernel;
    use crate::mcp::repo::McpServerRecord;
    use crate::policy::transaction::EffectManifest;
    use crate::policy::types::{DLevel, ELevel};
    use crate::repo::step_repo::{StepRecord, StepStatus};
    use crate::skills::manifest::{files_organize_manifest, SkillInput, SkillInputType};
    use std::collections::HashMap;
    use std::path::PathBuf;

    // ===== validate_input_against_manifest tests =====

    fn build_manifest_with_one_input(
        name: &str,
        input_type: SkillInputType,
        required: bool,
        allowed_roots: Vec<String>,
        allowed_values: Vec<String>,
        max_length: Option<u32>,
    ) -> SkillManifest {
        let mut inputs = HashMap::new();
        inputs.insert(
            name.to_string(),
            SkillInput {
                input_type,
                required,
                allowed_roots,
                allowed_values,
                max_length,
                default: None,
            },
        );
        let mut m = files_organize_manifest();
        m.inputs = inputs;
        m
    }

    #[test]
    fn validate_missing_required_field_fails() {
        let manifest = build_manifest_with_one_input(
            "source",
            SkillInputType::Directory,
            true,
            vec![],
            vec![],
            None,
        );
        let input = HashMap::new();
        let err = validate_input_against_manifest(&input, &manifest).unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(err.to_string().contains("source"));
        assert!(err.to_string().contains("required"));
    }

    #[test]
    fn validate_missing_optional_field_ok() {
        let manifest = build_manifest_with_one_input(
            "source",
            SkillInputType::Directory,
            false,
            vec![],
            vec![],
            None,
        );
        let input = HashMap::new();
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_allowed_roots_violation_fails() {
        let manifest = build_manifest_with_one_input(
            "source",
            SkillInputType::Directory,
            true,
            vec!["Downloads".to_string(), "Desktop".to_string()],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("source".to_string(), serde_json::json!("/etc/passwd"));
        let err = validate_input_against_manifest(&input, &manifest).unwrap_err();
        assert!(err.to_string().contains("allowed_root"));
    }

    #[test]
    fn validate_allowed_roots_match_ok() {
        let manifest = build_manifest_with_one_input(
            "source",
            SkillInputType::Directory,
            true,
            vec!["Downloads".to_string()],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("source".to_string(), serde_json::json!("Downloads/PDF"));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_allowed_roots_exact_match_ok() {
        let manifest = build_manifest_with_one_input(
            "source",
            SkillInputType::Directory,
            true,
            vec!["Downloads".to_string()],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("source".to_string(), serde_json::json!("Downloads"));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_allowed_roots_prefix_without_separator_fails() {
        // "Desktopx" starts with "Desktop" but not "Desktop/" — must fail.
        let manifest = build_manifest_with_one_input(
            "source",
            SkillInputType::Directory,
            true,
            vec!["Desktop".to_string()],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("source".to_string(), serde_json::json!("Desktopx/foo"));
        assert!(validate_input_against_manifest(&input, &manifest).is_err());
    }

    #[test]
    fn validate_enum_violation_fails() {
        let manifest = build_manifest_with_one_input(
            "mode",
            SkillInputType::Enum,
            true,
            vec![],
            vec!["auto".to_string(), "manual".to_string()],
            None,
        );
        let mut input = HashMap::new();
        input.insert("mode".to_string(), serde_json::json!("semi-auto"));
        let err = validate_input_against_manifest(&input, &manifest).unwrap_err();
        assert!(err.to_string().contains("allowed_values"));
    }

    #[test]
    fn validate_enum_match_ok() {
        let manifest = build_manifest_with_one_input(
            "mode",
            SkillInputType::Enum,
            true,
            vec![],
            vec!["auto".to_string(), "manual".to_string()],
            None,
        );
        let mut input = HashMap::new();
        input.insert("mode".to_string(), serde_json::json!("auto"));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_text_max_length_violation_fails() {
        let manifest = build_manifest_with_one_input(
            "filter",
            SkillInputType::Text,
            true,
            vec![],
            vec![],
            Some(5),
        );
        let mut input = HashMap::new();
        input.insert("filter".to_string(), serde_json::json!("abcdef"));
        let err = validate_input_against_manifest(&input, &manifest).unwrap_err();
        assert!(err.to_string().contains("max_length"));
    }

    #[test]
    fn validate_text_max_length_exact_ok() {
        let manifest = build_manifest_with_one_input(
            "filter",
            SkillInputType::Text,
            true,
            vec![],
            vec![],
            Some(5),
        );
        let mut input = HashMap::new();
        input.insert("filter".to_string(), serde_json::json!("abcde"));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_number_non_numeric_fails() {
        let manifest = build_manifest_with_one_input(
            "count",
            SkillInputType::Number,
            true,
            vec![],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("count".to_string(), serde_json::json!("not-a-number"));
        let err = validate_input_against_manifest(&input, &manifest).unwrap_err();
        assert!(err.to_string().contains("number"));
    }

    #[test]
    fn validate_number_numeric_string_ok() {
        let manifest = build_manifest_with_one_input(
            "count",
            SkillInputType::Number,
            true,
            vec![],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("count".to_string(), serde_json::json!("42"));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_number_json_number_ok() {
        let manifest = build_manifest_with_one_input(
            "count",
            SkillInputType::Number,
            true,
            vec![],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("count".to_string(), serde_json::json!(1.5));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_url_invalid_scheme_fails() {
        let manifest = build_manifest_with_one_input(
            "url",
            SkillInputType::Url,
            true,
            vec![],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("url".to_string(), serde_json::json!("ftp://example.com"));
        let err = validate_input_against_manifest(&input, &manifest).unwrap_err();
        assert!(err.to_string().contains("http"));
    }

    #[test]
    fn validate_url_https_ok() {
        let manifest = build_manifest_with_one_input(
            "url",
            SkillInputType::Url,
            true,
            vec![],
            vec![],
            None,
        );
        let mut input = HashMap::new();
        input.insert("url".to_string(), serde_json::json!("https://example.com"));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_files_organize_manifest_valid_input_ok() {
        let manifest = files_organize_manifest();
        let mut input = HashMap::new();
        input.insert("source".to_string(), serde_json::json!("Downloads"));
        input.insert("filter".to_string(), serde_json::json!("*.pdf"));
        input.insert("destination".to_string(), serde_json::json!("Workspace/papers"));
        assert!(validate_input_against_manifest(&input, &manifest).is_ok());
    }

    #[test]
    fn validate_files_organize_manifest_bad_destination_fails() {
        let manifest = files_organize_manifest();
        let mut input = HashMap::new();
        input.insert("source".to_string(), serde_json::json!("Downloads"));
        input.insert("filter".to_string(), serde_json::json!("*.pdf"));
        // /tmp is not in destination's allowed_roots (Workspace, Documents, Desktop).
        input.insert("destination".to_string(), serde_json::json!("/tmp/evil"));
        assert!(validate_input_against_manifest(&input, &manifest).is_err());
    }

    // ===== record_approval_decision tests =====

    fn blank_effect_manifest() -> EffectManifest {
        EffectManifest {
            sources: vec![],
            destination: "out".to_string(),
            conflicts: vec![],
            total_bytes: 0,
        }
    }

    fn setup_kernel_with_step() -> TrustKernel {
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel.create_task("t1", "test goal").unwrap();
        kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
        kernel
    }

    fn approval_ctx<'a>(task_id: &'a str, step_id: &'a str, hash: &'a str) -> ApprovalContext<'a> {
        ApprovalContext {
            task_id,
            step_id,
            destination: "out",
            preconditions_hash: hash,
            e_level: ELevel::E2,
            d_level: DLevel::D2,
            approval_scope: ApprovalScope::Single,
        }
    }

    #[test]
    fn record_approval_decision_allow_persists_record() {
        let kernel = setup_kernel_with_step();
        let approver = AutoApprover;
        let manifest = blank_effect_manifest();
        let ctx = approval_ctx("t1", "s1", "sha256:abc");
        let rec = record_approval_decision(&kernel, &approver, &manifest, &ctx).unwrap();
        assert_eq!(rec.user_decision, ApprovalDecision::Allow);
        assert_eq!(rec.task_id, "t1");
        assert_eq!(rec.step_id.as_deref(), Some("s1"));
        assert_eq!(rec.e_level, ELevel::E2);
        assert_eq!(rec.d_level, DLevel::D2);
        assert_eq!(rec.risk_level, "E2");
        // Kernel has 1 approval record for t1.
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].approval_id, rec.approval_id);
    }

    #[test]
    fn record_approval_decision_deny_persists_record() {
        let kernel = setup_kernel_with_step();
        let approver = AutoDenier;
        let manifest = blank_effect_manifest();
        let ctx = approval_ctx("t1", "s1", "sha256:abc");
        let rec = record_approval_decision(&kernel, &approver, &manifest, &ctx).unwrap();
        assert_eq!(rec.user_decision, ApprovalDecision::Deny);
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
    }

    // ===== create_post_commit_compensation tests =====

    #[test]
    fn create_post_commit_compensation_persists_record() {
        let kernel = setup_kernel_with_step();
        let moved: Vec<(PathBuf, PathBuf)> = vec![
            (PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf")),
            (PathBuf::from("src/b.pdf"), PathBuf::from("out/b.pdf")),
        ];
        let comp_id = create_post_commit_compensation(
            &kernel,
            "s1",
            &moved,
            "filesystem.reverse_move",
            CompensationLevel::Strong,
            ConflictPolicy::AutoReverse,
            3600,
        )
        .unwrap();
        assert!(comp_id.starts_with("comp-"));
        let comp = kernel
            .get_compensation(&comp_id)
            .unwrap()
            .expect("compensation must exist");
        assert_eq!(comp.level, CompensationLevel::Strong);
        assert_eq!(comp.status, "active");
        assert_eq!(comp.conflict_policy, ConflictPolicy::AutoReverse);
        assert_eq!(comp.compensate_fn, "filesystem.reverse_move");
        // reverse_payload contains both moves.
        let payload: serde_json::Value = serde_json::from_str(&comp.reverse_payload).unwrap();
        let moves = payload.get("moves").and_then(|v| v.as_array()).unwrap();
        assert_eq!(moves.len(), 2);
    }

    #[test]
    fn create_post_commit_compensation_empty_moves_ok() {
        let kernel = setup_kernel_with_step();
        let moved: Vec<(PathBuf, PathBuf)> = vec![];
        let comp_id = create_post_commit_compensation(
            &kernel,
            "s1",
            &moved,
            "filesystem.reverse_move",
            CompensationLevel::BestEffort,
            ConflictPolicy::RequireConfirmation,
            60,
        )
        .unwrap();
        let comp = kernel.get_compensation(&comp_id).unwrap().unwrap();
        let payload: serde_json::Value = serde_json::from_str(&comp.reverse_payload).unwrap();
        let moves = payload.get("moves").and_then(|v| v.as_array()).unwrap();
        assert!(moves.is_empty());
    }

    // ===== finalize_step_success tests =====

    #[test]
    fn finalize_step_success_marks_succeeded() {
        let kernel = setup_kernel_with_step();
        kernel.update_step_status("s1", StepStatus::Running).unwrap();
        let comp_id = create_post_commit_compensation(
            &kernel,
            "s1",
            &[],
            "filesystem.reverse_move",
            CompensationLevel::Strong,
            ConflictPolicy::AutoReverse,
            3600,
        )
        .unwrap();
        finalize_step_success(&kernel, "s1", "strong", Some(&comp_id)).unwrap();
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
        assert_eq!(step.compensation_ref.as_deref(), Some(comp_id.as_str()));
    }

    #[test]
    fn finalize_step_success_without_compensation_ref() {
        let kernel = setup_kernel_with_step();
        kernel.update_step_status("s1", StepStatus::Running).unwrap();
        finalize_step_success(&kernel, "s1", "weak", None).unwrap();
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));
        assert!(step.compensation_ref.is_none());
    }

    // ===== Composition: helpers work together =====

    #[test]
    fn pipeline_helpers_compose_end_to_end() {
        // Simulate: prepare (mock) → approve (Allow) → commit (mock) →
        // create compensation → finalize step.
        let kernel = setup_kernel_with_step();
        kernel.update_step_status("s1", StepStatus::Running).unwrap();
        let manifest = blank_effect_manifest();
        let approver = AutoApprover;
        let preconditions_hash = "sha256:fake".to_string();
        let ctx = approval_ctx("t1", "s1", &preconditions_hash);

        // Step A: record approval (Allow).
        let approval = record_approval_decision(&kernel, &approver, &manifest, &ctx).unwrap();
        assert_eq!(approval.user_decision, ApprovalDecision::Allow);

        // Step B: pretend commit happened, moved some paths.
        let moved: Vec<(PathBuf, PathBuf)> =
            vec![(PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf"))];
        let comp_id = create_post_commit_compensation(
            &kernel,
            "s1",
            &moved,
            "filesystem.reverse_move",
            CompensationLevel::Strong,
            ConflictPolicy::AutoReverse,
            3600,
        )
        .unwrap();

        // Step C: finalize step as Succeeded.
        finalize_step_success(&kernel, "s1", "strong", Some(&comp_id)).unwrap();

        // Verify final state.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.compensation_ref.as_deref(), Some(comp_id.as_str()));
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
        let comps = kernel.list_active_compensations().unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].comp_id, comp_id);
    }

    #[test]
    fn pipeline_helpers_compose_with_deny() {
        // Simulate: prepare → approve (Deny) → cancel step (no compensation).
        let kernel = setup_kernel_with_step();
        kernel.update_step_status("s1", StepStatus::Running).unwrap();
        let manifest = blank_effect_manifest();
        let approver = AutoDenier;
        let preconditions_hash = "sha256:fake".to_string();
        let ctx = approval_ctx("t1", "s1", &preconditions_hash);

        let approval = record_approval_decision(&kernel, &approver, &manifest, &ctx).unwrap();
        assert_eq!(approval.user_decision, ApprovalDecision::Deny);

        // On Deny: cancel the step, do NOT create compensation.
        kernel.update_step_status("s1", StepStatus::Cancelled).unwrap();

        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Cancelled);
        assert!(step.compensation_ref.is_none());
        let comps = kernel.list_active_compensations().unwrap();
        assert!(comps.is_empty());
        // Approval still recorded.
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
    }

    // ===== invoke_mcp_tool tests (W7 Plan 5 Task 3) =====

    #[test]
    fn invoke_mcp_tool_server_not_found_returns_err() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let result =
            invoke_mcp_tool(&kernel, "nonexistent-server", "echo", serde_json::json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "error should mention 'not found', got: {err}"
        );
    }

    #[test]
    fn invoke_mcp_tool_disabled_server_returns_err() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = McpServerRecord {
            server_id: "disabled-server".to_string(),
            name: "Disabled".to_string(),
            version: "1.0.0".to_string(),
            transport: "stdio".to_string(),
            enabled: false,
            trusted: false,
            protocol_version: None,
            allowed_origins: None,
            allowed_paths: None,
            command: Some("python".to_string()),
            args: Some("[]".to_string()),
            env: Some("{}".to_string()),
        };
        McpServerRepo::new()
            .create(&kernel.conn(), &rec)
            .expect("insert must succeed");
        let result =
            invoke_mcp_tool(&kernel, "disabled-server", "echo", serde_json::json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("disabled"),
            "error should mention 'disabled', got: {err}"
        );
    }

    #[test]
    fn invoke_mcp_tool_missing_command_returns_err() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = McpServerRecord {
            server_id: "no-cmd-server".to_string(),
            name: "NoCmd".to_string(),
            version: "1.0.0".to_string(),
            transport: "stdio".to_string(),
            enabled: true,
            trusted: false,
            protocol_version: None,
            allowed_origins: None,
            allowed_paths: None,
            command: None,
            args: Some("[]".to_string()),
            env: Some("{}".to_string()),
        };
        McpServerRepo::new()
            .create(&kernel.conn(), &rec)
            .expect("insert must succeed");
        let result =
            invoke_mcp_tool(&kernel, "no-cmd-server", "echo", serde_json::json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("missing command"),
            "error should mention 'missing command', got: {err}"
        );
    }

    #[test]
    fn invoke_mcp_tool_success_via_python_mock() {
        // Probe Python availability. Skip (pass) if absent — not fail.
        let python_probe = std::process::Command::new("python").arg("--version").output();
        let python_available = match python_probe {
            Ok(out) => out.status.success(),
            Err(_) => false,
        };
        if !python_available {
            eprintln!("skipping invoke_mcp_tool_success_via_python_mock: python not on PATH");
            return;
        }

        // Mock MCP server: for tools/call with name="echo", echoes back
        // arguments.msg as {"echo": <msg>}. This verifies args round-trip
        // through McpServerRecord.args JSON encoding → invoke_mcp_tool
        // decode → McpClient::spawn → MCP server → response payload.
        let mock_script = r#"
import sys, json
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    if msg.get("method") == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "serverInfo": {"name": "mock", "version": "0.1.0"}
            }
        })
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        args = msg.get("params", {}).get("arguments", {})
        if name == "echo":
            text_payload = json.dumps({"echo": args.get("msg")})
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "result": {
                    "content": [{"type": "text", "text": text_payload}],
                    "isError": False
                }
            })
        else:
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32601, "message": f"unknown tool {name}"}
            })
    else:
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        })
"#;

        // Build args JSON via serde_json so the embedded script's double
        // quotes are properly escaped in the JSON string column.
        let args_json = serde_json::json!(["-c", mock_script]).to_string();

        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = McpServerRecord {
            server_id: "mock-python".to_string(),
            name: "Mock Python".to_string(),
            version: "1.0.0".to_string(),
            transport: "stdio".to_string(),
            enabled: true,
            trusted: false,
            protocol_version: Some("2025-11-25".to_string()),
            allowed_origins: None,
            allowed_paths: None,
            command: Some("python".to_string()),
            args: Some(args_json),
            env: Some("{}".to_string()),
        };
        McpServerRepo::new()
            .create(&kernel.conn(), &rec)
            .expect("insert must succeed");

        let result = invoke_mcp_tool(
            &kernel,
            "mock-python",
            "echo",
            serde_json::json!({"msg": "hello"}),
        )
        .expect("invoke_mcp_tool must succeed");

        assert_eq!(result.status, ToolStatus::Succeeded);
        // The mock echoes arguments.msg back as {"echo": <msg>}. This
        // assertion is the load-bearing check that args were propagated.
        assert_eq!(result.data, serde_json::json!({"echo": "hello"}));
        assert!(
            result.idempotency_key.starts_with("mcp-"),
            "idempotency_key must start with 'mcp-', got: {}",
            result.idempotency_key
        );
        assert!(result.error_code.is_none());
        assert!(result.started_at <= result.finished_at);
    }
}
