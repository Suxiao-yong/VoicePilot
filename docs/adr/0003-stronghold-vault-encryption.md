# ADR 0003: Stronghold Vault 加密快照

- **状态:** Accepted
- **日期:** 2026-07-28
- **决策者:** VoicePilot Team
- **标签:** crypto, security

## 背景

W9 需要加密补偿快照(reverse_payload)与敏感数据,防止明文残留。选择何种加密方案?

## 决策

使用 IOTA Stronghold(`iota_stronghold`)做密钥保险库 + XChaCha20Poly1305 加密。

- feature 门控:`stronghold` cargo feature,默认构建不开启
- 密钥派生:Argon2id(m=64MB, t=3, p=4, output_len=32)从 user_password 派生 derived_key
- 加密原语:`iota_stronghold::procedures::{AeadEncrypt, AeadDecrypt}`(XChaCha20Poly1305)
- 降级模式 `degraded()`:解密失败时跳过加密 + 标记 `snapshot_vault_ref="degraded"`
- `privacy_mode=true` 强制要求 `stronghold_enabled + vault injected + unlocked`

## 替代方案

- **直接 AES-256-GCM 本地密钥:** 拒绝。密钥明文存储,无侧信道保护。
- **OS 密钥环 / Windows DPAPI:** 拒绝。跨用户绑定、与 Tauri 生态集成复杂。
- **XSalsa20Poly1305(密钥环方案):** 拒绝。`iota_stronghold` 2.1.0 `AeadCipher` 仅暴露 `Aes256Gcm` / `XChaCha20Poly1305`,两者同为 AEAD、安全等级一致。

## 后果

- <正向:加密快照 + 保险库 + 侧信道防护(wrong password 与 vault corrupted 不区分)>
- <负向:Stronghold 密码要求 32 字节,需 Argon2id 派生;Argon2id 在低端 Windows 可能 OOM(已记录 V1.1+)>
- <中性:feature 门控,默认构建无加密依赖>

## 参考

- W9 设计文档 `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md`
- `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs`