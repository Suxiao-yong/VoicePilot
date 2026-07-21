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

    let _ = registry.take_sender(&approval_id).expect("first take succeeds");

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
