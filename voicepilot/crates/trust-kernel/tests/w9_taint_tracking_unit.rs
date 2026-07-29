//! W9 Plan 3 Task 2 — TaintRepo CRUD 单元测试(11 个)。
//! 测试 taints 表的 upsert / find_by_value / find_by_hash /
//! list_by_provenance / list_by_source / delete_by_source +
//! 合并去重 / value_hash / source_ref / 级联删除 / 空 taints / now_iso8601。

use rusqlite::Connection;

use trust_kernel::policy::taint_repo::{
    compute_value_hash, make_taint_record, merge_taints, now_iso8601, TaintRepo,
};

/// W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,
/// 此 helper 将 &str 转为 Value(JSON 字符串 parse,普通字符串包装为 Value::String)。
fn to_value(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.to_string()))
}

/// 建内存 DB 并跑 001_init.sql + 006_taints_unique_index.sql migration。
fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    let sql = include_str!("../src/migrations/001_init.sql");
    conn.execute_batch(sql).unwrap();
    let idx = include_str!("../src/migrations/006_taints_unique_index.sql");
    conn.execute_batch(idx).unwrap();
    conn
}

/// 测试 1:upsert 插入新记录,find_by_value 能查到。
#[test]
fn test_upsert_and_find_by_value() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = r#"{"url": "https://example.com"}"#;
    let hash = compute_value_hash(&to_value(value));
    let rec = make_taint_record(
        hash.clone(),
        "web_page".into(),
        vec!["web_page".into()],
        Some("task-1:step-1".into()),
    );
    repo.upsert(&conn, &rec).unwrap();

    let found = repo.find_by_value(&conn, value).unwrap();
    assert!(found.is_some(), "find_by_value 必须查到刚 upsert 的记录");
    let found = found.unwrap();
    assert_eq!(found.value_hash, hash);
    assert_eq!(found.provenance, "web_page");
    assert_eq!(found.taints, vec!["web_page".to_string()]);
}

/// 测试 2:list_by_provenance 按 provenance 过滤。
#[test]
fn test_list_by_provenance() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    // 插入 3 条,2 条 web_page,1 条 llm_output
    for v in ["value-a", "value-b"] {
        let h = compute_value_hash(&to_value(v));
        repo.upsert(
            &conn,
            &make_taint_record(h, "web_page".into(), vec!["web_page".into()], None),
        )
        .unwrap();
    }
    let h = compute_value_hash(&to_value("value-c"));
    repo.upsert(
        &conn,
        &make_taint_record(h, "llm_output".into(), vec!["llm_output".into()], None),
    )
    .unwrap();

    let web_pages = repo.list_by_provenance(&conn, "web_page").unwrap();
    assert_eq!(web_pages.len(), 2, "web_page provenance 应有 2 条");
    let llm_outputs = repo.list_by_provenance(&conn, "llm_output").unwrap();
    assert_eq!(llm_outputs.len(), 1, "llm_output provenance 应有 1 条");
}

/// 测试 3:list_by_source 按 source_ref 过滤。
#[test]
fn test_list_by_source() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    for v in ["v1", "v2", "v3"] {
        let h = compute_value_hash(&to_value(v));
        repo.upsert(
            &conn,
            &make_taint_record(
                h,
                "user_input".into(),
                vec!["user_input".into()],
                Some("task-42".into()),
            ),
        )
        .unwrap();
    }
    // 另一个 source_ref 的记录
    let h = compute_value_hash(&to_value("v4"));
    repo.upsert(
        &conn,
        &make_taint_record(
            h,
            "user_input".into(),
            vec!["user_input".into()],
            Some("task-99".into()),
        ),
    )
    .unwrap();

    let task42 = repo.list_by_source(&conn, "task-42").unwrap();
    assert_eq!(task42.len(), 3, "task-42 source_ref 应有 3 条");
}

/// 测试 4:delete_by_source 删除并返回行数。
#[test]
fn test_delete_by_source() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    for v in ["v1", "v2"] {
        let h = compute_value_hash(&to_value(v));
        repo.upsert(
            &conn,
            &make_taint_record(h, "x".into(), vec!["x".into()], Some("task-1".into())),
        )
        .unwrap();
    }
    let deleted = repo.delete_by_source(&conn, "task-1").unwrap();
    assert_eq!(deleted, 2, "应删除 2 条");
    let remaining = repo.list_by_source(&conn, "task-1").unwrap();
    assert!(remaining.is_empty(), "删除后应查不到");
}

