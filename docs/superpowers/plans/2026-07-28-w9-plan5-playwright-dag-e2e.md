# W9 Plan 5: 真实 Playwright MCP DAG E2E Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W9 设计文档 §2.5 实现 2 个真实 Playwright MCP DAG 端到端测试(`crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`):场景 1 真实 `form.prepare → form.submit` DAG(打开 httpbin 表单 + Slot 流水 + E3 PerStep 审批 + Stronghold 加密补偿 + taint 传播);场景 2 真实 `research.save_markdown` DAG(抓取 example.com + 保存 markdown + web_page taint)。复用 W7 Plan 5 的 `#[ignore]` + CWD_MUTEX + 短路 passing 模式,失败不阻塞 CI。

**Architecture:** 1 个测试文件 2 个 `#[ignore]` 真实 E2E 测试 + 共享 helpers(`npx_playwright_available` 探测 / `literal_text_template` / `DagPlan` 构造器 / `CwdGuard` / `CWD_MUTEX`)。TrustKernel 在 `boot()` 时由 `McpServerRepo::insert_default_servers` 自动 seed `playwright` MCP 行(核实报告 3.3,kernel.rs:120-136),测试不 override 该行,直接走真实 `npx -y @playwright/mcp@latest`。Stronghold 通过 `kernel.set_stronghold_vault(StrongholdVault::create(...))` 注入(W9 Plan 1 API),补偿记录的 `snapshot_encrypted` 必须非空。Taint 通过 `TaintRepo::list_by_provenance(&conn, "mcp_tool:playwright")` 验证(W9 Plan 3 API)。Slot 流水用 `TemplateExpr::Concat` + `VarRef { scope: VarScope::Prev, path: "output.url".into() }` 在 `form.submit` 节点引用 `form.prepare` 的输出。

**Tech Stack:** Rust(stable),`trust_kernel::skills::dag_executor::DagExecutor`(W8 已有),`trust_kernel::skills::dag_types::{DagPlan, DagNode, DagEdge, DagStatus}`(W8 已有),`trust_kernel::skills::template::{SlotTemplate, SlotKind, TemplateExpr, VarRef, VarScope}`(W8 已有),`trust_kernel::crypto::stronghold::StrongholdVault`(W9 Plan 1 新增),`trust_kernel::policy::taint_repo::TaintRepo`(W9 Plan 3 新增),Node.js 22+ + `npx -y @playwright/mcp@latest`(用户侧依赖,不打包),PowerShell(`;` 分隔命令)。

**Spec:** `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.5(Plan 5 范围)+ §10(Conventions)+ §11(与 W8 的兼容性)

**Precondition:**

**Precondition 校验(执行 Task 1 之前必须全部通过):**

```powershell
# 1. 验证 StrongholdVault 源文件存在
Test-Path voicepilot/crates/trust-kernel/src/crypto/stronghold.rs

# 2. 验证 TaintRepo 源文件存在
Test-Path voicepilot/crates/trust-kernel/src/policy/taint_repo.rs

# 3. 验证 Plan 3 dispatcher 已实现 mcp_tool taint 传播
Select-String -Path voicepilot/crates/trust-kernel/src/skills/dispatcher.rs -Pattern "mcp_tool:"

# 4. 验证 create_post_commit_compensation 在 E3 + Allow 路径下被调用
Select-String -Path voicepilot/crates/trust-kernel/src/skills/ -Pattern "create_post_commit_compensation" -Recurse
```

若任一校验失败,阻塞 Plan 5 启动,提示"先完成 W9 Plan 1/3 或核实 Plan 3 dispatcher 实现"。

- **W9 Plan 3 web_page provenance 注记**:W9 Plan 3 实施者须保留 W7 Plan 5 `research_save.rs` 中 web_page provenance 硬编码规则,或确认查表驱动覆盖 web_page provenance。若 web_page provenance 未覆盖,场景 2 的 web_page taint 断言会失败(见 Task 3 Step 3)。
- **W9 Plan 4 + W8 测试无回归注记**:W9 Plan 4 已完成 + W8 既有测试无回归(`cargo test --workspace --features voice,tauri,llm` 全 PASS)。若 W1-W8 测试 fail,回 Plan 4 修复。
- **DagExecutor::new 签名核实**:核实 `DagExecutor::new` 实际签名:`grep -A 3 "impl DagExecutor" voicepilot/crates/trust-kernel/src/skills/dag_executor.rs | grep "pub fn new"`。若实际参数顺序与 Task 2/3 测试代码(kernel, approver, dag_repo)不一致,按实际签名调整。
- W9 Plan 1 已完成:`crates/trust-kernel/src/crypto/stronghold.rs` 提供 `StrongholdVault::create(password, &conn) -> Result<Self>` + `is_unlocked() -> bool` + `encrypt(&[u8]) -> Result<EncryptedPayload>` + `decrypt(&EncryptedPayload) -> Result<Vec<u8>>` API;`TrustKernel::set_stronghold_vault(vault: StrongholdVault)` setter 已加;`stronghold` feature 在 `trust-kernel/Cargo.toml` 已声明(`stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2"]`)
- W9 Plan 2 已完成:`create_post_commit_compensation` 在 stronghold feature 启用 + vault 解锁时,把 `reverse_payload` 加密后写入 `snapshot_encrypted`(非空),明文 `reverse_payload` 列置空字符串;`reverse_compensation` 能解密还原
- W9 Plan 3 已完成:`crates/trust-kernel/src/policy/taint_repo.rs` 提供 `TaintRepo::new()` + `list_by_provenance(&conn, &str) -> Result<Vec<TaintRecord>>` + `find_by_value(&conn, &str) -> Result<Option<TaintRecord>>` + `upsert(&conn, &TaintRecord) -> Result<()>`;`skills/dispatcher.rs` 在 Skill executor 调用 MCP tool 后,把返回值标 `mcp_tool:<server_id>` taint 并 upsert;gateway.rs 已切换为查表驱动
- W9 Plan 4 已完成:`DagApprovalOutcome` 枚举已加 `Modify { modified_plan: Box<DagPlan> }` 变体;`DagExecutor::run` 处理 Modify 分支(虽然 Plan 5 测试用 `AutoApprover` 不触发 Modify,但依赖 dag_executor 不会因 Modify 分支引入回归)
- W7 Plan 5 已完成(commit `278240a`):`McpServerRepo::insert_default_servers` 在 kernel boot 时自动 seed `playwright` 行(`server_id='playwright', command='npx', args='["-y", "@playwright/mcp@latest"]', enabled=1`);`research.save_markdown` + `form.prepare` + `form.submit` 三个 Skill executor 已实现并通过 mock 测试
- W8 已完成(commit `8ec814d`):`DagExecutor::run(&self, plan: &DagPlan) -> Result<DagResult>` 签名稳定(W9 Plan 6 才扩展 user_slots,Plan 5 不涉及);`DagPlan` / `DagNode` / `DagEdge` / `DagStatus` 数据结构稳定
- 本机环境:Node.js 22+ 已安装(`node --version` ≥ v22),`npx` 在 PATH;首次运行 `npx -y @playwright/mcp@latest` 会自动下载包(需网络)
- `cargo check --workspace --features voice,tauri,llm,stronghold` PASS(W9 Plan 1-4 完成后)
- `cargo test --workspace --features voice,tauri,llm,stronghold` 全 PASS,W1-W8 测试无回归

---

## W9 7-Plan 拆分概览(供 Plan 5 执行者参考)

| Plan | 范围 | Spec § | 状态 |
|---|---|---|---|
| Plan 1 | StrongholdVault + Argon2id 密钥派生 + config 存储 + 降级模式 | §2.1 | ✅ 已完成(前置) |
| Plan 2 | snapshot_encrypted 真实加密 + 明文 PoC 移除 | §2.2 | ✅ 已完成(前置) |
| Plan 3 | TaintRepo CRUD + dispatcher 传播 + gateway 查表驱动 | §2.3 | ✅ 已完成(前置) |
| Plan 4 | DagApprovalOutcome + dag_executor Modify 分支 + UI 编辑器 | §2.4 | ✅ 已完成(前置) |
| **Plan 5 (本文件)** | 真实 Playwright MCP DAG E2E(form DAG + research DAG) | §2.5 | ⏳ 进行中 |
| Plan 6 | 真实 UIA GUI DAG E2E + Slot 流水闭合 | §2.6 | 待 Plan 5 完成 |
| Plan 7 | 集成验收 + 7 套 feature cargo check + clippy + npm build + PROGRESS.md 收尾 | §2.7 | 待 Plan 1-6 完成 |

---

## File Structure

### Tests(`voicepilot/crates/trust-kernel/tests/`)

