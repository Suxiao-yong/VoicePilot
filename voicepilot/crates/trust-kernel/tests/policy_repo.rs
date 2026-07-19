use trust_kernel::db;
use trust_kernel::repo::policy_repo::{PolicyRecord, PolicyRepo};

fn fresh_conn() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

#[test]
fn create_policy_persists_and_can_be_loaded() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    let policy = PolicyRecord {
        policy_id: "default".to_string(),
        version: 1,
        rules_json: r#"{"rules":[]}"#.to_string(),
        hash: "sha256:abc".to_string(),
        enabled: true,
        cedar_policies: Some("permit(principal, action, resource);".to_string()),
        cedar_schema: None,
        rust_constraints: Some("max_files=100".to_string()),
    };
    repo.create(&conn, &policy).unwrap();

    let loaded = repo.get(&conn, "default").unwrap().expect("must exist");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.hash, "sha256:abc");
    assert!(loaded.enabled);
    assert!(loaded.cedar_policies.is_some());
}

#[test]
fn get_missing_policy_returns_none() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    assert!(repo.get(&conn, "nonexistent").unwrap().is_none());
}

#[test]
fn list_enabled_returns_only_enabled_policies() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    repo.create(&conn, &PolicyRecord {
        policy_id: "on".to_string(), version: 1, rules_json: "{}".to_string(),
        hash: "h1".to_string(), enabled: true,
        cedar_policies: None, cedar_schema: None, rust_constraints: None,
    }).unwrap();
    repo.create(&conn, &PolicyRecord {
        policy_id: "off".to_string(), version: 1, rules_json: "{}".to_string(),
        hash: "h2".to_string(), enabled: false,
        cedar_policies: None, cedar_schema: None, rust_constraints: None,
    }).unwrap();
    let enabled = repo.list_enabled(&conn).unwrap();
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].policy_id, "on");
}

#[test]
fn update_hash_replaces_existing() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    repo.create(&conn, &PolicyRecord {
        policy_id: "p".to_string(), version: 1, rules_json: "{}".to_string(),
        hash: "old".to_string(), enabled: true,
        cedar_policies: None, cedar_schema: None, rust_constraints: None,
    }).unwrap();
    repo.update_hash(&conn, "p", "new").unwrap();
    let loaded = repo.get(&conn, "p").unwrap().unwrap();
    assert_eq!(loaded.hash, "new");
}
