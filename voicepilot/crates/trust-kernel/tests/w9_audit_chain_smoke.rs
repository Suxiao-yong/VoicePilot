//! W9 Plan 7 — 哈希链 + 隐私脱敏 smoke 测试(3 个)。
//!
//! 覆盖 spec §5 "哈希链不断" + §6.4 审计事件隐私处理:
//!   1. audit_chain_hash_links_unbroken — 遍历 audit_logs 验证 prev_hash 链不断 +
//!      哈希计算正确(SHA256(prev_hash || canonical_json(payload)))
//!   2. audit_chain_w9_new_5_events_recorded — W9 新增 4 种事件全记录(stronghold 组合)
//!      (stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed /
//!       stronghold_degraded_mode_entered / taint_blocked;
//!       taint_propagated 由 dispatcher 传播,本测试不强求;
//!       dag_skeleton_modified + dag_modify_limit_exceeded 需 tauri feature,
//!       在 Task 7 全 feature 组合下验证)
//!   3. audit_chain_details_no_plaintext_secrets — details 字段隐私脱敏
//!      (不含 password= / passwd= / secret= / api_key= / sk- / plaintext= 密钥字面量前缀)
//!
//! 测试设计:
//! - 用例 1 + 3 用 default 组合(无 feature gate,可独立编译运行)
//! - 用例 2 用 `#[cfg(feature = "stronghold")]` 函数级门控
//! - 不写文件级 cfg gate,让 default 组合也能编译运行测试 1 + 测试 3
//!
//! 签名适配(W9 实际源码 vs Plan 预估):
//! - 哈希链算法:实际为 `SHA256(prev_hash || canonical_json({log_id, task_id, step_id,
//!   event_type, details, timestamp}))`(audit.rs `compute_hash`),按 task 分链
//!   (`last_hash_for_task`)。Plan 预估 `sha256(prev_hash + event_type + details)` 不正确,
//!   本测试按实际算法重算 + 验证链路。
//! - `audit_logs` 表无 `id` 列,主键为 `log_id`(TEXT UUID);`prev_hash` NULLABLE;
//!   `hash` NOT NULL。`read_audit_chain` 读取全部 8 列。
//! - `create_task(task_id, user_goal)` 实际 2 参数(Plan 预估 1 参数)。
//! - `create_post_commit_compensation(kernel, step_id, moved_paths: &[(PathBuf, PathBuf)],
//!   compensate_fn, level, conflict_policy, ttl_seconds) -> Result<String>`(返回 comp_id)。
//!   Plan 预估签名(JSON orig/curr + compensate_fn + JSON action)不正确。
//! - `StrongholdVault::create(password, &Connection)` / `degraded(&Connection)` 需要 &Connection。
//! - `set_stronghold_vault(Option<Arc<StrongholdVault>>)`(非直接传 vault)。
//! - `reverse_compensation` 不存在;用 `execute_compensate(kernel, input, approver)` 触发
//!   `stronghold_snapshot_decrypt_failed`(`decrypt_compensation_if_needed` 在 vault 锁定时审计)。
//! - `Resource { path, data_class, provenance }`(无 `value_hash` 字段);
//!   `Sink` 不存在,用 `EgressDest::ToolArgument`。
//! - `check_taint_policy(conn, value_hash, egress_dest)` 不审计;用
//!   `check_taint_policy_and_audit(kernel, task_id, step_id, value_hash, egress_dest)` 触发 `taint_blocked`。

use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};

/// audit_logs 一行(按 timestamp ASC, log_id ASC 排序)。
struct AuditRow {
    log_id: String,
    task_id: String,
    step_id: Option<String>,
    event_type: String,
    details: String,
    timestamp: String,
    prev_hash: Option<String>,
    hash: String,
}

/// 读取 audit_logs 全部记录(按 timestamp ASC, log_id ASC 排序)。
fn read_audit_chain(kernel: &TrustKernel) -> Vec<AuditRow> {
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare(
            "SELECT log_id, task_id, step_id, event_type, details, timestamp, prev_hash, hash \
             FROM audit_logs ORDER BY timestamp ASC, log_id ASC",
        )
        .expect("prepare audit_logs query");
    stmt.query_map([], |row| {
        Ok(AuditRow {
            log_id: row.get(0)?,
            task_id: row.get(1)?,
            step_id: row.get(2)?,
            event_type: row.get(3)?,
            details: row.get(4)?,
            timestamp: row.get(5)?,
            prev_hash: row.get(6)?,
            hash: row.get(7)?,
        })
    })
    .expect("query_map audit_logs")
    .filter_map(|r| r.ok())
    .collect()
}

