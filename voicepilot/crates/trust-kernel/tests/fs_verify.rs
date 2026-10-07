use std::fs;
use std::path::PathBuf;
use trust_kernel::policy::transaction::{EffectManifest, FileSnapshot};
use trust_kernel::toolresult::EvidenceStrength;
use trust_kernel::tools::fs::FilesystemTool;
use trust_kernel::tools::fs_snapshot::snapshot_file;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-verify-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn verify_move_strong_when_sha256_matches() {
    let dir = tmp_dir();
    let src = dir.join("a.txt");
    fs::write(&src, b"hello world").unwrap();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();
    fs::rename(&src, dest.join("a.txt")).unwrap();

    let original_snap = snapshot_file(&dir.join("a.txt")).unwrap_or_else(|_| {
        // src is now gone — build a synthetic snapshot for the test.
        // In real flow, the manifest comes from prepare_move before the move.
        FileSnapshot {
            canonical_path: "c:/placeholder/a.txt".to_string(),
            file_id: "test".to_string(),
            size: 11,
            last_write_time: "2026-07-19T00:00:00Z".to_string(),
            sha256: "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
                .to_string(),
        }
    });

    let manifest = EffectManifest {
        sources: vec![original_snap],
        destination: dest.to_string_lossy().replace('\\', "/").to_lowercase(),
        conflicts: vec![],
        total_bytes: 11,
    };

    let tool = FilesystemTool::new();
    let result = tool.verify_move(&manifest).unwrap();
    assert!(result.verified);
    assert_eq!(result.evidence_strength, EvidenceStrength::Strong);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn verify_move_fails_when_destination_missing() {
    let dir = tmp_dir();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();

    let manifest = EffectManifest {
        sources: vec![FileSnapshot {
            canonical_path: "c:/fake/source.txt".to_string(),
            file_id: "fake".to_string(),
            size: 5,
            last_write_time: "2026-07-19T00:00:00Z".to_string(),
            sha256: "sha256:fakehash".to_string(),
        }],
        destination: dest.to_string_lossy().replace('\\', "/").to_lowercase(),
        conflicts: vec![],
        total_bytes: 5,
    };

    let tool = FilesystemTool::new();
    let result = tool.verify_move(&manifest);
    assert!(
        result.is_err(),
        "verify must fail when dest file is missing"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn verify_move_fails_when_sha256_mismatches() {
    let dir = tmp_dir();
    let dest = dir.join("out");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("source.txt"), b"different content").unwrap();

    let manifest = EffectManifest {
        sources: vec![FileSnapshot {
            canonical_path: "c:/fake/source.txt".to_string(),
            file_id: "fake".to_string(),
            size: 99,
            last_write_time: "2026-07-19T00:00:00Z".to_string(),
            sha256: "sha256:expectednotmatching".to_string(),
        }],
        destination: dest.to_string_lossy().replace('\\', "/").to_lowercase(),
        conflicts: vec![],
        total_bytes: 99,
    };

    let tool = FilesystemTool::new();
    let result = tool.verify_move(&manifest);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}
