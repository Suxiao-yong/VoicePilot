//! DagExecutor — W8 §2.3.
//!
//! DAG 调度器:接收 LLM 拆解的 DagPlan,按拓扑序串行执行简单节点
//! (循环节点在 Plan 3 实现),失败时前面已 commit 不回滚(决策 #4),
//! DAG 终态为 Succeeded / Failed / PartiallySucceeded / Cancelled。
//!
//! 调度算法(spec §2.3):
//!
//!   Step 1: topological_sort(Kahn 算法)→ Vec<node_id>(检测隐式环)
//!   Step 2: approver.approve_dag_skeleton(plan) → Allow/Deny
//!           Deny → DagResult::cancelled()(0 节点执行)
//!   Step 3: 按拓扑序 for each node:
//!             - loop_specs 含 → run_loop_node(Plan 3;本 plan 返回 Err)
//!             - 否则 → run_simple_node(Task 4 实现)
//!           失败处理(决策 #4 + #8):节点 Failed 时,
//!             - 若有已成功节点 → DagStatus::PartiallySucceeded
//!             - 否则 → DagStatus::Failed
//!           不回滚前序已 commit 节点。
//!   Step 4: 全部成功 → DagStatus::Succeeded
//!
//! 状态持久化:每个节点 start/success/fail 时调 DagRepo.update_node_status,
//! 整个 plan 完成时调 DagRepo.update_plan_status。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::approval::approver::Approver;
use crate::approval::approver::DagApprovalOutcome;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::llm::types::ExtractedSlot;
use crate::skills::dag_repo::DagRepo;
use crate::skills::dag_types::{DagEdge, DagNode, DagNodeStatus, DagPlan, DagResult, DagStatus};
use crate::skills::dispatcher::dispatch_skill_executor;
use crate::skills::template::SlotTemplateEngine;

// W9 Plan 6 Task 6:thread-local UiaAdapter 注入点。
// UiaAdapter 是 !Send + !Sync(COM apartment 模型),不能用 Arc<dyn UiaAdapter>
// 作为 DagExecutor 字段(DagExecutor 需跨 await 点)。改用 thread-local:
// 测试在 run() 前调 set_thread_local_uia_adapter(Some(adapter)),
// dispatch_note_capture / dispatch_app_control 从 thread-local 取 adapter
// 调真实 executor。
//
// W9 修复:仅 `uia` feature 启用时编译(uiautomation 模块只在
// `cfg(all(windows, feature = "uia"))` 下存在,见 lib.rs:34)。
#[cfg(all(windows, feature = "uia"))]
thread_local! {
    static THREAD_LOCAL_UIA_ADAPTER: std::cell::RefCell<Option<Arc<dyn crate::uiautomation::UiaAdapter>>> =
        std::cell::RefCell::new(None);
}

/// W9 Plan 6 Task 6:在当前线程设置 thread-local UiaAdapter。
///
/// 测试在 `executor.run(...)` 前调 `set_thread_local_uia_adapter(Some(adapter))`,
/// `run(...)` 后调 `set_thread_local_uia_adapter(None)` 清理。
///
/// W9 修复:可见性为 `pub`(非 `pub(crate)`),供集成测试
/// (tests/w9_plan6_uia_dag_e2e.rs)从 crate 外部调用注入 adapter。
#[cfg(all(windows, feature = "uia"))]
pub fn set_thread_local_uia_adapter(
    adapter: Option<Arc<dyn crate::uiautomation::UiaAdapter>>,
) {
    THREAD_LOCAL_UIA_ADAPTER.with(|cell| {
        *cell.borrow_mut() = adapter;
    });
}

/// W9 Plan 6 Task 6:从当前线程获取 thread-local UiaAdapter 克隆(若已设置)。
#[cfg(all(windows, feature = "uia"))]
pub fn thread_local_uia_adapter() -> Option<Arc<dyn crate::uiautomation::UiaAdapter>> {
    THREAD_LOCAL_UIA_ADAPTER.with(|cell| cell.borrow().clone())
}

/// DAG 调度器。
///
/// 持有 `Arc<TrustKernel>`(TrustKernel 不是 Clone,见 project_memory.md)
/// 和 `Arc<dyn Approver>`(动态分发,支持 AutoApprover / TauriApprover)。
/// `DagRepo` 是无状态 accessor,每次 new() 一个新实例。
pub struct DagExecutor {
    kernel: Arc<TrustKernel>,
    approver: Arc<dyn Approver>,
    dag_repo: Arc<DagRepo>,
}

impl DagExecutor {
    /// 构造器。
    ///
    /// 参数:
    ///
    /// - `kernel`:Arc<TrustKernel>(调用方持 Arc,DagExecutor clone 一份)
    /// - `approver`:Arc<dyn Approver>(骨架审批 + 节点级审批都用同一个)
    /// - `dag_repo`:Arc<DagRepo>(无状态,可共享)
    pub fn new(
        kernel: Arc<TrustKernel>,
        approver: Arc<dyn Approver>,
        dag_repo: Arc<DagRepo>,
    ) -> Self {
        Self {
            kernel,
            approver,
            dag_repo,
        }
    }

    /// 暴露内部 DagRepo 引用(供集成测试验证 DB 持久化)。
    /// DagRepo 是无状态 accessor,共享引用不会引入耦合。
    pub fn dag_repo(&self) -> &DagRepo {
        &self.dag_repo
    }

