use std::fs;
use std::io::Write;
use std::path::PathBuf;
use trust_kernel::tools::fs_paths::canonicalize;
use trust_kernel::tools::fs_snapshot::snapshot_file;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn snapshot_file_captures_size_and_sha256() {
    let dir = tmp_dir();
    let file = dir.join("hello.txt");
    let mut f = fs::File::create(&file).unwrap();
    f.write_all(b"hello world").unwrap();
    drop(f);

    let snap = snapshot_file(&file).unwrap();
    assert_eq!(snap.size, 11);
    assert!(snap.sha256.starts_with("sha256:"));
    // canonical_path must equal canonicalize(input) per V1.1 §4.4. The plan's
    // original `.to_lowercase()` over-applied to all path components on Windows;
    // canonicalize only lowercases the drive letter. Assert against canonicalize
    // directly to test the actual contract.
    assert_eq!(snap.canonical_path, canonicalize(&file.to_string_lossy()));
    assert!(!snap.last_write_time.is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn snapshot_file_fails_for_missing_file() {
    let dir = tmp_dir();
    let missing = dir.join("nope.txt");
    let result = snapshot_file(&missing);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn snapshot_file_sha256_changes_with_content() {
    let dir = tmp_dir();
    let file = dir.join("a.txt");
    fs::write(&file, b"content v1").unwrap();
    let s1 = snapshot_file(&file).unwrap();

    fs::write(&file, b"content v2").unwrap();
    let s2 = snapshot_file(&file).unwrap();

    assert_ne!(s1.sha256, s2.sha256);
    assert_eq!(s1.canonical_path, s2.canonical_path);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn snapshot_file_file_id_is_stable_across_reads() {
    let dir = tmp_dir();
    let file = dir.join("stable.txt");
    fs::write(&file, b"stable").unwrap();
    let s1 = snapshot_file(&file).unwrap();
    let s2 = snapshot_file(&file).unwrap();
    // On Windows, file_id comes from dwVolumeSerialNumber + nFileIndexHigh + nFileIndexLow.
    // Two consecutive reads should yield the same id.
    assert_eq!(s1.file_id, s2.file_id);
    fs::remove_dir_all(&dir).ok();
}
