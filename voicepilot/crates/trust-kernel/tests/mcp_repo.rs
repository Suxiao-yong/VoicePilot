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
    };
    repo.create(&k.conn(), &rec).unwrap();
    let loaded = repo.get(&k.conn(), "voicepilot-filesystem").unwrap().expect("must exist");
    assert_eq!(loaded.name, "VoicePilot Filesystem");
    assert_eq!(loaded.transport, "stdio");
    assert!(loaded.enabled);
    assert!(loaded.trusted);
    assert_eq!(loaded.protocol_version.as_deref(), Some("2025-11-25"));
    assert_eq!(loaded.allowed_paths.as_deref(), Some(r#"["C:/Users","D:/"]"#));
}

#[test]
fn repo_list_returns_all_rows() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.create(&k.conn(), &sample("s1")).unwrap();
    repo.create(&k.conn(), &sample("s2")).unwrap();
    let list = repo.list(&k.conn()).unwrap();
    assert_eq!(list.len(), 2);
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
    }
}
