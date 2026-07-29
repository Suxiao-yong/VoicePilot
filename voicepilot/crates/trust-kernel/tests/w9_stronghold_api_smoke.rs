//! W9 Plan 1 Task 1 Step 5.5: Stronghold API 签名 smoke 测试。
//!
//! 用途:在 Task 4/5 实现前验证 tauri-plugin-stronghold 2.x + iota_stronghold 2.x 的真实 API,
//! 避免基于错误 API 假设写完 Task 4/5 后才发现编译失败。
//! 验证完毕后可保留作为回归测试,或删除(不强制)。

#![cfg(feature = "stronghold")]

use iota_stronghold::procedures::{AeadCipher, AeadDecrypt, AeadEncrypt, WriteVault};
use iota_stronghold::Location;
use tauri_plugin_stronghold::stronghold::Stronghold;

#[test]
fn stronghold_api_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("test.bin");
    // Stronghold::new 要求 password 恰好 32 字节(NCKey::load 限制);
    // 生产代码用 Argon2id 派生 32 字节 derived_key 再传入。
    let password = [0x42u8; 32].to_vec();

    // 1. 验证 Stronghold::new 真实签名(2 个参数:path + password)
    let stronghold = Stronghold::new(&vault_path, password);
    assert!(stronghold.is_ok(), "Stronghold::new failed: {:?}", stronghold.err());
    let stronghold = stronghold.unwrap();

    // 2. 验证 create_client API
    let client = stronghold.create_client(b"voicepilot-test-client");
    assert!(client.is_ok(), "create_client failed: {:?}", client.err());
    let client = client.unwrap();

    // 3. 验证 WriteVault procedure 写入加密 key
    let key_location = Location::generic(
        b"voicepilot-test-vault".to_vec(),
        b"reverse-payload-key".to_vec(),
    );
    let key_data = [0x42u8; 32].to_vec(); // 模拟 32 字节加密 key
    let write_proc = WriteVault {
        data: zeroize::Zeroizing::new(key_data),
        location: key_location.clone(),
    };
    let write_result = client.execute_procedure(write_proc);
    assert!(write_result.is_ok(), "WriteVault failed: {:?}", write_result.err());

    // 4. 验证 AeadEncrypt procedure
    let plaintext = b"hello stronghold".to_vec();
    let nonce = [0x11u8; 24].to_vec(); // XChaCha20Poly1305 nonce = 24 bytes
    let encrypt_proc = AeadEncrypt {
        cipher: AeadCipher::XChaCha20Poly1305,
        associated_data: vec![],
        plaintext: plaintext.clone(),
        nonce: nonce.clone(),
        key: key_location.clone(),
    };
    let encrypt_result = client.execute_procedure(encrypt_proc);
    assert!(encrypt_result.is_ok(), "AeadEncrypt failed: {:?}", encrypt_result.err());
    let combined = encrypt_result.unwrap();
    // AeadEncrypt output = tag (16 bytes) + ciphertext
    assert_eq!(combined.len(), 16 + plaintext.len(), "tag + ciphertext length mismatch");

    // 5. 验证 AeadDecrypt procedure(roundtrip)
    let tag = combined[..16].to_vec();
    let ciphertext = combined[16..].to_vec();
    let decrypt_proc = AeadDecrypt {
        cipher: AeadCipher::XChaCha20Poly1305,
        associated_data: vec![],
        ciphertext,
        tag,
        nonce: nonce.clone(),
        key: key_location.clone(),
    };
    let decrypt_result = client.execute_procedure(decrypt_proc);
    assert!(decrypt_result.is_ok(), "AeadDecrypt failed: {:?}", decrypt_result.err());
    let decrypted = decrypt_result.unwrap();
    assert_eq!(decrypted, plaintext, "decrypt roundtrip mismatch");

    // 6. 验证 save() 持久化 snapshot
    let save_result = stronghold.save();
    assert!(save_result.is_ok(), "save failed: {:?}", save_result.err());
    assert!(vault_path.exists(), "vault file must exist after save");
}
