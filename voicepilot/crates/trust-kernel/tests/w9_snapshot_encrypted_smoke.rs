//! W9 Plan 2 集成测试:Stronghold 加密 snapshot_encrypted。
//! spec: docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md §2.2
//!
//! 5 个测试覆盖:
//!   1. stronghold_encrypts_reverse_payload_when_unlocked — 加密成功路径
//!   2. stronghold_degraded_mode_skips_encryption — 降级模式(vault 未解锁)
//!   3. stronghold_feature_disabled_keeps_plaintext_poc — feature 运行时禁用 → 明文 PoC
//!   4. reverse_compensation_decrypts_and_reverses_move — 解密 + 反向移动成功
//!   5. reverse_compensation_fails_when_vault_locked — vault 锁定时解密失败 + 审计
//!
//! Task 5 审计断言:测试 1 验证 stronghold_snapshot_encrypted 隐私(details 不含 plaintext),
//!                测试 5 验证 stronghold_snapshot_decrypt_failed details 不含 password。
//!
//! 运行:cargo test --features stronghold,llm --test w9_snapshot_encrypted_smoke

#![cfg(feature = "stronghold")]

use std::path::PathBuf;
use std::sync::Arc;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::compensation::types::{CompensationLevel, ConflictPolicy};
use trust_kernel::crypto::stronghold::{EncryptedPayload, StrongholdVault};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::config_repo::ConfigRepo;
use trust_kernel::repo::step_repo::StepRecord;
use trust_kernel::skills::common::create_post_commit_compensation;
use trust_kernel::skills::task_compensate::{TaskCompensateInput, execute_compensate};

/// 测试辅助:创建 kernel + task + step + 解锁的 StrongholdVault。
/// vault_path 设到独立 tempdir,避免并发测试冲突。
fn setup_kernel_with_unlocked_vault(password: &str) -> Arc<TrustKernel> {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    kernel.create_task("t1", "test goal").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    // 设置独立 vault_path 到 tempdir(避免与 default data_dir 冲突)
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_path = tmp.path().join("stronghold.bin");
    // tempdir 必须存活到 vault create 之后,这里 leak 让它存活到测试结束
    let vault_path_str = vault_path.to_str().unwrap().to_string();
    std::mem::forget(tmp);
    {
        let conn = kernel.conn();
        ConfigRepo::new()
            .set(&conn, "stronghold.vault_path", &vault_path_str)
            .unwrap();
    }

    let vault = {
        let conn = kernel.conn();
        StrongholdVault::create(password, &conn).expect("create vault")
    };
    kernel.set_stronghold_vault(Some(Arc::new(vault)));
    kernel
}

/// 测试辅助:创建 kernel + task + step + 降级 vault(未解锁)。
fn setup_kernel_with_degraded_vault() -> Arc<TrustKernel> {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    kernel.create_task("t1", "test goal").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
    let vault = {
        let conn = kernel.conn();
        StrongholdVault::degraded(&conn)
    };
    kernel.set_stronghold_vault(Some(Arc::new(vault)));
    kernel
}

