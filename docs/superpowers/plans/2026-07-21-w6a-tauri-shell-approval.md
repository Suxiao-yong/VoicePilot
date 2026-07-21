# VoicePilot W6a: Tauri UI Shell + Approval 窗口实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标:** 为 VoicePilot 增加 Tauri 2 桌面 UI shell——通过 Tauri commands 桥接 `trust-kernel`,实现 `TauriApprover`(基于 IPC 的 Approver trait 实现),构建 Approval 窗口(React + TypeScript)展示 `EffectManifest` 并收集 y/N 决定,最后用端到端冒烟测试验证全链路。W6b 将补充 Main Chat、Settings、Audit Viewer、Trust Center 四个窗口。

**架构:** 新增 `voicepilot/crates/ui` crate(Tauri v2 应用,通过 `tauri` workspace feature 开启)。Tauri Rust 后端依赖 `trust-kernel`,暴露类型化的 Tauri commands。前端位于 `crates/ui/web/`(Vite + React + TypeScript)。Approval 窗口使用 Tauri 事件(`tauri::Emitter`/`tauri::Listener`)桥接同步的 `Approver::prompt` 调用(Rust 侧)与异步的 React 审批模态框(webview 侧)。基于 channel 的同步机制(`tokio::sync::oneshot`)确保 prepare→approve→commit 管道在用户点击 y/N 之前保持阻塞。

**Spec 对齐(V1.1.2 §8.2 + §8.3):**
- §8.2 窗口拆分:W6a 实现 **Main**(最小化——仅一个 "organize" 按钮 + 状态展示)+ **Approval**(完整 effect_manifest + Diff Preview 占位 + y/N 按钮)。Audit/Settings/Trust Center 延后到 W6b。
- §8.2 IPC 硬化红线强制执行:
  - WebView 不直接访问文件系统(所有 FS 操作通过 `FilesystemTool` Rust 适配器)
  - UI 不能直接调用 MCP(所有调用通过 `ActionGateway`/`FilesOrganizeSkill`)
  - Approval 窗口只接受一次性 `approval_request_id`(每次 prompt 生成 UUID v4,决定后消费)
- §8.3 Approval Modal 功能(W6a 范围):effect_manifest 展示、风险等级(E×D)、可补偿性、y/N 按钮。**Diff Preview 延后到 W6b**(需要文件内容读取器,不在 W6a 最小可行 shell 范围内)。

**Feature 门控(opt-in,与 W5 voice 决策一致):**
- `default = []` —— 纯 Rust,无 Tauri 依赖,W1-W5 测试 + CLI 仍正常工作(196 + voice opt-in 测试)
- `tauri = ["dep:tauri", "dep:tauri-build", "dep:tokio", "trust-kernel/voice"]` —— opt-in,开启 UI crate
- `ui` crate **从默认 workspace members 中排除**,通过 `default-members = ["crates/trust-kernel", "crates/cli"]` 实现,这样 `cargo test`(默认)永远不会触碰 Tauri
- 所有 Tauri commands 用 `#[cfg(feature = "voice")]` 门控依赖 voice 的操作(voice listen command 等);纯 IPC commands(route_text、organize_files)无需 voice feature 即可工作

**技术栈:**
- Tauri 2.x(稳定版,2024 年 10 月发布)
- React 18 + TypeScript 5
- Vite 5(构建工具,HMR)
- `@tauri-apps/api` 2.x(前端 IPC 绑定)
- `@tauri-apps/plugin-shell`(可选,延后——不在 W6a)
- `tokio` 1.x(Rust 异步运行时,用于 oneshot channel)
- `tauri::Manager` + `tauri::Emitter` + `tauri::Listener`(事件系统)

**构建前提条件(仅当使用 `--features tauri` 时):**
- Node 22+(已验证:v22.16.0)
- npm 10+(已验证:10.9.4)
- Rust 1.96+(已验证:cargo 1.96.1)
- WebView2 Runtime(Windows 11 预装;Windows 10 22H2 可能需要手动安装)
- **如果 Node 不可用:** Tauri 代码仍可编写并提交;用 `cargo check`(默认,无 tauri)验证 W1-W5 无回归。Tauri 特定的编译/测试需要 Node + 在 `crates/ui/web/` 中执行 npm install。