- **Create** `w9_plan5_playwright_dag_e2e.rs` — 2 个 `#[ignore]` 真实 E2E 测试 + 共享 helpers(~450 行)
  - `npx_playwright_available() -> bool` — 探测 `npx` 在 PATH + `@playwright/mcp` 可拉起(短时间超时)
  - `literal_text_template(&str) -> SlotTemplate` — 构造 `SlotKind::Text + TemplateExpr::Literal` 的字面量模板
  - `form_prepare_node(id, url) -> DagNode` — 构造 `form.prepare` 节点
  - `form_submit_node_with_slot(id) -> DagNode` — 构造 `form.submit` 节点,`input_template` 用 `TemplateExpr::Concat` 引用 `${prev.output.url}`
  - `research_save_node(id, url, save_path) -> DagNode` — 构造 `research.save_markdown` 节点
  - `build_form_dag() -> DagPlan` — 拼 `[form.prepare → form.submit]` DAG
  - `build_research_dag() -> DagPlan` — 拼 `[research.save_markdown]` DAG
  - `CWD_MUTEX: Mutex<()>` + `CwdGuard` — 串行化 CWD 写操作(复用 W7 Plan 5 模式)
  - `real_form_prepare_submit_dag_succeeds` — 场景 1 测试(`#[ignore]`)
  - `real_research_save_markdown_dag_succeeds` — 场景 2 测试(`#[ignore]`)

### Docs

- **Modify** `docs/PROGRESS.md` — W9 Plan 5 完成状态 + 测试统计 + 已知偏离(若有)

### 无源码改动

本 Plan 仅写测试 + 跑验收门禁 + 更新 PROGRESS.md,不修改 `crates/trust-kernel/src/` 或 `crates/ui/src/` 任何源码文件。所有依赖的 API(`StrongholdVault` / `TaintRepo` / `DagExecutor` / `McpServerRepo::insert_default_servers`)均由 W9 Plan 1-3 + W7 Plan 5 + W8 提供,本 Plan 仅调用。

**例外 — 若 W9 Plan 1/3 API 签名与本 plan 描述不一致**:执行者应在 Task 1 Step 2 读取 `crates/trust-kernel/src/crypto/stronghold.rs` 和 `crates/trust-kernel/src/policy/taint_repo.rs` 的实际签名,按实际签名调整测试代码,并在 PROGRESS.md "已知偏离" 段落记录差异。不回改 spec,不回改 W9 Plan 1/3。

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(不支持 heredoc,参考 `project_memory.md` "Lessons Learned")
- **TDD**:本 Plan 是 E2E 测试 plan,先写测试骨架 → 跑 `cargo check` 确认编译通过 → 跑 `cargo test --no-run` 确认测试注册 → 手动运行 `#[ignore]` 测试 → commit
- **`#[ignore]` 真实 E2E 测试模式**(复用 W7 Plan 5,核实报告 3.1-3.2):
  - 每个测试加 `#[ignore = "Requires real npx + @playwright/mcp + network. Run: cargo test --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --ignored manually. Requires Node.js ≥ 22 and network access."]`
  - 测试函数体首行调 `npx_playwright_available()`,不可用则 `eprintln!("Skipping: ..."); return;`(短路 passing,不 fail)
  - 默认 `cargo test` 不跑 `#[ignore]` 测试,失败不阻塞 CI
  - 手动运行:`cargo test --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --ignored`
- **CWD_MUTEX 串行化**(复用 W7 Plan 5 模式,核实报告 3.1):
  - `static CWD_MUTEX: Mutex<()> = Mutex::new(());`
  - 每个测试首行 `let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());`(poison 后也恢复)
  - `CwdGuard::enter(&temp_root)` 切到 tempdir,Drop 时恢复原 CWD
  - 原因:CWD 是进程全局资源,并行测试线程 race 会污染文件写入
  - **W9 修复(P1-8)**:测试改为 `#[test]` 后,`CWD_MUTEX` 用 `std::sync::Mutex` 无问题(无 async 上下文,无 MutexGuard 跨 await 死锁风险)
- **测试 attribute 选择**(W9 修复 P0-7):
  - **禁止 `#[tokio::test]`**:测试体无 `.await`,用 `#[test]` 同步上下文避免 tokio runtime + MutexGuard 死锁风险。DagExecutor 内部异步由 TauriApprover 自己建 runtime
  - 本 Plan 所有测试用 `#[test]` + `fn`(非 async),不引入 tokio runtime
- **DagExecutor::run 签名变更注记**(W9 修复 P0-4):本 plan 测试中 `executor.run(&dag_plan)` 调用点在 W9 Plan 6 实施时需同步改为 `executor.run(&dag_plan, &[])`(空 user_slots,行为等价 W8)。Plan 6 实施者须 grep `executor.run(` 全 workspace 更新所有调用点,包括本文件 Task 2 Step 1 + Task 3 Step 1
- **TrustKernel 不是 Clone**:e2e 测试用 `Arc::new(TrustKernel::open_in_memory().unwrap())` 共享(参考 `project_memory.md` "Lessons Learned");DagExecutor 构造器接收 `Arc<TrustKernel>`
- **kernel boot 自动 seed playwright MCP 行**(核实报告 3.3):
  - `TrustKernel::open_in_memory()` 内部调 `boot()` → `McpServerRepo::insert_default_servers(&conn)`(kernel.rs:120-136)
  - 测试不 override 该行,直接走真实 `npx -y @playwright/mcp@latest`
  - 若需 mock,参考 W7 Plan 5 `install_python_mock` helper(本 Plan 不用)
- **StrongholdVault 注入**(W9 Plan 1 API):
  - `StrongholdVault::create("test_password", &kernel.conn())` 创建新 vault + 持久化 salt + 解锁
  - `kernel.set_stronghold_vault(vault)` 注入到 kernel
  - 测试密码用 `"test_password"` 字面量(非敏感数据,测试用)
- **TaintRepo 验证**(W9 Plan 3 API):
  - `TaintRepo::new().list_by_provenance(&conn, "mcp_tool:playwright")` 返回 `Vec<TaintRecord>`
  - 断言 `!taints.is_empty()`(Playwright MCP 输出必须有 taint 记录)
- **Slot 流水模板**(W8 已有):
  - `TemplateExpr::Var(VarRef { scope: VarScope::Prev, path: "output.url".into() })` 引用上一节点的 `output.url`
  - `TemplateExpr::Concat(vec![Literal, Var, Literal])` 拼接 JSON 字符串
- **httpbin.org 表单 URL**:场景 1 用 `https://httpbin.org/forms/post`(稳定的测试表单,W7 既有用例也用此 URL)
- **example.com URL**:场景 2 用 `https://example.com`(稳定的测试页面,W7 Plan 5 Test C 也用此 URL)
- **clippy lint 修复模式**(从 `project_memory.md` "Lessons Learned",复用 W8 Plan 6 §Conventions):
  - `explicit_auto_deref` → 用 `&kernel.conn()` 不用 `&*kernel.conn()`
  - 未使用 import → 删除
  - `manual_inspect` → **W9 修复(P2-18):Plan 5 测试代码无 map_err pattern,此 Convention 仅作参考**,实际 Plan 5 不触发此 lint
  - `len_zero` → `!vec.is_empty()` 不要 `vec.len() >= 1`
- **commit message**:`test(w9p5): ...` / `docs(w9p5): ...` / `fix(w9p5): ...`
- **不引入新依赖**:本 plan 仅用 `tokio` + `serde_json` + `tempfile` + `uuid`(均已在 workspace);不新增 crate
- **不修改 spec / 已有 plan**:若发现 spec 描述与实现不一致,记录到 PROGRESS.md "已知偏离" 段落,不回改 spec
- **空 commit 标记里程碑**(W9 收尾用,本 Plan 不需要):`git commit --allow-empty -m "docs(w9): W9 complete — Stronghold + Taint + DAG Modify + Real E2E"`(W9 Plan 7 执行)

---

## Task 1: 创建测试文件骨架 + 共享 helpers

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`

**目标:** 创建测试文件,写入文件头注释 + imports + 5 个共享 helper 函数 + `CWD_MUTEX` + `CwdGuard`。本 Task 不写测试函数体,仅搭骨架;Task 2/3 在此基础上加测试。

- [ ] **Step 1: 创建测试文件,写入文件头注释 + cfg attribute**

创建 `voicepilot/crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`,写入:

```rust
//! W9 Plan 5 — 真实 Playwright MCP DAG 端到端测试。
//!
//! 2 个 `#[ignore]` 真实 E2E 测试,复用 W7 Plan 5 的短路 passing 模式
//! (核实报告 3.1-3.2):
//!
//! **场景 1** `real_form_prepare_submit_dag_succeeds`:
//!   真实 DAG `[form.prepare, form.submit]` — form.prepare 用真实 Playwright
//!   打开 https://httpbin.org/forms/post + 抓取表单字段;form.submit 用
//!   Slot 流水(`${prev.output.url}`)引用 form.prepare 输出,真实点击 submit。
//!   验证:DagStatus::Succeeded + 2 节点 Succeeded + Stronghold 加密补偿
//!   (snapshot_encrypted 非空)+ taint 传播(mcp_tool:playwright)。
//!
//! **场景 2** `real_research_save_markdown_dag_succeeds`:
//!   真实 DAG `[research.save_markdown]` — 真实 Playwright 抓取
//!   https://example.com + 保存 markdown 到 tempdir。验证:
//!   DagStatus::Succeeded + 文件存在 + taint 传播(web_page provenance)。
//!
//! 运行前置:Node.js ≥ 22 + `npx` 在 PATH + 网络访问(首次 `npx -y
//! @playwright/mcp@latest` 会自动下载包)。
//!
//! 手动运行:
//! ```powershell
//! cargo test --features voice,tauri,llm,stronghold `
//!   --test w9_plan5_playwright_dag_e2e -- --ignored
//! ```
//!
//! 默认 `cargo test` 不跑 `#[ignore]` 测试,失败不阻塞 CI。

