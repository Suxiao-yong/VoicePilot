#![cfg(windows)]

//! Wave 3 Task 3.1 — Windows-only: keyring-backed `SecretStore` against the
//! real Windows Credential Manager. Uses a throwaway key and always cleans
//! up after itself so CI / local runs leave no residue.

use trust_kernel::secrets::{KeyringSecretStore, SecretStore};

/// Throwaway entry key — distinct from the canonical LLM key so these tests
/// never touch a real user credential.
const TEST_KEY: &str = "voicepilot/test/throwaway-secret";

fn cleanup(store: &KeyringSecretStore) {
    let _ = store.delete_secret(TEST_KEY);
}

/// RAII cleanup:保证任何断言失败路径都会删除 throwaway 凭据,不残留。
struct Cleanup<'a> {
    store: &'a KeyringSecretStore,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        cleanup(self.store);
    }
}

#[test]
fn keyring_store_round_trips_secret_on_windows() {
    let store = KeyringSecretStore::default();
    let _guard = Cleanup { store: &store };

    store
        .set_secret(TEST_KEY, "sk-windows-test")
        .expect("set via Windows Credential Manager");
    assert_eq!(
        store
            .get_secret(TEST_KEY)
            .expect("get via Windows Credential Manager")
            .as_deref(),
        Some("sk-windows-test")
    );
}

#[test]
fn keyring_store_missing_returns_none() {
    let store = KeyringSecretStore::default();
    let _guard = Cleanup { store: &store };
    assert!(
        store.get_secret(TEST_KEY).expect("get missing key").is_none(),
        "missing keyring entry must read as None"
    );
}

#[test]
fn keyring_store_delete_missing_is_noop() {
    let store = KeyringSecretStore::default();
    let _guard = Cleanup { store: &store };
    store
        .delete_secret(TEST_KEY)
        .expect("deleting a missing keyring entry is a no-op");
}
