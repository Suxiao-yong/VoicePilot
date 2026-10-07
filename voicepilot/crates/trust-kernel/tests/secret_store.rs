//! Wave 3 Task 3.1 — SecretStore contract tests (RED→GREEN).
//!
//! The in-memory store is the deterministic unit under test here; the
//! keyring-backed store is exercised on Windows by `secret_store_windows.rs`.

use std::sync::Arc;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::secrets::{InMemorySecretStore, KEYRING_SERVICE, LLM_API_KEY_NAME, SecretStore};

#[test]
fn in_memory_store_round_trips_secret() {
    let store = InMemorySecretStore::default();
    store
        .set_secret(LLM_API_KEY_NAME, "sk-test")
        .expect("set secret");
    assert_eq!(
        store
            .get_secret(LLM_API_KEY_NAME)
            .expect("get secret")
            .as_deref(),
        Some("sk-test")
    );
}

#[test]
fn in_memory_store_missing_returns_none() {
    let store = InMemorySecretStore::default();
    assert!(
        store
            .get_secret(LLM_API_KEY_NAME)
            .expect("get secret")
            .is_none()
    );
}

#[test]
fn in_memory_store_delete_removes_secret() {
    let store = InMemorySecretStore::default();
    store
        .set_secret(LLM_API_KEY_NAME, "sk-test")
        .expect("set secret");
    store
        .delete_secret(LLM_API_KEY_NAME)
        .expect("delete secret");
    assert!(
        store
            .get_secret(LLM_API_KEY_NAME)
            .expect("get secret")
            .is_none()
    );
}

#[test]
fn in_memory_store_delete_missing_is_noop() {
    let store = InMemorySecretStore::default();
    store
        .delete_secret(LLM_API_KEY_NAME)
        .expect("deleting a missing secret is a no-op");
}

#[test]
fn keys_are_namespaced() {
    let store = InMemorySecretStore::default();
    store
        .set_secret("voicepilot/other", "other")
        .expect("set other");
    assert!(store.get_secret(LLM_API_KEY_NAME).expect("get").is_none());
}

/// The canonical key/service names are stable public constants.
#[test]
fn canonical_secret_names_are_stable() {
    assert_eq!(LLM_API_KEY_NAME, "voicepilot/llm/api_key");
    assert_eq!(KEYRING_SERVICE, "voicepilot");
}

/// Kernel exposes the injected store and reads/writes the LLM key through it.
#[test]
fn kernel_secret_store_injection_and_llm_key_helpers() {
    let store: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
    let kernel =
        TrustKernel::open_in_memory_with_secret_store(store).expect("open with injected store");

    assert!(
        kernel.llm_api_key().expect("no key yet").is_none(),
        "no key initially"
    );

    kernel
        .set_llm_api_key("sk-kernel")
        .expect("set llm key in store");
    assert_eq!(
        kernel.llm_api_key().expect("read key").as_deref(),
        Some("sk-kernel")
    );

    kernel.clear_llm_api_key().expect("clear key");
    assert!(
        kernel.llm_api_key().expect("read after clear").is_none(),
        "key cleared from store"
    );
}

/// The legacy plaintext `llm.api_key` in app_config is migrated to the store:
/// success removes the plaintext row, marks api_key_present, and audits.
#[test]
fn migrate_legacy_llm_key_moves_plaintext_into_store() {
    let store: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
    let kernel =
        TrustKernel::open_in_memory_with_secret_store(store).expect("open with injected store");

    // Seed the legacy plaintext row (as pre-Wave-3 versions did).
    {
        let conn = kernel.conn();
        kernel
            .config_repo()
            .set(&conn, "llm.api_key", "sk-legacy")
            .expect("seed legacy plaintext key");
    }

    let outcome = kernel.migrate_legacy_llm_key();

    assert_eq!(
        outcome,
        trust_kernel::kernel::SecretMigrationOutcome::Migrated,
        "migration must move the key into the store"
    );
    // Key now lives in the store, not in SQLite.
    assert_eq!(
        kernel.llm_api_key().expect("read store").as_deref(),
        Some("sk-legacy")
    );
    {
        let conn = kernel.conn();
        assert!(
            kernel
                .config_repo()
                .get(&conn, "llm.api_key")
                .expect("query legacy key")
                .is_none(),
            "plaintext llm.api_key must be deleted after successful migration"
        );
        assert_eq!(
            kernel
                .config_repo()
                .get(&conn, "llm.api_key_present")
                .expect("query presence flag")
                .as_deref(),
            Some("true"),
            "api_key_present flag must be persisted"
        );
    }
}

