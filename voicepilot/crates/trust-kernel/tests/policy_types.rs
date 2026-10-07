use trust_kernel::policy::types::{DLevel, Decision, ELevel, Effect, EgressDest, Resource};

#[test]
fn e_level_round_trips_through_serde() {
    let e = ELevel::E2;
    let s = serde_json::to_string(&e).unwrap();
    assert_eq!(s, "\"E2\"");
    let back: ELevel = serde_json::from_str(&s).unwrap();
    assert_eq!(back, e);
}

#[test]
fn d_level_round_trips_through_serde() {
    let d = DLevel::D3;
    let s = serde_json::to_string(&d).unwrap();
    assert_eq!(s, "\"D3\"");
    let back: DLevel = serde_json::from_str(&s).unwrap();
    assert_eq!(back, d);
}

#[test]
fn effect_serializes_as_lowercase_string() {
    assert_eq!(serde_json::to_string(&Effect::Allow).unwrap(), "\"allow\"");
    assert_eq!(
        serde_json::to_string(&Effect::Confirm).unwrap(),
        "\"confirm\""
    );
    assert_eq!(serde_json::to_string(&Effect::Deny).unwrap(), "\"deny\"");
}

#[test]
fn resource_carries_data_class_and_path() {
    let r = Resource {
        path: "/docs/readme.md".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"data_class\":\"D0\""));
    assert!(s.contains("\"path\":\"/docs/readme.md\""));
}

#[test]
fn decision_carries_effect_and_matched_policies() {
    let d = Decision {
        effect: Effect::Confirm,
        matched_policies: vec!["policy_d2_read".to_string()],
        normalized_args: serde_json::json!({"path": "/Docs/Readme.md"}),
        constraints_applied: vec!["max_files=100".to_string()],
        approval_scope: "single".to_string(),
        policy_bundle_hash: "sha256:abc".to_string(),
        reasons: vec!["D2 requires confirm".to_string()],
    };
    assert_eq!(d.effect, Effect::Confirm);
    assert_eq!(d.matched_policies, vec!["policy_d2_read".to_string()]);
    assert_eq!(d.reasons, vec!["D2 requires confirm".to_string()]);
    assert_eq!(d.approval_scope, "single");
    assert_eq!(d.policy_bundle_hash, "sha256:abc");
}

#[test]
fn egress_dest_as_str_matches_snake_case() {
    assert_eq!(EgressDest::LocalFile.as_str(), "local_file");
    assert_eq!(EgressDest::RemoteLlm.as_str(), "remote_llm");
    assert_eq!(EgressDest::RemoteMcp.as_str(), "remote_mcp");
    assert_eq!(EgressDest::ToolArgument.as_str(), "tool_argument");
}
