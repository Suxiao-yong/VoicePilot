use std::fs;
use std::path::PathBuf;
use trust_kernel::allowed_paths::AllowedPaths;
use trust_kernel::tools::fs::FilesystemTool;
use trust_kernel::policy::transaction::TransactionManager;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-paths-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn allowed_paths_check_passes_for_path_under_root() {
    let root = tmp_dir();
    let allowed = AllowedPaths::new(vec![root.to_string_lossy().to_string()]);
    let file = root.join("a.txt");
    fs::write(&file, b"hi").unwrap();
    assert!(allowed.check(&file).is_ok());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn allowed_paths_check_fails_for_path_outside_root() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![root_a.to_string_lossy().to_string()]);
    let file = root_b.join("a.txt");
    fs::write(&file, b"hi").unwrap();
    assert!(allowed.check(&file).is_err());
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn allowed_paths_check_accepts_multiple_roots() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![
        root_a.to_string_lossy().to_string(),
        root_b.to_string_lossy().to_string(),
    ]);
    let f_a = root_a.join("a.txt"); fs::write(&f_a, b"a").unwrap();
    let f_b = root_b.join("b.txt"); fs::write(&f_b, b"b").unwrap();
    assert!(allowed.check(&f_a).is_ok());
    assert!(allowed.check(&f_b).is_ok());
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn filesystem_tool_with_allowed_paths_rejects_outside_source() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![root_a.to_string_lossy().to_string()]);
    let tool = FilesystemTool::new_with_allowed_paths(allowed);
    let dest = root_a.join("out"); fs::create_dir_all(&dest).unwrap();
    let src_outside = root_b.join("a.txt"); fs::write(&src_outside, b"hi").unwrap();

    let mgr = TransactionManager::new();
    let result = tool.prepare_move("t1", "s1", &[&src_outside], &dest, &mgr);
    assert!(result.is_err(), "prepare must reject source outside allowed roots");
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn filesystem_tool_with_allowed_paths_rejects_outside_destination() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![root_a.to_string_lossy().to_string()]);
    let tool = FilesystemTool::new_with_allowed_paths(allowed);
    let src = root_a.join("a.txt"); fs::write(&src, b"hi").unwrap();
    let dest_outside = root_b.join("out"); fs::create_dir_all(&dest_outside).unwrap();

    let mgr = TransactionManager::new();
    let result = tool.prepare_move("t1", "s1", &[&src], &dest_outside, &mgr);
    assert!(result.is_err(), "prepare must reject destination outside allowed roots");
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn filesystem_tool_without_allowed_paths_allows_any_path() {
    // Backward compat: new() has no allowed_paths, so any path is allowed.
    let root = tmp_dir();
    let tool = FilesystemTool::new();
    let src = root.join("a.txt"); fs::write(&src, b"hi").unwrap();
    let dest = root.join("out"); fs::create_dir_all(&dest).unwrap();

    let mgr = TransactionManager::new();
    let result = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr);
    assert!(result.is_ok(), "default FilesystemTool must allow any path");
    fs::remove_dir_all(&root).ok();
}