**不在范围内(延后到 W6b):**
- Main Chat 窗口(voice 输入按钮、实时转写、route 结果反馈)
- Settings 面板(Whisper 模型路径配置、allowed_paths 编辑器、麦克风设备选择、VAD 阈值)
- Audit Viewer(只读 audit_logs 查询 + 展示)
- Trust Center(MCP server 列表、egress 策略、一键禁用)
- Skills Manager(已保存 Skills 列表、成功率、延迟)
- Approval Modal 中的 Diff Preview(需要文件内容读取器)
- Kill Switch Bar(常驻顶栏)
- 基于 VAD 的自动停止(替换 W5 PoC 的 5s 超时,issue #45)
- 模型自动下载(issue #46)
- Tauri 打包(NSIS 安装包、代码签名——W7+)
- 流式部分转写(issue #47)
- 唤醒词检测(issue #48)

---

## 文件结构

### 新增文件

| 文件 | 职责 |
|---|---|
| `voicepilot/crates/ui/Cargo.toml` | Tauri app crate 清单,`tauri` feature 门控,依赖 `trust-kernel` + `tauri` + `tokio` |
| `voicepilot/crates/ui/build.rs` | Tauri 构建脚本(调用 `tauri_build::build()`) |
| `voicepilot/crates/ui/tauri.conf.json` | Tauri 配置(productName、窗口列表、安全 CSP、打包设置) |
| `voicepilot/crates/ui/src/main.rs` | Tauri app 入口(`tauri::Builder` + command 注册 + 插件设置) |
| `voicepilot/crates/ui/src/lib.rs` | 模块声明 + 测试用 re-exports |
| `voicepilot/crates/ui/src/approver.rs` | `TauriApprover` —— 实现 `Approver` trait,通过事件 + oneshot channel 桥接到 webview |
| `voicepilot/crates/ui/src/commands.rs` | Tauri `#[command]` 函数:`route_text`、`organize_files`、`list_voice_models`、`submit_approval` |
| `voicepilot/crates/ui/src/state.rs` | `AppState` —— 持有 `Arc<TrustKernel>` + 待处理 approval 请求注册表 |
| `voicepilot/crates/ui/src/error.rs` | `UiError` 枚举 → Tauri `Result<T, String>` 序列化 |
| `voicepilot/crates/ui/tests/approver_unit.rs` | `TauriApprover` 单元测试(模拟事件发射,无真实 webview) |
| `voicepilot/crates/ui/tests/commands_unit.rs` | Tauri commands 单元测试(route_text、organize_files) |
| `voicepilot/crates/ui/tests/w6a_e2e_smoke.rs` | 端到端冒烟测试:注入 `AutoApprover` 调用 `organize_files` command,验证审计链 |
| `voicepilot/crates/ui/web/package.json` | npm 依赖:React、TypeScript、Vite、@tauri-apps/api |
| `voicepilot/crates/ui/web/vite.config.ts` | Vite 配置(Tauri 友好的 base + port) |
| `voicepilot/crates/ui/web/tsconfig.json` | TypeScript 严格模式配置 |
| `voicepilot/crates/ui/web/index.html` | Vite 入口 HTML |
| `voicepilot/crates/ui/web/src/main.tsx` | React app 入口 |
| `voicepilot/crates/ui/web/src/App.tsx` | 根组件(tab 切换:Main / Approval) |
| `voicepilot/crates/ui/web/src/components/MainView.tsx` | 最小化 Main 窗口(organize 表单 + 状态展示) |
| `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx` | Approval 窗口(EffectManifest 展示 + y/N 按钮 + 提交) |
| `voicepilot/crates/ui/web/src/api.ts` | Tauri `invoke` 包装 + 事件监听 |
| `voicepilot/crates/ui/web/src/types.ts` | TypeScript 类型镜像 Rust DTO(EffectManifest、RouteOutcome、ApprovalDecision) |

### 修改的文件

| 文件 | 变更 |
|---|---|
| `voicepilot/Cargo.toml` | 添加 `tauri`、`tauri-build`、`tokio` 到 workspace deps;添加 `default-members` 排除 `crates/ui`;添加 `ui` 到 members 列表 |
| `voicepilot/crates/trust-kernel/Cargo.toml` | 无变更(已通过 `kernel.rs` 暴露所需 API) |
| `voicepilot/crates/trust-kernel/src/lib.rs` | 无变更 |
| `voicepilot/crates/cli/Cargo.toml` | 无变更(CLI 仍是独立的 headless 入口) |
| `voicepilot/crates/trust-kernel/src/approval/approver.rs` | 在 doc comment 中添加 `Send + Sync` bound 说明(已存在,仅文档化原因) |

---

## 任务 1:添加 `ui` crate 到 workspace + Tauri feature 门控

**文件:**
- 修改:`voicepilot/Cargo.toml`
- 创建:`voicepilot/crates/ui/Cargo.toml`
- 创建:`voicepilot/crates/ui/build.rs`
- 创建:`voicepilot/crates/ui/src/lib.rs`

- [ ] **步骤 1:更新根 Cargo.toml —— 添加 `default-members` + 新 workspace deps + `ui` member**

编辑 `voicepilot/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/trust-kernel",
    "crates/cli",
    "crates/ui",
]
default-members = ["crates/trust-kernel", "crates/cli"]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.96"
authors = ["VoicePilot Team"]
license = "MIT"

[workspace.dependencies]
trust-kernel = { path = "crates/trust-kernel" }
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
uuid = { version = "1.10", features = ["v4", "serde"] }
sha2 = "0.10"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1.0"
cedar-policy = "4.11.2"
walkdir = "2.5"
whisper-rs = { version = "0.13" }
cpal = { version = "0.15" }
hound = { version = "3.5" }
tauri = { version = "2", features = ["wry"] }
tauri-build = { version = "2" }
tokio = { version = "1", features = ["sync", "rt", "macros"] }
```

- [ ] **步骤 2:创建 `voicepilot/crates/ui/Cargo.toml`**

```toml
[package]
name = "voicepilot-ui"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true

[lib]
name = "voicepilot_ui"
path = "src/lib.rs"

[[bin]]
name = "voicepilot-ui"
path = "src/main.rs"
required-features = ["tauri"]

[build-dependencies]
tauri-build = { workspace = true, optional = true }

[dependencies]
trust-kernel = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true, optional = true }
tauri = { workspace = true, optional = true }

[features]
default = []
tauri = ["dep:tauri", "dep:tauri-build", "dep:tokio", "trust-kernel/voice"]
```

- [ ] **步骤 3:创建 `voicepilot/crates/ui/build.rs`**

```rust
fn main() {
    #[cfg(feature = "tauri")]
    tauri_build::build()
}
```

- [ ] **步骤 4:创建 `voicepilot/crates/ui/src/lib.rs`(骨架)**

```rust
//! VoicePilot UI crate —— Tauri 2 桌面应用。
//!
//! W6a 范围:
//! - Tauri command 桥接到 `trust-kernel`
//! - `TauriApprover` 实现(基于 IPC 的 Approver)
//! - Approval 窗口(React + TypeScript)
//! - 端到端冒烟测试
//!
//! Feature 门控:`default = []` 保持 crate 纯 Rust(无 Tauri 也能编译)。
//! `tauri` feature 开启桌面 app 二进制 + voice feature。

pub mod error;
pub mod state;

#[cfg(feature = "tauri")]
pub mod approver;

#[cfg(feature = "tauri")]
pub mod commands;

#[cfg(feature = "tauri")]
pub mod app;

pub use error::UiError;
pub use state::AppState;
```

- [ ] **步骤 5:创建空的 `voicepilot/crates/ui/src/error.rs` + `state.rs` 骨架**

`error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UiError {
    #[error("kernel error: {0}")]
    Kernel(#[from] trust_kernel::error::KernelError),
    #[error("tauri error: {0}")]
    Tauri(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("approval timed out")]
    ApprovalTimeout,
    #[error("approval request not found: {0}")]
    ApprovalNotFound(String),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl From<UiError> for String {
    fn from(e: UiError) -> String {
        e.to_string()
    }
}

pub type UiResult<T> = Result<T, UiError>;
```

`state.rs`:
```rust
use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;

pub struct AppState {
    pub kernel: Arc<TrustKernel>,
}

impl AppState {
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
        }
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory()?;
        Ok(Self::new(kernel))
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_file(path)?;
        Ok(Self::new(kernel))
    }
}
```

- [ ] **步骤 6:验证默认 workspace 仍可编译**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml
# 期望:无错误,无警告;ui crate 在 members 中但只有 stub lib.rs
cargo test --manifest-path voicepilot\Cargo.toml
# 期望:196 个通过(W1-W4)+ voice opt-in 测试仍跳过
```

- [ ] **步骤 7:验证 Tauri feature 可编译**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# 期望:可编译(Tauri deps 已拉取,但还没有 commands —— 只有 state + error 模块)
```

- [ ] **步骤 8:提交**

```powershell
git add voicepilot/Cargo.toml voicepilot/crates/ui/
git commit -m "Task 1: ui crate scaffolding with tauri feature gate (V1.1 §8.2)"
```

---

## 任务 2:`TauriApprover` 骨架 —— 基于 channel 的 Approver 实现

**文件:**
- 创建:`voicepilot/crates/ui/src/approver.rs`
- 创建:`voicepilot/crates/ui/tests/approver_unit.rs`

**设计:**
`Approver` trait 是同步的(`fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision`)。Tauri 事件是异步的。桥接方式:
1. `TauriApprover` 持有一个 `AppHandle`(克隆,成本低——内部是 `Arc`)
2. 在 `prompt()` 中:
   - 生成 `approval_request_id`(UUID v4)
   - 创建 `tokio::sync::oneshot::channel::<ApprovalDecision>()`
   - 将 `Sender` 存入 `AppState` 上的 `Mutex<HashMap<String, Sender>>`
   - 向 webview 发射 `approval-request` 事件,携带 `{approval_request_id, manifest}`
   - 阻塞在 `Receiver::blocking_recv()` 上,带超时(默认 300s,可配置)
   - 超时 → 返回 `Deny`(更安全的默认值,与 CliApprover 约定一致)
   - 收到 → 返回决定
3. webview 的 "Approve"/"Deny" 按钮调用 `submit_approval` Tauri command,后者查找 `Sender` 并发送决定。

- [ ] **步骤 1:先写测试(red)**

`voicepilot/crates/ui/tests/approver_unit.rs`:

```rust
#![cfg(feature = "tauri")]

use std::time::Duration;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::transaction::EffectManifest;
use voicepilot_ui::approver::ApprovalRegistry;

fn dummy_manifest() -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: "D:/test/dest".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    }
}

#[test]
fn approval_registry_resolves_submitted_decision() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (approval_id, rx) = registry.create_request(&manifest);

    // 模拟 webview submit_approval command:取出 sender 并发送决定
    let sender = registry.take_sender(&approval_id).expect("sender exists");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = sender.send(ApprovalDecision::Allow);
    });

    let decision = registry.wait_for_decision(rx, Duration::from_secs(5));
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn approval_registry_times_out_to_deny() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (_approval_id, rx) = registry.create_request(&manifest);

    let decision = registry.wait_for_decision(rx, Duration::from_millis(100));
    assert_eq!(decision, ApprovalDecision::Deny);
}

#[test]
fn approval_registry_consumes_request_after_take() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (approval_id, _rx) = registry.create_request(&manifest);

    let _ = registry.take_sender(&approval_id).expect("first take succeeds");

    // 第二次 take 应该失败(§8.2 一次性)
    assert!(registry.take_sender(&approval_id).is_none());
}

#[test]
fn approval_registry_handles_sender_dropped() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let (approval_id, rx) = registry.create_request(&manifest);

    // 取出 sender 但从不发送——直接 drop
    let _sender = registry.take_sender(&approval_id).expect("exists");
    // _sender 在这里被 drop

    let decision = registry.wait_for_decision(rx, Duration::from_secs(1));
    assert_eq!(decision, ApprovalDecision::Deny);
}
```

- [ ] **步骤 2:运行测试(red)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test approver_unit
# 期望:编译错误 —— ApprovalRegistry 尚不存在
```

- [ ] **步骤 3:实现 `ApprovalRegistry` + `TauriApprover`(green)**

设计说明:
- `ApprovalRegistry` 用 `approval_request_id` 作 key 存储 `oneshot::Sender<ApprovalDecision>`。
- `create_request` 返回 `(approval_id, receiver)`,这样调用方(TauriApprover::prompt)持有 receiver 并阻塞在其上。`take_sender` 方法(由 `submit_approval` Tauri command 调用)从 map 中移除 sender 并返回用于投递——一次性,符合 §8.2。
- `wait_for_decision` 使用专用的 current-thread tokio runtime 阻塞 receiver 并带超时(避免在已存在的 Tauri async 上下文中调用时死锁)。超时或 sender 被丢弃时:返回 `Deny`(更安全的默认值,与 CliApprover 约定一致)。

`voicepilot/crates/ui/src/approver.rs`:

```rust
//! TauriApprover —— 将同步的 `Approver::prompt` 桥接到异步的 Tauri 事件。
//!
//! V1.1 §8.2 + §6.2:Approval 窗口必须接受一次性 approval_request_id
//! 并在决定后消费。本模块实现持有待处理 approval senders 的注册表
//! + 阻塞在 recv 上的 Approver trait 实现。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use trust_kernel::approval::approver::Approver;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::transaction::EffectManifest;
use uuid::Uuid;

const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct ApprovalRegistry {
    senders: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建新的待处理 approval 请求。
    /// 返回 (approval_request_id, receiver) —— 调用方阻塞在 receiver 上。
    /// 传入 manifest 以便生产环境的 TauriApprover 能将其与 approval_request_id
    /// 一起发射到 webview。
    pub fn create_request(
        &self,
        _manifest: &EffectManifest,
    ) -> (String, oneshot::Receiver<ApprovalDecision>) {
        let approval_id = format!("apr_{}", Uuid::new_v4());
        let (tx, rx) = oneshot::channel::<ApprovalDecision>();
        self.senders
            .lock()
            .unwrap()
            .insert(approval_id.clone(), tx);
        (approval_id, rx)
    }

    /// 查找并移除给定 approval_request_id 的 sender。
    /// 由 `submit_approval` Tauri command 调用。
    /// 如果请求已被消费或已过期,返回 None。
    pub fn take_sender(&self, approval_id: &str) -> Option<oneshot::Sender<ApprovalDecision>> {
        self.senders.lock().unwrap().remove(approval_id)
    }

    /// 阻塞直到决定到达或超时。
    /// 超时或 sender 被丢弃时:返回 Deny(更安全的默认值)。
    pub fn wait_for_decision(
        &self,
        rx: oneshot::Receiver<ApprovalDecision>,
        timeout: Duration,
    ) -> ApprovalDecision {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(decision)) => decision,
                Ok(Err(_)) => ApprovalDecision::Deny, // sender dropped
                Err(_) => ApprovalDecision::Deny,     // timeout
            }
        })
    }
}