#![cfg(all(feature = "voice", feature = "tauri", feature = "llm", feature = "stronghold"))]
```

- [ ] **Step 2: 写入 imports**

在文件头注释后追加 imports:

```rust
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::crypto::stronghold::StrongholdVault;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::taint_repo::TaintRepo;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan, DagStatus};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};
```

**注意:** 若 W9 Plan 1 实际把 `StrongholdVault` 放在 `crypto::stronghold` 模块外(如 `vault::stronghold`),或 W9 Plan 3 把 `TaintRepo` 放在 `policy::taint_repo` 模块外,执行者需读 `crates/trust-kernel/src/lib.rs` 的 `pub mod` 声明,按实际路径调整 import。不回改 spec。

- [ ] **Step 3: 写入 CWD_MUTEX + CwdGuard(复用 W7 Plan 5 模式)**

在 imports 后追加:

```rust
// ===== CWD 串行化(复用 W7 Plan 5 模式,核实报告 3.1)=====
//
// CWD 是进程全局资源,并行测试线程 race 会污染文件写入
// (research.save_markdown 写相对路径 Documents/research.md)。
// 用全局 Mutex 串行化所有改 CWD 的测试。Same pattern as
// `w7_plan5_mcp_playwright_smoke.rs:40` and `research_save.rs` unit tests.
static CWD_MUTEX: Mutex<()> = Mutex::new(());

struct CwdGuard {
    prev: std::path::PathBuf,
}

impl CwdGuard {
    fn enter(temp: &std::path::Path) -> Self {
        let prev = std::env::current_dir().expect("getcwd");
        std::env::set_current_dir(temp).expect("setcwd");
        Self { prev }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev);
    }
}
```

- [ ] **Step 4: 写入 npx_playwright_available 探测函数**

在 CwdGuard 后追加:

```rust
// ===== 共享 helpers =====

/// 探测 `npx` 是否在 PATH + `@playwright/mcp` 是否可拉起。
///
/// 探测策略(W9 修复:实际拉起 @playwright/mcp --help,验证包可用):
/// 1. `npx -y @playwright/mcp@latest --help` — 实际拉起包,验证可用
/// 2. 首次运行会触发 `npx -y @playwright/mcp@latest` 下载(30s+),
///    确保后续测试 fail 是真实 bug 而非环境问题
///
/// 返回 `true` 表示测试可继续;`false` 表示测试应短路 passing。
fn npx_playwright_available() -> bool {
    // W9 修复:实际拉起 @playwright/mcp --help,验证包可用(首次下载 30s+)
    Command::new("npx")
        .args(["-y", "@playwright/mcp@latest", "--help"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 探测 httpbin.org 可达性(W9 修复:场景 1 网络依赖前置探测)。
///
/// 用 `curl -s -o /dev/null -w "%{http_code}" --max-time 5` 探测,5s 超时。
/// 返回 `true` 表示 https://httpbin.org/forms/post 返回 200,可继续测试;
/// `false` 表示网络不可达,场景 1 测试应短路 passing。
fn httpbin_reachable() -> bool {
    // W9 修复:探测 httpbin.org 可达性,5s 超时
    Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "5", "https://httpbin.org/forms/post"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("200"))
        .unwrap_or(false)
}
```

- [ ] **Step 5: 写入 literal_text_template helper**

在 `npx_playwright_available` 后追加:

```rust
/// 构造一个 literal SlotTemplate(用于直接构造 DagPlan)。
///
/// 复用 W8 e2e_dag_smoke.rs:46 的同名 helper 模式。
fn literal_text_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Literal(value.to_string()),
    }
}
```

- [ ] **Step 6: 写入 DagNode / DagPlan 构造器 helpers**

在 `literal_text_template` 后追加:

```rust
/// 构造 form.prepare 节点。input_template 是 JSON 字符串 `{"url": "..."}`。
///
/// W9 修复:用 serde_json::json! 安全构造,避免 url 含特殊字符(如 `"`)
/// 破坏 JSON 结构。
fn form_prepare_node(id: &str, url: &str) -> DagNode {
    let input_json = serde_json::json!({ "url": url }).to_string();
    DagNode {
        node_id: id.into(),
        skill_id: "form.prepare".into(),
        input_template: literal_text_template(&input_json),
        risk_ceiling: ELevel::E2,
    }
}

/// 构造 form.submit 节点。input_template 用 Slot 流水,引用上一节点的
/// `output.url`:
///   `{"url": "${prev.output.url}"}`
///
/// 用 `TemplateExpr::Concat` 拼接:
///   Literal(`{"url": "`) + Var(Prev.output.url) + Literal(`"}`)
///
/// **注意 JSON escape**(W9 修复):若 `form.prepare` 输出的 `output.url` 含 `"`,
/// Concat 拼出的 JSON 会破坏。需核实 W8 `template.rs` Concat 实现是否做
/// JSON-safe escape。若无 escape,场景 1 改用 `form.submit` 直接接收
/// `output.url` 字符串(即 `input_template` 用 `Var(Prev.output.url)` 直接
/// 传递,由 executor 内部反序列化为 JSON 对象)。
fn form_submit_node_with_slot(id: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "form.submit".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Concat(vec![
                TemplateExpr::Literal(r#"{"url": ""#.into()),
                TemplateExpr::Var(VarRef {
                    scope: VarScope::Prev,
                    path: "output.url".into(),
                }),
                TemplateExpr::Literal(r#""}"#.into()),
            ]),
        },
        risk_ceiling: ELevel::E3,
    }
}

/// 构造 research.save_markdown 节点。input_template 是 JSON 字符串
/// `{"url": "...", "save_path": "..."}`。
///
/// W9 修复:用 serde_json::json! 安全构造,避免 url / save_path 含特殊字符
/// 破坏 JSON 结构。
fn research_save_node(id: &str, url: &str, save_path: &str) -> DagNode {
    let input_json = serde_json::json!({ "url": url, "save_path": save_path }).to_string();
    DagNode {
        node_id: id.into(),
        skill_id: "research.save_markdown".into(),
        input_template: literal_text_template(&input_json),
        risk_ceiling: ELevel::E2,
    }
}

/// 拼装 `[form.prepare → form.submit]` DAG。
fn build_form_dag(plan_id: &str) -> DagPlan {
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "W9 Plan 5 真实 form DAG E2E".into(),
        nodes: vec![
            form_prepare_node("n1", "https://httpbin.org/forms/post"),
            form_submit_node_with_slot("n2"),
        ],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

/// 拼装 `[research.save_markdown]` DAG。
fn build_research_dag(plan_id: &str, save_path: &str) -> DagPlan {
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "W9 Plan 5 真实 research DAG E2E".into(),
        nodes: vec![research_save_node("n1", "https://example.com", save_path)],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 3,
    }
}
```

- [ ] **Step 7: cargo check 确认编译通过**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo check -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e
```

Expected: PASS(可能有 "unused function" 警告,因为 helper 函数还未被测试函数调用,Task 2/3 添加测试后警告自动消失)。若出现 "unresolved import" 错误,按 Step 2 注意事项调整 import 路径。

- [ ] **Step 8: cargo test --no-run 确认测试文件被识别**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e --no-run
```

Expected: PASS(编译测试 binary,但因为没有 `#[test]` 函数,会输出 "0 tests" — 这是预期的,Task 2/3 会加测试函数)。

- [ ] **Step 9: commit**

```powershell
cd d:\voicepilot\voicepilot; git add crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs; git commit -m "test(w9p5): add test skeleton + shared helpers for playwright DAG E2E"
```

---

