//! W8 Plan 2 Task 1: Approver::approve_dag_skeleton 单元测试.
//!
//! Spec §2.3 决策 #2:DAG 骨架审批层 — Allow 才进入节点执行,
//! Deny 则 DagStatus=Cancelled,0 节点执行。
//!
//! 本测试覆盖 trust-kernel 内的 `AutoApprover` / `AutoDenier` 两实现 +
//! trait object 兼容性(`Arc<dyn Approver>`)。`TauriApprover` 实现位于
//! `ui` crate,其 stub 测试在 `ui/tests/w8_plan2_approver_dag_skeleton.rs`
//! (受 `tauri` feature 门控)。

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{Approver, AutoApprover, AutoDenier, DagApprovalOutcome};
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};

fn dummy_plan(plan_id: &str) -> DagPlan {
    let n1 = DagNode {
        node_id: "n1".into(),
        skill_id: "note.capture".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("notepad".into()),
        },
        risk_ceiling: ELevel::E1,
    };
    let n2 = DagNode {
        node_id: "n2".into(),
        skill_id: "files.move".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Path,
            template: TemplateExpr::Var(VarRef {
                scope: VarScope::Prev,
                path: "output.path".into(),
            }),
        },
        risk_ceiling: ELevel::E2,
    };
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "打开记事本写 TODO 然后保存到桌面".into(),
        nodes: vec![n1, n2],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: Some("output.path -> input.source".into()),
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn auto_approver_approve_dag_skeleton_returns_allow() {
    let approver = AutoApprover;
    let plan = dummy_plan("plan-auto-allow");
    let outcome = approver
        .approve_dag_skeleton(&plan)
        .expect("AutoApprover should not error");
    // `DagApprovalOutcome` 未 derive `PartialEq`(`Modify` 含 `Box<DagPlan>`,
    // `DagPlan` 未实现 `PartialEq`),用 `matches!` 断言 variant。
    assert!(matches!(outcome, DagApprovalOutcome::Allow));
}

#[test]
fn auto_denier_approve_dag_skeleton_returns_deny() {
    let approver = AutoDenier;
    let plan = dummy_plan("plan-auto-deny");
    let outcome = approver
        .approve_dag_skeleton(&plan)
        .expect("AutoDenier should not error");
    assert!(matches!(outcome, DagApprovalOutcome::Deny));
}

#[test]
fn approver_trait_object_can_call_approve_dag_skeleton() {
    // 验证 trait object 也能调用新方法(动态分发)。
    // DagExecutor 在生产代码中持有 `Arc<dyn Approver>`,需要 trait object 兼容。
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let plan = dummy_plan("plan-trait-object");
    let outcome = approver
        .approve_dag_skeleton(&plan)
        .expect("trait object should not error");
    assert!(matches!(outcome, DagApprovalOutcome::Allow));
}

#[test]
fn auto_approver_prompt_still_works_after_extension() {
    // 回归测试:确保新增 `approve_dag_skeleton` 不破坏既有 `prompt` 方法。
    use trust_kernel::policy::transaction::EffectManifest;
    let approver = AutoApprover;
    let manifest = EffectManifest {
        sources: vec![],
        destination: "D:/test/dest".into(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let decision = approver.prompt(&manifest);
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn auto_denier_prompt_still_works_after_extension() {
    // 回归测试:确保新增 `approve_dag_skeleton` 不破坏既有 `prompt` 方法。
    use trust_kernel::policy::transaction::EffectManifest;
    let approver = AutoDenier;
    let manifest = EffectManifest {
        sources: vec![],
        destination: "D:/test/dest".into(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let decision = approver.prompt(&manifest);
    assert_eq!(decision, ApprovalDecision::Deny);
}