impl Default for ApprovalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TauriApprover {
    registry: ApprovalRegistry,
}

impl TauriApprover {
    pub fn new(registry: ApprovalRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &ApprovalRegistry {
        &self.registry
    }
}

impl Approver for TauriApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        let (approval_id, rx) = self.registry.create_request(manifest);
        // 生产环境(任务 6):在此向 webview 发射 "approval-request" 事件。
        // 单元测试:调用方直接调用 `registry.take_sender(id).send(decision)`。
        let _ = approval_id; // 由 TauriApprover::prompt 在任务 6 中发射
        self.registry.wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }
}
```

- [ ] **步骤 4:运行测试(green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test approver_unit
# 期望:4 个通过
```

- [ ] **步骤 5:提交**

```powershell
git add voicepilot/crates/ui/src/approver.rs voicepilot/crates/ui/tests/approver_unit.rs
git commit -m "Task 2: TauriApprover with oneshot channel + 5min timeout (V1.1 §8.2 one-shot approval_request_id)"
```

---

## 任务 3:Tauri command `route_text` —— 桥接到 SkillRouter

**文件:**
- 创建:`voicepilot/crates/ui/src/commands.rs`
- 创建:`voicepilot/crates/ui/tests/commands_unit.rs`

- [ ] **步骤 1:先写测试(red)**

`voicepilot/crates/ui/tests/commands_unit.rs`:

```rust
#![cfg(feature = "tauri")]

use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::state::AppState;

#[test]
fn route_text_returns_routed_when_skill_keyword_matches() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "organize my downloads").unwrap();
    assert!(matches!(result, RouteTextResult::Routed { ref skill_id } if skill_id == "files.organize"));
}

#[test]
fn route_text_returns_unmatched_when_no_keyword() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "hello world").unwrap();
    assert!(matches!(result, RouteTextResult::Unmatched { .. }));
}

#[test]
fn route_text_returns_empty_for_whitespace() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "   ").unwrap();
    assert!(matches!(result, RouteTextResult::Empty));
}
```

- [ ] **步骤 2:运行测试(red)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# 期望:编译错误 —— commands 模块 + route_text 不存在
```

- [ ] **步骤 3:实现 `commands.rs`(green)**

```rust
//! Tauri commands —— V1.1 §8.2 webview 与 trust-kernel 之间的 IPC 桥接。
//!
//! 所有 commands 都用 `#[cfg(feature = "tauri")]` 门控。它们接收 `&AppState`
//! (由 Tauri 管理)并返回 `Result<T, String>` 供 webview 消费。

use serde::{Deserialize, Serialize};
use trust_kernel::skills::router::RouteDecision;
use trust_kernel::skills::manifest::files_organize_manifest;
use crate::error::{UiError, UiResult};
use crate::state::AppState;

/// 镜像 `trust_kernel::voice::router_bridge::RouteOutcome`,但带 Serialize
/// 作为 Tauri command 返回类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteTextResult {
    Routed { skill_id: String },
    Unmatched { text: String },
    Empty,
}