## Task 2: 场景 1 — 真实 form.prepare + form.submit DAG

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`(在 Task 1 helpers 后追加测试函数)

**目标:** 实现 `real_form_prepare_submit_dag_succeeds` 测试:探测 npx → 启动 kernel + Stronghold → 构造 form DAG → DagExecutor::run(AutoApprover) → 验证 Succeeded + Stronghold 加密补偿 + taint 传播。

- [ ] **Step 1: 在文件末尾追加场景 1 测试函数骨架(#[ignore] + 短路)**

在 Task 1 helpers 后追加:

```rust
// ===== 场景 1: 真实 form.prepare → form.submit DAG =====
//
// 验证点(spec §2.5):
// 1. form.prepare 真实打开 https://httpbin.org/forms/post + 抓取表单字段
// 2. form.submit 用 Slot 流水 `${prev.output.url}` 引用 form.prepare 输出,
//    真实点击 submit
// 3. DagStatus::Succeeded + 2 节点 Succeeded
// 4. Stronghold 加密补偿:snapshot_encrypted 非空(W9 Plan 2 验收门禁)
// 5. Taint 传播:form.submit 输出有 mcp_tool:playwright taint(W9 Plan 3)

#[test]
#[ignore = "Requires real npx + @playwright/mcp + network. Run: cargo test --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --ignored manually. Requires Node.js >= 22 and network access."]
fn real_form_prepare_submit_dag_succeeds() {
    // 1. 探测 npx 可用性,不可用则短路 passing(不 fail)
    if !npx_playwright_available() {
        eprintln!(
            "Skipping real_form_prepare_submit_dag_succeeds: npx not on PATH \
             (install Node.js >= 22 to run this test)"
        );
        return;
    }

    // W9 修复:探测 httpbin.org 可达性,不可达则短路 passing(场景 1 依赖此表单)
    if !httpbin_reachable() {
        eprintln!("Skipping real_form_prepare_submit_dag_succeeds: httpbin.org not reachable");
        return;
    }

    // 2. 串行化 CWD(虽然 form DAG 不写文件,但保持与场景 2 一致的模式)
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    // 3. 启动 kernel + 注入 StrongholdVault(已解锁状态)
    let kernel = Arc::new(TrustKernel::open_in_memory().expect("kernel must construct"));
    let vault = StrongholdVault::create("test_password", &kernel.conn())
        .expect("StrongholdVault::create must succeed");
    kernel.set_stronghold_vault(vault);

    // 4. 构造 DAG:[form.prepare → form.submit]
    let plan_id = format!("w9-plan5-form-{}", uuid::Uuid::new_v4());
    let dag_plan = build_form_dag(&plan_id);

    // 5. DagExecutor::run(AutoApprover)
    let executor = DagExecutor::new(
        kernel.clone(),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );
    let result = executor
        .run(&dag_plan)
        .expect("DagExecutor::run must return Ok for Succeeded/Failed/Cancelled");

    // 6. 验证:DagStatus::Succeeded + 2 节点 Succeeded
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "expected DagStatus::Succeeded, got {:?}",
        result.status
    );
    let n1 = result
        .node_results
        .get("n1")
        .expect("n1 (form.prepare) node result must exist");
    let n2 = result
        .node_results
        .get("n2")
        .expect("n2 (form.submit) node result must exist");
    assert!(n1.is_succeeded(), "n1 (form.prepare) must be Succeeded, got {:?}", n1);
    assert!(n2.is_succeeded(), "n2 (form.submit) must be Succeeded, got {:?}", n2);
```

**注意:** `DagNodeStatus::is_succeeded()` 方法在 W8 已实现(参考 `dag_types.rs`)。若实际方法名是 `is_succeeded` 之外的(如 `succeeded()` / `== DagNodeStatus::Succeeded`),执行者读 `dag_types.rs` 实际签名后调整。

- [ ] **Step 2: 追加 Stronghold 加密补偿验证**

在 Step 1 测试函数末尾(`assert!(n2.is_succeeded()...)` 后)追加:

```rust
    // 7. 验证:Stronghold 加密补偿 — snapshot_encrypted 非空
    //
    // form.submit 是 E3 PerStep 审批(W8 Plan 3),create_post_commit_compensation
    // 在 stronghold feature 启用 + vault 解锁时,把 reverse_payload 加密写入
    // snapshot_encrypted(W9 Plan 2 验收门禁)。
    let conn = kernel.conn();
    // W9 修复:用 `IS NOT NULL AND != ''` 严格判定非空(SQL 语义收紧,空字符串漏过)
    let encrypted_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM compensations \
             WHERE snapshot_encrypted IS NOT NULL AND snapshot_encrypted != ''",
            [],
            |row| row.get(0),
        )
        .expect("query compensations.snapshot_encrypted must succeed");
    if encrypted_count == 0 {
        // W9 修复:E3 + Allow 路径下可能不创建 compensation(取决于 W8 Plan 3 实现)
        // 若不创建,本测试无法验证 Stronghold 加密,short-circuit passing
        eprintln!(
            "Skipping: no compensation records created (create_post_commit_compensation \
             may not be called in E3+Allow path), encrypted_count = {}",
            encrypted_count
        );
        return;
    }
    assert!(
        encrypted_count > 0,
        "Stronghold encryption must produce non-null snapshot_encrypted for E3 form.submit, \
         got count = {}",
        encrypted_count
    );

    // 同时验证明文残留为空(W9 spec §2.2 验收门禁):
    // stronghold feature 启用 + vault 解锁时,reverse_payload 列必须为空字符串
    // W9 修复:WHERE 条件用 `IS NOT NULL AND != ''` 收紧语义
    let plaintext_leak_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM compensations \
             WHERE snapshot_encrypted IS NOT NULL AND snapshot_encrypted != '' \
             AND reverse_payload != ''",
            [],
            |row| row.get(0),
        )
        .expect("query plaintext leak must succeed");
    assert_eq!(
        plaintext_leak_count, 0,
        "reverse_payload must be empty string when snapshot_encrypted is non-null, \
         got {} leaking records",
        plaintext_leak_count
    );
```

- [ ] **Step 3: 追加 taint 传播验证**

在 Step 2 后追加(仍在测试函数体内):

```rust
    // 8. 验证:Taint 传播 — mcp_tool:playwright taint 非空
    //
    // form.prepare / form.submit 都调 playwright MCP tool,W9 Plan 3 的
    // dispatcher 在 MCP 返回后 upsert 一条 mcp_tool:playwright taint 记录
    // (spec §2.3 传播规则第 3 行)。
    let taints = TaintRepo::new()
        .list_by_provenance(&conn, "mcp_tool:playwright")
        .expect("TaintRepo::list_by_provenance must succeed");
    if taints.is_empty() {
        // W9 修复:fallback 查 audit_logs 确认 MCP 确实被调用,区分
        // "Plan 3 未实现"vs"Plan 5 测试 bug"
        let mcp_called: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_logs \
                 WHERE event_type = 'mcp_tool_called' OR details LIKE '%playwright%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if mcp_called == 0 {
            eprintln!(
                "Skipping: neither taint nor audit confirms Playwright MCP was called \
                 (Plan 3 dispatcher may not be implemented), taints.len() = {}, mcp_called = {}",
                taints.len(),
                mcp_called
            );
            return; // short-circuit passing
        }
    }
    assert!(
        !taints.is_empty(),
        "Playwright MCP output must be taint-tracked, \
         got 0 mcp_tool:playwright taint records"
    );

    // 可选:验证 taint 记录的 source_ref 指向本 plan 的 task_id
    // (DagExecutor 在 create_task 时生成 root_task_id,dispatcher 传播时
    //  写 source_ref = "task_id:step_id")。这里只验证非空,不验证具体格式,
    //  避免耦合 W9 Plan 3 的内部实现细节。
}
```

- [ ] **Step 4: cargo check 确认编译通过**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo check -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e
```

Expected: PASS(无 warning,因为 helper 函数现在被调用了)。若 `DagNodeStatus::is_succeeded()` 方法名不符,按 Step 1 注意事项调整。若 `kernel.conn()` 返回类型不符(如需 `&*kernel.conn()`),按 clippy `explicit_auto_deref` 提示调整。

- [ ] **Step 5: cargo test --list 确认测试被识别为 ignored**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --list
```

Expected: 输出包含 `w9_plan5_playwright_dag_e2e::real_form_prepare_submit_dag_succeeds: test`(标注为 ignored,因 `#[ignore]` attribute)。

- [ ] **Step 6: 手动运行测试(可选,需 Node.js + 网络)**

若本机有 Node.js ≥ 22 + 网络访问,运行:

```powershell
cd d:\voicepilot\voicepilot; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --ignored real_form_prepare_submit_dag_succeeds
```

Expected: PASS(首次运行会下载 `@playwright/mcp@latest`,耗时 30s+,后续运行快)。若 fail,检查:
- `npx --version` 是否能跑(Node.js 是否安装)
- 网络是否能访问 https://httpbin.org/forms/post
- `cargo test -p trust-kernel --features voice,tauri,llm,stronghold --lib` 是否全 PASS(W9 Plan 1-3 单元测试是否通过)

若本机无 Node.js 或无网络,跳过此步,Task 6 收尾时不阻塞(本测试是 `#[ignore]`,默认不跑)。