/// 计算某字符串的 SHA256 hex(复用 audit_append 用的算法)。
fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[allow(dead_code)] // 仅 stronghold-gated 测试 2 引用,default 组合下未使用
fn count_audit_events(kernel: &TrustKernel, event_type: &str) -> i64 {
    let conn = kernel.conn();
    conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE event_type = ?1",
        rusqlite::params![event_type],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

#[allow(dead_code)] // 仅 stronghold-gated 测试 2 引用,default 组合下未使用
fn list_distinct_event_types(kernel: &TrustKernel) -> Vec<String> {
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT DISTINCT event_type FROM audit_logs")
        .expect("prepare distinct event_type");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query_map distinct event_type")
        .filter_map(|r| r.ok())
        .collect()
}

// ===== 测试 1:哈希链不断 + 哈希计算正确 =====

/// 测试 1:audit_logs 哈希链不断。
///
/// 在同一 task 上触发多个审计事件(create_task → create_step → update_step_status × 2),
/// 验证:
///   - 第 0 条 prev_hash 为 None(创世)
///   - 第 i 条 prev_hash == 第 i-1 条的 hash(链不断)
///   - 每条 hash == SHA256(prev_hash || canonical_json({log_id, task_id, step_id,
///     event_type, details, timestamp}))(哈希计算正确,与 audit.rs `compute_hash` 一致)
#[test]
fn audit_chain_hash_links_unbroken() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");

    // 在同一 task 上触发 4 个审计事件,构建真实哈希链。
    // (不同 task 各只有 1 个 TASK_CREATED 事件,prev_hash 全为 None,无法验证链路)
    kernel.create_task("chain-task", "hash chain test goal").expect("create_task");
    kernel
        .create_step(&StepRecord::new("chain-step-1", "chain-task", 1))
        .expect("create_step");
    kernel
        .update_step_status("chain-step-1", StepStatus::Running)
        .expect("update_step_status Running");
    kernel
        .update_step_status("chain-step-1", StepStatus::Succeeded)
        .expect("update_step_status Succeeded");

    let chain = read_audit_chain(&kernel);
    assert!(
        chain.len() >= 4,
        "audit_logs must have at least 4 records after 4 events, got {}",
        chain.len()
    );

    // 第 0 条 prev_hash 必须是 None(创世,无前驱)
    assert!(
        chain[0].prev_hash.is_none(),
        "first audit_log prev_hash must be None (genesis), got: {:?}",
        chain[0].prev_hash
    );

    // 第 i 条 prev_hash 必须等于第 i-1 条的 hash(链不断)
    for i in 1..chain.len() {
        let prev = &chain[i - 1];
        let curr = &chain[i];
        assert_eq!(
            curr.prev_hash.as_deref(),
            Some(prev.hash.as_str()),
            "audit chain broken at record {}: curr.prev_hash='{:?}', prev.hash='{}'",
            i,
            curr.prev_hash,
            prev.hash
        );
    }

    // 哈希计算正确性:hash == SHA256(prev_hash || canonical_json(payload))
    // 算法来自 audit.rs `compute_hash`:
    //   SHA256(prev_hash_bytes? || serde_json::to_string(json!({
    //     log_id, task_id, step_id, event_type, details, timestamp
    //   })))
    // serde_json 默认 BTreeMap 序列化(字母序),audit.rs 与本测试用相同 json! 宏 + to_string,
    // 序列化结果一致。
    for row in &chain {
        let prev_hash_str: &str = row.prev_hash.as_deref().unwrap_or("");
        let details_value: serde_json::Value =
            serde_json::from_str(&row.details).unwrap_or(serde_json::Value::Null);
        let payload = serde_json::json!({
            "log_id": row.log_id,
            "task_id": row.task_id,
            "step_id": row.step_id,
            "event_type": row.event_type,
            "details": details_value,
            "timestamp": row.timestamp,
        });
        let canonical = serde_json::to_string(&payload).unwrap_or_default();
        // SHA256(prev_hash || canonical) — streaming hash 等价于拼接后哈希
        let concat = format!("{}{}", prev_hash_str, canonical);
        let expected_hash = sha256_hex(&concat);
        assert_eq!(
            row.hash, expected_hash,
            "hash mismatch for log_id={} event_type={}: expected={}, got={}",
            row.log_id, row.event_type, expected_hash, row.hash
        );
    }
}

