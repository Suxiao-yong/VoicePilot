# W9 Plan 1: Stronghold 加密基础 + 密钥管理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W9 设计文档(`docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.1 + §6.1)实现 VoicePilot TrustKernel 的 Stronghold 加密基础:新增 `crates/trust-kernel/src/crypto/stronghold.rs` 模块,封装 `tauri-plugin-stronghold`,提供 `StrongholdVault` 抽象,管理 reverse_payload 加密 / 解密 / 密钥派生 / 降级模式,并通过 feature flag `stronghold` 门控,使后续 Plan 2 能注入真实加密到 `create_post_commit_compensation`。

**Architecture:** `StrongholdVault` 内部持 `Mutex<Option<Stronghold>>`(None = 未解锁 / 降级模式)+ `AtomicBool`(unlocked 状态)+ `Salt`(持久化在 `app_config` 表的 KV)。密钥派生链路:`user_password → Argon2id(m=64MB, t=3, p=4) → derived_key (32B) → Stronghold SaltClientHash → master_key → XSalsa20Poly1305`。降级模式通过 `StrongholdVault::degraded()` 构造,`is_unlocked() = false`,后续 `create_post_commit_compensation` 据此跳过加密并标记 `snapshot_vault_ref = "degraded"`。`TrustKernel` 通过 `set_stronghold_vault` 注入 vault(参考 W8 Plan 4 `set_llm_client` 模式),`privacy_mode = true` 时强制要求 vault 已解锁(防止高隐私模式下明文落盘)。所有 Stronghold 代码用 `#[cfg(feature = "stronghold")]` 门控,与 `voice` / `tauri` feature 正交,可独立编译。

**Tech Stack:** Rust(stable)+ `tauri-plugin-stronghold` 2(XSalsa20Poly1305 + Argon2id + SaltClientHash)+ `argon2` 0.5(密钥派生)+ `rusqlite`(KV 持久化)+ `bincode`(EncryptedPayload 序列化)+ `zeroize`(key material 清零)+ `base64`(salt 序列化)+ TDD。

**Spec:** `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.1(Plan 1 范围 + 数据结构 + API + 密钥派生 + 配置存储 + 降级模式 + feature flag + privacy_mode 联动)+ §6.1(Stronghold 密钥管理安全约束)+ §10(Conventions:PowerShell / TDD / feature flag 模式 / 审计事件命名 / TrustKernel 非 Clone / commit message)+ §11(与 W8 兼容性:stronghold feature 未启用时保持 None 行为)

**Precondition:**
- W8 已完成(commit `8ec814d`,W8 Plan 1-6 全部验收门禁关闭,465 非门控测试通过)。
- W8 Plan 4 已实现 `privacy_mode()` accessor(从 `app_config.privacy.mode` 读取),`route_text_with_dag` 在 `privacy_mode = true` 时禁止调 LLM 拆解 —— 本 Plan 复用此 accessor。
- W8 Plan 4 已实现 `set_llm_client` / `llm_client` 注入模式(`Mutex<Option<Arc<LlmClient>>>`),本 Plan 的 `set_stronghold_vault` 沿用此模式。
- `app_config` 表已存在(migration 002,核实报告 1.7),`ConfigRepo::get` / `set` / `delete` 可用(`repo/config_repo.rs`)。
- `compensations` 表的 `snapshot_encrypted BLOB` + `snapshot_vault_ref TEXT` 列已存在(migration 001,核实报告 1.6),但当前 `snapshot_encrypted = None` 永远 —— Plan 2 修复,本 Plan 仅提供 vault 能力。
- `tauri-plugin-stronghold` 与 `argon2` 均未在 workspace `Cargo.toml`(核实报告 1.2-1.3),需新增。
- `trust-kernel/Cargo.toml` 已有 `uia = ["dep:uiautomation"]` feature 模式可参考(核实报告 3.4)。
- 既有 `#[cfg(feature = "llm")]` 门控模式可参考(`kernel.rs:38-39 / 80-81`)。
- `audit_append_external(task_id, step_id, event_type, details)` 已是 public 方法(`kernel.rs:290`),Plan 1 复用。
- `cargo check --workspace --features voice,tauri,llm,uia` 当前 PASS;clippy `-D warnings` 0 警告。
- 注意:`compensations` 表还有 `compensation_level TEXT` 列(`migrations/001_init.sql:70`,V1.1 alias),Plan 2 修改 `CompensationRepo::create` 时需注意该列存在(本 Plan 不涉及 CompensationRepo 改动)。

---

## File Structure

### Backend — Trust Kernel(`voicepilot/crates/trust-kernel/src/`)

- **Create** `crypto/mod.rs` — W9 Plan 1 新模块入口,声明 `stronghold` 子模块(整体 `#[cfg(feature = "stronghold")]` 门控)
- **Create** `crypto/stronghold.rs` — W9 Plan 1 核心模块,~400 行:
  - `StrongholdVault` 结构(`Mutex<Option<Stronghold>>` + `Salt` + `AtomicBool`)
  - `StrongholdError` 枚举(`NotUnlocked` / `WrongPassword` / `EncryptionFailed` / `DecryptionFailed` / `VaultCorrupted`)
  - `EncryptedPayload` 结构(`ciphertext` + `nonce` + `salt_ref`),`Serialize / Deserialize`(bincode)
  - `impl StrongholdVault`:`create` / `unlock` / `lock` / `encrypt` / `decrypt` / `is_unlocked` / `degraded`
  - 私有辅助:`derive_key_argon2id(password, salt) -> [u8; 32]` / `persist_salt(conn, salt) -> Result<()>` / `load_salt(conn) -> Result<Option<Salt>>` / `resolve_vault_path(conn) -> PathBuf`
- **Modify** `lib.rs` — 加 `pub mod crypto;`(无 feature gate,模块内部门控)
- **Modify** `kernel.rs` — 扩展 `TrustKernel`:
  - 新增字段 `stronghold_vault: std::sync::Mutex<Option<Arc<StrongholdVault>>>`(`#[cfg(feature = "stronghold")]` 门控;vault 不是 DTO 不跨进程边界,字段形状随 feature 变化可接受)
  - 新增 `set_stronghold_vault(&self, vault: Option<Arc<StrongholdVault>>)` 方法(`#[cfg(feature = "stronghold")]` 门控)
  - 新增 `stronghold_vault(&self) -> Option<Arc<StrongholdVault>>` 方法(`#[cfg(feature = "stronghold")]` 门控)
  - 新增 `stronghold_enabled(&self) -> bool` 方法(不门控,内部用 `#[cfg]` / `#[cfg(not)]` 分支:feature 启用时读 `app_config.stronghold.enabled`,否则返回 `false`)
  - 新增 `ensure_stronghold_ready_for_privacy(&self) -> Result<()>` 方法(不门控,`privacy_mode = true` 时,若 `stronghold_enabled && !vault.is_unlocked()` 返回 `KernelError::StrongholdRequired`)
  - 新增 `stronghold_enter_degraded_mode(&self, reason: &str) -> Result<()>` 方法(不门控,封装 `audit_append("stronghold_degraded_mode_entered", ...)`)
- **Modify** `error.rs` — 在 `KernelError` 加 2 个变体:
  - `Stronghold(String)` — 包装 `StrongholdError`(用 `#[from]` 自动转换)
  - `StrongholdRequired` — privacy_mode=true 但 vault 未解锁 / 未注入

### Build Configuration

- **Modify** `voicepilot/Cargo.toml`(workspace)— `[workspace.dependencies]` 加 2 项:
  - `tauri-plugin-stronghold = "2"`
  - `argon2 = "0.5"`
- **Modify** `voicepilot/crates/trust-kernel/Cargo.toml` — `[dependencies]` 加 2 项(optional)+ `[features]` 加 1 项:
  - `tauri-plugin-stronghold = { workspace = true, optional = true }`
  - `argon2 = { workspace = true, optional = true }`
  - `zeroize = { version = "1", optional = true, features = ["derive"] }`(W9 审查修复 P1-1:改为 optional + 通过 stronghold feature 启用)
  - `stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2", "dep:zeroize"]`(W9 审查修复 P1-1:加 `dep:zeroize`)
  - 另加非 optional:`bincode = "1"` / `base64 = "0.22"`

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w9_stronghold_unit.rs` — 8 个单元测试(整体 `#![cfg(feature = "stronghold")]`):
  1. `vault_create_persists_salt_and_path`
  2. `vault_unlock_with_correct_password_succeeds`
  3. `vault_unlock_with_wrong_password_returns_error`
  4. `vault_encrypt_decrypt_roundtrip`
  5. `vault_degraded_mode_is_unlocked_false`
  6. `vault_lock_clears_key_material`
  7. `privacy_mode_forces_stronghold_unlocked`
  8. `vault_corrupted_returns_error`

### Docs