fn moved_paths() -> Vec<(PathBuf, PathBuf)> {
    vec![
        (PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf")),
        (PathBuf::from("src/b.pdf"), PathBuf::from("out/b.pdf")),
    ]
}

// ===== 测试 1:加密成功路径 =====

#[test]
fn stronghold_encrypts_reverse_payload_when_unlocked() {
    let kernel = setup_kernel_with_unlocked_vault("test_password");

    let comp_id = create_post_commit_compensation(
        &kernel,
        "s1",
        &moved_paths(),
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .unwrap();

    let comp = kernel
        .get_compensation(&comp_id)
        .unwrap()
        .expect("compensation must exist");

    // 加密成功:snapshot_encrypted 非空
    assert!(
        comp.snapshot_encrypted.is_some(),
        "snapshot_encrypted must be Some when vault unlocked"
    );
    // reverse_payload 列为空字符串(明文不落盘)
    assert_eq!(
        comp.reverse_payload, "",
        "reverse_payload must be empty string when encrypted"
    );
    // snapshot_vault_ref 是 UUID v4(非 "degraded")
    let vault_ref = comp
        .snapshot_vault_ref
        .as_ref()
        .expect("vault_ref must be Some");
    assert_ne!(
        vault_ref, "degraded",
        "vault_ref must be UUID, not 'degraded'"
    );
    assert!(
        uuid::Uuid::parse_str(vault_ref).is_ok(),
        "vault_ref must be valid UUID"
    );

    // 验证密文可解密回原明文 JSON
    let encrypted_bytes = comp.snapshot_encrypted.as_ref().unwrap();
    let payload: EncryptedPayload = bincode::deserialize(encrypted_bytes).unwrap();
    let vault = kernel.stronghold_vault().expect("vault must be set");
    let plaintext = vault.decrypt(&payload).unwrap();
    let plaintext_str = String::from_utf8(plaintext).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&plaintext_str).unwrap();
    let moves = parsed.get("moves").and_then(|v| v.as_array()).unwrap();
    assert_eq!(moves.len(), 2, "decrypted payload must contain 2 moves");

    // W9 Plan 2 Task 5:验证审计事件 stronghold_snapshot_encrypted
    let conn = kernel.conn();
    let audit_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE event_type = 'stronghold_snapshot_encrypted'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        audit_count > 0,
        "must audit stronghold_snapshot_encrypted event"
    );

    // 验证审计 details 不含 plaintext(spec §6.4 隐私处理)
    let audit_details: String = conn
        .query_row(
            "SELECT details FROM audit_logs WHERE event_type = 'stronghold_snapshot_encrypted' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !audit_details.contains("moves"),
        "audit details must not contain plaintext 'moves' key"
    );
    assert!(
        audit_details.contains("vault_ref"),
        "audit details must contain vault_ref"
    );
    assert!(
        audit_details.contains("plaintext_len"),
        "audit details must contain plaintext_len"
    );
}

// ===== 测试 2:降级模式(vault 未解锁) =====