// ===== 测试 2:W9 新增 4 种事件全记录(stronghold 组合)=====

/// 测试 2:W9 新增 4 种事件全记录(stronghold 组合下可触发的事件)。
///
/// 触发 stronghold_snapshot_encrypted + stronghold_snapshot_decrypt_failed +
/// stronghold_degraded_mode_entered + taint_blocked 共 4 种事件,验证 audit_logs 全部记录。
/// (taint_propagated 由 dispatcher 传播,本测试不强求;
///  dag_skeleton_modified + dag_modify_limit_exceeded 需 tauri feature,在 Task 7 验证)
#[test]
#[cfg(feature = "stronghold")]
fn audit_chain_w9_new_5_events_recorded() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::compensation::types::{CompensationLevel, ConflictPolicy};
    use trust_kernel::crypto::stronghold::StrongholdVault;
    use trust_kernel::gateway::check_taint_policy_and_audit;
    use trust_kernel::policy::taint_repo::{TaintRepo, TaintRecord};
    use trust_kernel::policy::types::EgressDest;
    use trust_kernel::repo::config_repo::ConfigRepo;
    use trust_kernel::skills::common::create_post_commit_compensation;
    use trust_kernel::skills::task_compensate::{execute_compensate, TaskCompensateInput};

    let kernel = Arc::new(TrustKernel::open_in_memory().expect("open_in_memory"));

    // 设置独立 vault_path 到 tempdir(避免与 default data_dir 冲突)
    // 模式参考 w9_snapshot_encrypted_smoke.rs `setup_kernel_with_unlocked_vault`。
    let tmp = tempfile::TempDir::new().expect("create tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    let vault_path_str = vault_path.to_str().expect("vault path is utf-8").to_string();
    // leak tempdir 让它存活到测试结束(stronghold.save() 需要写文件)
    std::mem::forget(tmp);
    {
        let conn = kernel.conn();
        ConfigRepo::new()
            .set(&conn, "stronghold.vault_path", &vault_path_str)
            .expect("set stronghold.vault_path");
        ConfigRepo::new()
            .set(&conn, "stronghold.enabled", "true")
            .expect("set stronghold.enabled");
    }

    // 创建 task + step(供 create_post_commit_compensation 用)
    kernel.create_task("task-enc", "encrypt test goal").expect("create_task task-enc");
    kernel
        .create_step(&StepRecord::new("step-enc", "task-enc", 1))
        .expect("create_step step-enc");

    // ===== 触发 stronghold_snapshot_encrypted =====
    let vault = {
        let conn = kernel.conn();
        StrongholdVault::create("test_password", &conn).expect("StrongholdVault::create")
    };
    kernel.set_stronghold_vault(Some(Arc::new(vault)));
    let moved: Vec<(PathBuf, PathBuf)> = vec![
        (PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf")),
    ];
    let _comp_id = create_post_commit_compensation(
        &kernel,
        "step-enc",
        &moved,
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .expect("create_post_commit_compensation");
    assert!(
        count_audit_events(&kernel, "stronghold_snapshot_encrypted") >= 1,
        "stronghold_snapshot_encrypted must be logged"
    );

    // ===== 触发 stronghold_degraded_mode_entered =====
    // kernel.stronghold_enter_degraded_mode 创建占位 task + 审计事件
    // (set_stronghold_vault(degraded) 不触发审计,必须显式调 enter_degraded_mode)
    kernel
        .stronghold_enter_degraded_mode("wrong_password")
        .expect("stronghold_enter_degraded_mode");
    assert!(
        count_audit_events(&kernel, "stronghold_degraded_mode_entered") >= 1,
        "stronghold_degraded_mode_entered must be logged"
    );

    // ===== 触发 stronghold_snapshot_decrypt_failed =====
    // 锁定 vault 后调 execute_compensate,内部 decrypt_compensation_if_needed 检测到
    // vault 未解锁 → 审计 stronghold_snapshot_decrypt_failed(error="NotUnlocked")+ 返回 Err。
    // execute_compensate 在 decrypt 失败后即返回,不触达 filesystem auto_reverse_move。
    if let Some(vault) = kernel.stronghold_vault() {
        vault.lock();
    }
    let input = TaskCompensateInput {
        task_id: "task-dec".to_string(),
        step_id: "step-dec".to_string(),
        target_step_id: "step-enc".to_string(),
    };
    let _ = execute_compensate(&kernel, &input, &AutoApprover);
    assert!(
        count_audit_events(&kernel, "stronghold_snapshot_decrypt_failed") >= 1,
        "stronghold_snapshot_decrypt_failed must be logged when vault locked"
    );

    // ===== 触发 taint_blocked =====
    // upsert 一条 web_page taint,调 check_taint_policy_and_audit 检查 ToolArgument sink
    let repo = TaintRepo::new();
    let value_hash = "sha256:web-content-test".to_string();
    {
        let conn = kernel.conn();
        repo.upsert(
            &conn,
            &TaintRecord {
                taint_id: uuid::Uuid::new_v4().to_string(),
                value_hash: value_hash.clone(),
                provenance: "web_page".to_string(),
                taints: vec!["web_page".to_string()],
                collected_at: "2026-07-28T00:00:00Z".to_string(),
                source_ref: Some("task-enc:step-enc".to_string()),
            },
        )
        .expect("upsert taint");
    }
    // check_taint_policy_and_audit 需要已存在的 task_id + step_id 满足 audit_logs FK
    kernel.create_task("task-taint", "taint test goal").expect("create_task task-taint");
    kernel
        .create_step(&StepRecord::new("step-taint", "task-taint", 1))
        .expect("create_step step-taint");
    let result = check_taint_policy_and_audit(
        &kernel,
        "task-taint",
        Some("step-taint"),
        &value_hash,
        EgressDest::ToolArgument,
    );
    assert!(
        result.is_err(),
        "taint policy should block web_page → ToolArgument"
    );
    assert!(
        count_audit_events(&kernel, "taint_blocked") >= 1,
        "taint_blocked must be logged when web_page taint hits ToolArgument sink"
    );

    // ===== 验证 4 种事件命名都在 audit_logs 表中出现过 =====
    let logged_events = list_distinct_event_types(&kernel);
    let required = [
        "stronghold_snapshot_encrypted",
        "stronghold_snapshot_decrypt_failed",
        "stronghold_degraded_mode_entered",
        "taint_blocked",
    ];
    for et in &required {
        assert!(
            logged_events.contains(&et.to_string()),
            "event_type '{}' must be logged after triggering",
            et
        );
    }
}

// ===== 测试 3:details 字段隐私脱敏 =====

/// 测试 3:audit_logs.details 字段隐私脱敏。
///
/// 扫描所有 audit_logs.details,验证不含敏感密钥字面量前缀:
///   - "password=" / "passwd=" / "secret=" / "api_key=" / "sk-" / "plaintext="
///
/// spec §6.4:details 字段仅记录 hash / id / count / reason,不记录原始敏感值。
/// 用精确匹配密钥字面量前缀(非子串匹配 "secret"/"key"),避免误报合法字段
/// vault_key_ref / secret_id / api_key_hash(hash 字段不是密钥本身)。
///
/// stronghold 组合下额外验证 stronghold_snapshot_encrypted details 含 plaintext_len
/// 但不含 reverse_payload 明文(spec §6.4 + §2.2 审计事件表)。
#[test]
fn audit_chain_details_no_plaintext_secrets() {
    use std::path::PathBuf;

    use trust_kernel::compensation::types::{CompensationLevel, ConflictPolicy};
    use trust_kernel::skills::common::create_post_commit_compensation;

    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");

    // 触发若干审计事件(create_task / create_step / compensation)
    // default 组合下 create_post_commit_compensation 走明文 PoC 分支(无 stronghold 加密),
    // 但审计 details 仅含 {comp_id, level, conflict_policy, ttl_expires},不含 reverse_payload。
    kernel.create_task("privacy-task", "privacy test goal").expect("create_task");
    kernel
        .create_step(&StepRecord::new("privacy-step", "privacy-task", 1))
        .expect("create_step");
    let moved: Vec<(PathBuf, PathBuf)> = vec![
        (PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf")),
    ];
    let _ = create_post_commit_compensation(
        &kernel,
        "privacy-step",
        &moved,
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .expect("create_post_commit_compensation");

    // 读取所有 audit_logs
    let chain = read_audit_chain(&kernel);
    assert!(!chain.is_empty(), "audit_logs must have records");

    // 隐私黑名单关键字(精确匹配密钥字面量前缀,大小写不敏感)
    // 不用子串匹配 "secret"/"key" —— 会误报合法字段 vault_key_ref / secret_id / api_key_hash。
    // hash 字段(vault_key_ref / secret_id / api_key_hash)不是密钥本身,允许。
    let sensitive_keywords = [
        "password=",
        "passwd=",
        "secret=",
        "api_key=",
        "sk-", // OpenAI API key 前缀
        "plaintext=",
    ];

    for row in &chain {
        let details_lower = row.details.to_lowercase();
        for kw in &sensitive_keywords {
            assert!(
                !details_lower.contains(kw),
                "audit_log log_id={} event_type='{}' details='{}' contains sensitive keyword '{}'",
                row.log_id,
                row.event_type,
                row.details,
                kw
            );
        }
    }

    // stronghold 组合下额外验证:stronghold_snapshot_encrypted 事件的 details 必须含
    // plaintext_len(数字)但不含 reverse_payload 明文(spec §6.4 + §2.2 审计事件表)。
    // default 组合下 stronghold feature 未启用,事件不触发,此断言跳过。
    #[cfg(feature = "stronghold")]
    {
        use std::sync::Arc;

        use trust_kernel::crypto::stronghold::StrongholdVault;
        use trust_kernel::repo::config_repo::ConfigRepo;

        // 设置独立 vault_path(避免与 default data_dir 冲突)
        let tmp = tempfile::TempDir::new().expect("create tempdir");
        let vault_path = tmp.path().join("stronghold.bin");
        let vault_path_str = vault_path.to_str().expect("vault path is utf-8").to_string();
        std::mem::forget(tmp);
        {
            let conn = kernel.conn();
            ConfigRepo::new()
                .set(&conn, "stronghold.vault_path", &vault_path_str)
                .expect("set stronghold.vault_path");
            ConfigRepo::new()
                .set(&conn, "stronghold.enabled", "true")
                .expect("set stronghold.enabled");
        }

        // 创建另一个 task + step 用于加密补偿
        kernel
            .create_task("privacy-enc-task", "enc test goal")
            .expect("create_task privacy-enc-task");
        kernel
            .create_step(&StepRecord::new("privacy-enc-step", "privacy-enc-task", 1))
            .expect("create_step privacy-enc-step");

        let vault = {
            let conn = kernel.conn();
            StrongholdVault::create("test_password", &conn).expect("StrongholdVault::create")
        };
        kernel.set_stronghold_vault(Some(Arc::new(vault)));

        let moved_enc: Vec<(PathBuf, PathBuf)> = vec![
            (PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf")),
        ];
        let _ = create_post_commit_compensation(
            &kernel,
            "privacy-enc-step",
            &moved_enc,
            "filesystem.reverse_move",
            CompensationLevel::Strong,
            ConflictPolicy::AutoReverse,
            3600,
        )
        .expect("create_post_commit_compensation encrypted");

        // 重新读取 chain(包含新审计事件)
        let chain2 = read_audit_chain(&kernel);
        let encrypted_events: Vec<&AuditRow> = chain2
            .iter()
            .filter(|r| r.event_type == "stronghold_snapshot_encrypted")
            .collect();
        assert!(
            !encrypted_events.is_empty(),
            "must have at least one stronghold_snapshot_encrypted event in stronghold mode"
        );
        for row in encrypted_events {
            assert!(
                row.details.contains("plaintext_len"),
                "stronghold_snapshot_encrypted details must contain plaintext_len, got: {}",
                row.details
            );
            // reverse_payload 明文 JSON 含 "moves" 键,审计 details 不应含此键
            assert!(
                !row.details.contains("\"moves\""),
                "stronghold_snapshot_encrypted details must NOT contain raw reverse_payload (moves key), got: {}",
                row.details
            );
        }
    }
}