- **Modify** `docs/PROGRESS.md` — W9 Plan 1 完成状态 + 测试统计(8 个 stronghold 单元测试)+ 已知偏离(stronghold feature 默认禁用,需 `--features stronghold` 启用)

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 W8 plan + project_memory.md "Lessons Learned")
- **TDD**:每个 Task 先写失败测试 → 跑红 → 实现 → 跑绿 → commit(参考 W8 plan / project_memory.md "Engineering Conventions")
- **Feature flag 模式**:`stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2"]`,参考 `uia` feature(核实报告 3.4);`stronghold` feature 与 `voice` / `tauri` / `llm` feature 正交,可独立编译(spec §10 第 7 条)
- **`#[cfg(feature = "stronghold")]` 门控位置**:`crypto/mod.rs` 整体门控 `pub mod stronghold;`(参考 `lib.rs:24-25` `#[cfg(feature = "voice")] pub mod voice;` 模式);`kernel.rs` 的 `stronghold_vault` 字段 + `set_stronghold_vault` / `stronghold_vault` 方法门控(参考 `kernel.rs:38-39` `#[cfg(feature = "llm")] llm_client` 模式);`stronghold_enabled` / `ensure_stronghold_ready_for_privacy` / `stronghold_enter_degraded_mode` 不门控(用内部 `#[cfg]` / `#[cfg(not)]` 分支保证 feature 关闭时也可调用)
- **TrustKernel 不是 Clone**:`AppState.kernel: Arc<TrustKernel>`,vault 用 `Arc<StrongholdVault>` 共享(参考 project_memory.md "Lessons Learned" + W8 Plan 4 `Arc<LlmClient>` 模式)
- **`&kernel.conn()` 不用 `&*kernel.conn()`**:避免 clippy `explicit_auto_deref` lint(参考 project_memory.md "Lessons Learned" + W8 plan5 conventions)
- **Repo accessor pattern**:`ConfigRepo::new()` 不带参数,方法接收 `&Connection`(参考 `repo/config_repo.rs` + project_memory.md "Engineering Conventions")
- **审计事件命名**:沿用 W7/W8 的 `lower_snake_case`(`stronghold_degraded_mode_entered` / `stronghold_vault_created` / `stronghold_vault_unlocked` / `stronghold_vault_lock_failed`),不用 SCREAMING_SNAKE(spec §10 第 4 条)
- **审计事件 details 不含敏感**:`reason` 字段仅 `"wrong_password"` / `"vault_corrupted"`,不含密码本身 / 不含 derived_key / 不含 salt(spec §6.1 第 4 条 + §6.4)
- **Argon2id 参数固定**:`m = 64 * 1024 * 1024`(64MB)/ `t = 3` / `p = 4` / `output_len = 32`(spec §6.1 第 1 条);不可配置(防止降级攻击)
- **Salt 16 bytes 随机**:`thread_rng().fill_bytes(&mut salt)`(spec §2.1 配置存储表),base64 编码后存 `app_config.stronghold.salt`
- **vault 文件路径默认**:`${data_dir}/stronghold.bin`,`data_dir` 用 `dirs::data_dir()`(W7 已引入 `dirs` 依赖);用户可在 `app_config.stronghold.vault_path` 覆盖
- **`zeroize` 清零 key material**:`lock()` 时 `derived_key.zeroize()` + `inner.lock().take()` 释放 Stronghold(spec §6.1 第 3 条)
- **密码不在任何日志 / 审计 / 错误消息中出现**:`StrongholdError::WrongPassword` 的 `Display` 实现只写 `"wrong password"`,不含 password(spec §6.1 第 4 条)
- **降级模式语义**:`degraded()` 构造的 vault `is_unlocked() = false`,`encrypt()` 返回 `StrongholdError::NotUnlocked`;Plan 2 据此跳过加密 + 标记 `snapshot_vault_ref = "degraded"`(spec §2.1 降级模式语义)
- **commit message 格式**:`feat(w9p1): ...` / `test(w9p1): ...` / `fix(w9p1): ...` / `docs(w9p1): ...`(spec §10 第 8 条)
- **不引入非必要依赖**:本 Plan 仅新增 `tauri-plugin-stronghold` + `argon2` + `bincode` + `base64` + `zeroize` 5 项(均 workspace 级或单 crate 级,无重复)
- **不修改 spec / 已有 plan**:若发现 spec 描述与实现不一致,记录到 PROGRESS.md "已知偏离" 段落,不回改 spec(spec §10 第 10 条)
- **cargo test 命令**:`cargo test --features stronghold -p trust-kernel --test w9_stronghold_unit`(单文件);`cargo check --features stronghold -p trust-kernel`(编译验证)
- **clippy 命令**:`cargo clippy --features stronghold -p trust-kernel -- -D warnings`(0 警告门禁)
- **windows 二进制兼容性**:若 `cargo check --features stronghold` 在 Windows 上失败(spec §4 风险登记最后一项),降级路径:本 Plan 暂停,先在 PROGRESS.md 记录失败原因,再 brainstorm 替代方案(`ring` + `aes-gcm` 自实现);不在本 Plan 内强行修复

---

## Task 1: workspace + trust-kernel Cargo.toml 添加 stronghold feature + 依赖

**Files:**
- Modify: `voicepilot/Cargo.toml:20-53`(`[workspace.dependencies]` 段)
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml:9-49`(`[dependencies]` + `[features]` 段)

- [ ] **Step 1: 在 workspace Cargo.toml 的 `[workspace.dependencies]` 末尾追加 2 项**

修改 `voicepilot/Cargo.toml` 第 52-53 行后追加(注意保留 `serde_yaml = "0.9"` 既有行):

```toml
# W7: LLM + 用户自定义 Skill 解析(UIA 依赖在 Plan 4 添加)
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde_yaml = "0.9"
# W9 Plan 1: Stronghold 加密基础(spec §2.1 feature flag + §6.1 密钥管理)
# - tauri-plugin-stronghold: 封装 IOTA Stronghold(XSalsa20Poly1305 + Argon2id + SaltClientHash)
# - argon2: 密钥派生(user_password → derived_key,参数 m=64MB t=3 p=4)
tauri-plugin-stronghold = "2"
argon2 = "0.5"
```

- [ ] **Step 2: 在 trust-kernel/Cargo.toml `[dependencies]` 段追加 3 项 non-optional + 2 项 optional**

修改 `voicepilot/crates/trust-kernel/Cargo.toml` 第 36 行(`regex = { workspace = true }`)后追加:

```toml
# W7 Plan 3: user_loader.rs 用 regex 校验 skill id。
regex = { workspace = true }
# W9 Plan 1: Stronghold 加密辅助库(非 optional,因为 EncryptedPayload 的 Serialize/Deserialize
# 在 feature 关闭时也需要类型存在 —— 但 StrongholdVault 本身用 #[cfg(feature)] 门控)
bincode = "1"
base64 = "0.22"
# W9 审查修复 P1-1:zeroize 改为 optional,通过 stronghold feature 启用,
# 确保 derived_key 的 Zeroizing 包装在 feature 关闭时不会被错误引用。
zeroize = { version = "1", optional = true, features = ["derive"] }
# W9 Plan 1: Stronghold 加密(optional,通过 stronghold feature 启用)
tauri-plugin-stronghold = { workspace = true, optional = true }
argon2 = { workspace = true, optional = true }
```

- [ ] **Step 3: 在 trust-kernel/Cargo.toml `[features]` 段追加 stronghold feature**

修改 `voicepilot/crates/trust-kernel/Cargo.toml` 第 45-49 行(原 `[features]` 段)改为:

```toml
[features]
default = ["llm"]
voice = ["dep:sherpa-rs", "dep:cpal", "dep:hound", "dep:ureq", "dep:bzip2", "dep:tar"]
llm = ["dep:reqwest"]
uia = ["dep:uiautomation"]
# W9 Plan 1: Stronghold 加密基础(spec §2.1 feature flag)。
# 与 voice / tauri / llm feature 正交,可独立编译(spec §10 第 7 条)。
# 启用方式:cargo check --features stronghold / cargo test --features stronghold
# W9 审查修复 P1-1:加 dep:zeroize,使 derived_key 的 Zeroizing 包装可用。
stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2", "dep:zeroize"]
```

- [ ] **Step 4: 验证 workspace cargo check 仍 PASS(无 stronghold feature)**

Run: `cargo check --workspace`
Expected: `Finished` 无错误(本步只加依赖声明,未引入使用点,无 feature gate 编译验证)

- [ ] **Step 5: 验证 stronghold feature 在 trust-kernel 上能编译(虽无使用点,但依赖需可解析)**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误(确认 `tauri-plugin-stronghold` + `argon2` 在 Windows 上能解析)

- [ ] **Step 5.5: Smoke 测试验证 Stronghold API 签名**

W9 审查修复 P1-3:Plan 1 在 Task 4/5 假设 `Stronghold::new(&vault_path, derived_key.to_vec(), None)` / `stronghold.encrypt(plaintext, &nonce)` 等 API 签名,但未给验证步骤。本 Step 在 Task 1 Step 5 cargo check 通过后,先写一个临时 smoke 测试验证 `Stronghold::new` / `stronghold.encrypt` 真实签名,避免 Task 4/5 才发现 API 不符造成返工。

在 `voicepilot/crates/trust-kernel/tests/` 下创建临时 smoke 测试 `w9_stronghold_api_smoke.rs`(整体 `#![cfg(feature = "stronghold")]`):

```rust
//! W9 Plan 1 Task 1 Step 5.5: Stronghold API 签名 smoke 测试。
//!
//! 用途:在 Task 4/5 实现前验证 tauri-plugin-stronghold 2.x 的真实 API,
//! 避免基于错误 API 假设写完 Task 4/5 后才发现编译失败。
//! 验证完毕后可保留作为回归测试,或删除(不强制)。

#![cfg(feature = "stronghold")]

#[test]
fn stronghold_api_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("test.bin");
    let derived_key = [0u8; 32];
    // 验证 Stronghold::new 真实签名(参数顺序/类型)
    // Plan 1 假设:Stronghold::new(&Path, Vec<u8>, Option<???>) -> Result<Stronghold, Error>
    let stronghold = tauri_plugin_stronghold::Stronghold::new(&vault_path, derived_key.to_vec(), None);
    // 若签名不同,此处编译失败,需调整 Plan 1 实现
    assert!(stronghold.is_ok());
    let stronghold = stronghold.unwrap();

    // 验证 encrypt 签名(Plan 1 假设:encrypt(&self, &[u8], &[u8]) -> Result<Vec<u8>, Error>)
    let nonce = [0u8; 24];
    let ciphertext = stronghold.encrypt(b"plaintext", &nonce);
    assert!(ciphertext.is_ok(), "encrypt signature mismatch: {:?}", ciphertext.err());
}
```

