//! McpServerRepo::toggle_enabled 单元测试 —— V1.1.2 §8.3 Trust Center 一键停用。

use trust_kernel::db;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};

fn setup() -> rusqlite::Connection {
    let conn = db::open_in_memory().expect("open");
    db::run_migrations(&conn).expect("run_migrations");
    conn
}

fn make_record(id: &str) -> McpServerRecord {
    McpServerRecord {
        server_id: id.to_string(),
        name: format!("Test {id}"),
        version: "1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["C:/Users"]"#.to_string()),
        command: None,
        args: None,
        env: None,
    }
}

#[test]
fn toggle_enabled_flips_true_to_false() {
    let conn = setup();
    let repo = McpServerRepo::new();
    repo.create(&conn, &make_record("srv-1")).expect("create");
    repo.toggle_enabled(&conn, "srv-1", false).expect("toggle");
    let got = repo.get(&conn, "srv-1").expect("get").expect("exists");
    assert!(!got.enabled);
}

#[test]
fn toggle_enabled_flips_false_to_true() {
    let conn = setup();
    let repo = McpServerRepo::new();
    repo.create(&conn, &make_record("srv-2")).expect("create");
    repo.toggle_enabled(&conn, "srv-2", false).expect("toggle off");
    repo.toggle_enabled(&conn, "srv-2", true).expect("toggle on");
    let got = repo.get(&conn, "srv-2").expect("get").expect("exists");
    assert!(got.enabled);
}

#[test]
fn toggle_enabled_unknown_server_is_noop() {
    let conn = setup();
    let repo = McpServerRepo::new();
    repo.toggle_enabled(&conn, "nonexistent", false).expect("toggle should not error");
}
