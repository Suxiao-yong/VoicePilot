# W8 Plan 2: LlmClient::decompose_to_dag + DagExecutor 简单节点 + dispatch_skill_executor 路由 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Plan 1 已落地的 `SlotTemplateEngine` + `DagPlan/DagNode/DagStatus` 数据结构 + `DagRepo` 基础设施之上,实现 W8 spec §2.2(LLM 拆解为 DAG)+ §2.3(DagExecutor 简单节点调度 + 9 路 skill 分发器)+ §6.1(DAG 6 个审计事件),让一条"打开记事本写 TODO 然后保存到桌面"语音转写能被 LLM 拆解为 `[note.capture, files.move]` 两节点 DAG 并按拓扑序串行执行,每步走 W7 既有 prepare→approve→commit,失败时前面已 commit 节点不回滚(决策 #4),DAG 终态为 `Failed` 或 `PartiallySucceeded`。

**Architecture:** `trust-kernel` 加 `skills/dispatcher.rs`(`dispatch_skill_executor` 9 路 match + `DispatchOutcome` 适配器,统一 8 个 W7 executor 的异构返回类型为单一 outcome 结构);加 `skills/dag_executor.rs`(`DagExecutor::new(kernel, approver, dag_repo)` 构造器 + `run(&DagPlan) -> Result<DagResult>` 主入口,Kahn 拓扑排序 + 简单节点串行执行 + 失败短路 + `PartiallySucceeded` 分支);扩 `approval/approver.rs`(`Approver` trait 加 `approve_dag_skeleton(&DagPlan) -> Result<ApprovalDecision>` 方法,AutoApprover / AutoDenier / TauriApprover 各自实现);扩 `llm/client.rs`(`decompose_to_dag(user_text, candidate_skills, user_slots) -> Result<DagPlan>` 方法,复用 W7 OpenAI 兼容 `/chat/completions` + function calling,`#[cfg(feature = "llm")]` 门控,wiremock 6 场景测试)。

**Tech Stack:** Rust(stable),`reqwest`(已有,`llm` feature),`wiremock`(已有,dev-dependency),`serde_json`(已有),`rusqlite`(已有),`uuid`(已有),TDD,PowerShell。

**Spec:** `docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md` §2.2(LlmClient::decompose_to_dag)+ §2.3(DagExecutor + dispatch_skill_executor + Approver 扩展)+ §6.1(DAG 6 个审计事件)

**Precondition:** W8 Plan 1 已完成 — `cargo check -p trust-kernel` PASS,`SlotTemplateEngine`(模板解析/渲染/校验)+ `DagPlan/DagNode/DagEdge/LoopSpec/DagStatus/DagNodeStatus/DagResult`(纯数据结构)+ `DagRepo`(`dag_plans` / `dag_nodes` CRUD)+ `TaskExplanationRepo`(`task_explanations` CRUD)+ DB 迁移 `003_dag_plans.sql` 全部就绪。W7 既有 8 个 Skill executor(`FilesOrganizeSkill` / `execute_task_repeat` / `execute_explain` / `execute_task_compensate` / `execute_app_control` / `execute_note_capture` / `execute_research_save` / `execute_form_prepare`)+ `Approver` trait(`prompt(&EffectManifest) -> ApprovalDecision`)+ `AutoApprover` / `AutoDenier` / `TauriApprover` 三实现可被本 Plan 直接复用。

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Modify** `src/approval/approver.rs` — `Approver` trait 加 `approve_dag_skeleton(&DagPlan) -> Result<ApprovalDecision>` 方法 + `AutoApprover` / `AutoDenier` / `TauriApprover` 各自实现
- **Create** `src/skills/dispatcher.rs` — `dispatch_skill_executor` 9 路 match + `DispatchOutcome` 适配器 + 9 个 input 反序列化 helper
- **Create** `src/skills/dag_executor.rs` — `DagExecutor` struct + `new()` + `run(&DagPlan) -> Result<DagResult>` 主入口 + `topological_sort` Kahn 算法 + `run_simple_node` + 失败短路 + `PartiallySucceeded` 分支
- **Modify** `src/skills/mod.rs` — `pub mod dispatcher; pub mod dag_executor;`
- **Modify** `src/llm/client.rs` — `impl LlmClient` 块追加 `decompose_to_dag` 方法(`#[cfg(feature = "llm")]` 门控)+ `build_decompose_system_prompt` / `build_decompose_tool_schema` / `parse_decompose_response` 私有 helper
- **Modify** `src/llm/types.rs` — 加 `LlmDecomposeError` 别名(可选,若与 `LlmError` 一致则跳过)

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan2_approver_dag_skeleton.rs` — `approve_dag_skeleton` 单元测试(AutoApprover / AutoDenier / TauriApprover stub,3 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan2_dispatcher.rs` — `dispatch_skill_executor` 9 路命中 + 1 路未知(10 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs` — `topological_sort` + `run_simple_node` + 失败短路 + `PartiallySucceeded` + 隐式环检测 + Deny 短路(≥ 12 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan2_audit_events.rs` — 6 个 `event_type` 至少 1 个断言(6 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan2_llm_decompose.rs` — wiremock 6 场景(成功 / HTTP 失败 / 非法 skill_id / 模板错误 / 超时 / max_total_steps 超限,6 个 `#[cfg(feature = "llm")] #[tokio::test]`)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_e2e.rs` — DAG 端到端(mock LLM + AutoApprover + 真实 dispatcher,4 个集成测试)

### Docs

- **Modify** `docs/PROGRESS.md` — W8 Plan 2 完成状态 + 测试统计

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 project_memory.md "Lessons Learned")
- **TDD**:每个含逻辑的任务先写失败测试 → `cargo test` 跑红 → 实现最小代码 → `cargo test` 跑绿 → `git commit`
- **Repo accessor pattern**:`DagRepo::new()` / `TaskExplanationRepo::new()` 不带参数,方法接收 `&Connection`(参考 W4 `McpServerRepo`,Plan 1 已落地)
- **`Approver` import**:用完整路径 `use crate::approval::approver::Approver`(approval 模块未在 root re-export,参考 project_memory.md "Engineering Conventions")
- **`&Connection` 接收**:`&kernel.conn()` 而非 `&*kernel.conn()`(后者触发 clippy `explicit_auto_deref` lint,参考 project_memory.md)
- **`TrustKernel` 不是 `Clone`**(持有 `Mutex<Connection>`);`DagExecutor` 持有 `Arc<TrustKernel>` + `Arc<dyn Approver>`(参考 project_memory.md "Engineering Conventions")
- **`query_map` closure** 返回 `rusqlite::Result<T>` 不是 `crate::error::Result<T>`(参考 project_memory.md "Lessons Learned")
- **Feature gate**:`decompose_to_dag` 相关代码 `#[cfg(feature = "llm")]` 门控;测试用 `#[cfg(feature = "llm")] #[tokio::test]`;默认 `cargo build` 不包含 LLM 代码
- **Commit message**:`feat(w8p2): ...` / `test(w8p2): ...` / `docs(w8p2): ...` / `refactor(w8p2): ...`
- **不引入新依赖**:本 plan 仅用 `reqwest` + `wiremock` + `serde_json` + `rusqlite` + `uuid`(均已在 workspace),不加新 crate
- **不修改 W7 既有 8 个 executor**:`dispatch_skill_executor` 通过 `DispatchOutcome` 适配器统一异构返回类型,不重构既有 executor(避免 scope creep)
- **不实现循环节点**(`run_loop_node`)+ `form.submit` executor:Plan 3 范围,本 plan 仅在 `dispatcher.rs` 中 `form.submit` 返回 `Err("not implemented in Plan 2")` 占位
- **不实现 `task.explain` LLM 增强**(`execute_task_explain_with_llm`):Plan 3 范围,本 plan `dispatcher.rs` 中 `task.explain` 路由到 W7 既有 `execute_explain`
- **Windows-only**:本 plan 不引入 `cfg(target_os = ...)` 条件编译(参考 project_memory.md "Hard Constraints")

---

## Task 1: Approver trait 扩展(approve_dag_skeleton + 三实现)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/approval/approver.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_approver_dag_skeleton.rs`

**Spec §:** §2.3 "全局审批 — DAG 骨架预览 + Allow/Deny(决策 #2)"

**背景:** 现有 `Approver` trait 只有 `prompt(&EffectManifest) -> ApprovalDecision` 方法(W3b 单步审批)。W8 DAG 引入"骨架审批"层 — 在节点执行前,把整个 DAG 计划(节点列表 + 边 + 风险等级)展示给用户做一次性 Allow/Deny 决策(决策 #2)。两方法共存:骨架审批先发生,通过后才进入逐节点 prepare→approve→commit。

**依赖方向确认:** `approval::approver` 新增 `use crate::skills::dag_types::DagPlan` 不构成循环依赖 — `skills::dag_types` 仅 import `policy::types::ELevel` / `skills::manifest::SkillManifest` / `skills::template::SlotTemplate`,均不反向引用 `approval`。

- [ ] **Step 1: 写失败测试 — 创建 `tests/w8_plan2_approver_dag_skeleton.rs`**

```rust
//! W8 Plan 2 Task 1: Approver::approve_dag_skeleton 单元测试.
//!
//! Spec §2.3 决策 #2:DAG 骨架审批层 — Allow 才进入节点执行,
//! Deny 则 DagStatus=Cancelled,0 节点执行。
//! 本测试覆盖 AutoApprover / AutoDenier / TauriApprover stub 三实现。

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

fn dummy_plan(plan_id: &str) -> DagPlan {
    let n1 = DagNode {
        node_id: "n1".into(),
        skill_id: "note.capture".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("notepad".into()),
        },
        risk_ceiling: ELevel::E1,
    };
    let n2 = DagNode {
        node_id: "n2".into(),
        skill_id: "files.move".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Path,
            template: TemplateExpr::Var(trust_kernel::skills::template::VarRef {
                scope: trust_kernel::skills::template::VarScope::Prev,
                path: "output.path".into(),
            }),
        },
        risk_ceiling: ELevel::E2,
    };
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "打开记事本写 TODO 然后保存到桌面".into(),
        nodes: vec![n1, n2],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: Some("output.path -> input.source".into()),
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn auto_approver_approve_dag_skeleton_returns_allow() {
    let approver = AutoApprover;
    let plan = dummy_plan("plan-auto-allow");
    let decision = approver.approve_dag_skeleton(&plan).expect("AutoApprover should not error");
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn auto_denier_approve_dag_skeleton_returns_deny() {
    let approver = AutoDenier;
    let plan = dummy_plan("plan-auto-deny");
    let decision = approver.approve_dag_skeleton(&plan).expect("AutoDenier should not error");
    assert_eq!(decision, ApprovalDecision::Deny);
}

#[test]
fn tauri_approver_stub_approve_dag_skeleton_returns_deny_by_default() {
    // W6 TauriApprover 在没有 app handle 时返回 Deny(timeout 默认行为)。
    // Plan 5 会实现真实的 IPC channel + 5min timeout;此处仅验证 stub 不会 panic。
    use trust_kernel::approval::tauri_approver::TauriApprover;
    let approver = TauriApprover::new();
    let plan = dummy_plan("plan-tauri-stub");
    let decision = approver.approve_dag_skeleton(&plan).expect("TauriApprover stub should not error");
    assert_eq!(decision, ApprovalDecision::Deny);
}

#[test]
fn approver_trait_object_can_call_approve_dag_skeleton() {
    // 验证 trait object 也能调用新方法(动态分发)。
    // DagExecutor 在生产代码中持有 `Arc<dyn Approver>`,需要 trait object 兼容。
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let plan = dummy_plan("plan-trait-object");
    let decision = approver.approve_dag_skeleton(&plan).expect("trait object should not error");
    assert_eq!(decision, ApprovalDecision::Allow);
}
```

- [ ] **Step 2: 跑测试,确认失败(方法未定义)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_approver_dag_skeleton`
Expected: FAIL — 编译错误 `no method named approve_dag_skeleton found for struct AutoApprover in the current scope`

- [ ] **Step 3: 修改 `approval/approver.rs`,加 `approve_dag_skeleton` 方法到 trait + 三实现**

打开 `voicepilot/crates/trust-kernel/src/approval/approver.rs`,完整替换为:

```rust
//! Approver trait — V1.1 §6.2 approve phase.
//!
//! UI-agnostic: CLI implements it with stdin; Tauri implements it with
//! an IPC call to the approval window. The Skill executor calls
//! `approver.prompt(manifest)` between prepare and commit.
//!
//! W8 Plan 2: 扩展 `approve_dag_skeleton(&DagPlan) -> Result<ApprovalDecision>`
//! 方法,用于 DAG 骨架一次性审批(决策 #2)。两方法共存:
//!   1. `approve_dag_skeleton` — DAG 节点执行前,展示骨架 + Allow/Deny
//!   2. `prompt` — 每个节点 prepare→commit 之间的细粒度审批
//! Allow(DAG 骨架) + Allow(每步 prepare) 才进入 commit;任一 Deny 短路。

use crate::approval::types::ApprovalDecision;
use crate::error::Result;
use crate::policy::transaction::EffectManifest;
use crate::skills::dag_types::DagPlan;

/// Callback the Skill executor invokes between prepare and commit.
///
/// W8 Plan 2: `approve_dag_skeleton` 是 DAG 骨架层审批;`prompt` 是
/// 节点级 prepare→commit 审批。两层审批独立,各自 Allow/Deny。
pub trait Approver: Send + Sync {
    /// Show the effect_manifest to the user and return their decision.
    /// May block (CLI stdin) or return immediately (auto-approve / auto-deny).
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision;

    /// W8 Plan 2: DAG 骨架审批(决策 #2)。
    ///
    /// 在 DagExecutor::run 调用 topological_sort 之后、节点执行之前触发。
    /// 实现:
    ///   - AutoApprover → Ok(Allow)(测试 / headless)
    ///   - AutoDenier → Ok(Deny)(负路径测试)
    ///   - TauriApprover → 真实 IPC 弹窗(W6 既有 channel,5min timeout → Deny)
    ///
    /// 返回 `Result<ApprovalDecision>` 而非 `ApprovalDecision` 是为了让
    /// TauriApprover 能区分"通道失败"与"用户 Deny"(前者可作为 Err 向上
    /// 传播,后者是用户意图)。AutoApprover / AutoDenier 始终返回 Ok。
    fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<ApprovalDecision>;
}

/// Auto-approver for tests and headless runs. Always returns Allow.
pub struct AutoApprover;

impl Approver for AutoApprover {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Allow
    }

    fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<ApprovalDecision> {
        Ok(ApprovalDecision::Allow)
    }
}

/// Auto-denier for negative-path tests.
pub struct AutoDenier;

impl Approver for AutoDenier {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Deny
    }

    fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<ApprovalDecision> {
        Ok(ApprovalDecision::Deny)
    }
}
```

- [ ] **Step 4: 修改 `approval/tauri_approver.rs`,为 `TauriApprover` 加 `approve_dag_skeleton` stub 实现**

打开 `voicepilot/crates/trust-kernel/src/approval/tauri_approver.rs`(W6 既有),在 `impl Approver for TauriApprover` 块末尾(`prompt` 方法后)追加:

```rust
    /// W8 Plan 2 stub:DAG 骨架审批。
    ///
    /// W6 既有 `prompt` 用 oneshot channel + 5min timeout,默认 Deny。
    /// Plan 5 会实现真实 DAG 骨架弹窗(显示节点列表 + 边连线图)。
    /// 此 stub 返回 Ok(Deny) 以让 DagExecutor 的 Deny 短路逻辑可被测试。
    fn approve_dag_skeleton(&self, _plan: &crate::skills::dag_types::DagPlan) -> crate::error::Result<crate::approval::types::ApprovalDecision> {
        // Plan 5 TODO: 通过 app_handle 触发 DagApprovalDialog.vue,
        // 5min timeout → Deny;oneshot channel 接收 Allow/Deny。
        Ok(crate::approval::types::ApprovalDecision::Deny)
    }
```

若 `tauri_approver.rs` 文件不存在(某些 feature 组合下可能被 cfg gate),用 `#[cfg(feature = "tauri")]` 包裹整段 impl 块。检查文件顶部是否已有 `#[cfg(feature = "tauri")]`,若有则保持一致。

- [ ] **Step 5: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features tauri`
Expected: PASS(若有"unused import"警告,后续 Task 引用后消除)

- [ ] **Step 6: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_approver_dag_skeleton`
Expected: PASS(4 个测试全绿:`auto_approver_approve_dag_skeleton_returns_allow` / `auto_denier_approve_dag_skeleton_returns_deny` / `tauri_approver_stub_approve_dag_skeleton_returns_deny_by_default` / `approver_trait_object_can_call_approve_dag_skeleton`)

- [ ] **Step 7: 跑 clippy,确认无新警告**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features tauri -- -D warnings`
Expected: PASS(0 warnings)

- [ ] **Step 8: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/approval/approver.rs voicepilot/crates/trust-kernel/src/approval/tauri_approver.rs voicepilot/crates/trust-kernel/tests/w8_plan2_approver_dag_skeleton.rs
git commit -m "feat(w8p2): extend Approver trait with approve_dag_skeleton method + 3 implementations"
```

---

