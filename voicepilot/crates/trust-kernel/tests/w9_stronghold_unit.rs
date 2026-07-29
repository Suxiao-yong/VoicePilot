//! W9 Plan 1: Stronghold 加密单元测试。
//!
//! 8 个测试覆盖 spec §2.1 全部 API + §6.1 安全约束 + 降级模式语义。
//!
//! 运行:cargo test --features stronghold -p trust-kernel --test w9_stronghold_unit
//!
//! 注意:每个测试用独立的 tempdir + 独立的 in-memory DB,避免并发冲突。
//! Argon2id 参数 m=64MB t=3 p=4(固定,spec §6.1),每次 create/unlock 约 0.5s。

#![cfg(feature = "stronghold")]

use std::sync::Arc;
use trust_kernel::crypto::stronghold::{EncryptedPayload, StrongholdError, StrongholdVault};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::config_repo::ConfigRepo;

/// 辅助:在 KV 中设置 stronghold.vault_path 到独立 tempdir,返回 tempdir + path。
/// tempdir 由调用方持有(保持目录存活),path 用于断言。
fn setup_vault_path(kernel: &TrustKernel) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    let conn = kernel.conn();
    ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");
    (tmp, vault_path)
}

/// 测试 1:create(password) 后,salt 持久化到 app_config.stronghold.salt,
/// vault 文件路径解析为 ${vault_path},vault 文件已创建(create 内部调 save())。
#[test]
fn vault_create_persists_salt_and_path() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let (_tmp, vault_path) = setup_vault_path(&kernel);
    let conn = kernel.conn();

    let vault = StrongholdVault::create("test_password_123", &conn).expect("create vault");
    assert!(vault.is_unlocked(), "vault must be unlocked after create");
    assert!(vault_path.exists(), "vault file must be created after save()");

    // salt 持久化在 app_config
    let salt_kv = ConfigRepo::new()
        .get(&conn, "stronghold.salt")
        .expect("get salt");
    assert!(salt_kv.is_some(), "salt must be persisted in KV");
    assert!(!salt_kv.unwrap().is_empty(), "salt must be non-empty");
}

/// 测试 2:create(password) -> lock() -> unlock(password) 成功。
#[test]
fn vault_unlock_with_correct_password_succeeds() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let (_tmp, _vault_path) = setup_vault_path(&kernel);
    let conn = kernel.conn();

    let vault = StrongholdVault::create("correct_password", &conn).expect("create");
    vault.lock();
    assert!(!vault.is_unlocked(), "vault must be locked after lock()");

    vault.unlock("correct_password", &conn).expect("unlock");
    assert!(vault.is_unlocked(), "vault must be unlocked after correct password");
}

/// 测试 3:create(password) -> lock() -> unlock(wrong_password) 返回 WrongPassword。
#[test]
fn vault_unlock_with_wrong_password_returns_error() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let (_tmp, _vault_path) = setup_vault_path(&kernel);
    let conn = kernel.conn();

    let vault = StrongholdVault::create("correct_password", &conn).expect("create");
    vault.lock();

    let err = vault.unlock("wrong_password", &conn).unwrap_err();
    assert!(
        matches!(err, StrongholdError::WrongPassword),
        "wrong password must return WrongPassword, got: {:?}",
        err
    );
    assert!(!vault.is_unlocked(), "vault must remain locked");
}

/// 测试 4:create -> encrypt(plaintext) -> decrypt(payload) == plaintext。
#[test]
fn vault_encrypt_decrypt_roundtrip() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let (_tmp, _vault_path) = setup_vault_path(&kernel);
    let conn = kernel.conn();

    let vault = StrongholdVault::create("roundtrip_password", &conn).expect("create");

    let plaintext = b"{\"moves\": [{\"from\": \"a.txt\", \"to\": \"b.txt\"}]}";
    let payload: EncryptedPayload = vault.encrypt(plaintext).expect("encrypt");
    assert!(!payload.ciphertext.is_empty(), "ciphertext must be non-empty");
    assert_eq!(
        payload.nonce.len(),
        24,
        "XChaCha20Poly1305 nonce = 24 bytes"
    );
    assert_eq!(payload.salt_ref, "stronghold.salt");

    let decrypted = vault.decrypt(&payload).expect("decrypt");
    assert_eq!(
        decrypted.as_slice(),
        plaintext,
        "decrypt must return original plaintext"
    );
}

