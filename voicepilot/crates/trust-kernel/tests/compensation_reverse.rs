use std::fs;
use std::path::PathBuf;
use trust_kernel::compensation::executor::auto_reverse_move;
use trust_kernel::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-rev-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample_record(reverse_payload: &str) -> CompensationRecord {
    CompensationRecord {
        comp_id: "comp-1".to_string(),
        step_id: "step-1".to_string(),
        level: CompensationLevel::Strong,
        snapshot_encrypted: None,
        ttl_expires: "2026-07-19T16:00:00Z".to_string(),
        status: "active".to_string(),
        snapshot_vault_ref: None,
        conflict_policy: ConflictPolicy::AutoReverse,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: reverse_payload.to_string(),
    }
}

#[test]
fn auto_reverse_moves_files_back_to_original_locations() {
    let dir = tmp_dir();
    let original = dir.join("orig.txt");
    let moved_to = dir.join("moved.txt");
    fs::write(&moved_to, b"hello").unwrap();

    let payload = serde_json::json!({
        "moves": [
            {"from": original.to_string_lossy().replace('\\', "/"),
             "to": moved_to.to_string_lossy().replace('\\', "/")}
        ]
    }).to_string();

    let rec = sample_record(&payload);
    auto_reverse_move(&rec).unwrap();

    assert!(original.exists(), "original location must have the file back");
    assert!(!moved_to.exists(), "moved location must be empty");
    assert_eq!(fs::read(&original).unwrap(), b"hello");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn auto_reverse_fails_when_move_target_missing() {
    let dir = tmp_dir();
    let missing = dir.join("nonexistent.txt");
    let original = dir.join("orig.txt");

    let payload = serde_json::json!({
        "moves": [
            {"from": original.to_string_lossy().replace('\\', "/"),
             "to": missing.to_string_lossy().replace('\\', "/")}
        ]
    }).to_string();

    let rec = sample_record(&payload);
    let result = auto_reverse_move(&rec);
    assert!(result.is_err(), "reverse must fail when current location is missing");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn auto_reverse_skips_when_payload_empty() {
    let rec = sample_record("{}");
    let result = auto_reverse_move(&rec);
    assert!(result.is_ok(), "empty payload should be a no-op");
}

#[test]
fn auto_reverse_rolls_back_partial_on_failure() {
    let dir = tmp_dir();
    let ok_current = dir.join("ok_moved.txt");
    let ok_original = dir.join("ok_orig.txt");
    fs::write(&ok_current, b"ok content").unwrap();

    let bad_current = dir.join("bad_moved.txt"); // doesn't exist
    let bad_original = dir.join("bad_orig.txt");

    let payload = serde_json::json!({
        "moves": [
            {"from": ok_original.to_string_lossy().replace('\\', "/"),
             "to": ok_current.to_string_lossy().replace('\\', "/")},
            {"from": bad_original.to_string_lossy().replace('\\', "/"),
             "to": bad_current.to_string_lossy().replace('\\', "/")}
        ]
    }).to_string();

    let rec = sample_record(&payload);
    let result = auto_reverse_move(&rec);
    assert!(result.is_err(), "must fail on bad move");
    // Rollback: ok_current should be back where it was.
    assert!(ok_current.exists(), "partial rollback must restore already-reversed file");
    assert!(!ok_original.exists());
    fs::remove_dir_all(&dir).ok();
}