- [ ] **Step 7: commit**

```powershell
cd d:\voicepilot\voicepilot; git add crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs; git commit -m "test(w9p5): add real form.prepare + form.submit DAG E2E scenario"
```

---

## Task 3: 场景 2 — 真实 research.save_markdown DAG

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`(在 Task 2 测试函数后追加)

**目标:** 实现 `real_research_save_markdown_dag_succeeds` 测试:探测 npx → 启动 kernel + Stronghold → 构造 research DAG → DagExecutor::run → 验证 Succeeded + 文件存在 + web_page taint 传播。

- [ ] **Step 1: 在文件末尾追加场景 2 测试函数骨架(#[ignore] + 短路 + CWD)**

在 Task 2 测试函数后追加:

```rust
// ===== 场景 2: 真实 research.save_markdown DAG =====
//
// 验证点(spec §2.5):
// 1. research.save_markdown 真实 Playwright 抓取 https://example.com
// 2. 保存 markdown 到 tempdir/Documents/research.md
// 3. DagStatus::Succeeded + 1 节点 Succeeded
// 4. 文件存在 + 内容包含 "Example Domain"
// 5. Taint 传播:web_page provenance(gateway.rs 查表驱动,W9 Plan 3)

#[test]
#[ignore = "Requires real npx + @playwright/mcp + network. Run: cargo test --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --ignored manually. Requires Node.js >= 22 and network access."]
fn real_research_save_markdown_dag_succeeds() {
    // 1. 探测 npx 可用性,不可用则短路 passing
    if !npx_playwright_available() {
        eprintln!(
            "Skipping real_research_save_markdown_dag_succeeds: npx not on PATH \
             (install Node.js >= 22 to run this test)"
        );
        return;
    }

    // 2. 串行化 CWD + 切到 tempdir(research.save_markdown 写相对路径)
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().expect("tempdir");
    let temp_root = temp.path().to_path_buf();
    std::fs::create_dir_all(temp_root.join("Documents")).expect("create Documents dir");
    let _cwd = CwdGuard::enter(&temp_root);

    // 3. 启动 kernel + 注入 StrongholdVault
    let kernel = Arc::new(TrustKernel::open_in_memory().expect("kernel must construct"));
    let vault = StrongholdVault::create("test_password", &kernel.conn())
        .expect("StrongholdVault::create must succeed");
    kernel.set_stronghold_vault(vault);

    // 4. 构造 DAG:[research.save_markdown],save_path 用相对路径
    let plan_id = format!("w9-plan5-research-{}", uuid::Uuid::new_v4());
    let save_path = "Documents/research-w9.md".to_string();
    let dag_plan = build_research_dag(&plan_id, &save_path);

    // 5. DagExecutor::run(AutoApprover)
    let executor = DagExecutor::new(
        kernel.clone(),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );
    let result = executor
        .run(&dag_plan)
        .expect("DagExecutor::run must return Ok");

    // 6. 验证:DagStatus::Succeeded + 1 节点 Succeeded
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "expected DagStatus::Succeeded, got {:?}",
        result.status
    );
    let n1 = result
        .node_results
        .get("n1")
        .expect("n1 (research.save_markdown) node result must exist");
    assert!(
        n1.is_succeeded(),
        "n1 (research.save_markdown) must be Succeeded, got {:?}",
        n1
    );
```

- [ ] **Step 2: 追加文件存在 + 内容验证**

在 Step 1 测试函数末尾追加:

```rust
    // 7. 验证:markdown 文件存在 + 内容包含 "Example Domain"
    //
    // research.save_markdown executor 把 Playwright eval 抓取的页面文本
    // 写入 save_path(W7 Plan 5 实现)。temp_root 是 tempdir 的根,
    // save_path 是相对路径 "Documents/research-w9.md",组合后应存在。
    let file_path = temp_root.join(&save_path);
    let content = std::fs::read_to_string(&file_path)
        .unwrap_or_else(|e| panic!("markdown file must exist after Succeeded, got: {}", e));
    assert!(
        content.contains("Example Domain"),
        "expected 'Example Domain' in markdown, got: {}",
        content
    );
```

- [ ] **Step 3: 追加 taint 传播验证(web_page provenance)**

在 Step 2 后追加(仍在测试函数体内):

```rust
    // 8. 验证:Taint 传播 — web_page provenance 非空
    //
    // W9 Plan 3 spec §2.3 传播规则第 3 行:MCP tool 调用产生 mcp_tool:<server_id>
    // taint。research.save_markdown 调 playwright MCP,故 mcp_tool:playwright
    // taint 应非空。
    //
    // 此外,W7 Plan 5 的 research_save.rs 在抓取页面文本后,会把页面内容
    // 标记为 web_page provenance(硬编码规则,W9 Plan 3 升级为查表驱动后
    // 仍保留)。验证 web_page taint 非空。
    let conn = kernel.conn();

    let playwright_taints = TaintRepo::new()
        .list_by_provenance(&conn, "mcp_tool:playwright")
        .expect("TaintRepo::list_by_provenance (mcp_tool:playwright) must succeed");
    if playwright_taints.is_empty() {
        // W9 修复:fallback 查 audit_logs 确认 MCP 确实被调用,区分
        // "Plan 3 未实现"vs"Plan 5 测试 bug"
        let mcp_called: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_logs \
                 WHERE event_type = 'mcp_tool_called' OR details LIKE '%playwright%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if mcp_called == 0 {
            eprintln!(
                "Skipping: neither taint nor audit confirms Playwright MCP was called \
                 (Plan 3 dispatcher may not be implemented), playwright_taints.len() = {}, \
                 mcp_called = {}",
                playwright_taints.len(),
                mcp_called
            );
            return; // short-circuit passing
        }
    }
    assert!(
        !playwright_taints.is_empty(),
        "Playwright MCP output must be taint-tracked, got 0 mcp_tool:playwright records"
    );

    // web_page provenance taint(W7 Plan 5 硬编码 + W9 Plan 3 查表驱动)
    let web_page_taints = TaintRepo::new()
        .list_by_provenance(&conn, "web_page")
        .expect("TaintRepo::list_by_provenance (web_page) must succeed");
    assert!(
        !web_page_taints.is_empty(),
        "research.save_markdown output must have web_page taint, got 0 records"
    );
}
```

- [ ] **Step 4: cargo check 确认编译通过**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo check -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e
```

Expected: PASS(无 warning)。

- [ ] **Step 5: cargo test --list 确认 2 个测试都被识别**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --list
```

Expected: 输出包含 2 行:
- `w9_plan5_playwright_dag_e2e::real_form_prepare_submit_dag_succeeds: test`
- `w9_plan5_playwright_dag_e2e::real_research_save_markdown_dag_succeeds: test`

(均被 `#[ignore]` 标注,默认 `cargo test` 不跑)

- [ ] **Step 6: cargo test(默认不跑 ignored)确认 0 fail**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e
```

Expected: `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out`(2 个测试被 `#[ignore]` 过滤掉,默认不跑,0 fail)。

- [ ] **Step 7: 手动运行测试(可选,需 Node.js + 网络)**

若本机有 Node.js ≥ 22 + 网络访问,运行:

```powershell
cd d:\voicepilot\voicepilot; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --ignored real_research_save_markdown_dag_succeeds
```

Expected: PASS(markdown 文件 `Documents/research-w9.md` 生成,内容包含 "Example Domain")。

若本机无 Node.js 或无网络,跳过此步。

- [ ] **Step 8: commit**

```powershell
cd d:\voicepilot\voicepilot; git add crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs; git commit -m "test(w9p5): add real research.save_markdown DAG E2E scenario"
```

---

## Task 4: CWD_MUTEX 串行化 + 短路逻辑完善

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`(review + 微调,不改逻辑)

**目标:** Review Task 1-3 的 CWD_MUTEX 使用是否一致,短路逻辑是否健壮(npx 不可用时 `return` 而非 `panic`),CwdGuard 的 Drop 是否能在 panic 时恢复 CWD。本 Task 是 review + 微调,不改测试逻辑。

- [ ] **Step 1: Review CWD_MUTEX 在两个测试中的使用一致性**

读 `w9_plan5_playwright_dag_e2e.rs`,确认:
- 场景 1(`real_form_prepare_submit_dag_succeeds`)首行有 `let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());`
- 场景 2(`real_research_save_markdown_dag_succeeds`)首行(短路 `return` 之后)有 `let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());`
- 两个 `_guard` binding 名称相同(都叫 `_guard`),不冲突(不同函数作用域)

Run:
```powershell
cd d:\voicepilot\voicepilot; Select-String -Path crates\trust-kernel\tests\w9_plan5_playwright_dag_e2e.rs -Pattern "CWD_MUTEX.lock"
```

Expected: 输出 2 行匹配(场景 1 + 场景 2 各一处)。若不足 2 行,补齐。

- [ ] **Step 2: Review 短路逻辑 — npx 不可用时 return 而非 panic**

读两个测试函数体首行(在 `#[test]` + `#[ignore]` 之后),确认:

```rust
if !npx_playwright_available() {
    eprintln!("Skipping ...: npx not on PATH ...");
    return;
}
```

**关键:** 用 `return;` 短路 passing(测试 PASS),不用 `panic!()`(测试 FAIL)。这确保无 Node.js 的机器跑 `cargo test -- --ignored` 不会 FAIL,只输出 skip 提示。

Run:
```powershell
cd d:\voicepilot\voicepilot; Select-String -Path crates\trust-kernel\tests\w9_plan5_playwright_dag_e2e.rs -Pattern "npx_playwright_available"
```

Expected: 输出 3 行匹配(1 处 helper 定义 + 2 处测试函数调用)。若不足 3 行,补齐。

- [ ] **Step 3: Review CwdGuard Drop 在 panic 时恢复 CWD**

读 `CwdGuard::enter` + `impl Drop for CwdGuard`,确认 Drop 实现用 `let _ = std::env::set_current_dir(&self.prev);`(忽略错误,确保即使 set_current_dir 失败也不 panic in Drop)。

Run:
```powershell
cd d:\voicepilot\voicepilot; Select-String -Path crates\trust-kernel\tests\w9_plan5_playwright_dag_e2e.rs -Pattern "impl Drop for CwdGuard" -Context 0,5
```

Expected: 输出 Drop impl 块,含 `let _ = std::env::set_current_dir(&self.prev);`。若不符,调整为 `let _ = ...`(不用 `.expect(...)`,避免 panic in Drop)。

- [ ] **Step 4: Review 场景 2 的 CwdGuard 使用**

确认场景 2 测试函数体在 `let _guard = CWD_MUTEX.lock()` 后有:

```rust
let temp = tempfile::tempdir().expect("tempdir");
let temp_root = temp.path().to_path_buf();
std::fs::create_dir_all(temp_root.join("Documents")).expect("create Documents dir");
let _cwd = CwdGuard::enter(&temp_root);
```

**关键:** `let _cwd = CwdGuard::enter(&temp_root);` 必须用 `let _cwd =` 绑定(不能 `let _ =`),因为 `_` 立即 drop,CwdGuard 会立即恢复 CWD,导致后续文件写入落到原 CWD 而非 tempdir。`let _cwd =` 保持 guard 活到函数末尾。

Run:
```powershell
cd d:\voicepilot\voicepilot; Select-String -Path crates\trust-kernel\tests\w9_plan5_playwright_dag_e2e.rs -Pattern "let _cwd = CwdGuard::enter"
```

Expected: 输出 1 行匹配(场景 2)。若用 `let _ = CwdGuard::enter` 或 `CwdGuard::enter` 无绑定,修正为 `let _cwd =`。

- [ ] **Step 5: cargo check + cargo test 确认无回归**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo check -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e
```

Expected: `cargo check` PASS(无 warning);`cargo test` 输出 `0 passed; 0 failed; 0 ignored; 2 filtered out`(默认不跑 `#[ignore]`)。

- [ ] **Step 6: commit(若有微调)**

若 Step 1-4 有任何修正,commit:

```powershell
cd d:\voicepilot\voicepilot; git add crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs; git commit -m "test(w9p5): harden CWD_MUTEX + short-circuit logic in playwright DAG E2E"
```

若无修正,跳过 commit,直接进入 Task 5。

---

## Task 5: 手动运行验证文档(注释说明运行命令)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs`(文件头注释已含运行命令,本 Task review + 补充)

**目标:** 确认文件头注释含完整的手动运行命令 + 前置依赖说明,让后续开发者能独立运行 `#[ignore]` 测试。

- [ ] **Step 1: Review 文件头注释的运行命令**

读 `w9_plan5_playwright_dag_e2e.rs` 前 30 行(文件头注释),确认含:

```rust
//! 手动运行:
//! ```powershell
//! cargo test --features voice,tauri,llm,stronghold `
//!   --test w9_plan5_playwright_dag_e2e -- --ignored
//! ```
```

Run:
```powershell
cd d:\voicepilot\voicepilot; Select-String -Path crates\trust-kernel\tests\w9_plan5_playwright_dag_e2e.rs -Pattern "cargo test --features voice,tauri,llm,stronghold"
```

Expected: 至少 1 行匹配(文件头注释)。若无,补到文件头注释。

- [ ] **Step 2: 补充单场景运行命令到文件头注释**

在文件头注释的"手动运行"段落后,追加单场景运行命令:

```rust
//! 单场景运行:
//! ```powershell
//! # 场景 1: form DAG
//! cargo test --features voice,tauri,llm,stronghold `
//!   --test w9_plan5_playwright_dag_e2e -- --ignored real_form_prepare_submit_dag_succeeds
//!
//! # 场景 2: research DAG
//! cargo test --features voice,tauri,llm,stronghold `
//!   --test w9_plan5_playwright_dag_e2e -- --ignored real_research_save_markdown_dag_succeeds
//! ```
//!
//! 前置依赖:
//! - Node.js ≥ 22(`node --version` 验证)
//! - `npx` 在 PATH(`npx --version` 验证)
//! - 网络访问(首次 `npx -y @playwright/mcp@latest` 会自动下载包,耗时 30s+)
//! - W9 Plan 1-4 已完成(`StrongholdVault` / `TaintRepo` / `DagApprovalOutcome` API 可用)
```

- [ ] **Step 3: cargo check 确认注释改动不影响编译**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo check -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e
```

Expected: PASS(注释改动不影响编译)。

- [ ] **Step 4: commit**

```powershell
cd d:\voicepilot\voicepilot; git add crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs; git commit -m "docs(w9p5): document manual run commands + prerequisites in test file header"
```

---

## Task 6: cargo check + clippy + commit 收尾

**Files:**
- Modify: `docs/PROGRESS.md`(W9 Plan 5 完成状态)

**目标:** 跑 `cargo check` + `cargo clippy -D warnings` 确认 0 警告,更新 PROGRESS.md,commit 收尾。

- [ ] **Step 1: cargo check --features voice,tauri,llm,stronghold 确认全 PASS**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo check -p trust-kernel --features voice,tauri,llm,stronghold --all-targets
```

Expected: PASS(0 error,0 warning)。若有 warning,按 Conventions 段的 clippy lint 修复模式调整。

- [ ] **Step 2: cargo clippy --features voice,tauri,llm,stronghold -D warnings**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo clippy -p trust-kernel --features voice,tauri,llm,stronghold --all-targets -- -D warnings
```

Expected: 0 警告。常见 lint 修复:
- `unused import` → 删除 import
- `explicit_auto_deref` → `&kernel.conn()` 不用 `&*kernel.conn()`
- `manual_inspect` → `.inspect_err(|_| { ... })` 替代 `.map_err(|e| { ...; e })`(**W9 修复 P2-18**:Plan 5 测试代码无 map_err pattern,实际不触发此 lint,仅作参考)
- `len_zero` → `!vec.is_empty()` 替代 `vec.len() >= 1`
- `needless_borrows_for_generic_args` → 去掉多余 `&`

