use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};

#[test]
fn repo_create_and_get_round_trips() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "voicepilot-filesystem".to_string(),
        name: "VoicePilot Filesystem".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["C:/Users","D:/"]"#.to_string()),
        command: None,
        args: None,
        env: None,
    };
    repo.create(&k.conn(), &rec).unwrap();
    let loaded = repo
        .get(&k.conn(), "voicepilot-filesystem")
        .unwrap()
        .expect("must exist");
    assert_eq!(loaded.name, "VoicePilot Filesystem");
    assert_eq!(loaded.transport, "stdio");
    assert!(loaded.enabled);
    assert!(loaded.trusted);
    assert_eq!(loaded.protocol_version.as_deref(), Some("2025-11-25"));
    assert_eq!(
        loaded.allowed_paths.as_deref(),
        Some(r#"["C:/Users","D:/"]"#)
    );
}

#[test]
fn repo_list_returns_all_rows() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    // boot() already inserted the default rows (`playwright` W7 Plan 5 Task 2
    // + `mcp-windows` UIA backend), so the table starts with 2 rows;
    // we add 2 more and expect 4 total.
    repo.create(&k.conn(), &sample("s1")).unwrap();
    repo.create(&k.conn(), &sample("s2")).unwrap();
    let list = repo.list(&k.conn()).unwrap();
    assert_eq!(list.len(), 4);
}

#[test]
fn repo_update_changes_fields() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.create(&k.conn(), &sample("s1")).unwrap();
    let mut rec = repo.get(&k.conn(), "s1").unwrap().unwrap();
    rec.enabled = false;
    rec.allowed_paths = Some(r#"["D:/only"]"#.to_string());
    repo.update(&k.conn(), &rec).unwrap();
    let loaded = repo.get(&k.conn(), "s1").unwrap().unwrap();
    assert!(!loaded.enabled);
    assert_eq!(loaded.allowed_paths.as_deref(), Some(r#"["D:/only"]"#));
}

#[test]
fn repo_delete_removes_row() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.create(&k.conn(), &sample("s1")).unwrap();
    repo.delete(&k.conn(), "s1").unwrap();
    assert!(repo.get(&k.conn(), "s1").unwrap().is_none());
}

#[test]
fn repo_get_returns_none_for_missing() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    assert!(repo.get(&k.conn(), "nonexistent").unwrap().is_none());
}

fn sample(id: &str) -> McpServerRecord {
    McpServerRecord {
        server_id: id.to_string(),
        name: format!("Server {}", id),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: None,
        command: None,
        args: None,
        env: None,
    }
}

use std::path::Path;

#[test]
fn load_allowed_paths_returns_canonicalized_roots() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "s1".to_string(),
        name: "S1".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["C:/Users","D:/voicepilot"]"#.to_string()),
        command: None,
        args: None,
        env: None,
    };
    repo.create(&k.conn(), &rec).unwrap();

    let allowed = repo
        .load_allowed_paths(&k.conn(), "s1")
        .unwrap()
        .expect("must exist");
    // Path under root C:/Users should be allowed.
    assert!(allowed.check(Path::new("C:/Users/me/file.txt")).is_ok());
    // Path outside all roots should be rejected.
    assert!(allowed.check(Path::new("E:/elsewhere")).is_err());
}

#[test]
fn load_allowed_paths_returns_none_when_column_empty() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "s2".to_string(),
        name: "S2".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: None,
        allowed_origins: None,
        allowed_paths: None,
        command: None,
        args: None,
        env: None,
    };
    repo.create(&k.conn(), &rec).unwrap();
    assert!(repo.load_allowed_paths(&k.conn(), "s2").unwrap().is_none());
}

#[test]
fn load_allowed_paths_returns_none_when_server_missing() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    assert!(repo
        .load_allowed_paths(&k.conn(), "nonexistent")
        .unwrap()
        .is_none());
}

#[test]
fn load_allowed_paths_returns_err_on_invalid_json() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "s3".to_string(),
        name: "S3".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: None,
        allowed_origins: None,
        allowed_paths: Some("not valid json".to_string()),
        command: None,
        args: None,
        env: None,
    };
    repo.create(&k.conn(), &rec).unwrap();
    assert!(repo.load_allowed_paths(&k.conn(), "s3").is_err());
}