    /// 主入口 — 调度一个 DagPlan。
    ///
    /// 算法见模块注释。返回 `Result<DagResult>`:
    ///
    /// - Ok(DagResult) — DAG 执行完成(无论 Succeeded / Failed / PartiallySucceeded / Cancelled)
    /// - Err(KernelError) — 不可恢复错误(拓扑环 / DB 故障 / 模板校验失败)
    ///
    /// 注意:节点级 executor 失败不返回 Err,而是构造 `DagResult` with
    /// `DagStatus::Failed` / `PartiallySucceeded`,让调用方从 result 判断状态。
    pub fn run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot]) -> Result<DagResult> {
        // Step 0a: 创建 root task(供 audit_logs.task_id FK + dag_plans.root_task_id 关联)
        // spec §6.1 Task 7 Step 1 — 必须先创建 task 才能写 audit_logs(FK 约束)。
        let root_task_id = format!("task-dag-{}", uuid::Uuid::new_v4());
        self.kernel.create_task(&root_task_id, &plan.user_goal)?;

        // Step 0b: 持久化 plan 到 dag_plans 表(若不存在)。
        // root_task_id 已创建,可关联到 dag_plans.root_task_id(FK ON DELETE SET NULL)。
        {
            let conn = self.kernel.conn();
            if self.dag_repo.get_plan(&conn, &plan.plan_id)?.is_none() {
                self.dag_repo.create_plan(
                    &conn,
                    plan,
                    &DagStatus::Pending,
                    Some(&root_task_id),
                )?;
            }
            // 标记 plan 为 Running
            self.dag_repo
                .update_plan_status(&conn, &plan.plan_id, &DagStatus::Running, false)?;
        }

        // Step 0c: 审计 — dag_plan_created(spec §6.1 Task 7)
        self.kernel.audit_append_external(
            &root_task_id,
            None,
            "dag_plan_created",
            serde_json::json!({
                "plan_id": plan.plan_id,
                "node_count": plan.nodes.len(),
                "max_total_steps": plan.max_total_steps,
            }),
        )?;

        // Step 1: 拓扑排序(Kahn 算法)— 检测 edges 中的隐式环
        // W9 修复(P0-2):保留前置 fail-fast 环检测语义,不消费 order
        // (execute_nodes 内部对 effective_plan 重新调 topological_sort 拿 order,
        // 因 modified_plan 节点可能变化)。
        topological_sort(&plan.nodes, &plan.edges)?;

        // Step 2: 全局审批 — DAG 骨架 Allow/Deny/Modify(决策 #2 + W9 Plan 4 Modify 分支)
        let outcome = self.approver.approve_dag_skeleton(plan)?;
        // 审计 — dag_skeleton_approved(无论 Allow/Deny/Modify 都记录)
        self.kernel.audit_append_external(
            &root_task_id,
            None,
            "dag_skeleton_approved",
            serde_json::json!({
                "plan_id": plan.plan_id,
                "decision": outcome.as_str(),
                "phase": "initial",
            }),
        )?;

        let effective_plan: DagPlan = match outcome {
            DagApprovalOutcome::Allow => plan.clone(),
            DagApprovalOutcome::Deny => {
                // Deny → 0 节点执行 + DagStatus=Cancelled
                self.persist_dag_status(plan, &DagStatus::Cancelled)?;
                // 审计 — dag_completed(Cancelled)
                self.kernel.audit_append_external(
                    &root_task_id,
                    None,
                    "dag_completed",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "final_status": "cancelled",
                        "succeeded_count": 0,
                    }),
                )?;
                return Ok(DagResult::cancelled());
            }
            DagApprovalOutcome::Modify { modified_plan } => {
                // W9 Plan 4:审计 dag_skeleton_modified + 重新校验 + run_modified(第二次审批)
                let modified_node_count = modified_plan.nodes.len();
                let added_count = modified_plan
                    .nodes
                    .iter()
                    .filter(|n| !plan.nodes.iter().any(|o| o.node_id == n.node_id))
                    .count();
                let removed_count = plan
                    .nodes
                    .iter()
                    .filter(|o| !modified_plan.nodes.iter().any(|n| n.node_id == o.node_id))
                    .count();
                self.kernel.audit_append_external(
                    &root_task_id,
                    None,
                    "dag_skeleton_modified",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "modified_node_count": modified_node_count,
                        "added_count": added_count,
                        "removed_count": removed_count,
                    }),
                )?;

                // 重新校验 modified_plan(SlotTemplateEngine::validate_dag)
                if let Err(e) = SlotTemplateEngine::validate_dag(&modified_plan) {
                    return Err(KernelError::Skill(format!("modified_plan validate_dag failed: {}", e)));
                }

                // risk_ceiling 提权检查(spec §6.3 第三条)
                Self::check_risk_ceiling_no_escalation(plan, &modified_plan)?;

                // W9 修复(P0-2):对 modified_plan 做环检测(前置 fail-fast,避免 execute_nodes 内才报错)
                topological_sort(&modified_plan.nodes, &modified_plan.edges)?;

                // 第二次审批(只允许 Allow / Deny)
                return self.run_modified(&modified_plan, user_slots, &root_task_id);
            }
        };

        // Step 3-4:节点执行(抽为 execute_nodes,W9 Plan 4 重构)
        self.execute_nodes(&effective_plan, user_slots, &root_task_id)
    }

    /// W9 Plan 4:第二次审批 — 只允许 Allow / Deny,Modify 返回 `DagModifyLimitExceeded`。
    ///
    /// spec §6.3 第二条:Modify 只允许一次,防止无限递归。
    /// W9 修复(P0-3):modified_plan 用新 plan_id,单独走完整审计链
    /// (dag_plan_created + persist_dag_status(Pending) + dag_skeleton_approved + ...)。
    fn run_modified(
        &self,
        modified_plan: &DagPlan,
        user_slots: &[ExtractedSlot],
        root_task_id: &str,
    ) -> Result<DagResult> {
        // W9 修复(P0-3):modified_plan 用新 plan_id,单独走完整审计链
        let modified_plan_with_id = DagPlan {
            plan_id: format!("{}_modified", modified_plan.plan_id),
            ..modified_plan.clone()
        };
        // 持久化 modified_plan 到 dag_plans 表(必须先 INSERT,否则后续 create_node 会触发 FK 违规)
        {
            let conn = self.kernel.conn();
            self.dag_repo.create_plan(
                &conn,
                &modified_plan_with_id,
                &DagStatus::Pending,
                Some(root_task_id),
            )?;
        }
        self.kernel.audit_append_external(
            root_task_id,
            None,
            "dag_plan_created",
            serde_json::json!({
                "plan_id": modified_plan_with_id.plan_id,
                "source": "modify",
                "original_plan_id": modified_plan.plan_id,
            }),
        )?;

        let outcome = self.approver.approve_dag_skeleton(&modified_plan_with_id)?;
        self.kernel.audit_append_external(
            root_task_id,
            None,
            "dag_skeleton_approved",
            serde_json::json!({
                "plan_id": modified_plan_with_id.plan_id,
                "decision": outcome.as_str(),
                "phase": "after_modify",
            }),
        )?;

        match outcome {
            DagApprovalOutcome::Allow => {
                // 第二次 Allow → 用 modified_plan_with_id 走完整执行路径
                self.execute_nodes(&modified_plan_with_id, user_slots, root_task_id)
            }
            DagApprovalOutcome::Deny => {
                self.persist_dag_status(&modified_plan_with_id, &DagStatus::Cancelled)?;
                self.kernel.audit_append_external(
                    root_task_id,
                    None,
                    "dag_completed",
                    serde_json::json!({
                        "plan_id": modified_plan_with_id.plan_id,
                        "final_status": "cancelled",
                        "succeeded_count": 0,
                    }),
                )?;
                Ok(DagResult::cancelled())
            }
            DagApprovalOutcome::Modify { .. } => {
                // 第二次 Modify → 拒绝(spec §6.3 第二条)
                self.kernel.audit_append_external(
                    root_task_id,
                    None,
                    "dag_modify_limit_exceeded",
                    serde_json::json!({
                        "plan_id": modified_plan_with_id.plan_id,
                    }),
                )?;
                Err(KernelError::DagModifyLimitExceeded {
                    plan_id: modified_plan_with_id.plan_id,
                })
            }
        }
    }

    /// W9 Plan 4:节点执行循环(原 run 方法 Step 3-4 抽出,供 run_modified 复用)。
    ///
    /// 输入:`plan`(effective_plan,可能是原 plan 或 modified_plan)+ `root_task_id`
    /// 输出:DagResult(Succeeded / Failed / PartiallySucceeded)
    fn execute_nodes(
        &self,
        plan: &DagPlan,
        user_slots: &[ExtractedSlot],
        root_task_id: &str,
    ) -> Result<DagResult> {
        let order = topological_sort(&plan.nodes, &plan.edges)?;
        let mut node_outputs: HashMap<String, serde_json::Value> = HashMap::new();
        let mut node_results: HashMap<String, DagNodeStatus> = HashMap::new();
        let mut prev_node_id: Option<String> = None;

        for node_id in &order {
            let node = plan
                .nodes
                .iter()
                .find(|n| &n.node_id == node_id)
                .ok_or_else(|| {
                    KernelError::Skill(format!("topo sort returned unknown node_id: {}", node_id))
                })?;

            // 审计 — dag_node_started
            self.kernel.audit_append_external(
                root_task_id,
                None,
                "dag_node_started",
                serde_json::json!({
                    "plan_id": plan.plan_id,
                    "node_id": node.node_id,
                    "skill_id": node.skill_id,
                }),
            )?;

            // 节点执行 — spec §2.3:
            //   - 循环节点 → run_loop_node(Plan 3 Task 1;占位实现返回 Succeeded 空数组)
            //   - 简单节点 → run_simple_node(Plan 2)
            // 两者都返回 Ok(DagNodeStatus);失败语义由 execute_nodes 统一处理。
            let status = if let Some(loop_spec) = plan.loop_specs.get(node_id) {
                self.run_loop_node(
                    node_id,
                    plan,
                    loop_spec,
                    &node_outputs,
                    user_slots,
                    prev_node_id.as_deref(),
                )?
            } else {
                self.run_simple_node(
                    node,
                    plan,
                    &node_outputs,
                    user_slots,
                    prev_node_id.as_deref(),
                    root_task_id,
                )?
            };

            // 成功节点的 output 加入 node_outputs,供下游 `${prev.output.xxx}` 解析
            if let DagNodeStatus::Succeeded(ref out) = status {
                node_outputs.insert(node_id.clone(), out.clone());
                prev_node_id = Some(node_id.clone());
                // 审计 — dag_node_succeeded
                self.kernel.audit_append_external(
                    root_task_id,
                    None,
                    "dag_node_succeeded",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "node_id": node.node_id,
                        // W8 简化:实际 evidence_strength 应由 executor ToolResult 决定
                        "evidence_strength": "weak",
                    }),
                )?;
            } else if status.is_failed() {
                // 审计 — dag_node_failed
                let cause = match &status {
                    DagNodeStatus::Failed { cause } => cause.clone(),
                    _ => String::new(),
                };
                self.kernel.audit_append_external(
                    root_task_id,
                    None,
                    "dag_node_failed",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "node_id": node.node_id,
                        "cause": cause,
                    }),
                )?;
            }

            node_results.insert(node_id.clone(), status.clone());

            // 失败短路(决策 #4 + #8):
            //   - 若有已成功节点 → DagStatus::PartiallySucceeded
            //   - 否则 → DagStatus::Failed
            // 不回滚前序已 commit 节点。
            if let DagNodeStatus::Failed { ref cause } = status {
                let succeeded: Vec<String> = order
                    .iter()
                    .filter(|id| {
                        node_results
                            .get(*id)
                            .map(|s| s.is_succeeded())
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect();
                let final_status = if succeeded.is_empty() {
                    DagStatus::Failed {
                        failed_node: node_id.clone(),
                        cause: cause.clone(),
                    }
                } else {
                    DagStatus::PartiallySucceeded {
                        succeeded,
                        failed_node: node_id.clone(),
                        cause: cause.clone(),
                    }
                };
                self.persist_dag_status(plan, &final_status)?;
                // 审计 — dag_completed(Failed / PartiallySucceeded)
                let succeeded_count = order
                    .iter()
                    .filter(|id| {
                        node_results
                            .get(*id)
                            .map(|s| s.is_succeeded())
                            .unwrap_or(false)
                    })
                    .count();
                self.kernel.audit_append_external(
                    root_task_id,
                    None,
                    "dag_completed",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "final_status": final_status.as_str(),
                        "succeeded_count": succeeded_count,
                        "failed_node": node_id,
                    }),
                )?;
                return Ok(DagResult {
                    status: final_status,
                    node_results,
                });
            }
        }

        // Step 4: 全部成功
        let final_status = DagStatus::Succeeded;
        self.persist_dag_status(plan, &final_status)?;
        // 审计 — dag_completed(Succeeded)
        let succeeded_count = node_results.values().filter(|s| s.is_succeeded()).count();
        self.kernel.audit_append_external(
            root_task_id,
            None,
            "dag_completed",
            serde_json::json!({
                "plan_id": plan.plan_id,
                "final_status": final_status.as_str(),
                "succeeded_count": succeeded_count,
            }),
        )?;
        Ok(DagResult::succeeded(node_results))
    }

    /// W9 Plan 4:检查 modified_plan 的 risk_ceiling 没有超过原 plan 的 max risk。
    ///
    /// spec §6.3 第三条:用户不能通过 Modify 提权。
    /// 规则:
    ///   - 既有节点:modified.risk_ceiling ≤ original.risk_ceiling(同 node_id)
    ///   - 新增节点:modified.risk_ceiling ≤ max(original.nodes.risk_ceiling)
    ///
    /// `ELevel` 实现 `Ord`(`policy/types.rs`),直接比较。
    fn check_risk_ceiling_no_escalation(original: &DagPlan, modified: &DagPlan) -> Result<()> {
        use crate::policy::types::ELevel;
        let original_max: ELevel = original
            .nodes
            .iter()
            .map(|n| n.risk_ceiling)
            .max()
            .unwrap_or(ELevel::E0);

        for modified_node in &modified.nodes {
            let ceiling = modified_node.risk_ceiling;
            if let Some(original_node) = original.nodes.iter().find(|n| n.node_id == modified_node.node_id) {
                // 既有节点:不能超过原 ceiling
                if ceiling > original_node.risk_ceiling {
                    return Err(KernelError::Skill(format!(
                        "risk ceiling escalated for node {}: {:?} > {:?}",
                        modified_node.node_id, ceiling, original_node.risk_ceiling
                    )));
                }
            } else {
                // 新增节点:不能超过原 plan max ceiling
                if ceiling > original_max {
                    return Err(KernelError::Skill(format!(
                        "risk ceiling escalated for new node {}: {:?} > original max {:?}",
                        modified_node.node_id, ceiling, original_max
                    )));
                }
            }
        }
        Ok(())
    }

    /// 简单节点执行(spec §2.3)。
    ///
    /// 流程:
    /// 1. SlotTemplateEngine::resolve(node.input_template.template, ...) → serde_json::Value
    /// 2. 生成新 task_id + step_id(uuid)
    /// 3. dispatch_skill_executor(skill_id, kernel, resolved_input, approver, ...)
    /// 4. 根据 DispatchOutcome 构造 DagNodeStatus
    /// 5. DagRepo.update_node_status 持久化
    ///
    /// 失败语义(始终返回 Ok(status),让 run() 统一处理失败短路):
    /// - 模板 resolve 失败 → DagNodeStatus::Failed{cause: "template resolution error: ..."}
    /// - dispatch 返回 Err(e) → DagNodeStatus::Failed{cause: e.to_string()}
    /// - dispatch 返回 Ok + outcome.succeeded=false → DagNodeStatus::Failed{cause: outcome.error_cause}
    /// - dispatch 返回 Ok + outcome.succeeded=true → DagNodeStatus::Succeeded(outcome.output)
    ///
    /// 仅基础设施错误(DB 故障)返回 Err,由 run() 向上传播。
    fn run_simple_node(
        &self,
        node: &DagNode,
        plan: &DagPlan,
        node_outputs: &HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        prev_node_id: Option<&str>,
        _root_task_id: &str,
    ) -> Result<DagNodeStatus> {
        // Step 0: 持久化节点 start 状态(Pending → Running)+ 确保 dag_nodes 行存在。
        // task_id/step_id 暂不写入 — dag_nodes.task_id/step_id 有 FK 约束
        // (REFERENCES tasks/steps),但此刻 dispatch 尚未执行,executor 可能
        // 还未创建 task/step 行。dispatch 成功后再回填(见 Step 3)。
        let task_id = format!("task-{}", uuid::Uuid::new_v4());
        let step_id = format!("step-{}", uuid::Uuid::new_v4());
        {
            let conn = self.kernel.conn();
            let existing = self.dag_repo.list_nodes_by_plan(&conn, &plan.plan_id)?;
            if !existing.iter().any(|n| n.node_id == node.node_id) {
                self.dag_repo.create_node(&conn, &plan.plan_id, node)?;
            }
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                &node.node_id,
                &DagNodeStatus::Running,
                None,
                None,
            )?;
        }

        // Step 1: 解析模板 → serde_json::Value
        // W9 Plan 6:闭合 Slot 流水欠债 — 透传实际 user_slots(支持 ${user.xxx} 解析)。
        // iter_var 暂传 None — 简单节点无循环变量。
        let resolved_input = match SlotTemplateEngine::resolve(
            &node.input_template.template,
            node_outputs,
            user_slots,
            None,
            prev_node_id,
        ) {
            Ok(v) => v,
            Err(e) => {
                let cause = format!("template resolution error: {}", e);
                let status = DagNodeStatus::Failed { cause };
                // 模板失败:dispatch 未调用,task/step 不存在 → 传 None 避免 FK 违规
                self.persist_node_final_status(plan, &node.node_id, &status, None, None)?;
                return Ok(status);
            }
        };

        // Step 2: dispatch_skill_executor
        let outcome_result = dispatch_skill_executor(
            &node.skill_id,
            &self.kernel,
            &resolved_input,
            self.approver.as_ref(),
            &task_id,
            &step_id,
        );

        // Step 3: 构造 DagNodeStatus + 持久化
        // 仅当 dispatch 返回 Ok + succeeded=true 时,executor 一定已创建 task/step
        // (因为 W7 既有 executor 的成功路径都调 kernel.create_task/create_step)。
        // 其他情况下 task/step 可能不存在,保守传 None 避免 FK 违规。
        let (status, tid, sid) = match outcome_result {
            Ok(outcome) if outcome.succeeded => {
                let s = DagNodeStatus::Succeeded(outcome.output.clone());
                (s, Some(task_id.as_str()), Some(step_id.as_str()))
            }
            Ok(outcome) => {
                let cause = outcome.error_cause.unwrap_or_else(|| "unknown error".into());
                let s = DagNodeStatus::Failed { cause };
                (s, None, None)
            }
            Err(e) => {
                let cause = e.to_string();
                let s = DagNodeStatus::Failed { cause };
                (s, None, None)
            }
        };

        self.persist_node_final_status(plan, &node.node_id, &status, tid, sid)?;
        Ok(status)
    }

    /// 循环节点执行(spec §2.3 决策 #5 + #8)。
    ///
    /// 算法:
    ///   Step 1: resolve_iterable(iterable_source, node_outputs) → Vec<Value>
    ///   Step 2: items.into_iter().take(spec.max_iterations.min(50)) 强制截断
    ///   Step 3: 对每个 item:
    ///     - SlotTemplateEngine::resolve(template, ..., Some(&item)) 绑定 ${item}
    ///     - dispatch_skill_executor(skill_id, kernel, resolved_input, approver, ...)
    ///     - 失败 → iter_failed = Some(cause), break(决策 #8:终止循环)
    ///     - 成功 → push 到 iter_outputs
    ///     - 检查 break_condition,命中则 break
    ///   Step 4: 循环节点状态:
    ///     - 全成功 → DagNodeStatus::Succeeded(Array(iter_outputs))
    ///     - 有失败 → DagNodeStatus::Failed { cause }(让上层走 PartiallySucceeded 分支)
    ///
    /// 失败语义(始终返回 Ok(status),让 run() 统一处理失败短路):
    /// - 模板 resolve 失败 → DagNodeStatus::Failed{cause}
    /// - dispatch 返回 Err(e) → DagNodeStatus::Failed{cause: "iter N dispatch error: ..."}
    /// - dispatch 返回 Ok + outcome.succeeded=false → DagNodeStatus::Failed{cause}
    /// - 全部迭代成功 + 无 break_condition 触发 → DagNodeStatus::Succeeded(Array)
    ///
    /// 仅基础设施错误(DB 故障)返回 Err,由 run() 向上传播。
    fn run_loop_node(
        &self,
        node_id: &str,
        plan: &DagPlan,
        loop_spec: &crate::skills::dag_types::LoopSpec,
        node_outputs: &HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        prev_node_id: Option<&str>,
    ) -> Result<DagNodeStatus> {
        // Step 1: 解析 iterable → Vec<Value>
        let items = self.resolve_iterable(
            &loop_spec.iterable_source,
            node_outputs,
            user_slots,
            prev_node_id,
        )?;
        let items_len = items.len();

        // Step 2: 强制截断到 max_iterations.min(50)
        let max_iter = loop_spec
            .max_iterations
            .min(crate::skills::dag_types::MAX_LOOP_ITERATIONS_HARD_LIMIT);
        let truncated: Vec<serde_json::Value> = items.into_iter().take(max_iter as usize).collect();
        let truncated_len = truncated.len();
        if (truncated_len as u32) < items_len as u32 {
            tracing::warn!(
                node_id = node_id,
                original = items_len,
                truncated = truncated_len,
                max_iterations = loop_spec.max_iterations,
                hard_limit = crate::skills::dag_types::MAX_LOOP_ITERATIONS_HARD_LIMIT,
                "loop iterable truncated to max_iterations.min(50)"
            );
        }

        // Step 3: 查找节点(用于 input_template)
        let node = plan
            .nodes
            .iter()
            .find(|n| n.node_id == node_id)
            .ok_or_else(|| {
                KernelError::Skill(format!(
                    "run_loop_node: node '{}' not in plan.nodes",
                    node_id
                ))
            })?;

        // 持久化节点 start 状态(Pending → Running)+ 确保 dag_nodes 行存在。
        // task_id/step_id 暂不写入 — 循环节点有多个迭代 task,不持久化单个。
        // 循环结束后,最终终态持久化(见 Step 7)。
        {
            let conn = self.kernel.conn();
            let existing = self.dag_repo.list_nodes_by_plan(&conn, &plan.plan_id)?;
            if !existing.iter().any(|n| n.node_id == node_id) {
                self.dag_repo.create_node(&conn, &plan.plan_id, node)?;
            }
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                node_id,
                &DagNodeStatus::Running,
                None,
                None,
            )?;
        }

        // Step 4: 解析 break_condition(若有)
        let break_cond_parsed = loop_spec
            .break_condition
            .as_deref()
            .map(Self::parse_break_condition)
            .transpose()?;

        // Step 5: 迭代执行
        let mut iter_outputs: Vec<serde_json::Value> = Vec::with_capacity(truncated_len);
        let mut iter_failed: Option<String> = None;

        for (idx, item) in truncated.iter().enumerate() {
            // 5a: 生成新 task_id + step_id(每个迭代独立 task)
            let task_id = format!("task-loop-{}-iter-{}", uuid::Uuid::new_v4(), idx);
            let step_id = format!("step-loop-{}-iter-{}", uuid::Uuid::new_v4(), idx);

            // 5b: SlotTemplateEngine::resolve 绑定 ${item}
            // W9 Plan 6:循环节点 body 也接收 user_slots(供 ${user.xxx} 解析)。
            let resolved_input = match SlotTemplateEngine::resolve(
                &node.input_template.template,
                node_outputs,
                user_slots,
                Some(item),
                prev_node_id,
            ) {
                Ok(v) => v,
                Err(e) => {
                    let cause = format!("loop iter {} template resolution error: {}", idx, e);
                    iter_failed = Some(cause);
                    break; // 决策 #8:终止循环
                }
            };

            // 5c: dispatch_skill_executor
            let outcome_result = dispatch_skill_executor(
                &node.skill_id,
                &self.kernel,
                &resolved_input,
                self.approver.as_ref(),
                &task_id,
                &step_id,
            );

            // 5d: 处理结果
            match outcome_result {
                Ok(outcome) if outcome.succeeded => {
                    iter_outputs.push(outcome.output.clone());
                }
                Ok(outcome) => {
                    let cause = outcome.error_cause.unwrap_or_else(|| "unknown error".into());
                    iter_failed = Some(format!("iter {} failed: {}", idx, cause));
                    break; // 决策 #8:终止循环
                }
                Err(e) => {
                    let cause = format!("iter {} dispatch error: {}", idx, e);
                    iter_failed = Some(cause);
                    break; // 决策 #8:终止循环
                }
            }

            // 5e: 检查 break_condition
            if let Some((field, op, threshold)) = &break_cond_parsed {
                if Self::evaluate_break_condition(item, field, op, *threshold) {
                    tracing::info!(
                        node_id = node_id,
                        iter = idx,
                        field = field,
                        op = op,
                        threshold = threshold,
                        "break_condition matched, terminating loop early"
                    );
                    break;
                }
            }
        }

        // Step 6: 节点状态构造
        let status = if let Some(cause) = iter_failed {
            DagNodeStatus::Failed { cause }
        } else {
            DagNodeStatus::Succeeded(serde_json::Value::Array(iter_outputs))
        };

        // Step 7: 持久化节点终态
        // task_id/step_id 不写入 — 循环节点有多个迭代 task,不持久化单个。
        {
            let conn = self.kernel.conn();
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                node_id,
                &status,
                None,
                None,
            )?;
        }
        Ok(status)
    }

    /// 解析 break_condition 字符串 → (field, op, threshold)。
    ///
    /// 格式:`item.<field> <op> <number>`,op ∈ {>, <, >=, <=, ==, !=}。
    ///
    /// 例:`"item.size > 1048576"` → `("size", ">", 1048576.0)`
    ///     `"item.priority >= 3"`   → `("priority", ">=", 3.0)`
    ///
    /// 失败:格式不合法 / op 不支持 / number 解析失败 → Err
    fn parse_break_condition(cond: &str) -> Result<(&str, &str, f64)> {
        let parts: Vec<&str> = cond.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(KernelError::Skill(format!(
                "break_condition parse: expected 3 tokens, got {}: {:?}",
                parts.len(),
                cond
            )));
        }
        let field_part = parts[0];
        let op = parts[1];
        let num_str = parts[2];

        let field = field_part.strip_prefix("item.").ok_or_else(|| {
            KernelError::Skill(format!(
                "break_condition parse: field must start with 'item.', got '{}'",
                field_part
            ))
        })?;

        let valid_ops = [">", "<", ">=", "<=", "==", "!="];
        if !valid_ops.contains(&op) {
            return Err(KernelError::Skill(format!(
                "break_condition parse: unsupported op '{}', must be one of {:?}",
                op, valid_ops
            )));
        }

        let threshold: f64 = num_str.parse().map_err(|_| {
            KernelError::Skill(format!(
                "break_condition parse: threshold '{}' is not a number",
                num_str
            ))
        })?;

        Ok((field, op, threshold))
    }

    /// 求值 break_condition:从 item 按 field 提取数值,按 op 比较。
    ///
    /// item 必须是 serde_json::Value::Object,含 field 键,值可转 f64(as_f64)。
    /// 返回 true 表示命中中断条件(应 break 循环)。
    /// 字段不存在或非数值 → 返回 false(不中断,继续循环)。
    fn evaluate_break_condition(
        item: &serde_json::Value,
        field: &str,
        op: &str,
        threshold: f64,
    ) -> bool {
        let val = item.get(field).and_then(|v| v.as_f64());
        match val {
            Some(v) => match op {
                ">" => v > threshold,
                "<" => v < threshold,
                ">=" => v >= threshold,
                "<=" => v <= threshold,
                "==" => (v - threshold).abs() < f64::EPSILON,
                "!=" => (v - threshold).abs() >= f64::EPSILON,
                _ => false, // 不应发生(parse_break_condition 已校验)
            },
            None => false, // 字段不存在或非数值 → 不中断(继续循环)
        }
    }

    /// resolve_iterable — 把 IterableSource 解析为 Vec<Value>(spec §2.3 Step 1)。
    ///
    /// - PrevNodeOutput { node_id, port } → node_outputs[node_id] 按 port dotted path 提取数组
    /// - UserSlot { slot_kind } → W9 Plan 6:从 user_slots 查表匹配 kind,解析为 Vec<Value>
    ///   (先尝试 JSON 数组,失败则按 CSV 逗号分隔 fallback)
    /// - Literal(vec) → 直接转 Vec<Value>
    ///
    /// 失败:
    /// - PrevNodeOutput:node_id 不在 node_outputs / port 路径不存在 / 值非 Array → Err
    /// - UserSlot:user_slots 中找不到匹配 kind → Err
    fn resolve_iterable(
        &self,
        source: &crate::skills::dag_types::IterableSource,
        node_outputs: &HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        _prev_node_id: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        use crate::skills::dag_types::IterableSource;
        match source {
            IterableSource::Literal(items) => {
                Ok(items.iter().map(|s| serde_json::Value::String(s.clone())).collect())
            }
            IterableSource::PrevNodeOutput { node_id, port } => {
                let node_out = node_outputs.get(node_id).ok_or_else(|| {
                    KernelError::Skill(format!(
                        "resolve_iterable: PrevNodeOutput node '{}' not in node_outputs",
                        node_id
                    ))
                })?;
                // 按 port dotted path 提取(如 "output.files" → ["output"]["files"])
                let mut current = node_out;
                for key in port.split('.') {
                    current = current.get(key).ok_or_else(|| {
                        KernelError::Skill(format!(
                            "resolve_iterable: port path '{}' not found in node_outputs['{}']",
                            port, node_id
                        ))
                    })?;
                }
                match current {
                    serde_json::Value::Array(arr) => Ok(arr.clone()),
                    _ => Err(KernelError::Skill(format!(
                        "resolve_iterable: PrevNodeOutput port '{}' is not an array (got {})",
                        port,
                        type_name_of_value(current)
                    ))),
                }
            }
            IterableSource::UserSlot { slot_kind } => {
                // W9 Plan 6:闭合 Slot 流水欠债 — 从 user_slots 查表找匹配 kind 的 slot。
                // ExtractedSlot.kind 是 String(见 src/llm/types.rs:23-30),
                // slot_kind 来自 LoopSpec.iterable_source 也是 String,直接比较。
                let slot = user_slots
                    .iter()
                    .find(|s| s.kind == *slot_kind)
                    .ok_or_else(|| {
                        KernelError::Skill(format!(
                            "resolve_iterable: UserSlot kind '{}' not found in user_slots",
                            slot_kind
                        ))
                    })?;
                // slot.raw 是 String(W7 LLM ExtractedSlot 定义,见 src/llm/types.rs:23-30)。
                // W8 既有 VarScope::User 分支用 slot.raw,Plan 6 保持一致。
                // 尝试解析为 JSON 数组;失败则当作 CSV 逗号分隔(简单 fallback,
                // 适配 "a,b,c" 逗号分隔 或 单值)。
                match serde_json::from_str::<Vec<serde_json::Value>>(&slot.raw) {
                    Ok(arr) => Ok(arr),
                    Err(_) => {
                        // CSV fallback:按逗号分隔,trim 每项
                        let items: Vec<serde_json::Value> = slot
                            .raw
                            .split(',')
                            .map(|s| serde_json::Value::String(s.trim().to_string()))
                            .collect();
                        Ok(items)
                    }
                }
            }
        }
    }

    /// 持久化节点终态(Succeeded / Failed)到 dag_nodes 表。
    /// 非终态(Pending / Running)不写 completed_at。
    /// `task_id` / `step_id` 为 None 时不写入(FK 约束要求 tasks/steps 表
    /// 必须先存在对应行,某些失败路径下 task/step 可能未创建)。
    fn persist_node_final_status(
        &self,
        plan: &DagPlan,
        node_id: &str,
        status: &DagNodeStatus,
        task_id: Option<&str>,
        step_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.kernel.conn();
        self.dag_repo
            .update_node_status(&conn, &plan.plan_id, node_id, status, task_id, step_id)
    }

    /// 把 DAG 终态持久化到 dag_plans 表。
    /// 失败时返回 Err(但 DAG 执行结果已确定,此处仅记录失败)。
    fn persist_dag_status(&self, plan: &DagPlan, status: &DagStatus) -> Result<()> {
        let conn = self.kernel.conn();
        let completed = !matches!(status, DagStatus::Pending | DagStatus::Running);
        self.dag_repo
            .update_plan_status(&conn, &plan.plan_id, status, completed)
    }
}