运行:`cargo test --features stronghold -p trust-kernel --test w9_stronghold_api_smoke`

期望:
- 若 smoke 测试编译 + 运行 PASS:Plan 1 假设的 API 签名正确,可继续 Task 2+。
- 若 smoke 测试编译失败(如 `Stronghold::new` 参数数量 / 类型不符,或 `encrypt` 方法名 / 签名不同):说明 Plan 1 假设的 API 与实际不符,**暂停 Task 2+**,先 brainstorm 替代实现:
  - 选项 A:查阅 `tauri-plugin-stronghold` 2.x 实际 API 文档,回更 Plan 1 Task 4/5 的 API 调用代码后继续;
  - 选项 B:若 API 完全不兼容(如要求异步 runtime / Tauri context),用 `ring` + `aes-gcm` 自实现加密层(spec §4 风险登记最后一项的降级路径),回更 Plan 1 全部 stronghold.rs 实现。
- 任何选项都需要回更本 Plan + spec,不可绕过此验证直接进入 Task 2。

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/Cargo.toml voicepilot/crates/trust-kernel/Cargo.toml voicepilot/crates/trust-kernel/tests/w9_stronghold_api_smoke.rs
git commit -m "feat(w9p1): add stronghold feature flag + tauri-plugin-stronghold/argon2 deps + API smoke test"
```

---

## Task 2: StrongholdVault 结构 + StrongholdError + EncryptedPayload 数据结构

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/crypto/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs`(本 Task 仅写数据结构,API 实现见 Task 3-5)
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs:22`(加 `pub mod crypto;`)
- Modify: `voicepilot/crates/trust-kernel/src/error.rs:46`(加 `Stronghold` + `StrongholdRequired` 变体)

- [ ] **Step 1: 创建 crypto/mod.rs(模块入口,门控 stronghold 子模块)**

创建 `voicepilot/crates/trust-kernel/src/crypto/mod.rs`:

```rust
//! W9 Plan 1: 加密原语模块。
//!
//! 仅当 `stronghold` feature 启用时,`stronghold` 子模块可见。
//! `stronghold` 子模块封装 `tauri-plugin-stronghold`,提供 `StrongholdVault`
//! 抽象,管理 reverse_payload 加密 / 解密 / 密钥派生 / 降级模式(spec §2.1)。

#[cfg(feature = "stronghold")]
pub mod stronghold;
```

- [ ] **Step 2: 在 lib.rs 加 `pub mod crypto;` 声明**

修改 `voicepilot/crates/trust-kernel/src/lib.rs`,在 `pub mod llm;` 后追加 `pub mod crypto;`(W9 审查修复 P1-2:不依赖行号,经核实 `pub mod llm;` 在第 22 行,而非 `pub mod skills;`):

```rust
pub mod skills;
pub mod llm;
// W9 Plan 1: 加密原语模块(stronghold 子模块内部 #[cfg(feature = "stronghold")] 门控)
pub mod crypto;
```

- [ ] **Step 3: 在 error.rs 加 Stronghold + StrongholdRequired 变体**

修改 `voicepilot/crates/trust-kernel/src/error.rs`,在 `Uia(String)` 变体后追加(W9 审查修复 P1-2:不依赖行号,经核实 `Uia(String)` 在第 46 行,error.rs 共 48 行):

```rust
    /// W7 Plan 4: Windows UIA automation error (wraps `uiautomation::Error`).
    #[error("uia error: {0}")]
    Uia(String),
    /// W9 Plan 1: Stronghold 加密错误(包装 StrongholdError)。
    /// 用字符串而非 #[from]:StrongholdError 在 feature 关闭时不存在,
    /// 但 KernelError 必须在所有 feature 组合下都编译通过。
    #[cfg(feature = "stronghold")]
    #[error("stronghold error: {0}")]
    Stronghold(#[from] crate::crypto::stronghold::StrongholdError),
    /// W9 Plan 1: privacy_mode=true 但 Stronghold vault 未解锁 / 未注入。
    /// 防止高隐私模式下 reverse_payload 明文落盘(spec §2.1 privacy_mode 联动)。
    #[error("stronghold vault required: privacy_mode is true but vault not unlocked")]
    StrongholdRequired,
}
```

- [ ] **Step 4: 创建 crypto/stronghold.rs(本 Task 仅数据结构 + 错误类型,不写 impl)**

创建 `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs`:

```rust
//! W9 Plan 1: StrongholdVault —— 封装 tauri-plugin-stronghold 的加密抽象。
//!
//! ## 职责
//!
//! - 管理 reverse_payload 加密 / 解密(XSalsa20Poly1305)
//! - 密钥派生:user_password → Argon2id → derived_key → Stronghold SaltClientHash → master_key
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

use crate::error::KernelError;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri_plugin_stronghold::Stronghold;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    /// XSalsa20Poly1305 加密后的密文。
    pub ciphertext: Vec<u8>,
    /// XSalsa20Poly1305 的 nonce(24 bytes)。
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
```

- [ ] **Step 5: 验证 stronghold feature 下编译 PASS**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误(数据结构定义无逻辑,仅类型声明)

- [ ] **Step 6: 验证 default feature 下编译仍 PASS(无 feature gate 退化)**

Run: `cargo check -p trust-kernel`
Expected: `Finished` 无错误(`StrongholdVault` / `StrongholdError` / `EncryptedPayload` 整体被 `#[cfg(feature = "stronghold")]` 门控,不影响 default 编译)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/crypto/ voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/src/error.rs
git commit -m "feat(w9p1): add StrongholdVault/StrongholdError/EncryptedPayload data structures"
```

---

## Task 3: Argon2id 密钥派生 + config 存储(salt / vault_path / enabled)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs`(本 Task 加私有辅助函数 + KV 读写)

- [ ] **Step 1: 在 stronghold.rs 末尾追加私有辅助函数(密钥派生 + KV 读写)**

在 `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs` 末尾追加:

```rust
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

/// 用 Argon2id 从 user_password + salt 派生 32 字节 derived_key。
///
/// 参数固定:m=64MB t=3 p=4(spec §6.1 第 1 条)。
/// 失败返回 StrongholdError::EncryptionFailed(Argon2 内部错误)。
///
/// W9 审查修复 P1-1:返回 `Zeroizing<[u8; 32]>` 而非裸 `[u8; 32]`,
/// Drop 时自动清零,防止 key material 残留内存。
fn derive_key_argon2id(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; ARGON2_OUTPUT_LEN]>, StrongholdError> {
    use argon2::{Argon2, Algorithm, Version, Params};
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(ARGON2_OUTPUT_LEN))
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

/// 持久化 salt(base64 编码)到 app_config.stronghold.salt。
fn persist_salt(conn: &Connection, salt: &[u8]) -> Result<(), StrongholdError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let encoded = STANDARD.encode(salt);
    crate::repo::config_repo::ConfigRepo::new().set(conn, KV_KEY_SALT, &encoded)?;
    Ok(())
}

/// 从 app_config.stronghold.salt 加载 salt(base64 解码)。
/// 返回 None 表示 key 不存在(首次启动)。
fn load_salt(conn: &Connection) -> Result<Option<Vec<u8>>, StrongholdError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let repo = crate::repo::config_repo::ConfigRepo::new();
    match repo.get(conn, KV_KEY_SALT)? {
        None => Ok(None),
        Some(encoded) => {
            let salt = STANDARD
                .decode(encoded.trim())
                .map_err(|e| StrongholdError::VaultCorrupted(format!("salt base64 decode: {}", e)))?;
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
/// 缺失时回退到 ${data_dir}/stronghold.bin(spec §2.1 配置存储表)。
fn resolve_vault_path(conn: &Connection) -> Result<PathBuf, StrongholdError> {
    let repo = crate::repo::config_repo::ConfigRepo::new();
    if let Some(custom) = repo.get(conn, KV_KEY_VAULT_PATH)? {
        if !custom.trim().is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }
    let data_dir = dirs::data_dir()
        .ok_or_else(|| StrongholdError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "dirs::data_dir() returned None",
        )))?;
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
pub fn set_stronghold_enabled_in_config(conn: &Connection, enabled: bool) -> Result<(), StrongholdError> {
    let repo = crate::repo::config_repo::ConfigRepo::new();
    repo.set(conn, KV_KEY_ENABLED, if enabled { "true" } else { "false" })?;
    Ok(())
}
```

- [ ] **Step 2: 验证编译 PASS**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 3: 检查 clippy 0 警告(强约束)**

Run: `cargo clippy --features stronghold -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告(若 `unwrap_or_else` 链路或 `Vec<u8>` 长度判断触发 lint,按 clippy 提示修正)

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/crypto/stronghold.rs
git commit -m "feat(w9p1): add Argon2id key derivation + config KV storage helpers"
```

---