/// 测试 5:upsert 同 value_hash 时合并 taints(去重)。
#[test]
fn test_upsert_merges_taints_dedup() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = "shared-value";
    let hash = compute_value_hash(&to_value(value));

    // 第一次:标 web_page
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "web_page".into(), vec!["web_page".into()], None),
    )
    .unwrap();
    // 第二次:同 value,标 executor_output:skill-1
    repo.upsert(
        &conn,
        &make_taint_record(
            hash.clone(),
            "executor_output:skill-1".into(),
            vec!["executor_output:skill-1".into()],
            None,
        ),
    )
    .unwrap();

    let found = repo.find_by_value(&conn, value).unwrap().unwrap();
    assert_eq!(found.taints.len(), 2, "合并后应有 2 个 taint");
    assert!(found.taints.contains(&"web_page".to_string()));
    assert!(found.taints.contains(&"executor_output:skill-1".to_string()));
    // 不应产生重复行
    let all = repo.list_by_provenance(&conn, "web_page").unwrap();
    assert_eq!(all.len(), 1, "同 value_hash 只应有 1 行(provenance 保留原值)");
}

/// 测试 6:compute_value_hash 对相同输入稳定,对不同输入不同。
#[test]
fn test_compute_value_hash_stable_and_distinct() {
    let h1a = compute_value_hash(&to_value("hello"));
    let h1b = compute_value_hash(&to_value("hello"));
    let h2 = compute_value_hash(&to_value("world"));
    assert_eq!(h1a, h1b, "相同输入 hash 必须相同");
    assert_ne!(h1a, h2, "不同输入 hash 必须不同");
    assert_eq!(h1a.len(), 64, "SHA256 hex 应为 64 字符");
}

/// 测试 7:source_ref 关联 + None 处理。
#[test]
fn test_source_ref_none_and_some() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    // None source_ref
    let h1 = compute_value_hash(&to_value("v-no-src"));
    repo.upsert(
        &conn,
        &make_taint_record(h1, "p".into(), vec!["p".into()], None),
    )
    .unwrap();
    // Some source_ref
    let h2 = compute_value_hash(&to_value("v-with-src"));
    repo.upsert(
        &conn,
        &make_taint_record(h2, "p".into(), vec!["p".into()], Some("src-1".into())),
    )
    .unwrap();

    let no_src = repo.list_by_source(&conn, "src-1").unwrap();
    assert_eq!(no_src.len(), 1, "只有 1 条带 source_ref=src-1");
}

/// 测试 8:delete_by_source 不影响其他 source_ref(级联精确性)。
#[test]
fn test_delete_by_source_does_not_touch_others() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let h1 = compute_value_hash(&to_value("v1"));
    repo.upsert(
        &conn,
        &make_taint_record(h1, "p".into(), vec!["p".into()], Some("task-A".into())),
    )
    .unwrap();
    let h2 = compute_value_hash(&to_value("v2"));
    repo.upsert(
        &conn,
        &make_taint_record(h2, "p".into(), vec!["p".into()], Some("task-B".into())),
    )
    .unwrap();

    let deleted = repo.delete_by_source(&conn, "task-A").unwrap();
    assert_eq!(deleted, 1);
    // task-B 不受影响
    let task_b = repo.list_by_source(&conn, "task-B").unwrap();
    assert_eq!(task_b.len(), 1, "task-B 的 taint 不应被删");
}

/// 测试 9:空 taints 列表可存储可读取。
#[test]
fn test_empty_taints_list() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let h = compute_value_hash(&to_value("empty-taints-value"));
    repo.upsert(
        &conn,
        &make_taint_record(h.clone(), "p".into(), vec![], Some("s".into())),
    )
    .unwrap();
    let found = repo.find_by_hash(&conn, &h).unwrap().unwrap();
    assert!(found.taints.is_empty(), "空 taints 列表应正确往返");
}

/// 测试 10:merge_taints 辅助函数去重 + 保序。
#[test]
fn test_merge_taints_helper() {
    let existing = vec!["a".to_string(), "b".to_string()];
    let incoming = vec!["b".to_string(), "c".to_string()];
    let merged = merge_taints(&existing, &incoming);
    assert_eq!(
        merged,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

/// 测试 11:now_iso8601 格式正确。
#[test]
fn test_now_iso8601_format() {
    let ts = now_iso8601();
    assert!(ts.contains('T'), "ISO8601 应含 T 分隔符");
    assert!(ts.contains('+') || ts.contains('Z'), "应含时区");
}
