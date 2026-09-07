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
use crate::mcp::client::{McpCallLimits, McpClient};
use crate::mcp::repo::McpServerRepo;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::StepStatus;
use crate::skills::manifest::{SkillInput, SkillInputType, SkillManifest};
use crate::toolresult::{EvidenceStrength, ToolResult, ToolStatus};
use crate::tools::fs_paths::canonicalize;

/// 进程级测试锁：串行化所有改进程 CWD 的测试。
///
/// CWD 是进程全局的；note_capture / research_save 各自 `mod tests` 里
/// 的私有 `CWD_MUTEX` 只能串行化本模块，并行跑跨模块测试仍互踩
/// （`with_temp_cwd` 改 CWD 期间另一模块写相对路径文件即 ENOENT）。
/// 所有改 CWD 的测试统一拿这把锁。仅测试编译（`#[cfg(test)]`），
/// 生产代码零影响。
#[cfg(test)]
pub(crate) static TEST_CWD_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};

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
    let reverse_payload_json = serde_json::json!({
        "moves": moved_paths.iter().map(|(orig, curr)| {
            serde_json::json!({
                "from": orig.to_string_lossy().replace('\\', "/"),
                "to":   curr.to_string_lossy().replace('\\', "/"),
            })
        }).collect::<Vec<_>>()
    })
    .to_string();
    persist_compensation_record(
        kernel,
        step_id,
        compensate_fn,
        reverse_payload_json,
        level,
        conflict_policy,
        ttl_seconds,
    )
}

/// W10 Plan 2: Create a post-commit CompensationRecord with an arbitrary JSON
/// payload string. Used by Skills whose reverse operation is not move-based
/// (e.g. note.reverse_capture deletes a file, form.reverse_prepare clears
/// form fields via Playwright).
///
/// `reverse_payload_json` should be a valid JSON string (e.g.
/// `{"save_path": "..."}` or `{"fields": {...}}`). The reverse function is
/// responsible for parsing and validating the payload.
pub fn create_post_commit_compensation_with_payload(
    kernel: &TrustKernel,
    step_id: &str,
    compensate_fn: &str,
    reverse_payload_json: String,
    level: CompensationLevel,
    conflict_policy: ConflictPolicy,
    ttl_seconds: i64,
) -> Result<String> {
    persist_compensation_record(
        kernel,
        step_id,
        compensate_fn,
        reverse_payload_json,
        level,
        conflict_policy,
        ttl_seconds,
    )
}