## Task 4: create / unlock / lock / is_unlocked 方法实现

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs`(本 Task 加 `impl StrongholdVault` 的生命周期方法)

- [ ] **Step 1: 在 stronghold.rs 末尾追加 `impl StrongholdVault` 块(create / unlock / lock / is_unlocked / degraded)**

在 `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs` 末尾追加:

```rust
impl StrongholdVault {
    /// 创建新 vault(首次启动时调用)。
    ///
    /// 流程(spec §2.1 密钥派生策略):
    /// 1. 生成 16 字节随机 salt
    /// 2. base64 编码后持久化到 app_config.stronghold.salt
    /// 3. Argon2id(password, salt) → derived_key (32B)
    /// 4. Stronghold::new(vault_path, derived_key) → Stronghold 句柄
    /// 5. unlocked = true
    ///
    /// 若 vault_path 已存在文件,返回 AlreadyExists(用 create 覆盖已有 vault 是错误用法,
    /// 应用 unlock)。
    pub fn create(password: &str, db: &Connection) -> Result<Self, StrongholdError> {
        // 1. 检查 vault 文件是否已存在(防误覆盖)
        let vault_path = resolve_vault_path(db)?;
        if vault_path.exists() {
            // W9 审查修复 P0-3:之前用 VaultCorrupted 变体,把"已存在"误用为"损坏"。
            return Err(StrongholdError::AlreadyExists {
                path: vault_path.to_string_lossy().to_string(),
            });
        }

        // 2. 生成 + 持久化 salt
        let salt = generate_salt();
        persist_salt(db, &salt)?;

        // 3. Argon2id 派生
        let derived_key = derive_key_argon2id(password, &salt)?;

        // 4. Stronghold::new(用 derived_key 作为 passphrase)
        //    tauri-plugin-stronghold 内部会做 SaltClientHash 二次派生。
        //    derived_key 是 Zeroizing<[u8; 32]>,通过 Deref 自动 deref 调 to_vec()。
        let stronghold = Stronghold::new(&vault_path, derived_key.to_vec(), None)
            .map_err(|e| StrongholdError::EncryptionFailed(format!("stronghold new: {}", e)))?;

        // 5. W9 审查修复 P1-1:derived_key 已用 Zeroizing 包装,Drop 时自动清零;
        //    显式 drop 提前清零(早于函数返回),减少 key material 在内存中的停留时间。
        drop(derived_key);

        Ok(Self {
            inner: Mutex::new(Some(stronghold)),
            salt: Some(salt),
            vault_path,
            unlocked: AtomicBool::new(true),
        })
    }

    /// 用密码解锁已有 vault。密码错误返回 WrongPassword;vault 文件不存在返回 VaultNotFound。
    ///
    /// 流程:
    /// 1. 从 KV 加载 salt(若 None,vault 未创建,返回 WrongPassword)
    /// 2. Argon2id(password, salt) → derived_key
    /// 3. Stronghold::new(vault_path, derived_key) → 尝试加载 vault 文件
    ///    若 vault 文件不存在 → VaultNotFound(W9 审查修复 P0-2:之前误用 WrongPassword)
    ///    若 derived_key 不匹配 → WrongPassword
    /// 4. unlocked = true
    pub fn unlock(&self, password: &str, db: &Connection) -> Result<(), StrongholdError> {
        // 1. 加载 salt
        let salt = load_salt(db)?
            .ok_or(StrongholdError::WrongPassword)?;

        // 2. Argon2id 派生
        let derived_key = derive_key_argon2id(password, &salt)?;

        // 3. 尝试加载 Stronghold(若 vault 文件不存在,返回 VaultNotFound;
        //    若 key 不匹配,返回 WrongPassword)
        let vault_path = resolve_vault_path(db)?;
        if !vault_path.exists() {
            // W9 审查修复 P0-2:之前用 WrongPassword,把首次启动误判为密码错。
            return Err(StrongholdError::VaultNotFound {
                path: vault_path.to_string_lossy().to_string(),
            });
        }
        let stronghold = Stronghold::new(&vault_path, derived_key.to_vec(), None)
            .map_err(|e| {
                tracing::warn!(error = ?e, "stronghold unlock failed");
                StrongholdError::WrongPassword
            })?;

        // 4. 写入 inner + 标记 unlocked
        *self.inner.lock().unwrap() = Some(stronghold);
        self.unlocked.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// 锁定 vault(清空内存中的 key material)。
    ///
    /// spec §6.1 第 3 条:lock() 时 zeroize key material + 释放 Stronghold 句柄。
    pub fn lock(&self) {
        // take() 释放 Stronghold 句柄(其内部 Drop 会清零敏感字段)
        *self.inner.lock().unwrap() = None;
        self.unlocked.store(false, std::sync::atomic::Ordering::SeqCst);
        // W9 审查修复 P1-1:derived_key 已用 Zeroizing 包装,Drop 时自动清零;
        // Stronghold 句柄 take() 释放。lock() 调用时 derived_key 早已超出
        // create()/unlock() 作用域并被 Zeroizing::drop 清零,无需在此重复操作。
    }

    /// 是否已解锁。
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
        // W9 审查修复 P1-4:之前用 PathBuf::from("stronghold.bin") 相对路径,
        // cwd 不稳定(测试 / Tauri runtime / CLI 启动 cwd 不同),改为 temp_dir 绝对路径。
        let vault_path = resolve_vault_path(db).unwrap_or_else(|_| {
            PathBuf::from(std::env::temp_dir().join("voicepilot-stronghold-degraded.bin"))
        });
        Self {
            inner: Mutex::new(None),
            salt: None,
            vault_path,
            unlocked: AtomicBool::new(false),
        }
    }

    /// 返回 vault 文件路径(测试用,生产不暴露)。
    #[cfg(test)]
    pub fn vault_path(&self) -> &PathBuf {
        &self.vault_path
    }
}
```

- [ ] **Step 2: 验证编译 PASS**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 3: 检查 clippy 0 警告**

Run: `cargo clippy --features stronghold -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/crypto/stronghold.rs
git commit -m "feat(w9p1): implement StrongholdVault create/unlock/lock/is_unlocked/degraded"
```

---

## Task 5: encrypt / decrypt 方法实现

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs`(本 Task 在 `impl StrongholdVault` 块追加 encrypt / decrypt)

- [ ] **Step 1: 在 stronghold.rs 的 `impl StrongholdVault` 块中追加 encrypt / decrypt 方法**

在 `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs` 的 `impl StrongholdVault` 块(在 `vault_path` 测试方法之前)追加:

```rust
    /// 加密 plaintext → EncryptedPayload。未解锁时返回 NotUnlocked。
    ///
    /// 流程:
    /// 1. 检查 is_unlocked()(否则 NotUnlocked)
    /// 2. 取 Stronghold 句柄(MutexGuard)
    /// 3. 生成随机 nonce(24 bytes,XSalsa20Poly1305 标准)
    /// 4. Stronghold::encrypt(plaintext, nonce) → ciphertext
    /// 5. 构造 EncryptedPayload { ciphertext, nonce, salt_ref: "stronghold.salt" }
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedPayload, StrongholdError> {
        if !self.is_unlocked() {
            return Err(StrongholdError::NotUnlocked);
        }
        let guard = self.inner.lock().unwrap();
        let stronghold = guard
            .as_ref()
            .ok_or(StrongholdError::NotUnlocked)?;

        // 生成随机 nonce(XSalsa20Poly1305 nonce = 24 bytes)
        let mut nonce = vec![0u8; 24];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut nonce);

        // Stronghold 加密(API 形式可能因 tauri-plugin-stronghold 版本而异,
        // 此处以 2.x 主流 API 为准;若实际 API 不同,实现时调整)
        let ciphertext = stronghold
            .encrypt(plaintext, &nonce)
            .map_err(|e| StrongholdError::EncryptionFailed(format!("stronghold encrypt: {}", e)))?;

        Ok(EncryptedPayload {
            ciphertext,
            nonce,
            salt_ref: KV_KEY_SALT.to_string(),
        })
    }

    /// 解密 EncryptedPayload → plaintext。未解锁时返回 NotUnlocked。
    ///
    /// 流程:
    /// 1. 检查 is_unlocked()(否则 NotUnlocked)
    /// 2. 取 Stronghold 句柄(MutexGuard)
    /// 3. Stronghold::decrypt(ciphertext, nonce) → plaintext
    /// 4. 返回 plaintext(Vec<u8>)
    pub fn decrypt(&self, payload: &EncryptedPayload) -> Result<Vec<u8>, StrongholdError> {
        if !self.is_unlocked() {
            return Err(StrongholdError::NotUnlocked);
        }
        let guard = self.inner.lock().unwrap();
        let stronghold = guard
            .as_ref()
            .ok_or(StrongholdError::NotUnlocked)?;

        stronghold
            .decrypt(&payload.ciphertext, &payload.nonce)
            .map_err(|e| StrongholdError::DecryptionFailed(format!("stronghold decrypt: {}", e)))
    }
```

- [ ] **Step 2: 验证编译 PASS**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 3: 检查 clippy 0 警告**

Run: `cargo clippy --features stronghold -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/crypto/stronghold.rs
git commit -m "feat(w9p1): implement StrongholdVault encrypt/decrypt with XSalsa20Poly1305"
```

---

## Task 6: degraded() 降级模式审计事件 + audit_append 集成

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs`(本 Task 加 `enter_degraded_mode` 公开方法,触发审计事件)
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`(本 Task 加 `stronghold_enter_degraded_mode` 方法,封装 audit_append)

- [ ] **Step 1: 在 kernel.rs 加 `stronghold_enter_degraded_mode` 方法(封装审计)**

修改 `voicepilot/crates/trust-kernel/src/kernel.rs`,在 `audit_append_external` 方法(第 290 行)之后追加:

```rust
    /// W9 Plan 1: 记录 Stronghold 降级模式进入事件(spec §6.4 审计事件表)。
    ///
    /// `reason` 仅取 "wrong_password" / "vault_corrupted",不含密码 / derived_key /
    /// salt 等敏感字段(spec §6.1 第 4 条 + §6.4)。
    ///
    /// task_id 占位:降级模式无活跃 task,但 `audit_logs.task_id` 是 FK
    /// REFERENCES `tasks(task_id)`(migrations/001_init.sql:77),字面量
    /// "unknown-task" 会触发 FK 违约。本方法先创建占位 task 行满足 FK,
    /// 再用其 task_id 写审计。step_id = None。
    pub fn stronghold_enter_degraded_mode(&self, reason: &str) -> Result<()> {
        // FK 约束要求 task_id 必须存在于 tasks 表中。先创建占位 task。
        // TaskRepo::create 签名:create(&self, conn: &Connection, task: &TaskRecord) -> Result<()>
        // (核实 voicepilot/crates/trust-kernel/src/repo/task_repo.rs:45)
        let placeholder_task_id = format!("stronghold-degraded-{}", uuid::Uuid::new_v4());
        {
            // 用 block scope 限制 MutexGuard 生命周期,避免 audit_append 二次加锁死锁。
            let conn = self.conn();
            let placeholder = crate::repo::task_repo::TaskRecord::new(
                &placeholder_task_id,
                "stronghold degraded mode placeholder",
            );
            crate::repo::task_repo::TaskRepo::new().create(&conn, &placeholder)?;
        }
        self.audit_append(
            &placeholder_task_id,
            None,
            "stronghold_degraded_mode_entered",
            serde_json::json!({
                "reason": reason,
            }),
        )
    }
```

- [ ] **Step 2: 在 stronghold.rs 的 `impl StrongholdVault` 块追加 `enter_degraded_mode` 静态方法**

在 `voicepilot/crates/trust-kernel/src/crypto/stronghold.rs` 的 `impl StrongholdVault` 块末尾追加:

```rust
    /// 进入降级模式:构造 degraded vault + 审计记录原因。
    ///
    /// 调用方(TrustKernel 启动逻辑 / Plan 2 reverse_compensation)在以下场景调用:
    /// - 启动时 unlock(password) 返回 WrongPassword → reason = "wrong_password"
    /// - 启动时 vault 文件损坏 → reason = "vault_corrupted"
    ///
    /// 审计事件由调用方(TrustKernel)负责写入,本方法仅返回 (vault, reason) 元组,
    /// 避免StrongholdVault 持有 kernel 引用(防止循环依赖)。
    pub fn enter_degraded_mode(
        db: &Connection,
        reason: &'static str,
    ) -> (Self, &'static str) {
        let vault = Self::degraded(db);
        (vault, reason)
    }
```

- [ ] **Step 3: 验证编译 PASS**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 4: 验证 default feature 仍 PASS(kernel.rs 改动不应被 feature 门控)**

Run: `cargo check -p trust-kernel`
Expected: `Finished` 无错误(`stronghold_enter_degraded_mode` 方法不门控,审计事件字符串 "stronghold_degraded_mode_entered" 在任何 feature 下都可写入)

- [ ] **Step 5: 检查 clippy 0 警告(双 feature)**

Run: `cargo clippy --features stronghold -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

Run: `cargo clippy -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/crypto/stronghold.rs voicepilot/crates/trust-kernel/src/kernel.rs
git commit -m "feat(w9p1): add degraded mode entry + stronghold_degraded_mode_entered audit event"
```

---

## Task 7: TrustKernel 集成(set_stronghold_vault / stronghold_vault / stronghold_enabled)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs:19-40`(TrustKernel 结构体加字段)
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs:55-82`(with_conn 初始化字段)

- [ ] **Step 1: 在 TrustKernel 结构体加 `stronghold_vault` 字段**

修改 `voicepilot/crates/trust-kernel/src/kernel.rs` 第 39 行(`llm_client` 字段后)追加:

```rust
    #[cfg(feature = "llm")]
    llm_client: std::sync::Mutex<Option<Arc<crate::llm::client::LlmClient>>>,
    // W9 Plan 1: Stronghold vault(可选,None = 未注入 / feature 未启用)。
    // 用 `Mutex<Option<Arc<StrongholdVault>>>` 而非 `Arc<Mutex<...>>`:kernel 是唯一 owner,
    // 不需要 Arc 共享;StrongholdVault 内部已有 Mutex<Option<Stronghold>>,
    // 外层 Mutex 仅保护 "是否已注入" 状态的替换(set_stronghold_vault)。
    //
    // 门控决策:`#[cfg(feature = "stronghold")]` 门控(与 `llm_client` 模式一致)。
    // vault 不是 DTO(不跨进程边界 / 不序列化),字段形状随 feature 变化可接受;
    // `set_stronghold_vault` / `stronghold_vault` 方法同样门控;
    // `stronghold_enabled` / `ensure_stronghold_ready_for_privacy` 不门控(用内部 #[cfg] 分支)。
    #[cfg(feature = "stronghold")]
    stronghold_vault: std::sync::Mutex<Option<Arc<crate::crypto::stronghold::StrongholdVault>>>,
}
```

- [ ] **Step 2: 在 `with_conn` 初始化 stronghold_vault 字段**

修改 `voicepilot/crates/trust-kernel/src/kernel.rs` 第 80-82 行(`llm_client` 初始化后)追加:

```rust
            #[cfg(feature = "llm")]
            llm_client: std::sync::Mutex::new(None),
            // W9 Plan 1: 默认无 Stronghold vault(None)。Settings 面板或启动逻辑
            // 在用户输入密码后调 `set_stronghold_vault(Some(Arc::new(StrongholdVault::create(...))))` 注入。
            #[cfg(feature = "stronghold")]
            stronghold_vault: std::sync::Mutex::new(None),
        };
```

- [ ] **Step 3: 在 kernel.rs 的 `set_llm_client` 方法后追加 Stronghold accessors**

修改 `voicepilot/crates/trust-kernel/src/kernel.rs` 第 199 行(`set_llm_client` 方法结束后)追加:

```rust
    // ===== W9 Plan 1: Stronghold vault accessors =====

    /// W9 Plan 1: 注入或清除 Stronghold vault。
    /// Settings 面板 / 启动逻辑在用户输入密码后调
    /// `set_stronghold_vault(Some(Arc::new(StrongholdVault::create(password, &conn))))`;
    /// 降级模式调 `set_stronghold_vault(Some(Arc::new(StrongholdVault::degraded(&conn))))`;
    /// 退出登录调 `set_stronghold_vault(None)` + `vault.lock()`。
    #[cfg(feature = "stronghold")]
    pub fn set_stronghold_vault(&self, vault: Option<Arc<crate::crypto::stronghold::StrongholdVault>>) {
        // 若已有 vault,先 lock()(清零 key material)再替换。单次 lock 获取,避免双锁。
        let mut guard = self.stronghold_vault.lock().unwrap();
        if let Some(old) = guard.take() {
            old.lock();
        }
        *guard = vault;
    }

    /// W9 Plan 1: 返回当前 Stronghold vault 的 Arc 克隆(若有)。
    /// Plan 2 `create_post_commit_compensation` / `reverse_compensation` 用此方法
    /// 判断是否调 vault.encrypt() / vault.decrypt()。
    #[cfg(feature = "stronghold")]
    pub fn stronghold_vault(&self) -> Option<Arc<crate::crypto::stronghold::StrongholdVault>> {
        self.stronghold_vault.lock().unwrap().clone()
    }

    /// W9 Plan 1: Stronghold feature 是否启用 + 配置是否启用。
    /// - feature 关闭(编译时):返回 false
    /// - feature 启用 + app_config.stronghold.enabled 缺失 / "true":返回 true
    /// - feature 启用 + app_config.stronghold.enabled = "false":返回 false(测试用)
    pub fn stronghold_enabled(&self) -> bool {
        #[cfg(feature = "stronghold")]
        {
            let conn = self.conn();
            crate::crypto::stronghold::is_stronghold_enabled_in_config(&conn)
        }
        #[cfg(not(feature = "stronghold"))]
        {
            false
        }
    }
```

- [ ] **Step 4: 验证 stronghold feature 编译 PASS**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 5: 验证 default feature 编译 PASS(`stronghold_enabled` 的 `#[cfg(not)]` 分支)**

Run: `cargo check -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 6: 检查 clippy 0 警告(双 feature)**

Run: `cargo clippy --features stronghold -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

Run: `cargo clippy -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/kernel.rs
git commit -m "feat(w9p1): integrate StrongholdVault into TrustKernel (set/get/stronghold_enabled)"
```

---

## Task 8: privacy_mode 联动(privacy_mode=true 强制 Stronghold 已解锁)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`(本 Task 加 `ensure_stronghold_ready_for_privacy` 方法)

- [ ] **Step 1: 在 kernel.rs 的 Stronghold accessors 之后追加 `ensure_stronghold_ready_for_privacy`**

修改 `voicepilot/crates/trust-kernel/src/kernel.rs`,在 `stronghold_enabled` 方法(上一步追加)之后追加:

