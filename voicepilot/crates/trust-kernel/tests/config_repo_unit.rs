//! ConfigRepo 单元测试 —— KV 持久化(V1.1.2 §8.3 Settings)。

use rusqlite::Connection;
use trust_kernel::db;
use trust_kernel::repo::config_repo::ConfigRepo;

fn setup() -> (Connection, ConfigRepo) {
    let conn = db::open_in_memory().expect("open_in_memory");
    db::run_migrations(&conn).expect("run_migrations");
    let repo = ConfigRepo::new();
    (conn, repo)
}

#[test]
fn config_set_and_get_roundtrip() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.model_path", "/models/tiny.bin").expect("set");
    let val = repo.get(&conn, "voice.model_path").expect("get");
    assert_eq!(val.as_deref(), Some("/models/tiny.bin"));
}

#[test]
fn config_get_returns_none_for_missing_key() {
    let (conn, repo) = setup();
    let val = repo.get(&conn, "nonexistent.key").expect("get");
    assert!(val.is_none());
}

#[test]
fn config_set_overwrites_existing_value() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.threads", "4").expect("set 1");
    repo.set(&conn, "voice.threads", "8").expect("set 2");
    let val = repo.get(&conn, "voice.threads").expect("get");
    assert_eq!(val.as_deref(), Some("8"));
}

#[test]
fn config_list_returns_all_keys() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.model_path", "/m.bin").expect("set");
    repo.set(&conn, "privacy.mode", "true").expect("set");
    repo.set(&conn, "compensation.ttl_hours", "24").expect("set");
    let all = repo.list(&conn).expect("list");
    assert_eq!(all.len(), 3);
    let keys: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
    assert!(keys.contains(&"voice.model_path"));
    assert!(keys.contains(&"privacy.mode"));
    assert!(keys.contains(&"compensation.ttl_hours"));
}

#[test]
fn config_delete_removes_key() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.threads", "4").expect("set");
    repo.delete(&conn, "voice.threads").expect("delete");
    let val = repo.get(&conn, "voice.threads").expect("get");
    assert!(val.is_none());
}

#[test]
fn config_delete_missing_key_is_noop() {
    let (conn, repo) = setup();
    repo.delete(&conn, "nonexistent").expect("delete should not error");
}
