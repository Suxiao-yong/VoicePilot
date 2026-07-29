# W9 Plan 6: 真实 UIA GUI DAG E2E + Slot 流水闭合 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 闭合 W8 spec §8 留下的 Slot 流水欠债(`DagExecutor::run_simple_node` 中 `user_slots` 传 `&[]`),把 `DagExecutor::run` 签名扩展为 `run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot]) -> Result<DagResult>`(breaking change),实现 `IterableSource::UserSlot`(当前返回 Err),更新 W8 既有调用点为 `run(plan, &[])`,并新增 `tests/w9_plan6_uia_dag_e2e.rs` 用 2 个 `#[ignore]` 真实 E2E 场景验证 note.capture 单节点 DAG + note.capture→files.organize Slot 流水 DAG 在真实 Windows GUI 会话下的端到端正确性(含 Stronghold 加密补偿 + user_input taint 传播)。

**Architecture:** Task 1-3 闭合 Slot 流水欠债——`DagExecutor::run` 新增 `user_slots` 参数,沿调用链透传到 `run_simple_node` → `SlotTemplateEngine::resolve`;Task 2 在 `dag_executor.rs:700-707` 把 `IterableSource::UserSlot` 从 `Err(...)` 改为查表 `user_slots.iter().find(|s| s.kind == slot_kind)` 并解析为 iterable;Task 3 用 `grep` 找到 W8 既有 `executor.run(&plan)` 调用点(分布在 `w8_plan2_*.rs` / `w8_plan3_*.rs` / `w8_e2e_dag_smoke.rs` 等测试文件,约 40+ 处),统一改为 `executor.run(&plan, &[])`(空 user_slots,行为等价 W8)。Task 5-7 创建 E2E 测试文件,复用 W7 Plan 4 的 `#[ignore] real_gui` 模式 + `CWD_MUTEX` 串行化 + `windows_gui_available()` 探测短路;Task 6 在 `dispatch_note_capture` 中接入真实 `WindowsUiaAdapter`(W8 留了 `Err("... Plan 6 work")` 占位),通过 thread-local adapter 模式把 `UiaAdapter` 透传进 DagExecutor(规避 `!Send + !Sync` COM apartment 约束),并把 `save_path` 写入 DispatchOutcome.output 供下游 Slot 流水消费;Task 7 验证 `${prev.output.save_path}` 模板解析 + 真实文件移动。Task 10 跑 `voice,tauri,llm,uia,stronghold` feature 组合的 cargo check + clippy + 非门控测试数 + commit。

**Tech Stack:** Rust(stable),`trust_kernel::skills::dag_executor::DagExecutor`,`trust_kernel::skills::template::SlotTemplateEngine`,`trust_kernel::uiautomation::adapter::WindowsUiaAdapter`(W7 Plan 4 引入,Windows-only),`trust_kernel::crypto::stronghold::StrongholdVault`(W9 Plan 1 引入),`trust_kernel::policy::taint_repo::TaintRepo`(W9 Plan 3 引入),`tokio::test`(async 测试),PowerShell(`;` 分隔命令)。

**Spec:** `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.6(Plan 6 范围)+ §10(Conventions)+ §11(兼容性,DagExecutor::run 签名变更 breaking change)

**Precondition:**
- W9 Plan 1 已完成(`2026-07-28-w9-plan1-stronghold-vault.md`):`StrongholdVault` + Argon2id 密钥派生 + config 存储 + 降级模式 + `kernel.set_stronghold_vault()` / `kernel.stronghold_vault()` API
- W9 Plan 2 已完成(`2026-07-28-w9-plan2-snapshot-encrypted.md`):`create_post_commit_compensation` 注入 Stronghold 加密 + `reverse_compensation` 解密 + 明文残留检测(`snapshot_encrypted IS NOT NULL` when vault unlocked)
- W9 Plan 3 已完成(`2026-07-28-w9-plan3-taint-tracking.md`):`TaintRepo` CRUD + `dispatch_skill_executor` 传播 taint(输入 → 输出)+ gateway 查表驱动
- W9 Plan 4 已完成(`2026-07-28-w9-plan4-dag-modify.md`):`DagApprovalOutcome::Modify` 分支 + 重新审批闭环(本 Plan 不直接依赖 Modify,但 spec §3 依赖图要求 Plan 4 ✅)
- W7 Plan 4 已完成:`uia` feature + `WindowsUiaAdapter` + `note.capture` / `quick.app_control` Skill + `w7_plan4_uia_smoke.rs` 测试模式(`#[ignore] real_gui` + mock adapter 双层)
- W8 全部完成(commit `8ec814d`):`DagExecutor` + `SlotTemplateEngine` + `dispatch_skill_executor` 9 路分发 + `w8_e2e_dag_smoke.rs` 8 场景
- `cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold` PASS(Windows)
- `cargo test --workspace --features voice,tauri,llm,uia,stronghold` 全 PASS,W1-W8 测试无回归
- 运行环境:Windows 10/11 + 真实 GUI 会话(非 SSH/无头)+ `notepad.exe` 可用 + `C:\Users\Public\Desktop` 可写

---

## W9 7-Plan 拆分概览(供 Plan 6 执行者参考)

| Plan | 范围 | Spec § | 状态 |
|---|---|---|---|
| Plan 1 | StrongholdVault + Argon2id + 降级模式 | §2.1 | ✅ 已完成(前置) |
| Plan 2 | snapshot_encrypted 真实加密 + 明文 PoC 移除 | §2.2 | ✅ 已完成(前置) |
| Plan 3 | TaintRepo CRUD + dispatcher 传播 + gateway 查表 | §2.3 | ✅ 已完成(前置) |
| Plan 4 | DagApprovalOutcome::Modify + UI 编辑器 + 重新审批 | §2.4 | ✅ 已完成(前置) |
| Plan 5 | 真实 Playwright MCP DAG E2E | §2.5 | ⏳ 并行(不互斥) |
| **Plan 6 (本文件)** | Slot 流水闭合 + 真实 UIA GUI DAG E2E | §2.6 | ⏳ 进行中 |
| Plan 7 | 集成验收 + 7 套 feature cargo check + clippy + npm build + PROGRESS.md | §2.7 | ⏳ 待 Plan 1-6 ✅ |

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Modify** `src/skills/dag_executor.rs` — Task 1:`run()` 签名加 `user_slots: &[ExtractedSlot]` 参数 + `run_simple_node` 签名加 `user_slots: &[ExtractedSlot]` + 沿调用链透传(行 82 / 行 180-188 / 行 316-323 / 行 351-357);Task 2:`resolve_iterable` 的 `IterableSource::UserSlot` 分支(行 700-707)从 `Err(...)` 改为查表解析;Task 6:`DagExecutor` 加 thread-local `UiaAdapter` 注入点 + `dispatch_note_capture` 接入真实 adapter
- **Modify** `src/skills/dispatcher.rs` — Task 6:`dispatch_note_capture`(行 268-290)从 `Err("... Plan 6 work")` 改为调 `execute_note_capture` + 返回含 `save_path` 的 output;`dispatch_skill_executor` 签名加 `adapter: Option<&dyn UiaAdapter>` 参数(或用 thread-local,见 Task 6 设计决策)
- **Modify** `src/skills/dag_executor.rs` 的 `run_simple_node` 内 `dispatch_skill_executor` 调用(行 368-369) — Task 6:透传 thread-local adapter
- **Modify**(条件性) `src/skills/note_capture.rs` — Task 6:若 `execute_note_capture` 返回值不含 `save_path`,扩展返回值或由 dispatcher 构造含 `save_path` 的 output JSON

### Tests

- **Modify** `tests/w8_template_unit.rs` — Task 4:追加 3 个 UserSlot 单元测试(IterableSource::UserSlot 解析 / user_slots 传递 / 空数组 fallback)
- **Create** `tests/w9_plan6_uia_dag_e2e.rs` — Task 5-7:2 个 `#[ignore]` 真实 E2E 场景 + 共享 helpers(`windows_gui_available` / `literal_text_template` / `note_capture_node` / `files_organize_node_with_slot` / `CWD_MUTEX`)
- **Modify**(批量) W8 既有测试文件 — Task 3:把 `executor.run(&plan)` / `executor.run(&dag)` / `executor.run(&dag_plan)` 全部改为 `executor.run(&plan, &[])` / `executor.run(&dag, &[])` / `executor.run(&dag_plan, &[])`,涉及文件:
  - `tests/w8_plan2_dag_executor.rs`(约 8 处)
  - `tests/w8_plan2_dag_e2e.rs`(约 3 处)
  - `tests/w8_plan2_audit_events.rs`(约 12 处)
  - `tests/w8_plan3_loop_node.rs`(约 17 处)
  - `tests/w8_e2e_dag_smoke.rs`(约 6 处)
  - 其它发现的新调用点(用 grep 兜底)

### Docs

- **Modify** `docs/PROGRESS.md` — Task 10:W9 Plan 6 完成状态 + Slot 流水欠债闭合记录 + 真实 E2E 测试统计

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 `project_memory.md` "Lessons Learned",PowerShell 不支持 heredoc)
- **TDD**:Task 4 先写失败测试 → 跑红 → Task 2 实现 → 跑绿 → commit;Task 5-7 的 E2E 测试是 `#[ignore]` 手动验证,不参与 CI 门禁
- **`#[ignore]` 真实 E2E 模式**(复用 W7 Plan 4,核实报告 3.1-3.2):
  - 每个真实 E2E 测试加 `#[ignore = "requires real Windows GUI; run with --ignored --features voice,tauri,llm,uia,stronghold manually"]`
  - 测试体首行调 `windows_gui_available()`,不可用则 `eprintln!("Skipping: ...")` + `return`(短路 passing,不 fail)
  - feature gate:`#![cfg(all(windows, feature = "voice", feature = "tauri", feature = "llm", feature = "uia", feature = "stronghold"))]`(Windows 限定)
  - 运行命令:`cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored`(W9 修复:测试名不含 "real_gui",不能用 `--ignored real_gui` 过滤)
- **CWD_MUTEX 串行化**(复用 W7 Plan 4 / `note_capture.rs:373` 模式):
  - 真实 E2E 测试涉及文件系统写入(`save_path` / `files.organize` 移动),用 `static CWD_MUTEX: Mutex<()> = Mutex::new(())` + `let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner())` 串行化
  - `with_temp_cwd(body)` helper:创建 temp dir + `Documents/` 子目录 + chdir + body + 恢复原 CWD
- **Slot 流水模板语法**(spec §2.6 + 核实报告 4.1):
  - `${prev.output.xxx}` — 引用上游节点输出字段(`VarRef { scope: VarScope::Prev, path: "output.xxx".into() }`)
  - `${n1.output.xxx}` — 引用具名节点输出字段(`VarRef { scope: VarScope::Step("n1".into()), path: "output.xxx".into() }`)
  - **VarScope 实际变体**:`Prev` / `Step(String)` / `User` / `Iter`(见 `template.rs:74-83`),没有 `Named` 变体。引用具名节点用 `Step("n1".into())`。
  - `${user.xxx}` — 引用用户 slot(`VarRef { scope: VarScope::User, path: "xxx".into() }`,需 `user_slots` 非空)
  - `${item}` — 循环变量(循环节点内)
  - Filter:`SlotTemplate { kind, template: TemplateExpr::Filter(...) }` 对值做后处理
  - `SlotTemplateEngine::resolve(template, node_outputs, user_slots, iter_var, prev_node_id)` 完整支持以上 5 种
- **TrustKernel 不是 Clone**:E2E 测试用 `Arc<TrustKernel>` 共享或 owned `TrustKernel::open_in_memory()`(参考 `project_memory.md` "Lessons Learned")
- **Approver import 完整路径**:`use crate::approval::approver::Approver;`(approval 模块未在 root re-export)
- **UiaAdapter 是 `!Send + !Sync`**(COM apartment 模型):DagExecutor 不能持有 `Arc<dyn UiaAdapter>` 字段,改用 thread-local 注入(见 Task 6 设计决策);W8 `dispatch_app_control` / `dispatch_note_capture` 注释明确 "Plan 6 集成时通过 DagExecutor 透传 adapter"
- **DispatchOutcome.output 语义**(dispatcher.rs:43-52):
  - `from_skill_execution(exec, task_id, step_id)` → `output = exec.tool_result.data`(files.organize 走此路,含 `moved_count` / `destination` / `approval_id`)
  - `from_task_id(returned_task_id, step_id)` → `output = {"task_id": "...", "step_id": "..."}`(其余 8 路 executor)
  - Task 6:`dispatch_note_capture` 需自定义 output 含 `save_path`,供下游 `${prev.output.save_path}` 解析
- **commit message**:`feat(w9p6): ...`(Slot 流水闭合 / UiaAdapter 透传)/ `test(w9p6): ...`(E2E 测试)/ `fix(w9p6): ...`(W8 调用点更新)/ `docs(w9p6): ...`(PROGRESS.md)
- **不引入新依赖**:本 plan 仅用 `uiautomation-rs`(W7 已引入)+ `stronghold` feature(W9 Plan 1 引入)+ `tokio` + `serde_json`(均已在 workspace)
- **不修改 spec / 已有 plan**:若发现 spec 描述与实现不一致,记录到 PROGRESS.md "已知偏离" 段落,不回改 spec
- **breaking change 兼容策略**(spec §11):`DagExecutor::run` 签名变更,W8 既有调用点统一传 `&[]`(空 user_slots,行为等价 W8);`IterableSource::UserSlot` 实现后,`user_slots=[]` 时仍返回 `Err(UserSlotNotFound)`(向后兼容,只在没有对应 slot 时才报错)
- **spec §11 数字偏离**(W9 修复记录):spec §11 兼容性表写"W8 既有调用点需更新(8 处)",但 Plan 6 实测 grep `\.run\(&[a-z_]` 全 workspace 返回 44+ 处。此偏离记录到 PROGRESS.md "已知偏离" 段落,不回改 spec。

---

## Task 1: Slot 流水闭合 — DagExecutor::run 签名扩展 + run_simple_node 传 user_slots

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(行 82 / 行 179-189 / 行 316-323 / 行 351-357)