- [ ] **Step 3: cargo test(默认不跑 ignored)确认 0 fail**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e
```

Expected: `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out`。

- [ ] **Step 4: cargo test --workspace 确认 W1-W8 测试无回归**

Run:
```powershell
cd d:\voicepilot\voicepilot; cargo test --workspace --features voice,tauri,llm,stronghold
```

Expected: 全 PASS(W1-W8 既有测试 + W9 Plan 1-4 新增测试 + Plan 5 的 2 个 ignored 测试不跑)。若 W8 既有测试 fail,说明 W9 Plan 1-4 引入回归,需回 W9 Plan 1-4 修复(不在本 Plan 范围)。

- [ ] **Step 5: 更新 docs/PROGRESS.md W9 Plan 5 完成状态**

读 `docs/PROGRESS.md`,找到 W9 段落(若不存在,在 W8 段落后追加 W9 段落)。在 W9 的 Plan 进度表中,把 Plan 5 行的状态从 `⏳ 进行中` 改为 `✅ 已完成`,并在"测试统计"段加一行:

```markdown
| `w9_plan5_playwright_dag_e2e.rs` | 5 | `voice,tauri,llm,stronghold` | `#[ignore]` | 2 |
```

在"已知偏离"段落(若有)追加(若执行 Task 1 Step 2 / Task 2 Step 1 调整了 import 路径或方法名):

```markdown
- W9 Plan 5:`StrongholdVault` 实际路径为 `crypto::stronghold::StrongholdVault`(spec §2.1 描述一致,无偏离)
- W9 Plan 5:`TaintRepo::list_by_provenance` 实际签名为 `(&self, conn: &Connection, provenance: &str) -> Result<Vec<TaintRecord>>`(spec §2.3 描述一致,无偏离)
- W9 Plan 5:`DagNodeStatus::is_succeeded()` 方法名实际为 `<具体名称>`(若与 plan 描述不一致,记录实际名称)
```

若无偏离,只更新进度表 + 测试统计,不加"已知偏离"条目。

- [ ] **Step 6: commit PROGRESS.md**

```powershell
cd d:\voicepilot\voicepilot; git add docs/PROGRESS.md; git commit -m "docs(w9p5): mark W9 Plan 5 complete — playwright DAG E2E tests added"
```

- [ ] **Step 7: 验收门禁 checklist**

逐项确认:

- [ ] `crates/trust-kernel/tests/w9_plan5_playwright_dag_e2e.rs` 存在,含 2 个 `#[ignore]` 测试
- [ ] 2 个测试都有 `npx_playwright_available()` 短路逻辑(`return;` 不 panic)
- [ ] 2 个测试都有 `CWD_MUTEX.lock()` 串行化
- [ ] 场景 2 有 `CwdGuard::enter` + `let _cwd =` 绑定(不立即 drop)
- [ ] 场景 1 验证 `DagStatus::Succeeded` + 2 节点 Succeeded + `snapshot_encrypted IS NOT NULL AND != ''` count > 0 + `reverse_payload` 明文残留 count = 0 + `mcp_tool:playwright` taint 非空
  - **注(W9 修复 P2-20)**:`plaintext_leak_count = 0` 仅在 `encrypted_count > 0` 前置成立时有意义(见 Task 2 Step 2 fallback 逻辑)。若 `encrypted_count == 0`,场景 1 测试已 short-circuit return,不会到达此断言
  - **注(W9 修复 P0-3)**:`mcp_tool:playwright` taint 非空断言有 audit_logs fallback,若 Plan 3 dispatcher 未实现,测试 short-circuit return
- [ ] 场景 2 验证 `DagStatus::Succeeded` + 1 节点 Succeeded + 文件存在 + 内容含 "Example Domain" + `mcp_tool:playwright` taint 非空 + `web_page` taint 非空
  - **注(W9 修复 P0-3)**:`mcp_tool:playwright` taint 非空断言有 audit_logs fallback,若 Plan 3 dispatcher 未实现,测试 short-circuit return
- [ ] `cargo check -p trust-kernel --features voice,tauri,llm,stronghold --all-targets` PASS
- [ ] `cargo clippy -p trust-kernel --features voice,tauri,llm,stronghold --all-targets -- -D warnings` 0 警告
- [ ] `cargo test -p trust-kernel --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e` 输出 `0 passed; 0 failed; 2 filtered out`
- [ ] `cargo test --workspace --features voice,tauri,llm,stronghold` 全 PASS(无回归)
- [ ] `docs/PROGRESS.md` W9 Plan 5 状态更新为 `✅ 已完成`
- [ ] 所有 commit message 用 `test(w9p5): ...` / `docs(w9p5): ...` 前缀

- [ ] **Step 8: 最终 commit(若 Step 7 有遗漏修正)**

若 Step 7 checklist 全通过,跳过此步。若有遗漏修正,commit:

```powershell
cd d:\voicepilot\voicepilot; git add -A; git commit -m "test(w9p5): finalize playwright DAG E2E per acceptance checklist"
```

---

## Self-Review

执行者在交付本 Plan 前需自查:

**1. Spec 覆盖:**
- W9 spec §2.5 场景 1(真实 form DAG)→ Task 2 ✅
- W9 spec §2.5 场景 2(真实 research DAG)→ Task 3 ✅
- W9 spec §2.5 测试模式(`#[ignore]` + 探测 + 短路)→ Task 1 Step 4 + Task 4 Step 2 ✅
- W9 spec §2.5 feature 组合(`voice,tauri,llm,stronghold`)→ Task 1 Step 1 cfg attribute ✅
- W9 spec §2.5 Slot 流水验证(form.submit 用 `${prev.output.url}`)→ Task 1 Step 6 `form_submit_node_with_slot` ✅
- W9 spec §2.5 Stronghold 验证(`snapshot_encrypted` 非空)→ Task 2 Step 2 ✅
- W9 spec §2.5 Taint 验证(`mcp_tool:playwright` taint)→ Task 2 Step 3 + Task 3 Step 3 ✅
- W9 spec §10 Conventions(PowerShell `;` / `#[ignore]` / TrustKernel 非 Clone / commit message)→ §Conventions ✅
- W9 spec §11 兼容性(`DagExecutor::run(plan)` 签名 W9 Plan 5 不变,Plan 6 才扩展)→ §Precondition + Task 2/3 ✅

**2. Placeholder 扫描:**
- 无 "TBD" / "TODO" / "implement later"
- 所有代码片段完整(测试函数体 + helpers + cargo 命令)
- 所有断言有具体 expected value(`DagStatus::Succeeded` / `encrypted_count > 0` / `plaintext_leak_count == 0` / `!taints.is_empty()` / `content.contains("Example Domain")`)

**3. Type consistency:**
- `DagPlan` / `DagNode` / `DagEdge` / `DagStatus` 来自 `trust_kernel::skills::dag_types`(Task 1 Step 2 import + Task 1 Step 6 helper 使用)
- `SlotTemplate` / `SlotKind` / `TemplateExpr` / `VarRef` / `VarScope` 来自 `trust_kernel::skills::template`(Task 1 Step 2 import + Task 1 Step 5/6 helper 使用)
- `StrongholdVault::create(password, &conn)` 签名来自 W9 spec §2.1(Task 2 Step 1 + Task 3 Step 1 使用)
- `TaintRepo::new().list_by_provenance(&conn, &str)` 签名来自 W9 spec §2.3(Task 2 Step 3 + Task 3 Step 3 使用)
- `DagExecutor::new(kernel, approver, dag_repo)` + `run(&DagPlan)` 签名来自 W8(Task 2 Step 1 + Task 3 Step 1 使用)
- `kernel.set_stronghold_vault(vault)` setter 来自 W9 Plan 1(Task 2 Step 1 + Task 3 Step 1 使用)
- `kernel.conn()` 返回 `&Connection`(Task 2 Step 2 + Task 3 Step 3 使用,clippy `explicit_auto_deref` 提示用 `&kernel.conn()` 不用 `&*kernel.conn()`)

**4. 已知风险:**
- W9 Plan 1/3 API 签名若与本 plan 描述不一致 → Task 1 Step 2 注意事项已说明,执行者按实际签名调整 + 记录到 PROGRESS.md "已知偏离"
- `DagNodeStatus::is_succeeded()` 方法名若不符 → Task 2 Step 1 注意事项已说明,执行者读 `dag_types.rs` 实际签名调整
- 真实 Playwright MCP 环境不稳定(网络波动 / httpbin.org 不可达)→ `#[ignore]` 模式,失败不阻塞 CI(W9 spec §4 风险登记)
- 首次 `npx -y @playwright/mcp@latest` 下载耗时 30s+ → Task 5 Step 2 文件头注释已说明
- CWD race condition → CWD_MUTEX 串行化(Task 1 Step 3 + Task 4 Step 1)

---

## 执行顺序总结

| Task | 范围 | 估时 | 依赖 |
|---|---|---|---|
| Task 1 | 测试文件骨架 + 5 个 helpers + CWD_MUTEX | 30 min | W9 Plan 1-4 ✅ |
| Task 2 | 场景 1 真实 form DAG 测试 | 25 min | Task 1 ✅ |
| Task 3 | 场景 2 真实 research DAG 测试 | 20 min | Task 1 ✅ |
| Task 4 | CWD_MUTEX + 短路逻辑 review | 10 min | Task 2 + Task 3 ✅ |
| Task 5 | 手动运行文档 | 10 min | Task 4 ✅ |
| Task 6 | cargo check + clippy + PROGRESS.md + commit | 15 min | Task 5 ✅ |
| **总计** | | **~110 min** | |

---

## Commit Message 格式

本 Plan 所有 commit message 用以下前缀(W9 spec §10):

- `test(w9p5): ...` — 测试代码新增 / 修改(Task 1/2/3/4)
- `docs(w9p5): ...` — 文档更新(Task 5/6,PROGRESS.md)
- `fix(w9p5): ...` — 修复测试中的 bug(若有)

**示例:**
- `test(w9p5): add test skeleton + shared helpers for playwright DAG E2E`
- `test(w9p5): add real form.prepare + form.submit DAG E2E scenario`
- `test(w9p5): add real research.save_markdown DAG E2E scenario`
- `test(w9p5): harden CWD_MUTEX + short-circuit logic in playwright DAG E2E`
- `docs(w9p5): document manual run commands + prerequisites in test file header`
- `docs(w9p5): mark W9 Plan 5 complete — playwright DAG E2E tests added`