```rust
    /// W9 Plan 1: privacy_mode 联动校验(spec §2.1 与 privacy_mode 联动)。
    ///
    /// 调用时机:启动逻辑 / Settings 切换 privacy_mode=true 时 / route_text_with_dag 入口。
    ///
    /// 规则:
    /// - privacy_mode = false:直接返回 Ok(())(允许 Stronghold 降级模式启动)
    /// - privacy_mode = true + stronghold_enabled = false:返回 Err(StrongholdRequired)
    ///   (高隐私模式必须启用 Stronghold,防止 reverse_payload 明文落盘)
    /// - privacy_mode = true + stronghold_enabled = true + vault 未注入:返回 Err(StrongholdRequired)
    /// - privacy_mode = true + stronghold_enabled = true + vault 已注入但未解锁:返回 Err(StrongholdRequired)
    /// - privacy_mode = true + stronghold_enabled = true + vault 已解锁:返回 Ok(())
    ///
    /// 注意:此方法不门控 #[cfg(feature = "stronghold")],因为 privacy_mode 在所有
    /// feature 组合下都存在(W8 Plan 4 实现);feature 关闭时 stronghold_enabled() 恒 false,
    /// privacy_mode=true 必然返回 Err(防绕过)。
    pub fn ensure_stronghold_ready_for_privacy(&self) -> Result<()> {
        if !self.privacy_mode() {
            return Ok(());
        }
        // privacy_mode = true:要求 stronghold_enabled + vault 已注入 + 已解锁
        if !self.stronghold_enabled() {
            return Err(KernelError::StrongholdRequired);
        }
        #[cfg(feature = "stronghold")]
        {
            let vault = self.stronghold_vault()
                .ok_or(KernelError::StrongholdRequired)?;
            if !vault.is_unlocked() {
                return Err(KernelError::StrongholdRequired);
            }
        }
        // feature 关闭时 stronghold_enabled() 已返回 false,不会走到这里
        Ok(())
    }
```

- [ ] **Step 2: 验证 stronghold feature 编译 PASS**

Run: `cargo check --features stronghold -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 3: 验证 default feature 编译 PASS**

Run: `cargo check -p trust-kernel`
Expected: `Finished` 无错误

- [ ] **Step 4: 检查 clippy 0 警告(双 feature)**

Run: `cargo clippy --features stronghold -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

Run: `cargo clippy -p trust-kernel -- -D warnings`
Expected: `Finished` 无警告

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/kernel.rs
git commit -m "feat(w9p1): add ensure_stronghold_ready_for_privacy (privacy_mode forces unlock)"
```

---

## Task 9: 单元测试 w9_stronghold_unit.rs(8 个测试)

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_stronghold_unit.rs`

- [ ] **Step 1: 创建测试文件,文件头加 `#![cfg(feature = "stronghold")]`**

创建 `voicepilot/crates/trust-kernel/tests/w9_stronghold_unit.rs`:

```rust
//! W9 Plan 1: Stronghold 加密单元测试。
//!
//! 8 个测试覆盖 spec §2.1 全部 API + §6.1 安全约束 + 降级模式语义。
//!
//! 运行:cargo test --features stronghold -p trust-kernel --test w9_stronghold_unit

#![cfg(feature = "stronghold")]

use std::sync::Arc;
use trust_kernel::crypto::stronghold::{EncryptedPayload, StrongholdError, StrongholdVault};
use trust_kernel::TrustKernel;
```

- [ ] **Step 2: 加测试 1 — `vault_create_persists_salt_and_path`**

在 `voicepilot/crates/trust-kernel/tests/w9_stronghold_unit.rs` 末尾追加:

```rust
/// 测试 1:create(password) 后,salt 持久化到 app_config.stronghold.salt,
/// vault 文件路径解析为 ${data_dir}/voicepilot/stronghold.bin。
#[test]
fn vault_create_persists_salt_and_path() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    // 用 tempfile 创建临时 data_dir(避免污染真实 %APPDATA%)
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");

    let vault = StrongholdVault::create("test_password_123", &conn).expect("create vault");
    assert!(vault.is_unlocked(), "vault must be unlocked after create");
    assert!(vault_path.exists(), "vault file must be created");

    // salt 持久化在 app_config
    let salt_kv = trust_kernel::repo::config_repo::ConfigRepo::new()
        .get(&conn, "stronghold.salt")
        .expect("get salt");
    assert!(salt_kv.is_some(), "salt must be persisted in KV");
    assert!(!salt_kv.unwrap().is_empty(), "salt must be non-empty");
}
```

- [ ] **Step 3: 加测试 2 — `vault_unlock_with_correct_password_succeeds`**

```rust
/// 测试 2:create(password) → lock() → unlock(password) 成功。
#[test]
fn vault_unlock_with_correct_password_succeeds() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");

    let vault = StrongholdVault::create("correct_password", &conn).expect("create");
    vault.lock();
    assert!(!vault.is_unlocked(), "vault must be locked after lock()");

    // 用同一 vault 实例 unlock(测试场景;生产中 unlock 通常在新进程的 new vault 上调用)
    vault.unlock("correct_password", &conn).expect("unlock");
    assert!(vault.is_unlocked(), "vault must be unlocked after correct password");
}
```

- [ ] **Step 4: 加测试 3 — `vault_unlock_with_wrong_password_returns_error`**

```rust
/// 测试 3:create(password) → lock() → unlock(wrong_password) 返回 WrongPassword。
#[test]
fn vault_unlock_with_wrong_password_returns_error() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");

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
```

- [ ] **Step 5: 加测试 4 — `vault_encrypt_decrypt_roundtrip`**

```rust
/// 测试 4:create → encrypt(plaintext) → decrypt(payload) == plaintext。
#[test]
fn vault_encrypt_decrypt_roundtrip() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");

    let vault = StrongholdVault::create("roundtrip_password", &conn).expect("create");

    let plaintext = b"{\"moves\": [{\"from\": \"a.txt\", \"to\": \"b.txt\"}]}";
    let payload = vault.encrypt(plaintext).expect("encrypt");
    assert!(!payload.ciphertext.is_empty(), "ciphertext must be non-empty");
    assert_eq!(payload.nonce.len(), 24, "XSalsa20Poly1305 nonce = 24 bytes");
    assert_eq!(payload.salt_ref, "stronghold.salt");

    let decrypted = vault.decrypt(&payload).expect("decrypt");
    assert_eq!(decrypted.as_slice(), plaintext, "decrypt must return original plaintext");
}
```

- [ ] **Step 6: 加测试 5 — `vault_degraded_mode_is_unlocked_false`**

```rust
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
```

- [ ] **Step 7: 加测试 6 — `vault_lock_clears_key_material`**

```rust
/// 测试 6:create → lock() → encrypt 返回 NotUnlocked(key material 已清零)。
#[test]
fn vault_lock_clears_key_material() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");

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
```

- [ ] **Step 8: 加测试 7 — `privacy_mode_forces_stronghold_unlocked`**

```rust
/// 测试 7:privacy_mode=true 时,ensure_stronghold_ready_for_privacy 强制要求 vault 已解锁。
/// - privacy_mode=false:Ok
/// - privacy_mode=true + 未注入 vault:Err(StrongholdRequired)
/// - privacy_mode=true + 注入未解锁 vault:Err(StrongholdRequired)
/// - privacy_mode=true + 注入已解锁 vault:Ok
#[test]
fn privacy_mode_forces_stronghold_unlocked() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    // 场景 1:privacy_mode=false → Ok
    assert!(kernel.ensure_stronghold_ready_for_privacy().is_ok());

    // 开启 privacy_mode
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "privacy.mode", "true")
        .expect("set privacy.mode");
    assert!(kernel.privacy_mode(), "privacy_mode must be true");

    // 场景 2:privacy_mode=true + 未注入 vault → Err
    let err = kernel.ensure_stronghold_ready_for_privacy().unwrap_err();
    assert!(
        matches!(err, trust_kernel::error::KernelError::StrongholdRequired),
        "privacy_mode=true without vault must return StrongholdRequired, got: {:?}",
        err
    );

    // 场景 3:privacy_mode=true + 注入未解锁 vault(degraded) → Err
    let degraded_vault = Arc::new(StrongholdVault::degraded(&conn));
    kernel.set_stronghold_vault(Some(degraded_vault.clone()));
    let err = kernel.ensure_stronghold_ready_for_privacy().unwrap_err();
    assert!(
        matches!(err, trust_kernel::error::KernelError::StrongholdRequired),
        "privacy_mode=true with degraded vault must return StrongholdRequired"
    );

    // 场景 4:privacy_mode=true + 注入已解锁 vault → Ok
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");
    let unlocked_vault = Arc::new(
        StrongholdVault::create("privacy_test_password", &conn).expect("create"),
    );
    kernel.set_stronghold_vault(Some(unlocked_vault.clone()));
    assert!(kernel.ensure_stronghold_ready_for_privacy().is_ok());
}
```

- [ ] **Step 9: 加测试 8 — `vault_corrupted_returns_error`**

```rust
/// 测试 8:vault 文件损坏时,unlock 返回 WrongPassword 或 VaultCorrupted。
/// 模拟:创建 vault → 写入垃圾数据到 vault 文件 → unlock 应失败。
#[test]
fn vault_corrupted_returns_error() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let conn = kernel.conn();

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let vault_path = tmp.path().join("stronghold.bin");
    trust_kernel::repo::config_repo::ConfigRepo::new()
        .set(&conn, "stronghold.vault_path", vault_path.to_str().unwrap())
        .expect("set vault_path");

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
```

- [ ] **Step 10: 跑测试,验证全部 PASS(8 个)**

Run: `cargo test --features stronghold -p trust-kernel --test w9_stronghold_unit`
Expected: `8 passed` / `0 failed`

若测试失败,根据失败信息修正 Task 2-8 的实现(注意:Task 9 是 TDD 的"跑红 → 跑绿"阶段,允许先看到红;若实现已正确,应直接绿)