**目标:** 把 `DagExecutor::run` 签名从 `run(&self, plan: &DagPlan) -> Result<DagResult>` 扩展为 `run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot]) -> Result<DagResult>`(breaking change),并在调用 `run_simple_node` 时把 `user_slots` 透传(不再传 `&[]`),闭合 W8 spec §8 留下的 Slot 流水欠债。

- [ ] **Step 1: 确认 ExtractedSlot import 路径**

`ExtractedSlot` 定义在 `src/llm/types.rs:23`,DagExecutor 当前未 import。检查 `dag_executor.rs` 顶部 use 段,确认是否已有 `use crate::llm::types::ExtractedSlot;`,若无则追加。

```rust
// dag_executor.rs 顶部 use 段(追加,若缺失)
use crate::llm::types::ExtractedSlot;
```

- [ ] **Step 2: 修改 run() 签名 + 透传 user_slots 到 run_simple_node**

修改 `dag_executor.rs:82` 的 `run` 方法签名,并在行 182-188 调用 `run_simple_node` 时透传 `user_slots`:

```rust
// dag_executor.rs:82(修改前)
pub fn run(&self, plan: &DagPlan) -> Result<DagResult> {

// dag_executor.rs:82(修改后)
pub fn run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot]) -> Result<DagResult> {
```

```rust
// dag_executor.rs:179-189(修改前)
let status = if let Some(loop_spec) = plan.loop_specs.get(node_id) {
    self.run_loop_node(node_id, plan, loop_spec, &node_outputs, prev_node_id.as_deref())?
} else {
    self.run_simple_node(
        node,
        plan,
        &node_outputs,
        prev_node_id.as_deref(),
        &root_task_id,
    )?
};

// dag_executor.rs:179-189(修改后)
let status = if let Some(loop_spec) = plan.loop_specs.get(node_id) {
    // W9 Plan 6:循环节点也接收 user_slots(供 IterableSource::UserSlot 解析)
    self.run_loop_node(
        node_id,
        plan,
        loop_spec,
        &node_outputs,
        user_slots,
        prev_node_id.as_deref(),
    )?
} else {
    // W9 Plan 6:闭合 Slot 流水欠债 — 透传 user_slots(不再传 &[])
    self.run_simple_node(
        node,
        plan,
        &node_outputs,
        user_slots,
        prev_node_id.as_deref(),
        &root_task_id,
    )?
};
```

- [ ] **Step 3: 修改 run_simple_node 签名 + 透传 user_slots 到 SlotTemplateEngine::resolve**

修改 `dag_executor.rs:316-323` 的 `run_simple_node` 签名加 `user_slots: &[ExtractedSlot]` 参数,并在行 351-357 调用 `SlotTemplateEngine::resolve` 时透传(替换 `&[]`):

```rust
// dag_executor.rs:316-323(修改前)
fn run_simple_node(
    &self,
    node: &DagNode,
    plan: &DagPlan,
    node_outputs: &HashMap<String, serde_json::Value>,
    prev_node_id: Option<&str>,
    _root_task_id: &str,
) -> Result<DagNodeStatus> {

// dag_executor.rs:316-323(修改后)
fn run_simple_node(
    &self,
    node: &DagNode,
    plan: &DagPlan,
    node_outputs: &HashMap<String, serde_json::Value>,
    user_slots: &[ExtractedSlot],
    prev_node_id: Option<&str>,
    _root_task_id: &str,
) -> Result<DagNodeStatus> {
```

```rust
// dag_executor.rs:347-357(修改前)
// Step 1: 解析模板 → serde_json::Value
// user_slots 暂传 &[] — Plan 5 实现 UI 时 DagExecutor::run 签名扩展传入。
// 模板中 ${user.xxx} 会解析失败 → 节点 Failed(本 plan 可接受)。
// iter_var 暂传 None — 简单节点无循环变量。
let resolved_input = match SlotTemplateEngine::resolve(
    &node.input_template.template,
    node_outputs,
    &[],
    None,
    prev_node_id,
) {

// dag_executor.rs:347-357(修改后)
// Step 1: 解析模板 → serde_json::Value
// W9 Plan 6:闭合 Slot 流水欠债 — 透传实际 user_slots(支持 ${user.xxx} 解析)。
// iter_var 暂传 None — 简单节点无循环变量。
let resolved_input = match SlotTemplateEngine::resolve(
    &node.input_template.template,
    node_outputs,
    user_slots,
    None,
    prev_node_id,
) {
```

- [ ] **Step 4: 修改 run_loop_node 签名(若 Step 2 已传 user_slots)**

`run_loop_node` 在 Step 2 调用处新增了 `user_slots` 参数,需同步修改其定义签名。搜索 `fn run_loop_node` 定位(约 dag_executor.rs 模块内,行号随 W8 Plan 3 实现而定),在参数列表中 `node_outputs` 之后插入 `user_slots: &[ExtractedSlot]`,并在 `run_loop_node` 内部调用 `resolve_iterable` 时透传(见 Task 2 Step 3)。

```rust
// run_loop_node 签名(修改后,插入 user_slots 参数)
fn run_loop_node(
    &self,
    node_id: &str,
    plan: &DagPlan,
    loop_spec: &LoopSpec,
    node_outputs: &HashMap<String, serde_json::Value>,
    user_slots: &[ExtractedSlot],
    prev_node_id: Option<&str>,
) -> Result<DagNodeStatus> {
    // ... 既有逻辑
    // W9 Plan 6:resolve_iterable 调用透传 user_slots(见 Task 2 Step 3)
}
```

- [ ] **Step 5: cargo check 验证编译(预期失败 — W8 调用点未更新)**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold`

Expected: 编译失败,错误形如 `this method takes 2 arguments but 1 was supplied` / `expected 2 arguments, found 1`,指向 W8 既有测试文件中的 `executor.run(&plan)` 调用点(约 40+ 处)。这是 Task 3 要修复的调用点。源码 `dag_executor.rs` 本身应编译通过(签名已扩展)。

- [ ] **Step 6: 暂不 commit(Task 3 修完调用点后统一 commit)**

Task 1 仅修改源码签名,不 commit;Task 3 修完 W8 调用点后整体跑通 `cargo check` 再 commit。

---

## Task 2: Slot 流水闭合 — IterableSource::UserSlot 实现(当前返回 Err)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(行 655-708 `resolve_iterable` 方法)

**目标:** 把 `IterableSource::UserSlot`(dag_executor.rs:700-707)从 `Err("user_slots not wired; see Plan 5")` 改为查表 `user_slots.iter().find(|s| s.kind == slot_kind)` 并解析为 `Vec<serde_json::Value>`,闭合 W8 Plan 3 留下的循环源欠债。

- [ ] **Step 1: 修改 resolve_iterable 签名加 user_slots 参数**

`resolve_iterable` 当前签名(行 655-668)不含 `user_slots`,需扩展。搜索 `fn resolve_iterable` 定位:

```rust
// 修改前(W8 既有,2 参数):
fn resolve_iterable(&self, source: &IterableSource, node_outputs: &HashMap<String, DagNodeResult>) -> Result<Vec<serde_json::Value>>;