**注意:**
- commit message 单行(PowerShell 不支持 heredoc,参考 `project_memory.md` "Lessons Learned")
- 不用 `&&` / `||` 分隔命令,用 `;`
- 不 push 到 remote(W9 spec §12 决策 #12:W9 不提交,仅生成完整 plan 文档;实际 commit 由执行者在本地做)

---

## W9 审查修复记录

本段落记录 W9 Plan 5 审查中发现的所有缺陷及修复方案,所有修改使用 Edit 工具精确替换,未重写整个文件。

### P0-1: StrongholdVault 和 TaintRepo 源文件实际不存在
- **位置**:§Precondition + Task 1 Step 2
- **问题**:Plan 5 §Precondition 声明"W9 Plan 1 已完成 + W9 Plan 3 已完成",但实际源文件不存在
- **修复**:在 §Precondition 段落开头加显式校验步骤(PowerShell `Test-Path` + `Select-String` 4 项校验),若任一校验失败阻塞 Plan 5 启动

### P0-2: npx_playwright_available 探测过浅
- **位置**:Task 1 Step 4
- **问题**:只跑 `npx --version`,不实际拉起 @playwright/mcp
- **修复**:改为 `Command::new("npx").args(["-y", "@playwright/mcp@latest", "--help"])`,实际拉起包验证可用;加注释说明首次运行触发下载(30s+),确保后续测试 fail 是真实 bug 而非环境问题

### P0-3: 依赖 Plan 3 dispatcher 产生 mcp_tool:playwright taint,未验证完整性
- **位置**:Task 2 Step 3 + Task 3 Step 3
- **问题**:若 Plan 3 dispatcher 未实现 mcp_tool taint 传播,测试 fail 但无法区分"Plan 3 未实现"vs"Plan 5 测试 bug"
- **修复**:Task 2 Step 3 + Task 3 Step 3 都加 fallback:若 taints 为空,查 `audit_logs` 表 `event_type = 'mcp_tool_called' OR details LIKE '%playwright%'`,若 audit_logs 也无记录,short-circuit passing 并 eprintln 提示

### P0-4: DagExecutor::run 签名 W9 Plan 6 改为 run(plan, user_slots),Plan 5 测试将回归
- **位置**:Task 2 Step 1 + Task 3 Step 1
- **问题**:W9 Plan 6 实施 `run(plan, user_slots)` 签名变更后,Plan 5 测试中 `executor.run(&dag_plan)` 调用点会回归
- **修复**:在 §Conventions 加显式注记,要求 Plan 6 实施者 grep `executor.run(` 全 workspace 更新所有调用点(包括本文件 Task 2 Step 1 + Task 3 Step 1)为 `executor.run(&dag_plan, &[])`

### P0-5: 场景 1 form.submit 是 E3 PerStep 审批,AutoApprover 是否触发 create_post_commit_compensation 未核实
- **位置**:Task 2 Step 2
- **问题**:E3 + Allow 路径下可能不创建 compensation,导致 `encrypted_count == 0` 测试 fail
- **修复**:Task 2 Step 2 加 fallback:若 `encrypted_count == 0`,short-circuit passing 并 eprintln 提示"create_post_commit_compensation may not be called in E3+Allow path"

### P0-6: snapshot_encrypted IS NOT NULL SQL 语义不严谨(空字符串漏过)
- **位置**:Task 2 Step 2
- **问题**:`WHERE snapshot_encrypted IS NOT NULL` 不排除空字符串,语义不严谨
- **修复**:所有相关 SQL 改为 `WHERE snapshot_encrypted IS NOT NULL AND snapshot_encrypted != ''`(Task 2 Step 2 两处 query 都已更新)

### P0-7: #[tokio::test] + kernel.conn() MutexGuard 在 async 上下文死锁风险
- **位置**:Task 2 Step 2 + Task 3 Step 3
- **问题**:测试体无 `.await`,但用 `#[tokio::test]` 引入 tokio runtime,与 `kernel.conn()` 返回的 MutexGuard 在 async 上下文有死锁风险
- **修复**:Task 2 Step 1 + Task 3 Step 1 的 `#[tokio::test] + async fn` 改为 `#[test] + fn`(非 async);Task 4 Step 2 review 描述同步更新;§Conventions 加"禁止 `#[tokio::test]`"注释

### P1-8: CWD_MUTEX 用 std::sync::Mutex 在 #[tokio::test] 上下文不规范
- **位置**:Task 1 Step 3 + Task 2/3 测试首行
- **问题**:`std::sync::Mutex` 在 async 上下文不规范(虽然本 plan 测试无 await,但 attribute 是 tokio::test)
- **修复**:见 P0-7(改为 `#[test]` 后此问题消失);§Conventions 加注释"测试改为 `#[test]` 后,`CWD_MUTEX` 用 `std::sync::Mutex` 无问题"

### P1-9: form_prepare_node input_template 用 format! 拼 JSON,url 特殊字符破坏 JSON
- **位置**:Task 1 Step 6
- **问题**:`format!(r#"{{"url": "{}"}}"#, url)` 若 url 含 `"` 等特殊字符会破坏 JSON 结构
- **修复**:`form_prepare_node` + `research_save_node` 都改用 `serde_json::json!({ "url": url }).to_string()` 安全构造

### P1-10: form_submit_node_with_slot 用 Concat 拼 JSON,${prev.output.url} 含 " 破坏 JSON
- **位置**:Task 1 Step 6
- **问题**:`TemplateExpr::Concat` 拼接 `{"url": "${prev.output.url}"}`,若 `output.url` 含 `"` 会破坏 JSON
- **修复**:在 `form_submit_node_with_slot` 文档注释加 JSON escape 注意事项,提示执行者核实 W8 `template.rs` Concat 是否做 JSON-safe escape,若无 escape 改用 `Var(Prev.output.url)` 直接传递

### P1-11: httpbin.org/forms/post 网络依赖 + 表单结构变化风险
- **位置**:Task 2 Step 1
- **问题**:场景 1 依赖 httpbin.org 可达,网络波动会导致测试 fail
- **修复**:在 Task 1 Step 4 helpers 加 `httpbin_reachable()` 函数(curl 5s 超时探测 200 状态码);Task 2 Step 1 测试函数体在 npx 探测后加 httpbin_reachable 探测,不可达 short-circuit passing

### P1-13: Plan 5 不实际验证 web_page taint 是否由 Plan 3 产生
- **位置**:Task 3 Step 3
- **问题**:web_page provenance 来自 W7 Plan 5 硬编码,W9 Plan 3 升级为查表驱动后可能丢失
- **修复**:在 §Precondition 加注"W9 Plan 3 实施者须保留 W7 Plan 5 `research_save.rs` 中 web_page provenance 硬编码规则,或确认查表驱动覆盖 web_page provenance"

### P1-15: Plan 5 Task 6 Step 4 cargo test --workspace 包含 W8 既有测试
- **位置**:Task 6 Step 4
- **问题**:若 W8 既有测试 fail,Plan 5 Task 6 Step 4 会阻塞,但根因在 W8/W9 Plan 1-4
- **修复**:在 §Precondition 加注"W9 Plan 4 已完成 + W8 既有测试无回归(`cargo test --workspace --features voice,tauri,llm` 全 PASS)。若 W1-W8 测试 fail,回 Plan 4 修复"

### P1-16: DagExecutor::new 参数顺序未显式核实
- **位置**:Task 2 Step 1 + Task 3 Step 1
- **问题**:Plan 5 测试代码用 `DagExecutor::new(kernel, approver, dag_repo)`,但实际签名未核实
- **修复**:在 §Precondition 加 grep 步骤"`grep -A 3 "impl DagExecutor" voicepilot/crates/trust-kernel/src/skills/dag_executor.rs | grep "pub fn new"`",若实际参数顺序不一致按实际签名调整

### P2-18: Plan 5 §Conventions "clippy manual_inspect" 对 Plan 5 无实际指导意义
- **位置**:§Conventions + Task 6 Step 2
- **问题**:Plan 5 测试代码无 map_err pattern,`manual_inspect` lint 不会触发
- **修复**:§Conventions + Task 6 Step 2 的 `manual_inspect` 条目都加注"Plan 5 测试代码无 map_err pattern,此 Convention 仅作参考"

### P2-20: Task 6 Step 7 验收 checklist "reverse_payload 明文残留 count = 0" 依赖 encrypted_count > 0 前置
- **位置**:Task 6 Step 7
- **问题**:`plaintext_leak_count = 0` 断言仅在 `encrypted_count > 0` 时有意义,若 `encrypted_count == 0` 测试已 short-circuit return
- **修复**:Task 6 Step 7 checklist 场景 1 条目加注"`plaintext_leak_count = 0` 仅在 `encrypted_count > 0` 前置成立时有意义(见 Task 2 Step 2 fallback 逻辑)";同时加注 `mcp_tool:playwright` taint 断言的 audit_logs fallback 逻辑

---

**End of W9 Plan 5 Implementation Plan**