/// 通过 SkillRouter 路由转写文本(或任意文本输入)。
/// V1.1 §5.1 —— 纯关键词匹配(W7 将添加 LLM Planner fallback)。
pub fn route_text(state: &AppState, text: &str) -> UiResult<RouteTextResult> {
    use trust_kernel::skills::router::SkillRouter;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteTextResult::Empty);
    }
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteTextResult::Routed {
            skill_id: manifest.id,
        }),
        RouteDecision::Planner => Ok(RouteTextResult::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn route_text_command(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<RouteTextResult, String> {
    route_text(&state, &text).map_err(Into::into)
}
```

- [ ] **步骤 4:更新 `lib.rs` 导出 commands 模块**

已在任务 1 中通过 `pub mod commands;` 导出。

- [ ] **步骤 5:运行测试(green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# 期望:3 个通过
```

- [ ] **步骤 6:提交**

```powershell
git add voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/commands_unit.rs
git commit -m "Task 3: route_text Tauri command bridges SkillRouter (V1.1 §5.1, §8.2)"
```

---

## 任务 4:Tauri command `organize_files` —— 完整 Skill 管道 + TauriApprover

**文件:**
- 修改:`voicepilot/crates/ui/src/commands.rs`
- 修改:`voicepilot/crates/ui/src/state.rs`
- 修改:`voicepilot/crates/ui/tests/commands_unit.rs`

**设计:**
- `AppState` 新增 `approval_registry: ApprovalRegistry` 字段
- `organize_files` command:
  1. 在 kernel 中创建 task + step
  2. 构建 `FilesOrganizeInput`
  3. 从 registry 构造 `TauriApprover`
  4. 调用 `FilesOrganizeSkill::execute(kernel, input, approver)`
  5. 返回 `OrganizeResult { tool_result, moved_paths }` 给 webview

- [ ] **步骤 1:先写测试(red)**

追加到 `commands_unit.rs`:

```rust
use std::path::PathBuf;
use tempfile::TempDir;
use trust_kernel::approval::approver::AutoApprover;
use voicepilot_ui::commands::{organize_files, OrganizeInput, OrganizeResult};

#[test]
fn organize_files_with_auto_approver_commits_move() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    let dest_dir = tmp.path().join("dest");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();
    std::fs::write(src_dir.join("a.txt"), "hello").unwrap();

    let state = AppState::new_in_memory().unwrap();
    let result = organize_files(
        &state,
        &AutoApprover,
        &OrganizeInput {
            task_id: "t-test".to_string(),
            step_id: "s-test".to_string(),
            source: src_dir.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest_dir.to_string_lossy().into_owned(),
        },
    ).unwrap();

    assert!(result.committed);
    assert_eq!(result.moved_paths.len(), 1);
    assert!(dest_dir.join("a.txt").exists());
}
```

- [ ] **步骤 2:运行测试(red)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# 期望:编译错误 —— organize_files 不存在
```

- [ ] **步骤 3:实现 `organize_files`(green)**

添加到 `commands.rs`:

```rust
use std::path::PathBuf;
use trust_kernel::approval::approver::Approver;
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::repo::task_repo::TaskRecord;
use trust_kernel::repo::step_repo::StepRecord;
use trust_kernel::state::TaskState;

#[derive(Debug, Clone, Deserialize)]
pub struct OrganizeInput {
    pub task_id: String,
    pub step_id: String,
    pub source: String,
    pub filter: String,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrganizeResult {
    pub committed: bool,
    pub moved_paths: Vec<[String; 2]>, // [[from, to], ...]
    pub evidence_strength: String,
    pub compensation_ref: Option<String>,
    pub error: Option<String>,
}

pub fn organize_files(
    state: &AppState,
    approver: &dyn Approver,
    input: &OrganizeInput,
) -> UiResult<OrganizeResult> {
    // 如果不存在则创建 task + step(幂等,支持重试)
    if state.kernel.get_task(&input.task_id)?.is_none() {
        state.kernel.create_task(&input.task_id, "organize files")?;
    }
    if state.kernel.get_step(&input.step_id)?.is_none() {
        state.kernel.create_step(&StepRecord::new(
            &input.step_id,
            &input.task_id,
            1,
        ))?;
    }

    let skill_input = FilesOrganizeInput {
        task_id: input.task_id.clone(),
        step_id: input.step_id.clone(),
        source: PathBuf::from(&input.source),
        filter: input.filter.clone(),
        destination: PathBuf::from(&input.destination),
    };

    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(&state.kernel, &skill_input, approver)?;

    let moved_paths = execution
        .moved_paths
        .into_iter()
        .map(|(from, to)| [from.to_string_lossy().into_owned(), to.to_string_lossy().into_owned()])
        .collect();

    Ok(OrganizeResult {
        committed: execution.tool_result.status == trust_kernel::toolresult::ToolStatus::Succeeded,
        moved_paths,
        evidence_strength: execution.tool_result.evidence_strength.as_str().to_string(),
        compensation_ref: execution.tool_result.compensation_ref.clone(),
        error: None,
    })
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn organize_files_command(
    state: tauri::State<'_, AppState>,
    input: OrganizeInput,
) -> Result<OrganizeResult, String> {
    // 从 AppState 上的 registry 构造 TauriApprover
    let approver = crate::approver::TauriApprover::new(state.approval_registry.clone());
    organize_files(&state, &approver, &input).map_err(Into::into)
}
```

更新 `state.rs`:

```rust
use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;
use crate::approver::ApprovalRegistry;

pub struct AppState {
    pub kernel: Arc<TrustKernel>,
    #[cfg(feature = "tauri")]
    pub approval_registry: ApprovalRegistry,
}

impl AppState {
    #[cfg(feature = "tauri")]
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
            approval_registry: ApprovalRegistry::new(),
        }
    }

    #[cfg(not(feature = "tauri"))]
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
        }
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory()?;
        Ok(Self::new(kernel))
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_file(path)?;
        Ok(Self::new(kernel))
    }
}
```

- [ ] **步骤 4:添加 `tempfile` dev-dependency 到 ui crate**

编辑 `voicepilot/crates/ui/Cargo.toml`,添加:

```toml
[dev-dependencies]
tempfile = "3"
trust-kernel = { workspace = true }
```

- [ ] **步骤 5:运行测试(green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# 期望:4 个通过(3 route_text + 1 organize_files)
```

- [ ] **步骤 6:提交**

```powershell
git add voicepilot/crates/ui/Cargo.toml voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/src/state.rs voicepilot/crates/ui/tests/commands_unit.rs
git commit -m "Task 4: organize_files Tauri command wires FilesOrganizeSkill + TauriApprover (V1.1 §5.2, §6.2, §8.2)"
```

---

## 任务 5:`submit_approval` command —— webview 决定投递

**文件:**
- 修改:`voicepilot/crates/ui/src/commands.rs`
- 修改:`voicepilot/crates/ui/tests/commands_unit.rs`

- [ ] **步骤 1:先写测试(red)**

追加到 `commands_unit.rs`:

```rust
use trust_kernel::approval::types::ApprovalDecision;
use voicepilot_ui::commands::submit_approval;
use voicepilot_ui::approver::ApprovalRegistry;
use trust_kernel::policy::transaction::EffectManifest;

#[test]
fn submit_approval_delivers_decision_to_waiting_approver() {
    let state = AppState::new_in_memory().unwrap();
    let manifest = EffectManifest {
        sources: vec![],
        destination: "D:/test".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id, rx) = state.approval_registry.create_request(&manifest);

    // 派生一个线程等待决定
    let registry = state.approval_registry.clone();
    let handle = std::thread::spawn(move || {
        registry.wait_for_decision(rx, std::time::Duration::from_secs(5))
    });

    // 给线程一点时间开始等待
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 提交 approval 决定
    let result = submit_approval(&state, &approval_id, ApprovalDecision::Allow).unwrap();
    assert!(result);

    let decision = handle.join().unwrap();
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn submit_approval_returns_false_for_unknown_id() {
    let state = AppState::new_in_memory().unwrap();
    let result = submit_approval(&state, "apr_nonexistent", ApprovalDecision::Deny).unwrap();
    assert!(!result);
}
```

- [ ] **步骤 2:运行测试(red)**

