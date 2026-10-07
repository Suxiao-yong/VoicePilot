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
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
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
    /// 保留供 Plan 2+ 在加密 / 解密路径中读取当前 salt(例如 salt 轮换校验);
    /// 当前 Plan 1 仅在 create 时写入 DB,后续操作从 DB 重读。
    #[allow(dead_code)]
    salt: Option<Vec<u8>>,
    /// vault 文件路径(${data_dir}/stronghold.bin 或用户自定义)。
    /// 仅在 `#[cfg(test)]` 的 `vault_path()` accessor 中暴露。
    #[allow(dead_code)]
    vault_path: PathBuf,
    /// 是否已解锁(AtomicBool 允许并发读,is_unlocked() 无锁)。
    unlocked: AtomicBool,
}

// ===== W9 Plan 1 Task 3: 常量 + 私有辅助函数(Argon2id 密钥派生 + KV 读写)=====

/// KV 存储的 key 常量(参考 spec §2.1 配置存储表)。
const KV_KEY_SALT: &str = "stronghold.salt";
const KV_KEY_VAULT_PATH: &str = "stronghold.vault_path";
const KV_KEY_ENABLED: &str = "stronghold.enabled";

/// 默认 vault 文件名(放在 dirs::data_dir() 下)。
const DEFAULT_VAULT_FILENAME: &str = "stronghold.bin";

/// Argon2id 参数(spec §6.1 第 1 条,固定不可配置)。
const ARGON2_M_COST: u32 = 64 * 1024; // 64 MB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 4;
const ARGON2_OUTPUT_LEN: usize = 32;

/// Salt 长度(字节)。
const SALT_LEN: usize = 16;

/// XChaCha20Poly1305 nonce 长度(字节)。
pub const NONCE_LEN: usize = 24;

/// Poly1305 tag 长度(字节)。
pub const TAG_LEN: usize = 16;

/// Stronghold client path(固定,用于 create_client / load_client)。
const CLIENT_PATH: &[u8] = b"voicepilot-stronghold-client";

/// Stronghold vault 内部 key 的 Location vault_path。
const KEY_VAULT_PATH: &[u8] = b"voicepilot-reverse-payload-vault";

/// Stronghold vault 内部 key 的 Location record_path。
const KEY_RECORD_PATH: &[u8] = b"reverse-payload-key";

/// 用 Argon2id 从 user_password + salt 派生 32 字节 derived_key。
///
/// 参数固定:m=64MB t=3 p=4(spec §6.1 第 1 条)。
/// 失败返回 StrongholdError::EncryptionFailed(Argon2 内部错误)。
///
/// W9 审查修复 P1-1:返回 `Zeroizing<[u8; 32]>` 而非裸 `[u8; 32]`,
/// Drop 时自动清零,防止 key material 残留内存。
fn derive_key_argon2id(
    password: &str,
    salt: &[u8],
) -> Result<Zeroizing<[u8; ARGON2_OUTPUT_LEN]>, StrongholdError> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let params = Params::new(
        ARGON2_M_COST,
        ARGON2_T_COST,
        ARGON2_P_COST,
        Some(ARGON2_OUTPUT_LEN),
    )
    .map_err(|e| StrongholdError::EncryptionFailed(format!("argon2 params: {}", e)))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; ARGON2_OUTPUT_LEN]);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut *out)
        .map_err(|e| StrongholdError::EncryptionFailed(format!("argon2 hash: {}", e)))?;
    Ok(out)
}

