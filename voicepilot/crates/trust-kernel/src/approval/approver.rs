//! Approver trait — V1.1 §6.2 approve phase.
//!
//! UI-agnostic: CLI implements it with stdin; Tauri implements it with
//! an IPC call to the approval window. The Skill executor calls
//! `approver.prompt(manifest)` between prepare and commit.
//!
//! W8 Plan 2: 扩展 `approve_dag_skeleton(&DagPlan) -> Result<ApprovalDecision>`
//! 方法,用于 DAG 骨架一次性审批(决策 #2)。两方法共存:
//!
//!   1. `approve_dag_skeleton` — DAG 节点执行前,展示骨架 + Allow/Deny
//!   2. `prompt` — 每个节点 prepare→commit 之间的细粒度审批
//!
//! Allow(DAG 骨架) + Allow(每步 prepare) 才进入 commit;任一 Deny 短路。

use crate::approval::types::ApprovalDecision;
use crate::error::Result;
use crate::policy::transaction::EffectManifest;
use crate::skills::dag_types::DagPlan;

/// W9 Plan 4:DAG 骨架审批的完整决策结果。
///
/// 与 `ApprovalDecision` 区别:`ApprovalDecision` 用于单步 prepare→commit
/// 审批(`Approver::prompt`),Modify 是占位(W8 未实现 payload 回传);
/// `DagApprovalOutcome` 用于 DAG 骨架审批(`Approver::approve_dag_skeleton`),
/// Modify 携带完整 `modified_plan: Box<DagPlan>`(W9 Plan 4 实现)。
///
/// 用 `Box<DagPlan>` 避免枚举 size 爆炸(`DagPlan` 含 Vec + HashMap,栈上 size 大)。
#[derive(Debug, Clone)]
pub enum DagApprovalOutcome {
    Allow,
    Deny,
    /// 用户调整 DAG 骨架后回传的 modified_plan。
    /// 由 `DagExecutor::run` 处理:审计 `dag_skeleton_modified` →
    /// `SlotTemplateEngine::validate_dag` 重新校验 → `run_modified` 第二次审批。
    Modify { modified_plan: Box<DagPlan> },
}

impl DagApprovalOutcome {
    /// 返回标准字符串(供审计 details 字段使用,替换 W8 的 `format!("{:?}", decision)`)。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Modify { .. } => "modify",
        }
    }
}

/// Callback the Skill executor invokes between prepare and commit.
///
/// W8 Plan 2: `approve_dag_skeleton` 是 DAG 骨架层审批;`prompt` 是
/// 节点级 prepare→commit 审批。两层审批独立,各自 Allow/Deny。
pub trait Approver: Send + Sync {
    /// Show the effect_manifest to the user and return their decision.
    /// May block (CLI stdin) or return immediately (auto-approve / auto-deny).
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision;

    /// W8 Plan 2: DAG 骨架审批(决策 #2)。
    ///
    /// 在 DagExecutor::run 调用 topological_sort 之后、节点执行之前触发。
    /// 实现:
    ///
    ///   - AutoApprover → Ok(Allow)(测试 / headless)
    ///   - AutoDenier → Ok(Deny)(负路径测试)
    ///   - TauriApprover(ui crate)→ 真实 IPC 弹窗(W6 既有 channel,5min timeout → Deny)
    ///
    /// 返回 `Result<DagApprovalOutcome>`(W9 Plan 4:含 Modify payload)而非
    /// `ApprovalDecision`(单步审批用)是让 TauriApprover 能区分"通道失败"
    /// 与"用户 Deny"(前者可作为 Err 向上传播,后者是用户意图)。
    /// AutoApprover / AutoDenier 始终返回 Ok。
    fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<DagApprovalOutcome>;
}

/// Auto-approver for tests and headless runs. Always returns Allow.
pub struct AutoApprover;

impl Approver for AutoApprover {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Allow
    }

    fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<DagApprovalOutcome> {
        Ok(DagApprovalOutcome::Allow)
    }
}

/// Auto-denier for negative-path tests.
pub struct AutoDenier;

impl Approver for AutoDenier {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Deny
    }

    fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<DagApprovalOutcome> {
        Ok(DagApprovalOutcome::Deny)
    }
}
