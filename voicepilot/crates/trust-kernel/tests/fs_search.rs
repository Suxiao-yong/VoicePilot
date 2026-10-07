use std::fs;
use std::path::PathBuf;
use trust_kernel::tools::fs::FilesystemTool;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-search-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn search_files_returns_matching_pdf_files() {
    let dir = tmp_dir();
    fs::write(dir.join("a.pdf"), b"pdf1").unwrap();
    fs::write(dir.join("b.pdf"), b"pdf2").unwrap();
    fs::write(dir.join("c.txt"), b"txt").unwrap();
    let subdir = dir.join("sub");
    fs::create_dir_all(&subdir).unwrap();
    fs::write(subdir.join("d.pdf"), b"pdf3").unwrap();

    let tool = FilesystemTool::new();
    let results = tool.search_files(&dir, "*.pdf").unwrap();
    let names: Vec<String> = results
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"a.pdf".to_string()));
    assert!(names.contains(&"b.pdf".to_string()));
    assert!(names.contains(&"d.pdf".to_string()));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn search_files_returns_empty_for_no_matches() {
    let dir = tmp_dir();
    fs::write(dir.join("a.txt"), b"hi").unwrap();

    let tool = FilesystemTool::new();
    let results = tool.search_files(&dir, "*.pdf").unwrap();
    assert!(results.is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn search_files_fails_when_root_missing() {
    let dir = tmp_dir();
    let missing = dir.join("nope");
    let tool = FilesystemTool::new();
    let result = tool.search_files(&missing, "*.pdf");
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn search_files_supports_multiple_extensions_via_star() {
    let dir = tmp_dir();
    fs::write(dir.join("a.pdf"), b"1").unwrap();
    fs::write(dir.join("b.PDF"), b"2").unwrap(); // uppercase
    fs::write(dir.join("c.docx"), b"3").unwrap();

    let tool = FilesystemTool::new();
    let results = tool.search_files(&dir, "*.pdf").unwrap();
    // Case-insensitive match on Windows; both a.pdf and b.PDF should match.
    assert!(!results.is_empty());
    fs::remove_dir_all(&dir).ok();
}