/// 生成 16 字节随机 salt。
fn generate_salt() -> Vec<u8> {
    use rand::RngCore;
    let mut salt = vec![0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

/// 生成 24 字节随机 nonce(XChaCha20Poly1305)。
fn generate_nonce() -> Vec<u8> {
    use rand::RngCore;
    let mut nonce = vec![0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

/// 生成 32 字节随机 encryption key(存入 Stronghold vault)。
fn generate_encryption_key() -> Zeroizing<Vec<u8>> {
    use rand::RngCore;
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    Zeroizing::new(key)
}

/// 持久化 salt(base64 编码)到 app_config.stronghold.salt。
fn persist_salt(conn: &Connection, salt: &[u8]) -> Result<(), StrongholdError> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let encoded = STANDARD.encode(salt);
    crate::repo::config_repo::ConfigRepo::new()
        .set(conn, KV_KEY_SALT, &encoded)
        .map_err(|e| StrongholdError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
    Ok(())
}

/// 从 app_config.stronghold.salt 加载 salt(base64 解码)。
/// 返回 None 表示 key 不存在(首次启动)。
fn load_salt(conn: &Connection) -> Result<Option<Vec<u8>>, StrongholdError> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let repo = crate::repo::config_repo::ConfigRepo::new();
    let encoded_opt = repo
        .get(conn, KV_KEY_SALT)
        .map_err(|e| StrongholdError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
    match encoded_opt {
        None => Ok(None),
        Some(encoded) => {
            let salt = STANDARD.decode(encoded.trim()).map_err(|e| {
                StrongholdError::VaultCorrupted(format!("salt base64 decode: {}", e))
            })?;
            if salt.len() != SALT_LEN {
                return Err(StrongholdError::VaultCorrupted(format!(
                    "salt len {} != expected {}",
                    salt.len(),
                    SALT_LEN
                )));
            }
            Ok(Some(salt))
        }
    }
}

/// 解析 vault 文件路径:优先读 app_config.stronghold.vault_path,
/// 缺失时回退到 ${data_dir}/voicepilot/stronghold.bin(spec §2.1 配置存储表)。
fn resolve_vault_path(conn: &Connection) -> Result<PathBuf, StrongholdError> {
    let repo = crate::repo::config_repo::ConfigRepo::new();
    let custom_opt = repo
        .get(conn, KV_KEY_VAULT_PATH)
        .map_err(|e| StrongholdError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
    if let Some(custom) = custom_opt {
        if !custom.trim().is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }
    let data_dir = dirs::data_dir().ok_or_else(|| {
        StrongholdError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "dirs::data_dir() returned None",
        ))
    })?;
    let mut path = data_dir.join("voicepilot");
    // 确保父目录存在(best-effort,忽略 AlreadyExists)。
    std::fs::create_dir_all(&path)?;
    path.push(DEFAULT_VAULT_FILENAME);
    Ok(path)
}

/// 读取 stronghold.enabled 配置(默认 true,可禁用回退明文 PoC 用于测试)。
/// spec §2.1 配置存储表。
pub fn is_stronghold_enabled_in_config(conn: &Connection) -> bool {
    let repo = crate::repo::config_repo::ConfigRepo::new();
    match repo.get(conn, KV_KEY_ENABLED) {
        Ok(Some(v)) => v.trim().eq_ignore_ascii_case("true"),
        Ok(None) => true, // 默认启用
        Err(_) => true,   // 读取失败保守启用(防误禁用)
    }
}

/// 写入 stronghold.enabled 配置(Settings 面板可调用)。
pub fn set_stronghold_enabled_in_config(
    conn: &Connection,
    enabled: bool,
) -> Result<(), StrongholdError> {
    let repo = crate::repo::config_repo::ConfigRepo::new();
    repo.set(conn, KV_KEY_ENABLED, if enabled { "true" } else { "false" })
        .map_err(|e| StrongholdError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
    Ok(())
}

// ===== W9 Plan 1 Task 4/5: StrongholdVault 生命周期 + 加密方法 =====

use iota_stronghold::Location;
use iota_stronghold::procedures::{AeadCipher, AeadDecrypt, AeadEncrypt, WriteVault};

impl StrongholdVault {
    /// 创建新 vault(首次启动时调用)。
    ///
    /// 流程(spec §2.1 密钥派生策略 + W9 审查 P1-3 真实 API):
    /// 1. 检查 vault 文件不存在(防误覆盖,已存在返回 AlreadyExists)
    /// 2. 生成 16 字节随机 salt,base64 持久化到 app_config.stronghold.salt
    /// 3. Argon2id(password, salt) -> derived_key (32B, Zeroizing 包装)
    /// 4. Stronghold::new(vault_path, derived_key.to_vec()) 初始化 in-memory vault
    /// 5. stronghold.create_client(CLIENT_PATH) 创建 Client
    /// 6. 生成 32 字节随机 encryption key,WriteVault 写入 Location
    /// 7. stronghold.save() 持久化 snapshot 到 vault 文件
    /// 8. unlocked = true
    pub fn create(password: &str, db: &Connection) -> Result<Self, StrongholdError> {
        // 1. vault 文件路径 + 已存在检查
        let vault_path = resolve_vault_path(db)?;
        if vault_path.exists() {
            return Err(StrongholdError::AlreadyExists {
                path: vault_path.to_string_lossy().to_string(),
            });
        }

        // 2. 生成 + 持久化 salt
        let salt = generate_salt();
        persist_salt(db, &salt)?;

        // 3. Argon2id 派生 32 字节 derived_key
        let derived_key = derive_key_argon2id(password, &salt)?;

        // 4. Stronghold::new(derived_key 必须 32 字节,KeyProvider::try_from 限制)
        let stronghold = Stronghold::new(&vault_path, derived_key.to_vec())
            .map_err(|e| StrongholdError::EncryptionFailed(format!("stronghold new: {}", e)))?;

        // 5. create_client
        let client = stronghold
            .create_client(CLIENT_PATH)
            .map_err(|e| StrongholdError::EncryptionFailed(format!("create_client: {}", e)))?;

        // 6. 生成随机 encryption key + WriteVault 写入 Location
        let key_data = generate_encryption_key();
        let key_location = Location::generic(KEY_VAULT_PATH.to_vec(), KEY_RECORD_PATH.to_vec());
        let write_proc = WriteVault {
            data: key_data,
            location: key_location,
        };
        client
            .execute_procedure(write_proc)
            .map_err(|e| StrongholdError::EncryptionFailed(format!("WriteVault: {}", e)))?;

        // 7. save 持久化 snapshot
        stronghold
            .save()
            .map_err(|e| StrongholdError::EncryptionFailed(format!("save: {}", e)))?;

        // 8. 提前 drop derived_key(Zeroizing Drop 自动清零,减少 key material 在内存停留时间)
        drop(derived_key);

        Ok(Self {
            inner: Mutex::new(Some(stronghold)),
            salt: Some(salt),
            vault_path,
            unlocked: AtomicBool::new(true),
        })
    }

    /// 用密码解锁已有 vault。
    ///
    /// 流程:
    /// 1. 从 KV 加载 salt(若 None,vault 未创建,返回 WrongPassword)
    /// 2. Argon2id(password, salt) -> derived_key
    /// 3. 检查 vault 文件存在(否则 VaultNotFound,区分首次启动 vs 密码错)
    /// 4. Stronghold::new(vault_path, derived_key) 加载 snapshot
    ///    key 不匹配 / vault 损坏 -> Stronghold::new 失败 -> WrongPassword
    /// 5. load_client(CLIENT_PATH) 加载已存在的 Client
    ///    client 不存在 -> VaultCorrupted(snapshot 损坏 / 部分写入)
    /// 6. 标记 unlocked = true
    pub fn unlock(&self, password: &str, db: &Connection) -> Result<(), StrongholdError> {
        // 1. 加载 salt
        let salt = load_salt(db)?.ok_or(StrongholdError::WrongPassword)?;

        // 2. Argon2id 派生
        let derived_key = derive_key_argon2id(password, &salt)?;

        // 3. 检查 vault 文件存在
        let vault_path = resolve_vault_path(db)?;
        if !vault_path.exists() {
            return Err(StrongholdError::VaultNotFound {
                path: vault_path.to_string_lossy().to_string(),
            });
        }

        // 4. Stronghold::new 会调 load_snapshot;密码错 / vault 损坏均返回 Err
        //    攻击者无法区分两种情况,防侧信道(spec §6.1 第 4 条)
        let stronghold = Stronghold::new(&vault_path, derived_key.to_vec()).map_err(|e| {
            tracing::warn!(error = %e, "stronghold unlock failed (wrong password or corrupted)");
            StrongholdError::WrongPassword
        })?;

        // 5. load_client(CLIENT_PATH) - 从 snapshot 加载 Client
        stronghold
            .load_client(CLIENT_PATH)
            .map_err(|e| StrongholdError::VaultCorrupted(format!("load_client: {}", e)))?;

        // 6. 写入 inner + 标记 unlocked
        *self.inner.lock().unwrap() = Some(stronghold);
        self.unlocked
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // 提前 drop derived_key
        drop(derived_key);
        Ok(())
    }

    /// 锁定 vault(清空内存中的 key material)。
    ///
    /// spec §6.1 第 3 条:lock() 时 zeroize key material + 释放 Stronghold 句柄。
    /// W9 审查修复 P1-1:derived_key 用 Zeroizing 包装,create/unlock 作用域结束自动清零;
    /// Stronghold 句柄 take() 释放,其内部 KeyProvider Drop 时清零 NCKey。
    pub fn lock(&self) {
        *self.inner.lock().unwrap() = None;
        self.unlocked
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// 是否已解锁(AtomicBool 无锁读)。
    pub fn is_unlocked(&self) -> bool {
        self.unlocked.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 降级模式构造(无密码启动,不加载补偿记录)。
    ///
    /// spec §2.1 降级模式语义:
    /// - is_unlocked() = false
    /// - encrypt() 返回 NotUnlocked
    /// - decrypt() 返回 NotUnlocked
    /// - 调用方(Plan 2 create_post_commit_compensation)据此跳过加密 + 标记 snapshot_vault_ref = "degraded"
    pub fn degraded(db: &Connection) -> Self {
        // W9 审查修复 P1-4:用绝对路径(temp_dir)而非相对路径,避免 cwd 不稳定。
        let vault_path = resolve_vault_path(db)
            .unwrap_or_else(|_| std::env::temp_dir().join("voicepilot-stronghold-degraded.bin"));
        Self {
            inner: Mutex::new(None),
            salt: None,
            vault_path,
            unlocked: AtomicBool::new(false),
        }
    }

    // ===== W9 Plan 1 Task 5: encrypt / decrypt =====

    /// 加密 plaintext -> EncryptedPayload。未解锁时返回 NotUnlocked。
    ///
    /// 流程(W9 审查 P1-3 真实 API):
    /// 1. 检查 is_unlocked()(否则 NotUnlocked)
    /// 2. 取 Stronghold 句柄(MutexGuard)
    /// 3. get_client(CLIENT_PATH) 取已加载的 Client
    /// 4. 生成 24 字节随机 nonce(XChaCha20Poly1305)
    /// 5. AeadEncrypt procedure:输出 = tag (16B) + ciphertext 拼接
    /// 6. 构造 EncryptedPayload { ciphertext: tag+ciphertext, nonce, salt_ref }
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedPayload, StrongholdError> {
        if !self.is_unlocked() {
            return Err(StrongholdError::NotUnlocked);
        }
        let guard = self.inner.lock().unwrap();
        let stronghold = guard.as_ref().ok_or(StrongholdError::NotUnlocked)?;

        // get_client 取 create/unlock 时已加载的 Client
        let client = stronghold
            .get_client(CLIENT_PATH)
            .map_err(|e| StrongholdError::EncryptionFailed(format!("get_client: {}", e)))?;

        // 生成随机 nonce
        let nonce = generate_nonce();

        // AeadEncrypt:key location 指向 WriteVault 写入的随机 key
        let key_location = Location::generic(KEY_VAULT_PATH.to_vec(), KEY_RECORD_PATH.to_vec());
        let encrypt_proc = AeadEncrypt {
            cipher: AeadCipher::XChaCha20Poly1305,
            associated_data: vec![],
            plaintext: plaintext.to_vec(),
            nonce: nonce.clone(),
            key: key_location,
        };
        let combined = client
            .execute_procedure(encrypt_proc)
            .map_err(|e| StrongholdError::EncryptionFailed(format!("AeadEncrypt: {}", e)))?;

        Ok(EncryptedPayload {
            ciphertext: combined,
            nonce,
            salt_ref: KV_KEY_SALT.to_string(),
        })
    }

    /// 解密 EncryptedPayload -> plaintext。未解锁时返回 NotUnlocked。
    ///
    /// 流程:
    /// 1. 检查 is_unlocked()(否则 NotUnlocked)
    /// 2. 取 Stronghold 句柄
    /// 3. get_client(CLIENT_PATH)
    /// 4. 拆分 ciphertext:前 16 字节 = tag,其余 = 实际密文
    /// 5. AeadDecrypt procedure
    pub fn decrypt(&self, payload: &EncryptedPayload) -> Result<Vec<u8>, StrongholdError> {
        if !self.is_unlocked() {
            return Err(StrongholdError::NotUnlocked);
        }
        let guard = self.inner.lock().unwrap();
        let stronghold = guard.as_ref().ok_or(StrongholdError::NotUnlocked)?;

        let client = stronghold
            .get_client(CLIENT_PATH)
            .map_err(|e| StrongholdError::DecryptionFailed(format!("get_client: {}", e)))?;

        // 拆分 tag + ciphertext(AeadEncrypt 输出 = tag(16B) + ciphertext)
        if payload.ciphertext.len() < TAG_LEN {
            return Err(StrongholdError::DecryptionFailed(format!(
                "ciphertext too short: {} < {}",
                payload.ciphertext.len(),
                TAG_LEN
            )));
        }
        let tag = payload.ciphertext[..TAG_LEN].to_vec();
        let ciphertext = payload.ciphertext[TAG_LEN..].to_vec();

        let key_location = Location::generic(KEY_VAULT_PATH.to_vec(), KEY_RECORD_PATH.to_vec());
        let decrypt_proc = AeadDecrypt {
            cipher: AeadCipher::XChaCha20Poly1305,
            associated_data: vec![],
            ciphertext,
            tag,
            nonce: payload.nonce.clone(),
            key: key_location,
        };

        client
            .execute_procedure(decrypt_proc)
            .map_err(|e| StrongholdError::DecryptionFailed(format!("AeadDecrypt: {}", e)))
    }

    /// 返回 vault 文件路径(测试用,生产不暴露)。
    #[cfg(test)]
    pub fn vault_path(&self) -> &PathBuf {
        &self.vault_path
    }

    /// W9 Plan 1 Task 6: 进入降级模式:构造 degraded vault + 返回 reason。
    ///
    /// 审计事件由调用方(TrustKernel)负责写入,本方法仅返回 (vault, reason) 元组,
    /// 避免 StrongholdVault 持有 kernel 引用(防止循环依赖)。
    pub fn enter_degraded_mode(db: &Connection, reason: &'static str) -> (Self, &'static str) {
        let vault = Self::degraded(db);
        (vault, reason)
    }
}
