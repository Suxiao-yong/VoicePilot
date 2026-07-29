//! W9 Plan 1: StrongholdVault —— 封装 tauri-plugin-stronghold 的加密抽象。
//!
//! ## 职责
//!
//! - 管理 reverse_payload 加密 / 解密(XChaCha20Poly1305)
//! - 密钥派生:user_password -> Argon2id -> derived_key (32B) -> Stronghold SaltClientHash -> master_key
//! - 配置存储:salt / vault_path / enabled 持久化在 app_config 表(KV)
//! - 降级模式:密码错误时构造 `degraded()`,不加载补偿记录(spec §2.1 降级模式语义)
//!
//! ## 安全约束(spec §6.1)
//!
//! - Argon2id 参数固定:m=64MB t=3 p=4 output_len=32(不可配置,防降级攻击)
//! - Salt 16 bytes 随机,base64 编码后存 app_config.stronghold.salt
//! - lock() 时 zeroize 清零 derived_key + 释放 Stronghold 句柄
//! - 密码不在任何日志 / 审计 / 错误消息中出现
//! - vault 文件路径默认 ${data_dir}/stronghold.bin,文件权限 600(Windows ACL)
//!
//! ## 实现说明(W9 审查 P1-3 修复后的真实 API)
//!
//! `tauri-plugin-stronghold` 2.3.1 的 `Stronghold` 只暴露 `new` / `save` / `inner` / `Deref`,
//! 没有 `encrypt` / `decrypt` 方法。加密通过 `iota_stronghold::procedures::AeadEncrypt` /
//! `AeadDecrypt` 实现,需要 Client + Location + Key 管理。具体流程:
//!
//! 1. `Stronghold::new(path, derived_key.to_vec())` 初始化 / 加载 vault(derived_key 必须 32 字节)
//! 2. `stronghold.create_client(client_path)` 创建新 Client(首次)/ `load_client` 加载已有 Client
//! 3. `client.execute_procedure(WriteVault { data: random_key, location })` 写入加密 key
//! 4. `client.execute_procedure(AeadEncrypt { cipher: XChaCha20Poly1305, plaintext, nonce, key: location })` 加密
//! 5. `stronghold.save()` 持久化 snapshot 到 vault 文件
//!
//! 加密算法为 **XChaCha20Poly1305**(非 spec 写的 XSalsa20Poly1305),
//! 因为 `iota_stronghold` v2.1.0 的 `AeadCipher` enum 只暴露 `Aes256Gcm` / `XChaCha20Poly1305`
//! 两个变体。两者均为 AEAD,24 字节 nonce + 16 字节 Poly1305 tag,安全级别相同。
//! 偏离记录在 PROGRESS.md "已知偏离" 段落。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri_plugin_stronghold::stronghold::Stronghold;
// W9 审查修复 P1-1:用 Zeroizing 包装 derived_key,Drop 时自动清零,
// 替代不可靠的 drop(derived_key)。
use zeroize::Zeroizing;

/// Stronghold 加密错误。
///
/// `Display` 实现严格不含密码 / derived_key / salt 等敏感字段(spec §6.1 第 4 条)。
#[derive(Debug, thiserror::Error)]
pub enum StrongholdError {
    /// 降级模式或未调用 unlock。encrypt / decrypt 在此状态返回此错误。
    #[error("stronghold vault not unlocked")]
    NotUnlocked,
    /// Argon2id 派生密钥不匹配 / Stronghold 加载 vault 文件失败。
    /// 不含密码本身,仅 "wrong password or corrupted vault"。
    #[error("wrong password or corrupted vault")]
    WrongPassword,
    /// Stronghold 内部加密错误(序列化 / 加密原语失败)。
    #[error("encryption failed: {0}")]
    EncryptionFailed(String),
    /// Stronghold 内部解密错误(反序列化 / 解密原语失败)。
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    /// vault 文件损坏(无法加载 / 哈希校验失败)。
    #[error("vault corrupted: {0}")]
    VaultCorrupted(String),
    /// vault 文件不存在(unlock 时调用,与 WrongPassword 区分:首次启动 vs 密码错)。
    /// W9 审查修复 P0-2:之前 unlock 把"vault 文件不存在"误判为 WrongPassword。
    #[error("stronghold vault file not found at {path}")]
    VaultNotFound { path: String },
    /// vault 文件已存在(create 时调用,与 VaultCorrupted 区分:已存在 vs 损坏)。
    /// W9 审查修复 P0-3:之前 create 把"已存在"误用 VaultCorrupted 变体。
    #[error("stronghold vault file already exists at {path}")]
    AlreadyExists { path: String },
    /// KV 存储读写失败(包装 rusqlite::Error)。
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    /// I/O 错误(vault 文件读写)。
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Stronghold 加密后的负载,序列化为 BLOB 存入 compensations.snapshot_encrypted。
///
/// `salt_ref` 指向 app_config.stronghold.salt 的 key,便于未来 salt 轮换时
/// 定位历史记录用的 salt(当前实现仅一个 salt,salt_ref 恒为 "stronghold.salt")。
///
/// `ciphertext` 字段包含 Poly1305 tag(前 16 字节)+ 实际密文,
/// 因为 iota_stronghold::AeadEncrypt 输出 = tag + ciphertext 拼接。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    /// XChaCha20Poly1305 加密后的密文(前 16 字节为 Poly1305 tag)。
    pub ciphertext: Vec<u8>,
    /// XChaCha20Poly1305 的 nonce(24 bytes)。
    pub nonce: Vec<u8>,
    /// 指向 app_config 中存储的 salt 的 key(当前恒为 "stronghold.salt")。
    pub salt_ref: String,
}

/// StrongholdVault —— 封装 tauri-plugin-stronghold 的加密抽象。
///
/// ## 状态机
///
/// - `inner = None` + `unlocked = false`:降级模式或未调用 unlock
/// - `inner = Some(stronghold)` + `unlocked = true`:已解锁,可 encrypt / decrypt
/// - `lock()` 后:`inner = None` + `unlocked = false`(key material 已 zeroize)
///
/// ## 线程安全
///
/// `inner: Mutex<Option<Stronghold>>` 保护 Stronghold 句柄(Stronghold 内部
/// 非 Send + Sync,需 Mutex 串行化);`unlocked: AtomicBool` 允许并发读状态。
pub struct StrongholdVault {
    /// Stronghold 句柄,None = 未解锁 / 降级模式。
    inner: Mutex<Option<Stronghold>>,
    /// Argon2id salt(16 bytes),持久化在 app_config.stronghold.salt。
    /// 降级模式下为 None(无 salt 也能构造 degraded vault)。
    salt: Option<Vec<u8>>,
    /// vault 文件路径(${data_dir}/stronghold.bin 或用户自定义)。
    vault_path: PathBuf,
    /// 是否已解锁(AtomicBool 允许并发读,is_unlocked() 无锁)。
    unlocked: AtomicBool,
}
