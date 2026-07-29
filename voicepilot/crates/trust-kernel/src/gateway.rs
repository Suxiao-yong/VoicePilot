//! Action Gateway facade — V1.1 §4.2, §4.4, §6.2.
//!
//! Orchestrates the full policy pipeline per §4.4:
//!   1. Hard-deny rules (D3, shell, taint elevation).
//!   2. Cedar authorization (binary allow/deny).
//!   3. Rust Constraint Engine (args normalization + constraints).
//!   4. E×D risk classification (ternary allow/confirm/deny).
//!   5. Egress check (if data flows to remote destination).
//!   6. Return Decision with effect + reasons + matched_policies +
//!      normalized_args + constraints_applied + approval_scope + policy_bundle_hash.
//!
//! W2 skips §4.4 step 1 (resource path canonicalization) — args normalization
//! in step 3 covers the W2 tool surface. W3 will add resource.path canonicalization
//! when real filesystem tools land.

use crate::error::{KernelError, Result};
use crate::policy::cedar_engine::CedarEngine;
use crate::policy::constraint_engine::{ConstraintEngine, ConstraintSpec};
use crate::policy::egress::check_egress;
use crate::policy::taint_repo::TaintRepo;
use crate::policy::types::{Action, Decision, EgressDest, Resource, Effect};
use rusqlite::Connection;
use std::sync::Arc;

pub struct ActionGateway {
    cedar: CedarEngine,
    constraints: ConstraintEngine,
    bundle_hash: String,
    bundle_src: Arc<String>,
}

impl ActionGateway {
    /// Build a gateway from a Cedar source string.
    pub fn new(cedar_src: &str) -> Result<Self> {
        Ok(Self {
            cedar: CedarEngine::from_source(cedar_src)?,
            constraints: ConstraintEngine::new(),
            bundle_hash: CedarEngine::bundle_hash(cedar_src),
            bundle_src: Arc::new(cedar_src.to_string()),
        })
    }

    /// Register a per-tool Rust constraint.
    pub fn register_constraint(&mut self, tool: &str, spec: ConstraintSpec) {
        self.constraints.register(tool, spec);
    }

    /// The SHA-256 hash of the active Cedar policy bundle.
    pub fn bundle_hash(&self) -> &str {
        &self.bundle_hash
    }

    /// Full policy decision pipeline.
    ///
    /// `egress_dest`: Some(_) if the tool sends data to a remote destination.
    /// `args`: tool arguments (will be normalized + constrained).
    pub fn decide(
        &self,
        tool: &str,
        e_level: crate::policy::types::ELevel,
        resource: &Resource,
        egress_dest: Option<EgressDest>,
        args: Option<serde_json::Value>,
    ) -> Result<Decision> {
        let action = Action { name: tool.to_string(), e_level };
        let mut reasons: Vec<String> = Vec::new();

        // Step 1: hard-deny rules (D3, shell_exec, taint elevation).
        if resource.data_class == crate::policy::types::DLevel::D3 {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "D3 red line: credentials never enter model context",
            ));
        }
        if tool == "shell_exec" {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "shell_exec always denied",
            ));
        }
        // W9 Plan 3: 硬编码 web_page taint 规则已外移到独立函数 `check_taint_policy`。
        // `decide` 不再调 `check_taint_policy`(gateway 不持有 DB 连接);
        // 由调用方(dispatcher / invoke_mcp_tool / filesystem 工具函数)在需要时
        // 主动调 `check_taint_policy(&kernel.conn(), value_hash, egress_dest)`。
        // web_page → ToolArgument 的拦截由 `check_egress(_, ToolArgument) = Deny`
        // 在 Step 5 兜底(任何 data_class → ToolArgument 都 Deny)。

        // Step 2: Cedar authorization.
        let cedar_allows = self.cedar.is_allowed(&action, resource)?;
        if !cedar_allows {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "Cedar denied",
            ));
        }

        // Step 3: normalize args + apply constraints.
        let (normalized_args, constraints_applied) = if let Some(args) = args {
            let normalized = self.constraints.normalize_args(tool, &args)?;
            let (constrained, applied) = self.constraints.apply_constraints(tool, normalized)?;
            (constrained, applied)
        } else {
            (serde_json::Value::Null, vec![])
        };

        // Step 4: E×D risk classification.
        let mut effect = self.constraints.upgrade_effect(cedar_allows, e_level, resource.data_class);
        if matches!(effect, Effect::Deny) {
            reasons.push(format!("E{:?}×D{:?} = deny", e_level, resource.data_class));
        } else if matches!(effect, Effect::Confirm) {
            reasons.push(format!("E{:?}×D{:?} = confirm", e_level, resource.data_class));
        }

        // Step 5: egress check.
        // For remote destinations (RemoteLlm/RemoteMcp/ToolArgument), egress
        // overrides E×D — §4.3 is the specific rule for egress flows.
        // For LocalFile, egress returns Allow as a placeholder ("E×D covers it"
        // per egress.rs), so more_restrictive preserves the E×D decision.
        if let Some(dest) = egress_dest {
            let egress_effect = check_egress(resource.data_class, dest);
            effect = if dest == EgressDest::LocalFile {
                more_restrictive(effect, egress_effect)
            } else {
                egress_effect
            };
            if matches!(egress_effect, Effect::Deny) {
                reasons.push(format!("egress to {:?} denied for D{:?}", dest, resource.data_class));
            } else if matches!(egress_effect, Effect::Confirm) {
                reasons.push(format!("egress to {:?} requires confirm for D{:?}", dest, resource.data_class));
            }
        }

        // Step 6: assemble Decision.
        // W2: approval_scope is always "single"; batch scope lands in W3 (§8.1).
        let matched_policies = self.cedar.matched_policy_ids(&action, resource)?;
        let approval_scope = "single";

        Ok(Decision {
            effect,
            matched_policies,
            normalized_args,
            constraints_applied,
            approval_scope: approval_scope.to_string(),
            policy_bundle_hash: self.bundle_hash.clone(),
            reasons,
        })
    }

    /// Reference to the original Cedar source (for audit / persistence).
    pub fn cedar_source(&self) -> &str {
        &self.bundle_src
    }
}

