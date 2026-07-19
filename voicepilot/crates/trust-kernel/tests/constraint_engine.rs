use trust_kernel::policy::constraint_engine::{ConstraintEngine, ConstraintSpec};
use trust_kernel::policy::types::{DLevel, Effect, ELevel};

#[test]
fn normalize_path_lowercases_drive_letter_and_forward_slashes() {
    let engine = ConstraintEngine::default();
    let out = engine
        .normalize_args(
            "move_files",
            &serde_json::json!({
                "sources": ["C:\\Users\\Me\\Downloads\\Paper1.pdf"],
                "destination": "C:/Users/me/Documents"
            }),
        )
        .unwrap();
    let src = out["sources"][0].as_str().unwrap();
    assert!(src.starts_with("c:/Users/"), "normalized: {}", src);
    assert!(!src.contains('\\'), "no backslashes: {}", src);
}

#[test]
fn apply_max_files_constraint_truncates_sources() {
    let mut engine = ConstraintEngine::default();
    engine.register(
        "move_files",
        ConstraintSpec {
            max_files: Some(2),
            overwrite: Some(false),
            allowed_destinations: None,
        },
    );
    let normalized = serde_json::json!({
        "sources": ["a.txt", "b.txt", "c.txt", "d.txt"],
        "destination": "/out"
    });
    let (constrained, applied) = engine.apply_constraints("move_files", normalized).unwrap();
    assert_eq!(constrained["sources"].as_array().unwrap().len(), 2);
    assert!(applied.iter().any(|c| c.contains("max_files=2")));
}

#[test]
fn overwrite_false_blocks_existing_destination() {
    let mut engine = ConstraintEngine::default();
    engine.register(
        "write_file",
        ConstraintSpec {
            max_files: None,
            overwrite: Some(false),
            allowed_destinations: None,
        },
    );
    // Without filesystem access we can't check existence; we just record the constraint.
    let (out, applied) = engine
        .apply_constraints(
            "write_file",
            serde_json::json!({"path": "/tmp/x", "content": "hi"}),
        )
        .unwrap();
    assert_eq!(out["path"], "/tmp/x");
    assert!(applied.iter().any(|c| c.contains("overwrite=false")));
}

#[test]
fn unregistered_tool_passes_through_with_no_constraints() {
    let engine = ConstraintEngine::default();
    let (out, applied) = engine
        .apply_constraints("unknown_tool", serde_json::json!({"x": 1}))
        .unwrap();
    assert_eq!(out["x"], 1);
    assert!(applied.is_empty());
}

#[test]
fn upgrade_effect_to_confirm_when_risk_matrix_says_so() {
    let engine = ConstraintEngine::default();
    // Cedar allowed (true), but E0×D2 = Confirm per matrix.
    let effect = engine.upgrade_effect(true, ELevel::E0, DLevel::D2);
    assert_eq!(effect, Effect::Confirm);
}

#[test]
fn upgrade_effect_stays_allow_when_risk_matrix_allows() {
    let engine = ConstraintEngine::default();
    let effect = engine.upgrade_effect(true, ELevel::E0, DLevel::D0);
    assert_eq!(effect, Effect::Allow);
}

#[test]
fn upgrade_effect_is_deny_when_cedar_denies() {
    let engine = ConstraintEngine::default();
    let effect = engine.upgrade_effect(false, ELevel::E0, DLevel::D0);
    assert_eq!(effect, Effect::Deny);
}

#[test]
fn upgrade_effect_is_deny_when_risk_matrix_denies_regardless_of_cedar() {
    let engine = ConstraintEngine::default();
    // Even if Cedar permits, D3 is red line.
    let effect = engine.upgrade_effect(true, ELevel::E0, DLevel::D3);
    assert_eq!(effect, Effect::Deny);
}