## Task 2: dispatch_skill_executor 路由 + DispatchOutcome 适配器

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_dispatcher.rs`

**Spec §:** §2.3 "dispatch_skill_executor 路由"

**背景:** Spec §2.3 给出的 dispatcher 签名是 `(skill_id, kernel, input: &SkillInput, approver) -> Result<SkillExecution>`,但 W7 既有 8 个 executor 的返回类型异构:
- `FilesOrganizeSkill::execute` → `Result<SkillExecution>`(`SkillExecution` 有 `tool_result: ToolResult` + `moved_paths: Vec<(PathBuf, PathBuf)>`)
- 其余 7 个 executor(`execute_explain` / `execute_task_repeat` / `execute_task_compensate` / `execute_app_control` / `execute_note_capture` / `execute_research_save` / `execute_form_prepare`)→ `Result<String>`(仅返回 task_id)

直接复用 `SkillExecution` 会强加 `moved_paths` 字段到非 files.organize 的 executor(语义错误)。本 Plan 引入 `DispatchOutcome` 适配器统一两类型 — `tool_result.data` 作为节点 `output`(供下游 `${prev.output.xxx}` 模板解析),`succeeded` 字段让 `DagExecutor` 判定 `DagNodeStatus::Succeeded` / `Failed`。这是对 spec §2.3 的有意偏离(见 Self-Review §2 已知偏离),避免重构 7 个既有 executor。

**Input 反序列化:** DagExecutor 调用 `SlotTemplateEngine::resolve` 把 `DagNode.input_template` 渲染为 `serde_json::Value`。dispatcher 接收该 JSON value,按 `skill_id` 反序列化为对应 executor 的 input struct(如 `FilesOrganizeInput` / `TaskExplainInput`),然后调 executor。每个 input struct 的 `task_id` / `step_id` 字段由 DagExecutor 生成(uuid),其余字段从 JSON 提取。

- [ ] **Step 1: 创建 `skills/dispatcher.rs`,定义 `DispatchOutcome` + `dispatch_skill_executor` 函数骨架**

```rust
//! dispatch_skill_executor — W8 §2.3.
//!
//! 9 路 skill_id 分发器:把 DagExecutor 解析后的 JSON input 路由到对应
//! W7 既有 executor,统一返回 `DispatchOutcome` 适配器。
//!
//! 设计权衡(见 plan Self-Review §2 已知偏离):
//! - spec §2.3 期望返回 `SkillExecution`,但 W7 既有 8 个 executor 返回类型
//!   异构(`SkillExecution` vs `String` task_id),强行统一需重构 7 个 executor
//! - 本 plan 引入 `DispatchOutcome` 适配器,各分支按 executor 原生返回类型
//!   适配,`output` 字段为 `serde_json::Value`(供下游模板 `${prev.output.xxx}` 解析)
//! - `form.submit` 在 Plan 3 实现,本 plan 占位返回 Err
//! - `task.explain` 路由到 W7 既有 `execute_explain`,Plan 3 改为 `execute_task_explain_with_llm`

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill, SkillExecution};
use crate::skills::task_explain::{execute_explain, TaskExplainInput};
use crate::skills::task_repeat::{execute_task_repeat, TaskRepeatInput};
use crate::skills::task_compensate::{execute_task_compensate, TaskCompensateInput};
use crate::toolresult::ToolStatus;

#[cfg(all(windows, feature = "uia"))]
use crate::skills::app_control::{execute_app_control, AppControlInput};
#[cfg(all(windows, feature = "uia"))]
use crate::skills::note_capture::{execute_note_capture, NoteCaptureInput};

use crate::skills::research_save::{execute_research_save, ResearchSaveInput};
use crate::skills::form_prepare::{execute_form_prepare, FormPrepareInput};

/// dispatch_skill_executor 的统一返回值。
///
/// `output` 是节点的 output(供下游 `${prev.output.xxx}` 模板解析):
/// - files.organize → `tool_result.data`(含 `moved_count` / `destination` / `approval_id`)
/// - 其他 executor → `{"task_id": "...", "step_id": "..."}`(executor 仅返回 task_id)
///
/// `succeeded` 字段让 DagExecutor 判定 `DagNodeStatus::Succeeded` / `Failed`。
/// 注意:executor 返回 `Err` 时本函数直接传播 `Err`(不构造 `DispatchOutcome`),
/// DagExecutor 在 `run_simple_node` 中 catch 并标记节点 Failed。
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub task_id: String,
    pub step_id: String,
    /// 节点 output,作为下游模板 resolve 的 node_outputs[node_id]。
    pub output: serde_json::Value,
    /// 是否成功(executor 返回 Ok 但 ToolStatus 可能是 Cancelled)。
    pub succeeded: bool,
    /// 失败原因(若 succeeded=false)。
    pub error_cause: Option<String>,
}

impl DispatchOutcome {
    /// files.organize 分支:从 SkillExecution 提取 outcome。
    pub fn from_skill_execution(exec: SkillExecution, task_id: String, step_id: String) -> Self {
        let succeeded = matches!(exec.tool_result.status, ToolStatus::Succeeded);
        let error_cause = if succeeded {
            None
        } else {
            Some(format!("tool_status: {:?}", exec.tool_result.status))
        };
        Self {
            task_id,
            step_id,
            output: exec.tool_result.data,
            succeeded,
            error_cause,
        }
    }

    /// 其余 8 个 executor 分支:仅返回 task_id,构造最小 outcome。
    pub fn from_task_id(returned_task_id: String, step_id: String) -> Self {
        let output = serde_json::json!({
            "task_id": returned_task_id.clone(),
            "step_id": step_id.clone(),
        });
        Self {
            task_id: returned_task_id,
            step_id,
            output,
            succeeded: true,
            error_cause: None,
        }
    }
}

/// dispatch_skill_executor — 9 路 skill_id 分发。
///
/// 参数:
/// - `skill_id`:DagNode.skill_id(必须命中 9 路之一,否则 Err)
/// - `kernel`:TrustKernel 引用(传给 executor)
/// - `resolved_input`:`SlotTemplateEngine::resolve` 渲染后的 JSON value
/// - `approver`:节点级审批器(传给 executor,与 DAG 骨架审批独立)
/// - `task_id`:DagExecutor 生成的新 task_id(每个节点独立 task)
/// - `step_id`:DagExecutor 生成的新 step_id
///
/// 返回 `Result<DispatchOutcome>`:
/// - Ok(outcome) — executor 成功执行(ToolStatus 可能是 Succeeded / Cancelled)
/// - Err(KernelError::Skill(...)) — executor 失败 / 未知 skill_id / form.submit 占位
///
/// DagExecutor 在 `run_simple_node` 中:
///   1. 调本函数
///   2. Ok + outcome.succeeded=true → DagNodeStatus::Succeeded(outcome.output)
///   3. Ok + outcome.succeeded=false → DagNodeStatus::Failed{cause: outcome.error_cause}
///   4. Err(e) → DagNodeStatus::Failed{cause: e.to_string()}
pub fn dispatch_skill_executor(
    skill_id: &str,
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    match skill_id {
        "files.organize" => dispatch_files_organize(kernel, resolved_input, approver, task_id, step_id),
        "task.repeat_verified" => dispatch_task_repeat(kernel, resolved_input, approver, task_id, step_id),
        "task.explain" => dispatch_task_explain(kernel, resolved_input, approver, task_id, step_id),
        "task.compensate" => dispatch_task_compensate(kernel, resolved_input, approver, task_id, step_id),
        #[cfg(all(windows, feature = "uia"))]
        "quick.app_control" => dispatch_app_control(kernel, resolved_input, approver, task_id, step_id),
        #[cfg(all(windows, feature = "uia"))]
        "note.capture" => dispatch_note_capture(kernel, resolved_input, approver, task_id, step_id),
        "research.save_markdown" => dispatch_research_save(kernel, resolved_input, approver, task_id, step_id),
        "form.prepare" => dispatch_form_prepare(kernel, resolved_input, approver, task_id, step_id),
        "form.submit" => Err(KernelError::Skill(
            "form.submit not implemented in Plan 2; see Plan 3".into(),
        )),
        _ => Err(KernelError::Skill(format!("unknown skill_id: {}", skill_id))),
    }
}

fn dispatch_files_organize(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = FilesOrganizeInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        source: extract_path(resolved_input, "source")?,
        filter: extract_string(resolved_input, "filter")?,
        destination: extract_path(resolved_input, "destination")?,
    };
    let exec = FilesOrganizeSkill::new().execute(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_skill_execution(exec, task_id.into(), step_id.into()))
}

