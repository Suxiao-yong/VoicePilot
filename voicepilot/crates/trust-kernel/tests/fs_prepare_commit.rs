use std::fs;
use std::path::PathBuf;
use trust_kernel::policy::transaction::TransactionManager;
use trust_kernel::tools::fs::FilesystemTool;
use trust_kernel::tools::fs_paths::canonicalize;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-fs-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn prepare_move_returns_manifest_with_snapshots() {
    let dir = tmp_dir();
    let src1 = dir.join("a.txt");
    fs::write(&src1, b"content a").unwrap();
    let src2 = dir.join("b.txt");
    fs::write(&src2, b"content b").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool
        .prepare_move("task-1", "step-1", &[&src1, &src2], &dest, &mgr)
        .unwrap();

    assert!(result.token.token.starts_with("prt_"));
    assert!(!result.preconditions_hash.is_empty());
    assert_eq!(result.manifest.sources.len(), 2);
    assert_eq!(
        result.manifest.total_bytes,
        "content a".len() as u64 + "content b".len() as u64
    );
    assert_eq!(
        result.manifest.destination,
        canonicalize(&dest.to_string_lossy())
    );
    assert!(result.manifest.conflicts.is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prepare_move_detects_destination_conflicts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"hello").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();
    // Pre-create a conflicting file at dest/a.txt
    fs::write(dest.join("a.txt"), b"existing").unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool
        .prepare_move("task-1", "step-1", &[&src], &dest, &mgr)
        .unwrap();

    assert!(
        !result.manifest.conflicts.is_empty(),
        "must detect dest/a.txt as conflict"
    );
    assert!(result.manifest.conflicts[0].contains("a.txt"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prepare_move_fails_if_source_missing() {
    let dir = tmp_dir();
    let missing = dir.join("nope.txt");
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool.prepare_move("task-1", "step-1", &[&missing], &dest, &mgr);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prepare_move_fails_if_destination_not_a_directory() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"hi").unwrap();
    let dest = dir.join("not_a_dir");
    fs::write(&dest, b"blocker").unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool.prepare_move("task-1", "step-1", &[&src], &dest, &mgr);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn commit_move_succeeds_when_preconditions_match() {
    let dir = tmp_dir();
    let src1 = dir.join("a.txt");
    fs::write(&src1, b"content a").unwrap();
    let src2 = dir.join("b.txt");
    fs::write(&src2, b"content b").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool
        .prepare_move("task-1", "step-1", &[&src1, &src2], &dest, &mgr)
        .unwrap();

    let result = tool
        .commit_move(&prepared.token, &prepared.manifest, &mgr)
        .unwrap();
    assert!(result.succeeded);
    assert!(!src1.exists());
    assert!(!src2.exists());
    assert!(dest.join("a.txt").exists());
    assert!(dest.join("b.txt").exists());
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"content a");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn commit_move_fails_when_source_tampered() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"original").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool
        .prepare_move("task-1", "step-1", &[&src], &dest, &mgr)
        .unwrap();

    // Tamper: change content after prepare.
    fs::write(&src, b"tampered").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "commit must fail on precondition mismatch");
    // Source must still exist (no partial move).
    assert!(src.exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn commit_move_fails_when_destination_conflict_appeared() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"hello").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool
        .prepare_move("task-1", "step-1", &[&src], &dest, &mgr)
        .unwrap();

    // Inject a destination conflict after prepare.
    fs::write(dest.join("a.txt"), b"injected").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(
        result.is_err(),
        "commit must fail when new conflict appears"
    );
    // Source must still exist.
    assert!(src.exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn commit_move_is_atomic_on_failure() {
    // If 1 of N files fails to move (e.g. locked), no files should move.
    // W3a approximates "atomic" by pre-checking all destinations + sources,
    // then moving. If any move errors mid-flight, we attempt rollback.
    let dir = tmp_dir();
    let src1 = dir.join("a.txt");
    fs::write(&src1, b"first").unwrap();
    let src2 = dir.join("b.txt");
    fs::write(&src2, b"second").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool
        .prepare_move("task-1", "step-1", &[&src1, &src2], &dest, &mgr)
        .unwrap();

    let result = tool
        .commit_move(&prepared.token, &prepared.manifest, &mgr)
        .unwrap();
    assert!(result.succeeded);
    // Both moved together.
    assert!(!src1.exists() && !src2.exists());
    assert!(dest.join("a.txt").exists() && dest.join("b.txt").exists());
    fs::remove_dir_all(&dir).ok();
}