- [ ] **Step 11: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w9_stronghold_unit.rs
git commit -m "test(w9p1): add 8 stronghold unit tests (create/unlock/wrong_pwd/roundtrip/degraded/lock/privacy/corrupted)"
```

---

## Task 10: cargo check --features stronghold + clippy -D warnings + commit

**Files:**
- 无文件改动(本 Task 是验收门禁 + 文档收尾)
- Modify: `docs/PROGRESS.md`(W9 Plan 1 完成状态)

- [x] **Step 1: 跑 6 套 feature 组合 cargo check 矩阵(沿用 W8 模式 + 新增 stronghold 组合)**

Run(windows PowerShell,逐条执行):

```powershell
cargo check --workspace --no-default-features
cargo check --workspace --features llm
cargo check --workspace --features tauri
cargo check --workspace --features voice,tauri
cargo check --workspace --features voice,tauri,llm
cargo check --workspace --features voice,tauri,llm,uia
cargo check --workspace --features voice,tauri,llm,uia,stronghold
cargo check --workspace --features stronghold
```

Expected: 全部 `Finished` 无错误(8 套组合)

若 `--features stronghold` 单独编译失败(stronghold feature 与 default-features=false 组合):
- 检查 `stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2"]` 是否漏写 `default = ["llm"]` 之外的依赖
- 修正后重跑

- [x] **Step 2: 跑 clippy -D warnings(2 套代表性组合)**

Run:

```powershell
cargo clippy --workspace --no-default-features -- -D warnings
cargo clippy --workspace --features voice,tauri,llm,uia,stronghold -- -D warnings
```

Expected: 全部 `Finished` 无警告

若有警告,按 clippy 提示修正(常见:`needless_lifetimes` / `explicit_auto_deref` / `unwrap_used` 在测试外的代码中触发)

- [x] **Step 3: 跑 w9_stronghold_unit 测试 + 全量 trust-kernel 测试无回归**

Run:

```powershell
cargo test --features stronghold -p trust-kernel --test w9_stronghold_unit
cargo test --workspace --features voice,tauri,llm,uia
```

Expected:
- w9_stronghold_unit: `8 passed`
- 全量测试: W1-W8 既有 465 个测试无回归(全部 PASS)

- [x] **Step 4: 跑 default feature 测试无回归**

Run: `cargo test --workspace`
Expected: 全部 PASS(W1-W8 既有测试不受 stronghold feature 关闭影响)

- [x] **Step 5: 更新 docs/PROGRESS.md W9 Plan 1 完成状态**

修改 `docs/PROGRESS.md`,在 W9 段落(若不存在则在 W8 段落后追加)加入:

```markdown
### W9 Plan 1: Stronghold 加密基础 + 密钥管理(2026-07-28)

**状态:** ✅ 完成
**Commit:** <Task 10 Step 6 生成>
**测试:** 8 个 stronghold 单元测试 PASS(需 `--features stronghold`)
**Feature flag:** `stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2"]`(与 voice/tauri/llm 正交)

**新增文件:**
- `crates/trust-kernel/src/crypto/mod.rs`
- `crates/trust-kernel/src/crypto/stronghold.rs`
- `crates/trust-kernel/tests/w9_stronghold_unit.rs`

**修改文件:**
- `voicepilot/Cargo.toml`(workspace 依赖 +2 项)
- `voicepilot/crates/trust-kernel/Cargo.toml`(依赖 +5 项 / feature +1 项)
- `crates/trust-kernel/src/lib.rs`(pub mod crypto;)
- `crates/trust-kernel/src/error.rs`(Stronghold + StrongholdRequired 变体)
- `crates/trust-kernel/src/kernel.rs`(stronghold_vault 字段 + 4 个方法)

**已知偏离:**
- stronghold feature 默认禁用(default = ["llm"] 不含 stronghold);W9 Plan 7 验收矩阵新增 `--features stronghold` / `--features voice,tauri,llm,uia,stronghold` 两套组合
- `tauri-plugin-stronghold` 2.x 在 Windows 上的二进制兼容性已验证(本 Plan Task 1 Step 5 通过)
- Plan 2(create_post_commit_compensation 注入 Stronghold)依赖本 Plan 完成后启动

**Fitness Functions:**
- Stronghold 加密基础:8 个单元测试 PASS ✅
- privacy_mode 联动:测试 7 验证 4 个场景 ✅
- 降级模式:测试 5 + 测试 6 验证 ✅
- clippy -D warnings:2 套 feature 组合 0 警告 ✅
```

- [ ] **Step 6: Commit PROGRESS.md**(跳过 — 用户未显式授权 commit,按 project_memory.md "NEVER commit unless explicitly asked" 规则保留未提交状态,等待用户指示)

```powershell
git add docs/PROGRESS.md
git commit -m "docs(w9p1): update PROGRESS.md with W9 Plan 1 completion status"
```

- [ ] **Step 7: 空标记 commit(可选,标记 W9 Plan 1 里程碑)**(跳过 — 同 Step 6)

```powershell
git commit --allow-empty -m "chore(w9p1): Stronghold vault foundation complete — 8 tests pass"
```

---

## 验收门禁汇总(W9 Plan 1 完成判据)

| 门禁 | 命令 | 期望 |
|---|---|---|
| stronghold feature 编译 | `cargo check --features stronghold -p trust-kernel` | Finished 无错误 |
| default feature 编译 | `cargo check -p trust-kernel` | Finished 无错误 |
| 8 套 feature cargo check 矩阵 | `cargo check --workspace --features <each>` | 全部 Finished |
| clippy stronghold | `cargo clippy --features stronghold -p trust-kernel -- -D warnings` | 0 警告 |
| clippy default | `cargo clippy -p trust-kernel -- -D warnings` | 0 警告 |
| Stronghold 单元测试 | `cargo test --features stronghold -p trust-kernel --test w9_stronghold_unit` | 8 passed |
| W1-W8 测试无回归 | `cargo test --workspace --features voice,tauri,llm,uia` | 465 passed |
| default 测试无回归 | `cargo test --workspace` | 全部 PASS |
| PROGRESS.md 更新 | `git diff docs/PROGRESS.md` | 含 W9 Plan 1 段落 |

---

## Commit Message 格式

本 Plan 所有 commit 严格遵循 `feat(w9p1): ...` / `test(w9p1): ...` / `fix(w9p1): ...` / `docs(w9p1): ...` / `chore(w9p1): ...` 前缀(spec §10 第 8 条)。

各 Task 的 commit message 模板:

| Task | Commit Message |
|---|---|
| Task 1 | `feat(w9p1): add stronghold feature flag + tauri-plugin-stronghold/argon2 deps` |
| Task 2 | `feat(w9p1): add StrongholdVault/StrongholdError/EncryptedPayload data structures` |
| Task 3 | `feat(w9p1): add Argon2id key derivation + config KV storage helpers` |
| Task 4 | `feat(w9p1): implement StrongholdVault create/unlock/lock/is_unlocked/degraded` |
| Task 5 | `feat(w9p1): implement StrongholdVault encrypt/decrypt with XSalsa20Poly1305` |
| Task 6 | `feat(w9p1): add degraded mode entry + stronghold_degraded_mode_entered audit event` |
| Task 7 | `feat(w9p1): integrate StrongholdVault into TrustKernel (set/get/stronghold_enabled)` |
| Task 8 | `feat(w9p1): add ensure_stronghold_ready_for_privacy (privacy_mode forces unlock)` |
| Task 9 | `test(w9p1): add 8 stronghold unit tests (create/unlock/wrong_pwd/roundtrip/degraded/lock/privacy/corrupted)` |
| Task 10 | `docs(w9p1): update PROGRESS.md with W9 Plan 1 completion status` + 空 commit `chore(w9p1): Stronghold vault foundation complete — 8 tests pass` |

**注意:** 单行 commit message(PowerShell 不支持多行 heredoc,用单行);不提交 `.gitignore` / `Cargo.lock`(若 `Cargo.lock` 变更,跟随 Task 1 一起提交)。

---

## 与后续 Plan 的依赖关系

| 下游 Plan | 依赖本 Plan 的部分 | 接口契约 |
|---|---|---|
| Plan 2(snapshot_encrypted 加密) | `StrongholdVault::encrypt/decrypt` + `TrustKernel::stronghold_vault/stronghold_enabled` | `vault.encrypt(&[u8]) -> Result<EncryptedPayload>` / `vault.decrypt(&payload) -> Result<Vec<u8>>` |
| Plan 2(降级模式) | `StrongholdVault::degraded` + `StrongholdVault::is_unlocked` + `TrustKernel::stronghold_enter_degraded_mode` | `degraded(&Connection) -> Self` / `is_unlocked() -> bool` |
| Plan 5(真实 Playwright E2E) | `TrustKernel::set_stronghold_vault(Some(Arc::new(StrongholdVault::create(password, &conn))))` | 测试 setup 调用 |
| Plan 6(真实 UIA E2E) | 同上 | 同上 |
| Plan 7(集成验收) | `cargo check --features stronghold` 在 7 套 feature 矩阵中 | 编译验证 |

**接口稳定性保证:** 本 Plan 定义的所有 public API 在 W9 后续 Plan 中不再变更签名(若需变更,先 brainstorm + 回更新本 Plan + spec)。

---

## 风险与缓解

| 风险 | 可能性 | 影响 | 缓解 |
|---|---|---|---|
| `tauri-plugin-stronghold` 2.x API 与本 Plan 假设不符 | 中 | 中 | Task 4/5 Step 1 注释标注 "API 形式可能因版本而异",实现时查文档调整;若 API 完全不同,brainstorm 替代 |
| Argon2id 64MB 内存在低端 Windows 设备上 OOM | 低 | 中 | spec §6.1 第 1 条固定参数,不降级;若实测 OOM,记录 PROGRESS.md 已知偏离,延后 W10+ 优化 |
| `cargo check --features stronghold` 在 Windows 上失败(二进制兼容性) | 中 | 高 | spec §4 风险登记已记录;Task 1 Step 5 早期检测;若失败,降级为 `ring` + `aes-gcm`(需 brainstorm) |
| 测试 8(vault_corrupted)在 Stronghold 真实实现下行为不一致 | 中 | 低 | Task 9 Step 9 断言用 `matches!(err, WrongPassword | VaultCorrupted(_))` 兼容两种返回 |
| `set_stronghold_vault` 在已有 vault 时未正确 lock 旧 vault | 低 | 高 | Task 7 Step 3 实现已包含 `if let Some(old) = ... { old.lock(); }`;Task 9 测试 6 验证 lock 后 encrypt 返回 NotUnlocked |
| privacy_mode 测试场景 3(degraded vault)在 feature 关闭时编译失败 | 中 | 中 | 测试文件整体 `#![cfg(feature = "stronghold")]` 门控(Task 9 Step 1),feature 关闭时测试不编译 |

