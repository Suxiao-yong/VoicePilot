//! W9 Plan 3 Task 8 — Gateway 查表驱动 taint 策略集成测试(5 个)。
//!
//! 验证 spec §2.3 + §6.2:
//!   1. `web_page` taint → `ToolArgument` 被拦截(防止 LLM 投毒)
//!   2. `llm_output` taint → `LocalFile` 被拦截(防止 LLM 注入恶意路径)
//!   3. 无 taint 的 clean value 放行所有 sink
//!   4. multi-taint(`web_page` + `llm_output`)→ 多 sink 被拦截 + `RemoteLlm` 放行
//!   5. `check_taint_policy_and_audit` 在拦截时发射 `taint_blocked` 审计事件
//!      (details 仅含 taints + sink + resource_hash,不含原始 value)

use rusqlite::Connection;

use trust_kernel::error::KernelError;
use trust_kernel::gateway::{check_taint_policy, check_taint_policy_and_audit};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::taint_repo::{TaintRepo, compute_value_hash, make_taint_record};
use trust_kernel::policy::types::EgressDest;
use trust_kernel::repo::step_repo::StepRecord;

/// 将 &str 转为 `serde_json::Value`:
/// - 优先 parse 为 JSON(支持 `{"key": "value"}` 等)
/// - parse 失败时包装为 `Value::String`(普通字符串)
fn to_value(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.to_string()))
}

/// 建内存 DB + 跑 001_init.sql + 006_taints_unique_index.sql(直接复用 kernel 的迁移)。
fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    let sql = include_str!("../src/migrations/001_init.sql");
    conn.execute_batch(sql).unwrap();
    // W9 Plan 3 migration 006:taints.value_hash UNIQUE 约束
    let sql006 = include_str!("../src/migrations/006_taints_unique_index.sql");
    conn.execute_batch(sql006).unwrap();
    conn
}

/// 测试 1:`web_page` taint 不能成为 `ToolArgument`。
#[test]
fn test_web_page_taint_blocked_from_tool_argument() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = r#"{"html": "<script>...</script>"}"#;
    let hash = compute_value_hash(&to_value(value));
    repo.upsert(
        &conn,
        &make_taint_record(
            hash.clone(),
            "web_page".into(),
            vec!["web_page".into()],
            Some("t1:s1".into()),
        ),
    )
    .unwrap();

    let result = check_taint_policy(&conn, &hash, EgressDest::ToolArgument);
    assert!(result.is_err(), "web_page taint → ToolArgument 必须被拦截");

    let err = result.unwrap_err();
    if let KernelError::TaintPropagationBlocked { taints, sink } = err {
        assert!(
            taints.contains(&"web_page".to_string()),
            "taints 应含 web_page, got {:?}",
            taints
        );
        assert!(
            sink.contains("ToolArgument"),
            "sink 应含 ToolArgument, got {}",
            sink
        );
    } else {
        panic!("expected TaintPropagationBlocked, got {:?}", err);
    }
}

/// 测试 2:`llm_output` taint 不能写入文件系统(`LocalFile`)。
#[test]
fn test_llm_output_taint_blocked_from_filesystem() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = r#"{"path": "/etc/passwd", "content": "..."}"#;
    let hash = compute_value_hash(&to_value(value));
    repo.upsert(
        &conn,
        &make_taint_record(
            hash.clone(),
            "llm_output".into(),
            vec!["llm_output".into()],
            Some("t2:s2".into()),
        ),
    )
    .unwrap();

    let result = check_taint_policy(&conn, &hash, EgressDest::LocalFile);
    assert!(result.is_err(), "llm_output taint → LocalFile 必须被拦截");

    let err = result.unwrap_err();
    if let KernelError::TaintPropagationBlocked { taints, sink } = err {
        assert!(
            taints.contains(&"llm_output".to_string()),
            "taints 应含 llm_output, got {:?}",
            taints
        );
        assert!(
            sink.contains("LocalFile"),
            "sink 应含 LocalFile, got {}",
            sink
        );
    } else {
        panic!("expected TaintPropagationBlocked, got {:?}", err);
    }
}

/// 测试 3:无 taint 的 clean value 放行所有 sink。
#[test]
fn test_clean_value_allowed_all_sinks() {
    let conn = open_in_memory();
    let value = r#"{"normal": "data"}"#;
    let hash = compute_value_hash(&to_value(value));
    // 不 upsert 任何 taint — DB 中无该 value_hash 记录

    assert!(check_taint_policy(&conn, &hash, EgressDest::ToolArgument).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::LocalFile).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::RemoteLlm).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::RemoteMcp).is_ok());
}