/// Private helper: persist a CompensationRecord with the given JSON payload.
/// Handles stronghold encryption (3 branches per W9 Plan 2) + DB insert.
///
/// Branches:
/// 1. stronghold_enabled + vault unlocked → encrypt, snapshot_encrypted=Some(bincode),
///    reverse_payload="" (plaintext not persisted).
/// 2. stronghold_enabled + vault not unlocked → degraded mode, plaintext persisted.
/// 3. stronghold disabled → plaintext PoC (W3a behavior).
fn persist_compensation_record(
    kernel: &TrustKernel,
    step_id: &str,
    compensate_fn: &str,
    reverse_payload_json: String,
    level: CompensationLevel,
    conflict_policy: ConflictPolicy,
    ttl_seconds: i64,
) -> Result<String> {
    let comp_id = format!("comp-{}", uuid::Uuid::new_v4());

    // W9 Plan 2: Stronghold 加密 reverse_payload(spec §2.2 三分支)。
    #[cfg(feature = "stronghold")]
    let (snapshot_encrypted, snapshot_vault_ref, stored_reverse_payload) =
        if kernel.stronghold_enabled() {
            let vault_opt = kernel.stronghold_vault();
            if let Some(vault) = vault_opt {
                if vault.is_unlocked() {
                    let payload = vault
                        .encrypt(reverse_payload_json.as_bytes())
                        .map_err(|e| {
                            KernelError::Compensation(format!("stronghold encrypt failed: {}", e))
                        })?;
                    let payload_bytes = bincode::serialize(&payload).map_err(|e| {
                        KernelError::Compensation(format!("bincode serialize failed: {e}"))
                    })?;
                    let vault_ref = uuid::Uuid::new_v4().to_string();
                    let plaintext_len = reverse_payload_json.len();
                    let task_id = kernel.task_id_for_step(step_id)?.ok_or_else(|| {
                        KernelError::Compensation(format!("task_id not found for step {}", step_id))
                    })?;
                    kernel.audit_append_external(
                        &task_id,
                        Some(step_id),
                        "stronghold_snapshot_encrypted",
                        serde_json::json!({
                            "compensation_id": comp_id,
                            "vault_ref": vault_ref,
                            "plaintext_len": plaintext_len,
                        }),
                    )?;
                    (Some(payload_bytes), Some(vault_ref), String::new())
                } else {
                    (None, Some("degraded".to_string()), reverse_payload_json)
                }
            } else {
                (None, Some("degraded".to_string()), reverse_payload_json)
            }
        } else {
            (None, None, reverse_payload_json)
        };

    #[cfg(not(feature = "stronghold"))]
    let (snapshot_encrypted, snapshot_vault_ref, stored_reverse_payload) =
        (None, None, reverse_payload_json);

    let record = CompensationRecord {
        comp_id: comp_id.clone(),
        step_id: step_id.to_string(),
        level,
        snapshot_encrypted,
        ttl_expires: (Utc::now() + chrono::Duration::seconds(ttl_seconds)).to_rfc3339(),
        status: "active".to_string(),
        snapshot_vault_ref,
        conflict_policy,
        compensate_fn: compensate_fn.to_string(),
        reverse_payload: stored_reverse_payload,
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

    // Shared lookup inside a block scope so the MutexGuard is dropped before
    // we spawn the subprocess (avoids holding the DB lock across slow MCP I/O).
    let (command, args_vec, env_json) = {
        let conn = kernel.conn();
        McpServerRepo::new().spawn_config(&conn, server_id)?
    };
    // Phase A 密钥收编：env 里的 keyring 引用在 spawn 前解析为真实值；
    // 明文永不落 DB（导入时已收编进 SecretStore）。
    let env_json = resolve_mcp_env(kernel, &env_json)?;

    // Spawn the MCP subprocess, run the initialize handshake, and invoke
    // the tool under fixed call limits (timeout + max output bytes). All
    // errors here are already KernelError::Mcp(...).
    let result = run_mcp_call_limited(
        server_id,
        &command,
        &args_vec,
        &env_json,
        tool_name,
        args,
        McpCallLimits::default(),
    )?;

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

/// Phase A 密钥收编：外部 MCP env 凭据在 keyring 里的引用前缀。
/// 完整引用名 = `{prefix}{server_id}/{ENV_KEY}`，与 LLM API key 同一
/// service（Windows Credential Manager "voicepilot"）不同 user。
pub const MCP_ENV_KEYRING_PREFIX: &str = "voicepilot/mcp/";

/// Phase A 密钥收编：外部 skill 伴随 .env 凭据在 keyring 里的引用前缀。
pub const MCP_SKILL_ENV_KEYRING_PREFIX: &str = "voicepilot/skill/";

/// Phase A 密钥收编：解析 mcp_servers.env JSON 中的 keyring 引用。
///
/// 导入外部 MCP 时，疑似凭据的 env 值已迁移进 SecretStore，DB 里只存
/// 引用 `{"$keyring": "voicepilot/mcp/<server_id>/<KEY>"}`。本函数在
/// spawn 前把引用换回真实值；引用缺失（keyring 里没有）fail-closed 报错，
/// 绝不带空值拉起子进程。非引用值原样透传。
///
/// 错误信息只含 key 名，绝不含 secret 值（SecretStore 契约）。
pub fn resolve_mcp_env(
    kernel: &TrustKernel,
    env_json: &serde_json::Value,
) -> Result<serde_json::Value> {
    let Some(obj) = env_json.as_object() else {
        return Ok(env_json.clone());
    };
    let mut out = serde_json::Map::new();
    for (k, v) in obj {
        // Phase A 收编后的引用形态为字符串 `"$keyring:<name>"`（满足 register
        // 的 string→string env 校验；早期实现曾用 `{"$keyring": name}` 对象，
        // 兼容读取旧行以防已有 DB 数据）。
        let keyring_name = v
            .as_str()
            .and_then(|s| s.strip_prefix("$keyring:"))
            .or_else(|| {
                v.as_object()
                    .and_then(|o| o.get("$keyring"))
                    .and_then(|s| s.as_str())
            });
        match keyring_name {
            Some(name) => {
                let value = kernel.secret_store().get_secret(name)?.ok_or_else(|| {
                    KernelError::Mcp(format!(
                        "env key '{k}' references missing keyring secret '{name}'"
                    ))
                })?;
                out.insert(k.clone(), serde_json::Value::String(value));
            }
            None => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(serde_json::Value::Object(out))
}

/// Phase A 密钥收编：判断 env key 是否疑似凭据（导入时迁移进 keyring）。
/// 大小写不敏感；命中规则：含 KEY / TOKEN / SECRET / PASSWORD / PASSWD。
pub fn is_credential_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD"]
        .iter()
        .any(|marker| upper.contains(marker))
}

/// Reject tool results whose serialized size exceeds the call limit.
///
/// Public so tests (and future callers) can unit-test the boundary check
/// directly. Returns `Err(KernelError::Mcp(...))` mentioning the output
/// limit when `value` serializes to more than `max_output_bytes`.
pub fn enforce_mcp_output_limit(value: &serde_json::Value, max_output_bytes: u64) -> Result<()> {
    let bytes = serde_json::to_vec(value)?.len() as u64;
    if bytes > max_output_bytes {
        return Err(KernelError::Mcp(format!(
            "MCP tool result exceeds output limit: {bytes} bytes > {max_output_bytes} bytes"
        )));
    }
    Ok(())
}

/// Run one MCP call (spawn + initialize + tools/call) under `McpCallLimits`.
///
/// The subprocess lifecycle runs on a worker thread so a stuck MCP server
/// cannot block the caller beyond `limits.timeout`; the result is then
/// checked against `max_output_bytes` before it leaves the MCP boundary.
///
/// The worker publishes the spawned `Child` into a shared slot the moment it
/// exists; if the caller's `recv_timeout` fires first it kills and reaps the
/// child there, so the leak is bounded by reaping rather than by the child
/// eventually writing/EOF and the client dropping.
pub(crate) fn run_mcp_call_limited(
    server_id: &str,
    command: &str,
    args: &[String],
    env: &serde_json::Value,
    tool_name: &str,
    tool_args: serde_json::Value,
    limits: McpCallLimits,
) -> Result<serde_json::Value> {
    let (tx, rx) = std::sync::mpsc::channel();
    let command = command.to_string();
    let args = args.to_vec();
    let env = env.clone();
    let server_id = server_id.to_string();
    let tool_name = tool_name.to_string();
    // Worker-side clones for the late-result warn (the originals stay in
    // this thread for the timeout / output-limit error messages).
    let worker_server_id = server_id.clone();
    let worker_tool_name = tool_name.clone();
    // Shared slot for the spawned subprocess: the worker publishes the Child
    // here right after spawn; the caller reaps it on timeout, so a hung
    // server cannot outlive this call.
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let worker_slot = child_slot.clone();
    std::thread::spawn(move || {
        let outcome = (|| -> Result<serde_json::Value> {
            let mut client = McpClient::spawn_into(&command, &args, &env, worker_slot)?;
            client.initialize()?;
            client.invoke_tool(&worker_tool_name, tool_args)
        })();
        // The caller may already have timed out and dropped the receiver. A
        // slow-but-finished call whose side effects already ran must stay
        // auditable, not silently dropped.
        if let Err(send_err) = tx.send(outcome) {
            tracing::warn!(
                server_id = %worker_server_id,
                tool_name = %worker_tool_name,
                late_outcome = ?send_err.0,
                "MCP call finished after caller timed out; result dropped, side effects (if any) already executed"
            );
        }
    });
    let inner = rx.recv_timeout(limits.timeout).map_err(|e| match e {
        RecvTimeoutError::Timeout => {
            // Drop the receiver first so a late worker send fails and is
            // logged by the worker's tracing::warn! above; then reap the
            // subprocess instead of leaving it running.
            drop(rx);
            reap_shared_child(&child_slot);
            KernelError::Mcp(format!(
                "MCP tool call '{tool_name}' on server '{server_id}' exceeded timeout of {}s",
                limits.timeout.as_secs()
            ))
        }
        // The worker thread panicked (or the channel was dropped) before
        // responding — distinct from a timeout: no result will ever arrive.
        RecvTimeoutError::Disconnected => {
            reap_shared_child(&child_slot);
            KernelError::Mcp(format!(
                "MCP worker for tool '{tool_name}' on server '{server_id}' disconnected (worker thread panicked or channel dropped)"
            ))
        }
    })?;
    let result = inner?;
    enforce_mcp_output_limit(&result, limits.max_output_bytes).map_err(|e| {
        KernelError::Mcp(format!(
            "MCP tool call '{tool_name}' on server '{server_id}': {e}"
        ))
    })?;
    Ok(result)
}

/// Kill and reap the shared MCP subprocess if it is still alive.
///
/// Kill errors are ignored: the child may already have exited on its own
/// (or been reaped by the worker's client Drop).
fn reap_shared_child(child_slot: &Arc<Mutex<Option<Child>>>) {
    if let Some(child) = child_slot.lock().unwrap().as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
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
            if !spec.allowed_values.is_empty() && !spec.allowed_values.iter().any(|v| v == &s) {
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

    // ===== Phase A 密钥收编：resolve_mcp_env / is_credential_env_key =====

    #[test]
    fn credential_key_heuristics() {
        assert!(is_credential_env_key("CONTEXT7_API_KEY"));
        assert!(is_credential_env_key("github_token"));
        assert!(is_credential_env_key("ClientSecret"));
        assert!(is_credential_env_key("MY_PASSWORD"));
        assert!(!is_credential_env_key("BASE_URL"));
        assert!(!is_credential_env_key("REGION")); // 字面无凭据标记
    }

    #[test]
    fn resolve_mcp_env_resolves_and_fails_closed_on_missing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let store = kernel.secret_store();
        store
            .set_secret("voicepilot/mcp/ctx7/CONTEXT7_API_KEY", "kcy_123")
            .unwrap();
        let env = serde_json::json!({
            "CONTEXT7_API_KEY": "$keyring:voicepilot/mcp/ctx7/CONTEXT7_API_KEY",
            "BASE_URL": "https://api.x.com",
        });
        let out = resolve_mcp_env(&kernel, &env).unwrap();
        assert_eq!(out["CONTEXT7_API_KEY"], "kcy_123");
        assert_eq!(out["BASE_URL"], "https://api.x.com");
        // 早期对象形式（{"$keyring": name}）兼容读取。
        let legacy = serde_json::json!({
            "K": {"$keyring": "voicepilot/mcp/ctx7/CONTEXT7_API_KEY"},
        });
        assert_eq!(resolve_mcp_env(&kernel, &legacy).unwrap()["K"], "kcy_123");
        // 引用缺失 → fail-closed 报错（绝不带空值 spawn）。
        let missing = serde_json::json!({
            "K": "$keyring:voicepilot/mcp/nope/K",
        });
        let err = resolve_mcp_env(&kernel, &missing).unwrap_err().to_string();
        assert!(err.contains("missing keyring secret"), "got: {err}");
        // 非对象 env 原样透传。
        assert_eq!(
            resolve_mcp_env(&kernel, &serde_json::json!(null)).unwrap(),
            serde_json::json!(null)
        );
    }

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
        let manifest =
            build_manifest_with_one_input("url", SkillInputType::Url, true, vec![], vec![], None);
        let mut input = HashMap::new();
        input.insert("url".to_string(), serde_json::json!("ftp://example.com"));
        let err = validate_input_against_manifest(&input, &manifest).unwrap_err();
        assert!(err.to_string().contains("http"));
    }

    #[test]
    fn validate_url_https_ok() {
        let manifest =
            build_manifest_with_one_input("url", SkillInputType::Url, true, vec![], vec![], None);
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
        input.insert(
            "destination".to_string(),
            serde_json::json!("Workspace/papers"),
        );
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
        kernel
            .update_step_status("s1", StepStatus::Running)
            .unwrap();
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
        kernel
            .update_step_status("s1", StepStatus::Running)
            .unwrap();
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
        kernel
            .update_step_status("s1", StepStatus::Running)
            .unwrap();
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
        kernel
            .update_step_status("s1", StepStatus::Running)
            .unwrap();
        let manifest = blank_effect_manifest();
        let approver = AutoDenier;
        let preconditions_hash = "sha256:fake".to_string();
        let ctx = approval_ctx("t1", "s1", &preconditions_hash);

        let approval = record_approval_decision(&kernel, &approver, &manifest, &ctx).unwrap();
        assert_eq!(approval.user_decision, ApprovalDecision::Deny);

        // On Deny: cancel the step, do NOT create compensation.
        kernel
            .update_step_status("s1", StepStatus::Cancelled)
            .unwrap();

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
        let result = invoke_mcp_tool(&kernel, "nonexistent-server", "echo", serde_json::json!({}));
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
        let result = invoke_mcp_tool(&kernel, "disabled-server", "echo", serde_json::json!({}));
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
        let result = invoke_mcp_tool(&kernel, "no-cmd-server", "echo", serde_json::json!({}));
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
        let python_probe = std::process::Command::new("python")
            .arg("--version")
            .output();
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

    // ===== W10 Plan 2: create_post_commit_compensation_with_payload tests =====

    #[test]
    fn create_post_commit_compensation_with_payload_persists_custom_json() {
        let kernel = setup_kernel_with_step();
        let payload = serde_json::json!({
            "save_path": "Documents/note-abc.txt"
        })
        .to_string();
        let comp_id = create_post_commit_compensation_with_payload(
            &kernel,
            "s1",
            "note.reverse_capture",
            payload.clone(),
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
        assert_eq!(comp.compensate_fn, "note.reverse_capture");
        assert_eq!(comp.reverse_payload, payload);
    }

    #[test]
    fn create_post_commit_compensation_with_payload_empty_string_ok() {
        // 空 payload 字符串允许(某些 reverse fn 可能不需要参数),
        // 但 reverse 函数自身负责校验 payload 非空。
        let kernel = setup_kernel_with_step();
        let comp_id = create_post_commit_compensation_with_payload(
            &kernel,
            "s1",
            "custom.reverse",
            String::new(),
            CompensationLevel::BestEffort,
            ConflictPolicy::Fail,
            60,
        )
        .unwrap();
        let comp = kernel.get_compensation(&comp_id).unwrap().unwrap();
        assert_eq!(comp.reverse_payload, "");
    }
}