- [ ] **步骤 3:实现 `submit_approval`(green)**

添加到 `commands.rs`:

```rust
use trust_kernel::approval::types::ApprovalDecision;

/// 为待处理请求提交用户的 approval 决定。
/// 如果决定已投递返回 true,如果请求已被消费或从未存在返回 false
/// (一次性,符合 §8.2)。
pub fn submit_approval(
    state: &AppState,
    approval_id: &str,
    decision: ApprovalDecision,
) -> UiResult<bool> {
    let sender = match state.approval_registry.take_sender(approval_id) {
        Some(s) => s,
        None => return Ok(false),
    };
    let _ = sender.send(decision);
    Ok(true)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn submit_approval_command(
    state: tauri::State<'_, AppState>,
    approval_id: String,
    decision: ApprovalDecision,
) -> Result<bool, String> {
    submit_approval(&state, &approval_id, decision).map_err(Into::into)
}
```

- [ ] **步骤 4:运行测试(green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# 期望:6 个通过
```

- [ ] **步骤 5:提交**

```powershell
git add voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/commands_unit.rs
git commit -m "Task 5: submit_approval Tauri command delivers webview decision (V1.1 §8.2 one-shot)"
```

---

## 任务 6:Tauri app 入口 + approval 请求事件发射

**文件:**
- 创建:`voicepilot/crates/ui/src/main.rs`
- 创建:`voicepilot/crates/ui/src/app.rs`
- 创建:`voicepilot/crates/ui/tauri.conf.json`

**设计:**
- `TauriApprover::prompt` 向所有 webview 发射 `approval-request` 事件,携带 `{approval_request_id, manifest}`
- webview 的 ApprovalModal 通过 `@tauri-apps/api/event` 监听
- 提交时,webview 调用 `submit_approval_command`

- [ ] **步骤 1:创建 `tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "VoicePilot",
  "version": "0.1.0",
  "identifier": "com.voicepilot.app",
  "build": {
    "frontendDist": "web/dist",
    "devUrl": "http://localhost:5173",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "VoicePilot",
        "width": 1024,
        "height": 768,
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline'"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/icon.png"]
  }
}
```

- [ ] **步骤 2:创建 `app.rs` 包含 Tauri builder**

```rust
//! Tauri app builder + command 注册。

use tauri::Manager;
use crate::commands::{
    route_text_command, organize_files_command, submit_approval_command,
};
use crate::state::AppState;
use crate::error::UiResult;

pub fn run(kernel: trust_kernel::kernel::TrustKernel) -> UiResult<()> {
    let state = AppState::new(kernel);
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            route_text_command,
            organize_files_command,
            submit_approval_command,
        ])
        .setup(|_app| {
            // W6b:按需通过 app.get_webview_window("approval") 打开 Approval 窗口
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| crate::error::UiError::Tauri(e.to_string()))?;
    Ok(())
}
```

- [ ] **步骤 3:创建 `main.rs`**

```rust
use voicepilot_ui::app;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    let db_path = std::env::var("VOICEPILOT_DB")
        .unwrap_or_else(|_| "voicepilot.db".to_string());
    let kernel = trust_kernel::kernel::TrustKernel::open_file(&db_path)
        .expect("failed to open kernel");

    app::run(kernel).expect("failed to run Tauri app");
}
```

- [ ] **步骤 4:更新 `TauriApprover` 以发射事件**

修改 `approver.rs` —— `TauriApprover` 需要一个 `AppHandle` 来发射事件。添加新构造函数并更新 `prompt`:

```rust
#[cfg(feature = "tauri")]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(feature = "tauri")]
pub struct TauriApprover {
    registry: ApprovalRegistry,
    app: AppHandle,
}

#[cfg(feature = "tauri")]
impl TauriApprover {
    pub fn new(registry: ApprovalRegistry, app: AppHandle) -> Self {
        Self { registry, app }
    }

    pub fn registry(&self) -> &ApprovalRegistry {
        &self.registry
    }
}

#[cfg(feature = "tauri")]
#[derive(serde::Serialize)]
struct ApprovalRequestPayload {
    approval_request_id: String,
    manifest: trust_kernel::policy::transaction::EffectManifest,
}

#[cfg(feature = "tauri")]
impl Approver for TauriApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        let (approval_id, rx) = self.registry.create_request(manifest);
        let payload = ApprovalRequestPayload {
            approval_request_id: approval_id.clone(),
            manifest: manifest.clone(),
        };
        // 向所有 webview 发射;ApprovalModal 通过 @tauri-apps/api/event 监听
        let _ = self.app.emit("approval-request", payload);
        self.registry.wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }
}
```

更新 `organize_files_command` 以用 `AppHandle` 构造 TauriApprover:

```rust
#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn organize_files_command(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    input: OrganizeInput,
) -> Result<OrganizeResult, String> {
    let approver = crate::approver::TauriApprover::new(state.approval_registry.clone(), app);
    organize_files(&state, &approver, &input).map_err(Into::into)
}
```

- [ ] **步骤 5:验证 Tauri 可编译(无 webview —— main.rs 使用 `tauri::generate_context!` 需要 `tauri.conf.json`)**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# 期望:可编译。(Tauri context 在运行时无 web/dist 会失败,但 check 通过。)
```

- [ ] **步骤 6:提交**

```powershell
git add voicepilot/crates/ui/src/main.rs voicepilot/crates/ui/src/app.rs voicepilot/crates/ui/src/approver.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tauri.conf.json
git commit -m "Task 6: Tauri app entry + approval-request event emission (V1.1 §8.2)"
```

---

## 任务 7:React 前端 —— Approval 模态框 + Main 视图

**文件:**
- 创建:`voicepilot/crates/ui/web/package.json`
- 创建:`voicepilot/crates/ui/web/vite.config.ts`
- 创建:`voicepilot/crates/ui/web/tsconfig.json`
- 创建:`voicepilot/crates/ui/web/index.html`
- 创建:`voicepilot/crates/ui/web/src/main.tsx`
- 创建:`voicepilot/crates/ui/web/src/App.tsx`
- 创建:`voicepilot/crates/ui/web/src/components/MainView.tsx`
- 创建:`voicepilot/crates/ui/web/src/components/ApprovalModal.tsx`
- 创建:`voicepilot/crates/ui/web/src/api.ts`
- 创建:`voicepilot/crates/ui/web/src/types.ts`

**设计:**
前端使用 **Trae frontend-design 美学指南**(根据已加载的 skill):大胆的字体、独特的配色,不是通用的 AI slop。对于 VoicePilot——一个本地优先的隐私工具——美学应该是**可信赖、技术感、略带编辑性**。想象一下:深色主题、等宽字体强调、锐利的字体、"工程控制台"感觉。

美学方向:**"Engineering Console"** —— 深海军蓝 + 暖琥珀色强调,IBM Plex Mono 用于代码/数据,IBM Plex Sans 用于正文,大量留白,锐利的 4px 边角(不圆角),微妙的网格背景。

- [ ] **步骤 1:创建 `web/package.json`**

```json
{
  "name": "voicepilot-ui-web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "react": "^18.3.1",
    "react-dom": "^18.3.1"
  },
  "devDependencies": {
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0"
  }
}
```

- [ ] **步骤 2:创建 `web/vite.config.ts`**

```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
});
```

- [ ] **步骤 3:创建 `web/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2021",
    "useDefineForClassFields": true,
    "lib": ["ES2021", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src"]
}
```

- [ ] **步骤 4:创建 `web/index.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>VoicePilot</title>
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&family=IBM+Plex+Sans:wght@400;500;600;700&display=swap" rel="stylesheet" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **步骤 5:创建 `web/src/types.ts`**(镜像 Rust DTO)

```typescript
export interface EffectManifest {
  sources: FileSnapshot[];
  destination: string;
  conflicts: string[];
  total_bytes: number;
}

