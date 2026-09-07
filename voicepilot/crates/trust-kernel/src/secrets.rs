//! Wave 3 Task 3.1 — SecretStore: 机密(当前是云 LLM API key)存放在
//! SQLite `app_config` 之外,避免明文落盘。
//!
//! 生产实现(Windows)用 `keyring` 读写 Windows Credential Manager;
//! 测试 / 非 Windows 回退用 `InMemorySecretStore`。
//!
//! 契约:
//!   - 实现必须永不把 secret 值写入日志 / tracing / audit details。
//!   - `LLM_API_KEY_NAME` 是 LLM API key 的唯一 canonical key。
//!   - Stronghold 只负责 encrypted compensation payload / privacy_mode 所需
//!     vault,与本 keyring 互不替代(spec §6.1)。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::{KernelError, Result};

/// LLM API key 的 canonical secret key。
pub const LLM_API_KEY_NAME: &str = "voicepilot/llm/api_key";

/// Windows Credential Manager 的 service 名。
pub const KEYRING_SERVICE: &str = "voicepilot";

/// 最小 SecretStore 抽象。实现必须满足上述"永不泄露值"契约。
pub trait SecretStore: Send + Sync {
    fn set_secret(&self, key: &str, value: &str) -> Result<()>;
    fn get_secret(&self, key: &str) -> Result<Option<String>>;
    fn delete_secret(&self, key: &str) -> Result<()>;
}

/// 内存实现(测试 / 非 Windows 回退)。不持久化。
#[derive(Default)]
pub struct InMemorySecretStore {
    map: Mutex<HashMap<String, String>>,
}

/// Debug 只暴露条目数,绝不打印 secret 值(安全契约)。
impl std::fmt::Debug for InMemorySecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemorySecretStore")
            .field("secret_count", &self.map.lock().unwrap().len())
            .finish()
    }
}

impl SecretStore for InMemorySecretStore {
    fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        self.map
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }

    fn delete_secret(&self, key: &str) -> Result<()> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }
}

/// 生产实现:Windows Credential Manager(keyring)。Windows-only。
#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct KeyringSecretStore {
    service: String,
}

#[cfg(windows)]
impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self {
            service: KEYRING_SERVICE.to_string(),
        }
    }
}

#[cfg(windows)]
impl SecretStore for KeyringSecretStore {
    fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        keyring::Entry::new(&self.service, key)
            .map_err(keyring_err)?
            .set_password(value)
            .map_err(keyring_err)
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(&self.service, key).map_err(keyring_err)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        }
    }

    fn delete_secret(&self, key: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, key).map_err(keyring_err)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(e)),
        }
    }
}

#[cfg(windows)]
fn keyring_err(e: keyring::Error) -> KernelError {
    // 错误信息只含 keyring 提供的文案,不携带 secret 值。
    KernelError::Secret(format!("keyring: {e}"))
}
