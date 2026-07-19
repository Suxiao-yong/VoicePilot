use trust_kernel::gateway::ActionGateway;
use trust_kernel::policy::types::{DLevel, ELevel, EgressDest, Resource};
use trust_kernel::policy::constraint_engine::ConstraintSpec;

fn load_gateway() -> ActionGateway {
    let cedar_src = include_str!("../src/policies/default.cedar");
    let mut gw = ActionGateway::new(cedar_src).unwrap();
    // Register a max_files constraint for move_files.
    gw.register_constraint("move_files", ConstraintSpec {
        max_files: Some(2),
        overwrite: Some(false),
        allowed_destinations: None,
    });
    gw
}

#[test]
fn d0_public_read_is_allowed() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/readme.md".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Allow);
}

#[test]
fn d3_credential_read_is_denied() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/secrets/token.txt".to_string(),
        data_class: DLevel::D3,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Deny);
    assert!(decision.reasons.iter().any(|r| r.contains("D3") || r.contains("deny")));
}

#[test]
fn d2_private_read_is_confirm() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/private.md".to_string(),
        data_class: DLevel::D2,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Confirm);
}

#[test]
fn egress_to_remote_llm_with_d2_is_confirm() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/private.md".to_string(),
        data_class: DLevel::D2,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("send_to_remote_llm", ELevel::E3, &resource,
                             Some(EgressDest::RemoteLlm), None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Confirm);
}

#[test]
fn egress_to_tool_argument_is_always_denied() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/web/page".to_string(),
        data_class: DLevel::D0,
        provenance: "web_page".to_string(),
    };
    let decision = gw.decide("inject_arg", ELevel::E0, &resource,
                             Some(EgressDest::ToolArgument), None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Deny);
}

#[test]
fn shell_exec_is_always_denied() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/bin/sh".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("shell_exec", ELevel::E3, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Deny);
}

#[test]
fn decision_includes_policy_bundle_hash() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/readme.md".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert!(decision.policy_bundle_hash.starts_with("sha256:"));
}

#[test]
fn constraints_appear_in_decision_when_max_files_triggers() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/file.txt".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let args = serde_json::json!({
        "sources": ["a.txt", "b.txt", "c.txt", "d.txt"],
        "destination": "/out"
    });
    let decision = gw.decide("move_files", ELevel::E1, &resource, None, Some(args)).unwrap();
    assert!(decision.constraints_applied.iter().any(|c| c.contains("max_files=2")));
}
