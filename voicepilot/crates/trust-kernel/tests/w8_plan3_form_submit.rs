//! W8 Plan 3 Task 4: form_submit_manifest 字段断言.
//!
//! Spec §2.4:form.submit manifest 字段约束。
//! 验证 risk_ceiling=E3 / PerStep approval / None compensation / Weak verifier。

use trust_kernel::compensation::types::{CompensationLevel, ConflictPolicy};
use trust_kernel::policy::types::{DLevel, ELevel};
use trust_kernel::skills::manifest::{form_submit_manifest, ApprovalMode, EgressKind};

#[test]
fn form_submit_manifest_basic_fields() {
    let m = form_submit_manifest();
    assert_eq!(m.id, "form.submit");
    assert_eq!(m.version, "1.0.0");
    assert_eq!(m.title, "提交表单");
    assert_eq!(m.description, "通过 Playwright MCP 点击 submit 按钮");
    assert!(m.intent_examples.contains(&"提交".to_string()));
    assert!(m.intent_examples.contains(&"submit".to_string()));
    assert!(m.keywords.contains(&"submit".to_string()));
    assert!(m.keywords.contains(&"提交".to_string()));
}

#[test]
fn form_submit_manifest_risk_ceiling_is_e3() {
    // spec §2.4:提交不可逆 → E3(与 form.prepare 的 E2 区分)
    let m = form_submit_manifest();
    assert_eq!(m.risk_ceiling, ELevel::E3, "form.submit risk_ceiling must be E3");
    assert_eq!(m.data_class_ceiling, DLevel::D2);
}

#[test]
fn form_submit_manifest_approval_is_per_step() {
    // spec §2.4:PerStep 强制每步审批(E3 不可逆)
    let m = form_submit_manifest();
    assert_eq!(m.approval.mode, ApprovalMode::PerStep);
    assert_eq!(m.approval.required_for, "both");
    assert!(m.approval.show_effect_manifest);
    assert_eq!(m.approval.max_approval_scope, 1);
}

#[test]
fn form_submit_manifest_compensation_is_none() {
    // spec §2.4 + 决策 #4:不可逆 → None compensation
    let m = form_submit_manifest();
    assert_eq!(m.compensation.level, CompensationLevel::None);
    assert_eq!(m.compensation.ttl_seconds, 0);
    assert_eq!(
        m.compensation.conflict_policy,
        ConflictPolicy::RequireConfirmation
    );
}

#[test]
fn form_submit_manifest_verifier_is_weak() {
    // spec §2.4:浏览器无文件 evidence → Weak
    let m = form_submit_manifest();
    assert_eq!(m.verifier.strategy, "weak");
    assert_eq!(m.verifier.recheck_after_seconds, 0);
}

#[test]
fn form_submit_manifest_inputs_and_tools() {
    let m = form_submit_manifest();
    // inputs: url(required Url) + submit_selector(optional Text, default "button[type=submit]")
    let url_input = m.inputs.get("url").expect("url input must exist");
    assert!(url_input.required, "url must be required");
    let submit_input = m
        .inputs
        .get("submit_selector")
        .expect("submit_selector input must exist");
    assert!(!submit_input.required, "submit_selector must be optional");
    let default = submit_input
        .default
        .as_ref()
        .expect("submit_selector must have default");
    assert_eq!(default, &serde_json::json!("button[type=submit]"));

    // tools: navigate + click
    assert!(m.tools.iter().any(|t| t.contains("navigate")));
    assert!(m.tools.iter().any(|t| t.contains("click")));
    assert_eq!(m.max_steps, 1);

    // egress: LocalToWebSubmit
    assert_eq!(m.egress, EgressKind::LocalToWebSubmit);

    // failure_policy: 不重试,不重规划,询问用户
    assert_eq!(m.failure_policy.max_retries, 0);
    assert!(!m.failure_policy.allow_replan);
    assert_eq!(m.failure_policy.on_fail, "ask_user");
}