/// 返回 serde_json::Value 的类型名(用于错误消息)。
fn type_name_of_value(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// 拓扑排序 — Kahn 算法。
///
/// 算法:
///
/// 1. 计算每个节点的 in-degree(入度 = 多少条 edge 指向它)
/// 2. 把 in-degree=0 的节点入队
/// 3. BFS:出队一个节点 → 加入 result → 把它的所有后继节点 in-degree 减 1
///    → 若 in-degree 变为 0 则入队
/// 4. 若 result.len() < nodes.len() → 存在环(返回 Err)
///
/// 注:LoopSpec 显式循环节点(节点内部迭代)与 edges 隐式环(节点依赖环)
/// 是不同概念。本函数仅检测后者;前者由 Plan 3 的 run_loop_node 处理。
pub fn topological_sort(nodes: &[DagNode], edges: &[DagEdge]) -> Result<Vec<String>> {
    let mut in_degree: HashMap<String, u32> = HashMap::new();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();

    for node in nodes {
        in_degree.insert(node.node_id.clone(), 0);
        adjacency.insert(node.node_id.clone(), Vec::new());
    }

    for edge in edges {
        // 校验 edge.from / edge.to 在 nodes 中
        if !in_degree.contains_key(&edge.from) {
            return Err(KernelError::Skill(format!(
                "topological_sort: edge.from '{}' not in nodes",
                edge.from
            )));
        }
        if !in_degree.contains_key(&edge.to) {
            return Err(KernelError::Skill(format!(
                "topological_sort: edge.to '{}' not in nodes",
                edge.to
            )));
        }
        *in_degree.get_mut(&edge.to).unwrap() += 1;
        adjacency.get_mut(&edge.from).unwrap().push(edge.to.clone());
    }

    let queue: VecDeque<String> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(k, _)| k.clone())
        .collect();

    // 为了结果稳定,初始 queue 按 node_id 字典序排序
    let mut sorted_queue: Vec<String> = queue.into_iter().collect();
    sorted_queue.sort();
    let mut queue: VecDeque<String> = sorted_queue.into_iter().collect();

    let mut result: Vec<String> = Vec::with_capacity(nodes.len());
    while let Some(node_id) = queue.pop_front() {
        result.push(node_id.clone());
        let mut next_zero: Vec<String> = Vec::new();
        if let Some(succs) = adjacency.get(&node_id) {
            for succ in succs {
                let deg = in_degree.get_mut(succ).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    next_zero.push(succ.clone());
                }
            }
        }
        // 稳定排序:每批新入度为 0 的节点按字典序加入
        next_zero.sort();
        for s in next_zero {
            queue.push_back(s);
        }
    }

    if result.len() != nodes.len() {
        // 检测到环 — 找出环中的节点(in_degree > 0 的)
        let cycle_nodes: Vec<String> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg > 0)
            .map(|(k, _)| k.clone())
            .collect();
        return Err(KernelError::Skill(format!(
            "topological_sort: cycle detected involving nodes: {:?}",
            cycle_nodes
        )));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::types::ELevel;
    use crate::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

    fn node(id: &str) -> DagNode {
        DagNode {
            node_id: id.into(),
            skill_id: "test.skill".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("x".into()),
            },
            risk_ceiling: ELevel::E1,
        }
    }

    fn edge(from: &str, to: &str) -> DagEdge {
        DagEdge {
            from: from.into(),
            to: to.into(),
            port_binding: None,
        }
    }

    #[test]
    fn topo_sort_single_node_no_edges() {
        let nodes = vec![node("n1")];
        let edges = vec![];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(order, vec!["n1".to_string()]);
    }

    #[test]
    fn topo_sort_linear_chain() {
        let nodes = vec![node("n1"), node("n2"), node("n3")];
        let edges = vec![edge("n1", "n2"), edge("n2", "n3")];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(
            order,
            vec!["n1".to_string(), "n2".to_string(), "n3".to_string()]
        );
    }

    #[test]
    fn topo_sort_diamond_dependency() {
        // n1 → n2, n1 → n3, n2 → n4, n3 → n4
        let nodes = vec![node("n1"), node("n2"), node("n3"), node("n4")];
        let edges = vec![
            edge("n1", "n2"),
            edge("n1", "n3"),
            edge("n2", "n4"),
            edge("n3", "n4"),
        ];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(order[0], "n1");
        assert_eq!(order[3], "n4");
        // n2 和 n3 顺序按字典序:n2 < n3
        assert_eq!(order[1], "n2");
        assert_eq!(order[2], "n3");
    }

    #[test]
    fn topo_sort_disjoint_nodes() {
        // 两个不连通的节点,按字典序输出
        let nodes = vec![node("b"), node("a")];
        let edges = vec![];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(order, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn topo_sort_detects_cycle_returns_err() {
        // n1 → n2 → n1(环)
        let nodes = vec![node("n1"), node("n2")];
        let edges = vec![edge("n1", "n2"), edge("n2", "n1")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cycle"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_detects_self_loop() {
        let nodes = vec![node("n1")];
        let edges = vec![edge("n1", "n1")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cycle"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_rejects_edge_with_unknown_from() {
        let nodes = vec![node("n1")];
        let edges = vec![edge("n99", "n1")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not in nodes"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_rejects_edge_with_unknown_to() {
        let nodes = vec![node("n1")];
        let edges = vec![edge("n1", "n99")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not in nodes"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_empty_nodes_returns_empty() {
        let order = topological_sort(&[], &[]).unwrap();
        assert!(order.is_empty());
    }

    // ===== W8 Plan 3 Task 2: parse_break_condition + evaluate_break_condition =====

    #[test]
    fn parse_break_condition_valid_formats() {
        // "item.size > 1048576" → ("size", ">", 1048576.0)
        let (field, op, threshold) =
            DagExecutor::parse_break_condition("item.size > 1048576").unwrap();
        assert_eq!(field, "size");
        assert_eq!(op, ">");
        assert!((threshold - 1048576.0).abs() < f64::EPSILON);

        // 6 个合法 op 全部解析成功
        for op_str in [">", "<", ">=", "<=", "==", "!="] {
            let cond = format!("item.priority {} 3", op_str);
            let (_, parsed_op, threshold) = DagExecutor::parse_break_condition(&cond).unwrap();
            assert_eq!(parsed_op, op_str);
            assert!((threshold - 3.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn parse_break_condition_invalid_op_errors() {
        // ">>" 不在 6 个合法 op 中
        let err = DagExecutor::parse_break_condition("item.size >> 1000").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unsupported op"), "got: {}", msg);

        // "eq" 不是合法 op(必须用 ==)
        let err = DagExecutor::parse_break_condition("item.size eq 1000").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unsupported op"), "got: {}", msg);
    }

    #[test]
    fn parse_break_condition_invalid_format_errors() {
        // 2 tokens(缺阈值)
        let err = DagExecutor::parse_break_condition("item.size >").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("expected 3 tokens"), "got: {}", msg);

        // 4 tokens(多了一个)
        let err = DagExecutor::parse_break_condition("item.size > 1000 extra").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("expected 3 tokens"), "got: {}", msg);

        // 缺 item. 前缀
        let err = DagExecutor::parse_break_condition("size > 1000").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("must start with 'item.'"), "got: {}", msg);
    }

    #[test]
    fn parse_break_condition_non_numeric_threshold_errors() {
        // 阈值不是数字
        let err = DagExecutor::parse_break_condition("item.size > abc").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not a number"), "got: {}", msg);
    }

    #[test]
    fn evaluate_break_condition_object_item_matches() {
        // item.size > 1000,item = {"size": 5000} → true
        let item = serde_json::json!({"size": 5000});
        assert!(DagExecutor::evaluate_break_condition(&item, "size", ">", 1000.0));
        assert!(DagExecutor::evaluate_break_condition(&item, "size", ">=", 1000.0));
        assert!(DagExecutor::evaluate_break_condition(&item, "size", "!=", 1000.0));
        assert!(!DagExecutor::evaluate_break_condition(&item, "size", "<", 1000.0));
        assert!(!DagExecutor::evaluate_break_condition(&item, "size", "<=", 1000.0));
        assert!(!DagExecutor::evaluate_break_condition(&item, "size", "==", 1000.0));
    }

    #[test]
    fn evaluate_break_condition_string_item_no_field_returns_false() {
        // item 是 string,无 .size → 字段不存在 → false(不中断)
        let item = serde_json::Value::String("a".into());
        assert!(!DagExecutor::evaluate_break_condition(&item, "size", ">", 1000.0));
    }

    #[test]
    fn evaluate_break_condition_missing_field_returns_false() {
        // item 是 object,但无 "size" 字段 → false
        let item = serde_json::json!({"name": "a"});
        assert!(!DagExecutor::evaluate_break_condition(&item, "size", ">", 1000.0));
    }

    #[test]
    fn evaluate_break_condition_non_numeric_field_returns_false() {
        // item.size 是 string,as_f64 返回 None → false
        let item = serde_json::json!({"size": "large"});
        assert!(!DagExecutor::evaluate_break_condition(&item, "size", ">", 1000.0));
    }
}