export interface FileSnapshot {
  canonical_path: string;
  file_id: string;
  size: number;
  last_write_time: string;
  sha256: string;
}

export type ApprovalDecision = "allow" | "deny" | "modify";

export interface ApprovalRequestPayload {
  approval_request_id: string;
  manifest: EffectManifest;
}

export type RouteTextResult =
  | { kind: "routed"; skill_id: string }
  | { kind: "unmatched"; text: string }
  | { kind: "empty" };

export interface OrganizeInput {
  task_id: string;
  step_id: string;
  source: string;
  filter: string;
  destination: string;
}

export interface OrganizeResult {
  committed: boolean;
  moved_paths: [string, string][];
  evidence_strength: string;
  compensation_ref: string | null;
  error: string | null;
}
```

- [ ] **步骤 6:创建 `web/src/api.ts`**

```typescript
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
} from "./types";

export async function routeText(text: string): Promise<RouteTextResult> {
  return invoke<RouteTextResult>("route_text_command", { text });
}

export async function organizeFiles(
  input: OrganizeInput
): Promise<OrganizeResult> {
  return invoke<OrganizeResult>("organize_files_command", { input });
}

export async function submitApproval(
  approvalRequestId: string,
  decision: ApprovalDecision
): Promise<boolean> {
  return invoke<boolean>("submit_approval_command", {
    approvalId: approvalRequestId,
    decision,
  });
}

export function onApprovalRequest(
  handler: (payload: ApprovalRequestPayload) => void
): Promise<UnlistenFn> {
  return listen<ApprovalRequestPayload>("approval-request", (event) => {
    handler(event.payload);
  });
}
```

- [ ] **步骤 7:创建 `web/src/main.tsx`**

```typescript
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

- [ ] **步骤 8:创建 `web/src/styles.css`**(Engineering Console 美学)

```css
:root {
  --bg-deep: #0a0e1a;
  --bg-base: #111827;
  --bg-elev: #1a2234;
  --bg-elev2: #232d44;
  --border: #2d3650;
  --border-bright: #3d4866;
  --text-primary: #e6edf7;
  --text-secondary: #94a3b8;
  --text-muted: #64748b;
  --accent: #f59e0b;
  --accent-bright: #fbbf24;
  --danger: #ef4444;
  --success: #10b981;
  --mono: "IBM Plex Mono", "SF Mono", "Monaco", monospace;
  --sans: "IBM Plex Sans", "Inter", system-ui, sans-serif;
}

* {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html, body, #root {
  height: 100%;
  background: var(--bg-deep);
  color: var(--text-primary);
  font-family: var(--sans);
  font-size: 14px;
  line-height: 1.55;
  -webkit-font-smoothing: antialiased;
  overflow: hidden;
}

.app-shell {
  display: grid;
  grid-template-rows: 48px 1fr;
  height: 100vh;
}

.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 24px;
  background: var(--bg-base);
  border-bottom: 1px solid var(--border);
  font-family: var(--mono);
  font-size: 12px;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--text-secondary);
}

.topbar .brand {
  color: var(--accent);
  font-weight: 600;
}

.topbar .brand::before {
  content: "◆ ";
  color: var(--accent-bright);
}

.main {
  display: grid;
  grid-template-columns: 1fr 320px;
  gap: 1px;
  background: var(--border);
  overflow: hidden;
}

.panel {
  background: var(--bg-base);
  padding: 32px 40px;
  overflow-y: auto;
}

.panel-header {
  font-family: var(--mono);
  font-size: 11px;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--text-muted);
  margin-bottom: 12px;
}

.panel-title {
  font-family: var(--sans);
  font-size: 28px;
  font-weight: 600;
  letter-spacing: -0.02em;
  margin-bottom: 32px;
  color: var(--text-primary);
}

.panel-title em {
  font-style: normal;
  color: var(--accent);
}

.form-row {
  display: grid;
  grid-template-columns: 120px 1fr;
  gap: 16px;
  align-items: center;
  margin-bottom: 16px;
}

.form-row label {
  font-family: var(--mono);
  font-size: 11px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-secondary);
}

.form-row input {
  background: var(--bg-deep);
  border: 1px solid var(--border);
  color: var(--text-primary);
  padding: 8px 12px;
  font-family: var(--mono);
  font-size: 13px;
  border-radius: 0;
  outline: none;
  transition: border-color 0.15s;
}

.form-row input:focus {
  border-color: var(--accent);
}

.btn {
  font-family: var(--mono);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  padding: 10px 20px;
  border: 1px solid var(--border-bright);
  background: var(--bg-elev);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 0;
  transition: all 0.15s;
}

.btn:hover {
  background: var(--bg-elev2);
  border-color: var(--accent);
}

.btn-primary {
  background: var(--accent);
  color: var(--bg-deep);
  border-color: var(--accent);
  font-weight: 600;
}

.btn-primary:hover {
  background: var(--accent-bright);
  border-color: var(--accent-bright);
}

.btn-danger {
  border-color: var(--danger);
  color: var(--danger);
}

.btn-danger:hover {
  background: var(--danger);
  color: var(--bg-deep);
}

.status-panel {
  background: var(--bg-deep);
  padding: 24px;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--text-secondary);
}

.status-line {
  margin-bottom: 8px;
  display: flex;
  gap: 12px;
}

.status-line .key {
  color: var(--text-muted);
  min-width: 100px;
}

.status-line .val {
  color: var(--text-primary);
}

/* Approval Modal */
.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(10, 14, 26, 0.85);
  backdrop-filter: blur(8px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  animation: fadeIn 0.15s ease-out;
}

@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

.modal {
  background: var(--bg-base);
  border: 1px solid var(--border-bright);
  width: min(680px, 90vw);
  max-height: 85vh;
  overflow-y: auto;
  display: grid;
  grid-template-rows: auto 1fr auto;
  animation: slideUp 0.2s ease-out;
}

@keyframes slideUp {
  from { transform: translateY(20px); opacity: 0; }
  to { transform: translateY(0); opacity: 1; }
}

.modal-header {
  padding: 20px 28px;
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.modal-header h2 {
  font-family: var(--sans);
  font-size: 18px;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.modal-header .badge {
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--accent);
  border: 1px solid var(--accent);
  padding: 4px 8px;
}

.modal-body {
  padding: 24px 28px;
}

.manifest-table {
  width: 100%;
  border-collapse: collapse;
  font-family: var(--mono);
  font-size: 12px;
}

.manifest-table th,
.manifest-table td {
  text-align: left;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}

.manifest-table th {
  color: var(--text-muted);
  font-weight: 500;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  font-size: 10px;
}

.manifest-table td {
  color: var(--text-primary);
}

.manifest-table .path {
  color: var(--accent);
}

.manifest-summary {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
  margin-bottom: 24px;
  padding: 16px;
  background: var(--bg-deep);
  border: 1px solid var(--border);
}

.summary-stat {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.summary-stat .label {
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.summary-stat .value {
  font-family: var(--mono);
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary);
}

.summary-stat .value.danger {
  color: var(--danger);
}

.modal-footer {
  padding: 16px 28px;
  border-top: 1px solid var(--border);
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  background: var(--bg-deep);
}

.conflicts-list {
  margin-top: 12px;
  padding: 12px 16px;
  background: rgba(239, 68, 68, 0.08);
  border-left: 3px solid var(--danger);
  font-family: var(--mono);
  font-size: 12px;
  color: var(--danger);
}

.conflicts-list ul {
  list-style: none;
  margin-top: 8px;
}

.conflicts-list li {
  padding: 2px 0;
}

.route-result {
  padding: 16px;
  background: var(--bg-deep);
  border: 1px solid var(--border);
  margin-top: 16px;
  font-family: var(--mono);
  font-size: 13px;
}

.route-result.routed {
  border-left: 3px solid var(--success);
}

.route-result.unmatched {
  border-left: 3px solid var(--accent);
}
```

