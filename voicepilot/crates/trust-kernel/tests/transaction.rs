use trust_kernel::policy::transaction::{EffectManifest, PrepareToken, TransactionManager};

fn sample_manifest() -> EffectManifest {
    EffectManifest {
        sources: vec![trust_kernel::policy::transaction::FileSnapshot {
            canonical_path: "C:/Users/me/Downloads/paper1.pdf".to_string(),
            file_id: "fi_001".to_string(),
            size: 12345,
            last_write_time: "2026-07-18T10:00:00Z".to_string(),
            sha256: "a1b2c3".to_string(),
        }],
        destination: "C:/Users/me/Documents/Papers".to_string(),
        conflicts: vec![],
        total_bytes: 12345,
    }
}

#[test]
fn prepare_returns_token_and_hash() {
    let mgr = TransactionManager::new();
    let manifest = sample_manifest();
    let token = mgr.prepare("task-1", "step-1", &manifest).unwrap();
    assert!(token.token.starts_with("prt_"));
    assert!(!token.preconditions_hash.is_empty());
    assert!(token.expires_at > chrono::Utc::now());
}

#[test]
fn commit_succeeds_when_preconditions_match() {
    let mgr = TransactionManager::new();
    let manifest = sample_manifest();
    let token = mgr.prepare("task-1", "step-1", &manifest).unwrap();

    // Re-supply identical manifest at commit.
    let result = mgr.commit(&token, &manifest).unwrap();
    assert!(result.committed);
    assert_eq!(result.preconditions_recheck, token.preconditions_hash);
}

#[test]
fn commit_fails_when_preconditions_differ() {
    let mgr = TransactionManager::new();
    let manifest = sample_manifest();
    let token = mgr.prepare("task-1", "step-1", &manifest).unwrap();

    // Tamper with manifest — file size changed.
    let mut modified = manifest.clone();
    modified.sources[0].size = 99999;

    let result = mgr.commit(&token, &modified);
    assert!(result.is_err(), "commit must fail on precondition mismatch");
    let err = result.unwrap_err();
    assert!(matches!(
        err,
        trust_kernel::error::KernelError::PreconditionMismatch { .. }
    ));
}

#[test]
fn commit_fails_for_unknown_token() {
    let mgr = TransactionManager::new();
    let bogus = PrepareToken {
        token: "prt_unknown".to_string(),
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        preconditions_hash: "sha256:bogus".to_string(),
    };
    let result = mgr.commit(&bogus, &sample_manifest());
    assert!(result.is_err());
}

#[test]
fn commit_fails_for_expired_token() {
    let mgr = TransactionManager::new();
    let mut token = mgr.prepare("task-1", "step-1", &sample_manifest()).unwrap();
    token.expires_at = chrono::Utc::now() - chrono::Duration::minutes(1);
    let result = mgr.commit(&token, &sample_manifest());
    assert!(result.is_err());
}

#[test]
fn preconditions_hash_changes_when_any_source_field_changes() {
    let mgr = TransactionManager::new();
    let m1 = sample_manifest();
    let mut m2 = m1.clone();
    m2.sources[0].sha256 = "different".to_string();
    let h1 = mgr.preconditions_hash(&m1);
    let h2 = mgr.preconditions_hash(&m2);
    assert_ne!(h1, h2, "hash must change when sha256 changes");
}