#[test]
fn stronghold_degraded_mode_skips_encryption() {
    let kernel = setup_kernel_with_degraded_vault();

    let comp_id = create_post_commit_compensation(
        &kernel,
        "s1",
        &moved_paths(),
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .unwrap();

    let comp = kernel.get_compensation(&comp_id).unwrap().unwrap();

    // 降级模式:snapshot_encrypted = None
    assert!(
        comp.snapshot_encrypted.is_none(),
        "snapshot_encrypted must be None in degraded mode"
    );
    // snapshot_vault_ref = "degraded"
    assert_eq!(
        comp.snapshot_vault_ref.as_deref(),
        Some("degraded"),
        "snapshot_vault_ref must be 'degraded' in degraded mode"
    );
    // reverse_payload 保留明文(降级可读)
    assert!(
        !comp.reverse_payload.is_empty(),
        "reverse_payload must contain plaintext in degraded mode"
    );
    let parsed: serde_json::Value = serde_json::from_str(&comp.reverse_payload).unwrap();
    assert!(
        parsed.get("moves").is_some(),
        "plaintext must be valid JSON"
    );
}

// ===== 测试 3:feature 运行时禁用明文 PoC =====
// 注意:本测试在 #[cfg(feature = "stronghold")] 下编译,通过 config 表
// stronghold.enabled = "false" 在运行时禁用,模拟 feature 未启用的行为。

#[test]
fn stronghold_feature_disabled_keeps_plaintext_poc() {
    let kernel = setup_kernel_with_unlocked_vault("test_password");

    // 运行时禁用 stronghold(config 表 stronghold.enabled = "false")
    // 用 block scope 限制 conn guard 生命周期,避免 deadlock
    {
        let conn = kernel.conn();
        ConfigRepo::new()
            .set(&conn, "stronghold.enabled", "false")
            .unwrap();
    }
    assert!(
        !kernel.stronghold_enabled(),
        "stronghold_enabled must be false after config set"
    );

    let comp_id = create_post_commit_compensation(
        &kernel,
        "s1",
        &moved_paths(),
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .unwrap();

    let comp = kernel.get_compensation(&comp_id).unwrap().unwrap();

    // feature 运行时禁用:保持 W3a PoC 行为
    assert!(
        comp.snapshot_encrypted.is_none(),
        "snapshot_encrypted must be None when stronghold disabled"
    );
    assert!(
        comp.snapshot_vault_ref.is_none(),
        "snapshot_vault_ref must be None when stronghold disabled"
    );
    assert!(
        !comp.reverse_payload.is_empty(),
        "reverse_payload must contain plaintext when stronghold disabled"
    );
}

// ===== 测试 4:解密 + 反向移动成功 =====

#[test]
fn reverse_compensation_decrypts_and_reverses_move() {
    let kernel = setup_kernel_with_unlocked_vault("test_password");

    // 准备真实文件系统状态:curr 存在,orig 不存在
    let tmp = tempfile::tempdir().unwrap();
    let orig = tmp.path().join("orig").join("file.txt");
    let curr = tmp.path().join("curr").join("file.txt");
    std::fs::create_dir_all(curr.parent().unwrap()).unwrap();
    std::fs::write(&curr, b"hello").unwrap();

    // 创建加密的 compensation record(from=orig, to=curr)
    let moved = vec![(orig.clone(), curr.clone())];
    let comp_id = create_post_commit_compensation(
        &kernel,
        "s1",
        &moved,
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .unwrap();

    // 验证加密:snapshot_encrypted 非空
    let comp = kernel.get_compensation(&comp_id).unwrap().unwrap();
    assert!(
        comp.snapshot_encrypted.is_some(),
        "snapshot_encrypted must be Some"
    );

    // execute_compensate 内部会创建新 task t2 + step s2,无需预创建
    let input = TaskCompensateInput {
        task_id: "t2".to_string(),
        step_id: "s2".to_string(),
        target_step_id: "s1".to_string(),
    };
    let result = execute_compensate(&kernel, &input, &AutoApprover);

    assert!(
        result.is_ok(),
        "execute_compensate must succeed, got {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), "t2");

    // 验证文件从 curr 移回 orig
    assert!(orig.exists(), "orig must exist after reverse");
    assert!(!curr.exists(), "curr must not exist after reverse");
    let content = std::fs::read_to_string(&orig).unwrap();
    assert_eq!(content, "hello");
}

// ===== 测试 5:vault 锁定时解密失败 + 审计 =====

#[test]
fn reverse_compensation_fails_when_vault_locked() {
    let kernel = setup_kernel_with_unlocked_vault("test_password");

    // 创建加密的 compensation record
    let _comp_id = create_post_commit_compensation(
        &kernel,
        "s1",
        &moved_paths(),
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .unwrap();

    // 锁定 vault(模拟降级或用户锁定)
    if let Some(vault) = kernel.stronghold_vault() {
        vault.lock();
    }

    // execute_compensate 内部会创建新 task t2 + step s2,无需预创建
    let input = TaskCompensateInput {
        task_id: "t2".to_string(),
        step_id: "s2".to_string(),
        target_step_id: "s1".to_string(),
    };
    let result = execute_compensate(&kernel, &input, &AutoApprover);

    assert!(
        result.is_err(),
        "execute_compensate must fail when vault locked, got Ok"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("NotUnlocked") || err_msg.contains("decrypt"),
        "error must mention NotUnlocked or decrypt, got: {err_msg}"
    );

    // 验证审计事件 stronghold_snapshot_decrypt_failed
    let conn = kernel.conn();
    let audit_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE event_type = 'stronghold_snapshot_decrypt_failed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        audit_count > 0,
        "must audit stronghold_snapshot_decrypt_failed event"
    );

    // W9 Plan 2 Task 5:验证审计 details 不含密钥 / 密文 / 密码(spec §6.4)
    let audit_details: String = conn
        .query_row(
            "SELECT details FROM audit_logs WHERE event_type = 'stronghold_snapshot_decrypt_failed' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        audit_details.contains("error"),
        "audit details must contain error field"
    );
    assert!(
        !audit_details.contains("password"),
        "audit details must not contain 'password'"
    );
    // 验证 error 字段是错误变体名,不是密文
    let parsed: serde_json::Value = serde_json::from_str(&audit_details).unwrap();
    let error_str = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .expect("error field must be a string");
    assert!(
        error_str.contains("NotUnlocked") || error_str.contains("DecryptionFailed"),
        "error must be variant name, got: {error_str}"
    );
}