/// 测试 5:degraded() 构造的 vault is_unlocked() = false,encrypt 返回 NotUnlocked。
#[test]
fn vault_degraded_mode_is_unlocked_false() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    let vault = StrongholdVault::degraded(&conn);
    assert!(!vault.is_unlocked(), "degraded vault must not be unlocked");

    let err = vault.encrypt(b"test").unwrap_err();
    assert!(
        matches!(err, StrongholdError::NotUnlocked),
        "degraded encrypt must return NotUnlocked, got: {:?}",
        err
    );
}

/// 测试 6:create -> lock() -> encrypt 返回 NotUnlocked(key material 已清零)。
#[test]
fn vault_lock_clears_key_material() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let (_tmp, _vault_path) = setup_vault_path(&kernel);
    let conn = kernel.conn();

    let vault = StrongholdVault::create("lock_test_password", &conn).expect("create");
    assert!(vault.is_unlocked());

    vault.lock();
    assert!(!vault.is_unlocked(), "vault must be locked after lock()");

    let err = vault.encrypt(b"after lock").unwrap_err();
    assert!(
        matches!(err, StrongholdError::NotUnlocked),
        "encrypt after lock must return NotUnlocked, got: {:?}",
        err
    );
}

/// 测试 7:privacy_mode=true 时,ensure_stronghold_ready_for_privacy 强制要求 vault 已解锁。
/// - privacy_mode=false:Ok
/// - privacy_mode=true + 未注入 vault:Err(StrongholdRequired)
/// - privacy_mode=true + 注入未解锁 vault(degraded):Err(StrongholdRequired)
/// - privacy_mode=true + 注入已解锁 vault:Ok
#[test]
fn privacy_mode_forces_stronghold_unlocked() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");

    // 场景 1:privacy_mode=false -> Ok(默认未设 privacy.mode)
    assert!(kernel.ensure_stronghold_ready_for_privacy().is_ok());

    // 开启 privacy_mode(用 block scope 限制 conn guard 生命周期,避免 deadlock)
    {
        let conn = kernel.conn();
        ConfigRepo::new()
            .set(&conn, "privacy.mode", "true")
            .expect("set privacy.mode");
    }
    assert!(kernel.privacy_mode(), "privacy_mode must be true");

    // 场景 2:privacy_mode=true + 未注入 vault -> Err
    let err = kernel.ensure_stronghold_ready_for_privacy().unwrap_err();
    assert!(
        matches!(err, trust_kernel::error::KernelError::StrongholdRequired),
        "privacy_mode=true without vault must return StrongholdRequired, got: {:?}",
        err
    );

    // 场景 3:privacy_mode=true + 注入未解锁 vault(degraded) -> Err
    let degraded_vault = {
        let conn = kernel.conn();
        Arc::new(StrongholdVault::degraded(&conn))
    };
    kernel.set_stronghold_vault(Some(degraded_vault));
    let err = kernel.ensure_stronghold_ready_for_privacy().unwrap_err();
    assert!(
        matches!(err, trust_kernel::error::KernelError::StrongholdRequired),
        "privacy_mode=true with degraded vault must return StrongholdRequired, got: {:?}",
        err
    );

    // 场景 4:privacy_mode=true + 注入已解锁 vault -> Ok
    let (_tmp, _vault_path) = setup_vault_path(&kernel);
    let unlocked_vault = {
        let conn = kernel.conn();
        Arc::new(StrongholdVault::create("privacy_test_password", &conn).expect("create"))
    };
    kernel.set_stronghold_vault(Some(unlocked_vault));
    assert!(
        kernel.ensure_stronghold_ready_for_privacy().is_ok(),
        "privacy_mode=true with unlocked vault must return Ok"
    );
}

/// 测试 8:vault 文件损坏时,unlock 返回 WrongPassword 或 VaultCorrupted。
/// 模拟:创建 vault -> 写入垃圾数据到 vault 文件 -> unlock 应失败。
#[test]
fn vault_corrupted_returns_error() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let (_tmp, vault_path) = setup_vault_path(&kernel);
    let conn = kernel.conn();

    let vault = StrongholdVault::create("original_password", &conn).expect("create");
    vault.lock();

    // 写入垃圾数据覆盖 vault 文件
    std::fs::write(&vault_path, b"corrupted vault data not a valid stronghold file")
        .expect("write corrupted data");

    // unlock 应返回 WrongPassword(Stronghold 加载失败被映射为 WrongPassword,
    // 因为攻击者无法区分 "密码错" 和 "文件损坏",防止侧信道)
    let err = vault.unlock("original_password", &conn).unwrap_err();
    assert!(
        matches!(err, StrongholdError::WrongPassword | StrongholdError::VaultCorrupted(_)),
        "corrupted vault must return WrongPassword or VaultCorrupted, got: {:?}",
        err
    );
}
