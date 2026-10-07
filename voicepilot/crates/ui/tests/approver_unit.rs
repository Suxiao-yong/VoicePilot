#![cfg(feature = "tauri")]

use std::time::Duration;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::transaction::EffectManifest;
use voicepilot_ui::approver::ApprovalRegistry;

fn dummy_manifest() -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: "D:/test/dest".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    }
}

#[test]
fn approval_registry_resolves_submitted_decision() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (approval_id, rx) = registry.create_request(&manifest);

    let sender = registry.take_sender(&approval_id).expect("sender exists");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = sender.send(ApprovalDecision::Allow);
    });

    let decision = registry.wait_for_decision(rx, Duration::from_secs(5));
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn approval_registry_times_out_to_deny() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (_approval_id, rx) = registry.create_request(&manifest);

    let decision = registry.wait_for_decision(rx, Duration::from_millis(100));
    assert_eq!(decision, ApprovalDecision::Deny);
}

#[test]
fn approval_registry_consumes_request_after_take() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (approval_id, _rx) = registry.create_request(&manifest);

    let _ = registry
        .take_sender(&approval_id)
        .expect("first take succeeds");

    assert!(registry.take_sender(&approval_id).is_none());
}

#[test]
fn approval_registry_handles_sender_dropped() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (approval_id, rx) = registry.create_request(&manifest);

    let _sender = registry.take_sender(&approval_id).expect("exists");

    let decision = registry.wait_for_decision(rx, Duration::from_secs(1));
    assert_eq!(decision, ApprovalDecision::Deny);
}

// ===== W9 Plan 4 Task 5: ApprovalRegistry DAG channel(Modify payload)=====

use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_types::{DagNode, DagPlan};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
use voicepilot_ui::approver::DagApprovalPayload;

fn make_test_plan(plan_id: &str) -> DagPlan {
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "测试".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("5".into()),
            },
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs: std::collections::HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn approval_registry_creates_dag_request_with_dag_prefix() {
    let registry = ApprovalRegistry::new();
    let (approval_id, _rx) = registry.create_dag_request();
    assert!(
        approval_id.starts_with("dag_"),
        "DAG approval_id should start with 'dag_', got: {}",
        approval_id
    );
}

#[test]
fn approval_registry_take_dag_sender_returns_sender_once() {
    let registry = ApprovalRegistry::new();
    let (approval_id, _rx) = registry.create_dag_request();

    let sender = registry.take_dag_sender(&approval_id);
    assert!(sender.is_some(), "first take should succeed");

    let second = registry.take_dag_sender(&approval_id);
    assert!(second.is_none(), "second take should fail (single-use)");
}

#[test]
fn approval_registry_take_dag_sender_returns_none_for_unknown_id() {
    let registry = ApprovalRegistry::new();
    let sender = registry.take_dag_sender("dag_nonexistent");
    assert!(sender.is_none(), "unknown approval_id should return None");
}

#[test]
fn approval_registry_wait_for_dag_decision_delivers_modify_payload() {
    // 模拟用户 Modify 决策:投递 DagApprovalPayload { decision: Modify, modified_plan: Some(...) }
    // 验证 wait_for_dag_decision 能正确返回该 payload
    let registry = ApprovalRegistry::new();
    let (approval_id, rx) = registry.create_dag_request();

    let modified_plan = make_test_plan("test-modify-plan");
    let expected_plan_id = modified_plan.plan_id.clone();
    let sender = registry
        .take_dag_sender(&approval_id)
        .expect("sender exists");
    let payload_to_send = DagApprovalPayload {
        decision: ApprovalDecision::Modify,
        modified_plan: Some(modified_plan),
    };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = sender.send(payload_to_send);
    });

    let payload = registry.wait_for_dag_decision(rx, Duration::from_secs(5));
    assert_eq!(payload.decision, ApprovalDecision::Modify);
    let modified_plan = payload.modified_plan.expect("modified_plan should be Some");
    assert_eq!(modified_plan.plan_id, expected_plan_id);
}

#[test]
fn approval_registry_wait_for_dag_decision_times_out_to_deny() {
    // 超时 → 返回 Deny + None modified_plan(默认安全)
    let registry = ApprovalRegistry::new();
    let (_approval_id, rx) = registry.create_dag_request();

    let payload = registry.wait_for_dag_decision(rx, Duration::from_millis(100));
    assert_eq!(payload.decision, ApprovalDecision::Deny);
    assert!(payload.modified_plan.is_none());
}

#[test]
fn approval_registry_wait_for_dag_decision_handles_sender_dropped() {
    // sender 被丢�?�?返回 Deny + None modified_plan
    let registry = ApprovalRegistry::new();
    let (approval_id, rx) = registry.create_dag_request();

    let _sender = registry.take_dag_sender(&approval_id).expect("exists");
    // _sender 在此 scope 结束时被 drop

    let payload = registry.wait_for_dag_decision(rx, Duration::from_secs(1));
    assert_eq!(payload.decision, ApprovalDecision::Deny);
    assert!(payload.modified_plan.is_none());
}

#[test]
fn waits_are_safe_inside_tokio_runtime() {
    // 回归：Tauri 命令协程内调 wait_* 曾因嵌套 block_on panic。
    // multi-thread runtime 复刻命令执行上下文（async context）。
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    rt.block_on(async {
        let registry = ApprovalRegistry::new();
        // decision 通道：无人投递 → 超时 Deny。
        let (_id, rx) = registry.create_request(&dummy_manifest());
        assert_eq!(
            registry.wait_for_decision(rx, Duration::from_millis(100)),
            ApprovalDecision::Deny
        );
        // clarification 通道：无人投递 → 超时回 default。
        let (_id, rx) = registry.create_clarify_request();
        assert_eq!(
            registry.wait_for_clarification(rx, Duration::from_millis(100), 2),
            2
        );
        // dag 通道：无人投递 → 超时 Deny。
        let (_id, rx) = registry.create_dag_request();
        let payload = registry.wait_for_dag_decision(rx, Duration::from_millis(100));
        assert_eq!(payload.decision, ApprovalDecision::Deny);
        assert!(payload.modified_plan.is_none());
    });
}
