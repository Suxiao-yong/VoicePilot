//! SkillRepo 单元测试 —— V1.1.2 §8.3 Skills Manager。

use trust_kernel::db;
use trust_kernel::skills::repo::{SkillRecord, SkillRepo};

fn make_record(id: &str) -> SkillRecord {
    SkillRecord {
        skill_id: id.to_string(),
        version: 1,
        manifest_json: r#"{"id":"test"}"#.to_string(),
        enabled: true,
        success_count: 0,
        avg_latency_ms: 0.0,
    }
}

#[test]
fn skill_upsert_creates_new_record() {
    let conn = db::open_in_memory().expect("open");
    db::run_migrations(&conn).expect("run_migrations");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("files.organize")).expect("upsert");
    let got = repo.get(&conn, "files.organize").expect("get").expect("exists");
    assert_eq!(got.skill_id, "files.organize");
    assert_eq!(got.success_count, 0);
}

#[test]
fn skill_upsert_overwrites_existing() {
    let conn = db::open_in_memory().expect("open");
    db::run_migrations(&conn).expect("run_migrations");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("files.organize")).expect("upsert 1");
    let mut rec = make_record("files.organize");
    rec.version = 2;
    repo.upsert(&conn, &rec).expect("upsert 2");
    let got = repo.get(&conn, "files.organize").expect("get").expect("exists");
    assert_eq!(got.version, 2);
}

#[test]
fn skill_list_returns_all() {
    let conn = db::open_in_memory().expect("open");
    db::run_migrations(&conn).expect("run_migrations");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("skill-1")).expect("upsert");
    repo.upsert(&conn, &make_record("skill-2")).expect("upsert");
    let all = repo.list(&conn).expect("list");
    assert_eq!(all.len(), 2);
}

#[test]
fn skill_toggle_flips_enabled() {
    let conn = db::open_in_memory().expect("open");
    db::run_migrations(&conn).expect("run_migrations");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("skill-1")).expect("upsert");
    repo.toggle(&conn, "skill-1", false).expect("toggle");
    let got = repo.get(&conn, "skill-1").expect("get").expect("exists");
    assert!(!got.enabled);
}

#[test]
fn skill_incr_success_increments_count_and_updates_avg_latency() {
    let conn = db::open_in_memory().expect("open");
    db::run_migrations(&conn).expect("run_migrations");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("skill-1")).expect("upsert");
    repo.incr_success(&conn, "skill-1", 1000.0).expect("incr 1");
    repo.incr_success(&conn, "skill-1", 2000.0).expect("incr 2");
    let got = repo.get(&conn, "skill-1").expect("get").expect("exists");
    assert_eq!(got.success_count, 2);
    assert!((got.avg_latency_ms - 1500.0).abs() < 0.1);
}