/// 测试 4:multi-taint(`web_page` + `llm_output`)→ 多 sink 被拦截 + `RemoteLlm` 放行。
#[test]
fn test_multi_taint_blocked_from_tool_argument_and_filesystem() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = "multi-tainted-value";
    let hash = compute_value_hash(&to_value(value));
    // 第一次 upsert:标 web_page
    repo.upsert(
        &conn,
        &make_taint_record(
            hash.clone(),
            "web_page".into(),
            vec!["web_page".into()],
            None,
        ),
    )
    .unwrap();
    // 第二次 upsert:同 value,加 llm_output(触发合并去重)
    repo.upsert(
        &conn,
        &make_taint_record(
            hash.clone(),
            "llm_output".into(),
            vec!["llm_output".into()],
            None,
        ),
    )
    .unwrap();

    // 合并后 taints = ["web_page", "llm_output"]
    let found = repo.find_by_hash(&conn, &hash).unwrap().unwrap();
    assert_eq!(found.taints.len(), 2, "合并后应有 2 个 taint");

    // → ToolArgument 被 web_page 拦截
    let result = check_taint_policy(&conn, &hash, EgressDest::ToolArgument);
    assert!(
        result.is_err(),
        "含 web_page 的 multi-taint → ToolArgument 必须被拦截"
    );

    // → LocalFile 被 llm_output 拦截
    let result = check_taint_policy(&conn, &hash, EgressDest::LocalFile);
    assert!(
        result.is_err(),
        "含 llm_output 的 multi-taint → LocalFile 必须被拦截"
    );

    // → RemoteLlm 放行(无规则拦截)
    let result = check_taint_policy(&conn, &hash, EgressDest::RemoteLlm);
    assert!(result.is_ok(), "RemoteLlm 无 taint 拦截规则,应放行");
}

/// 测试 5(辅助):`user_input` taint 不触发任何拦截(只 web_page / llm_output 有规则)。
#[test]
fn test_user_input_taint_allowed_all_sinks() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = "user-typed-text";
    let hash = compute_value_hash(&to_value(value));
    repo.upsert(
        &conn,
        &make_taint_record(
            hash.clone(),
            "user_input".into(),
            vec!["user_input".into()],
            None,
        ),
    )
    .unwrap();

    assert!(check_taint_policy(&conn, &hash, EgressDest::ToolArgument).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::LocalFile).is_ok());
}

/// 测试 6:W9 Plan 3 Task 7 — `check_taint_policy_and_audit` 在拦截时发射
/// `taint_blocked` 审计事件,且 details 仅含 taints + sink + resource_hash
/// (不含原始 value,spec §6.2 隐私约束)。
#[test]
fn test_taint_blocked_audit_event_emitted() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    // FK 约束:audit_logs.task_id REFERENCES tasks(task_id),需先建 task + step
    kernel
        .create_task("t-audit-blocked", "taint blocked audit test")
        .unwrap();
    kernel
        .create_step(&StepRecord::new("s-audit-blocked", "t-audit-blocked", 1))
        .unwrap();

    // 插入 web_page taint
    let value = r#"{"html": "<script>alert(1)</script>"}"#;
    let hash = compute_value_hash(&to_value(value));
    {
        let conn = kernel.conn();
        TaintRepo::new()
            .upsert(
                &conn,
                &make_taint_record(
                    hash.clone(),
                    "web_page".into(),
                    vec!["web_page".into()],
                    Some("t-audit-blocked:s-audit-blocked".into()),
                ),
            )
            .unwrap();
    }

    let before = kernel.audit_count_for_task("t-audit-blocked").unwrap();
    // 调 check_taint_policy_and_audit 触发拦截 + 审计
    let result = check_taint_policy_and_audit(
        &kernel,
        "t-audit-blocked",
        Some("s-audit-blocked"),
        &hash,
        EgressDest::ToolArgument,
    );
    let after = kernel.audit_count_for_task("t-audit-blocked").unwrap();

    assert!(result.is_err(), "web_page → ToolArgument 必须被拦截");
    assert_eq!(
        after,
        before + 1,
        "taint_blocked 必须发射 1 条审计事件 (before={}, after={})",
        before,
        after
    );

    // 验证审计事件 details(spec §6.2:taints + sink + resource_hash,不含原始 value)
    let logs = kernel.list_audit_for_task("t-audit-blocked").unwrap();
    let blocked_event = logs
        .iter()
        .find(|e| e.event_type == "taint_blocked")
        .expect("audit_logs 必须含 taint_blocked 事件");

    let details = &blocked_event.details;
    let taints = details
        .get("taints")
        .and_then(|v| v.as_array())
        .expect("details.taints 必须是数组");
    assert!(
        taints.iter().any(|t| t == "web_page"),
        "taints 应含 web_page, got {:?}",
        taints
    );
    let sink = details
        .get("sink")
        .and_then(|v| v.as_str())
        .expect("details.sink 必须是字符串");
    assert!(
        sink.contains("ToolArgument"),
        "sink 应含 ToolArgument, got {}",
        sink
    );
    let resource_hash = details
        .get("resource_hash")
        .and_then(|v| v.as_str())
        .expect("details.resource_hash 必须是字符串");
    assert_eq!(resource_hash, hash, "resource_hash 应等于传入的 value_hash");

    // spec §6.2 隐私约束:details 不含原始 value
    let details_str = details.to_string();
    assert!(
        !details_str.contains("alert(1)"),
        "details 不应含原始 value 内容, got {}",
        details_str
    );
    assert!(
        !details_str.contains("<script>"),
        "details 不应含原始 value 内容, got {}",
        details_str
    );
}