/// No legacy plaintext → NoLegacyKey outcome, no store write, no audit event.
#[test]
fn migrate_legacy_llm_key_noop_without_legacy_key() {
    let store: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
    let kernel =
        TrustKernel::open_in_memory_with_secret_store(store).expect("open with injected store");

    let outcome = kernel.migrate_legacy_llm_key();
    assert_eq!(
        outcome,
        trust_kernel::kernel::SecretMigrationOutcome::NoLegacyKey
    );
    assert!(kernel.llm_api_key().expect("read store").is_none());
}

/// Store stub whose set_secret can be forced to fail, so the migration's
/// fail-closed write path (the core security property) is testable.
struct FlakySecretStore {
    fail_set: bool,
    map: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SecretStore for FlakySecretStore {
    fn set_secret(&self, key: &str, value: &str) -> trust_kernel::error::Result<()> {
        if self.fail_set {
            return Err(trust_kernel::error::KernelError::Secret(
                "test-forced set failure".into(),
            ));
        }
        self.map
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get_secret(&self, key: &str) -> trust_kernel::error::Result<Option<String>> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }

    fn delete_secret(&self, key: &str) -> trust_kernel::error::Result<()> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }
}

fn seed_legacy_key(kernel: &TrustKernel, key: &str) {
    let conn = kernel.conn();
    kernel
        .config_repo()
        .set(&conn, "llm.api_key", key)
        .expect("seed legacy plaintext key");
}

/// Fail-closed: writing to the SecretStore fails → keep the plaintext,
/// disable LLM, report Failed, never delete the old value.
#[test]
fn migrate_legacy_llm_key_set_failure_keeps_plaintext_and_fails_closed() {
    let store: Arc<dyn SecretStore> = Arc::new(FlakySecretStore {
        fail_set: true,
        map: Default::default(),
    });
    let kernel =
        TrustKernel::open_in_memory_with_secret_store(store).expect("open with injected store");
    seed_legacy_key(&kernel, "sk-legacy");

    let outcome = kernel.migrate_legacy_llm_key();
    assert_eq!(
        outcome,
        trust_kernel::kernel::SecretMigrationOutcome::Failed
    );

    // 旧明文保留(fail-closed:任一步失败都不删除旧值)。
    {
        let conn = kernel.conn();
        assert_eq!(
            kernel
                .config_repo()
                .get(&conn, "llm.api_key")
                .expect("query legacy key")
                .as_deref(),
            Some("sk-legacy")
        );
    }
    // store 未写入。
    assert!(kernel.llm_api_key().expect("read store").is_none());
    // LLM 已禁用(禁止远程调用)。
    #[cfg(feature = "llm")]
    assert!(
        kernel.llm_client().is_none(),
        "migration failure must disable LLM"
    );
}

/// Boot-time migration failure is non-fatal: the kernel still constructs, but
/// LLM is disabled (fail-closed).
#[test]
fn migrate_legacy_llm_key_boot_failure_is_non_fatal() {
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp
        .path()
        .to_str()
        .expect("tempfile path is utf-8")
        .to_string();

    // 先在一个文件 DB 中种入旧明文(以及 provider)。
    {
        let seed = TrustKernel::open_file_with_secret_store(
            &path,
            Arc::new(InMemorySecretStore::default()),
        )
        .expect("seed kernel");
        seed_legacy_key(&seed, "sk-legacy");
        let conn = seed.conn();
        seed.config_repo()
            .set(&conn, "llm.base_url", "https://example.com/v1")
            .expect("seed provider");
    }

    // 用失败 store 重新打开:boot 迁移失败,但构造不能失败。
    let store: Arc<dyn SecretStore> = Arc::new(FlakySecretStore {
        fail_set: true,
        map: Default::default(),
    });
    let kernel = TrustKernel::open_file_with_secret_store(&path, store)
        .expect("boot must not fail when migration fails");
    // fail-closed:LLM 禁用。
    #[cfg(feature = "llm")]
    assert!(
        kernel.llm_client().is_none(),
        "boot migration failure must disable LLM"
    );
}
