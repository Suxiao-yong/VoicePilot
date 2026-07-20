use trust_kernel::policy::types::{DLevel, ELevel};
use trust_kernel::skills::manifest::{
    files_organize_manifest, ApprovalMode, CompensationConfig, FailurePolicy,
    SkillInput, SkillInputType, SkillManifest, VerifierConfig,
};

#[test]
fn files_organize_manifest_has_correct_metadata() {
    let m = files_organize_manifest();
    assert_eq!(m.id, "files.organize");
    assert_eq!(m.version, "1.0.0");
    assert_eq!(m.title, "整理文件");
    assert!(!m.intent_examples.is_empty());
    assert_eq!(m.risk_ceiling, ELevel::E2);
    assert_eq!(m.data_class_ceiling, DLevel::D2);
    assert_eq!(m.egress.as_str(), "local_only");
    assert_eq!(m.max_steps, 4);
}

#[test]
fn files_organize_manifest_declares_tool_whitelist() {
    let m = files_organize_manifest();
    assert!(m.tools.contains(&"filesystem.search_files".to_string()));
    assert!(m.tools.contains(&"filesystem.prepare_move".to_string()));
    assert!(m.tools.contains(&"filesystem.commit_move".to_string()));
    assert!(m.tools.contains(&"filesystem.verify_move".to_string()));
    // No shell_exec, no send_to_remote_llm — must be a closed whitelist.
    assert!(!m.tools.iter().any(|t| t == "shell_exec"));
}

#[test]
fn files_organize_manifest_uses_batch_once_approval() {
    let m = files_organize_manifest();
    assert_eq!(m.approval.mode, ApprovalMode::BatchOnce);
    assert_eq!(m.approval.required_for, "commit");
    assert!(m.approval.show_effect_manifest);
    assert_eq!(m.approval.max_approval_scope, 3);
}

#[test]
fn files_organize_manifest_declares_strong_compensation() {
    let m = files_organize_manifest();
    assert_eq!(m.compensation.level, trust_kernel::compensation::types::CompensationLevel::Strong);
    assert_eq!(m.compensation.ttl_seconds, 3600);
    assert_eq!(
        m.compensation.conflict_policy,
        trust_kernel::compensation::types::ConflictPolicy::RequireConfirmation
    );
}

#[test]
fn files_organize_manifest_declares_inputs_with_allowed_roots() {
    let m = files_organize_manifest();
    let source = m.inputs.get("source").expect("source input required");
    assert_eq!(source.input_type, SkillInputType::Directory);
    assert!(!source.allowed_roots.is_empty());

    let dest = m.inputs.get("destination").expect("destination input required");
    assert_eq!(dest.input_type, SkillInputType::Directory);
    assert!(!dest.allowed_roots.is_empty());

    let filter = m.inputs.get("filter").expect("filter input required");
    assert_eq!(filter.input_type, SkillInputType::FileFilter);
}

#[test]
fn files_organize_manifest_disallows_replan() {
    let m = files_organize_manifest();
    assert!(!m.failure_policy.allow_replan);
    assert_eq!(m.failure_policy.max_retries, 1);
}