fn dispatch_task_repeat(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = TaskRepeatInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        target_task_id: extract_string(resolved_input, "target_task_id")?,
        source_filter: extract_string(resolved_input, "source_filter")?,
    };
    let returned = execute_task_repeat(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_task_explain(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let limit = extract_u32(resolved_input, "limit")?.unwrap_or(10);
    let input = TaskExplainInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        limit,
    };
    let returned = execute_explain(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_task_compensate(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = TaskCompensateInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        target_step_id: extract_string(resolved_input, "target_step_id")?,
    };
    let returned = execute_task_compensate(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

#[cfg(all(windows, feature = "uia"))]
fn dispatch_app_control(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let action = extract_string(resolved_input, "action").ok().unwrap_or_else(|| "launch".into());
    let input = AppControlInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        app_name: extract_string(resolved_input, "app_name")?,
        action,
    };
    let returned = execute_app_control(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

#[cfg(all(windows, feature = "uia"))]
fn dispatch_note_capture(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = NoteCaptureInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        content: extract_string(resolved_input, "content")?,
        save_path: extract_string(resolved_input, "save_path")?,
    };
    let returned = execute_note_capture(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_research_save(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = ResearchSaveInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        url: extract_string(resolved_input, "url")?,
        save_path: extract_string(resolved_input, "save_path")?,
    };
    let returned = execute_research_save(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_form_prepare(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = FormPrepareInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        url: extract_string(resolved_input, "url")?,
        fields: extract_string(resolved_input, "fields")?,
    };
    let returned = execute_form_prepare(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

// ===== JSON 提取 helper =====

fn extract_string(v: &serde_json::Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| KernelError::Skill(format!("dispatch: missing or invalid string field '{}'", key)))
}

fn extract_u32(v: &serde_json::Value, key: &str) -> Result<Option<u32>> {
    match v.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(None),
        Some(x) => x
            .as_u64()
            .map(|n| Some(n as u32))
            .ok_or_else(|| KernelError::Skill(format!("dispatch: field '{}' must be u32", key))),
    }
}

fn extract_path(v: &serde_json::Value, key: &str) -> Result<std::path::PathBuf> {
    let s = extract_string(v, key)?;
    Ok(std::path::PathBuf::from(s))
}
```

- [ ] **Step 2: 在 `skills/mod.rs` 加 `pub mod dispatcher;`**

打开 `voicepilot/crates/trust-kernel/src/skills/mod.rs`,在现有 `pub mod` 声明后追加(W8 Plan 2 Task 2):

```rust
// W8 Plan 2 Task 2: dispatch_skill_executor 路由 + DispatchOutcome 适配器
pub mod dispatcher;
// W8 Plan 2 Task 3: DagExecutor 简单节点调度
pub mod dag_executor;
```

注意:`dag_executor` 在 Task 3 创建,此处提前声明 mod 让 Step 3 的 `cargo check` 通过 — 若 Task 3 文件尚未创建,先注释掉 `pub mod dag_executor;` 一行,Task 3 Step 1 创建文件后再取消注释。

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features voice,tauri,llm,uia`
Expected: PASS(若 `task_repeat.rs` / `task_compensate.rs` / `research_save.rs` / `form_prepare.rs` 的 input struct 名称与本 plan 不一致,需根据实际文件调整 import 和字段名)

若编译失败,检查以下实际签名(用 `cargo doc --open` 或读源文件):
- `voicepilot/crates/trust-kernel/src/skills/task_repeat.rs` — `TaskRepeatInput` 字段名
- `voicepilot/crates/trust-kernel/src/skills/task_compensate.rs` — `TaskCompensateInput` 字段名
- `voicepilot/crates/trust-kernel/src/skills/research_save.rs` — `ResearchSaveInput` 字段名
- `voicepilot/crates/trust-kernel/src/skills/form_prepare.rs` — `FormPrepareInput` 字段名
- `voicepilot/crates/trust-kernel/src/skills/app_control.rs` — `AppControlInput` 字段名(仅 `windows + uia` feature)
- `voicepilot/crates/trust-kernel/src/skills/note_capture.rs` — `NoteCaptureInput` 字段名(仅 `windows + uia` feature)

按实际签名调整 `dispatch_*` 函数中的 `input` 构造代码。

- [ ] **Step 4: 写失败测试 — 创建 `tests/w8_plan2_dispatcher.rs`**

```rust
//! W8 Plan 2 Task 2: dispatch_skill_executor 路由测试.
//!
//! 覆盖 9 路 skill_id 命中 + 1 路未知 skill_id 报错。
//! 注:form.submit 在 Plan 3 实现,本 plan 测试期望 Err("not implemented")。

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;

fn kernel() -> TrustKernel {
    TrustKernel::open_in_memory().expect("open_in_memory")
}

#[test]
fn dispatch_unknown_skill_id_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "nonexistent.skill",
        &kernel,
        &input,
        &approver,
        "task-x",
        "step-x",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("unknown skill_id"), "got: {}", msg);
}

#[test]
fn dispatch_form_submit_returns_not_implemented_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"url": "https://example.com", "submit_selector": "button[type=submit]"});
    let result = dispatch_skill_executor(
        "form.submit",
        &kernel,
        &input,
        &approver,
        "task-submit",
        "step-submit",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("not implemented"), "got: {}", msg);
}

#[test]
fn dispatch_files_organize_routes_to_files_organize_skill() {
    // 验证 files.organize 分支命中:用非法 destination 触发 executor 错误,
    // 错误消息应来自 FilesOrganizeSkill(executor 层),不是 "unknown skill_id"。
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({
        "source": "Z:/nonexistent_source_dir",
        "filter": "*.pdf",
        "destination": "Z:/nonexistent_dest_dir"
    });
    let result = dispatch_skill_executor(
        "files.organize",
        &kernel,
        &input,
        &approver,
        "task-files",
        "step-files",
    );
    // executor 大概率返回 Err(无文件匹配 / 路径不允许)。
    // 关键断言:不是 "unknown skill_id" 错误,证明路由命中。
    if let Err(e) = result {
        let msg = format!("{}", e);
        assert!(!msg.contains("unknown skill_id"), "should not be unknown skill_id, got: {}", msg);
    }
}

#[test]
fn dispatch_task_explain_routes_to_execute_explain() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"limit": 5});
    let result = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &input,
        &approver,
        "task-explain",
        "step-explain",
    );
    let outcome = result.expect("task.explain with valid limit should succeed");
    assert!(outcome.succeeded);
    assert_eq!(outcome.step_id, "step-explain");
    assert!(outcome.output.get("task_id").is_some());
}

#[test]
fn dispatch_task_explain_uses_default_limit_when_missing() {
    let kernel = kernel();
    let approver = AutoApprover;
    // limit 缺失 → extract_u32 返回 None → unwrap_or(10)
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &input,
        &approver,
        "task-explain-default",
        "step-explain-default",
    );
    let outcome = result.expect("task.explain with default limit should succeed");
    assert!(outcome.succeeded);
}

#[test]
fn dispatch_task_explain_rejects_invalid_limit_type() {
    let kernel = kernel();
    let approver = AutoApprover;
    // limit 是字符串而非数字 → extract_u32 返回 Err
    let input = serde_json::json!({"limit": "not-a-number"});
    let result = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &input,
        &approver,
        "task-explain-bad",
        "step-explain-bad",
    );
    assert!(result.is_err());
}

#[test]
fn dispatch_task_repeat_missing_target_task_id_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"source_filter": "*.pdf"});
    let result = dispatch_skill_executor(
        "task.repeat_verified",
        &kernel,
        &input,
        &approver,
        "task-repeat",
        "step-repeat",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("target_task_id"), "got: {}", msg);
}

#[test]
fn dispatch_task_compensate_missing_target_step_id_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "task.compensate",
        &kernel,
        &input,
        &approver,
        "task-comp",
        "step-comp",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("target_step_id"), "got: {}", msg);
}

#[test]
fn dispatch_research_save_missing_url_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"save_path": "C:/test.md"});
    let result = dispatch_skill_executor(
        "research.save_markdown",
        &kernel,
        &input,
        &approver,
        "task-research",
        "step-research",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("url"), "got: {}", msg);
}

#[test]
fn dispatch_form_prepare_missing_fields_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"url": "https://example.com"});
    let result = dispatch_skill_executor(
        "form.prepare",
        &kernel,
        &input,
        &approver,
        "task-form",
        "step-form",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("fields"), "got: {}", msg);
}

#[cfg(all(windows, feature = "uia"))]
#[test]
fn dispatch_app_control_missing_app_name_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"action": "launch"});
    let result = dispatch_skill_executor(
        "quick.app_control",
        &kernel,
        &input,
        &approver,
        "task-app",
        "step-app",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("app_name"), "got: {}", msg);
}

#[cfg(all(windows, feature = "uia"))]
#[test]
fn dispatch_note_capture_missing_content_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"save_path": "C:/test.txt"});
    let result = dispatch_skill_executor(
        "note.capture",
        &kernel,
        &input,
        &approver,
        "task-note",
        "step-note",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("content"), "got: {}", msg);
}
```

- [ ] **Step 5: 跑测试,确认通过(或确认 input struct 字段名偏离后调整)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_dispatcher --features voice,tauri,llm,uia`
Expected: PASS(默认 feature 下 ≥ 7 个测试,`windows + uia` feature 下 +2 = 9 个测试;外加 `dispatch_form_submit_returns_not_implemented_err` = 10 个测试)

若失败,根据错误信息调整 `dispatcher.rs` 中对应 input struct 的字段名(例如 `TaskRepeatInput.target_task_id` 在实际代码中可能是 `target_task` 或其他名称)。

- [ ] **Step 6: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features voice,tauri,llm,uia -- -D warnings`
Expected: PASS(0 warnings;若有"unused import"提示,删除对应 use)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dispatcher.rs voicepilot/crates/trust-kernel/src/skills/mod.rs voicepilot/crates/trust-kernel/tests/w8_plan2_dispatcher.rs
git commit -m "feat(w8p2): add dispatch_skill_executor with 9-way routing + DispatchOutcome adapter"
```

---

## Task 3: DagExecutor 骨架(new + run 主入口 + topological_sort Kahn 算法)

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`(若 Task 2 Step 2 已声明则跳过)
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs`(本 Task 仅写 topo sort 测试,后续 Task 4-6 在同文件追加)

**Spec §:** §2.3 "DagExecutor::run" + "拓扑排序(Kahn 算法)"

**背景:** DagExecutor 是 W8 DAG 调度核心。`run(plan: &DagPlan) -> Result<DagResult>` 主入口流程:
1. `topological_sort(&plan.nodes, &plan.edges) -> Result<Vec<String>>`(Kahn 算法,检测隐式环)
2. `approver.approve_dag_skeleton(plan)?`(全局骨架审批,Deny → `DagResult::cancelled()`)
3. 按拓扑序 `run_simple_node`(Task 4 实现) / `run_loop_node`(Plan 3 实现,本 plan skip 标记 TODO)
4. 失败处理(Task 6 实现):节点 Failed 时,前面已 commit 不回滚,DAG 走 Failed / PartiallySucceeded

**Kahn 算法:** 计算 in-degree,从 in-degree=0 的节点出发,BFS 拓扑序。若排序后节点数 < 输入节点数 → 存在环(返回 Err)。这与 LoopSpec 显式循环节点不同 — LoopSpec 是节点内部的循环迭代,edges 中的环是节点依赖环。

**TrustKernel 不是 Clone:** `DagExecutor` 持有 `Arc<TrustKernel>`(参考 project_memory.md "Engineering Conventions")。Approver 同样用 `Arc<dyn Approver>` 持有(动态分发,AutoApprover / TauriApprover 都满足 `Send + Sync`)。

- [ ] **Step 1: 创建 `skills/dag_executor.rs`,定义 `DagExecutor` struct + `new()` + `topological_sort` + `run()` 主入口骨架**

```rust
//! DagExecutor — W8 §2.3.
//!
//! DAG 调度器:接收 LLM 拆解的 DagPlan,按拓扑序串行执行简单节点
//! (循环节点在 Plan 3 实现),失败时前面已 commit 不回滚(决策 #4),
//! DAG 终态为 Succeeded / Failed / PartiallySucceeded / Cancelled。
//!
//! 调度算法(spec §2.3):
//!   Step 1: topological_sort(Kahn 算法)→ Vec<node_id>(检测隐式环)
//!   Step 2: approver.approve_dag_skeleton(plan) → Allow/Deny
//!           Deny → DagResult::cancelled()(0 节点执行)
//!   Step 3: 按拓扑序 for each node:
//!             - loop_specs 含 → run_loop_node(Plan 3;本 plan 返回 Err)
//!             - 否则 → run_simple_node
//!           失败处理(决策 #4 + #8):节点 Failed 时,
//!             - 若有已成功节点 → DagStatus::PartiallySucceeded
//!             - 否则 → DagStatus::Failed
//!           不回滚前序已 commit 节点。
//!   Step 4: 全部成功 → DagStatus::Succeeded
//!
//! 状态持久化:每个节点 start/success/fail 时调 DagRepo.update_node_status,
//! 整个 plan 完成时调 DagRepo.update_plan_status。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::dag_repo::DagRepo;
use crate::skills::dag_types::{
    DagNode, DagNodeStatus, DagPlan, DagResult, DagStatus,
};
use crate::skills::dispatcher::{dispatch_skill_executor, DispatchOutcome};
use crate::skills::template::SlotTemplateEngine;

/// DAG 调度器。
///
/// 持有 `Arc<TrustKernel>`(TrustKernel 不是 Clone,见 project_memory.md)
/// 和 `Arc<dyn Approver>`(动态分发,支持 AutoApprover / TauriApprover)。
/// `DagRepo` 是无状态 accessor,每次 new() 一个新实例。
pub struct DagExecutor {
    kernel: Arc<TrustKernel>,
    approver: Arc<dyn Approver>,
    dag_repo: Arc<DagRepo>,
}

impl DagExecutor {
    /// 构造器。
    ///
    /// 参数:
    /// - `kernel`:Arc<TrustKernel>(调用方持 Arc,DagExecutor clone 一份)
    /// - `approver`:Arc<dyn Approver>(骨架审批 + 节点级审批都用同一个)
    /// - `dag_repo`:Arc<DagRepo>(无状态,可共享)
    pub fn new(kernel: Arc<TrustKernel>, approver: Arc<dyn Approver>, dag_repo: Arc<DagRepo>) -> Self {
        Self { kernel, approver, dag_repo }
    }

    /// 主入口 — 调度一个 DagPlan。
    ///
    /// 算法见模块注释。返回 `Result<DagResult>`:
    /// - Ok(DagResult) — DAG 执行完成(无论 Succeeded / Failed / PartiallySucceeded / Cancelled)
    /// - Err(KernelError) — 不可恢复错误(拓扑环 / DB 故障 / 模板校验失败)
    ///
    /// 注意:节点级 executor 失败不返回 Err,而是构造 `DagResult` with
    /// `DagStatus::Failed` / `PartiallySucceeded`,让调用方从 result 判断状态。
    pub fn run(&self, plan: &DagPlan) -> Result<DagResult> {
        // Step 1: 拓扑排序(Kahn 算法)— 检测 edges 中的隐式环
        let order = topological_sort(&plan.nodes, &plan.edges)?;

        // Step 2: 全局审批 — DAG 骨架 Allow/Deny(决策 #2)
        let decision = self.approver.approve_dag_skeleton(plan)?;
        if matches!(decision, crate::approval::types::ApprovalDecision::Deny) {
            // Deny → 0 节点执行 + DagStatus=Cancelled
            self.persist_dag_status(plan, &DagStatus::Cancelled)?;
            return Ok(DagResult::cancelled());
        }

        // Step 3: 按拓扑序执行简单节点(循环节点 Plan 3 实现,本 plan skip)
        let mut node_outputs: HashMap<String, serde_json::Value> = HashMap::new();
        let mut node_results: HashMap<String, DagNodeStatus> = HashMap::new();
        let mut prev_node_id: Option<String> = None;

        for node_id in &order {
            let node = plan.nodes.iter().find(|n| &n.node_id == node_id)
                .ok_or_else(|| KernelError::Skill(format!("topo sort returned unknown node_id: {}", node_id)))?;

            // 循环节点检测 — Plan 3 实现,本 plan 返回 Err
            if plan.loop_specs.contains_key(node_id) {
                return Err(KernelError::Skill(format!(
                    "loop node '{}' not supported in Plan 2; see Plan 3",
                    node_id
                )));
            }

            // 简单节点执行
            let status = self.run_simple_node(
                node,
                plan,
                &node_outputs,
                prev_node_id.as_deref(),
            )?;

            let is_failed = status.is_failed();
            node_results.insert(node_id.clone(), status.clone());
            if let DagNodeStatus::Succeeded(out) = &status {
                node_outputs.insert(node_id.clone(), out.clone());
                prev_node_id = Some(node_id.clone());
            }

            // 失败处理(决策 #4 + #8):前序已 commit 不回滚,
            // DAG 走 Failed(无成功节点)或 PartiallySucceeded(有成功节点)
            if is_failed {
                let succeeded: Vec<String> = order.iter()
                    .filter(|nid| node_results.get(*nid).map(|s| s.is_succeeded()).unwrap_or(false))
                    .cloned()
                    .collect();
                let cause = match &status {
                    DagNodeStatus::Failed { cause } => cause.clone(),
                    _ => String::new(),
                };
                let final_status = if succeeded.is_empty() {
                    DagStatus::Failed { failed_node: node_id.clone(), cause }
                } else {
                    DagStatus::PartiallySucceeded { succeeded, failed_node: node_id.clone(), cause }
                };
                self.persist_dag_status(plan, &final_status)?;
                return Ok(DagResult {
                    status: final_status,
                    node_results,
                });
            }
        }

        // Step 4: 全部成功
        let final_status = DagStatus::Succeeded;
        self.persist_dag_status(plan, &final_status)?;
        Ok(DagResult::succeeded(node_results))
    }

    /// 把 DAG 终态持久化到 dag_plans 表。
    /// 失败时返回 Err(但 DAG 执行结果已确定,此处仅记录失败)。
    fn persist_dag_status(&self, plan: &DagPlan, status: &DagStatus) -> Result<()> {
        let conn = self.kernel.conn();
        let completed = !matches!(status, DagStatus::Pending | DagStatus::Running);
        self.dag_repo.update_plan_status(&conn, &plan.plan_id, status, completed)
    }
}

/// 拓扑排序 — Kahn 算法。
///
/// 算法:
/// 1. 计算每个节点的 in-degree(入度 = 多少条 edge 指向它)
/// 2. 把 in-degree=0 的节点入队
/// 3. BFS:出队一个节点 → 加入 result → 把它的所有后继节点 in-degree 减 1
///    → 若 in-degree 变为 0 则入队
/// 4. 若 result.len() < nodes.len() → 存在环(返回 Err)
///
/// 注:LoopSpec 显式循环节点(节点内部迭代)与 edges 隐式环(节点依赖环)
/// 是不同概念。本函数仅检测后者;前者由 Plan 3 的 run_loop_node 处理。
pub fn topological_sort(nodes: &[DagNode], edges: &[crate::skills::dag_types::DagEdge]) -> Result<Vec<String>> {
    let mut in_degree: HashMap<String, u32> = HashMap::new();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();

    for node in nodes {
        in_degree.insert(node.node_id.clone(), 0);
        adjacency.insert(node.node_id.clone(), Vec::new());
    }

    for edge in edges {
        // 校验 edge.from / edge.to 在 nodes 中
        if !in_degree.contains_key(&edge.from) {
            return Err(KernelError::Skill(format!(
                "topological_sort: edge.from '{}' not in nodes", edge.from
            )));
        }
        if !in_degree.contains_key(&edge.to) {
            return Err(KernelError::Skill(format!(
                "topological_sort: edge.to '{}' not in nodes", edge.to
            )));
        }
        *in_degree.get_mut(&edge.to).unwrap() += 1;
        adjacency.get_mut(&edge.from).unwrap().push(edge.to.clone());
    }

    let mut queue: VecDeque<String> = in_degree.iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(k, _)| k.clone())
        .collect();

    // 为了结果稳定,初始 queue 按 node_id 字典序排序
    let mut sorted_queue: Vec<String> = queue.drain(..).collect();
    sorted_queue.sort();
    for s in sorted_queue {
        queue.push_back(s);
    }

    let mut result: Vec<String> = Vec::with_capacity(nodes.len());
    while let Some(node_id) = queue.pop_front() {
        result.push(node_id.clone());
        let mut next_zero: Vec<String> = Vec::new();
        if let Some(succs) = adjacency.get(&node_id) {
            for succ in succs {
                let deg = in_degree.get_mut(succ).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    next_zero.push(succ.clone());
                }
            }
        }
        // 稳定排序:每批新入度为 0 的节点按字典序加入
        next_zero.sort();
        for s in next_zero {
            queue.push_back(s);
        }
    }

    if result.len() != nodes.len() {
        // 检测到环 — 找出环中的节点(in_degree > 0 的)
        let cycle_nodes: Vec<String> = in_degree.iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(k, _)| k.clone())
            .collect();
        return Err(KernelError::Skill(format!(
            "topological_sort: cycle detected involving nodes: {:?}", cycle_nodes
        )));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> DagNode {
        DagNode {
            node_id: id.into(),
            skill_id: "test.skill".into(),
            input_template: crate::skills::template::SlotTemplate {
                kind: crate::skills::template::SlotKind::Text,
                template: crate::skills::template::TemplateExpr::Literal("x".into()),
            },
            risk_ceiling: crate::policy::types::ELevel::E1,
        }
    }

    fn edge(from: &str, to: &str) -> crate::skills::dag_types::DagEdge {
        crate::skills::dag_types::DagEdge {
            from: from.into(),
            to: to.into(),
            port_binding: None,
        }
    }

    #[test]
    fn topo_sort_single_node_no_edges() {
        let nodes = vec![node("n1")];
        let edges = vec![];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(order, vec!["n1".to_string()]);
    }

    #[test]
    fn topo_sort_linear_chain() {
        let nodes = vec![node("n1"), node("n2"), node("n3")];
        let edges = vec![edge("n1", "n2"), edge("n2", "n3")];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(order, vec!["n1".to_string(), "n2".to_string(), "n3".to_string()]);
    }

    #[test]
    fn topo_sort_diamond_dependency() {
        // n1 → n2, n1 → n3, n2 → n4, n3 → n4
        let nodes = vec![node("n1"), node("n2"), node("n3"), node("n4")];
        let edges = vec![
            edge("n1", "n2"),
            edge("n1", "n3"),
            edge("n2", "n4"),
            edge("n3", "n4"),
        ];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(order[0], "n1");
        assert_eq!(order[3], "n4");
        // n2 和 n3 顺序按字典序:n2 < n3
        assert_eq!(order[1], "n2");
        assert_eq!(order[2], "n3");
    }

    #[test]
    fn topo_sort_disjoint_nodes() {
        // 两个不连通的节点,按字典序输出
        let nodes = vec![node("b"), node("a")];
        let edges = vec![];
        let order = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(order, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn topo_sort_detects_cycle_returns_err() {
        // n1 → n2 → n1(环)
        let nodes = vec![node("n1"), node("n2")];
        let edges = vec![edge("n1", "n2"), edge("n2", "n1")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cycle"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_detects_self_loop() {
        let nodes = vec![node("n1")];
        let edges = vec![edge("n1", "n1")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cycle"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_rejects_edge_with_unknown_from() {
        let nodes = vec![node("n1")];
        let edges = vec![edge("n99", "n1")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not in nodes"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_rejects_edge_with_unknown_to() {
        let nodes = vec![node("n1")];
        let edges = vec![edge("n1", "n99")];
        let err = topological_sort(&nodes, &edges).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("not in nodes"), "got: {}", msg);
    }

    #[test]
    fn topo_sort_empty_nodes_returns_empty() {
        let order = topological_sort(&[], &[]).unwrap();
        assert!(order.is_empty());
    }
}
```

- [ ] **Step 2: 在 `skills/mod.rs` 取消注释 `pub mod dag_executor;`(若 Task 2 Step 2 注释了)**

确认 `voicepilot/crates/trust-kernel/src/skills/mod.rs` 已有:

```rust
// W8 Plan 2 Task 3: DagExecutor 简单节点调度
pub mod dag_executor;
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS(可能有"field is never read" 警告 — `run_simple_node` 在 Task 4 实现,此 Task 暂未引用 `kernel` / `dag_repo` / `approver` 字段,后续 Task 引用后消除)

- [ ] **Step 4: 跑 `topological_sort` 单元测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib skills::dag_executor::tests::topo`
Expected: PASS(8 个 topo 测试全绿:`topo_sort_single_node_no_edges` / `topo_sort_linear_chain` / `topo_sort_diamond_dependency` / `topo_sort_disjoint_nodes` / `topo_sort_detects_cycle_returns_err` / `topo_sort_detects_self_loop` / `topo_sort_rejects_edge_with_unknown_from` / `topo_sort_rejects_edge_with_unknown_to` / `topo_sort_empty_nodes_returns_empty`)

注:`run_simple_node` 方法签名在本 Task 已声明但未实现,会被 `cargo check` 报"method never used"。本 Task Step 3 仅 `cargo check`(允许 warning),Step 4 `cargo test` 跑 topo 测试。

实际上 `run_simple_node` 在本 Task 的 `dag_executor.rs` 中**未**实现(只有 `run()` 调用它),所以会编译失败。修复方案:Task 4 实现 `run_simple_node`,本 Task 暂时在 `run()` 中把 `self.run_simple_node(...)` 调用替换为 `unimplemented!("Plan 2 Task 4")`,Task 4 再换回真实实现。

调整 Step 1 代码 — 在 `run()` 方法中临时把 `run_simple_node` 调用改为 stub:

```rust
            // 简单节点执行 — Task 4 实现
            let status: DagNodeStatus = if plan.loop_specs.contains_key(node_id) {
                return Err(KernelError::Skill(format!(
                    "loop node '{}' not supported in Plan 2; see Plan 3", node_id
                )));
            } else {
                // Task 4 will implement run_simple_node; for now stub with Pending
                return Err(KernelError::Skill(format!(
                    "Plan 2 Task 4 TODO: run_simple_node not yet implemented for node '{}'", node_id
                )));
            };
```

本 Task 暂时使用 stub — Task 4 实现真实 `run_simple_node` 后,替换回 Step 1 的原始 `run_simple_node` 调用代码。

- [ ] **Step 5: 再次跑 `cargo check`,确认编译通过(stub 版)**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 6: 跑 topo 单元测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib skills::dag_executor::tests::topo`
Expected: PASS(9 个 topo 测试全绿)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_executor.rs voicepilot/crates/trust-kernel/src/skills/mod.rs
git commit -m "feat(w8p2): add DagExecutor skeleton with Kahn topological sort (9 unit tests)"
```

---

## Task 4: DagExecutor 简单节点执行(run_simple_node + DagRepo 集成)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs`(追加 `run_simple_node` 测试)

**Spec §:** §2.3 "简单节点执行(`run_simple_node`)"

**背景:** 简单节点执行流程(spec §2.3):
1. `SlotTemplateEngine::resolve(node.input_template, node_outputs, user_slots, None, prev_node_id)` → 解析 input 为 `serde_json::Value`
2. 生成新 task_id + step_id(uuid)
3. 调 `dispatch_skill_executor(skill_id, kernel, resolved_input, approver, task_id, step_id)`
4. 根据 `DispatchOutcome`:
   - `outcome.succeeded=true` → `DagNodeStatus::Succeeded(outcome.output)`
   - `outcome.succeeded=false` → `DagNodeStatus::Failed{cause: outcome.error_cause}`
   - `Err(e)` → `DagNodeStatus::Failed{cause: e.to_string()}`
5. 持久化到 `DagRepo.update_node_status`(节点 start/success/fail 时调)

**user_slots 来源:** Plan 2 暂不实现"用户审批阶段填 Slot"(spec §2.1 提到 `SlotTemplate::validate` 校验 `user_slot_kinds`,但实际 Slot 值由 UI 在 Plan 5 传入)。本 Task 在 `run_simple_node` 中传空 `&[]`,模板中 `${user.xxx}` 会解析失败 → 节点 Failed。Plan 5 实现 UI 时,DagExecutor::run 签名扩展为 `run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot])`。

**DagPlan 持久化:** Plan 1 已实现 `DagRepo.create_plan` + `create_node`。本 Task 在 `run()` 主入口开始时调 `create_plan`(若 plan 不存在),每个节点执行前调 `create_node`。但 `create_plan` 需要 `&Connection` + `&DagPlan` + `&DagStatus` + `root_task_id: Option<&str>`,本 plan 暂不创建 root task(Plan 4 Router Bridge 集成时传入),传 `None`。

- [ ] **Step 1: 在 `dag_executor.rs` 实现 `run_simple_node` + `create_plan` / `create_node` 调用**

替换 Task 3 Step 1 中 `run()` 方法里的 stub `run_simple_node` 调用,并新增 `run_simple_node` 方法 + helper:

```rust
impl DagExecutor {
    // ... Task 3 的 new() / run() / persist_dag_status() 不变 ...

    /// 简单节点执行(spec §2.3)。
    ///
    /// 流程:
    /// 1. SlotTemplateEngine::resolve(node.input_template, ...) → serde_json::Value
    /// 2. 生成新 task_id + step_id(uuid)
    /// 3. dispatch_skill_executor(skill_id, kernel, resolved_input, approver, ...)
    /// 4. 根据 DispatchOutcome 构造 DagNodeStatus
    /// 5. DagRepo.update_node_status 持久化
    ///
    /// 失败语义:
    /// - 模板 resolve 失败 → DagNodeStatus::Failed{cause: "template resolution error: ..."}
    /// - dispatch 返回 Err(e) → DagNodeStatus::Failed{cause: e.to_string()}
    /// - dispatch 返回 Ok + outcome.succeeded=false → DagNodeStatus::Failed{cause: outcome.error_cause}
    /// - dispatch 返回 Ok + outcome.succeeded=true → DagNodeStatus::Succeeded(outcome.output)
    fn run_simple_node(
        &self,
        node: &DagNode,
        plan: &DagPlan,
        node_outputs: &HashMap<String, serde_json::Value>,
        prev_node_id: Option<&str>,
    ) -> Result<DagNodeStatus> {
        // Step 0: 持久化节点 start 状态(Pending → Running)
        let task_id = format!("task-{}", uuid::Uuid::new_v4());
        let step_id = format!("step-{}", uuid::Uuid::new_v4());
        {
            let conn = self.kernel.conn();
            // 若节点行不存在,先创建(spec §2.6:每个节点一行 dag_nodes)
            let existing = self.dag_repo.list_nodes_by_plan(&conn, &plan.plan_id)?;
            if !existing.iter().any(|n| n.node_id == node.node_id) {
                self.dag_repo.create_node(&conn, &plan.plan_id, node)?;
            }
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                &node.node_id,
                &DagNodeStatus::Running,
                Some(&task_id),
                Some(&step_id),
            )?;
        }

        // Step 1: 解析模板 → serde_json::Value
        let resolved_input = SlotTemplateEngine::resolve(
            &node.input_template.template,
            node_outputs,
            &[],  // user_slots: Plan 5 实现 UI 时传入,本 plan 为空
            None, // iter_var: 简单节点无循环变量
            prev_node_id,
        ).map_err(|e| {
            // 模板解析失败 → 节点 Failed
            let cause = format!("template resolution error: {}", e);
            let _ = self.mark_node_failed(plan, &node.node_id, &cause);
            KernelError::Skill(cause)
        })?;

        // Step 2: dispatch_skill_executor
        let outcome_result = dispatch_skill_executor(
            &node.skill_id,
            &self.kernel,
            &resolved_input,
            self.approver.as_ref(),
            &task_id,
            &step_id,
        );

        // Step 3: 构造 DagNodeStatus + 持久化
        let status = match outcome_result {
            Ok(outcome) if outcome.succeeded => {
                DagNodeStatus::Succeeded(outcome.output.clone())
            }
            Ok(outcome) => {
                let cause = outcome.error_cause.unwrap_or_else(|| "unknown error".into());
                DagNodeStatus::Failed { cause }
            }
            Err(e) => {
                let cause = e.to_string();
                DagNodeStatus::Failed { cause }
            }
        };

        // 持久化节点终态
        {
            let conn = self.kernel.conn();
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                &node.node_id,
                &status,
                Some(&task_id),
                Some(&step_id),
            )?;
        }

        Ok(status)
    }

    /// 标记节点 Failed(模板解析失败时调,持久化失败状态)。
    fn mark_node_failed(&self, plan: &DagPlan, node_id: &str, cause: &str) -> Result<()> {
        let conn = self.kernel.conn();
        let status = DagNodeStatus::Failed { cause: cause.to_string() };
        self.dag_repo.update_node_status(&conn, &plan.plan_id, node_id, &status, None, None)
    }
}
```

- [ ] **Step 2: 在 `run()` 方法中,把 Task 3 Step 1 的 stub 调用替换为真实 `run_simple_node`**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,找到 `run()` 方法中的 stub:

```rust
            // 简单节点执行 — Task 4 实现
            let status: DagNodeStatus = if plan.loop_specs.contains_key(node_id) {
                return Err(KernelError::Skill(format!(
                    "loop node '{}' not supported in Plan 2; see Plan 3", node_id
                )));
            } else {
                // Task 4 will implement run_simple_node; for now stub with Pending
                return Err(KernelError::Skill(format!(
                    "Plan 2 Task 4 TODO: run_simple_node not yet implemented for node '{}'", node_id
                )));
            };
```

替换为:

```rust
            // 循环节点检测 — Plan 3 实现,本 plan 返回 Err
            if plan.loop_specs.contains_key(node_id) {
                return Err(KernelError::Skill(format!(
                    "loop node '{}' not supported in Plan 2; see Plan 3",
                    node_id
                )));
            }

            // 简单节点执行
            let status = self.run_simple_node(
                node,
                plan,
                &node_outputs,
                prev_node_id.as_deref(),
            )?;
```

- [ ] **Step 3: 在 `run()` 方法开始处,加 `DagRepo.create_plan` 调用**

在 `run()` 方法 Step 1(拓扑排序)之前,加 plan 持久化:

```rust
    pub fn run(&self, plan: &DagPlan) -> Result<DagResult> {
        // Step 0: 持久化 plan 到 dag_plans 表(若不存在)
        {
            let conn = self.kernel.conn();
            if self.dag_repo.get_plan(&conn, &plan.plan_id)?.is_none() {
                self.dag_repo.create_plan(
                    &conn,
                    plan,
                    &DagStatus::Pending,
                    None, // root_task_id: Plan 4 Router Bridge 集成时传入
                )?;
            }
            // 标记 plan 为 Running
            self.dag_repo.update_plan_status(&conn, &plan.plan_id, &DagStatus::Running, false)?;
        }

        // Step 1: 拓扑排序(Kahn 算法)— 检测 edges 中的隐式环
        let order = topological_sort(&plan.nodes, &plan.edges)?;
        // ... 后续不变 ...
```

- [ ] **Step 4: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 5: 在 `tests/w8_plan2_dag_executor.rs` 追加 `run_simple_node` 集成测试**

创建测试文件(本 Task 仅写 `run_simple_node` + 简单节点成功 / 模板解析失败 / dispatch 失败 3 个测试,后续 Task 5-6 追加 Deny / PartiallySucceeded 测试):

```rust
//! W8 Plan 2 Task 4-6: DagExecutor 集成测试.
//!
//! 覆盖:
//! - Task 4: run_simple_node(简单节点成功 / 模板失败 / dispatch 失败)
//! - Task 5: DAG 骨架审批 Deny 短路
//! - Task 6: 失败处理 + PartiallySucceeded 分支

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagNodeStatus, DagPlan, DagStatus,
};
use trust_kernel::skills::template::{
    SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope,
};

fn literal_node(id: &str, skill_id: &str, literal: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal(literal.into()),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn var_node(id: &str, skill_id: &str, scope: VarScope, path: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Var(VarRef { scope, path: path.into() }),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with(nodes: Vec<DagNode>, edges: Vec<DagEdge>) -> DagPlan {
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test goal".into(),
        nodes,
        edges,
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    }
}

fn make_executor(kernel: TrustKernel, approver: Arc<dyn Approver>) -> (Arc<TrustKernel>, DagExecutor) {
    let kernel_arc = Arc::new(kernel);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);
    (kernel_arc, executor)
}

#[test]
fn run_simple_node_succeeds_with_task_explain() {
    // 单节点 DAG,task.explain + limit=5 + AutoApprover → 节点 Succeeded
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let node = DagNode {
        node_id: "n1".into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Number,
            template: TemplateExpr::Literal("5".into()),
        },
        risk_ceiling: ELevel::E0,
    };
    // 用 serde_json 构造 resolved input(direct call run_simple_node)
    // 但 run_simple_node 是 private — 通过 run() 间接测试
    let plan = plan_with(vec![node], vec![]);
    let result = executor.run(&plan).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    let n1_status = result.node_results.get("n1").unwrap();
    assert!(n1_status.is_succeeded(), "got: {:?}", n1_status);
}

#[test]
fn run_simple_node_fails_on_template_resolution_error() {
    // ${prev.output.path} 但 prev_node_id=None → VarNotFound → 节点 Failed
    // 注:模板校验在 LLM 返回后立即做(spec §2.1 双层防御),此处测试 Layer 2
    // (执行前再次校验)。LLM 校验在 Task 8 测试。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let node = var_node("n1", "task.explain", VarScope::Prev, "output.path");
    let plan = plan_with(vec![node], vec![]);
    let result = executor.run(&plan).unwrap();
    // 模板解析失败 → 节点 Failed → DAG Failed(无成功节点)
    match result.status {
        DagStatus::Failed { ref failed_node, .. } => {
            assert_eq!(failed_node, "n1");
        }
        other => panic!("expected Failed, got {:?}", other),
    }
    let n1_status = result.node_results.get("n1").unwrap();
    assert!(n1_status.is_failed(), "got: {:?}", n1_status);
}

#[test]
fn run_simple_node_fails_on_unknown_skill_id() {
    // skill_id = "nonexistent.skill" → dispatch_skill_executor 返回 Err
    // → 节点 Failed → DAG Failed
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let node = literal_node("n1", "nonexistent.skill", "test");
    let plan = plan_with(vec![node], vec![]);
    let result = executor.run(&plan).unwrap();
    match result.status {
        DagStatus::Failed { ref failed_node, ref cause } => {
            assert_eq!(failed_node, "n1");
            assert!(cause.contains("unknown skill_id"), "got: {}", cause);
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}
```

- [ ] **Step 6: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_dag_executor`
Expected: PASS(3 个测试:`run_simple_node_succeeds_with_task_explain` / `run_simple_node_fails_on_template_resolution_error` / `run_simple_node_fails_on_unknown_skill_id`)

注意:`run_simple_node_succeeds_with_task_explain` 中节点 input_template 是 `Literal("5")`,`resolve` 后得到 `serde_json::Value::String("5")`。`dispatch_task_explain` 调 `extract_u32(v, "limit")` 会失败(因为 "5" 是 String 而非 Number)。修复:在 dispatcher.rs 的 `extract_u32` 中支持字符串数字解析,或在测试中用 `Literal` 但实际是数字字面量。

更简单的修复:把 `extract_u32` 改为容错(尝试 as_u64 / as_str 然后 parse):

```rust
fn extract_u32(v: &serde_json::Value, key: &str) -> Result<Option<u32>> {
    match v.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => {
            n.as_u64().map(|x| Some(x as u32))
                .ok_or_else(|| KernelError::Skill(format!("dispatch: field '{}' must be u32", key)))
        }
        Some(serde_json::Value::String(s)) => {
            s.parse::<u32>().map(Some).map_err(|_| KernelError::Skill(format!("dispatch: field '{}' must be u32", key)))
        }
        Some(_) => Err(KernelError::Skill(format!("dispatch: field '{}' must be u32", key))),
    }
}
```

但这样 `dispatch_task_explain` 仍要 `v.get("limit")` 找到 `"5"` — 而 `run_simple_node` 中 `resolved_input = resolve(template)`,若 template 是 `Literal("5")`,resolved 是 `Value::String("5")`,**不是** `{"limit": "5"}` 对象。

修复方案:测试改为使用 `Literal("{\"limit\": 5}")` 并在 dispatcher 中 `serde_json::from_str` 解析?这不对。

更好的方案:`run_simple_node` 中 `resolved_input` 是单个 `serde_json::Value`,而 dispatcher 期望它是 JSON 对象(`v.get(key)`)。所以 DagNode 的 `input_template` 应该解析为对象,而非字符串。

实际场景中,LLM 返回的 `input_template.template` 是一个对象字面量,如 `{"limit": 5}`,解析后是 `Value::Object`。所以 `SlotTemplateEngine::parse` 应支持对象字面量?但 Plan 1 的 `parse` 只支持字符串字面量 + `${...}` 占位符。

**关键设计点:** Plan 1 的 `SlotTemplate` 是单个表达式,LLM 返回的 `input_template.template` 应该是单个 JSON 值的字符串表示,如 `"5"` 或 `"${prev.output.path}"`。dispatcher 接收 `resolved_input: &serde_json::Value`,直接把它当作整个 input 对象传给 executor 的 input struct 反序列化?

但 `dispatch_task_explain` 中 `extract_u32(v, "limit")` 期望 `v` 是 `{"limit": 5}` 对象。所以 `run_simple_node` 解析模板后得到的是 `Value::String("5")`,不是 `Value::Object({"limit": 5})`。

修复方案 A:`dispatch_*` 函数中,若 resolved_input 不是对象,把它包装成 `{"value": <resolved>}`?不对。

修复方案 B:LLM 拆解时,`input_template.template` 字符串本身就是一个 JSON 对象字面量,如 `"{\"limit\": 5}"`,解析后 `TemplateExpr::Literal("{\"limit\": 5}")`,`resolve` 后得到 `Value::String("{\"limit\": 5}")`,然后 dispatcher 中 `serde_json::from_str` 解析为对象。

这是正确的语义。修改 dispatcher 中所有 `dispatch_*` 函数:在 `extract_string` / `extract_u32` 之前,若 `resolved_input` 是 String,先 `serde_json::from_str` 解析为 Value。

更好的方案:在 `dispatch_skill_executor` 入口加一个 normalize 步骤:

```rust
pub fn dispatch_skill_executor(
    skill_id: &str,
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // 若 resolved_input 是 String,尝试解析为 JSON 对象(LLM 模板常返回 JSON 字符串)
    let normalized = if let Some(s) = resolved_input.as_str() {
        serde_json::from_str::<serde_json::Value>(s).unwrap_or_else(|_| resolved_input.clone())
    } else {
        resolved_input.clone()
    };
    let resolved_input = &normalized;

    match skill_id {
        // ...
    }
}
```

修改 Step 1 的 `dispatch_skill_executor` 函数,加入 normalize 逻辑。

- [ ] **Step 7: 修改 `dispatcher.rs` 的 `dispatch_skill_executor`,加入 input normalize**

打开 `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs`,在 `dispatch_skill_executor` 函数开头(`match skill_id` 之前)加:

```rust
    // Normalize: 若 resolved_input 是 String,尝试解析为 JSON 对象。
    // LLM 拆解时 input_template.template 常返回 JSON 字符串(如 '{"limit": 5}'),
    // SlotTemplateEngine::resolve 把 Literal 编译为 Value::String,
    // 此处把 String 解析回 Object,让 dispatch_* 的 extract_* 能正常工作。
    let normalized: serde_json::Value = if let Some(s) = resolved_input.as_str() {
        serde_json::from_str(s).unwrap_or_else(|_| resolved_input.clone())
    } else {
        resolved_input.clone()
    };
    let resolved_input = &normalized;

    match skill_id {
```

- [ ] **Step 8: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_dag_executor`
Expected: PASS(3 个测试全绿)

- [ ] **Step 9: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS(0 warnings)

- [ ] **Step 10: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_executor.rs voicepilot/crates/trust-kernel/src/skills/dispatcher.rs voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs
git commit -m "feat(w8p2): implement DagExecutor::run_simple_node with template resolution + dispatch + DagRepo persistence"
```

---

## Task 5: DAG 骨架审批 + Deny 短路

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(已在 Task 3-4 实现 `run()` 中的 `approve_dag_skeleton` 调用,本 Task 仅追加测试)
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs`(追加 Deny 短路测试)

**Spec §:** §2.3 "全局审批 — DAG 骨架预览 + Allow/Deny(决策 #2)" + §5 错误处理 "DAG 骨架审批 Deny → DagStatus=Cancelled,不执行任何节点"

**背景:** Task 3 的 `run()` 方法 Step 2 已调用 `self.approver.approve_dag_skeleton(plan)?`,若返回 Deny 则 `DagResult::cancelled()`。本 Task 仅追加测试验证 Deny 短路语义 — 0 节点执行,审计记录 `dag_skeleton_approved` Deny(Task 7 实现)。

- [ ] **Step 1: 在 `tests/w8_plan2_dag_executor.rs` 追加 Deny 短路测试**

```rust
#[test]
fn run_with_auto_denier_returns_cancelled_and_zero_node_results() {
    // AutoDenier 在骨架审批阶段 Deny → DagStatus=Cancelled,0 节点执行
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let n1 = literal_node("n1", "task.explain", "5");
    let n2 = literal_node("n2", "task.explain", "10");
    let plan = plan_with(vec![n1, n2], vec![DagEdge {
        from: "n1".into(),
        to: "n2".into(),
        port_binding: None,
    }]);
    let result = executor.run(&plan).unwrap();
    assert_eq!(result.status, DagStatus::Cancelled);
    // 0 节点执行(Deny 短路)
    assert!(result.node_results.is_empty(), "Deny short-circuit must execute 0 nodes, got: {:?}", result.node_results);
}

#[test]
fn run_with_auto_approver_proceeds_to_node_execution() {
    // AutoApprover 在骨架审批阶段 Allow → 节点正常执行
    // 已在 Task 4 Step 5 测试 `run_simple_node_succeeds_with_task_explain`,
    // 此处补一个 2 节点 DAG 串行执行的测试。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let n1 = literal_node("n1", "task.explain", "5");
    let n2 = literal_node("n2", "task.explain", "10");
    let plan = plan_with(
        vec![n1, n2],
        vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
    );
    let result = executor.run(&plan).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    assert_eq!(result.node_results.len(), 2);
    assert!(result.node_results.get("n1").unwrap().is_succeeded());
    assert!(result.node_results.get("n2").unwrap().is_succeeded());
}

#[test]
fn run_with_deny_persists_cancelled_status_to_dag_plans_table() {
    // 验证 Deny 短路后,dag_plans.status 被持久化为 "cancelled"
    let kernel = TrustKernel::open_in_memory().unwrap();
    let dag_repo = Arc::new(DagRepo::new());
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo.clone());

    let n1 = literal_node("n1", "task.explain", "5");
    let plan = plan_with(vec![n1], vec![]);
    let result = executor.run(&plan).unwrap();
    assert_eq!(result.status, DagStatus::Cancelled);

    // 验证 DB 持久化
    let conn = kernel_arc.conn();
    let rec = dag_repo.get_plan(&conn, &plan.plan_id).unwrap().unwrap();
    assert_eq!(rec.status, "cancelled");
}
```

- [ ] **Step 2: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_dag_executor`
Expected: PASS(Task 4 的 3 个 + Task 5 的 3 个 = 6 个测试全绿)

- [ ] **Step 3: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs
git commit -m "test(w8p2): add DAG skeleton Deny short-circuit tests (3 tests)"
```

---

## Task 6: 失败处理 + PartiallySucceeded 分支

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(Task 3 已实现 `run()` 中的失败处理逻辑,本 Task 仅追加测试验证)
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs`(追加 PartiallySucceeded 测试)

**Spec §:** §2.3 "失败处理(决策 #4 + #8):节点 Failed 时,前面已 commit 不回滚,DAG 走 Failed 或 PartiallySucceeded"

**背景:** Task 3 Step 1 的 `run()` 方法已实现失败处理逻辑:
- 节点 Failed 时,收集已成功节点 `succeeded: Vec<String>`
- 若 `succeeded.is_empty()` → `DagStatus::Failed { failed_node, cause }`
- 否则 → `DagStatus::PartiallySucceeded { succeeded, failed_node, cause }`
- 前序已 commit 节点不回滚(决策 #4)

本 Task 通过 mock 第一个节点成功 + 第二个节点失败,验证 PartiallySucceeded 分支。由于 `dispatch_skill_executor` 路由到真实 executor,要让第二个节点失败,简单方法:第二个节点用 `unknown.skill`(触发 `unknown skill_id` 错误)。

- [ ] **Step 1: 在 `tests/w8_plan2_dag_executor.rs` 追加 PartiallySucceeded 测试**

```rust
#[test]
fn run_with_first_node_fails_returns_failed_status() {
    // 单节点 DAG + 未知 skill_id → 节点 Failed → DAG Failed(无成功节点)
    // 已在 Task 4 Step 5 测试 `run_simple_node_fails_on_unknown_skill_id`,
    // 此处显式断言 DagStatus::Failed(而非 PartiallySucceeded)。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let n1 = literal_node("n1", "nonexistent.skill", "test");
    let plan = plan_with(vec![n1], vec![]);
    let result = executor.run(&plan).unwrap();
    match result.status {
        DagStatus::Failed { ref failed_node, ref cause } => {
            assert_eq!(failed_node, "n1");
            assert!(cause.contains("unknown skill_id"), "got: {}", cause);
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

#[test]
fn run_with_second_node_fails_returns_partially_succeeded() {
    // 2 节点 DAG:n1 = task.explain(成功)+ n2 = nonexistent.skill(失败)
    // → DAG PartiallySucceeded { succeeded: ["n1"], failed_node: "n2" }
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let n1 = literal_node("n1", "task.explain", "5");
    let n2 = literal_node("n2", "nonexistent.skill", "test");
    let plan = plan_with(
        vec![n1, n2],
        vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
    );
    let result = executor.run(&plan).unwrap();
    match result.status {
        DagStatus::PartiallySucceeded { ref succeeded, ref failed_node, .. } => {
            assert_eq!(failed_node, "n2");
            assert_eq!(succeeded.len(), 1);
            assert_eq!(succeeded[0], "n1");
        }
        other => panic!("expected PartiallySucceeded, got {:?}", other),
    }
    // n1 应该是 Succeeded,n2 应该是 Failed
    assert!(result.node_results.get("n1").unwrap().is_succeeded());
    assert!(result.node_results.get("n2").unwrap().is_failed());
}

#[test]
fn run_with_middle_node_fails_does_not_rollback_committed_nodes() {
    // 3 节点 DAG:n1(成功)+ n2(失败)+ n3(应被跳过,因为 n2 失败短路)
    // → DAG PartiallySucceeded { succeeded: ["n1"], failed_node: "n2" }
    // n3 不在 node_results 中(短路)
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let n1 = literal_node("n1", "task.explain", "5");
    let n2 = literal_node("n2", "nonexistent.skill", "test");
    let n3 = literal_node("n3", "task.explain", "10");
    let plan = plan_with(
        vec![n1, n2, n3],
        vec![
            DagEdge { from: "n1".into(), to: "n2".into(), port_binding: None },
            DagEdge { from: "n2".into(), to: "n3".into(), port_binding: None },
        ],
    );
    let result = executor.run(&plan).unwrap();
    match result.status {
        DagStatus::PartiallySucceeded { ref succeeded, ref failed_node, .. } => {
            assert_eq!(failed_node, "n2");
            assert_eq!(succeeded, &vec!["n1".to_string()]);
        }
        other => panic!("expected PartiallySucceeded, got {:?}", other),
    }
    // n3 应不在 node_results(失败短路,未执行)
    assert!(result.node_results.get("n3").is_none(),
        "n3 should not be in node_results (short-circuited), got: {:?}", result.node_results);
}

#[test]
fn run_persists_succeeded_status_to_dag_plans_table() {
    // 验证全部成功后,dag_plans.status 被持久化为 "succeeded"
    let kernel = TrustKernel::open_in_memory().unwrap();
    let dag_repo = Arc::new(DagRepo::new());
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo.clone());

    let n1 = literal_node("n1", "task.explain", "5");
    let plan = plan_with(vec![n1], vec![]);
    let result = executor.run(&plan).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);

    let conn = kernel_arc.conn();
    let rec = dag_repo.get_plan(&conn, &plan.plan_id).unwrap().unwrap();
    assert_eq!(rec.status, "succeeded");
    assert!(rec.completed_at.is_some(), "completed_at should be set");
}

#[test]
fn run_persists_failed_status_to_dag_plans_table() {
    // 验证 Failed 终态持久化
    let kernel = TrustKernel::open_in_memory().unwrap();
    let dag_repo = Arc::new(DagRepo::new());
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo.clone());

    let n1 = literal_node("n1", "nonexistent.skill", "test");
    let plan = plan_with(vec![n1], vec![]);
    let result = executor.run(&plan).unwrap();
    assert!(matches!(result.status, DagStatus::Failed { .. }));

    let conn = kernel_arc.conn();
    let rec = dag_repo.get_plan(&conn, &plan.plan_id).unwrap().unwrap();
    assert_eq!(rec.status, "failed");
    assert!(rec.completed_at.is_some());
}
```

- [ ] **Step 2: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_dag_executor`
Expected: PASS(Task 4 的 3 个 + Task 5 的 3 个 + Task 6 的 5 个 = 11 个测试全绿)

- [ ] **Step 3: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w8_plan2_dag_executor.rs
git commit -m "test(w8p2): add PartiallySucceeded + failed-status persistence tests (5 tests)"
```

---

## Task 7: 审计事件 6 个 event_type

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(在 `run()` / `run_simple_node()` 中追加 `audit_append_external` 调用)
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_audit_events.rs`

**Spec §:** §6.1 审计事件类型(W8 新增 `audit_logs.event_type`)

**6 个事件:**

| event_type | 触发时机 | 关键字段 |
|---|---|---|
| `dag_plan_created` | LLM 拆解完成 / DagExecutor::run 开始 | `plan_id`, `node_count`, `max_total_steps` |
| `dag_skeleton_approved` | 用户审批 DAG 骨架 Allow/Deny | `plan_id`, `decision` |
| `dag_node_started` | 节点开始执行 | `plan_id`, `node_id`, `skill_id` |
| `dag_node_succeeded` | 节点成功 | `plan_id`, `node_id`, `evidence_strength` |
| `dag_node_failed` | 节点失败 | `plan_id`, `node_id`, `cause` |
| `dag_completed` | DAG 终态 | `plan_id`, `final_status`, `succeeded_count`, `failed_node?` |

**审计复用 W1 `audit_append_external`:** TrustKernel 已有 `audit_append_external(task_id, step_id, event_type, details) -> Result<()>`。DagExecutor 在 6 个时机调此方法,task_id 用 DAG plan_id(或临时 task_id,因为 plan 不一定有 root_task_id)。

**FK 约束:** `audit_logs.task_id` REFERENCES `tasks(task_id)`,所以必须先创建一个 task。DagExecutor 在 `run()` 开始时调 `kernel.create_task(task_id, user_goal)` 创建 root task,然后所有审计事件用此 task_id。`DagRepo.create_plan` 的 `root_task_id` 参数也传入此 task_id。

- [ ] **Step 1: 修改 `dag_executor.rs` 的 `run()` 方法,在 Step 0 创建 root task + 发 `dag_plan_created` 事件**

在 `run()` 方法 Step 0(持久化 plan)之前,加 root task 创建:

```rust
    pub fn run(&self, plan: &DagPlan) -> Result<DagResult> {
        // Step 0a: 创建 root task(供 audit_logs.task_id FK + dag_plans.root_task_id 关联)
        let root_task_id = format!("task-dag-{}", uuid::Uuid::new_v4());
        self.kernel.create_task(&root_task_id, &plan.user_goal)?;

        // Step 0b: 持久化 plan 到 dag_plans 表(若不存在)
        {
            let conn = self.kernel.conn();
            if self.dag_repo.get_plan(&conn, &plan.plan_id)?.is_none() {
                self.dag_repo.create_plan(
                    &conn,
                    plan,
                    &DagStatus::Pending,
                    Some(&root_task_id),
                )?;
            }
            self.dag_repo.update_plan_status(&conn, &plan.plan_id, &DagStatus::Running, false)?;
        }

        // Step 0c: 审计 — dag_plan_created
        self.kernel.audit_append_external(
            &root_task_id,
            None,
            "dag_plan_created",
            serde_json::json!({
                "plan_id": plan.plan_id,
                "node_count": plan.nodes.len(),
                "max_total_steps": plan.max_total_steps,
            }),
        )?;

        // Step 1: 拓扑排序
        let order = topological_sort(&plan.nodes, &plan.edges)?;

        // Step 2: 全局审批
        let decision = self.approver.approve_dag_skeleton(plan)?;
        // 审计 — dag_skeleton_approved(无论 Allow/Deny 都记录)
        self.kernel.audit_append_external(
            &root_task_id,
            None,
            "dag_skeleton_approved",
            serde_json::json!({
                "plan_id": plan.plan_id,
                "decision": format!("{:?}", decision),
            }),
        )?;
        if matches!(decision, crate::approval::types::ApprovalDecision::Deny) {
            self.persist_dag_status(plan, &DagStatus::Cancelled)?;
            // 审计 — dag_completed(Cancelled)
            self.kernel.audit_append_external(
                &root_task_id,
                None,
                "dag_completed",
                serde_json::json!({
                    "plan_id": plan.plan_id,
                    "final_status": "cancelled",
                    "succeeded_count": 0,
                }),
            )?;
            return Ok(DagResult::cancelled());
        }

        // Step 3: 按拓扑序执行
        let mut node_outputs: HashMap<String, serde_json::Value> = HashMap::new();
        let mut node_results: HashMap<String, DagNodeStatus> = HashMap::new();
        let mut prev_node_id: Option<String> = None;

        for node_id in &order {
            let node = plan.nodes.iter().find(|n| &n.node_id == node_id)
                .ok_or_else(|| KernelError::Skill(format!("topo sort returned unknown node_id: {}", node_id)))?;

            if plan.loop_specs.contains_key(node_id) {
                return Err(KernelError::Skill(format!(
                    "loop node '{}' not supported in Plan 2; see Plan 3", node_id
                )));
            }

            // 审计 — dag_node_started
            self.kernel.audit_append_external(
                &root_task_id,
                None,
                "dag_node_started",
                serde_json::json!({
                    "plan_id": plan.plan_id,
                    "node_id": node.node_id,
                    "skill_id": node.skill_id,
                }),
            )?;

            let status = self.run_simple_node(
                node,
                plan,
                &node_outputs,
                prev_node_id.as_deref(),
                &root_task_id,
            )?;

            let is_failed = status.is_failed();
            node_results.insert(node_id.clone(), status.clone());

            // 审计 — dag_node_succeeded / dag_node_failed
            if let DagNodeStatus::Succeeded(out) = &status {
                node_outputs.insert(node_id.clone(), out.clone());
                prev_node_id = Some(node_id.clone());
                self.kernel.audit_append_external(
                    &root_task_id,
                    None,
                    "dag_node_succeeded",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "node_id": node.node_id,
                        "evidence_strength": "weak", // W8 简化,实际由 executor ToolResult 决定
                    }),
                )?;
            } else if status.is_failed() {
                let cause = match &status {
                    DagNodeStatus::Failed { cause } => cause.clone(),
                    _ => String::new(),
                };
                self.kernel.audit_append_external(
                    &root_task_id,
                    None,
                    "dag_node_failed",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "node_id": node.node_id,
                        "cause": cause,
                    }),
                )?;
            }

            if is_failed {
                let succeeded: Vec<String> = order.iter()
                    .filter(|nid| node_results.get(*nid).map(|s| s.is_succeeded()).unwrap_or(false))
                    .cloned()
                    .collect();
                let cause = match &status {
                    DagNodeStatus::Failed { cause } => cause.clone(),
                    _ => String::new(),
                };
                let final_status = if succeeded.is_empty() {
                    DagStatus::Failed { failed_node: node_id.clone(), cause }
                } else {
                    DagStatus::PartiallySucceeded { succeeded, failed_node: node_id.clone(), cause }
                };
                self.persist_dag_status(plan, &final_status)?;
                // 审计 — dag_completed
                let succeeded_count = order.iter()
                    .filter(|nid| node_results.get(*nid).map(|s| s.is_succeeded()).unwrap_or(false))
                    .count();
                self.kernel.audit_append_external(
                    &root_task_id,
                    None,
                    "dag_completed",
                    serde_json::json!({
                        "plan_id": plan.plan_id,
                        "final_status": final_status.as_str(),
                        "succeeded_count": succeeded_count,
                        "failed_node": node_id,
                    }),
                )?;
                return Ok(DagResult { status: final_status, node_results });
            }
        }

        // Step 4: 全部成功
        let final_status = DagStatus::Succeeded;
        self.persist_dag_status(plan, &final_status)?;
        // 审计 — dag_completed
        let succeeded_count = node_results.values().filter(|s| s.is_succeeded()).count();
        self.kernel.audit_append_external(
            &root_task_id,
            None,
            "dag_completed",
            serde_json::json!({
                "plan_id": plan.plan_id,
                "final_status": final_status.as_str(),
                "succeeded_count": succeeded_count,
            }),
        )?;
        Ok(DagResult::succeeded(node_results))
    }
```

- [ ] **Step 2: 修改 `run_simple_node` 签名,加 `root_task_id: &str` 参数**

```rust
    fn run_simple_node(
        &self,
        node: &DagNode,
        plan: &DagPlan,
        node_outputs: &HashMap<String, serde_json::Value>,
        prev_node_id: Option<&str>,
        root_task_id: &str,
    ) -> Result<DagNodeStatus> {
        // ... 实现不变,但内部 dispatch_skill_executor 调用不变,
        // root_task_id 仅用于 audit(若需要在节点级也发 audit,可用 root_task_id)。
        // 本 plan 中节点级 audit 在 run() 中发,run_simple_node 不重复发。
```

注:`root_task_id` 在 `run_simple_node` 内部暂未使用,保留参数为后续 Plan 3 循环节点审计用。若 clippy 报"unused variable",加 `_` 前缀:`_root_task_id`。

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 4: 创建 `tests/w8_plan2_audit_events.rs`,6 个 event_type 各 1 个断言**

```rust
//! W8 Plan 2 Task 7: DAG 审计事件测试.
//!
//! Spec §6.1:6 个 event_type 各至少 1 个断言。
//! - dag_plan_created
//! - dag_skeleton_approved
//! - dag_node_started
//! - dag_node_succeeded
//! - dag_node_failed
//! - dag_completed

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagNode, DagPlan, DagStatus};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

fn literal_node(id: &str, skill_id: &str, lit: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal(lit.into()),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with(nodes: Vec<DagNode>) -> DagPlan {
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "audit test goal".into(),
        nodes,
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    }
}

fn find_audit_events(kernel: &TrustKernel, event_type: &str) -> Vec<serde_json::Value> {
    let recent = kernel.list_audit_recent(100).unwrap();
    recent.into_iter()
        .filter(|e| e.event_type == event_type)
        .map(|e| e.details)
        .collect()
}

#[test]
fn audit_emits_dag_plan_created_on_run_start() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_plan_created");
    assert!(!events.is_empty(), "dag_plan_created must be emitted");
    let e = &events[0];
    assert_eq!(e["plan_id"], plan.plan_id);
    assert_eq!(e["node_count"], 1);
    assert_eq!(e["max_total_steps"], 10);
}

#[test]
fn audit_emits_dag_skeleton_approved_on_allow() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_skeleton_approved");
    assert!(!events.is_empty());
    let e = &events[0];
    assert_eq!(e["plan_id"], plan.plan_id);
    assert_eq!(e["decision"], "Allow");
}

#[test]
fn audit_emits_dag_skeleton_approved_on_deny() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_skeleton_approved");
    assert!(!events.is_empty());
    let e = &events[0];
    assert_eq!(e["decision"], "Deny");
}

#[test]
fn audit_emits_dag_node_started_on_each_node() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    let plan = plan_with(vec![
        literal_node("n1", "task.explain", "5"),
        literal_node("n2", "task.explain", "10"),
    ]);
    executor.run(&plan).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_node_started");
    assert_eq!(events.len(), 2, "expected 2 dag_node_started events, got: {:?}", events);
    let n1_event = events.iter().find(|e| e["node_id"] == "n1").unwrap();
    assert_eq!(n1_event["skill_id"], "task.explain");
}

#[test]
fn audit_emits_dag_node_succeeded_on_success() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_node_succeeded");
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e["node_id"], "n1");
    assert!(e["evidence_strength"].is_string());
}

#[test]
fn audit_emits_dag_node_failed_on_failure() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    let plan = plan_with(vec![literal_node("n1", "nonexistent.skill", "test")]);
    executor.run(&plan).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_node_failed");
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e["node_id"], "n1");
    let cause = e["cause"].as_str().unwrap();
    assert!(cause.contains("unknown skill_id"), "got: {}", cause);
}

#[test]
fn audit_emits_dag_completed_on_terminal_status() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    // 测试 4 种终态各发 1 个 dag_completed 事件
    // 1. Succeeded
    let plan_ok = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan_ok).unwrap();
    // 2. Failed
    let plan_fail = plan_with(vec![literal_node("n1", "nonexistent.skill", "x")]);
    executor.run(&plan_fail).unwrap();
    // 3. Cancelled(用 AutoDenier)
    let kernel2 = TrustKernel::open_in_memory().unwrap();
    let dag_repo2 = Arc::new(DagRepo::new());
    let approver_deny: Arc<dyn Approver> = Arc::new(AutoDenier);
    let kernel2_arc = Arc::new(kernel2);
    let executor2 = DagExecutor::new(kernel2_arc.clone(), approver_deny, dag_repo2);
    let plan_cancel = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor2.run(&plan_cancel).unwrap();

    // 验证 kernel1(Succeeded + Failed)
    let events1 = find_audit_events(&kernel_arc, "dag_completed");
    assert_eq!(events1.len(), 2, "expected 2 dag_completed events, got: {:?}", events1);
    let succeeded_ev = events1.iter().find(|e| e["final_status"] == "succeeded").unwrap();
    assert_eq!(succeeded_ev["succeeded_count"], 1);
    let failed_ev = events1.iter().find(|e| e["final_status"] == "failed").unwrap();
    assert_eq!(failed_ev["failed_node"], "n1");

    // 验证 kernel2(Cancelled)
    let events2 = find_audit_events(&kernel2_arc, "dag_completed");
    assert_eq!(events2.len(), 1);
    let cancelled_ev = &events2[0];
    assert_eq!(cancelled_ev["final_status"], "cancelled");
    assert_eq!(cancelled_ev["succeeded_count"], 0);
}

#[test]
fn audit_hash_chain_remains_intact_after_dag_events() {
    // 验证 DAG 审计事件不破坏 W1 哈希链
    // W1 audit_append 自动链接 prev_hash;此处仅验证事件序列可被 list_audit_recent 读出
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan).unwrap();

    let recent = kernel_arc.list_audit_recent(50).unwrap();
    // 至少应有:dag_plan_created + dag_skeleton_approved + dag_node_started + dag_node_succeeded + dag_completed
    // + W1 TASK_CREATED + STATE_TRANSITION(kernel.create_task 触发)
    assert!(recent.len() >= 5, "expected >= 5 audit events, got: {}", recent.len());

    // 验证所有事件都有 hash + prev_hash(W1 哈希链)
    for ev in &recent {
        assert!(!ev.hash.is_empty(), "audit event hash must be non-empty: {:?}", ev);
    }
}
```

- [ ] **Step 5: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_audit_events`
Expected: PASS(8 个测试全绿)

- [ ] **Step 6: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_executor.rs voicepilot/crates/trust-kernel/tests/w8_plan2_audit_events.rs
git commit -m "feat(w8p2): emit 6 DAG audit event types (plan_created/skeleton_approved/node_started/succeeded/failed/completed)"
```

---

## Task 8: LlmClient::decompose_to_dag 实现 + wiremock 6 场景测试

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/llm/client.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan2_llm_decompose.rs`

**Spec §:** §2.2 LlmClient::decompose_to_dag

**Feature gate:** `#[cfg(feature = "llm")]` 门控 — 默认 `cargo build` 不含 LLM 代码,本 Task 所有代码在 `#[cfg(feature = "llm")]` 块内。

**Function calling schema(spec §2.2):**

```json
{
  "name": "decompose_to_dag",
  "parameters": {
    "type": "object",
    "properties": {
      "nodes": {"type": "array", "items": {...}},
      "edges": {"type": "array", ...},
      "loop_specs": {"type": "object"},
      "max_total_steps": {"type": "integer", "maximum": 20}
    }
  }
}
```

**System prompt 中文约束(spec §2.2):**
- 你是 VoicePilot 的 DAG 拆解器
- 不得引用未在 candidate_skills 中的 skill_id
- 模板 `${...}` 必须指向合法 scope(Prev/Step/User/Iter)
- 循环必须填 max_iterations(默认 10,硬上限 50)
- max_total_steps 硬上限 20
- 若只需单 Skill,返回单节点 DAG

**错误处理(spec §2.2 + §5):**
- HTTP 失败 / JSON 解析失败 → Err(LlmError::Http / Parse)
- 非法 skill_id(不在 candidate_skills)→ Err(LlmError::Parse)
- 模板语法错误 → Err(LlmError::Parse)(SlotTemplateEngine::validate_dag 失败)
- 超时 30s → Err(LlmError::Timeout)
- API key 无效(401)→ Err(LlmError::Http)
- max_total_steps > 20 → Err(LlmError::Parse)
- 调用方(router_bridge Plan 4)catch 这些 Err 并回退到 W7 单 Skill 路由

**Privacy:**
- `is_enabled() = false`(api_key 空)→ Err(LlmError::NotConfigured)
- `privacy_mode = true` 由调用方检查(spec §2.2),本方法不直接读 privacy_mode

**校验时机:** LLM 返回后立即校验(双层防御 Layer 1):
1. `max_total_steps <= 20`(硬上限)
2. 所有 `skill_id` 在 `candidate_skills` 中
3. `SlotTemplateEngine::validate_dag(&dag)` 通过(模板语法 + node_id 引用 + iter 仅在循环节点)
4. `DagPlan::validate_edges()` + `validate_loop_specs()` 通过

- [ ] **Step 1: 在 `llm/client.rs` 末尾(`impl LlmClient` 块内)追加 `decompose_to_dag` 方法 + 私有 helper**

打开 `voicepilot/crates/trust-kernel/src/llm/client.rs`,在 `impl LlmClient` 块内(`parse_tool_call_response` 方法之后、`#[cfg(test)] mod tests` 之前)追加:

```rust
    /// W8 Plan 2:语音 → 完整 DAG plan。
    ///
    /// 复用 W7 的 OpenAI 兼容 `/chat/completions` + function calling。
    /// 失败时返回 `LlmError`,调用方(router_bridge Plan 4)catch 并回退到
    /// W7 单 Skill 关键词路由。
    ///
    /// 校验(双层防御 Layer 1,spec §2.1):
    /// 1. max_total_steps ≤ 20
    /// 2. 所有 skill_id 在 candidate_skills 中
    /// 3. SlotTemplateEngine::validate_dag 通过(模板语法 + 引用合法性)
    /// 4. DagPlan::validate_edges + validate_loop_specs 通过
    ///
    /// 任一校验失败 → Err(LlmError::Parse),不返回部分结果。
    #[cfg(feature = "llm")]
    pub async fn decompose_to_dag(
        &self,
        user_text: &str,
        candidate_skills: &[crate::skills::manifest::SkillManifest],
        user_slots: &[crate::llm::types::ExtractedSlot],
    ) -> LlmResult<crate::skills::dag_types::DagPlan> {
        use crate::skills::dag_types::{DagPlan, MAX_TOTAL_STEPS_HARD_LIMIT};
        use crate::skills::template::SlotTemplateEngine;

        if !self.is_enabled() {
            return Err(crate::llm::types::LlmError::NotConfigured);
        }

        let system_prompt = self.build_decompose_system_prompt(candidate_skills, user_slots);
        let tools = self.build_decompose_tool_schema();
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_text},
            ],
            "tools": tools,
            "tool_choice": {"type": "function", "function": {"name": "decompose_to_dag"}},
            "temperature": 0.1,
        });

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    crate::llm::types::LlmError::Timeout(self.timeout)
                } else {
                    crate::llm::types::LlmError::Http(e.to_string())
                }
            })?;

        if !resp.status().is_success() {
            return Err(crate::llm::types::LlmError::Http(format!("HTTP {}", resp.status())));
        }

        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::llm::types::LlmError::Parse(format!("response body parse: {e}")))?;

        let plan = self.parse_decompose_response(&resp_json, candidate_skills)?;

        // 校验 1:max_total_steps ≤ 20
        if plan.max_total_steps > MAX_TOTAL_STEPS_HARD_LIMIT {
            return Err(crate::llm::types::LlmError::Parse(format!(
                "max_total_steps {} exceeds hard limit {}",
                plan.max_total_steps, MAX_TOTAL_STEPS_HARD_LIMIT
            )));
        }

        // 校验 2:所有 skill_id 在 candidate_skills 中
        let candidate_ids: std::collections::HashSet<&str> =
            candidate_skills.iter().map(|s| s.id.as_str()).collect();
        for node in &plan.nodes {
            if !candidate_ids.contains(node.skill_id.as_str()) {
                return Err(crate::llm::types::LlmError::Parse(format!(
                    "LLM returned illegal skill_id '{}' not in candidate_skills",
                    node.skill_id
                )));
            }
        }

        // 校验 3:SlotTemplateEngine::validate_dag
        SlotTemplateEngine::validate_dag(&plan).map_err(|e| {
            crate::llm::types::LlmError::Parse(format!("template validation failed: {}", e))
        })?;

        // 校验 4:DagPlan 内置校验
        plan.validate_edges().map_err(|e| {
            crate::llm::types::LlmError::Parse(format!("edge validation failed: {}", e))
        })?;
        plan.validate_loop_specs().map_err(|e| {
            crate::llm::types::LlmError::Parse(format!("loop spec validation failed: {}", e))
        })?;

        Ok(plan)
    }

    /// W8 Plan 2 Task 8:LLM 模型名 accessor(供 router_bridge 在
    /// `llm_decompose_called` 审计事件中记录 `llm_model` 字段,spec §6.1)。
    ///
    /// 不暴露 `api_key` / `base_url` 等敏感字段,仅暴露模型名。
    #[cfg(feature = "llm")]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// 构建 DAG 拆解的 system prompt(中文约束)。
    #[cfg(feature = "llm")]
    fn build_decompose_system_prompt(
        &self,
        skills: &[crate::skills::manifest::SkillManifest],
        user_slots: &[crate::llm::types::ExtractedSlot],
    ) -> String {
        let mut s = String::from(
            "你是 VoicePilot 的 DAG 拆解器,从用户语音转写文本中识别要执行的多步 Skill 编排。\n\n\
             候选 Skill 列表:\n",
        );
        for skill in skills {
            s.push_str(&format!(
                "- id: {}\n  title: {}\n  description: {}\n  intent_examples: {:?}\n  inputs: {:?}\n\n",
                skill.id, skill.title, skill.description, skill.intent_examples, skill.inputs.keys().collect::<Vec<_>>()
            ));
        }
        s.push_str(&format!("\n用户已填 Slot 列表:{:?}\n\n", user_slots));
        s.push_str(
            "约束:\n\
             1. 不得引用未在上述 candidate_skills 中的 skill_id\n\
             2. input_template.template 中所有 ${...} 必须指向合法 scope:\n\
                - ${prev.output.xxx} — 紧邻上游节点(拓扑序前驱)的 output.xxx\n\
                - ${n1.output.xxx} — 指定节点 n1 的 output.xxx\n\
                - ${user.xxx} — 用户审批阶段填的 Slot\n\
                - ${item} / ${item.xxx} — 循环变量(仅在循环节点内合法)\n\
             3. 循环节点必须填 max_iterations,默认 10,硬上限 50\n\
             4. max_total_steps 硬上限 20(防爆炸 DAG)\n\
             5. 若用户意图只需单 Skill,返回单节点 DAG(不强制多步)\n\
             6. edges 中 from / to 必须在 nodes 中存在\n\
             7. 不得编造未在 candidate_skills 中的工具或动作\n\n\
             示例(用户:\"打开记事本写 TODO 然后保存到桌面\"):\n\
             nodes: [\n  {node_id: \"n1\", skill_id: \"note.capture\", input_template: {kind: \"text\", template: \"{\\\"content\\\": \\\"TODO\\\", \\\"save_path\\\": \\\"C:/Users/test/Desktop/note.txt\\\"}\"}, risk_ceiling: \"E2\"},\n  {node_id: \"n2\", skill_id: \"files.move\", input_template: {kind: \"path\", template: \"${prev.output.save_path}\"}, risk_ceiling: \"E2\"}\n]\nedges: [{from: \"n1\", to: \"n2\"}]\nmax_total_steps: 5\n\
             \n注意:input_template.template 字段值必须是 JSON 字符串(对象序列化后),\n\
             调用方会先 SlotTemplateEngine::parse 解析占位符,再 serde_json::from_str 解析为对象。"
        );
        s
    }

    /// 构建 decompose_to_dag function calling schema(spec §2.2)。
    #[cfg(feature = "llm")]
    fn build_decompose_tool_schema(&self) -> serde_json::Value {
        json!([{
            "type": "function",
            "function": {
                "name": "decompose_to_dag",
                "description": "Decompose user voice transcription into a multi-step DAG plan",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "nodes": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "node_id": {"type": "string"},
                                    "skill_id": {"type": "string"},
                                    "input_template": {
                                        "type": "object",
                                        "properties": {
                                            "kind": {"type": "string", "enum": ["text","path","app","number","time_range","url","files"]},
                                            "template": {"type": "string"}
                                        },
                                        "required": ["kind", "template"]
                                    },
                                    "risk_ceiling": {"type": "string", "enum": ["E0","E1","E2","E3"]}
                                },
                                "required": ["node_id", "skill_id", "input_template", "risk_ceiling"]
                            }
                        },
                        "edges": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "from": {"type": "string"},
                                    "to": {"type": "string"},
                                    "port_binding": {"type": ["string", "null"]}
                                },
                                "required": ["from", "to"]
                            }
                        },
                        "loop_specs": {
                            "type": "object",
                            "additionalProperties": {
                                "type": "object",
                                "properties": {
                                    "loop_var": {"type": "string"},
                                    "iterable_source": {"type": "object"},
                                    "max_iterations": {"type": "integer", "maximum": 50},
                                    "break_condition": {"type": ["string", "null"]}
                                }
                            }
                        },
                        "max_total_steps": {"type": "integer", "maximum": 20}
                    },
                    "required": ["nodes", "edges", "max_total_steps"]
                }
            }
        }])
    }

    /// 解析 LLM `/chat/completions` 响应为 DagPlan(spec §2.2)。
    ///
    /// 期望响应结构(OpenAI 兼容):
    /// ```json
    /// {
    ///   "choices": [{
    ///     "message": {
    ///       "tool_calls": [{
    ///         "function": {
    ///           "name": "decompose_to_dag",
    ///           "arguments": "{\"nodes\":[...],\"edges\":[...],...}"
    ///         }
    ///       }]
    ///     }
    ///   }]
    /// }
    /// ```
    ///
    /// `arguments` 是 JSON 字符串(OpenAI 规范),需二次 `serde_json::from_str`。
    /// 本方法只解析结构,不做语义校验(校验在 `decompose_to_dag` 主体中)。
    #[cfg(feature = "llm")]
    fn parse_decompose_response(
        &self,
        resp: &serde_json::Value,
        _candidate_skills: &[crate::skills::manifest::SkillManifest],
    ) -> LlmResult<crate::skills::dag_types::DagPlan> {
        use crate::skills::dag_types::DagPlan;

        // Step 1: choices[0].message.tool_calls[0].function.arguments
        let arguments_str = resp
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("tool_calls"))
            .and_then(|t| t.get(0))
            .and_then(|t| t.get("function"))
            .and_then(|f| f.get("arguments"))
            .and_then(|a| a.as_str())
            .ok_or_else(|| crate::llm::types::LlmError::Parse(
                "response missing choices[0].message.tool_calls[0].function.arguments".into()
            ))?;

        // Step 2: 反序列化为 DagPlan(arguments 是 JSON 字符串)
        let plan: DagPlan = serde_json::from_str(arguments_str).map_err(|e| {
            crate::llm::types::LlmError::Parse(format!("failed to parse arguments as DagPlan: {}", e))
        })?;

        Ok(plan)
    }
```

- [ ] **Step 2: 跑 `cargo check --features llm`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features llm`
Expected: PASS(若有"unused import"提示,后续 Step 引用后消除;若 `LlmError::NotConfigured` / `LlmError::Timeout` 变体名与实际不符,读 `voicepilot/crates/trust-kernel/src/llm/types.rs` 调整)

注:若 `LlmError` 变体名不一致(如实际是 `LlmError::Timeout` vs `LlmError::TimeoutError`),读 `src/llm/types.rs` 修正。常见变体(参考 W7 实现):
- `NotConfigured` — api_key 空
- `Http(String)` — HTTP 错误
- `Parse(String)` — JSON 解析 / 校验失败
- `Timeout(std::time::Duration)` — 请求超时

- [ ] **Step 3: 写失败测试 — 创建 `tests/w8_plan2_llm_decompose.rs`,6 个 wiremock 场景**

```rust
//! W8 Plan 2 Task 8: LlmClient::decompose_to_dag wiremock 测试.
//!
//! 6 个场景(spec §2.2 + §5 错误处理):
//! 1. 成功 — LLM 返回合法 DAG,parse + 校验通过
//! 2. HTTP 失败 — 500 错误,期望 LlmError::Http
//! 3. 非法 skill_id — LLM 返回 DAG 含未在 candidate_skills 中的 skill_id,期望 LlmError::Parse
//! 4. 模板错误 — LLM 返回 DAG 含 ${prev.output.x} 但 prev 不存在,期望 LlmError::Parse(validate_dag 失败)
//! 5. 超时 — wiremock delay > client timeout,期望 LlmError::Timeout
//! 6. max_total_steps 超限 — LLM 返回 max_total_steps=100,期望 LlmError::Parse

#![cfg(feature = "llm")]

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::llm::types::LlmError;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan, MAX_TOTAL_STEPS_HARD_LIMIT};
use trust_kernel::skills::manifest::SkillManifest;
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_client(base_url: &str, timeout: Duration) -> LlmClient {
    // 假设 LlmClient::new(api_key, base_url, model, timeout) 签名(参考 W7 实际实现)
    // 若签名不同,按 W7 src/llm/client.rs 调整
    LlmClient::new(
        "test-key".into(),
        base_url.to_string(),
        "gpt-4o-mini".into(),
        timeout,
    )
}

fn candidate_skills() -> Vec<SkillManifest> {
    // 最小 manifest 列表(只填 id / title / description / intent_examples / inputs)
    // 实际 SkillManifest 字段较多,此处用 ..Default::default() 补齐(若 manifest 有 Default)
    // 若无 Default,需读 src/skills/manifest.rs 按实际构造器调用
    vec![
        SkillManifest {
            id: "task.explain".into(),
            title: "Explain task".into(),
            description: "Read audit log".into(),
            intent_examples: vec!["explain".into()],
            keywords: vec!["explain".into()],
            inputs: HashMap::from([
                ("limit".into(), trust_kernel::skills::manifest::SkillInput {
                    input_type: trust_kernel::skills::manifest::SkillInputType::Number,
                    required: false,
                    default: Some(serde_json::json!(10)),
                    ..Default::default()
                }),
            ]),
            ..Default::default()
        },
        SkillManifest {
            id: "note.capture".into(),
            title: "Capture note".into(),
            description: "Open Notepad and save content".into(),
            intent_examples: vec!["note".into(), "记事本".into()],
            keywords: vec!["note".into()],
            inputs: HashMap::new(),
            ..Default::default()
        },
    ]
}

fn ok_response_with_dag(plan: &DagPlan) -> serde_json::Value {
    let plan_json = serde_json::to_string(plan).unwrap();
    json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": plan_json
                    }
                }]
            }
        }]
    })
}

fn single_node_plan(skill_id: &str, max_steps: u32) -> DagPlan {
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: skill_id.into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("test".into()),
            },
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: max_steps,
    }
}

#[tokio::test]
async fn decompose_to_dag_succeeds_with_valid_dag() {
    let server = MockServer::start().await;
    let plan = single_node_plan("task.explain", 5);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri(), Duration::from_secs(30));
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain recent", &skills, &[]).await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let dag = result.unwrap();
    assert_eq!(dag.nodes.len(), 1);
    assert_eq!(dag.nodes[0].skill_id, "task.explain");
}

#[tokio::test]
async fn decompose_to_dag_returns_http_error_on_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
        .mount(&server)
        .await;

    let client = build_client(&server.uri(), Duration::from_secs(30));
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, LlmError::Http(_)), "expected Http, got {:?}", err);
}

#[tokio::test]
async fn decompose_to_dag_rejects_illegal_skill_id() {
    let server = MockServer::start().await;
    // LLM 返回 skill_id = "nonexistent.skill"(不在 candidate_skills)
    let plan = single_node_plan("nonexistent.skill", 5);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri(), Duration::from_secs(30));
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = format!("{:?}", err);
    assert!(matches!(err, LlmError::Parse(_)), "expected Parse, got {}", msg);
    assert!(msg.contains("illegal skill_id") || msg.contains("not in candidate_skills"),
        "got: {}", msg);
}

#[tokio::test]
async fn decompose_to_dag_rejects_max_total_steps_over_20() {
    let server = MockServer::start().await;
    // LLM 返回 max_total_steps = 100(硬上限 20)
    // 注:OpenAI function calling schema 写了 maximum: 20,但 LLM 可能不遵守 → 必须运行时校验
    let plan = single_node_plan("task.explain", MAX_TOTAL_STEPS_HARD_LIMIT + 80);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri(), Duration::from_secs(30));
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = format!("{:?}", err);
    assert!(matches!(err, LlmError::Parse(_)), "expected Parse, got {}", msg);
    assert!(msg.contains("max_total_steps") || msg.contains("hard limit"),
        "got: {}", msg);
}

#[tokio::test]
async fn decompose_to_dag_returns_timeout_on_slow_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&single_node_plan("task.explain", 5)))
            .set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;

    // client timeout = 1s,server delay = 5s → 超时
    let client = build_client(&server.uri(), Duration::from_secs(1));
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, LlmError::Timeout(_)), "expected Timeout, got {:?}", err);
}

#[tokio::test]
async fn decompose_to_dag_returns_not_configured_when_api_key_empty() {
    let server = MockServer::start().await;
    // 不 mount 任何 mock — is_enabled() 应在发送前返回 false
    let client = LlmClient::new(
        "".into(),  // 空 api_key
        server.uri(),
        "gpt-4o-mini".into(),
        Duration::from_secs(30),
    );
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, LlmError::NotConfigured), "expected NotConfigured, got {:?}", err);
}
```

- [ ] **Step 4: 跑 `cargo check --features llm` 确认测试编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features llm --tests`
Expected: PASS

若 `SkillManifest` / `SkillInput` / `SkillInputType` 的字段名与实际不符(如 `inputs` 实际是 `HashMap<String, SkillInput>` 还是 `Vec<SkillInput>`),读 `voicepilot/crates/trust-kernel/src/skills/manifest.rs` 调整 `candidate_skills()` 函数。

若 `LlmClient::new` 签名不同(如多了 `builder` / `config` 参数),读 `voicepilot/crates/trust-kernel/src/llm/client.rs` 顶部构造器调整 `build_client` 函数。

- [ ] **Step 5: 跑 wiremock 测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features llm --test w8_plan2_llm_decompose`
Expected: PASS(6 个测试全绿)

若 `decompose_to_dag_returns_timeout_on_slow_response` 偶发失败(wiremock delay 不稳定),把 client timeout 调到 100ms,server delay 调到 2s,增大差异。

- [ ] **Step 6: 跑 clippy(llm feature)**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features llm -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/llm/client.rs voicepilot/crates/trust-kernel/tests/w8_plan2_llm_decompose.rs
git commit -m "feat(w8p2): add LlmClient::decompose_to_dag with 4-layer validation + 6 wiremock test scenarios"
```

---

## Task 9: DAG 端到端集成测试(mock LLM + 真实 dispatcher + AutoApprover)

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w8_plan2_dag_e2e.rs`

**Spec §:** §3.1 DAG 拆解 + 执行流程 + §7.3 功能门禁

**背景:** Task 1-8 分别测试了各组件(Approver 扩展 / dispatcher / DagExecutor / 审计 / LLM 拆解)。本 Task 串联所有组件:用 wiremock 模拟 LLM 返回 DAG,然后用真实 DagExecutor + AutoApprover + 真实 dispatcher 执行,验证端到端流程。

**4 个集成测试:**
1. 单节点 DAG(note.capture 失败因 UIA 不可用 — 但 dispatcher 会路由到 executor,executor 失败 → DAG Failed)— 改用 `task.explain` 保证成功
2. 2 节点 DAG(n1 = task.explain 成功 + n2 = task.explain 成功)— 验证串行 + prev output 传递
3. LLM 拆解失败 → 调用方 catch Err,回退逻辑(本 Task 验证 Err 传播)
4. AutoDenier DAG 骨架审批 → 0 节点执行 + DagStatus::Cancelled

- [ ] **Step 1: 创建 `tests/w8_plan2_dag_e2e.rs`**

```rust
//! W8 Plan 2 Task 9: DAG 端到端集成测试.
//!
//! 串联 LlmClient::decompose_to_dag + DagExecutor + dispatch_skill_executor +
//! Approver + DagRepo + audit,验证 spec §3.1 完整流程。

#![cfg(feature = "llm")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan, DagStatus};
use trust_kernel::skills::manifest::SkillManifest;
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn candidate_skills() -> Vec<SkillManifest> {
    vec![
        SkillManifest {
            id: "task.explain".into(),
            title: "Explain".into(),
            description: "Read audit".into(),
            intent_examples: vec!["explain".into()],
            keywords: vec!["explain".into()],
            inputs: HashMap::new(),
            ..Default::default()
        },
        SkillManifest {
            id: "note.capture".into(),
            title: "Note".into(),
            description: "Notepad".into(),
            intent_examples: vec!["note".into()],
            keywords: vec!["note".into()],
            inputs: HashMap::new(),
            ..Default::default()
        },
    ]
}

fn llm_response_with_plan(plan: &DagPlan) -> serde_json::Value {
    let plan_json = serde_json::to_string(plan).unwrap();
    json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": plan_json
                    }
                }]
            }
        }]
    })
}

fn explain_node(id: &str, limit: u32) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            // dispatcher 期望对象,用 JSON 字符串字面量(LLM 返回格式)
            template: TemplateExpr::Literal(format!("{{\"limit\": {}}}", limit)),
        },
        risk_ceiling: ELevel::E0,
    }
}

#[tokio::test]
async fn e2e_single_node_dag_succeeds() {
    let server = MockServer::start().await;
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "explain recent".into(),
        nodes: vec![explain_node("n1", 5)],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(llm_response_with_plan(&plan)))
        .mount(&server)
        .await;

    let client = LlmClient::new("test-key".into(), server.uri(), "gpt-4o-mini".into(), Duration::from_secs(30));
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    // Step 1: LLM 拆解
    let dag = client.decompose_to_dag("explain recent", &candidate_skills(), &[]).await
        .expect("LLM decompose should succeed");

    // Step 2: DagExecutor 执行
    let result = executor.run(&dag).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    assert!(result.node_results.get("n1").unwrap().is_succeeded());
}

#[tokio::test]
async fn e2e_two_node_dag_succeeds_in_topo_order() {
    let server = MockServer::start().await;
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "explain 5 then explain 10".into(),
        nodes: vec![explain_node("n1", 5), explain_node("n2", 10)],
        edges: vec![DagEdge { from: "n1".into(), to: "n2".into(), port_binding: None }],
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(llm_response_with_plan(&plan)))
        .mount(&server)
        .await;

    let client = LlmClient::new("test-key".into(), server.uri(), "gpt-4o-mini".into(), Duration::from_secs(30));
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    let dag = client.decompose_to_dag("explain 5 then 10", &candidate_skills(), &[]).await.unwrap();
    let result = executor.run(&dag).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    assert_eq!(result.node_results.len(), 2);
    assert!(result.node_results.get("n1").unwrap().is_succeeded());
    assert!(result.node_results.get("n2").unwrap().is_succeeded());
}

#[tokio::test]
async fn e2e_deny_short_circuits_zero_node_execution() {
    let server = MockServer::start().await;
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "explain".into(),
        nodes: vec![explain_node("n1", 5)],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(llm_response_with_plan(&plan)))
        .mount(&server)
        .await;

    let client = LlmClient::new("test-key".into(), server.uri(), "gpt-4o-mini".into(), Duration::from_secs(30));
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    let dag = client.decompose_to_dag("explain", &candidate_skills(), &[]).await.unwrap();
    let result = executor.run(&dag).unwrap();
    assert_eq!(result.status, DagStatus::Cancelled);
    assert!(result.node_results.is_empty(), "Deny must short-circuit, got: {:?}", result.node_results);
}

#[tokio::test]
async fn e2e_llm_failure_propagates_err_to_caller() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
        .mount(&server)
        .await;

    let client = LlmClient::new("test-key".into(), server.uri(), "gpt-4o-mini".into(), Duration::from_secs(30));
    // LLM 拆解失败 → Err 传播给调用方(router_bridge Plan 4 会 catch 并回退)
    let result = client.decompose_to_dag("explain", &candidate_skills(), &[]).await;
    assert!(result.is_err(), "expected LLM failure to propagate as Err");
    // 调用方在 Plan 4 实现 catch + 回退逻辑,本测试仅验证 Err 传播
}
```

- [ ] **Step 2: 跑 `cargo check --features llm`**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features llm --tests`
Expected: PASS

- [ ] **Step 3: 跑 E2E 测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features llm --test w8_plan2_dag_e2e`
Expected: PASS(4 个测试全绿)

- [ ] **Step 4: 跑全部 W8 Plan 2 测试(无 feature / +llm)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_approver_dag_skeleton ; cargo test -p trust-kernel --test w8_plan2_dispatcher ; cargo test -p trust-kernel --test w8_plan2_dag_executor ; cargo test -p trust-kernel --test w8_plan2_audit_events`
Expected: PASS(4 + 10 + 11 + 8 = 33 个测试,无 LLM feature)

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features llm --test w8_plan2_llm_decompose ; cargo test -p trust-kernel --features llm --test w8_plan2_dag_e2e`
Expected: PASS(6 + 4 = 10 个测试,LLM feature)

总计:43 个测试全绿。

- [ ] **Step 5: 跑 clippy(全 feature)**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features voice,tauri,llm,uia -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w8_plan2_dag_e2e.rs
git commit -m "test(w8p2): add 4 DAG end-to-end integration tests (mock LLM + real dispatcher + AutoApprover)"
```

---

## Task 10: PROGRESS.md 更新 + 最终自检

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: 跑全部测试矩阵,记录通过数**

Run(在 `d:\voicepilot\voicepilot`):
```powershell
cargo test -p trust-kernel --test w8_plan2_approver_dag_skeleton
cargo test -p trust-kernel --test w8_plan2_dispatcher
cargo test -p trust-kernel --test w8_plan2_dag_executor
cargo test -p trust-kernel --test w8_plan2_audit_events
cargo test -p trust-kernel --features llm --test w8_plan2_llm_decompose
cargo test -p trust-kernel --features llm --test w8_plan2_dag_e2e
```
Expected: 全部 PASS,统计总测试数(预期 ≥ 43)

Run(回归测试):
```powershell
cargo test --workspace --features voice,tauri,llm,uia
```
Expected: W7 既有测试全 PASS(无回归),W8 Plan 2 新增测试 PASS

- [ ] **Step 2: 跑 6 套 feature 组合 cargo check 矩阵**

Run(在 `d:\voicepilot\voicepilot`):
```powershell
cargo check -p trust-kernel
cargo check -p trust-kernel --features llm
cargo check -p trust-kernel --features tauri
cargo check -p trust-kernel --features voice,tauri
cargo check -p trust-kernel --features voice,tauri,llm
cargo check -p trust-kernel --features voice,tauri,llm,uia
```
Expected: 6 套全 PASS

- [ ] **Step 3: 跑 clippy 矩阵**

Run:
```powershell
cargo clippy -p trust-kernel -- -D warnings
cargo clippy -p trust-kernel --features voice,tauri,llm,uia -- -D warnings
cargo clippy --workspace --features voice,tauri,llm,uia -- -D warnings
```
Expected: 0 warnings

- [ ] **Step 4: 更新 `docs/PROGRESS.md`**

在 PROGRESS.md 中找到 W8 章节(若不存在,在 W7 章节后追加),加 Plan 2 完成段落:

```markdown
## W8 Plan 2: LlmClient::decompose_to_dag + DagExecutor 简单节点 + dispatch_skill_executor 路由

**状态:** ✅ 已完成
**提交范围:** `feat(w8p2): ...` × 8 commits(Task 1-8 各 1 个 commit + Task 9 测试 commit + Task 10 docs commit)
**测试统计:** 43 个新增测试全 PASS
- `w8_plan2_approver_dag_skeleton` — 4 个(Approver trait 扩展)
- `w8_plan2_dispatcher` — 10 个(9 路 skill_id + 未知 / form.submit 占位)
- `w8_plan2_dag_executor` — 11 个(topological_sort 9 + run_simple_node 3 + Deny 短路 3 + PartiallySucceeded 5)
- `w8_plan2_audit_events` — 8 个(6 个 event_type + 哈希链完整性)
- `w8_plan2_llm_decompose` — 6 个(wiremock 6 场景,需 --features llm)
- `w8_plan2_dag_e2e` — 4 个(端到端集成,需 --features llm)

**已实现:**
- Approver trait 扩展 `approve_dag_skeleton(&DagPlan) -> Result<ApprovalDecision>`(AutoApprover / AutoDenier / TauriApprover stub 三实现)
- `dispatch_skill_executor` 9 路 skill_id 路由 + `DispatchOutcome` 适配器(统一 8 个 W7 executor 异构返回类型)
- `DagExecutor::run` 主入口 + Kahn 拓扑排序 + 简单节点串行执行 + 失败短路 + PartiallySucceeded 分支
- 6 个 DAG 审计事件(dag_plan_created / dag_skeleton_approved / dag_node_started / dag_node_succeeded / dag_node_failed / dag_completed)+ W1 哈希链保持
- `LlmClient::decompose_to_dag` 方法(`#[cfg(feature = "llm")]` 门控)+ 4 层校验(max_total_steps / skill_id / template / edges)+ wiremock 6 场景测试

**已知偏离(本 Plan 范围内):**
- `form.submit` 路由占位返回 Err("not implemented in Plan 2; see Plan 3"),Plan 3 实现
- `task.explain` 路由到 W7 既有 `execute_explain`,Plan 3 改为 `execute_task_explain_with_llm`
- 循环节点(`run_loop_node`)返回 Err,Plan 3 实现
- TauriApprover::approve_dag_skeleton 是 stub(返回 Deny),Plan 5 实现真实 IPC 弹窗

**Spec 覆盖:**
- §2.2 LlmClient::decompose_to_dag — ✅ Task 8
- §2.3 DagExecutor + dispatch_skill_executor + Approver 扩展 — ✅ Task 1-7
- §6.1 DAG 6 个审计事件 — ✅ Task 7

**下一步:** W8 Plan 3(DagExecutor 循环节点 + form.submit 新 Skill + task.explain LLM 增强)
```

- [ ] **Step 5: 跑 spec 覆盖自检**

读 spec `docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md` §2.2 + §2.3 + §6.1,确认每项均有对应 Task:

| Spec 项 | 对应 Task | 状态 |
|---|---|---|
| §2.2 LlmClient::decompose_to_dag 签名 | Task 8 | ✅ |
| §2.2 function calling schema | Task 8 Step 1 `build_decompose_tool_schema` | ✅ |
| §2.2 system prompt 中文约束 | Task 8 Step 1 `build_decompose_system_prompt` | ✅ |
| §2.2 错误处理(HTTP / 校验 / 超时) | Task 8 Step 3 wiremock 6 场景 | ✅ |
| §2.2 privacy(is_enabled / privacy_mode) | Task 8 Step 1 `is_enabled()` 检查 + Step 3 `decompose_to_dag_returns_not_configured_when_api_key_empty` | ✅ |
| §2.2 校验双层防御 Layer 1 | Task 8 Step 1 校验 1-4 | ✅ |
| §2.3 DagPlan / DagNode 数据结构 | Plan 1 已实现,本 Plan 引用 | ✅ |
| §2.3 DagExecutor::run 算法 | Task 3 Step 1 `run()` | ✅ |
| §2.3 topological_sort(Kahn) | Task 3 Step 1 `topological_sort` + 9 个测试 | ✅ |
| §2.3 run_simple_node | Task 4 Step 1 | ✅ |
| §2.3 dispatch_skill_executor 9 路 | Task 2 Step 1 | ✅ |
| §2.3 DAG 骨架审批(决策 #2) | Task 1 + Task 5 | ✅ |
| §2.3 失败处理(决策 #4 + #8) | Task 6 | ✅ |
| §6.1 dag_plan_created | Task 7 Step 1 + 测试 | ✅ |
| §6.1 dag_skeleton_approved | Task 7 Step 1 + 测试 | ✅ |
| §6.1 dag_node_started | Task 7 Step 1 + 测试 | ✅ |
| §6.1 dag_node_succeeded | Task 7 Step 1 + 测试 | ✅ |
| §6.1 dag_node_failed | Task 7 Step 1 + 测试 | ✅ |
| §6.1 dag_completed | Task 7 Step 1 + 测试 | ✅ |

- [ ] **Step 6: 跑 placeholder 扫描**

搜索 plan 文档中的 placeholder 关键词(`TBD` / `TODO` / `fill in` / `implement later`),确认全部为"已知偏离"标注(Plan 3 范围),非未完成 placeholder:

Run(在 `d:\voicepilot`):
```powershell
Select-String -Path "docs\superpowers\plans\2026-07-26-w8-plan2-llm-decompose-dag-executor.md" -Pattern "TBD|TODO|fill in|implement later" | Select-Object LineNumber, Line
```
Expected: 仅有的 `TODO` / `Plan 3` 出现在以下合法上下文:
- `form.submit not implemented in Plan 2; see Plan 3`(Task 2 dispatcher 占位)
- `loop node '{}' not supported in Plan 2; see Plan 3`(Task 3 + Task 4 dag_executor)
- `Plan 5 TODO: 通过 app_handle 触发 DagApprovalDialog.vue`(Task 1 TauriApprover stub)
- `execute_task_explain_with_llm`(Plan 3 范围,本 plan 不实现)

无未完成 placeholder。

- [ ] **Step 7: 类型一致性检查**

读 plan 文档,确认各 Task 引用的类型 / 方法签名一致:

| 类型 / 方法 | 定义 Task | 使用 Task | 一致性 |
|---|---|---|---|
| `Approver::approve_dag_skeleton(&DagPlan) -> Result<ApprovalDecision>` | Task 1 | Task 3, 5, 7 | ✅ |
| `dispatch_skill_executor(skill_id, kernel, resolved_input, approver, task_id, step_id) -> Result<DispatchOutcome>` | Task 2 | Task 4, 9 | ✅ |
| `DispatchOutcome { task_id, step_id, output, succeeded, error_cause }` | Task 2 | Task 4 | ✅ |
| `DagExecutor::new(kernel, approver, dag_repo)` | Task 3 | Task 5, 6, 7, 9 | ✅ |
| `DagExecutor::run(&DagPlan) -> Result<DagResult>` | Task 3 | Task 5, 6, 7, 9 | ✅ |
| `topological_sort(&[DagNode], &[DagEdge]) -> Result<Vec<String>>` | Task 3 | Task 3, 7 | ✅ |
| `DagNodeStatus::Succeeded(serde_json::Value)` / `Failed { cause }` | Plan 1(类型定义) | Task 3, 4, 6 | ✅(Plan 1 已定义) |
| `DagStatus::Failed { failed_node, cause }` / `PartiallySucceeded { succeeded, failed_node, cause }` | Plan 1(类型定义) | Task 3, 6 | ✅(Plan 1 已定义) |
| `LlmClient::decompose_to_dag(user_text, candidate_skills, user_slots) -> LlmResult<DagPlan>` | Task 8 | Task 9 | ✅ |
| `LlmError::NotConfigured` / `Http(_)` / `Parse(_)` / `Timeout(_)` | W7 既有 | Task 8 | ✅(读 src/llm/types.rs 确认变体名) |

- [ ] **Step 8: Commit**

```powershell
git add docs/PROGRESS.md
git commit -m "docs(w8p2): update PROGRESS.md with W8 Plan 2 completion status + 43 test statistics"
```

---

## Self-Review

### 1. Spec 覆盖

**§2.2 LlmClient::decompose_to_dag** — ✅ Task 8 完整覆盖:
- 方法签名匹配 spec §2.2 `pub async fn decompose_to_dag(&self, user_text, candidate_skills, user_slots) -> Result<DagPlan>`
- function calling schema 在 `build_decompose_tool_schema` 中定义(nodes / edges / loop_specs / max_total_steps)
- system prompt 中文约束在 `build_decompose_system_prompt` 中实现(7 条约束)
- 错误处理 6 场景由 wiremock 测试覆盖
- privacy 由 `is_enabled()` 检查 + `privacy_mode` 调用方检查(spec §2.2 隐私注解)
- 双层防御 Layer 1 由 4 层校验覆盖

**§2.3 DagExecutor + dispatch_skill_executor + Approver 扩展** — ✅ Task 1-7 完整覆盖:
- Approver trait 扩展(Task 1)
- dispatch_skill_executor 9 路路由(Task 2,form.submit 占位属已知偏离 Plan 3)
- DagExecutor::run 主入口 + 拓扑排序(Task 3)
- run_simple_node(Task 4)
- DAG 骨架审批 Deny 短路(Task 5)
- 失败处理 + PartiallySucceeded(Task 6)
- 6 个审计事件(Task 7)
- 循环节点 run_loop_node 属已知偏离 Plan 3

**§6.1 6 个审计事件** — ✅ Task 7 完整覆盖,每个 event_type 至少 1 个断言测试。

**未覆盖项(Plan 3+ 范围,非本 Plan 遗漏):**
- §2.4 form.submit 新 Skill — Plan 3
- §2.5 task.explain LLM 增强 — Plan 3
- §2.7 UI 扩展 — Plan 5
- §2.8 Router Bridge 集成 — Plan 4

### 2. Placeholder 扫描

✅ 通过(见 Task 10 Step 6)。所有 `TODO` / `Plan 3` 出现均在已知偏离上下文:
- `form.submit not implemented in Plan 2; see Plan 3` — dispatcher 占位
- `loop node '{}' not supported in Plan 2; see Plan 3` — DagExecutor 拒绝循环节点
- `Plan 5 TODO: 通过 app_handle 触发 DagApprovalDialog.vue` — TauriApprover stub
- `execute_task_explain_with_llm` — Plan 3 范围

无未完成 placeholder,无"add appropriate error handling"等模糊描述。

### 3. 类型一致性

✅ 通过(见 Task 10 Step 7)。关键类型 / 方法签名在定义 Task 和使用 Task 间一致:
- `Approver::approve_dag_skeleton` 签名一致(Task 1 定义,Task 3/5/7 使用)
- `dispatch_skill_executor` 签名一致(Task 2 定义,Task 4/9 使用)
- `DagExecutor::new` / `run` 签名一致(Task 3 定义,Task 5/6/7/9 使用)
- `LlmClient::decompose_to_dag` 签名一致(Task 8 定义,Task 9 使用)
- `DagStatus` / `DagNodeStatus` 变体名引用 Plan 1 定义(本 Plan 不重复定义)

### 4. 已知偏离(显式记录,非缺陷)

1. **dispatch_skill_executor 返回 `DispatchOutcome` 而非 spec §2.3 的 `SkillExecution`**:W7 既有 8 个 executor 返回类型异构(`SkillExecution` vs `String` task_id),强行统一需重构 7 个 executor(scope creep)。`DispatchOutcome` 适配器统一两类型,`output: serde_json::Value` 供下游模板 `${prev.output.xxx}` 解析。spec §2.3 的 `SkillExecution` 仍由 `FilesOrganizeSkill` 内部返回,`DispatchOutcome::from_skill_execution` 适配。

2. **`form.submit` 路由占位返回 Err**:Plan 3 实现 `execute_form_submit`,本 Plan 在 dispatcher 中返回 `Err("not implemented in Plan 2; see Plan 3")`,Task 2 测试 `dispatch_form_submit_returns_not_implemented_err` 验证。

3. **`task.explain` 路由到 W7 既有 `execute_explain`**:Plan 3 改为 `execute_task_explain_with_llm`(LLM 失败归因),本 Plan 不实现。

4. **循环节点 `run_loop_node` 不实现**:Plan 3 范围。本 Plan 在 `DagExecutor::run` 中检测 `loop_specs.contains_key(node_id)` 返回 Err,Task 3 测试隐式覆盖(`loop_specs` 默认空,不触发)。

5. **TauriApprover::approve_dag_skeleton 是 stub**:返回 `Ok(Deny)`(W6 既有 `prompt` 用 oneshot channel + 5min timeout 默认 Deny)。Plan 5 实现真实 DAG 骨架弹窗(DagApprovalDialog.vue)。

6. **`run_simple_node` 中 `user_slots` 传空 `&[]`**:Plan 5 实现 UI 时,DagExecutor::run 签名扩展为 `run(&self, plan: &DagPlan, user_slots: &[ExtractedSlot])`。本 Plan 模板中 `${user.xxx}` 会解析失败 → 节点 Failed,符合 spec §2.1 双层防御 Layer 2(执行前再次校验)。

7. **`dag_node_succeeded` 审计事件 `evidence_strength` 硬编码 "weak"**:W8 简化,实际由 executor ToolResult 决定。Plan 3+ 实现 DispatchOutcome 携带 evidence_strength 字段时,Task 7 审计代码同步更新。

### 5. 风险评估

| 风险 | 缓解 |
|---|---|
| W7 既有 executor input struct 字段名与本 Plan 不一致 | Task 2 Step 3 显式提示读源文件调整;`extract_*` helper 集中处理 JSON 提取,字段名变更只改 helper |
| `LlmError` 变体名与实际不符 | Task 8 Step 2 显式提示读 `src/llm/types.rs` 调整 |
| `LlmClient::new` 构造器签名与实际不符 | Task 8 Step 3 显式提示读 `src/llm/client.rs` 顶部构造器调整 `build_client` |
| wiremock timeout 测试偶发失败 | Task 8 Step 5 提供调优建议(client timeout 100ms,server delay 2s) |
| `SlotTemplateEngine::resolve` 签名与 Plan 1 实际不符 | Task 4 Step 1 调用 `SlotTemplateEngine::resolve(&TemplateExpr, &HashMap, &[ExtractedSlot], Option<&str>, Option<&str>)`,若 Plan 1 实际签名不同需调整 |
| `DagRepo` 方法名与 Plan 1 实际不符 | Task 4 Step 1 调用 `create_plan` / `update_plan_status` / `create_node` / `update_node_status` / `get_plan` / `list_nodes_by_plan`,若 Plan 1 实际方法名不同需调整 |

### 6. 完成判定

- [x] 10 个 Task 全部含完整代码片段(无 placeholder)
- [x] 每个 Task 含 TDD 步骤(写测试 → 跑红 → 实现 → 跑绿 → commit)
- [x] 每个 Task 含 `cargo check` / `cargo test` / `cargo clippy` 验证命令
- [x] 6 套 feature 组合 cargo check 矩阵(Task 10 Step 2)
- [x] spec §2.2 + §2.3 + §6.1 全覆盖(Task 10 Step 5 spec 覆盖表)
- [x] 类型一致性(Task 10 Step 7)
- [x] 已知偏离显式记录(Self-Review §4)
- [x] 风险评估 + 缓解(Self-Review §5)
- [x] PROGRESS.md 更新(Task 10 Step 4)

**Plan 2 完成。** 可进入 Plan 3(DagExecutor 循环节点 + form.submit 新 Skill + task.explain LLM 增强)。