---

**End of W9 Plan 1 Implementation Plan**

---

## W9 审查修复记录

本段落由 W9 Plan 1 审查阶段追加,记录所有修复项。修复时使用 Edit 工具精确替换,未重写整个文件。

### P0-1: audit_logs.task_id FK 违约修复

**位置:** Task 6 Step 1(`stronghold_enter_degraded_mode` 函数)

**问题:** 原代码用字面量 `"unknown-task"` 调 `audit_append` / `audit_append_external`,但 `audit_logs.task_id` 是 FK REFERENCES `tasks(task_id)`(`migrations/001_init.sql:77`),`"unknown-task"` 不在 tasks 表中,必触发 FK 约束失败。

**修复:**
- 在写审计前先用 `TaskRepo::new().create(&conn, &placeholder)` 创建占位 task 行,使 FK 满足。
- 占位 task_id 用 `format!("stronghold-degraded-{}", uuid::Uuid::new_v4())` 保证唯一性。
- 用 block scope `{ let conn = self.conn(); ... }` 限制 MutexGuard 生命周期,避免 `audit_append` 二次加锁死锁。
- 因 `TaskRepo::create` 实际签名为 `create(&self, conn: &Connection, task: &TaskRecord) -> Result<()>`(核实 `voicepilot/crates/trust-kernel/src/repo/task_repo.rs:45`),需用 `TaskRecord::new(task_id, user_goal)` 构造记录而非 4 个字符串参数。
- 同步更新方法文档注释,说明占位 task 的必要性。

### P0-2: unlock 把"vault 文件不存在"误判为 WrongPassword 修复

**位置:** Task 4 Step 1(`unlock` 方法)

**问题:** `if !vault_path.exists() { return Err(StrongholdError::WrongPassword); }` 把首次启动(vault 文件未创建)与密码错混淆,调用方无法区分"应调 create()"还是"提示用户重输密码"。

**修复:**
- 在 `StrongholdError` 枚举新增 `VaultNotFound { path: String }` 变体(`#[error("stronghold vault file not found at {path}")]`)。
- `unlock` 方法把 `return Err(StrongholdError::WrongPassword)` 改为 `return Err(StrongholdError::VaultNotFound { path: vault_path.to_string_lossy().to_string() })`。
- 同步更新方法内注释,区分"文件不存在 → VaultNotFound"与"key 不匹配 → WrongPassword"。

### P0-3: create 在 vault 文件已存在时返回 VaultCorrupted(变体误用)修复

**位置:** Task 4 Step 1(`create` 方法)

**问题:** `if vault_path.exists() { return Err(StrongholdError::VaultCorrupted(...)); }` 把"已存在"当成"损坏",错误类型语义错误,调用方据错误类型做决策时会误判。

**修复:**
- 在 `StrongholdError` 枚举新增 `AlreadyExists { path: String }` 变体(`#[error("stronghold vault file already exists at {path}")]`)。
- `create` 方法把 `return Err(StrongholdError::VaultCorrupted(...))` 改为 `return Err(StrongholdError::AlreadyExists { path: vault_path.to_string_lossy().to_string() })`。
- 同步更新方法文档注释,把"返回 WrongPassword"改为"返回 AlreadyExists"。

### P1-1: zeroize 未真正应用到 derived_key 修复

**位置:** Task 4 Step 1(`create` / `unlock` / `lock` 方法)+ Cargo.toml(feature 定义)

**问题:**
1. `drop(derived_key)` 不保证内存清零(Rust 编译器可能复用栈槽,key 残留)。
2. `lock()` 只 `take()` 释放 Stronghold 句柄,未对 derived_key 调 zeroize(虽然 derived_key 是 create/unlock 的局部变量,但原 Plan 注释声称 "lock() 时 zeroize key material",实际未做到)。

**修复:**
1. `trust-kernel/Cargo.toml` 中 `zeroize` 改为 `optional = true`,加入 `stronghold` feature:`stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2", "dep:zeroize"]`(同步更新 File Structure 段 + Task 1 Step 2 + Step 3 的 TOML 片段)。
2. `stronghold.rs` imports 加 `use zeroize::Zeroizing;`。
3. `derive_key_argon2id` 返回类型从 `[u8; ARGON2_OUTPUT_LEN]` 改为 `Zeroizing<[u8; ARGON2_OUTPUT_LEN]>`,内部 `let mut out = Zeroizing::new([0u8; ARGON2_OUTPUT_LEN]);` + `hash_password_into(..., &mut *out)`。
4. `create` / `unlock` 中 `derived_key.to_vec()` 不变(Zeroizing 的 Deref 自动解引用),`drop(derived_key)` 显式提前触发 Drop 清零(早于函数返回)。
5. `lock()` 方法末尾加注释说明:derived_key 已用 Zeroizing 包装,Drop 时自动清零;Stronghold 句柄 take() 释放;lock() 调用时 derived_key 早已超出作用域被清零,无需重复操作。

### P1-2: error.rs / lib.rs 行号描述偏差修复

**位置:** Task 2 Step 2(lib.rs)+ Task 2 Step 3(error.rs)

**问题:**
- 原描述 "在 lib.rs 第 22 行(`pub mod skills;` 后)插入",但经核实 lib.rs 第 21 行是 `pub mod skills;`,第 22 行是 `pub mod llm;`,描述与实际不符。
- 原描述 "error.rs 第 46 行",经核实 error.rs 共 48 行,`Uia(String)` 在第 46 行,描述准确但依赖行号,代码变动后易失效。

**修复:**
- lib.rs 描述改为:"在 `pub mod llm;` 后追加 `pub mod crypto;`(不依赖行号)"。
- error.rs 描述改为:"在 `Uia(String)` 变体后追加(不依赖行号)"。

### P1-3: tauri-plugin-stronghold 2.x API 假设未验证修复

**位置:** Task 1 Step 5 之后(新增 Step 5.5)

**问题:** Plan 自己在 Task 4/5 注释 "API 形式可能因版本而异" 但未给验证步骤,若 API 假设错误,Task 4/5 写完后才发现编译失败,造成大量返工。

**修复:**
- 在 Task 1 Step 5 之后追加 Task 1 Step 5.5 "Smoke 测试验证 Stronghold API 签名"。
- 新建临时 smoke 测试 `voicepilot/crates/trust-kernel/tests/w9_stronghold_api_smoke.rs`,验证 `Stronghold::new(&vault_path, derived_key.to_vec(), None)` + `stronghold.encrypt(b"plaintext", &nonce)` 真实签名。
- 若编译失败,提供两个降级选项:查文档调整 API 调用 / 改用 `ring` + `aes-gcm` 自实现。
- 同步更新 Task 1 Step 6 的 git add 命令包含 smoke 测试文件,commit message 加 "+ API smoke test"。

### P1-4: degraded() 在 vault_path 解析失败时硬编码相对路径修复

**位置:** Task 4 Step 1(`degraded` 方法)

**问题:** `PathBuf::from("stronghold.bin")` 是相对路径,依赖 cwd。测试 / Tauri runtime / CLI 启动时 cwd 不一致,可能写到不可预测的位置(如仓库根目录 / Tauri 资源目录)。

**修复:**
- 改为 `PathBuf::from(std::env::temp_dir().join("voicepilot-stronghold-degraded.bin"))`,使用系统 temp 目录的绝对路径。
- 加注释说明原相对路径的问题。

### P2-1: Plan 1 Precondition 未提 compensation_level 列修复

**位置:** Precondition 段落末尾(原最后一行 `cargo check --workspace --features voice,tauri,llm,uia` 之后)

**问题:** `compensations` 表除 Plan 1 已提及的 `snapshot_encrypted` + `snapshot_vault_ref` 外,还有 `compensation_level TEXT` 列(`migrations/001_init.sql:70`,V1.1 alias),Plan 2 修改 `CompensationRepo::create` 时若忽视此列可能漏写。

**修复:**
- 在 Precondition 段落末尾追加注释:"注意:`compensations` 表还有 `compensation_level TEXT` 列(`migrations/001_init.sql:70`,V1.1 alias),Plan 2 修改 `CompensationRepo::create` 时需注意该列存在(本 Plan 不涉及 CompensationRepo 改动)。"