#[test]
fn repo_seed_builtin_creates_row_if_missing() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    // Confirm row does not exist yet.
    assert!(repo
        .get(&k.conn(), "voicepilot-filesystem")
        .unwrap()
        .is_none());
    // Seed.
    repo.seed_builtin_filesystem(&k.conn()).unwrap();
    let rec = repo
        .get(&k.conn(), "voicepilot-filesystem")
        .unwrap()
        .expect("row must exist after seed");
    assert_eq!(rec.name, "VoicePilot Filesystem");
    assert_eq!(rec.protocol_version.as_deref(), Some("2025-11-25"));
    assert_eq!(rec.transport, "stdio");
    assert!(rec.enabled);
    assert!(rec.trusted);
    // allowed_paths should be a non-empty JSON array.
    assert!(rec.allowed_paths.is_some());
}

#[test]
fn repo_seed_builtin_is_idempotent() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.seed_builtin_filesystem(&k.conn()).unwrap();
    // Modify the row.
    let mut rec = repo
        .get(&k.conn(), "voicepilot-filesystem")
        .unwrap()
        .unwrap();
    rec.allowed_paths = Some(r#"["D:/custom"]"#.to_string());
    repo.update(&k.conn(), &rec).unwrap();
    // Seed again — must NOT overwrite the custom allowed_paths.
    repo.seed_builtin_filesystem(&k.conn()).unwrap();
    let loaded = repo
        .get(&k.conn(), "voicepilot-filesystem")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.allowed_paths.as_deref(), Some(r#"["D:/custom"]"#));
}

// W7 Plan 5 Task 1: insert_default_servers (playwright)

#[test]
fn repo_insert_default_servers_creates_playwright_row() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    // boot() already inserted the `playwright` default row (W7 Plan 5 Task 2),
    // so the row exists before we call insert_default_servers again here.
    // This test verifies the row's fields match the spec §2.7 spawn contract;
    // idempotency (re-insert does not overwrite) is covered by the next test.
    repo.insert_default_servers(&k.conn()).unwrap();
    let rec = repo
        .get(&k.conn(), "playwright")
        .unwrap()
        .expect("playwright row must exist after insert_default_servers");
    assert_eq!(rec.server_id, "playwright");
    assert_eq!(rec.name, "Playwright MCP");
    assert_eq!(rec.transport, "stdio");
    assert!(rec.enabled);
    assert!(!rec.trusted);
    assert_eq!(rec.protocol_version.as_deref(), Some("2025-11-25"));
    // allowed_paths is empty array — Playwright MCP has no filesystem access.
    assert_eq!(rec.allowed_paths.as_deref(), Some("[]"));
    // Spawn spec per spec §2.7.
    assert_eq!(rec.command.as_deref(), Some("npx"));
    assert_eq!(
        rec.args.as_deref(),
        Some(r#"["-y","@playwright/mcp@latest"]"#)
    );
    assert_eq!(rec.env.as_deref(), Some("{}"));
}

#[test]
fn repo_insert_default_servers_creates_mcp_windows_row() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.insert_default_servers(&k.conn()).unwrap();
    let rec = repo
        .get(&k.conn(), "mcp-windows")
        .unwrap()
        .expect("mcp-windows row must exist after insert_default_servers");
    assert_eq!(rec.transport, "stdio");
    assert!(rec.enabled);
    assert!(!rec.trusted);
    assert_eq!(rec.command.as_deref(), Some("Sbroenne.WindowsMcp.exe"));
    assert_eq!(rec.args.as_deref(), Some("[]"));
    assert_eq!(rec.env.as_deref(), Some("{}"));
}

#[test]
fn repo_insert_default_servers_is_idempotent_and_preserves_user_customization() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.insert_default_servers(&k.conn()).unwrap();
    // User disables playwright via Trust Center.
    let mut rec = repo.get(&k.conn(), "playwright").unwrap().unwrap();
    rec.enabled = false;
    repo.update(&k.conn(), &rec).unwrap();
    // Insert defaults again — must NOT overwrite the user's `enabled = false`.
    repo.insert_default_servers(&k.conn()).unwrap();
    let loaded = repo.get(&k.conn(), "playwright").unwrap().unwrap();
    assert!(!loaded.enabled);
}