// 修改后(W9 Plan 6,4 参数,加 user_slots + prev_node_id):
fn resolve_iterable(
    &self,
    source: &crate::skills::dag_types::IterableSource,
    node_outputs: &HashMap<String, serde_json::Value>,
    user_slots: &[ExtractedSlot],
    prev_node_id: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
```

- [ ] **Step 2: 实现 IterableSource::UserSlot 分支**

修改 `dag_executor.rs:700-707` 的 `UserSlot` 分支:

```rust
// dag_executor.rs:700-707(修改前)
IterableSource::UserSlot { slot_kind } => {
    // W8 简化:user_slots 在 Plan 2 run_simple_node 中传 &[];
    // Plan 5 UI 集成时会传入实际 slot,本 plan 暂不支持。
    Err(KernelError::Skill(format!(
        "resolve_iterable: UserSlot kind '{}' not supported in Plan 3 (user_slots not wired; see Plan 5)",
        slot_kind
    )))
}

// dag_executor.rs:700-707(修改后)
IterableSource::UserSlot { slot_kind } => {
    // W9 Plan 6:闭合 Slot 流水欠债 — 从 user_slots 查表找匹配 kind 的 slot。
    // ExtractedSlot.kind 是 SlotKind(如 Path / App / Text),
    // slot_kind 来自 LoopSpec.iterable_source,是 String,需做类型匹配。
    let slot = user_slots
        .iter()
        .find(|s| s.kind == *slot_kind)
        .ok_or_else(|| {
            // W9 修复(P1-9 错误类型不一致):与 spec §2.6 一致用 TemplateError::UserSlotNotFound
            TemplateError::UserSlotNotFound(slot_kind.clone()).into()
        })?;
    // slot.raw 是 String(W7 LLM ExtractedSlot 定义,见 src/llm/types.rs:23-30)。
    // W8 既有 VarScope::User 分支用 slot.raw,Plan 6 保持一致。
    // 尝试解析为 JSON 数组;失败则当作单元素数组(适配 "a,b,c" 逗号分隔 或 单值)。
    match serde_json::from_str::<Vec<serde_json::Value>>(&slot.raw) {
        Ok(arr) => Ok(arr),
        Err(_) => {
            // 尝试逗号分隔(简单 fallback)
            let items: Vec<serde_json::Value> = slot
                .raw
                .split(',')
                .map(|s| serde_json::Value::String(s.trim().to_string()))
                .collect();
            Ok(items)
        }
    }
}
```

> **ExtractedSlot 实际字段**:`kind: SlotKind` / `raw: String` / `high_risk: bool`(见 `src/llm/types.rs:23-30`)。W8 既有 `VarScope::User` 分支用 `slot.raw`,Plan 6 保持一致。原 Plan 6 误用 `slot.value` / `slot.start` / `slot.end` 已修正为 `slot.raw`。

- [ ] **Step 3: 在 run_loop_node 内调用 resolve_iterable 处透传 user_slots**

搜索 `resolve_iterable` 调用点(在 `run_loop_node` 内),把 `user_slots` 透传:

```rust
// run_loop_node 内(修改前)
let items = self.resolve_iterable(
    &loop_spec.iterable_source,
    &node_outputs,
    prev_node_id,
)?;

// run_loop_node 内(修改后)
let items = self.resolve_iterable(
    &loop_spec.iterable_source,
    &node_outputs,
    user_slots,
    prev_node_id,
)?;
```

- [ ] **Step 4: cargo check 验证 resolve_iterable 签名变更传播**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold`

Expected: 仍有 W8 调用点未更新的编译错误(Task 3 修复),但 `resolve_iterable` / `IterableSource::UserSlot` 分支本身应编译通过(无新错误)。

- [ ] **Step 5: 暂不 commit(Task 3 修完调用点后统一 commit)**

---

## Task 3: Slot 流水闭合 — 更新 W8 既有 DagExecutor::run 调用点为 run(plan, &[])

**Files:**
- Modify(批量): `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs`
- Modify(批量): `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_e2e.rs`
- Modify(批量): `voicepilot/crates/trust-kernel/tests/w8_plan2_audit_events.rs`
- Modify(批量): `voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs`
- Modify(批量): `voicepilot/crates/trust-kernel/tests/w8_e2e_dag_smoke.rs`
- Modify(若存在): 其它 `executor.run(&...)` 调用点(grep 兜底)

**目标:** 把 W8 既有测试文件中所有 `executor.run(&plan)` / `executor.run(&dag)` / `executor.run(&dag_plan)` / `executor2.run(&plan_cancel)` 等调用统一加上 `, &[]` 第二参数(空 user_slots,行为等价 W8),闭合 Task 1 签名变更带来的 breaking change。

- [ ] **Step 1: grep 全量定位调用点**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold 2>&1 | Select-String "this method takes|expected 2 arguments"`

Expected: 输出形如 `error[E0061]: this method takes 2 arguments but 1 was supplied` 的错误列表,每条错误指向一个 `.run(&...)` 调用点(约 40+ 处,分布在 5 个 W8 测试文件)。

备选 grep 命令(直接搜源码):

Run: `cd d:\voicepilot\voicepilot\crates\trust-kernel ; Find-Str -Pattern "\.run\(&" -Path tests\*.rs -Recurse`

- [ ] **Step 2: 批量替换 w8_plan2_dag_executor.rs(约 8 处)**

逐个把 `executor.run(&plan).unwrap()` 改为 `executor.run(&plan, &[]).unwrap()`。注意该文件可能有 `executor.run(&plan).expect("...")` 形式,同样加 `, &[]`:

```rust
// 修改前(8 处,行号约 85 / 102 / 124 / 156 / 205 / 233 / 335 等)
let result = executor.run(&plan).unwrap();

// 修改后
let result = executor.run(&plan, &[]).unwrap();
```

- [ ] **Step 3: 批量替换 w8_plan2_dag_e2e.rs(约 3 处)**

```rust
// 修改前(3 处,行号约 105 / 140 / 178)
let result = executor.run(&dag).unwrap();

// 修改后
let result = executor.run(&dag, &[]).unwrap();
```

- [ ] **Step 4: 批量替换 w8_plan2_audit_events.rs(约 12 处)**

注意该文件含 `executor.run(&plan_ok)` / `executor.run(&plan_fail)` / `executor2.run(&plan_cancel)` 等不同变量名,统一加 `, &[]`:

```rust
// 修改前(行号约 73 / 90 / 106 / 124 / 144 / 160 / 179 / 182 / 189 / 230 / 249)
executor.run(&plan).unwrap();
executor.run(&plan_ok).unwrap();
executor.run(&plan_fail).unwrap();
executor2.run(&plan_cancel).unwrap();

// 修改后
executor.run(&plan, &[]).unwrap();
executor.run(&plan_ok, &[]).unwrap();
executor.run(&plan_fail, &[]).unwrap();
executor2.run(&plan_cancel, &[]).unwrap();
```

- [ ] **Step 5: 批量替换 w8_plan3_loop_node.rs(约 17 处)**

注意该文件含 `.run(&plan).expect("...")` / `.run(&plan).expect("run should succeed")` / `.run(&plan).expect("run should not error")` 等形式:

```rust
// 修改前(行号约 70 / 108 / 153 / 179 / 199 / 223 / 251 / 276 / 302 / 347 / 379 / 398 / 415 / 432 / 451 / 512 / 553)
let result = executor.run(&plan).expect("run should succeed");
let result = executor.run(&plan).expect("run should not error");
executor.run(&plan).expect("run should succeed");
        .run(&plan)
        .expect("...");

// 修改后
let result = executor.run(&plan, &[]).expect("run should succeed");
let result = executor.run(&plan, &[]).expect("run should not error");
executor.run(&plan, &[]).expect("run should succeed");
        .run(&plan, &[])
        .expect("...");
```

- [ ] **Step 6: 批量替换 w8_e2e_dag_smoke.rs(约 6 处)**

```rust
// 修改前(行号约 202 / 285 / 384 / 484 / 728 / 908)
let result = executor.run(&dag_plan).expect("DagExecutor::run must succeed");
let result = executor.run(&dag_plan).expect("run must not infra-error");
let result = executor.run(&dag_plan).expect("DagExecutor::run must succeed even on Deny");
        .run(&dag_plan)
        .expect("...");

// 修改后
let result = executor.run(&dag_plan, &[]).expect("DagExecutor::run must succeed");
let result = executor.run(&dag_plan, &[]).expect("run must not infra-error");
let result = executor.run(&dag_plan, &[]).expect("DagExecutor::run must succeed even on Deny");
        .run(&dag_plan, &[])
        .expect("...");
```

- [ ] **Step 7: 兜底 grep 确认无遗漏**

Run: `cd d:\voicepilot\voicepilot\crates\trust-kernel ; Select-String -Path tests\*.rs -Pattern "\.run\(&[a-z]" | Select-Object -Property LineNumber, Line | Where-Object { $_.Line -notmatch "user_slots" }`

Expected: 输出为空(所有 `.run(&...)` 调用都已加 `, &[]`)。若有遗漏,逐个补上。

- [ ] **Step 8: cargo check 验证全量编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold`

Expected: PASS(无编译错误)。若有错误,根据错误信息定位遗漏的调用点或签名不匹配处。

- [ ] **Step 9: cargo check 验证 default feature 也通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`

Expected: PASS(W8 既有测试在 default feature 下也应编译通过)。

- [ ] **Step 10: 跑 W8 既有测试确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w8_plan2_dag_executor --test w8_plan2_dag_e2e --test w8_plan2_audit_events --test w8_plan3_loop_node --test w8_e2e_dag_smoke`

Expected: 全 PASS(空 user_slots 行为等价 W8,无回归)。

- [ ] **Step 11: commit Task 1-3(Slot 流水闭合 + 调用点更新)**

Run: `cd d:\voicepilot\voicepilot ; git add crates/trust-kernel/src/skills/dag_executor.rs crates/trust-kernel/tests/w8_plan2_dag_executor.rs crates/trust-kernel/tests/w8_plan2_dag_e2e.rs crates/trust-kernel/tests/w8_plan2_audit_events.rs crates/trust-kernel/tests/w8_plan3_loop_node.rs crates/trust-kernel/tests/w8_e2e_dag_smoke.rs ; git commit -m "feat(w9p6): close slot-flow debt — DagExecutor::run takes user_slots + IterableSource::UserSlot implemented + W8 call sites updated to run(plan, &[])"`

---

## Task 4: TDD — w9_template_unit.rs 追加 UserSlot 单元测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_template_unit.rs`(新文件,与 W8 `w8_template_unit.rs` 同级,聚焦 W9 新增的 UserSlot 行为)

**目标:** 用 TDD 验证 Task 1-2 的 Slot 流水闭合:`IterableSource::UserSlot` 能从 `user_slots` 解析 / `user_slots` 能透传到 `SlotTemplateEngine::resolve` / `user_slots=[]` 时 `IterableSource::UserSlot` 返回 `Err`。

- [ ] **Step 1: 创建测试文件骨架 + imports**

创建 `voicepilot/crates/trust-kernel/tests/w9_template_unit.rs`:

```rust
//! W9 Plan 6 Task 4 — Slot 流水闭合单元测试。
//!
//! 验证 Task 1-2 的闭合工作:
//! 1. IterableSource::UserSlot 能从 user_slots 解析为 Vec<Value>(JSON 数组)
//! 2. IterableSource::UserSlot 支持 CSV fallback("a,b,c" → 3 元素)
//! 3. IterableSource::UserSlot 在 user_slots=[] 时返回 Err(向后兼容)
//! 4. SlotTemplateEngine::resolve 能透传 user_slots 解析 ${user.xxx}
//! 5. DagExecutor::run(plan, &[]) 行为等价 W8 run(plan)(空 user_slots)

#![cfg(feature = "llm")]

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::types::ExtractedSlot;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagPlan, DagStatus, IterableSource, LoopSpec,
};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

fn literal_text_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Literal(value.to_string()),
    }
}
```

- [ ] **Step 2: 写失败测试 1 — IterableSource::UserSlot JSON 数组解析**

> **IterableSource::UserSlot 是 struct variant**:`UserSlot { slot_kind: String }`(见 `dag_types.rs:67`),不是 tuple variant。Plan 7 Task 1 Step 9 D 组测试需同步用 struct variant 构造。
>
> **ExtractedSlot 实际字段**:`kind: SlotKind` / `raw: String` / `high_risk: bool`(见 `src/llm/types.rs:23-30`)。W8 既有 `VarScope::User` 分支用 `slot.raw`,Plan 6 保持一致。本测试构造 `ExtractedSlot { kind, raw, high_risk }` 三字段,删除 value/start/end。
>
> **task.explain 依赖 LLM**:测试 1-3 用 `--features llm`,若 task.explain 需真实 LLM 调用可能 Failed。建议用 mock LLM 或改为不依赖 LLM 的 skill(如 `task.echo` 若存在)。若测试 Failed 而非 Succeeded,核实 task.explain 的 mock 行为。

```rust
#[test]
fn user_slot_iterable_resolves_json_array() {
    // W9 Plan 6 Task 4:IterableSource::UserSlot 从 user_slots 解析 JSON 数组。
    // 这是 Task 2 的核心行为 — 闭合 W8 "user_slots not wired" 欠债。
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    // user_slots 含一个 kind="files" 的 slot,raw 是 JSON 数组
    // ExtractedSlot 实际三字段:kind / raw / high_risk(见 src/llm/types.rs:23-30)
    let user_slots = vec![ExtractedSlot {
        kind: SlotKind::Files,
        raw: r#"["a.txt","b.txt","c.txt"]"#.to_string(),
        high_risk: false,
    }];

    // 构造循环节点,iterable_source = UserSlot { slot_kind: "files" }(struct variant)
    let plan = DagPlan {
        plan_id: format!("w9p6-test-{}", uuid::Uuid::new_v4()),
        user_goal: "test user slot iterable".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(),
            input_template: literal_text_template(r#"{"limit": 1}"#),
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: {
            let mut m = HashMap::new();
            m.insert(
                "n1".to_string(),
                LoopSpec {
                    iterable_source: IterableSource::UserSlot {
                        slot_kind: "files".to_string(),
                    },
                    body_template: literal_text_template(r#"{"limit": 1}"#),
                    break_condition: None,
                    max_iterations: 10,
                },
            );
            m
        },
        max_total_steps: 5,
    };

    let result = executor.run(&plan, &user_slots).expect("run must succeed");
    // 循环节点应 Succeeded(3 次 body 执行)
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "expected Succeeded, got {:?}",
        result.status
    );
}
```

- [ ] **Step 3: 跑测试 1 确认通过(Task 2 已实现)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_template_unit user_slot_iterable_resolves_json_array`

Expected: PASS(Task 2 已实现 `IterableSource::UserSlot`,若失败说明 Task 2 实现有误,回头修)。

- [ ] **Step 4: 写测试 2 — IterableSource::UserSlot CSV fallback**

```rust
#[test]
fn user_slot_iterable_csv_fallback() {
    // W9 Plan 6 Task 4:slot.value 不是合法 JSON 数组时,
    // 按 CSV 分隔解析(Task 2 Step 2 的 fallback 分支)。
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    let user_slots = vec![ExtractedSlot {
        kind: SlotKind::Files,
        raw: "a.txt,b.txt,c.txt".to_string(),  // 非 JSON,CSV 格式
        high_risk: false,
    }];

    let plan = DagPlan {
        plan_id: format!("w9p6-test-{}", uuid::Uuid::new_v4()),
        user_goal: "test csv fallback".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(),
            input_template: literal_text_template(r#"{"limit": 1}"#),
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: {
            let mut m = HashMap::new();
            m.insert(
                "n1".to_string(),
                LoopSpec {
                    iterable_source: IterableSource::UserSlot {
                        slot_kind: "files".to_string(),
                    },
                    body_template: literal_text_template(r#"{"limit": 1}"#),
                    break_condition: None,
                    max_iterations: 10,
                },
            );
            m
        },
        max_total_steps: 5,
    };

    let result = executor.run(&plan, &user_slots).expect("run must succeed");
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "CSV fallback should produce 3 items, got {:?}",
        result.status
    );
}
```

- [ ] **Step 5: 写测试 3 — IterableSource::UserSlot 空数组 fallback(向后兼容)**

```rust
#[test]
fn user_slot_iterable_empty_user_slots_returns_failed() {
    // W9 Plan 6 Task 4:user_slots=[] 时 IterableSource::UserSlot
    // 找不到匹配 slot → 节点 Failed(向后兼容 W8 行为)。
    // 这保证 Task 3 的 run(plan, &[]) 不会破坏既有语义。
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    let plan = DagPlan {
        plan_id: format!("w9p6-test-{}", uuid::Uuid::new_v4()),
        user_goal: "test empty user_slots".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(),
            input_template: literal_text_template(r#"{"limit": 1}"#),
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: {
            let mut m = HashMap::new();
            m.insert(
                "n1".to_string(),
                LoopSpec {
                    iterable_source: IterableSource::UserSlot {
                        slot_kind: "files".to_string(),
                    },
                    body_template: literal_text_template(r#"{"limit": 1}"#),
                    break_condition: None,
                    max_iterations: 10,
                },
            );
            m
        },
        max_total_steps: 5,
    };

    // user_slots = &[](Task 3 既有调用点的等价行为)
    let result = executor.run(&plan, &[]).expect("run must not infra-error");
    // 循环节点应 Failed(UserSlot not found),DagStatus 可能 Failed 或 PartiallySucceeded
    assert!(
        matches!(result.status, DagStatus::Failed | DagStatus::PartiallySucceeded),
        "empty user_slots should fail the loop node, got {:?}",
        result.status
    );
}
```

- [ ] **Step 6: 写测试 4 — SlotTemplateEngine::resolve 透传 user_slots 解析 ${user.xxx}**

```rust
#[test]
fn resolve_template_with_user_slot_var() {
    // W9 Plan 6 Task 4:${user.xxx} 模板变量能从 user_slots 解析。
    // 验证 Task 1 透传 user_slots 到 SlotTemplateEngine::resolve 后,
    // 简单节点的 input_template 能引用 ${user.files}。
    use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};

    let template = SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Var(VarRef {
            scope: VarScope::User,
            path: "files".into(),
        }),
    };

    let user_slots = vec![ExtractedSlot {
        kind: SlotKind::Files,
        raw: "hello from user slot".to_string(),
        high_risk: false,
    }];

    let node_outputs = HashMap::new();
    let resolved = trust_kernel::skills::template::SlotTemplateEngine::resolve(
        &template.template,
        &node_outputs,
        &user_slots,
        None,
        None,
    )
    .expect("resolve must succeed");

    // resolved 应为 "hello from user slot"(字符串)
    assert_eq!(resolved, serde_json::json!("hello from user slot"));
}
```

- [ ] **Step 7: 写测试 5 — DagExecutor::run(plan, &[]) 行为等价 W8**

```rust
#[test]
fn run_with_empty_user_slots_equivalent_to_w8() {
    // W9 Plan 6 Task 4:run(plan, &[]) 应等价 W8 的 run(plan)。
    // 用 task.explain 节点(无 ${user.xxx} 依赖)验证空 user_slots 不破坏既有路径。
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    let plan = DagPlan {
        plan_id: format!("w9p6-test-{}", uuid::Uuid::new_v4()),
        user_goal: "test empty user_slots equivalence".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(),
            input_template: literal_text_template(r#"{"limit": 1}"#),
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };

    let result = executor.run(&plan, &[]).expect("run must succeed");
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "empty user_slots with no ${user.xxx} dependency should Succeed, got {:?}",
        result.status
    );
}
```

- [ ] **Step 8: 跑全部 5 个测试确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_template_unit`

Expected: 5 个测试全 PASS。若测试 1-3 失败,回头检查 Task 2 `IterableSource::UserSlot` 实现;若测试 4 失败,检查 `SlotTemplateEngine::resolve` 的 `VarScope::User` 分支是否已正确查表 `user_slots`(template.rs:312 附近)。

- [ ] **Step 9: cargo check default feature 验证(cfg(feature = "llm") 门控正确)**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`

Expected: PASS(测试文件 `#![cfg(feature = "llm")]` 门控,default feature 下不编译)。

- [ ] **Step 10: commit Task 4**

Run: `cd d:\voicepilot\voicepilot ; git add crates/trust-kernel/tests/w9_template_unit.rs ; git commit -m "test(w9p6): add UserSlot unit tests — JSON array / CSV fallback / empty fallback / \${user.xxx} resolve / run(plan, &[]) equivalence"`

---

## Task 5: 创建 w9_plan6_uia_dag_e2e.rs 测试文件骨架 + 共享 helpers

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs`

**目标:** 创建 E2E 测试文件骨架,包含 feature gate + 共享 helpers(`windows_gui_available` 探测 / `literal_text_template` / `note_capture_node` / `files_organize_node_with_slot` / `CWD_MUTEX` + `with_temp_cwd`),为 Task 6-7 的 2 个真实 E2E 场景做准备。

- [ ] **Step 1: 创建测试文件骨架 + feature gate + imports**

创建 `voicepilot/crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs`:

```rust
//! W9 Plan 6 — 真实 UIA GUI DAG E2E 测试。
//!
//! 2 个 `#[ignore]` 场景(手动运行,不参与 CI):
//!   1. `real_note_capture_dag_succeeds`:
//!      真实 note.capture 单节点 DAG → 打开记事本 + UIA 写 TODO +
//!      保存桌面 + Stronghold 加密补偿 + user_input taint 传播。
//!   2. `real_note_capture_files_organize_dag_succeeds`:
//!      note.capture → files.organize Slot 流水 DAG →
//!      note.capture 输出 save_path → files.organize input_template
//!      ${prev.output.save_path} → 真实文件移动。
//!
//! 测试模式(复用 W7 Plan 4 w7_plan4_uia_smoke.rs):
//! - `#![cfg(all(windows, feature = "voice", feature = "tauri", feature = "llm", feature = "uia", feature = "stronghold"))]` Windows + 全 feature 门控
//! - `#[ignore]` 标记 + `--ignored` 手动运行(W9 修复:测试名不含 "real_gui",用 `--ignored` 跑全部)
//! - `windows_gui_available()` 探测,不可用则 `eprintln!` + `return`(短路 passing)
//! - `CWD_MUTEX` 串行化文件系统操作(`with_temp_cwd` helper)
//!
//! 运行命令:
//!   cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold \
//!     --test w9_plan6_uia_dag_e2e -- --ignored
//!
//! 前置条件:
//! - Windows 10/11 真实 GUI 会话(非 SSH/无头)
//! - notepad.exe 可用(PATH 可解析)
//! - C:\Users\Public\Desktop 可写(场景 1 save_path 目标)
//! - Stronghold vault 已通过 W9 Plan 1 实现

#![cfg(all(windows, feature = "voice", feature = "tauri", feature = "llm", feature = "uia", feature = "stronghold"))]

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::types::ExtractedSlot;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan, DagStatus};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};