- [ ] **步骤 9:创建 `web/src/components/MainView.tsx`**

```typescript
import { useState } from "react";
import { routeText, organizeFiles } from "../api";
import type { RouteTextResult, OrganizeResult } from "../types";

export function MainView() {
  const [text, setText] = useState("");
  const [routeResult, setRouteResult] = useState<RouteTextResult | null>(null);
  const [source, setSource] = useState("");
  const [filter, setFilter] = useState("*.txt");
  const [destination, setDestination] = useState("");
  const [organizeResult, setOrganizeResult] = useState<OrganizeResult | null>(null);
  const [busy, setBusy] = useState(false);

  async function onRoute() {
    setBusy(true);
    try {
      const r = await routeText(text);
      setRouteResult(r);
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  async function onOrganize() {
    setBusy(true);
    try {
      const r = await organizeFiles({
        task_id: `t-${Date.now()}`,
        step_id: `s-${Date.now()}`,
        source,
        filter,
        destination,
      });
      setOrganizeResult(r);
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <div className="panel-header">§ 5.1 Skill Router</div>
      <h1 className="panel-title">
        Route <em>intent</em> → Skill
      </h1>

      <div className="form-row">
        <label>Text</label>
        <input
          type="text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="organize my downloads"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onRoute} disabled={busy}>
          Route
        </button>
      </div>

      {routeResult && (
        <div
          className={`route-result ${
            routeResult.kind === "routed" ? "routed" : "unmatched"
          }`}
        >
          {routeResult.kind === "routed" && (
            <>✓ Routed to skill: <strong>{routeResult.skill_id}</strong></>
          )}
          {routeResult.kind === "unmatched" && (
            <>? No skill matched: <strong>{routeResult.text}</strong></>
          )}
          {routeResult.kind === "empty" && <>∅ Empty input</>}
        </div>
      )}

      <div className="panel-header" style={{ marginTop: 48 }}>§ 5.2 Files Organize</div>
      <h1 className="panel-title">
        Run <em>files.organize</em>
      </h1>

      <div className="form-row">
        <label>Source</label>
        <input
          type="text"
          value={source}
          onChange={(e) => setSource(e.target.value)}
          placeholder="D:/Downloads"
        />
      </div>
      <div className="form-row">
        <label>Filter</label>
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="*.pdf"
        />
      </div>
      <div className="form-row">
        <label>Destination</label>
        <input
          type="text"
          value={destination}
          onChange={(e) => setDestination(e.target.value)}
          placeholder="D:/Documents/Papers"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onOrganize} disabled={busy}>
          Organize
        </button>
      </div>

      {organizeResult && (
        <div className="route-result routed">
          <div>
            committed: <strong>{String(organizeResult.committed)}</strong>
          </div>
          <div>
            moved: <strong>{organizeResult.moved_paths.length}</strong> file(s)
          </div>
          <div>
            evidence: <strong>{organizeResult.evidence_strength}</strong>
          </div>
          {organizeResult.compensation_ref && (
            <div>
              compensation_ref: <strong>{organizeResult.compensation_ref}</strong>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
```

- [ ] **步骤 10:创建 `web/src/components/ApprovalModal.tsx`**

```typescript
import { useEffect, useState } from "react";
import { submitApproval } from "../api";
import type { ApprovalRequestPayload } from "../types";

interface Props {
  payload: ApprovalRequestPayload;
  onDismiss: () => void;
}

export function ApprovalModal({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const { approval_request_id, manifest } = payload;

  async function decide(decision: "allow" | "deny") {
    setSubmitting(true);
    try {
      await submitApproval(approval_request_id, decision);
      onDismiss();
    } catch (e) {
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // 卸载时自动拒绝(例如用户关闭窗口)
  useEffect(() => {
    return () => {
      // 关闭时尽力发送 deny —— 但仅当尚未提交
      // Rust 端如果已消费会返回 false(一次性)
      submitApproval(approval_request_id, "deny").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <div className="modal-header">
          <h2>Approve File Operation</h2>
          <span className="badge">E2 · D2 · Local</span>
        </div>
        <div className="modal-body">
          <div className="manifest-summary">
            <div className="summary-stat">
              <span className="label">Sources</span>
              <span className="value">{manifest.sources.length}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Total Bytes</span>
              <span className="value">{manifest.total_bytes.toLocaleString()}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Conflicts</span>
              <span className="value danger">
                {manifest.conflicts.length}
              </span>
            </div>
          </div>

          <table className="manifest-table">
            <thead>
              <tr>
                <th>Path</th>
                <th>Size</th>
                <th>SHA-256</th>
              </tr>
            </thead>
            <tbody>
              {manifest.sources.map((s) => (
                <tr key={s.canonical_path}>
                  <td className="path">{s.canonical_path}</td>
                  <td>{s.size}</td>
                  <td>{s.sha256.slice(0, 16)}…</td>
                </tr>
              ))}
            </tbody>
          </table>

          <div className="form-row" style={{ marginTop: 24 }}>
            <label>Destination</label>
            <input type="text" value={manifest.destination} readOnly />
          </div>

          {manifest.conflicts.length > 0 && (
            <div className="conflicts-list">
              ⚠ {manifest.conflicts.length} conflict(s) detected:
              <ul>
                {manifest.conflicts.map((c, i) => (
                  <li key={i}>{c}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
        <div className="modal-footer">
          <button
            className="btn btn-danger"
            onClick={() => decide("deny")}
            disabled={submitting}
          >
            Deny
          </button>
          <button
            className="btn btn-primary"
            onClick={() => decide("allow")}
            disabled={submitting}
          >
            Allow
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **步骤 11:创建 `web/src/App.tsx`**

```typescript
import { useEffect, useState } from "react";
import { MainView } from "./components/MainView";
import { ApprovalModal } from "./components/ApprovalModal";
import { onApprovalRequest } from "./api";
import type { ApprovalRequestPayload } from "./types";

export function App() {
  const [approval, setApproval] = useState<ApprovalRequestPayload | null>(null);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) => {
      setApproval(payload);
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return (
    <div className="app-shell">
      <div className="topbar">
        <span className="brand">VoicePilot</span>
        <span>W6a · Trust Kernel · Single Rust Architecture</span>
      </div>
      <div className="main">
        <MainView />
        <div className="status-panel">
          <div className="panel-header">Kernel Status</div>
          <div className="status-line">
            <span className="key">Spec:</span>
            <span className="val">V1.1.2</span>
          </div>
          <div className="status-line">
            <span className="key">Architecture:</span>
            <span className="val">Single Rust Kernel</span>
          </div>
          <div className="status-line">
            <span className="key">Voice:</span>
            <span className="val">opt-in (W5)</span>
          </div>
          <div className="status-line">
            <span className="key">Approval TTL:</span>
            <span className="val">300s</span>
          </div>
        </div>
      </div>
      {approval && (
        <ApprovalModal
          payload={approval}
          onDismiss={() => setApproval(null)}
        />
      )}
    </div>
  );
}
```

- [ ] **步骤 12:安装 npm deps + 构建前端**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force
npm install
npm run build
# 期望:web/dist/ 包含 index.html + assets/
```