fn more_restrictive(a: Effect, b: Effect) -> Effect {
    use Effect::*;
    match (a, b) {
        (Deny, _) | (_, Deny) => Deny,
        (Confirm, _) | (_, Confirm) => Confirm,
        (Allow, Allow) => Allow,
    }
}

/// W9 Plan 3: 查表驱动的 taint 策略检查(spec §2.3 + §6.2)。
///
/// 独立函数(非 `ActionGateway` 方法),因为 `ActionGateway` 不持有 DB 连接。
/// 由调用方(dispatcher / `invoke_mcp_tool` / filesystem 工具函数)在需要时
/// 主动传入 `&Connection`。`gateway.decide` 内部不调此函数。
///
/// 规则:
///   - `web_page` taint 不能成为 `ToolArgument`(防止 LLM 投毒)
///   - `llm_output` taint 不能写入文件系统(`LocalFile`,防止 LLM 注入恶意路径)
///
/// `value_hash` 由调用方从 resource.path 或实际 value 计算(SHA256 hex)。
/// 返回 `Ok(())` 表示放行,返回 `Err(KernelError::TaintPropagationBlocked)`
/// 表示拦截(调用方负责审计 `taint_blocked` 事件,见 Task 7)。
pub fn check_taint_policy(
    conn: &Connection,
    value_hash: &str,
    egress_dest: EgressDest,
) -> Result<()> {
    let taints: Vec<String> = TaintRepo::new()
        .find_by_hash(conn, value_hash)?
        .map(|r| r.taints)
        .unwrap_or_default();

    // 规则 1:web_page taint 不能成为 ToolArgument(防止 LLM 投毒)。
    if taints.iter().any(|t| t == "web_page") && egress_dest == EgressDest::ToolArgument {
        return Err(KernelError::TaintPropagationBlocked {
            taints,
            sink: format!("{:?}", egress_dest),
        });
    }

    // 规则 2:llm_output taint 不能写入文件系统(LocalFile)。
    if taints.iter().any(|t| t == "llm_output") && egress_dest == EgressDest::LocalFile {
        return Err(KernelError::TaintPropagationBlocked {
            taints,
            sink: format!("{:?}", egress_dest),
        });
    }

    Ok(())
}

/// W9 Plan 3 Task 7: 封装 `check_taint_policy` + `taint_blocked` 审计发射。
///
/// 调用方(invoke_mcp_tool / filesystem 工具函数 / dispatcher 等)在需要
/// 检查 taint 策略且持有 task_id + step_id 上下文时调本函数。本函数:
///   1. 获取 conn 锁,调 `check_taint_policy` 查表判定
///   2. 释放 conn 锁(避免 reentrancy deadlock,因为 `audit_append_external`
///      会重新获取同一锁;见 project_memory.md "Mutex reentrancy deadlock")
///   3. 若被拦截,调 `kernel.audit_append_external` 记录 `taint_blocked` 事件
///      (details 仅含 taints + sink + resource_hash,不含原始 value,spec §6.2)
///   4. 返回原 `Result`,调用方可继续传播 `TaintPropagationBlocked` 错误
///
/// `task_id` 必须是已存在的任务 ID(`audit_logs.task_id` FK 约束)。
/// `step_id` 可选(`audit_logs.step_id` NULLABLE,但若提供必须存在)。
pub fn check_taint_policy_and_audit(
    kernel: &crate::kernel::TrustKernel,
    task_id: &str,
    step_id: Option<&str>,
    value_hash: &str,
    egress_dest: EgressDest,
) -> Result<()> {
    // 持锁查 taint,然后释放锁再审计(避免 reentrancy deadlock)。
    let result = {
        let conn = kernel.conn();
        check_taint_policy(&conn, value_hash, egress_dest)
    };

    if let Err(KernelError::TaintPropagationBlocked { taints, sink }) = &result {
        kernel.audit_append_external(
            task_id,
            step_id,
            "taint_blocked",
            serde_json::json!({
                "taints": taints,
                "sink": sink,
                "resource_hash": value_hash,
            }),
        )?;
    }

    result
}