// ===== 共享 helpers =====

/// 串行化文件系统 CWD 操作的 process-wide mutex。
/// 真实 E2E 测试涉及 notepad 写文件 + files.organize 移动文件,
/// CWD 是 process-global 状态,必须串行化避免竞争。
static CWD_MUTEX: Mutex<()> = Mutex::new(());
```

- [ ] **Step 2: 实现 windows_gui_available 探测 helper**

```rust
/// 探测当前进程是否在真实 Windows GUI 会话中运行。
///
/// 返回 `false` 的情形:
/// - SSH 无头会话(无 desktop session)
/// - Windows Service 上下文(Session 0)
/// - CI 环境(GITHUB_ACTIONS / CI 环境变量设置)
///
/// 探测策略(三重保险):
/// 1. `std::env::var("SESSIONNAME")` 含 "Console" → 真实 console 会话
/// 2. `std::env::var("GITHUB_ACTIONS")` / `CI` 设置 → CI 环境,短路
/// 3. 尝试 `GetProcessWindowStation` + `GetUserObjectInformation` 检查 interactive
///    window station(简化版:仅检查 env,完整 WMI 探测延后)
fn windows_gui_available() -> bool {
    // CI 环境直接短路
    if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
        eprintln!("[windows_gui_available] CI environment detected, skipping");
        return false;
    }
    // SESSIONNAME 含 "Console" 表示真实交互会话(非 RDP/SSH)
    match std::env::var("SESSIONNAME") {
        Ok(s) if s.contains("Console") => true,
        Ok(s) if s.contains("RDP") => {
            // RDP 会话也算 GUI(远程桌面)
            eprintln!("[windows_gui_available] RDP session detected: {}", s);
            true
        }
        Ok(s) => {
            eprintln!("[windows_gui_available] non-Console session: {}, assuming no GUI", s);
            false
        }
        Err(_) => {
            eprintln!("[windows_gui_available] SESSIONNAME not set, assuming no GUI");
            false
        }
    }
}
```

- [ ] **Step 3: 实现 literal_text_template helper**

```rust
/// 构造一个 literal SlotTemplate(用于直接构造 DagPlan 的 input_template)。
/// 复用 w8_e2e_dag_smoke.rs:46 的同名 helper。
fn literal_text_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Literal(value.to_string()),
    }
}
```

- [ ] **Step 4: 实现 note_capture_node helper**

```rust
/// 构造 note.capture 节点。
/// input_template 是 JSON 字符串字面量,含 content + save_path 两个字段。
/// save_path 必须在 allowed_roots ["Documents", "Desktop"] 之内
/// (note_capture_manifest 约束,note_capture.rs:35)。
fn note_capture_node(id: &str, content: &str, save_path: &str) -> DagNode {
    let input_json = format!(
        r#"{{"content": "{}", "save_path": "{}"}}"#,
        content.replace('"', "\\\""),
        save_path.replace('"', "\\\"")
    );
    DagNode {
        node_id: id.into(),
        skill_id: "note.capture".into(),
        input_template: literal_text_template(&input_json),
        risk_ceiling: ELevel::E1,
    }
}
```

- [ ] **Step 5: 实现 files_organize_node_with_slot helper(Slot 流水模板)**

```rust
/// 构造 files.organize 节点,input_template 用 ${prev.output.save_path} Slot 流水。
///
/// files.organize 需要 source / filter / destination 三个字段(dispatcher.rs:167-173)。
/// 这里用 Concat 模板把 note.capture 的 save_path 注入 source 字段:
///   {"source": "${prev.output.save_path}", "filter": "*.txt", "destination": "Desktop/organized"}
///
/// 注意:note.capture 的 save_path 是单个文件路径,files.organize 的 source 是目录,
/// 实际 E2E 中 save_path 指向 Documents/ 子目录,files.organize 用其父目录作为 source。
/// 为简化 E2E 验证,这里 save_path 直接作为 source(files.organize search_files
/// 对单文件路径会返回该文件本身,filter "*.txt" 匹配)。
fn files_organize_node_with_slot(id: &str, destination: &str) -> DagNode {
    let dest_escaped = destination.replace('"', "\\\"");
    // W9 修复(P1-13):删除 format! 拼 input_json 的死代码,直接用 Concat 模板。
    // literal_text_template + ${...} 占位符在 SlotTemplateEngine::resolve
    // 中不会被解析(Literal 不含变量),改用 Concat 显式拼接。
    let template = TemplateExpr::Concat(vec![
        TemplateExpr::Literal(r#"{"source": ""#.into()),
        TemplateExpr::Var(VarRef {
            scope: VarScope::Prev,
            path: "output.save_path".into(),
        }),
        TemplateExpr::Literal(r#"", "filter": "*.txt", "destination": ""#.into()),
        TemplateExpr::Literal(dest_escaped),
        TemplateExpr::Literal(r#""}"#.into()),
    ]);
    DagNode {
        node_id: id.into(),
        skill_id: "files.organize".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Files,
            template,
        },
        risk_ceiling: ELevel::E2,
    }
}
```

- [ ] **Step 6: 实现 with_temp_cwd helper(复用 W7 Plan 4 模式)**

```rust
/// 在临时目录中执行 body,CWD 串行化。
///
/// 创建 temp dir + `Documents/` 子目录(适配 note.capture allowed_roots),
/// chdir 到 temp dir,执行 body(temp 路径传入),恢复原 CWD。
/// 用 CWD_MUTEX 串行化避免并发测试竞争 process-global CWD。
fn with_temp_cwd<F, R>(body: F) -> R
where
    F: FnOnce(&std::path::Path) -> R,
{
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let original_cwd = std::env::current_dir().expect("current_dir");
    let temp = std::env::temp_dir().join(format!(
        "w9p6-uia-e2e-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp).expect("create temp dir");
    std::fs::create_dir_all(temp.join("Documents")).expect("create Documents subdir");
    std::env::set_current_dir(&temp).expect("set_current_dir to temp");
    let result = body(&temp);
    // 恢复原 CWD(即使 body panic,Drop guard 也会释放 mutex)
    let _ = std::env::set_current_dir(&original_cwd);
    // 清理 temp dir(最佳努力,失败不报错 — notepad 可能仍持有文件锁)
    let _ = std::fs::remove_dir_all(&temp);
    result
}
```

- [ ] **Step 7: 实现 setup_kernel_with_stronghold helper(复用 W9 Plan 1/2/3 基础设施)**

```rust
/// 构造带 Stronghold vault 的 in-memory kernel(供 E2E 测试用)。
///
/// 依赖 W9 Plan 1(StrongholdVault::create)+ Plan 2(snapshot_encrypted 加密)
/// + Plan 3(TaintRepo)已完成。
fn setup_kernel_with_stronghold() -> Arc<TrustKernel> {
    let kernel = Arc::new(TrustKernel::open_in_memory().expect("open_in_memory"));
    // W9 Plan 1:创建 Stronghold vault(测试密码)
    let vault = trust_kernel::crypto::stronghold::StrongholdVault::create(
        "w9p6_test_password",
        &kernel.conn(),
    )
    .expect("StrongholdVault::create");
    kernel.set_stronghold_vault(vault);
    kernel
}
```

- [ ] **Step 8: 占位测试函数(验证骨架编译通过)**

```rust
/// 占位测试 — 验证 helpers 编译通过(非 #[ignore],参与 CI 编译门禁)。
#[test]
fn helpers_compile_check() {
    // 仅调用各 helper,不执行真实 GUI 逻辑
    let _node = note_capture_node("n1", "test", "Documents/test.txt");
    let _node2 = files_organize_node_with_slot("n2", "Desktop/organized");
    let _template = literal_text_template(r#"{"x": 1}"#);
    assert!(true, "helpers compile and construct without panic");
}
```

- [ ] **Step 9: cargo check 验证骨架编译(含 helpers)**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e`

Expected: PASS。若有 `StrongholdVault` 路径错误,确认 W9 Plan 1 已完成 + import 路径 `trust_kernel::crypto::stronghold::StrongholdVault` 正确;若有 `set_stronghold_vault` 方法找不到,确认 Plan 1 在 `kernel.rs` 暴露了该方法。

- [ ] **Step 10: 跑占位测试确认 helpers 运行不 panic**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e helpers_compile_check`

Expected: PASS(占位测试不依赖真实 GUI,可在任何环境跑)。

- [ ] **Step 11: 暂不 commit(Task 6-7 写完测试后统一 commit)**

---

## Task 6: 场景 1 — 真实 note.capture DAG(UiaAdapter 透传 + Stronghold + taint)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(加 thread-local UiaAdapter 注入点)
- Modify: `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs`(`dispatch_note_capture` 接入真实 adapter + output 含 save_path)
- Modify(条件性): `voicepilot/crates/trust-kernel/src/skills/note_capture.rs`(若需扩展返回值)
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs`(写场景 1 测试)

**目标:** 实现 W8 留下的 `dispatch_note_capture` 占位(当前返回 `Err("... Plan 6 work")`),通过 thread-local UiaAdapter 模式把 `WindowsUiaAdapter` 透传进 DagExecutor,并在 E2E 测试中验证:`[note.capture]` 单节点 DAG 真实打开记事本 + UIA 写 TODO + 保存桌面 + Stronghold 加密补偿记录 + user_input taint 传播。

**设计决策 — UiaAdapter 透传模式:**

W8 `dispatch_app_control` / `dispatch_note_capture` 注释明确 `UiaAdapter` 是 `!Send + !Sync`(COM apartment 模型),DagExecutor 不能持有 `Arc<dyn UiaAdapter>` 字段。采用 thread-local 注入:

```rust
// dag_executor.rs 顶部
thread_local! {
    static THREAD_LOCAL_UIA_ADAPTER: std::cell::RefCell<Option<std::sync::Arc<dyn trust_kernel::uiautomation::UiaAdapter>>> =
        std::cell::RefCell::new(None);
}

/// W9 Plan 6:在当前线程设置 thread-local UiaAdapter。
/// DagExecutor::run 调用 dispatch_skill_executor 时,dispatch_note_capture
/// 从 thread-local 取 adapter(若 Some)调真实 execute_note_capture。
/// 测试在 run() 前调 set_thread_local_uia_adapter(Some(adapter)),
/// run() 后调 set_thread_local_uia_adapter(None) 清理。
pub(crate) fn set_thread_local_uia_adapter(adapter: Option<std::sync::Arc<dyn trust_kernel::uiautomation::UiaAdapter>>) {
    THREAD_LOCAL_UIA_ADAPTER.with(|cell| {
        *cell.borrow_mut() = adapter;
    });
}
```

- [ ] **Step 0: 预验证 Win11 notepad 控件类型**

在实现 Task 6 之前,先跑一个探测测试确认 Win11 notepad 用的是 Edit 还是 RichEdit 控件:

```rust
#[test]
#[ignore = "Manual: verify Win11 notepad control type"]
fn probe_notepad_control_type() {
    // 用 uiautomation-rs 打开 notepad,查询文本框控件类型
    // 若是 Edit,用 set_text;若是 RichEdit,用 set_value
}
```

若 Win11 notepad 用 RichEdit,`execute_note_capture` 内 `set_text` 改为 `set_value`。

- [ ] **Step 1: 在 dag_executor.rs 加 thread-local UiaAdapter 注入点**

在 `dag_executor.rs` 顶部(use 段之后)追加:

```rust
// W9 Plan 6:thread-local UiaAdapter 注入点。
// UiaAdapter 是 !Send + !Sync(COM apartment 模型),不能用 Arc<dyn UiaAdapter>
// 作为 DagExecutor 字段(DagExecutor 需跨 await 点)。改用 thread-local:
// 测试在 run() 前调 set_thread_local_uia_adapter(Some(adapter)),
// dispatch_note_capture 从 thread-local 取 adapter 调真实 executor。
thread_local! {
    static THREAD_LOCAL_UIA_ADAPTER: std::cell::RefCell<Option<Arc<dyn crate::uiautomation::UiaAdapter>>> =
        std::cell::RefCell::new(None);
}

/// W9 Plan 6:在当前线程设置 thread-local UiaAdapter。
/// W9 修复(P1-10):可见性直接写 pub(crate) fn,与 Step 4 保持一致,消除前后矛盾。
pub(crate) fn set_thread_local_uia_adapter(adapter: Option<Arc<dyn crate::uiautomation::UiaAdapter>>) {
    THREAD_LOCAL_UIA_ADAPTER.with(|cell| {
        *cell.borrow_mut() = adapter;
    });
}

/// W9 Plan 6:从当前线程获取 thread-local UiaAdapter 克隆(若已设置)。
pub(crate) fn thread_local_uia_adapter() -> Option<Arc<dyn crate::uiautomation::UiaAdapter>> {
    THREAD_LOCAL_UIA_ADAPTER.with(|cell| cell.borrow().clone())
}
```

- [ ] **Step 1.5: 核实 execute_note_capture 实际签名**

执行 `grep -A 5 "pub fn execute_note_capture" voicepilot/crates/trust-kernel/src/skills/note_capture.rs` 确认签名:
- 参数顺序(kernel, input, approver, adapter)
- adapter 类型(&dyn UiaAdapter / Arc<dyn UiaAdapter> / 其他)
- 返回类型(Result<DispatchOutcome> / 其他)

若签名与 Task 6 Step 2 假设不符,调整调用代码。

- [ ] **Step 2: 修改 dispatch_note_capture 接入真实 adapter + output 含 save_path**

修改 `dispatcher.rs:268-290` 的 `dispatch_note_capture`:

```rust
// dispatcher.rs:268-290(修改前)
#[cfg(all(windows, feature = "uia"))]
fn dispatch_note_capture(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // W8 Plan 4 修复:同 dispatch_app_control,execute_note_capture 需要
    // `adapter: &dyn UiaAdapter`,Plan 2 漏传。保留字段校验,返回 Err。
    // Plan 6 集成时通过 DagExecutor 透传 adapter。
    let _ = (kernel, approver, task_id, step_id);
    let _content = extract_string(resolved_input, "content")?;
    let _save_path = extract_string(resolved_input, "save_path")?;
    let _input = NoteCaptureInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        content: _content,
        save_path: _save_path,
    };
    Err(KernelError::Skill(
        "note.capture in DagExecutor requires UiaAdapter plumbing — Plan 6 work".into(),
    ))
}

// dispatcher.rs:268-290(修改后)
#[cfg(all(windows, feature = "uia"))]
fn dispatch_note_capture(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // W9 Plan 6:从 thread-local 取 UiaAdapter(由 DagExecutor 测试在 run() 前注入)。
    // 若 thread-local 未设置(非 E2E 测试场景),返回 Err 引导调用方注入 adapter。
    let content = extract_string(resolved_input, "content")?;
    let save_path = extract_string(resolved_input, "save_path")?;
    let input = NoteCaptureInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        content: content.clone(),
        save_path: save_path.clone(),
    };

    let adapter = crate::skills::dag_executor::thread_local_uia_adapter().ok_or_else(|| {
        KernelError::Skill(
            "note.capture in DagExecutor requires thread-local UiaAdapter — call set_thread_local_uia_adapter before run()".into(),
        )
    })?;

    let returned_task_id = crate::skills::note_capture::execute_note_capture(
        kernel, &input, approver, adapter.as_ref(),
    )?;

    // W9 Plan 6:output 含 save_path(供下游 ${prev.output.save_path} Slot 流水)。
    // 既有 from_task_id 只返回 {task_id, step_id},这里自定义 output 追加 save_path。
    let output = serde_json::json!({
        "task_id": returned_task_id,
        "step_id": step_id,
        "save_path": save_path,
        "content": content,
    });
    Ok(DispatchOutcome {
        task_id: returned_task_id,
        step_id: step_id.to_string(),
        output,
        succeeded: true,
        error_cause: None,
    })
}
```

- [ ] **Step 3: dispatch_app_control 接入 thread-local adapter(必须修改)**

`dispatcher.rs:246-265` 的 `dispatch_app_control` 当前是 `Err("... Plan 6 work")` 占位,必须与 `dispatch_note_capture` 同步接入 thread-local adapter。

实现逻辑与 Step 2 类似:从 thread-local 取 adapter,调 `execute_app_control`。修改 `dispatcher.rs:233-265` 的 `dispatch_app_control`,模式同 Step 2:

```rust
// dispatch_app_control 修改后(模式同 dispatch_note_capture)
let adapter = crate::skills::dag_executor::thread_local_uia_adapter().ok_or_else(|| {
    KernelError::Skill(
        "quick.app_control in DagExecutor requires thread-local UiaAdapter".into(),
    )
})?;
let action = extract_string(resolved_input, "action")?;
let app_name = extract_string(resolved_input, "app_name")?;
let input = AppControlInput { /* ... */ };
let returned = crate::skills::app_control::execute_app_control(kernel, &input, approver, adapter.as_ref())?;
Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
```

- [ ] **Step 4: 验证 thread-local adapter 可见性(已在 Step 1 完成)**

W9 修复(P1-10):Step 1 已直接写最终可见性 `pub(crate) fn`,本 Step 仅做验证,不再调整:

```rust
// dag_executor.rs(Step 1 已写最终可见性,无需调整)
pub(crate) fn set_thread_local_uia_adapter(adapter: Option<Arc<dyn crate::uiautomation::UiaAdapter>>) { ... }

pub(crate) fn thread_local_uia_adapter() -> Option<Arc<dyn crate::uiautomation::UiaAdapter>> { ... }
```

> **注**:若集成测试(`tests/w9_plan6_uia_dag_e2e.rs`)需从 crate 外部调用 `set_thread_local_uia_adapter`,需在 `lib.rs` 或 `skills/mod.rs` 暴露 `pub use` 重导出,或改为 `pub fn`。当前实现按 spec 要求用 `pub(crate)`,如阻碍测试编译再调整。

- [ ] **Step 5: cargo check 验证 dispatcher 改动编译**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold`

Expected: PASS。若 `execute_note_capture` 签名不匹配(如 adapter 参数类型),回头核对 `note_capture.rs:84` 的 `execute_note_capture` 签名,确认 `adapter: &dyn UiaAdapter` 参数。

- [ ] **Step 6: 写场景 1 测试 — real_note_capture_dag_succeeds**

在 `w9_plan6_uia_dag_e2e.rs` 追加场景 1 测试:

```rust
/// 场景 1:真实 note.capture 单节点 DAG。
///
/// 验证点:
/// 1. 真实打开记事本(WindowsUiaAdapter.launch_app("notepad"))
/// 2. UIA 写 TODO 内容到 Edit 控件(set_text)
/// 3. 保存到 Desktop(w9_test_<uuid>.txt)
/// 4. Stronghold 加密补偿记录(snapshot_encrypted 非空)
/// 5. user_input taint 传播(taints 表有 user_input provenance 记录)
///
/// 运行命令:
///   cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold \
///     --test w9_plan6_uia_dag_e2e -- --ignored
#[tokio::test(flavor = "current_thread")]  // W9 修复:thread-local UiaAdapter 跨 await 点会丢失,必须单线程 runtime
#[ignore = "requires real Windows GUI + notepad.exe; run with --ignored --features voice,tauri,llm,uia,stronghold"]
async fn real_note_capture_dag_succeeds() {
    // 1. 探测 Windows GUI 会话(不可用则短路 passing)
    if !windows_gui_available() {
        eprintln!("[real_note_capture_dag_succeeds] Skipping: no Windows GUI session");
        return;
    }

    with_temp_cwd(|_temp| {
        // 2. 启动 kernel + Stronghold vault
        let kernel = setup_kernel_with_stronghold();

        // 3. 注入 thread-local WindowsUiaAdapter(真实 COM 初始化)
        let adapter: Arc<dyn trust_kernel::uiautomation::UiaAdapter> =
            Arc::new(trust_kernel::uiautomation::adapter::WindowsUiaAdapter::new()
                .expect("WindowsUiaAdapter::new"));
        trust_kernel::skills::dag_executor::set_thread_local_uia_adapter(Some(adapter.clone()));

        // 4. 构造 DAG:[note.capture]
        //    save_path 用 Desktop 绝对路径(allowed_roots 约束)
        //    W9 修复(P2):用 PathBuf 拼接,避免正斜杠在 Windows UIA 调用中触发路径解析问题。
        let save_path = std::path::PathBuf::from(r"C:\Users\Public\Desktop")
            .join(format!("w9p6_test_{}.txt", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        let dag_plan = DagPlan {
            plan_id: format!("w9-plan6-scenario1-{}", uuid::Uuid::new_v4()),
            user_goal: "打开记事本写 TODO 然后保存到桌面".into(),
            nodes: vec![note_capture_node(
                "n1",
                "W9 Plan 6 E2E 测试 TODO",
                &save_path,
            )],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        };

        // 5. DagExecutor::run(AutoApprover, user_slots=[])
        let executor = DagExecutor::new(
            kernel.clone(),
            Arc::new(AutoApprover),
            Arc::new(DagRepo::new()),
        );
        let result = executor
            .run(&dag_plan, &[])
            .expect("DagExecutor::run must not infra-error");

        // 6. 清理 thread-local adapter(避免泄漏到后续测试)
        trust_kernel::skills::dag_executor::set_thread_local_uia_adapter(None);

        // 7. 验证:DagStatus::Succeeded
        assert!(
            matches!(result.status, DagStatus::Succeeded),
            "expected Succeeded, got {:?}",
            result.status
        );

        // 8. 验证:节点 n1 Succeeded
        let n1_status = result
            .node_results
            .get("n1")
            .expect("n1 result must exist");
        assert!(
            n1_status.is_succeeded(),
            "n1 should be Succeeded, got {:?}",
            n1_status
        );

        // 9. 验证:真实文件已写入桌面
        let file_content = std::fs::read_to_string(&save_path)
            .expect("file should exist on Desktop after note.capture");
        assert_eq!(file_content, "W9 Plan 6 E2E 测试 TODO");

        // 10. 验证:Stronghold 加密补偿记录(snapshot_encrypted 非空)
        //     W9 修复:SQL 加 WHERE skill_id = 'note.capture' 过滤,避免其他 skill 的补偿记录干扰计数。
        let conn = kernel.conn();
        let encrypted_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NOT NULL AND snapshot_encrypted != '' AND skill_id = 'note.capture'",
            [], |row| row.get(0)
        ).expect("query compensations");
        assert!(
            encrypted_count > 0,
            "Stronghold encryption must produce non-null snapshot_encrypted"
        );

        // 11. 验证:user_input taint 传播(W9 Plan 3)
        let taints = trust_kernel::policy::taint_repo::TaintRepo::new()
            .list_by_provenance(&conn, "user_input")
            .expect("list_by_provenance user_input");
        assert!(
            !taints.is_empty(),
            "note.capture input must be taint-tracked as user_input"
        );
        // W9 修复:验证 taint 的 value_hash 实际等于 note.capture 输入的 SHA256,
        // 避免其他 user_input taint 干扰断言。
        let input_hash = compute_value_hash(&serde_json::json!({"content": "W9 Plan 6 E2E 测试 TODO"}));
        assert!(
            taints.iter().any(|t| t.value_hash == input_hash),
            "taint value_hash must match note.capture input"
        );

        // 12. 清理桌面测试文件(最佳努力)
        let _ = std::fs::remove_file(&save_path);

        eprintln!("[real_note_capture_dag_succeeds] 请手动关闭 Notepad 窗口");
    });
}
```

- [ ] **Step 7: cargo check 验证场景 1 编译**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e`

Expected: PASS。若 `is_succeeded()` 方法不存在,检查 `DagNodeStatus` 是否有该方法(W8 Plan 2 应已实现),否则用 `matches!(n1_status, DagNodeStatus::Succeeded(_))` 替代。

- [ ] **Step 8: 跑场景 1(手动,需真实 Windows GUI)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored real_note_capture_dag_succeeds`

Expected: 测试启动真实 notepad.exe,写入 "W9 Plan 6 E2E 测试 TODO" 到 Edit 控件,保存到桌面 `w9p6_test_<uuid>.txt`,DagStatus::Succeeded,Stronghold 加密记录非空,user_input taint 非空。测试通过后打印 "请手动关闭 Notepad 窗口"。

若测试在 CI/SSH 环境跑,`windows_gui_available()` 返回 false,测试短路 passing(不 fail)。

- [ ] **Step 9: 暂不 commit(Task 7 写完后统一 commit)**

---

## Task 7: 场景 2 — 真实 note.capture + files.organize DAG(Slot 流水验证)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs`(追加场景 2 测试)

**目标:** 验证 Slot 流水闭合的端到端正确性:`[note.capture, files.organize]` DAG 中,note.capture 输出的 `save_path` 通过 `${prev.output.save_path}` 模板解析注入 files.organize 的 source 字段,files.organize 真实执行文件移动(prepare → approve → commit_move → verify)。

- [ ] **Step 1: 写场景 2 测试 — real_note_capture_files_organize_dag_succeeds**

在 `w9_plan6_uia_dag_e2e.rs` 追加场景 2 测试:

```rust
/// 场景 2:真实 note.capture → files.organize Slot 流水 DAG。
///
/// 验证点:
/// 1. note.capture 真实打开记事本 + 写内容 + 保存到 Documents/w9p6_slot_<uuid>.txt
/// 2. files.organize 的 input_template 用 ${prev.output.save_path} Slot 流水
///    (Task 5 Step 5 的 files_organize_node_with_slot helper)
/// 3. Slot 流水正确解析:files.organize source = note.capture output.save_path
/// 4. files.organize 真实执行文件移动(prepare_move → commit_move → verify_move)
/// 5. DagStatus::Succeeded + 两节点均 Succeeded
/// 6. Stronghold 加密补偿记录(files.organize 移动产生补偿)
///
/// 运行命令:
///   cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold \
///     --test w9_plan6_uia_dag_e2e -- --ignored real_note_capture_files_organize
#[tokio::test(flavor = "current_thread")]  // W9 修复:thread-local UiaAdapter 跨 await 点会丢失,必须单线程 runtime
#[ignore = "requires real Windows GUI + notepad.exe; run with --ignored --features voice,tauri,llm,uia,stronghold"]
async fn real_note_capture_files_organize_dag_succeeds() {
    // 1. 探测 Windows GUI 会话
    if !windows_gui_available() {
        eprintln!("[real_note_capture_files_organize_dag_succeeds] Skipping: no Windows GUI session");
        return;
    }

    with_temp_cwd(|temp| {
        // 2. 启动 kernel + Stronghold
        let kernel = setup_kernel_with_stronghold();

        // 3. 注入 thread-local WindowsUiaAdapter
        let adapter: Arc<dyn trust_kernel::uiautomation::UiaAdapter> =
            Arc::new(trust_kernel::uiautomation::adapter::WindowsUiaAdapter::new()
                .expect("WindowsUiaAdapter::new"));
        trust_kernel::skills::dag_executor::set_thread_local_uia_adapter(Some(adapter.clone()));

        // 4. 构造 DAG:[note.capture → files.organize]
        //    note.capture save_path 用 Documents/w9p6_slot_<uuid>.txt
        //    (allowed_roots ["Documents", "Desktop"] 约束)
        let note_save_path = format!(
            "Documents/w9p6_slot_{}.txt",
            uuid::Uuid::new_v4()
        );
        // files.organize destination 用 Desktop/organized(allowed_roots 之内)
        // 注意:files.organize destination 必须是绝对路径或 allowed_roots 相对路径,
        // 这里用 temp/Documents/organized 作为移动目标(同 Documents 根下)
        // W9 修复(P1-15):核实 files.organize allowed_roots 校验逻辑 —
        // 若 allowed_roots 是相对根(["Documents", "Desktop"]),destination 必须是相对路径,
        // 用绝对路径会违反 allowed_roots 约束导致 Deny。改用相对路径 "Documents/organized"。
        let organize_dest = "Documents/organized".to_string();
        std::fs::create_dir_all(temp.join(&organize_dest)).expect("create organized dir");

        let dag_plan = DagPlan {
            plan_id: format!("w9-plan6-scenario2-{}", uuid::Uuid::new_v4()),
            user_goal: "写 TODO 然后用 files.organize 移动到 organized 目录".into(),
            nodes: vec![
                note_capture_node(
                    "n1",
                    "W9 Plan 6 Slot 流水测试",
                    &note_save_path,
                ),
                files_organize_node_with_slot("n2", &organize_dest),
            ],
            edges: vec![DagEdge {
                from: "n1".into(),
                to: "n2".into(),
                port_binding: None,
            }],
            loop_specs: HashMap::new(),
            max_total_steps: 10,
        };

        // 5. DagExecutor::run(AutoApprover, user_slots=[])
        let executor = DagExecutor::new(
            kernel.clone(),
            Arc::new(AutoApprover),
            Arc::new(DagRepo::new()),
        );
        let result = executor
            .run(&dag_plan, &[])
            .expect("DagExecutor::run must not infra-error");

        // 6. 清理 thread-local adapter
        trust_kernel::skills::dag_executor::set_thread_local_uia_adapter(None);

        // 7. 验证:DagStatus::Succeeded
        assert!(
            matches!(result.status, DagStatus::Succeeded),
            "expected Succeeded, got {:?}",
            result.status
        );

        // 8. 验证:两节点均 Succeeded
        let n1 = result.node_results.get("n1").expect("n1 result");
        let n2 = result.node_results.get("n2").expect("n2 result");
        assert!(n1.is_succeeded(), "n1 (note.capture) should Succeed: {:?}", n1);
        assert!(n2.is_succeeded(), "n2 (files.organize) should Succeed: {:?}", n2);

        // 9. 验证:Slot 流水正确 — files.organize 接收到 note.capture 的 save_path
        //    files.organize 的 DispatchOutcome.output 含 moved_count / destination
        //    (dispatcher.rs:36 from_skill_execution → tool_result.data)
        //    检查 n2 的 output(若 DagNodeStatus::Succeeded(out) 携带 output)
        if let trust_kernel::skills::dag_types::DagNodeStatus::Succeeded(ref out) = n2 {
            assert!(
                out.get("moved_count").is_some(),
                "files.organize output must contain moved_count, got: {:?}",
                out
            );
        }

        // 10. 验证:note.capture 写入的文件已被 files.organize 移动
        //     原文件 Documents/w9p6_slot_<uuid>.txt 不应存在
        //     目标路径 organized/ 下应有 w9p6_slot_<uuid>.txt
        let original_path = temp.join(&note_save_path);
        assert!(
            !original_path.exists(),
            "original file should be moved away: {:?}",
            original_path
        );
        // organized 目录下应有文件(moved_count >= 1)
        let organized_files: Vec<_> = std::fs::read_dir(&organize_dest)
            .expect("read organized dir")
            .collect();
        assert!(
            !organized_files.is_empty(),
            "organized dir should contain moved file(s)"
        );

        // 11. 验证:Stronghold 加密补偿(files.organize 移动产生补偿)
        let conn = kernel.conn();
        let encrypted_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("query compensations");
        assert!(
            encrypted_count >= 1,
            "files.organize move must produce Stronghold-encrypted compensation"
        );

        eprintln!("[real_note_capture_files_organize_dag_succeeds] 请手动关闭 Notepad 窗口");
    });
}
```

- [ ] **Step 2: cargo check 验证场景 2 编译**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e`

Expected: PASS。若 `DagNodeStatus::Succeeded(ref out)` 模式不匹配,核对 `dag_types.rs` 中 `DagNodeStatus::Succeeded(serde_json::Value)` 变体定义。

- [ ] **Step 3: 跑场景 2(手动,需真实 Windows GUI)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored real_note_capture_files_organize`

Expected: note.capture 写文件到 `Documents/w9p6_slot_<uuid>.txt`,files.organize 通过 `${prev.output.save_path}` 接收该路径,执行 `prepare_move` → `commit_move` → `verify_move`,文件移动到 `Documents/organized/`,DagStatus::Succeeded,两节点 Succeeded,Stronghold 补偿非空。

- [ ] **Step 4: 跑全部 ignored 测试(两个场景一起跑)**

W9 修复(P2):原命令 `-- --ignored real_gui` 会过滤名字含 "real_gui" 的测试,但本 plan 测试名是 `real_note_capture_dag_succeeds` / `real_note_capture_files_organize_dag_succeeds`,不含 "real_gui",会匹配 0 个测试。改用 `-- --ignored` 运行所有 ignored 测试:

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored`

Expected: 两个场景都跑(若 GUI 可用)或都短路 passing(若 GUI 不可用)。

- [ ] **Step 5: 跑非 ignored 测试(helpers_compile_check)确认默认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e`

Expected: `helpers_compile_check` PASS,两个 `#[ignore]` 测试被跳过(输出 "ignored")。

- [ ] **Step 6: commit Task 5-7(E2E 测试 + UiaAdapter 透传)**

Run: `cd d:\voicepilot\voicepilot ; git add crates/trust-kernel/src/skills/dag_executor.rs crates/trust-kernel/src/skills/dispatcher.rs crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs ; git commit -m "test(w9p6): add real UIA DAG E2E tests — note.capture single-node + note.capture→files.organize slot-flow with thread-local UiaAdapter plumbing"`

---

## Task 8: CWD_MUTEX 串行化 + 短路逻辑完善

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs`(完善 CWD_MUTEX + windows_gui_available 短路)

**目标:** 确保 2 个 E2E 测试在并发 `cargo test` 下不竞争 CWD(notepad 写文件 + files.organize 移动文件都涉及 CWD),并完善 `windows_gui_available()` 短路逻辑(避免 CI 误 fail)。

- [ ] **Step 1: 确认 CWD_MUTEX 已在 Task 5 Step 1 定义**

检查 `w9_plan6_uia_dag_e2e.rs` 顶部是否有 `static CWD_MUTEX: Mutex<()> = Mutex::new(());`(Task 5 Step 1 已加)。若无,补上。

- [ ] **Step 2: 确认 with_temp_cwd 已用 CWD_MUTEX 串行化**

检查 Task 5 Step 6 的 `with_temp_cwd` 实现首行是否 `let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());`。这是关键 — 保证两个 E2E 测试不会同时 chdir 到不同 temp dir 导致 CWD 竞争。

- [ ] **Step 3: 完善 windows_gui_available 短路逻辑**

检查 Task 5 Step 2 的 `windows_gui_available` 实现。补充 `SSH_CLIENT` / `SSH_CONNECTION` 环境变量检测(SSH 会话通常无 GUI):

```rust
// windows_gui_available 补充 SSH 检测(在 CI 检测之后)
fn windows_gui_available() -> bool {
    // CI 环境短路
    if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
        eprintln!("[windows_gui_available] CI environment detected, skipping");
        return false;
    }
    // SSH 会话通常无交互桌面(Windows OpenSSH 默认 Session 0)
    if std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_CONNECTION").is_ok() {
        eprintln!("[windows_gui_available] SSH session detected, skipping");
        return false;
    }
    // SESSIONNAME 含 "Console" 表示真实交互会话
    match std::env::var("SESSIONNAME") {
        Ok(s) if s.contains("Console") => true,
        Ok(s) if s.contains("RDP") => true,
        Ok(s) => {
            eprintln!("[windows_gui_available] non-Console session: {}, assuming no GUI", s);
            false
        }
        Err(_) => {
            eprintln!("[windows_gui_available] SESSIONNAME not set, assuming no GUI");
            false
        }
    }
}
```

- [ ] **Step 4: 验证两个 E2E 测试体首行都调 windows_gui_available 短路**

检查 Task 6 Step 6 场景 1 + Task 7 Step 1 场景 2 的测试体首行:

```rust
if !windows_gui_available() {
    eprintln!("[...] Skipping: no Windows GUI session");
    return;
}
```

确认两个测试都有此短路(避免 CI / SSH 环境 fail)。

- [ ] **Step 5: 验证两个 E2E 测试体都在 with_temp_cwd 闭包内**

检查场景 1 / 场景 2 的核心逻辑(构造 DAG + run + 断言)都在 `with_temp_cwd(|temp| { ... })` 闭包内,确保 CWD_MUTEX 串行化覆盖整个测试体。

- [ ] **Step 6: 验证 thread-local adapter 清理(set_thread_local_uia_adapter(None))**

检查场景 1 / 场景 2 在 `executor.run(...)` 之后都调用了 `set_thread_local_uia_adapter(None)` 清理,避免 thread-local adapter 泄漏到后续测试(若 adapter 持有 COM 引用,泄漏会导致 COM apartment 状态混乱)。

- [ ] **Step 7: cargo test 跑全部测试(含 ignored + 非 ignored)确认无竞争**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored --test-threads=1`

Expected: `--test-threads=1` 强制串行,两个场景依次跑(若 GUI 可用)或依次短路(若不可用),无 CWD 竞争 panic。

> **双重保险(W9 修复)**:CWD_MUTEX 已串行化所有改 CWD 的测试,`--test-threads=1` 是额外保险,避免 thread-local UiaAdapter 跨线程污染。两者并存确保即使 CWD_MUTEX 实现有遗漏,也不会因并发引发 COM apartment 状态混乱。

- [ ] **Step 8: cargo test 多线程验证(CWD_MUTEX 应保证安全)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored`

Expected: 即使 `--test-threads` 默认(多线程),`CWD_MUTEX` 应串行化两个测试,无竞争。若有 panic 形如 "current_dir" / "set_current_dir",说明 CWD_MUTEX 未覆盖某段代码,回头检查。

- [ ] **Step 9: commit Task 8(CWD_MUTEX + 短路逻辑完善)**

Run: `cd d:\voicepilot\voicepilot ; git add crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs ; git commit -m "test(w9p6): harden CWD_MUTEX serialization + windows_gui_available short-circuit (CI/SSH/SESSIONNAME detection)"`

---

## Task 9: 手动运行验证文档

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs`(在文件头注释中补充运行前置条件)

**目标:** 在测试文件头注释中明确记录手动运行的前置条件 + 运行命令 + 预期输出 + 故障排查,供后续维护者参考。

- [ ] **Step 1: 完善测试文件头注释(运行前置条件 + 故障排查)**

在 `w9_plan6_uia_dag_e2e.rs` 文件头(Task 5 Step 1 已写的注释)追加 "运行前置条件" + "故障排查" 段落:

```rust
//! ## 运行前置条件
//!
//! 1. **操作系统**:Windows 10/11(不支持 Linux/macOS,uiautomation-rs Windows-only)
//! 2. **GUI 会话**:真实交互桌面(非 SSH/无头/CI)
//!    - SESSIONNAME 环境变量含 "Console" 或 "RDP"
//!    - 或手动在 cmd/PowerShell 交互终端运行(非 Windows Service)
//! 3. **notepad.exe**:PATH 可解析(默认 C:\Windows\System32\notepad.exe)
//! 4. **桌面可写**:`C:\Users\Public\Desktop` 目录可写(场景 1 save_path 目标)
//! 5. **Documents 可写**:当前用户 Documents 目录可写(场景 2 save_path 目标)
//! 6. **Stronghold vault**:W9 Plan 1 已实现,测试内自动创建(in-memory,测试密码)
//! 7. **feature 组合**:`voice,tauri,llm,uia,stronghold`(必须全部启用)
//!
//! ## 运行命令
//!
//! ```powershell
//! cd d:\voicepilot\voicepilot
//! cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold `
//!   --test w9_plan6_uia_dag_e2e -- --ignored
//! ```
//!
//! 运行单个场景:
//!
//! ```powershell
//! cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold `
//!   --test w9_plan6_uia_dag_e2e -- --ignored real_note_capture_dag_succeeds
//! ```
//!
//! ## 预期输出
//!
//! - 场景 1:真实打开 Notepad 窗口 + 写入 "W9 Plan 6 E2E 测试 TODO" +
//!   桌面生成 w9p6_test_<uuid>.txt + 测试 PASS + 打印 "请手动关闭 Notepad 窗口"
//! - 场景 2:真实打开 Notepad + 写文件到 Documents/w9p6_slot_<uuid>.txt +
//!   files.organize 移动到 Documents/organized/ + 测试 PASS
//! - CI/SSH 环境:两个场景短路 passing(输出 "Skipping: no Windows GUI session"),
//!   不 fail
//!
//! ## 故障排查
//!
//! - **"WindowsUiaAdapter::new" panic**:COM 初始化失败,确认在交互桌面运行
//! - **"launch_app('notepad') should succeed" panic**:notepad.exe 不在 PATH,
//!   检查 `where notepad` 是否返回路径
//! - **"find_window('Notepad') should be found" panic**:Notepad 窗口未在 3s 内出现,
//!   可能系统负载高,尝试增加 adapter 超时或重跑
//! - **"set_text on Edit control should succeed" panic**:Edit 控件未就绪,
//!   Notepad 新版(Win11)可能用 RichEdit,确认 uiautomation-rs 0.16 兼容
//! - **"Stronghold encryption must produce non-null snapshot_encrypted" panic**:
//!   W9 Plan 1/2 未完成,或 vault 未解锁,检查 setup_kernel_with_stronghold
//! - **"user_input taint must be propagated" panic**:W9 Plan 3 未完成,
//!   或 dispatcher 未调 TaintRepo::upsert,检查 dispatcher.rs 传播逻辑
//! - **"original file should be moved away" panic**:files.organize 未真实移动文件,
//!   检查 Slot 流水 ${prev.output.save_path} 是否解析成功(看 n2 的 input JSON)
```

- [ ] **Step 2: cargo check 验证注释不破坏编译**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e`

Expected: PASS(注释不影响编译)。

- [ ] **Step 3: commit Task 9**

Run: `cd d:\voicepilot\voicepilot ; git add crates/trust-kernel/tests/w9_plan6_uia_dag_e2e.rs ; git commit -m "docs(w9p6): document manual run prerequisites + troubleshooting for real UIA DAG E2E tests"`

---

## Task 10: cargo check + clippy + 非门控测试数 + PROGRESS.md + commit

**Files:**
- Modify: `docs/PROGRESS.md`(W9 Plan 6 完成状态 + Slot 流水欠债闭合记录)

**目标:** 跑 `voice,tauri,llm,uia,stronghold` feature 组合的 cargo check + clippy `-D warnings` + 全量非门控测试数统计 + 更新 PROGRESS.md + 最终 commit。

- [ ] **Step 1: cargo check 验证全 feature 组合编译**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia,stronghold`

Expected: PASS。若有 warning(非 error),记录但继续(Task 10 Step 2 clippy 会捕获)。

- [ ] **Step 2: cargo check 验证 default feature(无 uia/stronghold)也通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`

Expected: PASS(W8 既有测试在 default feature 下也应编译,Task 3 调用点更新应兼容)。

- [ ] **Step 3: cargo clippy 验证 0 警告(代表性 feature 组合)**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features voice,tauri,llm,uia,stronghold -- -D warnings`

Expected: 0 警告。常见 clippy lint 修复(参考 W8 Plan 6 Conventions):
- `explicit_auto_deref` → 用 `&kernel.conn()` 不用 `&*kernel.conn()`
- `needless_borrows_for_generic_args` → `hasher.update(x.to_le_bytes())` 不要 `&x.to_le_bytes()`
- `manual_inspect` → 用 `.inspect_err(|_| { ... })` 替代 `.map_err(|e| { ...; e })`
- 未使用 import → 删除

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`

Expected: 0 警告(default feature 组合)。

- [ ] **Step 4: cargo clippy 验证测试代码也 0 警告**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features voice,tauri,llm,uia,stronghold --tests -- -D warnings`

Expected: 0 警告(含 `w9_plan6_uia_dag_e2e.rs` / `w9_template_unit.rs` / W8 既有测试)。

- [ ] **Step 5: 跑 W9 Plan 4 单元测试确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w9_template_unit`

Expected: 5 个测试全 PASS(Task 4 Step 8 已验证,此处回归确认)。

- [ ] **Step 6: 跑 W8 既有测试确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features voice,tauri,llm,uia,stronghold --test w8_plan2_dag_executor --test w8_plan2_dag_e2e --test w8_plan2_audit_events --test w8_plan3_loop_node --test w8_e2e_dag_smoke`

Expected: 全 PASS(Task 3 Step 10 已验证,此处回归确认)。

- [ ] **Step 7: 统计非门控测试数(全 workspace)**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri,llm,uia,stronghold --no-run 2>&1 | Select-String "test result"`

Expected: 输出各测试 binary 的测试数。汇总后总数应 ≥ 520(W8 收尾 465 + W9 Plan 6 新增 ~8 = ~473,加 W9 Plan 1-5 新增约 50,总计 ~520)。spec §9 测试矩阵预估 W9 完成 ≈ 520。

备选命令(列测试名):

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri,llm,uia,stronghold -- --list 2>&1 | Measure-Object -Line`

Expected: 行数 ≥ 520(每行一个测试名)。

- [ ] **Step 8: 更新 PROGRESS.md W9 Plan 6 完成状态**

在 `docs/PROGRESS.md` 的 W9 段落追加 Plan 6 完成记录:

```markdown
### W9 Plan 6:真实 UIA GUI DAG E2E + Slot 流水闭合 ✅

**完成日期:** 2026-07-28
**commit:** `<task 10 step 11 commit hash>`

**闭合欠债:**
- W8 spec §8 #10 Slot 流水欠债:`DagExecutor::run_simple_node` 中 `user_slots` 传 `&[]` →
  W9 Plan 6 扩展 `run(plan, user_slots)` 签名(breaking change),透传到
  `SlotTemplateEngine::resolve` + `resolve_iterable`,W8 既有 ~40 处调用点统一改为
  `run(plan, &[])`(行为等价 W8)
- W8 `IterableSource::UserSlot` 返回 `Err("user_slots not wired; see Plan 5")` →
  W9 Plan 6 实现查表解析(JSON 数组 + CSV fallback)
- W8 `dispatch_note_capture` / `dispatch_app_control` 返回
  `Err("... Plan 6 work")` → W9 Plan 6 通过 thread-local UiaAdapter 模式接入
  真实 `WindowsUiaAdapter`(规避 `!Send + !Sync` COM apartment 约束)

**新增测试:**
- `tests/w9_template_unit.rs`(5 个单元测试,验证 UserSlot 解析 / CSV fallback /
  空数组 fallback / ${user.xxx} resolve / run(plan, &[]) 等价)
- `tests/w9_plan6_uia_dag_e2e.rs`(2 个 `#[ignore]` 真实 E2E 场景:
  note.capture 单节点 + note.capture→files.organize Slot 流水)

**非门控测试数:** 465(W8)→ ~473(W9 Plan 6 新增 8),累计随 Plan 1-5 新增约 ~520

**已知偏离:**
- thread-local UiaAdapter 模式限制:DagExecutor::run 必须在与 set_thread_local_uia_adapter
  相同的线程调用(跨 await 点可能丢失 thread-local)。当前 E2E 测试在单线程 tokio
  runtime 下验证通过;若未来 DagExecutor 需跨线程调度,需改用 scoped thread + adapter
  参数透传(延后 W10+)
- windows_gui_available 探测基于环境变量(SESSIONNAME / CI / SSH_CLIENT),
  非完整 WMI 检测;复杂会话场景(如 Windows Sandbox)可能误判,延后 W10+ 完善
```

- [ ] **Step 9: 更新 PROGRESS.md 里程碑表**

在 PROGRESS.md 里程碑表追加 W9 Plan 6 行:

```markdown
| 日期 | 里程碑 | commit | 备注 |
|---|---|---|---|
| 2026-07-28 | W9 Plan 6:Slot 流水闭合 + 真实 UIA DAG E2E | `<hash>` | 闭合 W8 §8 #10 欠债;2 个 #[ignore] E2E 场景 |
```

- [ ] **Step 10: git add + commit PROGRESS.md**

Run: `cd d:\voicepilot\voicepilot ; git add docs/PROGRESS.md ; git commit -m "docs(w9p6): update PROGRESS.md — Slot flow debt closed + 2 real UIA E2E scenarios + thread-local UiaAdapter pattern"`

- [ ] **Step 11: 最终 commit(空 commit 标记 W9 Plan 6 完成,可选)**

若需标记 W9 Plan 6 里程碑(参考 W8 Plan 6 收尾模式):

Run: `cd d:\voicepilot\voicepilot ; git commit --allow-empty -m "docs(w9p6): W9 Plan 6 complete — Slot flow debt closed + real UIA DAG E2E"`

- [ ] **Step 12: 验证 git log 确认 commit 历史**

Run: `cd d:\voicepilot\voicepilot ; git log --oneline -10`

Expected: 看到形如:
- `docs(w9p6): W9 Plan 6 complete — ...`(空 commit,若 Step 11 执行)
- `docs(w9p6): update PROGRESS.md — ...`
- `test(w9p6): harden CWD_MUTEX serialization + ...`
- `docs(w9p6): document manual run prerequisites + ...`
- `test(w9p6): add real UIA DAG E2E tests — ...`
- `test(w9p6): add UserSlot unit tests — ...`
- `feat(w9p6): close slot-flow debt — ...`

---

## Commit Message 格式

本 plan 的 commit message 遵循以下格式(参考 W8/W9 Conventions):

| 类型 | 格式 | 用途 |
|---|---|---|
| `feat(w9p6)` | `feat(w9p6): close slot-flow debt — DagExecutor::run takes user_slots + IterableSource::UserSlot implemented + W8 call sites updated to run(plan, &[])` | Task 1-3:Slot 流水闭合 + 调用点更新 |
| `test(w9p6)` | `test(w9p6): add UserSlot unit tests — JSON array / CSV fallback / empty fallback / ${user.xxx} resolve / run(plan, &[]) equivalence` | Task 4:单元测试 |
| `test(w9p6)` | `test(w9p6): add real UIA DAG E2E tests — note.capture single-node + note.capture→files.organize slot-flow with thread-local UiaAdapter plumbing` | Task 5-7:E2E 测试 + UiaAdapter 透传 |
| `test(w9p6)` | `test(w9p6): harden CWD_MUTEX serialization + windows_gui_available short-circuit (CI/SSH/SESSIONNAME detection)` | Task 8:串行化 + 短路逻辑 |
| `docs(w9p6)` | `docs(w9p6): document manual run prerequisites + troubleshooting for real UIA DAG E2E tests` | Task 9:手动运行文档 |
| `docs(w9p6)` | `docs(w9p6): update PROGRESS.md — Slot flow debt closed + 2 real UIA E2E scenarios + thread-local UiaAdapter pattern` | Task 10:PROGRESS.md 更新 |
| `docs(w9p6)` | `docs(w9p6): W9 Plan 6 complete — Slot flow debt closed + real UIA DAG E2E` | Task 11(可选):空 commit 标记里程碑 |

**注意:**
- `feat(w9p6)` 用于源码功能变更(dag_executor.rs / dispatcher.rs 签名扩展 + UiaAdapter 透传 + IterableSource::UserSlot 实现)
- `test(w9p6)` 用于测试代码(w9_template_unit.rs / w9_plan6_uia_dag_e2e.rs / W8 调用点更新)
- `fix(w9p6)` 用于修复(若 Task 3 调用点更新发现 W8 既有 bug)
- `docs(w9p6)` 用于文档(PROGRESS.md / 测试文件头注释)
- 单行 commit message(PowerShell 不支持 heredoc),用 `—` 分隔主标题与细节

---

## Self-Review 核对

**1. Spec 覆盖(spec §2.6 Plan 6 范围):**

| spec §2.6 要求 | 对应 Task | 覆盖 |
|---|---|---|
| `DagExecutor::run` 签名扩展 `run(plan, user_slots)` | Task 1 Step 2 | ✅ |
| `run_simple_node` 传 user_slots(不再传 `&[]`) | Task 1 Step 3 | ✅ |
| `IterableSource::UserSlot` 实现(当前返回 Err) | Task 2 Step 2 | ✅ |
| 更新 W8 既有调用点为 `run(plan, &[])` | Task 3 Step 1-7 | ✅ |
| 新增 `w9_plan6_uia_dag_e2e.rs` | Task 5 | ✅ |
| 真实 note.capture DAG 场景 | Task 6 | ✅ |
| 真实 note + files DAG Slot 流水场景 | Task 7 | ✅ |
| `#[ignore]` + 探测 Windows GUI + 短路 passing | Task 5 Step 2 + Task 8 | ✅ |
| feature 组合 `voice,tauri,llm,uia,stronghold` | Task 5 Step 1 + Task 10 | ✅ |

**2. 占位符扫描:** 无 "TBD" / "TODO" / "fill in details" / "Similar to Task N" 等占位符;每个步骤含具体代码 / 命令 / 预期输出。

**3. 类型一致性:**
- `ExtractedSlot`(来自 `llm::types`)— Task 1/2/4 一致引用
- `IterableSource::UserSlot { slot_kind: String }` — Task 2 Step 2 / Task 4 Step 2 一致(核实 `dag_types.rs:63` enum 定义)
- `SlotTemplateEngine::resolve(template, node_outputs, user_slots, iter_var, prev_node_id)` — Task 1 Step 3 / Task 4 Step 6 一致(5 参数)
- `DagExecutor::run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot])` — Task 1/3/4/6/7 一致
- `dispatch_note_capture` 返回 `DispatchOutcome { output: {save_path, ...} }` — Task 6 Step 2 / Task 7 Step 1 一致
- `set_thread_local_uia_adapter(Option<Arc<dyn UiaAdapter>>)` — Task 6 Step 1/4 / Task 6 Step 6 / Task 7 Step 1 一致

---

**End of W9 Plan 6 Implementation Plan**

---

## W9 审查修复记录

本段落记录 W9 Plan 6 审查阶段发现并修复的所有缺陷。修复原则:使用 Edit 工具精确替换,不重写整个文件;每项修复在原位置加 `W9 修复(Pxx):` 注释,便于追溯。

### P0-1: ExtractedSlot 字段误用

- **位置:** Task 2 Step 2 + Task 4 Step 2/4/6
- **问题:** ExtractedSlot 实际只有 `kind: SlotKind` / `raw: String` / `high_risk: bool` 三个字段(见 `src/llm/types.rs:23-30`),Plan 6 误用 `value` / `start` / `end` 不存在字段;W8 既有 `VarScope::User` 分支用 `slot.raw`
- **修复:**
  1. Task 2 Step 2 的 `slot.value`(2 处:`serde_json::from_str` 与 `split(',')`)全部改为 `slot.raw`,注释从 "slot.value 是 String" 改为 "slot.raw 是 String"
  2. Task 4 Step 2(测试 1)/ Step 4(测试 2)/ Step 6(测试 4)三处 `ExtractedSlot` 构造改为三字段 `kind: SlotKind::Files, raw: ..., high_risk: false`,删除 `value` / `start` / `end`
  3. 在 Task 2 Step 2 代码块后追加注释:"**ExtractedSlot 实际字段**:`kind: SlotKind` / `raw: String` / `high_risk: bool`(见 `src/llm/types.rs:23-30`)。W8 既有 `VarScope::User` 分支用 `slot.raw`,Plan 6 保持一致。"

### P0-2: IterableSource::UserSlot 类型与 Plan 7 不一致

- **位置:** Task 4 Step 2 vs Plan 7 Task 1 Step 9 D 组
- **问题:** Plan 6 用 struct variant `{ slot_kind: String }`(正确),Plan 7 用 tuple variant `(SlotKind::Text)`(错误)
- **修复:** Task 4 Step 2 保持 struct variant,在代码块前加注释:"**IterableSource::UserSlot 是 struct variant**:`UserSlot { slot_kind: String }`(见 `dag_types.rs:67`),不是 tuple variant。Plan 7 Task 1 Step 9 D 组测试需同步用 struct variant 构造。"

### P0-3: VarScope::Named 不存在

- **位置:** Conventions(约行 81)
- **问题:** `VarScope::Named("n1".into())` 不存在,实际是 `VarScope::Step(String)`
- **修复:** 改为 `VarScope::Step("n1".into())`,加注释:"**VarScope 实际变体**:`Prev` / `Step(String)` / `User` / `Iter`(见 `template.rs:74-83`),没有 `Named` 变体。引用具名节点用 `Step("n1".into())`。"

### P0-4: resolve_iterable 签名描述错误

- **位置:** Task 2 Step 1
- **问题:** "修改前" 写 3 参数(source+node_outputs+prev_node_id),实际 W8 既有只有 2 参数(source+node_outputs)
- **修复:** 修正 "修改前" 代码块为 2 参数 `fn resolve_iterable(&self, source: &IterableSource, node_outputs: &HashMap<String, DagNodeResult>) -> Result<Vec<serde_json::Value>>;`,并标注 "修改前(W8 既有,2 参数)" 与 "修改后(W9 Plan 6,4 参数,加 user_slots + prev_node_id)"

### P1-9: dispatch_note_capture 行号偏移 + execute_note_capture 签名未核实

- **位置:** Task 6 Step 2
- **修复:** 在 Task 6 Step 2 之前加 "Step 1.5: 核实 execute_note_capture 实际签名",执行 `grep -A 5 "pub fn execute_note_capture"` 确认参数顺序 / adapter 类型 / 返回类型,若签名与假设不符则调整调用代码

### P1-9 (错误类型不一致): Task 6 Step 2 vs spec §2.6

- **位置:** Task 2 Step 2(`IterableSource::UserSlot` 分支错误返回)
- **问题:** 原 Plan 6 用 `KernelError::Skill(format!(...))`,与 spec §2.6 要求的 `TemplateError::UserSlotNotFound` 不一致
- **修复:** `ok_or_else` 内改为 `TemplateError::UserSlotNotFound(slot_kind.clone()).into()`,加注释 "W9 修复(P1-9 错误类型不一致):与 spec §2.6 一致用 TemplateError::UserSlotNotFound"

### P1-10: thread_local_uia_adapter 可见性前后矛盾

- **位置:** Task 6 Step 1 vs Step 4
- **问题:** Step 1 写 `pub fn set_thread_local_uia_adapter` + `fn thread_local_uia_adapter`(私有),Step 4 又说改为 `pub fn` + `pub(crate) fn`,前后矛盾
- **修复:** Task 6 Step 1 直接写最终可见性,两个函数均为 `pub(crate) fn`(含设计决策块同步更新);Step 4 改为验证步骤(仅做验证,不再调整可见性),并加注:若集成测试需从 crate 外部调用,需 `pub use` 重导出或改为 `pub fn`

### P1-12: dispatch_app_control 实际也是占位,Plan 6 列为"条件性"修改不够坚决

- **位置:** Task 6 Step 3
- **修复:** 改为 "Step 3: dispatch_app_control 接入 thread-local adapter(必须修改)",明确 `dispatcher.rs:246-265` 的 `dispatch_app_control` 当前是 `Err("... Plan 6 work")` 占位,必须与 `dispatch_note_capture` 同步接入 thread-local adapter

### P1-13: Slot 流水模板实现有死代码

- **位置:** Task 5 Step 5(`files_organize_node_with_slot` helper)
- **问题:** 函数内 `let input_json = format!(...)` 计算后从未使用(死代码),实际用 Concat 模板构造
- **修复:** 删除 `format!` 拼 `input_json` 的死代码,直接用 Concat 模板,加注释 "W9 修复(P1-13):删除 format! 拼 input_json 的死代码,直接用 Concat 模板"

### P1-14: Task 4 测试 1-3 用 task.explain skill

- **位置:** Task 4 测试 1-3(Step 2/4/5)
- **修复:** 在 Task 4 Step 2 代码块前加注释:"**task.explain 依赖 LLM**:测试 1-3 用 `--features llm`,若 task.explain 需真实 LLM 调用可能 Failed。建议用 mock LLM 或改为不依赖 LLM 的 skill(如 `task.echo` 若存在)。若测试 Failed 而非 Succeeded,核实 task.explain 的 mock 行为。"

### P1-15: Task 7 测试 organize_dest 路径违反 allowed_roots

- **位置:** Task 7 Step 1
- **问题:** `organize_dest` 用 `format!("{}/Documents/organized", temp.to_string_lossy()...)` 构造绝对路径,可能违反 files.organize 的 allowed_roots 约束
- **修复:** 改为相对路径 `let organize_dest = "Documents/organized".to_string();`,`std::fs::create_dir_all` 用 `temp.join(&organize_dest)`,加注释 "W9 修复(P1-15):核实 files.organize allowed_roots 校验逻辑 — 若 allowed_roots 是相对根,destination 必须是相对路径"

### P1 (thread-local async): #[tokio::test] async + thread-local UiaAdapter 跨 await 点丢失

- **位置:** Task 5/6/7 测试(`real_note_capture_dag_succeeds` / `real_note_capture_files_organize_dag_succeeds`)
- **问题:** 默认 `#[tokio::test]` 用多线程 runtime,thread-local UiaAdapter 跨 await 点会丢失
- **修复:** 所有 `#[tokio::test]` 改为 `#[tokio::test(flavor = "current_thread")]`,加注释 "W9 修复:thread-local UiaAdapter 跨 await 点会丢失,必须单线程 runtime"

### P1 (Stronghold 补偿验证未过滤 skill_id): Task 6 Step 6

- **位置:** Task 6 Step 6(SQL 查询)
- **问题:** 原 SQL `SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NOT NULL` 未过滤 skill_id,可能匹配其他 skill 的补偿记录
- **修复:** SQL 改为 `SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NOT NULL AND snapshot_encrypted != '' AND skill_id = 'note.capture'`,加注释说明过滤原因

### P1 (user_input taint 验证不严谨): Task 6 Step 6

- **位置:** Task 6 Step 6(taint 断言)
- **问题:** 原断言仅检查 `taints.is_empty()`,未验证 taint 的 value_hash 实际等于 note.capture 输入的 SHA256,可能匹配其他 user_input taint
- **修复:** 加 `value_hash` 断言:`let input_hash = compute_value_hash(&serde_json::json!({"content": "W9 Plan 6 E2E 测试 TODO"})); assert!(taints.iter().any(|t| t.value_hash == input_hash), ...)`

### P1 (notepad Win11 RichEdit 兼容性): Task 9 故障排查

- **位置:** Task 6(新增 Step 0)
- **问题:** Win11 notepad 可能用 RichEdit 控件而非 Edit 控件,`set_text` 调用可能失败
- **修复:** 在 Task 6 Step 1 之前加 "Step 0: 预验证 Win11 notepad 控件类型",先跑一个 `#[ignore]` 探测测试 `probe_notepad_control_type` 确认控件类型,若为 RichEdit 则 `execute_note_capture` 内 `set_text` 改为 `set_value`

### P2 (Task 7 Step 4 运行命令矛盾): -- --ignored real_gui

- **位置:** Task 7 Step 4 + Conventions + Task 5 Step 1 测试文件头 + Task 6 Step 6 docstring
- **问题:** `-- --ignored real_gui` 会过滤名字含 "real_gui" 的测试,但本 plan 测试名是 `real_note_capture_dag_succeeds` / `real_note_capture_files_organize_dag_succeeds`,不含 "real_gui",会匹配 0 个测试
- **修复:** 所有 `--ignored real_gui` 改为 `--ignored`(共 5 处:Conventions 行 75 / Task 5 Step 1 头注释 2 处 / Task 6 Step 6 docstring / Task 7 Step 4),并在 Task 7 Step 4 加注释说明原因

### P2 (Task 6 桌面路径正斜杠): C:/Users/Public/Desktop/...

- **位置:** Task 6 Step 6(`save_path` 构造)
- **问题:** `format!("C:/Users/Public/Desktop/...")` 用正斜杠,Windows UIA 调用中可能触发路径解析问题
- **修复:** 改用 `std::path::PathBuf::from(r"C:\Users\Public\Desktop").join(format!("w9p6_test_{}.txt", uuid::Uuid::new_v4())).to_string_lossy().to_string()`,加注释 "W9 修复(P2):用 PathBuf 拼接,避免正斜杠在 Windows UIA 调用中触发路径解析问题"

### P2 (Task 8 --test-threads=1): 与 CWD_MUTEX 双重保险

- **位置:** Task 8 Step 7
- **修复:** 保留 `--test-threads=1`,加注释 "**双重保险(W9 修复)**:CWD_MUTEX 已串行化所有改 CWD 的测试,`--test-threads=1` 是额外保险,避免 thread-local UiaAdapter 跨线程污染。两者并存确保即使 CWD_MUTEX 实现有遗漏,也不会因并发引发 COM apartment 状态混乱。"

### 跨 Plan 一致性: spec §11 调用点数字 8 处 vs 实测 44 处

- **位置:** Conventions + Task 3
- **问题:** spec §11 兼容性表写 "W8 既有调用点需更新(8 处)",但 Plan 6 实测 grep `\.run\(&[a-z_]` 全 workspace 返回 44+ 处
- **修复:** 在 §Conventions 加 "spec §11 数字偏离" 条目,记录偏离事实,此偏离记录到 PROGRESS.md "已知偏离" 段落,不回改 spec(遵循 "不修改 spec" 原则)

---

### 修复统计

| 优先级 | 缺陷数 | 修复状态 |
|---|---|---|
| P0 | 4 | ✅ 全部修复 |
| P1 | 9 | ✅ 全部修复 |
| P2 | 3 | ✅ 全部修复 |
| 跨 Plan | 1 | ✅ 全部修复 |
| **总计** | **17** | **✅ 全部修复** |

### 修复影响范围

- **Conventions 段落:** 3 处修改(VarScope::Named → Step / spec §11 数字偏离 / `--ignored real_gui` → `--ignored`)
- **Task 2 Step 1:** 1 处修改(resolve_iterable "修改前" 签名 3 参数 → 2 参数)
- **Task 2 Step 2:** 3 处修改(`slot.value` → `slot.raw` × 2 / 错误类型 → `TemplateError::UserSlotNotFound` / 加 ExtractedSlot 字段注释)
- **Task 4 Step 2/4/6:** 4 处修改(三处 ExtractedSlot 字段 + Step 2 加 IterableSource struct variant 注释 + task.explain LLM 依赖注释)
- **Task 5 Step 5:** 1 处修改(删除 `format!` 拼 `input_json` 死代码)
- **Task 6 Step 0(新增):** 1 处新增(预验证 Win11 notepad 控件类型)
- **Task 6 Step 1:** 2 处修改(设计决策块 + Step 1 代码块的 `pub fn` / `fn` → `pub(crate) fn`)
- **Task 6 Step 1.5(新增):** 1 处新增(核实 execute_note_capture 实际签名)
- **Task 6 Step 3:** 1 处修改(条件性 → 必须修改)
- **Task 6 Step 4:** 1 处修改(调整可见性 → 验证可见性)
- **Task 6 Step 6:** 4 处修改(`#[tokio::test]` → `flavor = "current_thread"` / `save_path` 改 PathBuf / SQL 加 skill_id 过滤 / taint 加 value_hash 断言)
- **Task 7 Step 1:** 2 处修改(`#[tokio::test]` → `flavor = "current_thread"` / `organize_dest` 改相对路径)
- **Task 7 Step 4:** 1 处修改(`--ignored real_gui` → `--ignored`)
- **Task 8 Step 7:** 1 处修改(加双重保险注释)

### 待执行者注意

1. **ExtractedSlot.kind 类型**:本修复将测试中的 `kind: "files".to_string()` 改为 `kind: SlotKind::Files`。执行者需核实 `SlotKind` 枚举是否有 `Files` 变体;若无,需用实际变体名(如 `SlotKind::Path` 或 `SlotKind::Text`)。同时 `Task 2 Step 2` 的 `s.kind == *slot_kind` 比较可能需调整(若 `kind` 是 `SlotKind` 而 `slot_kind` 是 `String`,需实现 `PartialEq<String>` 或调整比较逻辑)。
2. **set_thread_local_uia_adapter 可见性**:本修复按 spec 要求改为 `pub(crate)`。若集成测试编译失败(无法从 crate 外部调用),需在 `lib.rs` 或 `skills/mod.rs` 加 `pub use` 重导出,或改回 `pub fn`。
3. **TemplateError import**:`TemplateError::UserSlotNotFound` 需在 `dag_executor.rs` 顶部 import,执行者需确认 import 路径(可能在 `crate::skills::template` 模块)。
4. **compute_value_hash 函数**:`compute_value_hash` 需在测试文件 import 或定义,执行者需确认 W9 Plan 3 是否已暴露此 helper(可能在 `taint_repo` 模块)。
5. **probe_notepad_control_type 测试**:`Step 0` 的探测测试需手动实现,本 plan 仅给出骨架,执行者需用 `uiautomation-rs` API 完成控件类型查询。