- [ ] **步骤 13:验证 Tauri app 与前端一起编译**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# 期望:可编译。tauri::generate_context! 拾取 web/dist。
```

- [ ] **步骤 14:提交**

```powershell
git add voicepilot/crates/ui/web/ voicepilot/crates/ui/src/
git commit -m "Task 7: React frontend — Main view + Approval modal (Engineering Console aesthetic)"
```

---

## 任务 8:端到端冒烟测试 —— Tauri command 管道

**文件:**
- 创建:`voicepilot/crates/ui/tests/w6a_e2e_smoke.rs`

**设计:**
测试完整管道,**不启动真实 Tauri webview**(需要 GUI)。而是:
1. 用内存 kernel 创建 `AppState`
2. 用测试文件创建临时目录
3. 用 `AutoApprover` 调用 `organize_files`(绕过 Tauri 事件系统)
4. 验证文件已移动 + 审计链完整 + 补偿已记录

这验证 Rust 侧管道。完整 webview E2E(实际按钮点击)是 W6b 的手动测试。

- [ ] **步骤 1:编写冒烟测试**

`voicepilot/crates/ui/tests/w6a_e2e_smoke.rs`:

```rust
#![cfg(feature = "tauri")]

use tempfile::TempDir;
use trust_kernel::approval::approver::AutoApprover;
use voicepilot_ui::commands::{organize_files, OrganizeInput};
use voicepilot_ui::state::AppState;

/// W6a §11.1 gate:端到端 organize_files 完整管道。
/// 使用 AutoApprover(绕过 Tauri 事件系统)验证 Rust 侧 Skill executor
/// + 审计链 + 补偿记录。TauriApprover 特定的事件发射在 approver_unit.rs 中覆盖。
#[test]
fn end_to_end_organize_files_with_auto_approver_full_pipeline() {
    // Setup:临时目录包含 2 个 .txt 文件 + 1 个 .log 文件 + 目标目录
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    std::fs::write(src.join("b.txt"), "beta").unwrap();
    std::fs::write(src.join("c.log"), "gamma").unwrap(); // 不匹配

    // AppState 用内存 kernel(TauriApprover 的 registry 在 AppState 上,
    // 但此处未使用,因为直接注入 AutoApprover)
    let state = AppState::new_in_memory().unwrap();

    let result = organize_files(
        &state,
        &AutoApprover,
        &OrganizeInput {
            task_id: "t-w6a-smoke".to_string(),
            step_id: "s-w6a-smoke".to_string(),
            source: src.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest.to_string_lossy().into_owned(),
        },
    ).unwrap();

    // 验证:2 个文件已移动(a.txt + b.txt),c.log 未触碰
    assert!(result.committed, "tool_result should be committed");
    assert_eq!(result.moved_paths.len(), 2);
    assert!(dest.join("a.txt").exists());
    assert!(dest.join("b.txt").exists());
    assert!(src.join("c.log").exists(), "non-matching file untouched");

    // 验证:审计链有预期事件(TASK_CREATED + STEP_CREATED +
    // STEP_PREPARED + APPROVAL_RECORDED + STEP_COMMITTED + COMPENSATION_CREATED)
    let audit_count = state
        .kernel
        .audit_count_for_task("t-w6a-smoke")
        .unwrap();
    assert!(audit_count >= 4, "expected ≥4 audit events, got {}", audit_count);

    // 验证:step 处于 Committed 状态
    let step = state.kernel.get_step("s-w6a-smoke").unwrap().unwrap();
    assert_eq!(
        step.status,
        trust_kernel::repo::step_repo::StepStatus::Committed
    );

    // 验证:补偿已记录(强 + auto_reverse 就绪)
    assert!(
        result.compensation_ref.is_some(),
        "compensation_ref must be set after successful commit"
    );
}

/// 测试拒绝会取消操作而不提交。
#[test]
fn end_to_end_deny_cancels_commit() {
    use trust_kernel::approval::approver::AutoDenier;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();

    let state = AppState::new_in_memory().unwrap();
    let result = organize_files(
        &state,
        &AutoDenier,
        &OrganizeInput {
            task_id: "t-w6a-deny".to_string(),
            step_id: "s-w6a-deny".to_string(),
            source: src.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest.to_string_lossy().into_owned(),
        },
    );

    // 拒绝 → Skill executor 返回错误(跳过提交)
    assert!(result.is_err(), "deny should produce an error");

    // 验证:源文件未被触碰
    assert!(src.join("a.txt").exists(), "source file must not be moved on deny");
    assert!(!dest.join("a.txt").exists(), "dest must not contain the file on deny");
}
```

- [ ] **步骤 2:运行冒烟测试**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test w6a_e2e_smoke
# 期望:2 个通过
```

- [ ] **步骤 3:运行完整 ui 测试套件**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# 期望:approver_unit (4) + commands_unit (6) + w6a_e2e_smoke (2) = 12 个通过
```

- [ ] **步骤 4:运行默认 workspace 测试(无回归检查)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml
# 期望:196 个通过(W1-W4)+ ui crate 跳过(default-members = trust-kernel + cli only)
```

- [ ] **步骤 5:提交**

```powershell
git add voicepilot/crates/ui/tests/w6a_e2e_smoke.rs
git commit -m "Task 8: W6a end-to-end smoke test — organize_files + audit chain + compensation (V1.1 §11.1 W6a gate)"
```

---

## 最终审查清单

所有 8 个任务完成后,验证:

- [ ] `cargo test --manifest-path voicepilot\Cargo.toml` —— 196 个通过(W1-W4 无回归)
- [ ] `cargo test --manifest-path voicepilot\Cargo.toml --features voice` —— voice opt-in 测试仍通过(W5 无回归)
- [ ] `cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri` —— 12 个通过(W6a)
- [ ] `cargo check --manifest-path voicepilot\Cargo.toml` —— 0 警告(默认)
- [ ] `cargo check --manifest-path voicepilot\Cargo.toml --features tauri` —— 0 警告
- [ ] `cargo clippy --manifest-path voicepilot\Cargo.toml --features tauri -- -D warnings` —— 0 警告(仅 W6a 代码;预先存在的 W1-W5 nits 不在范围内)
- [ ] `cd voicepilot/crates/ui/web && npm run build` —— 生成包含 `index.html` 的 `web/dist/`
- [ ] Git log 显示 8 个 commits + 1 个 plan commit(W6a 共 9 个)
- [ ] PROGRESS.md 更新 W6a 部分

---

## 已知 Spec Issues(W6a 期间可能浮现)

预期 spec issues(按用户指示"遇到spec issue直接修复"):

| # | 主题 | 可能触发点 |
|---|---|---|
| #50 | 每窗口 IPC 硬化的 Tauri capability/permission schema 未指定 | 任务 6(tauri.conf.json 缺少 capabilities 部分) |
| #51 | Approval 窗口生命周期(打开/关闭/超时)未指定 | 任务 6(何时打开 Approval 窗口 vs. 内联模态框) |
| #52 | `approval_request_id` 格式未指定(UUID v4 vs. 顺序) | 任务 2(选择 `apr_<uuid>` 前缀) |
| #53 | Approval 超时默认值(300s vs. 可配置)未指定 | 任务 2(选择 300s 匹配 prepare_token TTL) |
| #54 | Tauri command 错误 → webview 错误映射未指定 | 任务 3-5(使用 `String` 错误,可能需要结构化错误) |
| #55 | `web/dist` 构建产物 gitignore 策略未指定 | 任务 7(添加到 .gitignore) |
| #56 | 前端 bundle 签名/完整性检查未指定 | 任务 7(Tauri CSP 已设置,但无 SRI) |

如果其中任何一个浮现,直接修复 spec(按用户指示)并在 PROGRESS.md §4.2 中记录修复。
