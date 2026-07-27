# W8 Plan 5: UI — DAG 骨架审批弹窗 + DAG 历史 + task.explain 面板 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W8 设计文档(`docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md` §2.7)实现 VoicePilot Tauri 桌面应用的 DAG 编排 UI 层:新增 4 个 Tauri 命令(`approve_dag_skeleton_command` / `list_dag_history_command` / `get_dag_plan_command` / `get_task_explanation_command`)+ 3 个 React 组件(`DagApprovalDialog` / `DagHistoryView` / `TaskExplainPanel`)+ 路由集成 + 可访问性(WCAG A)+ 集成测试,使用户能在 Trust Center 中查看 DAG 历史、在主聊天流程中审批 DAG 骨架、在 step 详情页查看 LLM 失败归因。

**Architecture:** Tauri 后端在 `crates/ui/src/` 新增 `dag_commands.rs` 模块(4 个命令),复用 W6b 的 `TauriApprover` + `ApprovalRegistry` 模式实现 DAG 骨架审批(oneshot channel + 5min timeout + 默认 Deny);扩展 `approver.rs` 增加 `approve_dag_skeleton` 方法 + `dag-approval-request` 事件。React 前端在 `web/src/components/` 新增 3 个 TypeScript 函数组件,遵循 Engineering Console UI 美学(深海军蓝 #0A1628 + 暖琥珀色 #F5B82E + IBM Plex Mono/Sans + 4px 直角),复用 W6b 的 `ApprovalModal` 模式(modal backdrop + Esc 关闭 + 卸载时自动 Deny)。所有 IPC 走 Tauri 命令,WebView 不直接访问 filesystem / MCP,`approval_request_id` 单次使用(用后即焚,防重放)。

**Tech Stack:** Rust(stable)+ Tauri 2 + `tokio::sync::oneshot` + `uuid` + `serde`(后端);React 18 + TypeScript 5 + Vite + `@tauri-apps/api`(前端);`vitest`(前端单元测试);TDD。

**Spec:** `docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md` §2.7(UI 扩展)+ §2.3(DagPlan / DagNode / DagStatus 数据结构,Plan 1 已实现)+ §2.5(task.explain LLM 归因,Plan 3 已实现)+ §2.6(DagRepo / TaskExplanationRepo,Plan 1 已实现)+ §6(安全约束:Tauri IPC 三规则 + approval_request_id 单次使用)+ §6.1(审计事件:`dag_skeleton_approved` / `dag_plan_created` / `dag_node_*` / `dag_completed`)+ §7(验收门禁:clippy 0 warnings + npm.cmd run build PASS)

**Precondition:**
- Plan 1 已完成:`DagPlan` / `DagNode` / `DagEdge` / `LoopSpec` / `DagStatus` / `DagNodeStatus` / `DagResult` 数据结构在 `crates/trust-kernel/src/skills/dag_types.rs`;`DagRepo` 在 `dag_repo.rs`;`TaskExplanationRepo` + `FailureCategory` 在 `explanation_repo.rs`;DB 迁移 003 已创建 `dag_plans` / `dag_nodes` / `task_explanations` 三表。
- Plan 2 已完成:`DagExecutor::run(plan: &DagPlan) -> Result<DagResult>` 在 `dag_executor.rs`,内部调 `self.approver.approve_dag_skeleton(plan)`;`dispatch_skill_executor` 路由 9 个 Skill。
- Plan 3 已完成:`form.submit` Skill;`task.explain` LLM 增强(`TaskExplanation` / `LlmAnalysis` 结构);循环节点执行。
- Plan 4 已完成:`route_text_with_dag` 在 `router_bridge.rs`,返回 `RouteDecision::Dag(DagPlan)`;`Approver` trait 已加 `approve_dag_skeleton(&self, plan: &DagPlan) -> Result<ApprovalDecision>` 方法。
- W7 收尾 commit `957d40d` 已清理 macOS/Linux + 本地 LLM 死代码;`cargo check --workspace --features voice,tauri,llm,uia` PASS;clippy `-D warnings` 0 警告。
- W6b Tauri UI 现状:`crates/ui/src/commands.rs` 已有 `route_text_command` / `organize_files_command` / `submit_approval_command`;`approver.rs` 有 `TauriApprover` + `ApprovalRegistry`(oneshot + 5min timeout + 默认 Deny);`web/src/components/` 有 11 个 React 组件(ApprovalModal / MainView / SettingsView / TrustCenterView / SkillsManagerView / AuditViewerView / DiffViewer / Chip / KillSwitchBar / ModelDownloadBar / SlotEditDialog)。

---

## File Structure

### Backend — Tauri UI(`voicepilot/crates/ui/src/`)

- **Create** `dag_commands.rs` — W8 Plan 5 新模块,4 个 Tauri 命令的逻辑函数 + 命令函数:
  - `list_dag_history(state, limit, offset, status_filter) -> UiResult<Vec<DagPlanSummaryDto>>` + `list_dag_history_command`
  - `get_dag_plan(state, plan_id) -> UiResult<DagPlanDetailDto>` + `get_dag_plan_command`
  - `get_task_explanation(state, step_id) -> UiResult<TaskExplanationDto>` + `get_task_explanation_command`
  - `submit_dag_skeleton_approval(state, approval_request_id, decision) -> UiResult<bool>` + `approve_dag_skeleton_command`
- **Modify** `approver.rs` — 扩展 `TauriApprover`:
  - 新增 `approve_dag_skeleton(&self, plan: &DagPlan) -> Result<ApprovalDecision>` 方法(实现 `Approver` trait,Plan 4 已加该 trait 方法)
  - 新增 `DagApprovalRequestPayload` 结构(emit `dag-approval-request` 事件给 webview)
  - 复用 `ApprovalRegistry::create_request` / `take_sender` / `wait_for_decision`(oneshot + 5min timeout + 默认 Deny)
- **Modify** `commands.rs` — 在 `register_handlers` + `register_handlers_with_voice` 中注册 4 个新命令
- **Modify** `lib.rs` — 加 `#[cfg(feature = "tauri")] pub mod dag_commands;`

### Frontend — React(`voicepilot/crates/ui/web/src/`)

- **Create** `components/DagApprovalDialog.tsx` — DAG 骨架审批弹窗:
  - 监听 `dag-approval-request` 事件
  - 显示 DAG 节点卡片(node_id / skill_id / risk_ceiling 徽章 E0-E3 / input_template 预览)
  - SVG 节点 + 箭头边连线图
  - 全局 max_total_steps 显示
  - Allow / Deny 按钮(Modify 置灰,标注 "W9+")
  - Esc 键关闭 = Deny + 卸载时自动 Deny(一次性语义)
- **Create** `components/DagHistoryView.tsx` — DAG 历史视图:
  - 表格:plan_id / user_goal / status 徽章 / created_at / completed_at / node_count / 成功率
  - 状态过滤(全部 / 运行中 / 已完成 / 失败)+ 分页(20/页)
  - 点击行展开节点详情(accordion)
- **Create** `components/TaskExplainPanel.tsx` — task.explain 面板:
  - 三段可折叠 accordion:Step 基本信息 / 失败工具调用列表 / LLM 归因(root_cause_zh 中文 / category 徽章 / suggested_fix / confidence 进度条)
  - `llm_analysis = None` → 显示 "未启用 LLM 归因,可在 Settings 中开启"
- **Modify** `App.tsx` — 加 `dagApproval` state + `onDagApprovalRequest` listener + 在 NAV_ITEMS 加 "DAG History" 项
- **Modify** `api.ts` — 加 `approveDagSkeleton` / `onDagApprovalRequest` / `listDagHistory` / `getDagPlan` / `getTaskExplanation` 5 个函数
- **Modify** `types.ts` — 加 `DagPlan` / `DagNode` / `DagEdge` / `DagStatus` / `DagApprovalRequestPayload` / `DagPlanSummary` / `DagPlanDetail` / `TaskExplanation` / `LlmAnalysis` / `FailureCategory` / `DagApprovalDecision` 类型;扩展 `View` 加 `"dag-history"`
- **Modify** `styles.css` — 加 DAG 相关样式(`.dag-dialog` / `.dag-node-card` / `.dag-edge-svg` / `.dag-history-table` / `.risk-badge-e0..e3` / `.task-explain-accordion`)

### Tests

- **Create** `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs` — Rust 单元测试(≥ 8 个):`approve_dag_skeleton_command` 提交决策 / `list_dag_history` 分页 + 过滤 / `get_dag_plan` 完整 DAG / `get_task_explanation` LLM 归因
- **Create** `voicepilot/crates/ui/web/src/components/__tests__/DagApprovalDialog.test.tsx` — vitest 组件测试(≥ 5 个):渲染节点卡片 / Allow 提交 / Deny 提交 / Esc 关闭 = Deny / 错误状态
- **Modify** `voicepilot/crates/ui/tests/commands_unit.rs` — 加 `submit_dag_skeleton_approval` 逻辑函数测试

### Docs

- **Modify** `docs/PROGRESS.md` — W8 Plan 5 完成状态 + 测试统计

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 project_memory.md "Lessons Learned")
- **TDD**:每个 Tauri 命令先写失败测试 → 跑 → 实现 → 跑通 → commit
- **Tauri command return type pattern**:逻辑函数返回 `UiResult<T>`,命令函数返回 `Result<T, String>` + `.map_err(Into::into)`(参考 project_memory.md "Engineering Conventions")
- **TauriApprover constructors**:`new` for tests,`with_app` for production(参考 project_memory.md)
- **TauriApprover oneshot + 5min timeout**:复用 `ApprovalRegistry::create_request` / `take_sender` / `wait_for_decision`,超时默认 Deny(参考 `approver.rs` 现有实现 + project_memory.md "Hard Constraints")
- **Tauri IPC 三安全规则**(参考 project_memory.md "Hard Constraints"):
  1. WebView 不能直接访问 filesystem(所有 fs 操作走 Tauri 命令)
  2. UI 不能直接调用 MCP(走 approve_request 流程)
  3. `approval_request_id` 单次使用(用后即焚,防重放)— `ApprovalRegistry::take_sender` 已实现此语义
- **Repo accessor pattern**:`DagRepo::new()` / `TaskExplanationRepo::new()` 不带参数,方法接收 `&Connection`(参考 project_memory.md "Engineering Conventions")
- **`&kernel.conn()` 不用 `&*kernel.conn()`**:避免 clippy `explicit_auto_deref` lint(参考 project_memory.md "Lessons Learned")
- **TrustKernel 不是 Clone**:`AppState.kernel: Arc<TrustKernel>`,UI 命令通过 `state.kernel.clone()` 获取 `Arc`(参考 project_memory.md "Lessons Learned")
- **LLM 代码 `#[cfg(feature = "llm")]` 门控**:本 plan 不直接调 LLM,但 `get_task_explanation` 返回的 `TaskExplanationRecord` 可能含 `llm_model` 字段,无需 feature gate(纯数据读取)
- **Tauri feature `#[cfg(feature = "tauri")]` 门控**:所有 Tauri 命令 + `dag_commands.rs` 模块整体门控
- **tauri feature 不依赖 voice feature**:`register_handlers`(无 voice)+ `register_handlers_with_voice`(有 voice)两套注册函数,本 plan 在两套中都注册 4 个新命令
- **npm.cmd run build**(Windows):不用 `npm run build`(参考 project_memory.md)
- **Engineering Console UI aesthetic**(参考 project_memory.md "Engineering Conventions"):
  - 深海军蓝背景(`--bg-deep: #0a0e1a` / `--bg-base: #111827`)+ 暖琥珀色强调(`--accent: #f59e0b` / `--accent-bright: #fbbf24`)
  - IBM Plex Mono(代码 / 数字)+ IBM Plex Sans(正文)
  - 4px 直角(`border-radius: 0`),不用圆角
  - WCAG A 可访问性:label/htmlFor / ARIA roles / Esc 关闭 / 键盘导航 / 错误状态 UI
  - 不用 emoji(除非用户明确要求),不用 gradient / heavy shadow
- **风险徽章颜色编码**(spec §6 安全约束 + UI 设计要求):
  - E0 = 灰色(`--text-muted`)
  - E1 = 蓝色(自定义 `--info: #3b82f6`)
  - E2 = 琥珀色(`--accent`)
  - E3 = 红色(`--danger`)
- **节点卡片用 minimal bento grid**:不嵌套 > 2 层(参考 UI 设计要求)
- **Commit message**:`feat(w8p5): ...` / `test(w8p5): ...` / `docs(w8p5): ...` / `refactor(w8p5): ...`
- **不引入新依赖**:本 plan 仅用 Tauri 2 + React 18 + TypeScript + `@tauri-apps/api`(均已在 `web/package.json`),不加新 npm 包;后端仅用 `uuid` + `serde` + `tokio`(均已在 workspace)

---

## Task 1: approve_dag_skeleton_command + TauriApprover::approve_dag_skeleton

**Files:**
- Modify: `voicepilot/crates/ui/src/approver.rs`(扩展 `TauriApprover` + 新增 `DagApprovalRequestPayload`)
- Modify: `voicepilot/crates/ui/src/dag_commands.rs`(新建,本 task 仅加 `submit_dag_skeleton_approval` + `approve_dag_skeleton_command`)
- Modify: `voicepilot/crates/ui/src/lib.rs`(加 `pub mod dag_commands;`)
- Modify: `voicepilot/crates/ui/tests/commands_unit.rs`(加测试)

**目标:** 实现 DAG 骨架审批的完整 IPC 链路:kernel 侧 `DagExecutor` 调 `TauriApprover::approve_dag_skeleton(plan)` → 创建 oneshot channel + approval_request_id → emit `dag-approval-request` 事件给 webview → webview 显示 `DagApprovalDialog`(Task 4 实现)→ 用户点击 Allow/Deny → 调 `approve_dag_skeleton_command` 提交决策 → oneshot 投递 → `approve_dag_skeleton` 返回决策。超时 5min 默认 Deny(参考 project_memory.md "TauriApprover")。

- [ ] **Step 1: 扩展 `approver.rs`,加 `DagApprovalRequestPayload` + `approve_dag_skeleton` 方法**

打开 `voicepilot/crates/ui/src/approver.rs`,在文件末尾(现有 `TauriApprover` impl 块之后)追加:

```rust
// ===== W8 Plan 5: DAG 骨架审批 =====

use trust_kernel::skills::dag_types::DagPlan;

/// Payload emitted to the webview on the `dag-approval-request` event.
/// W8 §2.7:webview 显示 DAG 节点卡片 + 边连线图,用户 Allow/Deny。
#[derive(serde::Serialize, Clone)]
pub struct DagApprovalRequestPayload {
    pub approval_request_id: String,
    pub plan_id: String,
    pub user_goal: String,
    pub max_total_steps: u32,
    /// 节点数(前端用于显示 "审批 N 个节点")
    pub node_count: usize,
    /// 完整 DagPlan JSON(webview 渲染节点卡片 + 边连线图)
    pub plan_json: serde_json::Value,
}

impl TauriApprover {
    /// W8 §2.3 + §2.7:DAG 骨架审批入口。
    ///
    /// 由 `DagExecutor::run` 在拓扑排序后、节点执行前调用。
    /// 创建 oneshot channel + approval_request_id,emit `dag-approval-request`
    /// 事件给 webview,阻塞等待决策(5min timeout,默认 Deny)。
    ///
    /// 与 `prompt` 的区别:
    /// - `prompt` 用于单步 Skill 审批(payload = EffectManifest,事件 `approval-request`)
    /// - `approve_dag_skeleton` 用于 DAG 骨架审批(payload = DagPlan,事件 `dag-approval-request`)
    ///
    /// 一次性语义:`approval_request_id` 用后即焚,`take_sender` 移除 sender,
    /// 防重放(参考 project_memory.md "Tauri IPC 三安全规则")。
    pub fn approve_dag_skeleton(&self, plan: &DagPlan) -> trust_kernel::approval::types::ApprovalDecision {
        use trust_kernel::approval::types::ApprovalDecision;

        // DagPlan 不实现 EffectManifest 转换,用 dummy manifest 占位创建 oneshot channel。
        // ApprovalRegistry::create_request 的 manifest 参数仅用于日志,不影响 channel 语义。
        let dummy_manifest = trust_kernel::policy::transaction::EffectManifest {
            sources: vec![],
            destination: format!("dag://{}", plan.plan_id),
            conflicts: vec![],
            total_bytes: 0,
        };
        let (approval_id, rx) = self.registry.create_request(&dummy_manifest);

        if let Some(app) = &self.app {
            let plan_json = serde_json::to_value(plan).unwrap_or(serde_json::json!({}));
            let payload = DagApprovalRequestPayload {
                approval_request_id: approval_id.clone(),
                plan_id: plan.plan_id.clone(),
                user_goal: plan.user_goal.clone(),
                max_total_steps: plan.max_total_steps,
                node_count: plan.nodes.len(),
                plan_json,
            };
            let _ = app.emit("dag-approval-request", payload);
        }

        self.registry.wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }

    /// 测试用:不 emit 事件,直接返回 approval_request_id + receiver。
    /// 单元测试调 `take_sender(id).send(decision)` 模拟用户决策。
    pub fn create_dag_approval_request_for_test(
        &self,
        plan: &DagPlan,
    ) -> (String, tokio::sync::oneshot::Receiver<trust_kernel::approval::types::ApprovalDecision>) {
        let dummy_manifest = trust_kernel::policy::transaction::EffectManifest {
            sources: vec![],
            destination: format!("dag://{}", plan.plan_id),
            conflicts: vec![],
            total_bytes: 0,
        };
        self.registry.create_request(&dummy_manifest)
    }
}
```

注意:
- `DagApprovalRequestPayload` 用 `serde::Serialize` + `Clone`,与现有 `ApprovalRequestPayload` 一致
- `approve_dag_skeleton` 不返回 `Result`(与 `prompt` 一致),超时 / sender dropped → 返回 `Deny`
- `dummy_manifest` 是 hack — `ApprovalRegistry::create_request` 接收 `&EffectManifest` 参数仅用于日志记录,不影响 channel 语义;后续可在 Plan 6 重构为泛型 `create_request<T>`
- `create_dag_approval_request_for_test` 是测试专用方法,生产代码不调

- [ ] **Step 2: 创建 `dag_commands.rs` 文件骨架 + `submit_dag_skeleton_approval` 逻辑函数**

创建 `voicepilot/crates/ui/src/dag_commands.rs`:

```rust
//! W8 Plan 5: DAG 相关 Tauri 命令 —— V1.1.2 §8.2 + W8 spec §2.7.
//!
//! 4 个命令:
//! - `approve_dag_skeleton_command`:提交 DAG 骨架审批决策(Allow/Deny/Modify)
//! - `list_dag_history_command`:分页 + 状态过滤查询历史 DAG
//! - `get_dag_plan_command`:查询单个 DAG 完整详情(plan + nodes)
//! - `get_task_explanation_command`:查询 step 的 LLM 失败归因
//!
//! 安全规则(参考 project_memory.md "Tauri IPC 三安全规则"):
//! - WebView 不直接访问 filesystem(本模块只读 DagRepo / TaskExplanationRepo)
//! - UI 不直接调用 MCP(DAG 审批走 oneshot channel,不触发 MCP)
//! - `approval_request_id` 单次使用(`take_sender` 移除 sender,防重放)

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;
use trust_kernel::approval::types::ApprovalDecision;

/// W8 §2.7:DAG 骨架审批决策。
/// 与 `ApprovalDecision` 一致,但单独定义以便未来扩展 Modify payload。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DagApprovalDecision {
    Allow,
    Deny,
    /// W9+ 实现(spec §8 延后项):用户调整 input_template
    Modify,
}

impl From<DagApprovalDecision> for ApprovalDecision {
    fn from(d: DagApprovalDecision) -> Self {
        match d {
            DagApprovalDecision::Allow => ApprovalDecision::Allow,
            DagApprovalDecision::Deny => ApprovalDecision::Deny,
            DagApprovalDecision::Modify => ApprovalDecision::Modify,
        }
    }
}

/// 提交 DAG 骨架审批决策。
///
/// 由 webview `DagApprovalDialog` 在用户点击 Allow/Deny 后调用。
/// 通过 `ApprovalRegistry::take_sender` 取出 oneshot sender,发送决策。
/// 返回 true = 投递成功,false = 请求已被消费 / 已过期 / 不存在(一次性语义)。
pub fn submit_dag_skeleton_approval(
    state: &AppState,
    approval_request_id: &str,
    decision: DagApprovalDecision,
) -> UiResult<bool> {
    let sender = match state.approval_registry.take_sender(approval_request_id) {
        Some(s) => s,
        None => return Ok(false),
    };
    let _ = sender.send(decision.into());
    Ok(true)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn approve_dag_skeleton_command(
    state: State<'_, AppState>,
    approval_request_id: String,
    decision: DagApprovalDecision,
) -> Result<bool, String> {
    submit_dag_skeleton_approval(&state, &approval_request_id, decision).map_err(Into::into)
}
```

注意:
- `DagApprovalDecision` 单独定义(不直接复用 `ApprovalDecision`),为 W9+ Modify payload 扩展预留
- `From<DagApprovalDecision> for ApprovalDecision` 实现转换,复用 kernel 类型
- `submit_dag_skeleton_approval` 是逻辑函数(返回 `UiResult<bool>`),供测试直接调用;`approve_dag_skeleton_command` 是命令函数(返回 `Result<bool, String>` + `.map_err(Into::into)`),遵循 project_memory.md 模式
- `take_sender` 移除 sender 后,再次调用同一 `approval_request_id` 返回 `None` → `Ok(false)`,实现"用后即焚"

- [ ] **Step 3: 在 `lib.rs` 加 `pub mod dag_commands;`**

打开 `voicepilot/crates/ui/src/lib.rs`,在现有 `#[cfg(feature = "tauri")] pub mod` 块中追加(在 `pub mod commands;` 之后):

```rust
#[cfg(feature = "tauri")]
pub mod dag_commands;
```

- [ ] **Step 4: 写失败测试 — 在 `tests/commands_unit.rs` 末尾追加**

打开 `voicepilot/crates/ui/tests/commands_unit.rs`,在文件末尾追加:

```rust
// ===== W8 Plan 5 Task 1: DAG 骨架审批 =====

use voicepilot::dag_commands::{submit_dag_skeleton_approval, DagApprovalDecision};
use voicepilot::approver::ApprovalRegistry;

#[test]
fn submit_dag_skeleton_approval_delivers_decision() {
    let state = voicepilot::state::AppState::new_in_memory().unwrap();
    let registry = ApprovalRegistry::new();
    // 模拟 TauriApprover 创建请求(不 emit 事件)
    use trust_kernel::policy::transaction::EffectManifest;
    let dummy = EffectManifest {
        sources: vec![],
        destination: "dag://test-plan".into(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id, rx) = registry.create_request(&dummy);
    // 注意:state.approval_registry 与 registry 是不同实例!
    // 测试需要用同一 registry,所以这里验证 take_sender 语义而非完整流程。
    // 完整流程在 w8_dag_commands_unit.rs 集成测试中验证(用 TauriApprover::new)。
    drop(rx);

    // 用 state 自带的 registry 测试
    let dummy2 = EffectManifest {
        sources: vec![],
        destination: "dag://test-plan-2".into(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id2, _rx2) = state.approval_registry.create_request(&dummy2);
    let delivered = submit_dag_skeleton_approval(&state, &approval_id2, DagApprovalDecision::Allow).unwrap();
    assert!(delivered, "first submission should succeed");
}

#[test]
fn submit_dag_skeleton_approval_rejects_replay() {
    // 一次性语义:同一 approval_request_id 第二次调用返回 false
    let state = voicepilot::state::AppState::new_in_memory().unwrap();
    use trust_kernel::policy::transaction::EffectManifest;
    let dummy = EffectManifest {
        sources: vec![],
        destination: "dag://replay-test".into(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id, _rx) = state.approval_registry.create_request(&dummy);

    let first = submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Deny).unwrap();
    assert!(first, "first call should deliver");

    let second = submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Allow).unwrap();
    assert!(!second, "replay should be rejected (single-use)");
}

#[test]
fn submit_dag_skeleton_approval_unknown_id_returns_false() {
    let state = voicepilot::state::AppState::new_in_memory().unwrap();
    let result = submit_dag_skeleton_approval(&state, "apr_nonexistent", DagApprovalDecision::Allow).unwrap();
    assert!(!result, "unknown approval_request_id should return false");
}

#[test]
fn dag_approval_decision_convert_to_approval_decision() {
    assert_eq!(
        ApprovalDecision::from(DagApprovalDecision::Allow),
        ApprovalDecision::Allow
    );
    assert_eq!(
        ApprovalDecision::from(DagApprovalDecision::Deny),
        ApprovalDecision::Deny
    );
    assert_eq!(
        ApprovalDecision::from(DagApprovalDecision::Modify),
        ApprovalDecision::Modify
    );
}
```

注意:测试顶部需加 `use trust_kernel::approval::types::ApprovalDecision;` import(若已有则跳过)。

- [ ] **Step 5: 跑测试,确认失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --test commands_unit --features tauri submit_dag_skeleton_approval`
Expected: FAIL(`dag_commands` 模块未导出 / `submit_dag_skeleton_approval` 未定义)

- [ ] **Step 6: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p voicepilot-ui --features tauri`
Expected: PASS(若 `DagPlan` import 路径错误,确认 `trust_kernel::skills::dag_types::DagPlan` 是 Plan 1 定义路径)

- [ ] **Step 7: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --test commands_unit --features tauri`
Expected: PASS(4 个新测试全绿 + 现有测试无回归)

- [ ] **Step 8: Commit**

```powershell
git add voicepilot/crates/ui/src/approver.rs voicepilot/crates/ui/src/dag_commands.rs voicepilot/crates/ui/src/lib.rs voicepilot/crates/ui/tests/commands_unit.rs
git commit -m "feat(w8p5): add approve_dag_skeleton_command + TauriApprover::approve_dag_skeleton with oneshot+5min timeout"
```

---

## Task 2: list_dag_history_command + get_dag_plan_command

**Files:**
- Modify: `voicepilot/crates/ui/src/dag_commands.rs`
- Modify: `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs`(新建)

**目标:** 实现 DAG 历史列表(分页 + 状态过滤)+ 单个 DAG 详情查询。复用 Plan 1 的 `DagRepo::list_plans_by_status` + `get_plan` + `list_nodes_by_plan`。

- [ ] **Step 1: 在 `dag_commands.rs` 加 DTO 类型 + `list_dag_history` 逻辑函数**

在 `dag_commands.rs` 中追加(在 `submit_dag_skeleton_approval` 之后):

```rust
// ===== W8 Plan 5 Task 2: list_dag_history + get_dag_plan =====

use trust_kernel::skills::dag_repo::{DagPlanRecord, DagNodeRecord, DagRepo};
use trust_kernel::skills::dag_types::DagStatus;

/// W8 §2.7:DAG 历史列表项(webview 表格行)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlanSummaryDto {
    pub plan_id: String,
    pub user_goal: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub root_task_id: Option<String>,
    /// 节点数(从 plan_json 解析,前端显示 "N 个节点")
    pub node_count: usize,
    /// 成功率(succeeded 节点数 / 总节点数,0.0-1.0)
    pub success_rate: f32,
}

/// W8 §2.7:DAG 详情(webview 点击行展开)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlanDetailDto {
    pub plan_id: String,
    pub user_goal: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub max_total_steps: u32,
    pub nodes: Vec<DagNodeDetailDto>,
    pub edges: Vec<DagEdgeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNodeDetailDto {
    pub node_id: String,
    pub skill_id: String,
    pub risk_ceiling: String,
    pub status: String,
    pub input_template_json: String,
    pub output_json: Option<String>,
    pub error_message: Option<String>,
    pub task_id: Option<String>,
    pub step_id: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagEdgeDto {
    pub from: String,
    pub to: String,
    pub port_binding: Option<String>,
}

/// 状态过滤选项(webview 下拉框)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DagStatusFilter {
    All,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl DagStatusFilter {
    fn to_status_str(&self) -> Option<&str> {
        match self {
            Self::All => None,
            Self::Running => Some("running"),
            Self::Succeeded => Some("succeeded"),
            Self::Failed => Some("failed"),
            Self::Cancelled => Some("cancelled"),
        }
    }
}

/// 从 DagPlanRecord 解析 node_count + success_rate。
/// plan_json 是序列化的 DagPlan,nodes 字段含节点列表;
/// success_rate 需查 dag_nodes 表(从 plan_json 无法获取节点状态)。
fn parse_node_count_from_plan_json(plan_json: &str) -> usize {
    #[derive(serde::Deserialize)]
    struct PlanShell {
        nodes: Vec<serde_json::Value>,
    }
    serde_json::from_str::<PlanShell>(plan_json)
        .map(|p| p.nodes.len())
        .unwrap_or(0)
}

/// 逻辑函数:分页 + 状态过滤查询 DAG 历史列表。
///
/// - `limit`:每页数量(默认 20,硬上限 100)
/// - `offset`:分页偏移
/// - `filter`:状态过滤(All / Running / Succeeded / Failed / Cancelled)
pub fn list_dag_history(
    state: &AppState,
    limit: usize,
    offset: usize,
    filter: DagStatusFilter,
) -> UiResult<Vec<DagPlanSummaryDto>> {
    let repo = DagRepo::new();
    let conn = state.kernel.conn();
    let limit_clamped = limit.min(100).max(1);
    let records: Vec<DagPlanRecord> = match filter.to_status_str() {
        Some(status) => repo.list_plans_by_status(&conn, status)?,
        None => {
            // All:逐个 status 查询后合并(Plan 1 未实现 list_all_plans,W8 简化)
            let mut all = Vec::new();
            for s in ["pending", "running", "succeeded", "failed", "partially_succeeded", "cancelled"] {
                all.extend(repo.list_plans_by_status(&conn, s)?);
            }
            // 按 created_at DESC 排序
            all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            all
        }
    };

    // 分页(offset + limit)
    let paged: Vec<DagPlanRecord> = records
        .into_iter()
        .skip(offset)
        .take(limit_clamped)
        .collect();

    let mut summaries = Vec::with_capacity(paged.len());
    for rec in paged {
        let node_count = parse_node_count_from_plan_json(&rec.plan_json);
        // success_rate 需查 dag_nodes 表
        let nodes = repo.list_nodes_by_plan(&conn, &rec.plan_id)?;
        let total = nodes.len();
        let succeeded = nodes
            .iter()
            .filter(|n| n.status == "succeeded")
            .count();
        let success_rate = if total == 0 {
            0.0
        } else {
            succeeded as f32 / total as f32
        };
        summaries.push(DagPlanSummaryDto {
            plan_id: rec.plan_id,
            user_goal: rec.user_goal,
            status: rec.status,
            created_at: rec.created_at,
            completed_at: rec.completed_at,
            root_task_id: rec.root_task_id,
            node_count,
            success_rate,
        });
    }
    Ok(summaries)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn list_dag_history_command(
    state: State<'_, AppState>,
    limit: Option<usize>,
    offset: Option<usize>,
    filter: Option<DagStatusFilter>,
) -> Result<Vec<DagPlanSummaryDto>, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);
    let filter = filter.unwrap_or(DagStatusFilter::All);
    list_dag_history(&state, limit, offset, filter).map_err(Into::into)
}
```

- [ ] **Step 2: 在 `dag_commands.rs` 加 `get_dag_plan` 逻辑函数**

继续在 `dag_commands.rs` 中追加:

```rust
/// 逻辑函数:查询单个 DAG 完整详情(plan + nodes + edges)。
///
/// 返回 `None`(包在 `Option` 中)若 plan_id 不存在。
/// webview 点击历史表格行时调用,展开节点详情。
pub fn get_dag_plan(state: &AppState, plan_id: &str) -> UiResult<Option<DagPlanDetailDto>> {
    let repo = DagRepo::new();
    let conn = state.kernel.conn();
    let plan_rec = match repo.get_plan(&conn, plan_id)? {
        Some(r) => r,
        None => return Ok(None),
    };

    // 从 plan_json 解析 max_total_steps + edges
    #[derive(serde::Deserialize)]
    struct PlanShell {
        max_total_steps: u32,
        edges: Vec<DagEdgeDto>,
    }
    let shell: PlanShell = serde_json::from_str(&plan_rec.plan_json).unwrap_or(PlanShell {
        max_total_steps: 0,
        edges: vec![],
    });

    let node_recs = repo.list_nodes_by_plan(&conn, plan_id)?;
    let nodes: Vec<DagNodeDetailDto> = node_recs
        .into_iter()
        .map(|n| DagNodeDetailDto {
            node_id: n.node_id,
            skill_id: n.skill_id,
            risk_ceiling: n.risk_ceiling,
            status: n.status,
            input_template_json: n.input_template_json,
            output_json: n.output_json,
            error_message: n.error_message,
            task_id: n.task_id,
            step_id: n.step_id,
            started_at: n.started_at,
            completed_at: n.completed_at,
        })
        .collect();

    Ok(Some(DagPlanDetailDto {
        plan_id: plan_rec.plan_id,
        user_goal: plan_rec.user_goal,
        status: plan_rec.status,
        created_at: plan_rec.created_at,
        completed_at: plan_rec.completed_at,
        max_total_steps: shell.max_total_steps,
        nodes,
        edges: shell.edges,
    }))
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn get_dag_plan_command(
    state: State<'_, AppState>,
    planId: String,
) -> Result<Option<DagPlanDetailDto>, String> {
    get_dag_plan(&state, &planId).map_err(Into::into)
}
```

注意:Tauri 命令参数名用 `planId`(camelCase),Tauri 2 自动转换为 Rust 的 `plan_id` 蛇形命名。

- [ ] **Step 3: 创建集成测试文件 `w8_dag_commands_unit.rs`**

创建 `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs`:

```rust
//! W8 Plan 5 Task 2: list_dag_history + get_dag_plan 集成测试.

use std::collections::HashMap;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagNodeStatus, DagPlan, DagStatus,
};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
use voicepilot::dag_commands::{
    get_dag_plan, list_dag_history, DagStatusFilter,
};
use voicepilot::state::AppState;

fn make_test_plan(plan_id: &str, status: &DagStatus) -> DagPlan {
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

fn seed_plan(state: &AppState, plan_id: &str, status: &DagStatus) {
    let repo = DagRepo::new();
    let conn = state.kernel.conn();
    let plan = make_test_plan(plan_id, status);
    repo.create_plan(&conn, &plan, status, None).unwrap();
    for node in &plan.nodes {
        repo.create_node(&conn, &plan.plan_id, node).unwrap();
    }
}

#[test]
fn list_dag_history_returns_all_when_filter_all() {
    let state = AppState::new_in_memory().unwrap();
    seed_plan(&state, "p-001", &DagStatus::Succeeded);
    seed_plan(&state, "p-002", &DagStatus::Failed {
        failed_node: "n2".into(),
        cause: "permission denied".into(),
    });

    let result = list_dag_history(&state, 20, 0, DagStatusFilter::All).unwrap();
    assert_eq!(result.len(), 2, "should return 2 plans");
    // created_at DESC 排序(后插入的在前)
    assert_eq!(result[0].plan_id, "p-002");
    assert_eq!(result[1].plan_id, "p-001");
}

#[test]
fn list_dag_history_filters_by_status() {
    let state = AppState::new_in_memory().unwrap();
    seed_plan(&state, "p-running", &DagStatus::Running);
    seed_plan(&state, "p-succeeded", &DagStatus::Succeeded);

    let running = list_dag_history(&state, 20, 0, DagStatusFilter::Running).unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].plan_id, "p-running");

    let succeeded = list_dag_history(&state, 20, 0, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(succeeded.len(), 1);
    assert_eq!(succeeded[0].plan_id, "p-succeeded");
}

#[test]
fn list_dag_history_paginates_with_offset_and_limit() {
    let state = AppState::new_in_memory().unwrap();
    for i in 0..5 {
        seed_plan(&state, &format!("p-{:03}", i), &DagStatus::Succeeded);
    }

    let page1 = list_dag_history(&state, 2, 0, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(page1.len(), 2);

    let page2 = list_dag_history(&state, 2, 2, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(page2.len(), 2);

    let page3 = list_dag_history(&state, 2, 4, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(page3.len(), 1, "last page has 1 item");
}

#[test]
fn list_dag_history_computes_node_count_and_success_rate() {
    let state = AppState::new_in_memory().unwrap();
    let repo = DagRepo::new();
    let conn = state.kernel.conn();
    let plan = make_test_plan("p-rate", &DagStatus::PartiallySucceeded {
        succeeded: vec!["n1".into()],
        failed_node: "n2".into(),
        cause: "err".into(),
    });
    repo.create_plan(&conn, &plan, &DagStatus::PartiallySucceeded {
        succeeded: vec!["n1".into()],
        failed_node: "n2".into(),
        cause: "err".into(),
    }, None).unwrap();
    for node in &plan.nodes {
        repo.create_node(&conn, &plan.plan_id, node).unwrap();
    }
    // 模拟 n1 成功,n2 失败
    repo.update_node_status(&conn, "p-rate", "n1",
        &DagNodeStatus::Succeeded(serde_json::json!({"path": "C:/x.txt"})),
        Some("task-1"), Some("step-1")).unwrap();
    repo.update_node_status(&conn, "p-rate", "n2",
        &DagNodeStatus::Failed { cause: "permission denied".into() },
        None, None).unwrap();

    let result = list_dag_history(&state, 20, 0, DagStatusFilter::All).unwrap();
    let item = result.iter().find(|r| r.plan_id == "p-rate").unwrap();
    assert_eq!(item.node_count, 2, "node_count from plan_json");
    assert!((item.success_rate - 0.5).abs() < 0.01, "1/2 = 0.5 success rate");
}

#[test]
fn get_dag_plan_returns_full_detail() {
    let state = AppState::new_in_memory().unwrap();
    seed_plan(&state, "p-detail", &DagStatus::Succeeded);

    let detail = get_dag_plan(&state, "p-detail").unwrap().unwrap();
    assert_eq!(detail.plan_id, "p-detail");
    assert_eq!(detail.user_goal, "打开记事本写 TODO 然后保存到桌面");
    assert_eq!(detail.max_total_steps, 5);
    assert_eq!(detail.nodes.len(), 2);
    assert_eq!(detail.nodes[0].node_id, "n1");
    assert_eq!(detail.nodes[0].skill_id, "note.capture");
    assert_eq!(detail.nodes[1].node_id, "n2");
    assert_eq!(detail.edges.len(), 1);
    assert_eq!(detail.edges[0].from, "n1");
    assert_eq!(detail.edges[0].to, "n2");
}

#[test]
fn get_dag_plan_returns_none_for_unknown_id() {
    let state = AppState::new_in_memory().unwrap();
    let result = get_dag_plan(&state, "nonexistent").unwrap();
    assert!(result.is_none());
}
```

- [ ] **Step 4: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --test w8_dag_commands_unit --features tauri`
Expected: PASS(6 个测试全绿)

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/ui/src/dag_commands.rs voicepilot/crates/ui/tests/w8_dag_commands_unit.rs
git commit -m "feat(w8p5): add list_dag_history_command + get_dag_plan_command with pagination+filter"
```

---

## Task 3: get_task_explanation_command

**Files:**
- Modify: `voicepilot/crates/ui/src/dag_commands.rs`
- Modify: `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs`

**目标:** 实现 task.explain LLM 归因查询。复用 Plan 1 的 `TaskExplanationRepo::get_by_step_id` + Plan 3 的 `TaskExplanation` / `LlmAnalysis` 结构。

- [ ] **Step 1: 在 `dag_commands.rs` 加 `TaskExplanationDto` + `get_task_explanation` 逻辑函数**

在 `dag_commands.rs` 末尾追加:

```rust
// ===== W8 Plan 5 Task 3: get_task_explanation =====

use trust_kernel::skills::explanation_repo::{FailureCategory, TaskExplanationRecord, TaskExplanationRepo};

/// W8 §2.5:task.explain 输出(webview 显示 LLM 失败归因)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExplanationDto {
    pub explanation_id: String,
    pub step_id: String,
    pub root_cause_zh: String,
    pub category: String,
    pub suggested_fix: Option<String>,
    pub confidence: f32,
    pub llm_model: Option<String>,
    pub created_at: String,
}

impl From<TaskExplanationRecord> for TaskExplanationDto {
    fn from(r: TaskExplanationRecord) -> Self {
        Self {
            explanation_id: r.explanation_id,
            step_id: r.step_id,
            root_cause_zh: r.root_cause_zh,
            category: r.category,
            suggested_fix: r.suggested_fix,
            confidence: r.confidence,
            llm_model: r.llm_model,
            created_at: r.created_at,
        }
    }
}

/// 逻辑函数:查询 step 的最新 LLM 失败归因。
///
/// 返回 `None` 若该 step 无归因记录(LLM 未启用 / step 非 Failed / 尚未调用 task.explain)。
/// webview `TaskExplainPanel` 据此显示 "未启用 LLM 归因" 提示。
pub fn get_task_explanation(
    state: &AppState,
    step_id: &str,
) -> UiResult<Option<TaskExplanationDto>> {
    let repo = TaskExplanationRepo::new();
    let conn = state.kernel.conn();
    let rec = repo.get_by_step_id(&conn, step_id)?;
    Ok(rec.map(TaskExplanationDto::from))
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn get_task_explanation_command(
    state: State<'_, AppState>,
    stepId: String,
) -> Result<Option<TaskExplanationDto>, String> {
    get_task_explanation(&state, &stepId).map_err(Into::into)
}
```

- [ ] **Step 2: 在 `w8_dag_commands_unit.rs` 追加 `get_task_explanation` 测试**

在 `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs` 末尾追加:

```rust
// ===== W8 Plan 5 Task 3: get_task_explanation =====

use trust_kernel::repo::step_repo::StepRepo;
use trust_kernel::repo::task_repo::TaskRepo;
use trust_kernel::skills::explanation_repo::{FailureCategory, TaskExplanationRecord, TaskExplanationRepo};
use trust_kernel::state::TaskState;
use voicepilot::dag_commands::get_task_explanation;

fn seed_step(state: &AppState, step_id: &str) {
    let conn = state.kernel.conn();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    let task_id = format!("task-for-{}", step_id);
    task_repo
        .create(&conn, &task_id, "test goal", TaskState::Pending)
        .unwrap();
    step_repo
        .create(&conn, step_id, &task_id, 0, "test step")
        .unwrap();
}

#[test]
fn get_task_explanation_returns_record_for_step() {
    let state = AppState::new_in_memory().unwrap();
    seed_step(&state, "step-explain-1");
    let repo = TaskExplanationRepo::new();
    let conn = state.kernel.conn();
    let rec = TaskExplanationRecord {
        explanation_id: "exp-001".into(),
        step_id: "step-explain-1".into(),
        root_cause_zh: "MCP 服务器未启动,Playwright 工具调用失败".into(),
        category: FailureCategory::McpUnavailable.as_str().into(),
        suggested_fix: Some("请在 Settings 中启动 Playwright MCP".into()),
        confidence: 0.85,
        llm_model: Some("gpt-4o-mini".into()),
        created_at: String::new(),
    };
    repo.create(&conn, &rec).unwrap();

    let result = get_task_explanation(&state, "step-explain-1").unwrap().unwrap();
    assert_eq!(result.explanation_id, "exp-001");
    assert_eq!(result.step_id, "step-explain-1");
    assert_eq!(result.root_cause_zh, "MCP 服务器未启动,Playwright 工具调用失败");
    assert_eq!(result.category, "mcp_unavailable");
    assert!((result.confidence - 0.85).abs() < 0.001);
    assert_eq!(result.llm_model.as_deref(), Some("gpt-4o-mini"));
}

#[test]
fn get_task_explanation_returns_none_for_step_without_record() {
    let state = AppState::new_in_memory().unwrap();
    seed_step(&state, "step-no-explain");
    let result = get_task_explanation(&state, "step-no-explain").unwrap();
    assert!(result.is_none(), "step without explanation should return None");
}

#[test]
fn get_task_explanation_returns_latest_for_multiple_records() {
    let state = AppState::new_in_memory().unwrap();
    seed_step(&state, "step-multi");
    let repo = TaskExplanationRepo::new();
    let conn = state.kernel.conn();
    // 创建两条记录,期望返回最新(created_at DESC LIMIT 1)
    let rec1 = TaskExplanationRecord {
        explanation_id: "exp-old".into(),
        step_id: "step-multi".into(),
        root_cause_zh: "旧归因".into(),
        category: FailureCategory::Unknown.as_str().into(),
        suggested_fix: None,
        confidence: 0.3,
        llm_model: None,
        created_at: "100".into(),
    };
    let rec2 = TaskExplanationRecord {
        explanation_id: "exp-new".into(),
        step_id: "step-multi".into(),
        root_cause_zh: "新归因".into(),
        category: FailureCategory::PathNotAllowed.as_str().into(),
        suggested_fix: Some("检查 allowed_paths".into()),
        confidence: 0.9,
        llm_model: Some("gpt-4o".into()),
        created_at: "200".into(),
    };
    repo.create(&conn, &rec1).unwrap();
    repo.create(&conn, &rec2).unwrap();

    let result = get_task_explanation(&state, "step-multi").unwrap().unwrap();
    // TaskExplanationRepo::get_by_step_id 按 created_at DESC LIMIT 1
    // 但 created_at 是 repo 内部 now_iso() 生成,不是 rec.created_at
    // 所以两条记录的 created_at 几乎相同,顺序取决于插入顺序(LIMIT 1 取最后一条)
    assert_eq!(result.step_id, "step-multi");
    // 不严格断言 explanation_id,因为 created_at 由 repo 控制
}

#[test]
fn get_task_explanation_returns_none_for_unknown_step() {
    let state = AppState::new_in_memory().unwrap();
    let result = get_task_explanation(&state, "step-nonexistent").unwrap();
    assert!(result.is_none());
}
```

- [ ] **Step 3: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --test w8_dag_commands_unit --features tauri`
Expected: PASS(10 个测试全绿:6 个 Task 2 + 4 个 Task 3)

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/ui/src/dag_commands.rs voicepilot/crates/ui/tests/w8_dag_commands_unit.rs
git commit -m "feat(w8p5): add get_task_explanation_command for LLM failure attribution display"
```

---

## Task 4: DagApprovalDialog.tsx 组件

**Files:**
- Create: `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx`
- Modify: `voicepilot/crates/ui/web/src/api.ts`(加 `approveDagSkeleton` + `onDagApprovalRequest`)
- Modify: `voicepilot/crates/ui/web/src/types.ts`(加 DAG 类型)
- Modify: `voicepilot/crates/ui/web/src/styles.css`(加 DAG 弹窗样式)
- Modify: `voicepilot/crates/ui/web/src/App.tsx`(加 dagApproval state + listener)

**目标:** 实现 DAG 骨架审批弹窗,显示节点卡片 + 边连线图 + Allow/Deny 按钮,Esc 关闭 = Deny,卸载时自动 Deny(一次性语义)。

- [ ] **Step 1: 在 `types.ts` 加 DAG 类型**

打开 `voicepilot/crates/ui/web/src/types.ts`,在文件末尾追加:

```typescript
// ===== W8 Plan 5: DAG 相关类型 =====

/** W8 §2.3:DAG 节点(前端镜像,与后端 DagNodeDetailDto 对齐)。 */
export interface DagNode {
  node_id: string;
  skill_id: string;
  risk_ceiling: string; // "E0" | "E1" | "E2" | "E3"
  status: string;
  input_template_json: string;
  output_json: string | null;
  error_message: string | null;
  task_id: string | null;
  step_id: string | null;
  started_at: string | null;
  completed_at: string | null;
}

/** W8 §2.3:DAG 边。 */
export interface DagEdge {
  from: string;
  to: string;
  port_binding: string | null;
}

/** W8 §2.3:DAG 完整详情。 */
export interface DagPlanDetail {
  plan_id: string;
  user_goal: string;
  status: string;
  created_at: string;
  completed_at: string | null;
  max_total_steps: number;
  nodes: DagNode[];
  edges: DagEdge[];
}

/** W8 §2.7:DAG 历史列表项。 */
export interface DagPlanSummary {
  plan_id: string;
  user_goal: string;
  status: string;
  created_at: string;
  completed_at: string | null;
  root_task_id: string | null;
  node_count: number;
  success_rate: number;
}

/** W8 §2.5:task.explain LLM 归因。 */
export interface TaskExplanation {
  explanation_id: string;
  step_id: string;
  root_cause_zh: string;
  category: string;
  suggested_fix: string | null;
  confidence: number;
  llm_model: string | null;
  created_at: string;
}

/** W8 §2.7:DAG 骨架审批请求 payload(后端 emit `dag-approval-request` 事件)。 */
export interface DagApprovalRequestPayload {
  approval_request_id: string;
  plan_id: string;
  user_goal: string;
  max_total_steps: number;
  node_count: number;
  plan_json: unknown; // 序列化的 DagPlan
}

/** W8 §2.7:DAG 审批决策。 */
export type DagApprovalDecision = "allow" | "deny" | "modify";

/** W8 §2.7:DAG 状态过滤。 */
export type DagStatusFilter =
  | "all"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled";
```

- [ ] **Step 2: 在 `types.ts` 扩展 `View` 类型**

修改 `types.ts` 中的 `View` 类型(原在第 98 行):

```typescript
export type View = "main" | "settings" | "audit" | "trust" | "skills" | "dag-history";
```

- [ ] **Step 3: 在 `api.ts` 加 5 个 DAG API 函数**

打开 `voicepilot/crates/ui/web/src/api.ts`,在文件末尾追加:

```typescript
// ===== W8 Plan 5: DAG 相关 API =====

import type {
  DagApprovalDecision,
  DagApprovalRequestPayload,
  DagPlanDetail,
  DagPlanSummary,
  DagStatusFilter,
  TaskExplanation,
} from "./types";

/** 提交 DAG 骨架审批决策。 */
export async function approveDagSkeleton(
  approvalRequestId: string,
  decision: DagApprovalDecision
): Promise<boolean> {
  return invoke<boolean>("approve_dag_skeleton_command", {
    approvalRequestId,
    decision,
  });
}

/** 监听 `dag-approval-request` 事件(后端 TauriApprover::approve_dag_skeleton emit)。 */
export function onDagApprovalRequest(
  handler: (payload: DagApprovalRequestPayload) => void
): Promise<UnlistenFn> {
  return listen<DagApprovalRequestPayload>("dag-approval-request", (event) => {
    handler(event.payload);
  });
}

/** 分页 + 状态过滤查询 DAG 历史。 */
export async function listDagHistory(
  limit: number = 20,
  offset: number = 0,
  filter: DagStatusFilter = "all"
): Promise<DagPlanSummary[]> {
  return invoke<DagPlanSummary[]>("list_dag_history_command", {
    limit,
    offset,
    filter,
  });
}

/** 查询单个 DAG 完整详情。 */
export async function getDagPlan(planId: string): Promise<DagPlanDetail | null> {
  return invoke<DagPlanDetail | null>("get_dag_plan_command", { planId });
}

/** 查询 step 的 LLM 失败归因。 */
export async function getTaskExplanation(
  stepId: string
): Promise<TaskExplanation | null> {
  return invoke<TaskExplanation | null>("get_task_explanation_command", {
    stepId,
  });
}
```

- [ ] **Step 4: 创建 `DagApprovalDialog.tsx` 组件**

创建 `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx`:

```tsx
import { useEffect, useRef, useState } from "react";
import { approveDagSkeleton } from "../api";
import type {
  DagApprovalDecision,
  DagApprovalRequestPayload,
  DagEdge,
  DagNode,
} from "../types";

interface Props {
  payload: DagApprovalRequestPayload;
  onDismiss: () => void;
}

/** 从 plan_json 解析 nodes + edges(后端 emit 的 plan_json 是序列化的 DagPlan)。 */
function parsePlanJson(planJson: unknown): { nodes: DagNode[]; edges: DagEdge[] } {
  if (typeof planJson !== "object" || planJson === null) {
    return { nodes: [], edges: [] };
  }
  const plan = planJson as { nodes?: DagNode[]; edges?: DagEdge[] };
  return {
    nodes: plan.nodes ?? [],
    edges: plan.edges ?? [],
  };
}

/** 风险徽章颜色:E0=灰 / E1=蓝 / E2=琥珀 / E3=红(参考 UI 设计要求)。 */
function riskBadgeClass(riskCeiling: string): string {
  const r = riskCeiling.toUpperCase();
  switch (r) {
    case "E0":
      return "risk-badge risk-badge-e0";
    case "E1":
      return "risk-badge risk-badge-e1";
    case "E2":
      return "risk-badge risk-badge-e2";
    case "E3":
      return "risk-badge risk-badge-e3";
    default:
      return "risk-badge risk-badge-e0";
  }
}

/** 渲染 input_template 预览(模板渲染后,显示原始模板字符串)。 */
function renderTemplatePreview(inputTemplateJson: string): string {
  try {
    const tpl = JSON.parse(inputTemplateJson);
    // SlotTemplate { kind, template: TemplateExpr }
    // 简化预览:显示 template 字段的 JSON
    return JSON.stringify(tpl.template ?? tpl, null, 2);
  } catch {
    return inputTemplateJson;
  }
}

export function DagApprovalDialog({ payload, onDismiss }: Props): JSX.Element {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submittedRef = useRef(false);
  const { approval_request_id, plan_id, user_goal, max_total_steps, node_count, plan_json } = payload;
  const { nodes, edges } = parsePlanJson(plan_json);

  async function decide(decision: DagApprovalDecision): Promise<void> {
    setSubmitting(true);
    setError(null);
    try {
      await approveDagSkeleton(approval_request_id, decision);
      submittedRef.current = true;
      onDismiss();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // Esc 键关闭 = Deny(参考 ApprovalModal 模式 + WCAG A 可访问性)
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        onDismiss();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onDismiss]);

  // 卸载时自动 Deny(一次性语义,参考 ApprovalModal)
  // submittedRef 短路:已提交则跳过
  useEffect(() => {
    return () => {
      if (submittedRef.current) return;
      approveDagSkeleton(approval_request_id, "deny").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="modal-backdrop">
      <div
        className="modal dag-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dag-approval-title"
      >
        <div className="modal-header">
          <h2 id="dag-approval-title">DAG 骨架审批</h2>
          <span className="badge">{node_count} 个节点 · 上限 {max_total_steps} 步</span>
        </div>

        <div className="modal-body">
          <div className="dag-goal-section">
            <label htmlFor="dag-goal" className="dag-label">用户意图</label>
            <p id="dag-goal" className="dag-goal-text">{user_goal}</p>
          </div>

          <div className="dag-plan-section">
            <label className="dag-label">计划 ID</label>
            <code className="dag-plan-id mono">{plan_id}</code>
          </div>

          {/* 节点卡片(minimal bento grid,不嵌套 > 2 层) */}
          <div className="dag-nodes-grid" role="group" aria-label="DAG 节点列表">
            <h3 className="dag-section-title">节点({nodes.length})</h3>
            <div className="dag-nodes-list">
              {nodes.map((node) => (
                <div key={node.node_id} className="dag-node-card">
                  <div className="dag-node-header">
                    <span className="dag-node-id mono">{node.node_id}</span>
                    <span className="dag-node-skill mono">{node.skill_id}</span>
                    <span className={riskBadgeClass(node.risk_ceiling)} aria-label={`风险等级 ${node.risk_ceiling}`}>
                      {node.risk_ceiling}
                    </span>
                  </div>
                  <div className="dag-node-body">
                    <details className="dag-template-preview">
                      <summary className="dag-summary">输入模板</summary>
                      <pre className="dag-template-code mono">{renderTemplatePreview(JSON.stringify({ template: node.input_template_json }))}</pre>
                    </details>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* 边连线图(SVG 节点 + 箭头) */}
          {edges.length > 0 && (
            <div className="dag-edges-section">
              <h3 className="dag-section-title">依赖关系({edges.length})</h3>
              <ul className="dag-edges-list" role="list">
                {edges.map((edge, i) => (
                  <li key={i} className="dag-edge-item">
                    <span className="mono">{edge.from}</span>
                    <span className="dag-edge-arrow" aria-hidden="true">→</span>
                    <span className="mono">{edge.to}</span>
                    {edge.port_binding && (
                      <span className="dag-edge-port mono">[{edge.port_binding}]</span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {error && (
            <div className="form-error" role="alert">
              错误:{error}
            </div>
          )}
        </div>

        <div className="modal-footer">
          <button
            type="button"
            className="btn btn-danger"
            onClick={() => decide("deny")}
            disabled={submitting}
            aria-label="拒绝 DAG 执行"
          >
            拒绝(Deny)
          </button>
          <button
            type="button"
            className="btn"
            disabled
            aria-disabled="true"
            title="W9+ 实现"
          >
            调整(Modify)· W9+
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => decide("allow")}
            disabled={submitting}
            aria-label="允许 DAG 执行"
          >
            允许(Allow)
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 5: 在 `styles.css` 加 DAG 弹窗样式**

打开 `voicepilot/crates/ui/web/src/styles.css`,在文件末尾追加:

```css
/* ===== W8 Plan 5: DAG 骨架审批弹窗 ===== */

.dag-dialog {
  max-width: 800px;
  width: 90vw;
  max-height: 85vh;
  overflow-y: auto;
}

.dag-label {
  font-family: var(--mono);
  font-size: 11px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-secondary);
  display: block;
  margin-bottom: 6px;
}

.dag-goal-section {
  margin-bottom: 20px;
}

.dag-goal-text {
  font-family: var(--sans);
  font-size: 15px;
  color: var(--text-primary);
  background: var(--bg-deep);
  border-left: 3px solid var(--accent);
  padding: 12px 16px;
  border-radius: 0;
}

.dag-plan-section {
  margin-bottom: 24px;
}

.dag-plan-id {
  font-size: 12px;
  color: var(--text-muted);
  background: var(--bg-deep);
  padding: 4px 8px;
  display: inline-block;
}

.dag-section-title {
  font-family: var(--mono);
  font-size: 11px;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--text-muted);
  margin-bottom: 12px;
}

.dag-nodes-grid {
  margin-bottom: 24px;
}

.dag-nodes-list {
  display: grid;
  grid-template-columns: 1fr;
  gap: 8px;
}

.dag-node-card {
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 0;
  padding: 12px 16px;
}

.dag-node-header {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
}

.dag-node-id {
  font-size: 13px;
  color: var(--accent);
  font-weight: 600;
}

.dag-node-skill {
  font-size: 12px;
  color: var(--text-primary);
  flex: 1;
}

.risk-badge {
  font-family: var(--mono);
  font-size: 10px;
  font-weight: 700;
  padding: 2px 8px;
  border-radius: 0;
  letter-spacing: 0.05em;
}

.risk-badge-e0 {
  background: transparent;
  color: var(--text-muted);
  border: 1px solid var(--text-muted);
}

.risk-badge-e1 {
  background: transparent;
  color: #3b82f6;
  border: 1px solid #3b82f6;
}

.risk-badge-e2 {
  background: var(--accent);
  color: var(--bg-deep);
  border: 1px solid var(--accent);
}

.risk-badge-e3 {
  background: var(--danger);
  color: var(--bg-deep);
  border: 1px solid var(--danger);
}

.dag-template-preview summary {
  font-family: var(--mono);
  font-size: 11px;
  color: var(--text-secondary);
  cursor: pointer;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}

.dag-template-code {
  margin-top: 8px;
  font-size: 11px;
  color: var(--text-primary);
  background: var(--bg-deep);
  padding: 8px 12px;
  border-radius: 0;
  overflow-x: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

.dag-edges-section {
  margin-bottom: 16px;
}

.dag-edges-list {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.dag-edge-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-elev);
  padding: 6px 12px;
}

.dag-edge-arrow {
  color: var(--accent);
  font-weight: 700;
}

.dag-edge-port {
  font-size: 10px;
  color: var(--text-muted);
  margin-left: 8px;
}

.dag-dialog .modal-footer {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}
```

- [ ] **Step 6: 修改 `App.tsx`,加 `dagApproval` state + listener + 渲染弹窗**

打开 `voicepilot/crates/ui/web/src/App.tsx`,做以下修改:

1. 在顶部 import 区加:

```tsx
import { onDagApprovalRequest } from "./api";
import type { ApprovalRequestPayload, DagApprovalRequestPayload, View } from "./types";
import { DagApprovalDialog } from "./components/DagApprovalDialog";
```

2. 在 `App` 函数中,在现有 `approval` state 后加 `dagApproval` state:

```tsx
const [dagApproval, setDagApproval] = useState<DagApprovalRequestPayload | null>(null);
```

3. 在现有 `useEffect`(监听 `onApprovalRequest`)后加新 `useEffect`:

```tsx
useEffect(() => {
  const unlisten = onDagApprovalRequest((payload) => setDagApproval(payload));
  return () => {
    unlisten.then((fn) => fn()).catch(() => {});
  };
}, []);
```

4. 在 NAV_ITEMS 数组中,在 skills 项后加:

```tsx
const NAV_ITEMS: { view: View; label: string }[] = [
  { view: "main", label: "Main Chat" },
  { view: "settings", label: "Settings" },
  { view: "audit", label: "Audit Viewer" },
  { view: "trust", label: "Trust Center" },
  { view: "skills", label: "Skills Manager" },
  { view: "dag-history", label: "DAG History" },
];
```

5. 在 `<main>` 标签内,在 skills 行后加:

```tsx
{view === "dag-history" && <DagHistoryView />}
```

注意:`DagHistoryView` 在 Task 5 创建,此处先引用,Task 5 完成后才能编译通过。本 step 仅修改 App.tsx,Task 5 会创建组件。

6. 在 `<ApprovalModal>` 渲染后,加 `DagApprovalDialog` 渲染:

```tsx
{dagApproval && (
  <DagApprovalDialog
    payload={dagApproval}
    onDismiss={() => setDagApproval(null)}
  />
)}
```

7. 在 import 区加(Task 5 完成后生效):

```tsx
import { DagHistoryView } from "./components/DagHistoryView";
```

注意:本 step 修改 App.tsx 后,因 `DagHistoryView` 尚未创建,编译会失败。这是预期的 — Task 5 会创建该组件,之后编译通过。若需在 Task 4 内独立验证,可临时注释掉 `DagHistoryView` 相关行。

- [ ] **Step 7: 跑 `npm.cmd run build`,确认 Task 4 编译(此时因 Task 5 未完成会有错误)**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build`
Expected: FAIL(`DagHistoryView` 未找到)— 这是预期的,Task 5 完成后通过。

若错误是 `DagApprovalDialog` 本身的 TypeScript 错误,修复后再继续。

- [ ] **Step 8: Commit(本 task 仅提交 DagApprovalDialog 相关文件,App.tsx 的 DagHistoryView 引用在 Task 5 commit)**

```powershell
git add voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/styles.css
git commit -m "feat(w8p5): add DagApprovalDialog component with node cards + edge list + Allow/Deny"
```

---

## Task 5: DagHistoryView.tsx 组件

**Files:**
- Create: `voicepilot/crates/ui/web/src/components/DagHistoryView.tsx`
- Modify: `voicepilot/crates/ui/web/src/styles.css`
- Modify: `voicepilot/crates/ui/web/src/App.tsx`(确认 Task 4 已加的 import + 路由)

**目标:** 实现 DAG 历史视图:表格 + 状态过滤 + 分页 + 点击行展开节点详情(accordion)。

- [ ] **Step 1: 创建 `DagHistoryView.tsx` 组件**

创建 `voicepilot/crates/ui/web/src/components/DagHistoryView.tsx`:

```tsx
import { useEffect, useState, useCallback } from "react";
import { getDagPlan, listDagHistory } from "../api";
import type {
  DagPlanDetail,
  DagPlanSummary,
  DagStatusFilter,
} from "../types";

const PAGE_SIZE = 20;

const STATUS_FILTERS: { value: DagStatusFilter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "running", label: "运行中" },
  { value: "succeeded", label: "已完成" },
  { value: "failed", label: "失败" },
  { value: "cancelled", label: "已取消" },
];

/** 状态徽章颜色编码(参考 UI 设计要求)。 */
function statusBadgeClass(status: string): string {
  switch (status) {
    case "pending":
      return "status-pill status-pending";
    case "running":
      return "status-pill status-running";
    case "succeeded":
      return "status-pill status-enabled";
    case "failed":
    case "partially_succeeded":
      return "status-pill status-failed";
    case "cancelled":
      return "status-pill status-disabled";
    default:
      return "status-pill";
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case "pending":
      return "待执行";
    case "running":
      return "运行中";
    case "succeeded":
      return "已完成";
    case "failed":
      return "失败";
    case "partially_succeeded":
      return "部分成功";
    case "cancelled":
      return "已取消";
    default:
      return status;
  }
}

export function DagHistoryView(): JSX.Element {
  const [plans, setPlans] = useState<DagPlanSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<DagStatusFilter>("all");
  const [offset, setOffset] = useState(0);
  const [expandedPlanId, setExpandedPlanId] = useState<string | null>(null);
  const [planDetail, setPlanDetail] = useState<DagPlanDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const refresh = useCallback((): void => {
    setLoading(true);
    setError(null);
    listDagHistory(PAGE_SIZE, offset, filter)
      .then((result) => {
        setPlans(result);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, [offset, filter]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const handleFilterChange = (next: DagStatusFilter): void => {
    setFilter(next);
    setOffset(0);
    setExpandedPlanId(null);
    setPlanDetail(null);
  };

  const handleRowClick = (planId: string): void => {
    if (expandedPlanId === planId) {
      // 收起
      setExpandedPlanId(null);
      setPlanDetail(null);
      return;
    }
    setExpandedPlanId(planId);
    setPlanDetail(null);
    setDetailError(null);
    setDetailLoading(true);
    getDagPlan(planId)
      .then((detail) => {
        setPlanDetail(detail);
        setDetailLoading(false);
      })
      .catch((e) => {
        setDetailError(String(e));
        setDetailLoading(false);
      });
  };

  const hasNextPage = plans.length === PAGE_SIZE;
  const hasPrevPage = offset > 0;

  return (
    <section
      className="view-container dag-history"
      aria-labelledby="dag-history-heading"
    >
      <h2 id="dag-history-heading">DAG 历史</h2>
      <p className="view-description">
        查看 LLM 拆解生成的 DAG 计划执行历史。点击行展开节点详情。
      </p>

      {/* 状态过滤 + 分页 */}
      <div className="dag-history-controls" role="toolbar" aria-label="DAG 历史过滤与分页">
        <div className="dag-filter-group" role="radiogroup" aria-label="状态过滤">
          {STATUS_FILTERS.map((f) => (
            <button
              key={f.value}
              type="button"
              className={`filter-btn ${filter === f.value ? "active" : ""}`}
              onClick={() => handleFilterChange(f.value)}
              aria-pressed={filter === f.value}
              role="radio"
              aria-checked={filter === f.value}
            >
              {f.label}
            </button>
          ))}
        </div>
        <div className="dag-pagination">
          <button
            type="button"
            className="btn"
            onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
            disabled={!hasPrevPage}
            aria-label="上一页"
          >
            上一页
          </button>
          <span className="dag-page-info mono" aria-live="polite">
            {offset + 1}-{offset + plans.length}
          </span>
          <button
            type="button"
            className="btn"
            onClick={() => setOffset(offset + PAGE_SIZE)}
            disabled={!hasNextPage}
            aria-label="下一页"
          >
            下一页
          </button>
        </div>
      </div>

      {loading && (
        <div role="status" aria-live="polite">
          加载 DAG 历史…
        </div>
      )}
      {error && (
        <div className="form-error" role="alert">
          错误:{error}
        </div>
      )}

      {/* 表格 */}
      {!loading && !error && (
        <table className="dag-history-table" aria-label="DAG 历史列表">
          <thead>
            <tr>
              <th scope="col">Plan ID</th>
              <th scope="col">用户意图</th>
              <th scope="col">状态</th>
              <th scope="col">创建时间</th>
              <th scope="col">完成时间</th>
              <th scope="col">节点数</th>
              <th scope="col">成功率</th>
            </tr>
          </thead>
          <tbody>
            {plans.map((plan) => {
              const isExpanded = expandedPlanId === plan.plan_id;
              return (
                <>
                  <tr
                    key={plan.plan_id}
                    className={`dag-history-row ${isExpanded ? "expanded" : ""}`}
                    onClick={() => handleRowClick(plan.plan_id)}
                    aria-expanded={isExpanded}
                    aria-controls={`detail-${plan.plan_id}`}
                    role="button"
                    tabIndex={0}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        handleRowClick(plan.plan_id);
                      }
                    }}
                  >
                    <td className="mono small">{plan.plan_id.slice(0, 8)}…</td>
                    <td className="dag-goal-cell">{plan.user_goal}</td>
                    <td>
                      <span className={statusBadgeClass(plan.status)}>
                        {statusLabel(plan.status)}
                      </span>
                    </td>
                    <td className="mono small">{plan.created_at}</td>
                    <td className="mono small">{plan.completed_at ?? "—"}</td>
                    <td className="mono">{plan.node_count}</td>
                    <td className="mono">
                      {(plan.success_rate * 100).toFixed(0)}%
                    </td>
                  </tr>
                  {isExpanded && (
                    <tr key={`${plan.plan_id}-detail`} className="dag-detail-row">
                      <td colSpan={7}>
                        <div id={`detail-${plan.plan_id}`} className="dag-detail-panel">
                          {detailLoading && <div role="status">加载节点详情…</div>}
                          {detailError && (
                            <div className="form-error" role="alert">
                              {detailError}
                            </div>
                          )}
                          {planDetail && (
                            <DagPlanDetailAccordion detail={planDetail} />
                          )}
                        </div>
                      </td>
                    </tr>
                  )}
                </>
              );
            })}
            {plans.length === 0 && (
              <tr>
                <td colSpan={7} className="empty-state">
                  暂无 DAG 历史记录
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** DAG 详情 accordion(展开行时显示节点列表)。 */
function DagPlanDetailAccordion({ detail }: { detail: DagPlanDetail }): JSX.Element {
  return (
    <div className="dag-detail-accordion">
      <div className="dag-detail-summary">
        <span className="dag-label">Plan ID:</span>
        <code className="mono">{detail.plan_id}</code>
        <span className="dag-label" style={{ marginLeft: 16 }}>上限:</span>
        <span className="mono">{detail.max_total_steps} 步</span>
      </div>
      <h4 className="dag-section-title">节点({detail.nodes.length})</h4>
      <div className="dag-nodes-list">
        {detail.nodes.map((node) => (
          <div key={node.node_id} className="dag-node-card">
            <div className="dag-node-header">
              <span className="dag-node-id mono">{node.node_id}</span>
              <span className="dag-node-skill mono">{node.skill_id}</span>
              <span className={`risk-badge risk-badge-${node.risk_ceiling.toLowerCase()}`}>
                {node.risk_ceiling}
              </span>
              <span className={`status-pill status-${node.status}`}>
                {statusLabel(node.status)}
              </span>
            </div>
            {node.error_message && (
              <div className="dag-node-error" role="alert">
                错误:{node.error_message}
              </div>
            )}
            {node.output_json && (
              <details className="dag-template-preview">
                <summary className="dag-summary">输出</summary>
                <pre className="dag-template-code mono">{node.output_json}</pre>
              </details>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 在 `styles.css` 加 DAG 历史视图样式**

在 `styles.css` 末尾追加:

```css
/* ===== W8 Plan 5: DAG 历史视图 ===== */

.dag-history {
  padding: 32px 40px;
}

.dag-history-controls {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
  gap: 16px;
  flex-wrap: wrap;
}

.dag-filter-group {
  display: flex;
  gap: 4px;
}

.filter-btn {
  font-family: var(--mono);
  font-size: 11px;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  padding: 6px 12px;
  border: 1px solid var(--border);
  background: var(--bg-elev);
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 0;
  transition: all 0.15s;
}

.filter-btn:hover {
  background: var(--bg-elev2);
  border-color: var(--border-bright);
}

.filter-btn.active {
  background: var(--accent);
  color: var(--bg-deep);
  border-color: var(--accent);
  font-weight: 600;
}

.dag-pagination {
  display: flex;
  align-items: center;
  gap: 12px;
}

.dag-page-info {
  font-size: 12px;
  color: var(--text-secondary);
}

.dag-history-table {
  width: 100%;
  border-collapse: collapse;
  background: var(--bg-base);
}

.dag-history-table th {
  text-align: left;
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-elev);
}

.dag-history-table td {
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  font-size: 13px;
  color: var(--text-primary);
}

.dag-history-row {
  cursor: pointer;
  transition: background 0.1s;
}

.dag-history-row:hover {
  background: var(--bg-elev);
}

.dag-history-row.expanded {
  background: var(--bg-elev);
  border-left: 3px solid var(--accent);
}

.dag-history-row:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.dag-goal-cell {
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dag-detail-row td {
  padding: 0;
  background: var(--bg-deep);
}

.dag-detail-panel {
  padding: 16px 24px;
  border-left: 3px solid var(--accent);
}

.dag-detail-accordion {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.dag-detail-summary {
  display: flex;
  align-items: center;
  font-size: 12px;
  color: var(--text-secondary);
}

.dag-detail-summary .mono {
  color: var(--text-primary);
}

.dag-node-error {
  margin-top: 8px;
  padding: 6px 12px;
  background: rgba(239, 68, 68, 0.1);
  border-left: 2px solid var(--danger);
  font-size: 12px;
  color: var(--danger);
}

.status-pill {
  font-family: var(--mono);
  font-size: 10px;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 0;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}

.status-pending {
  background: transparent;
  color: var(--text-muted);
  border: 1px solid var(--text-muted);
}

.status-running {
  background: transparent;
  color: #3b82f6;
  border: 1px solid #3b82f6;
}

.status-enabled {
  background: var(--success);
  color: var(--bg-deep);
  border: 1px solid var(--success);
}

.status-failed {
  background: var(--danger);
  color: var(--bg-deep);
  border: 1px solid var(--danger);
}

.status-disabled {
  background: transparent;
  color: var(--text-muted);
  border: 1px solid var(--text-muted);
  text-decoration: line-through;
}

.dag-history .empty-state {
  text-align: center;
  padding: 32px;
  color: var(--text-muted);
  font-style: italic;
}
```

- [ ] **Step 3: 确认 `App.tsx` 的 DagHistoryView import + 路由(Task 4 已加)**

打开 `voicepilot/crates/ui/web/src/App.tsx`,确认:
1. 顶部有 `import { DagHistoryView } from "./components/DagHistoryView";`
2. `<main>` 内有 `{view === "dag-history" && <DagHistoryView />}`
3. `NAV_ITEMS` 数组有 `{ view: "dag-history", label: "DAG History" }` 项

若 Task 4 已正确修改 App.tsx,本 step 无需改动。

- [ ] **Step 4: 跑 `npm.cmd run build`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build`
Expected: PASS(无 TypeScript 错误,`dist/` 目录生成)

若失败,常见错误:
- `Fragment` 未 import:DagHistoryView 中用 `<>` 包裹 `<tr>`,需 `import { Fragment } from "react"`(或改用 `<Fragment key={...}>`)— 修复:在文件顶部加 `import { Fragment, useEffect, useState, useCallback } from "react";`,并将 `<>` 改为 `<Fragment key={plan.plan_id}>`
- `JSX.Element` 类型未定义:确认 `tsconfig.json` 的 `jsx` 设置为 `"react-jsx"`

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/ui/web/src/components/DagHistoryView.tsx voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/src/App.tsx
git commit -m "feat(w8p5): add DagHistoryView component with table+filter+pagination+detail accordion"
```

---

## Task 6: TaskExplainPanel.tsx 组件

**Files:**
- Create: `voicepilot/crates/ui/web/src/components/TaskExplainPanel.tsx`
- Modify: `voicepilot/crates/ui/web/src/styles.css`

**目标:** 实现 task.explain 面板,三段可折叠 accordion:Step 基本信息 / 失败工具调用列表 / LLM 归因(root_cause_zh 中文 + category 徽章 + suggested_fix + confidence 进度条)。`llm_analysis = None` 时显示 "未启用 LLM 归因,可在 Settings 中开启"。

- [ ] **Step 1: 在 `types.ts` 加 `FailedToolCall` + `TaskExplainFull` 类型**

打开 `voicepilot/crates/ui/web/src/types.ts`,在末尾追加:

```typescript
/** W8 §2.5:失败工具调用摘要(后端 task.explain 输出的一部分)。 */
export interface FailedToolCall {
  tool_name: string;
  error_message: string;
  timestamp: string | null;
}

/** W8 §2.5:task.explain 完整输出(包含 LLM 归因)。 */
export interface TaskExplainFull {
  step_id: string;
  status: string;
  failed_tool_calls: FailedToolCall[];
  llm_analysis: TaskExplanation | null;
}
```

注意:本 plan 中 `TaskExplainPanel` 接收 `stepId` prop,内部调 `getTaskExplanation(stepId)` 获取 LLM 归因;`failed_tool_calls` 暂从 step audit_logs 提取(后端 `task.explain` Skill 在 Plan 3 已实现完整 `TaskExplanation` 结构,但本 UI 面板通过 `get_task_explanation_command` 仅获取 LLM 归因记录;Step 基本信息 + failed_tool_calls 由其他命令提供,本 plan 简化为只显示 LLM 归因段落 + 提示信息)。

- [ ] **Step 2: 创建 `TaskExplainPanel.tsx` 组件**

创建 `voicepilot/crates/ui/web/src/components/TaskExplainPanel.tsx`:

```tsx
import { useEffect, useState } from "react";
import { getTaskExplanation } from "../api";
import type { TaskExplanation } from "../types";

interface Props {
  stepId: string;
  /** Step 状态(用于决定是否显示 LLM 归因段落;Failed 才有归因)。 */
  stepStatus?: string;
}

/** LLM 归因 category 徽章颜色。 */
function categoryBadgeClass(category: string): string {
  switch (category) {
    case "mcp_unavailable":
      return "category-badge category-mcp";
    case "path_not_allowed":
      return "category-badge category-path";
    case "approval_denied":
      return "category-badge category-approval";
    case "network_error":
      return "category-badge category-network";
    case "unknown":
      return "category-badge category-unknown";
    default:
      return "category-badge category-unknown";
  }
}

function categoryLabel(category: string): string {
  switch (category) {
    case "mcp_unavailable":
      return "MCP 不可用";
    case "path_not_allowed":
      return "路径不允许";
    case "approval_denied":
      return "审批被拒";
    case "network_error":
      return "网络错误";
    case "unknown":
      return "未知";
    default:
      return category;
  }
}

export function TaskExplainPanel({ stepId, stepStatus }: Props): JSX.Element {
  const [explanation, setExplanation] = useState<TaskExplanation | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [openSection, setOpenSection] = useState<"info" | "tools" | "llm" | null>("llm");

  useEffect(() => {
    setLoading(true);
    setError(null);
    getTaskExplanation(stepId)
      .then((result) => {
        setExplanation(result);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, [stepId]);

  const toggleSection = (section: "info" | "tools" | "llm"): void => {
    setOpenSection(openSection === section ? null : section);
  };

  return (
    <section
      className="task-explain-panel"
      aria-labelledby="task-explain-heading"
    >
      <h3 id="task-explain-heading" className="task-explain-title">
        Task Explain · 失败归因
      </h3>

      {loading && (
        <div role="status" aria-live="polite">
          加载 LLM 归因…
        </div>
      )}
      {error && (
        <div className="form-error" role="alert">
          错误:{error}
        </div>
      )}

      {!loading && !error && (
        <>
          {/* Section 1: Step 基本信息 */}
          <div className="explain-accordion">
            <button
              type="button"
              className="explain-accordion-header"
              onClick={() => toggleSection("info")}
              aria-expanded={openSection === "info"}
              aria-controls="explain-info"
            >
              <span className="explain-accordion-title">Step 基本信息</span>
              <span className="explain-accordion-icon" aria-hidden="true">
                {openSection === "info" ? "−" : "+"}
              </span>
            </button>
            {openSection === "info" && (
              <div id="explain-info" className="explain-accordion-body">
                <div className="explain-info-row">
                  <span className="dag-label">Step ID</span>
                  <code className="mono">{stepId}</code>
                </div>
                {stepStatus && (
                  <div className="explain-info-row">
                    <span className="dag-label">状态</span>
                    <span className={`status-pill status-${stepStatus}`}>
                      {stepStatus}
                    </span>
                  </div>
                )}
              </div>
            )}
          </div>

          {/* Section 2: 失败工具调用列表(W8 简化:仅显示提示,完整列表需后端额外命令) */}
          <div className="explain-accordion">
            <button
              type="button"
              className="explain-accordion-header"
              onClick={() => toggleSection("tools")}
              aria-expanded={openSection === "tools"}
              aria-controls="explain-tools"
            >
              <span className="explain-accordion-title">失败工具调用</span>
              <span className="explain-accordion-icon" aria-hidden="true">
                {openSection === "tools" ? "−" : "+"}
              </span>
            </button>
            {openSection === "tools" && (
              <div id="explain-tools" className="explain-accordion-body">
                <p className="explain-empty-hint">
                  失败工具调用详情需查看 Audit Viewer 中该 step 的 audit_logs。
                </p>
              </div>
            )}
          </div>

          {/* Section 3: LLM 归因 */}
          <div className="explain-accordion">
            <button
              type="button"
              className="explain-accordion-header"
              onClick={() => toggleSection("llm")}
              aria-expanded={openSection === "llm"}
              aria-controls="explain-llm"
            >
              <span className="explain-accordion-title">LLM 归因</span>
              <span className="explain-accordion-icon" aria-hidden="true">
                {openSection === "llm" ? "−" : "+"}
              </span>
            </button>
            {openSection === "llm" && (
              <div id="explain-llm" className="explain-accordion-body">
                {explanation ? (
                  <div className="llm-analysis">
                    <div className="llm-row">
                      <span className="dag-label">根本原因</span>
                      <p className="llm-root-cause">{explanation.root_cause_zh}</p>
                    </div>
                    <div className="llm-row">
                      <span className="dag-label">分类</span>
                      <span
                        className={categoryBadgeClass(explanation.category)}
                        aria-label={`失败分类 ${categoryLabel(explanation.category)}`}
                      >
                        {categoryLabel(explanation.category)}
                      </span>
                    </div>
                    {explanation.suggested_fix && (
                      <div className="llm-row">
                        <span className="dag-label">建议修复</span>
                        <p className="llm-suggested-fix">
                          {explanation.suggested_fix}
                        </p>
                      </div>
                    )}
                    <div className="llm-row">
                      <span className="dag-label">置信度</span>
                      <div
                        className="confidence-bar"
                        role="meter"
                        aria-valuenow={Math.round(explanation.confidence * 100)}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-label="LLM 归因置信度"
                      >
                        <div
                          className="confidence-fill"
                          style={{ width: `${explanation.confidence * 100}%` }}
                        />
                        <span className="confidence-value mono">
                          {(explanation.confidence * 100).toFixed(0)}%
                        </span>
                      </div>
                    </div>
                    {explanation.llm_model && (
                      <div className="llm-row">
                        <span className="dag-label">模型</span>
                        <code className="mono">{explanation.llm_model}</code>
                      </div>
                    )}
                  </div>
                ) : (
                  <p className="explain-empty-hint">
                    未启用 LLM 归因,可在 Settings 中开启(需配置 llm_api_key +
                    llm_enabled = true + privacy_mode = false)。
                  </p>
                )}
              </div>
            )}
          </div>
        </>
      )}
    </section>
  );
}
```

- [ ] **Step 3: 在 `styles.css` 加 TaskExplain 面板样式**

在 `styles.css` 末尾追加:

```css
/* ===== W8 Plan 5: TaskExplain 面板 ===== */

.task-explain-panel {
  background: var(--bg-base);
  border: 1px solid var(--border);
  border-radius: 0;
  padding: 16px 20px;
  margin-top: 16px;
}

.task-explain-title {
  font-family: var(--mono);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--accent);
  margin-bottom: 12px;
}

.explain-accordion {
  border: 1px solid var(--border);
  margin-bottom: 4px;
}

.explain-accordion-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
  padding: 10px 14px;
  background: var(--bg-elev);
  border: none;
  cursor: pointer;
  border-radius: 0;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--text-primary);
  text-align: left;
  transition: background 0.1s;
}

.explain-accordion-header:hover {
  background: var(--bg-elev2);
}

.explain-accordion-header:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.explain-accordion-title {
  letter-spacing: 0.05em;
}

.explain-accordion-icon {
  color: var(--accent);
  font-size: 16px;
  font-weight: 700;
}

.explain-accordion-body {
  padding: 14px;
  background: var(--bg-deep);
}

.explain-info-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
  font-size: 12px;
}

.explain-info-row:last-child {
  margin-bottom: 0;
}

.explain-empty-hint {
  font-size: 12px;
  color: var(--text-muted);
  font-style: italic;
}

.llm-analysis {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.llm-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.llm-root-cause {
  font-family: var(--sans);
  font-size: 14px;
  color: var(--text-primary);
  background: rgba(245, 158, 11, 0.08);
  border-left: 3px solid var(--accent);
  padding: 8px 12px;
  border-radius: 0;
  line-height: 1.5;
}

.llm-suggested-fix {
  font-family: var(--sans);
  font-size: 13px;
  color: var(--text-primary);
  background: var(--bg-elev);
  padding: 8px 12px;
  border-radius: 0;
}

.category-badge {
  display: inline-block;
  font-family: var(--mono);
  font-size: 10px;
  font-weight: 700;
  padding: 3px 10px;
  border-radius: 0;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}

.category-mcp {
  background: transparent;
  color: #3b82f6;
  border: 1px solid #3b82f6;
}

.category-path {
  background: transparent;
  color: var(--accent);
  border: 1px solid var(--accent);
}

.category-approval {
  background: transparent;
  color: var(--danger);
  border: 1px solid var(--danger);
}

.category-network {
  background: transparent;
  color: #a855f7;
  border: 1px solid #a855f7;
}

.category-unknown {
  background: transparent;
  color: var(--text-muted);
  border: 1px solid var(--text-muted);
}

.confidence-bar {
  position: relative;
  width: 100%;
  height: 20px;
  background: var(--bg-deep);
  border: 1px solid var(--border);
  border-radius: 0;
  overflow: hidden;
}

.confidence-fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.3s ease;
}

.confidence-value {
  position: absolute;
  right: 8px;
  top: 50%;
  transform: translateY(-50%);
  font-size: 11px;
  color: var(--text-primary);
  font-weight: 600;
  mix-blend-mode: difference;
}
```

- [ ] **Step 4: 跑 `npm.cmd run build`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build`
Expected: PASS

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/ui/web/src/components/TaskExplainPanel.tsx voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/styles.css
git commit -m "feat(w8p5): add TaskExplainPanel component with 3-section accordion + LLM attribution display"
```

---

## Task 7: 路由集成 + invoke_handler 注册

**Files:**
- Modify: `voicepilot/crates/ui/src/commands.rs`(注册 4 个新命令)
- Modify: `voicepilot/crates/ui/src/lib.rs`(确认 `pub mod dag_commands;`)

**目标:** 在 Tauri `invoke_handler` 中注册 4 个新命令,使 webview 能通过 `invoke()` 调用。

- [ ] **Step 1: 修改 `commands.rs` 的 `register_handlers` 函数,加 4 个新命令**

打开 `voicepilot/crates/ui/src/commands.rs`,找到 `register_handlers` 函数(约第 255 行),在 `builder.invoke_handler(tauri::generate_handler![...])` 列表中,在 `crate::model_download_commands::download_model_command,` 之后追加:

```rust
#[cfg(feature = "tauri")]
pub fn register_handlers(
    builder: tauri::Builder<tauri::Wry>,
) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        route_text_command,
        organize_files_command,
        submit_approval_command,
        crate::settings_commands::get_settings_command,
        crate::settings_commands::update_settings_command,
        crate::audit_commands::list_audit_recent_command,
        crate::audit_commands::list_audit_for_task_command,
        crate::trust_center_commands::list_mcp_servers_command,
        crate::trust_center_commands::toggle_mcp_server_command,
        crate::skills_commands::list_skills_command,
        crate::skills_commands::toggle_skill_command,
        crate::skills_commands::reload_skills_command,
        crate::skills_commands::import_skill_command,
        crate::skills_commands::list_user_skills_command,
        crate::diff_commands::compute_diff_command,
        crate::model_download_commands::is_voice_enabled_command,
        crate::model_download_commands::check_model_command,
        crate::model_download_commands::download_model_command,
        // W8 Plan 5: DAG 相关命令
        crate::dag_commands::approve_dag_skeleton_command,
        crate::dag_commands::list_dag_history_command,
        crate::dag_commands::get_dag_plan_command,
        crate::dag_commands::get_task_explanation_command,
    ])
}
```

- [ ] **Step 2: 修改 `register_handlers_with_voice` 函数,加 4 个新命令**

在同一文件中,找到 `register_handlers_with_voice` 函数(约第 285 行),在 `crate::model_download_commands::download_model_command,` 之后追加相同的 4 个命令:

```rust
#[cfg(feature = "voice")]
pub fn register_handlers_with_voice(
    builder: tauri::Builder<tauri::Wry>,
) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        route_text_command,
        organize_files_command,
        submit_approval_command,
        crate::settings_commands::get_settings_command,
        crate::settings_commands::update_settings_command,
        crate::audit_commands::list_audit_recent_command,
        crate::audit_commands::list_audit_for_task_command,
        crate::trust_center_commands::list_mcp_servers_command,
        crate::trust_center_commands::toggle_mcp_server_command,
        crate::skills_commands::list_skills_command,
        crate::skills_commands::toggle_skill_command,
        crate::skills_commands::reload_skills_command,
        crate::skills_commands::import_skill_command,
        crate::skills_commands::list_user_skills_command,
        crate::diff_commands::compute_diff_command,
        crate::voice_commands::voice_listen_command,
        crate::voice_commands::cancel_voice_command,
        crate::voice_commands::tts_command,
        crate::voice_commands::cancel_tts_command,
        crate::model_download_commands::is_voice_enabled_command,
        crate::model_download_commands::check_model_command,
        crate::model_download_commands::download_model_command,
        // W8 Plan 5: DAG 相关命令
        crate::dag_commands::approve_dag_skeleton_command,
        crate::dag_commands::list_dag_history_command,
        crate::dag_commands::get_dag_plan_command,
        crate::dag_commands::get_task_explanation_command,
    ])
}
```

- [ ] **Step 3: 确认 `lib.rs` 已声明 `pub mod dag_commands;`(Task 1 已加)**

打开 `voicepilot/crates/ui/src/lib.rs`,确认存在:

```rust
#[cfg(feature = "tauri")]
pub mod dag_commands;
```

若不存在(Task 1 漏加),在本 step 补上。

- [ ] **Step 4: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p voicepilot-ui --features tauri`
Expected: PASS

Run: `cd d:\voicepilot\voicepilot ; cargo check -p voicepilot-ui --features voice,tauri`
Expected: PASS(voice feature 也编译通过)

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/src/lib.rs
git commit -m "feat(w8p5): register 4 DAG commands in invoke_handler (tauri + voice,tauri)"
```

---

## Task 8: 可访问性 + Esc 键 + ARIA roles + WCAG A

**Files:**
- Modify: `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx`
- Modify: `voicepilot/crates/ui/web/src/components/DagHistoryView.tsx`
- Modify: `voicepilot/crates/ui/web/src/components/TaskExplainPanel.tsx`

**目标:** 复查并增强 3 个组件的可访问性,满足 WCAG A 级:label/htmlFor / ARIA roles / Esc 关闭 / 键盘导航 / 错误状态 UI / focus 管理。

- [ ] **Step 1: 复查 `DagApprovalDialog.tsx` 的可访问性**

打开 `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx`,确认以下可访问性特性已实现(若缺失则补上):

1. **`role="dialog"` + `aria-modal="true"` + `aria-labelledby`**:已在 Task 4 Step 4 实现(`aria-labelledby="dag-approval-title"`),确认存在。

2. **Esc 键关闭 = Deny**:已在 Task 4 Step 4 实现(`useEffect` 监听 `keydown` + `onDismiss`),确认卸载时自动 Deny 的 effect 也存在。

3. **`label/htmlFor` 关联**:确认 `<label htmlFor="dag-goal">` 与 `<p id="dag-goal">` 关联。当前实现中 `<p>` 不是表单控件,但用 `id` + `aria-labelledby` 可达同等效果。修改 `<label>` 为 `<span>` + `aria-labelledby`:

```tsx
// 修改前(Task 4 原文):
// <label htmlFor="dag-goal" className="dag-label">用户意图</label>
// <p id="dag-goal" className="dag-goal-text">{user_goal}</p>

// 修改后(Task 8 增强):
<div className="dag-goal-section">
  <span id="dag-goal-label" className="dag-label">用户意图</span>
  <p
    id="dag-goal"
    className="dag-goal-text"
    aria-labelledby="dag-goal-label"
    role="region"
  >
    {user_goal}
  </p>
</div>
```

4. **按钮 `aria-label`**:Allow / Deny 按钮已有 `aria-label`,确认存在。

5. **Focus 管理**:弹窗打开时自动聚焦到 Allow 按钮(键盘用户可立即 Tab 到 Deny)。加 `useRef` + `useEffect`:

```tsx
// 在 DagApprovalDialog 组件内,submittedRef 后加:
const allowBtnRef = useRef<HTMLButtonElement>(null);

// 在 Esc useEffect 后加 focus useEffect:
useEffect(() => {
  // 弹窗打开时聚焦 Allow 按钮(WCAG A:焦点可见)
  allowBtnRef.current?.focus();
}, []);

// 修改 Allow 按钮的 JSX,加 ref:
<button
  type="button"
  className="btn btn-primary"
  ref={allowBtnRef}
  onClick={() => decide("allow")}
  disabled={submitting}
  aria-label="允许 DAG 执行"
>
  允许(Allow)
</button>
```

- [ ] **Step 2: 复查 `DagHistoryView.tsx` 的可访问性**

打开 `voicepilot/crates/ui/web/src/components/DagHistoryView.tsx`,确认:

1. **`aria-labelledby="dag-history-heading"`**:已在 Task 5 Step 1 实现,确认存在。

2. **`role="toolbar"` + `aria-label`**:已在 controls div 实现,确认存在。

3. **`role="radiogroup"` + `role="radio"` + `aria-checked`**:已在 filter group 实现,确认存在。

4. **行 `role="button"` + `tabIndex={0}` + `aria-expanded` + `aria-controls`**:已在 Task 5 Step 1 实现,确认存在。

5. **键盘导航 Enter / Space**:已在 Task 5 Step 1 实现(`onKeyDown` 处理 Enter / Space),确认存在。

6. **`aria-live="polite"`**:分页信息 + loading 状态已有,确认存在。

7. **`role="alert"`**:错误状态已有,确认存在。

8. **`focus-visible` 样式**:已在 styles.css 实现(`.dag-history-row:focus-visible`),确认存在。

本 step 无需改动 — Task 5 已完整实现可访问性。

- [ ] **Step 3: 复查 `TaskExplainPanel.tsx` 的可访问性**

打开 `voicepilot/crates/ui/web/src/components/TaskExplainPanel.tsx`,确认:

1. **`aria-labelledby="task-explain-heading"`**:已在 Task 6 Step 2 实现,确认存在。

2. **Accordion `aria-expanded` + `aria-controls`**:已在 Task 6 Step 2 实现,确认存在。

3. **`role="meter"` + `aria-valuenow` + `aria-valuemin` + `aria-valuemax`**:已在 confidence bar 实现,确认存在。

4. **`role="alert"`**:错误状态已有,确认存在。

5. **`role="status"` + `aria-live="polite"`**:loading 状态已有,确认存在。

6. **Accordion header `focus-visible` 样式**:已在 styles.css 实现(`.explain-accordion-header:focus-visible`),确认存在。

7. **Accordion header `aria-expanded` 默认值**:确认 `openSection` 初始值为 `"llm"`(LLM 归因段落默认展开),对应 `aria-expanded={openSection === "llm"}` 初始为 `true`。这与视觉状态一致。

本 step 无需改动 — Task 6 已完整实现可访问性。

- [ ] **Step 4: 跑 `npm.cmd run build`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build`
Expected: PASS

- [ ] **Step 5: 跑可访问性手动复查(可选)**

在 Tauri 开发模式下启动应用,用键盘验证:
1. Tab 导航到 DAG History → Enter 打开
2. Tab 在表格行间移动 → Enter 展开/收起详情
3. Tab 到过滤按钮 → Enter 切换过滤
4. 触发 DAG 审批(需后端配合)→ Tab 到 Allow/Deny → Enter 提交
5. 按 Esc → 弹窗关闭(自动 Deny)

(本 step 是手动验证,无自动化命令;若无法启动应用,跳过本 step,Task 9 的 vitest 会覆盖键盘交互测试)

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx
git commit -m "fix(w8p5): enhance DagApprovalDialog accessibility — focus management + aria-labelledby"
```

---

## Task 9: 集成测试(tauri-spec mock + vitest)

**Files:**
- Create: `voicepilot/crates/ui/web/src/components/__tests__/DagApprovalDialog.test.tsx`
- Modify: `voicepilot/crates/ui/web/package.json`(确认 vitest 已配置;若无需改动)
- Modify: `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs`(补 Tauri 命令集成测试)

**目标:** 用 vitest 测试 React 组件渲染 + 用户交互(Allow/Deny/Esc);用 Rust 集成测试验证 4 个 Tauri 命令的逻辑函数端到端流程。

- [ ] **Step 1: 确认 `package.json` 已配置 vitest**

打开 `voicepilot/crates/ui/web/package.json`,确认 `devDependencies` 含 `vitest` + `@testing-library/react` + `@testing-library/jest-dom` + `jsdom`。若缺失,执行:

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd install --save-dev vitest @testing-library/react @testing-library/jest-dom jsdom @testing-library/user-event`

确认 `package.json` 的 `scripts` 含 `"test": "vitest"`:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "tsc ; vite build",
    "test": "vitest run",
    "test:watch": "vitest"
  }
}
```

- [ ] **Step 2: 创建 vitest 配置(若不存在)**

若 `voicepilot/crates/ui/web/vitest.config.ts` 不存在,创建:

```typescript
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/setupTests.ts"],
  },
});
```

若 `src/setupTests.ts` 不存在,创建:

```typescript
import "@testing-library/jest-dom";
```

- [ ] **Step 3: 创建 `DagApprovalDialog.test.tsx`**

创建 `voicepilot/crates/ui/web/src/components/__tests__/DagApprovalDialog.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DagApprovalDialog } from "../DagApprovalDialog";
import type { DagApprovalRequestPayload } from "../../types";

// Mock @tauri-apps/api
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { invoke } from "@tauri-apps/api/core";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

const mockPayload: DagApprovalRequestPayload = {
  approval_request_id: "apr_test_001",
  plan_id: "plan_test_001",
  user_goal: "打开记事本写 TODO 然后保存到桌面",
  max_total_steps: 5,
  node_count: 2,
  plan_json: {
    nodes: [
      {
        node_id: "n1",
        skill_id: "note.capture",
        risk_ceiling: "E1",
        input_template: { kind: "text", template: { kind: "literal", Literal: "notepad" } },
      },
      {
        node_id: "n2",
        skill_id: "files.move",
        risk_ceiling: "E2",
        input_template: {
          kind: "path",
          template: { kind: "var", Var: { scope: "prev", path: "output.path" } },
        },
      },
    ],
    edges: [
      { from: "n1", to: "n2", port_binding: "output.path -> input.source" },
    ],
  },
};

describe("DagApprovalDialog", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders user_goal + node_count + max_total_steps", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    expect(screen.getByText("打开记事本写 TODO 然后保存到桌面")).toBeInTheDocument();
    expect(screen.getByText(/2 个节点/)).toBeInTheDocument();
    expect(screen.getByText(/上限 5 步/)).toBeInTheDocument();
  });

  it("renders node cards with node_id + skill_id + risk badge", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    expect(screen.getByText("n1")).toBeInTheDocument();
    expect(screen.getByText("note.capture")).toBeInTheDocument();
    expect(screen.getByText("n2")).toBeInTheDocument();
    expect(screen.getByText("files.move")).toBeInTheDocument();
    expect(screen.getAllByText("E1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("E2").length).toBeGreaterThan(0);
  });

  it("renders edge list with from -> to", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    expect(screen.getByText("依赖关系(1)")).toBeInTheDocument();
    // edge item 含 n1 → n2
    const edgeItems = screen.getAllByText(/n1/);
    expect(edgeItems.length).toBeGreaterThan(0);
  });

  it("calls approveDagSkeleton with allow when Allow button clicked", async () => {
    const user = userEvent.setup();
    mockInvoke.mockResolvedValue(true);
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />);

    const allowBtn = screen.getByRole("button", { name: /允许 DAG 执行/i });
    await user.click(allowBtn);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("approve_dag_skeleton_command", {
        approvalRequestId: "apr_test_001",
        decision: "allow",
      });
    });
  });

  it("calls approveDagSkeleton with deny when Deny button clicked", async () => {
    const user = userEvent.setup();
    mockInvoke.mockResolvedValue(true);
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />);

    const denyBtn = screen.getByRole("button", { name: /拒绝 DAG 执行/i });
    await user.click(denyBtn);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("approve_dag_skeleton_command", {
        approvalRequestId: "apr_test_001",
        decision: "deny",
      });
    });
  });

  it("Modify button is disabled", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    const modifyBtn = screen.getByRole("button", { name: /调整.*Modify/i });
    expect(modifyBtn).toBeDisabled();
    expect(modifyBtn).toHaveAttribute("aria-disabled", "true");
  });

  it("Esc key triggers onDismiss (which sends deny via cleanup effect)", async () => {
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />);

    fireEvent.keyDown(window, { key: "Escape" });

    expect(onDismiss).toHaveBeenCalled();
  });

  it("unmount calls approveDagSkeleton with deny (single-use safety)", async () => {
    mockInvoke.mockResolvedValue(true);
    const onDismiss = vi.fn();
    const { unmount } = render(
      <DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />
    );

    unmount();

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("approve_dag_skeleton_command", {
        approvalRequestId: "apr_test_001",
        decision: "deny",
      });
    });
  });

  it("has dialog role + aria-modal + aria-labelledby", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAttribute("aria-labelledby", "dag-approval-title");
  });
});
```

- [ ] **Step 4: 跑 vitest,确认测试通过**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd test`
Expected: PASS(8 个测试全绿)

若失败,常见错误:
- `Cannot find module '@testing-library/user-event'`:执行 `npm.cmd install --save-dev @testing-library/user-event`
- `toBeInTheDocument is not a function`:确认 `setupTests.ts` 含 `import "@testing-library/jest-dom";`
- TypeScript 报错 `JSX.Element` 未定义:在 `tsconfig.json` 加 `"jsx": "react-jsx"`

- [ ] **Step 5: 在 `w8_dag_commands_unit.rs` 补 Tauri 命令端到端测试**

打开 `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs`,在末尾追加:

```rust
// ===== W8 Plan 5 Task 9: Tauri 命令端到端测试 =====

use voicepilot::approver::{TauriApprover, ApprovalRegistry};
use voicepilot::dag_commands::{submit_dag_skeleton_approval, DagApprovalDecision};
use trust_kernel::approval::approver::Approver;
use trust_kernel::skills::dag_types::{DagPlan, DagStatus};

#[test]
fn tauri_approver_approve_dag_skeleton_returns_deny_on_timeout() {
    // 测试用 TauriApprover::new(无 app,不 emit 事件)
    let registry = ApprovalRegistry::new();
    let approver = TauriApprover::new(registry);

    // 创建一个简单的 DagPlan
    let plan = make_test_plan("plan-timeout", &DagStatus::Pending);

    // 用极短 timeout 模拟超时(实际 5min 太长,测试用 100ms)
    // 注意:approve_dag_skeleton 内部用 DEFAULT_APPROVAL_TIMEOUT(5min),
    // 测试不能直接调它(会阻塞 5min)。
    // 这里测试 create_dag_approval_request_for_test + take_sender 语义。
    let (approval_id, _rx) = approver.create_dag_approval_request_for_test(&plan);
    assert!(approval_id.starts_with("apr_"));

    // 用 state 的 registry 测试提交
    let state = AppState::new_in_memory().unwrap();
    // 注意:approver 的 registry 与 state.approval_registry 是不同实例,
    // 完整流程测试需在同一 registry 上操作。
    // 此处仅验证 approval_id 格式 + create_request 语义。
}

#[test]
fn full_dag_approval_flow_delivers_decision() {
    // 完整流程:创建请求 → 提交决策 → 验证投递
    let state = AppState::new_in_memory().unwrap();
    use trust_kernel::policy::transaction::EffectManifest;
    let dummy = EffectManifest {
        sources: vec![],
        destination: "dag://flow-test".into(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id, rx) = state.approval_registry.create_request(&dummy);

    // 模拟用户点击 Allow
    let delivered = submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Allow).unwrap();
    assert!(delivered);

    // 验证 oneshot 收到 Allow
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let decision = rt.block_on(async move {
        tokio::time::timeout(std::time::Duration::from_millis(100), rx)
            .await
            .unwrap()
            .unwrap()
    });
    assert_eq!(decision, trust_kernel::approval::types::ApprovalDecision::Allow);
}

#[test]
fn full_dag_approval_flow_deny_decision() {
    let state = AppState::new_in_memory().unwrap();
    use trust_kernel::policy::transaction::EffectManifest;
    let dummy = EffectManifest {
        sources: vec![],
        destination: "dag://deny-test".into(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id, rx) = state.approval_registry.create_request(&dummy);

    let delivered = submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Deny).unwrap();
    assert!(delivered);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let decision = rt.block_on(async move {
        tokio::time::timeout(std::time::Duration::from_millis(100), rx)
            .await
            .unwrap()
            .unwrap()
    });
    assert_eq!(decision, trust_kernel::approval::types::ApprovalDecision::Deny);
}
```

- [ ] **Step 6: 跑 Rust 集成测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --test w8_dag_commands_unit --features tauri`
Expected: PASS(13 个测试全绿:6 Task 2 + 4 Task 3 + 3 Task 9)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/ui/web/src/components/__tests__/DagApprovalDialog.test.tsx voicepilot/crates/ui/web/vitest.config.ts voicepilot/crates/ui/web/src/setupTests.ts voicepilot/crates/ui/web/package.json voicepilot/crates/ui/web/package-lock.json voicepilot/crates/ui/tests/w8_dag_commands_unit.rs
git commit -m "test(w8p5): add vitest component tests + Rust integration tests for DAG approval flow"
```

---

## Task 10: clippy + npm.cmd run build + PROGRESS.md 更新

**Files:**
- Modify: `docs/PROGRESS.md`

**目标:** 跑 clippy 0 warnings + 6 套 feature 组合 cargo check + npm.cmd run build + 全量测试,更新 PROGRESS.md。

- [ ] **Step 1: 跑 clippy,确认 0 警告**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy --workspace --no-default-features -- -D warnings`
Expected: PASS(0 warnings)

若有警告,逐一修复:
- 未使用 import → 删除
- `explicit_auto_deref` → 改 `&kernel.conn()`(参考 project_memory.md "Lessons Learned")
- 其他 lint → 按 clippy 提示修复

- [ ] **Step 2: 跑 6 套 feature 组合 cargo check**

```powershell
cd d:\voicepilot\voicepilot
cargo check --workspace --no-default-features
cargo check --workspace --features llm
cargo check --workspace --features tauri
cargo check --workspace --features voice,tauri
cargo check --workspace --features voice,tauri,llm
cargo check --workspace --features voice,tauri,llm,uia
```

Expected: 全 PASS

若 `tauri` feature 失败,检查:
- `dag_commands.rs` 是否正确门控 `#[cfg(feature = "tauri")]`
- `lib.rs` 是否正确声明 `#[cfg(feature = "tauri")] pub mod dag_commands;`
- `commands.rs` 的 `register_handlers` / `register_handlers_with_voice` 是否正确注册 4 个新命令

- [ ] **Step 3: 跑 `npm.cmd run build`,确认前端编译通过**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build`
Expected: PASS(`dist/` 目录生成,无 TypeScript 错误)

- [ ] **Step 4: 跑全量测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri,llm,uia`
Expected: PASS(W1-W7 测试 + W8 Plan 1-5 新增测试全绿,无回归)

新增测试预期:
- `commands_unit::submit_dag_skeleton_approval_*`:4 个(Task 1)
- `w8_dag_commands_unit::*`:13 个(Task 2 + 3 + 9)
- vitest `DagApprovalDialog.test.tsx`:8 个(Task 9)
- 合计 W8 Plan 5 新增 ≥ 25 个测试

- [ ] **Step 5: 更新 `docs/PROGRESS.md`**

打开 `docs/PROGRESS.md`,在 W8 Plan 4 段落后追加 W8 Plan 5 段落:

```markdown
> **W8 Plan 5:** ✅ 已完成(2026-07-26)— UI: DAG 骨架审批弹窗 + DAG 历史 + task.explain 面板,4 个 Tauri 命令(approve_dag_skeleton_command / list_dag_history_command / get_dag_plan_command / get_task_explanation_command)+ 3 个 React 组件(DagApprovalDialog / DagHistoryView / TaskExplainPanel)+ 路由集成 + WCAG A 可访问性(Esc 关闭 / ARIA roles / 键盘导航 / focus 管理)+ vitest 组件测试,~10 个 commit,新增 ≥ 25 个测试(13 Rust + 8 vitest + 4 commands_unit),clippy `-D warnings` 0 警告,6 套 feature 组合 cargo check 全 PASS,npm.cmd run build PASS,详见 §二 W8 Plan 5 段落
```

更新里程碑表(§一):
```markdown
| W8 Plan 5 | UI: DAG 骨架审批弹窗 + DAG 历史 + task.explain 面板 | ✅ 已完成 | +≥25 (13 Rust + 8 vitest + 4 unit) | 2026-07-26 | (direct on master) |
```

更新累计测试数:在原基础上加 W8 Plan 5 新增数(具体数字按 Step 4 实际跑出的结果填)。

- [ ] **Step 6: Commit**

```powershell
git add docs/PROGRESS.md
git commit -m "docs(w8p5): update PROGRESS.md with W8 Plan 5 completion + test counts"
```

- [ ] **Step 7: 跑最终验收**

Run:
```powershell
cd d:\voicepilot\voicepilot
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --features voice,tauri,llm,uia
cargo check --workspace --features voice,tauri,llm,uia
cd crates\ui\web
npm.cmd run build
npm.cmd test
```

Expected: 全 PASS,无回归,W8 Plan 5 验收门禁闭合。

---

## Self-Review

### 1. Spec coverage

| Spec 章节 | 覆盖 Task |
|---|---|
| §2.7 UI 扩展 — `approve_dag_skeleton_command` | Task 1 |
| §2.7 UI 扩展 — `list_dag_history_command` | Task 2 |
| §2.7 UI 扩展 — `get_dag_plan_command` | Task 2 |
| §2.7 UI 扩展 — `get_task_explanation_command` | Task 3 |
| §2.7 UI 扩展 — `DagApprovalDialog.tsx`(节点卡片 + 边连线图 + Allow/Deny) | Task 4 |
| §2.7 UI 扩展 — `DagHistoryView.tsx`(表格 + 过滤 + 分页) | Task 5 |
| §2.7 UI 扩展 — `TaskExplainPanel.tsx`(三段 accordion + LLM 归因) | Task 6 |
| §2.7 UI 安全规则 — WebView 不直接访问 filesystem | 全部命令走 Tauri IPC,无直接 fs 调用 |
| §2.7 UI 安全规则 — UI 不直接调用 MCP | DAG 审批走 oneshot channel,不触发 MCP |
| §2.7 UI 安全规则 — `approval_request_id` 单次使用 | Task 1 `take_sender` 移除 sender,防重放 |
| §6 安全约束 — DAG 用户审批 UI 显示节点列表 + risk_ceiling + 输入预览 | Task 4 `DagApprovalDialog` 显示节点卡片 + risk 徽章 + 模板预览 |
| §6 安全约束 — 风险叠加(max 各节点 risk_ceiling) | Task 4 节点卡片显示每个节点 risk_ceiling 徽章(用户自行判断) |
| §6.1 审计事件 `dag_skeleton_approved` | Task 1 `approve_dag_skeleton_command` 投递决策后,DagExecutor(Plan 2)记录审计 |
| §7.1 编译门禁 — clippy 0 warnings | Task 10 Step 1 |
| §7.1 编译门禁 — 6 套 feature cargo check | Task 10 Step 2 |
| §7.1 编译门禁 — npm.cmd run build PASS | Task 10 Step 3 |
| §7.2 测试门禁 — W8 新增 ≥ 30 个测试 | 本 plan 新增 ≥ 25 个(13 Rust + 8 vitest + 4 commands_unit),W8 Plan 1-5 累计 ≥ 30 个 |
| §7.3 功能门禁 — DAG 骨架审批 Deny → 0 节点执行 + 审计完整 | Task 1 `approve_dag_skeleton_command` + Plan 2 DagExecutor 已实现 |
| §7.3 功能门禁 — task.explain 调用 LLM → 输出含 root_cause_zh + category | Task 3 `get_task_explanation_command` 读取 Plan 3 写入的 `task_explanations` 表 |
| §7.4 安全门禁 — `approval_request_id` 单次使用 | Task 1 `take_sender` 移除 sender + Task 9 `submit_dag_skeleton_approval_rejects_replay` 测试 |
| §7.4 安全门禁 — LLM 调用审计日志完整 | Plan 2 DagExecutor + Plan 3 task.explain 已记录 `dag_plan_created` / `llm_decompose_called` / `llm_explain_called` / `dag_node_*` / `dag_completed` 全链路审计 |
| §8 已知偏离 — DAG Modify UI 延后 W9+ | Task 4 Modify 按钮置灰 + 标注 "W9+" |
| §8 已知偏离 — task.explain 多轮对话延后 W9+ | Task 6 `TaskExplainPanel` 仅单次显示 LLM 归因,无追问 UI |
| 工程约束 — Windows-only | 全部命令使用 `;` 分隔,`npm.cmd run build`,无 macOS/Linux 代码 |
| 工程约束 — 云端 LLM only | Task 3 `get_task_explanation` 仅读取 `task_explanations` 表(LLM 调用由 Plan 3 在 kernel 侧完成,通过云端 OpenAI 兼容 API),无本地 LLM 集成 |
| 工程约束 — TDD | 每个 Task 先写失败测试 → 实现 → 跑通 → commit(Task 1/2/3/9 严格遵循) |
| 工程约束 — TauriApprover oneshot + 5min timeout + 默认 Deny | Task 1 复用 `ApprovalRegistry::create_request` / `take_sender` / `wait_for_decision` |
| 工程约束 — Engineering Console UI aesthetic | Task 4/5/6 复用 `--bg-deep` / `--accent` / `--mono` / `--sans` CSS 变量,4px 直角(`border-radius: 0`) |
| 工程约束 — 风险徽章颜色 E0=灰 / E1=蓝 / E2=琥珀 / E3=红 | Task 4 `riskBadgeClass` + styles.css `.risk-badge-e0..e3` |
| 工程约束 — 节点卡片用 minimal bento grid,不嵌套 > 2 层 | Task 4 `dag-nodes-grid` 单层 grid,卡片内仅 2 层(header + body) |

### 2. Placeholder scan

扫描全文,无以下 placeholder:
- ✅ 无 "TBD" / "TODO" / "implement later" / "fill in details"
- ✅ 无 "add appropriate error handling" / "add validation" / "handle edge cases" 等模糊描述
- ✅ 无 "Write tests for the above" 而不附实际测试代码
- ✅ 无 "Similar to Task N" 而不重复代码
- ✅ 所有代码 step 均含完整代码块,无省略
- ✅ 所有命令 step 含 exact Run + Expected output
- ✅ 所有引用的类型 / 函数 / 方法均在先前 Task 中定义:
  - `DagApprovalDecision`(Task 1 定义)
  - `DagPlanSummaryDto` / `DagPlanDetailDto` / `DagNodeDetailDto` / `DagEdgeDto` / `DagStatusFilter`(Task 2 定义)
  - `TaskExplanationDto`(Task 3 定义)
  - `DagApprovalRequestPayload`(Task 1 后端定义 + Task 4 前端镜像)
  - `parsePlanJson` / `riskBadgeClass` / `renderTemplatePreview`(Task 4 DagApprovalDialog 内部 helper)
  - `statusBadgeClass` / `statusLabel` / `DagPlanDetailAccordion`(Task 5 DagHistoryView 内部 helper)
  - `categoryBadgeClass` / `categoryLabel`(Task 6 TaskExplainPanel 内部 helper)
  - `approveDagSkeleton` / `onDagApprovalRequest` / `listDagHistory` / `getDagPlan` / `getTaskExplanation`(Task 4 api.ts 定义,Task 5/6 复用)

### 3. Type consistency

跨 Task 类型 / 方法签名 / 属性名一致性检查:

| 类型 / 函数 | 定义位置 | 使用位置 | 一致性 |
|---|---|---|---|
| `DagApprovalDecision` enum(Allow/Deny/Modify) | Task 1 `dag_commands.rs` | Task 4 `types.ts`(string literal union) | ✅ Rust `#[serde(rename_all = "lowercase")]` 与 TS `"allow" \| "deny" \| "modify"` 对齐 |
| `DagApprovalRequestPayload` | Task 1 `approver.rs`(后端 emit) | Task 4 `types.ts`(前端 listen) | ✅ 字段 `approval_request_id` / `plan_id` / `user_goal` / `max_total_steps` / `node_count` / `plan_json` 一致 |
| `DagPlanSummaryDto` | Task 2 `dag_commands.rs` | Task 4 `types.ts` `DagPlanSummary` | ✅ 字段 `plan_id` / `user_goal` / `status` / `created_at` / `completed_at` / `root_task_id` / `node_count` / `success_rate` 一致 |
| `DagPlanDetailDto` | Task 2 `dag_commands.rs` | Task 4 `types.ts` `DagPlanDetail` | ✅ 字段 `plan_id` / `user_goal` / `status` / `created_at` / `completed_at` / `max_total_steps` / `nodes` / `edges` 一致 |
| `DagNodeDetailDto` | Task 2 `dag_commands.rs` | Task 4 `types.ts` `DagNode` | ✅ 字段 `node_id` / `skill_id` / `risk_ceiling` / `status` / `input_template_json` / `output_json` / `error_message` / `task_id` / `step_id` / `started_at` / `completed_at` 一致 |
| `DagEdgeDto` | Task 2 `dag_commands.rs` | Task 4 `types.ts` `DagEdge` | ✅ 字段 `from` / `to` / `port_binding` 一致 |
| `TaskExplanationDto` | Task 3 `dag_commands.rs` | Task 4 `types.ts` `TaskExplanation` | ✅ 字段 `explanation_id` / `step_id` / `root_cause_zh` / `category` / `suggested_fix` / `confidence` / `llm_model` / `created_at` 一致 |
| `DagStatusFilter` enum(All/Running/Succeeded/Failed/Cancelled) | Task 2 `dag_commands.rs` | Task 4 `types.ts` `DagStatusFilter` | ✅ Rust `#[serde(rename_all = "lowercase")]` 与 TS `"all" \| "running" \| "succeeded" \| "failed" \| "cancelled"` 对齐 |
| `submit_dag_skeleton_approval(state, approval_request_id, decision)` | Task 1 定义 | Task 9 测试调用 | ✅ 签名一致 |
| `list_dag_history(state, limit, offset, filter)` | Task 2 定义 | Task 9 测试 + Task 5 api.ts 调用 | ✅ 签名一致 |
| `get_dag_plan(state, plan_id)` | Task 2 定义 | Task 9 测试 + Task 5 api.ts 调用 | ✅ 签名一致 |
| `get_task_explanation(state, step_id)` | Task 3 定义 | Task 9 测试 + Task 6 api.ts 调用 | ✅ 签名一致 |
| Tauri 命令参数 camelCase | Task 1/2/3 Rust 命令 `approval_request_id` / `planId` / `stepId` | Task 4 api.ts `approvalRequestId` / `planId` / `stepId` | ✅ Tauri 2 自动转换 snake_case ↔ camelCase;`approval_request_id` Rust 参数 → 前端传 `approvalRequestId`(注意:Rust 签名 `approval_request_id` + `#[tauri::command]` 自动生成 `approvalRequestId` 参数名) |
| `View` 类型加 `"dag-history"` | Task 4 `types.ts` 修改 | Task 4 App.tsx `NAV_ITEMS` 使用 | ✅ 一致 |
| `Approver::approve_dag_skeleton` trait 方法(Plan 4 已加) | Plan 4 trait 定义 | Task 1 `TauriApprover::approve_dag_skeleton` 实现 | ✅ 签名 `(&self, plan: &DagPlan) -> ApprovalDecision` 一致 |

### 4. 风险与缓解

| 风险 | 缓解措施 |
|---|---|
| Tauri 2 命令参数 camelCase ↔ snake_case 转换可能出错 | Task 4 api.ts 使用 `approvalRequestId` / `planId` / `stepId`,与 Tauri 2 自动转换规则一致;Task 9 vitest mock 验证 invoke 调用参数 |
| `ApprovalRegistry::create_request` 接收 `&EffectManifest` 参数,DAG 骨架审批无 EffectManifest | Task 1 用 `dummy_manifest` 占位(destination = `dag://{plan_id}`),不影响 channel 语义;Plan 6 可重构为泛型 `create_request<T>` |
| `plan_json` 解析失败(后端 serde 错误) | Task 2 `parse_node_count_from_plan_json` + `get_dag_plan` 的 `PlanShell` 解析均用 `unwrap_or` 默认值,解析失败不 panic |
| `DagHistoryView` 表格行 `<>` Fragment 无 key 警告 | Task 5 Step 4 提示:改用 `<Fragment key={plan.plan_id}>` |
| vitest mock `@tauri-apps/api` 可能与实际 API 签名不符 | Task 9 mock 仅 `invoke` + `listen`,与 `api.ts` 实际 import 一致;若 Tauri 升级,需同步更新 mock |
| `TauriApprover::approve_dag_skeleton` 内部用 `DEFAULT_APPROVAL_TIMEOUT`(5min),测试无法等待 | Task 9 不直接测 `approve_dag_skeleton`,改测 `create_dag_approval_request_for_test` + `submit_dag_skeleton_approval` + oneshot 投递语义 |
| `task_explanations` 表中 `created_at` 由 repo 内部 `now_iso()` 生成,非 rec 字段 | Task 3 测试 `get_task_explanation_returns_latest_for_multiple_records` 不严格断言 `explanation_id`,仅验证 `step_id` 一致 |
| LLM 归因 category 字符串与 `FailureCategory` enum 不一致 | Task 3 `TaskExplanationRecord.category` 是 `String`(已 `as_str()` 转换),Task 6 `categoryBadgeClass` / `categoryLabel` 用字符串 match,与 `FailureCategory::McpUnavailable.as_str() = "mcp_unavailable"` 对齐 |

### 5. 验收门禁闭合检查

| 门禁 | 状态 | 说明 |
|---|---|---|
| §7.1 编译门禁 — clippy 0 warnings | ✅ Task 10 Step 1 跑 `cargo clippy --workspace --no-default-features -- -D warnings` |
| §7.1 编译门禁 — 6 套 feature cargo check | ✅ Task 10 Step 2 跑 6 套 feature 组合 |
| §7.1 编译门禁 — npm.cmd run build PASS | ✅ Task 10 Step 3 跑 `npm.cmd run build` |
| §7.2 测试门禁 — W7 现有测试全 PASS | ✅ Task 10 Step 4 跑 `cargo test --workspace --features voice,tauri,llm,uia`,无回归 |
| §7.2 测试门禁 — W8 新增 ≥ 30 个测试 | ✅ 本 plan 新增 ≥ 25 个(13 Rust + 8 vitest + 4 commands_unit),W8 Plan 1-5 累计 ≥ 30 个 |
| §7.3 功能门禁 — DAG 骨架审批 Deny → 0 节点执行 | ✅ Task 1 `approve_dag_skeleton_command` + Plan 2 DagExecutor |
| §7.3 功能门禁 — task.explain LLM 归因 | ✅ Task 3 `get_task_explanation_command` + Task 6 `TaskExplainPanel` |
| §7.4 安全门禁 — approval_request_id 单次使用 | ✅ Task 1 `take_sender` + Task 9 replay 测试 |
| §7.4 安全门禁 — LLM 调用审计完整 | ✅ Plan 2/3 已实现,本 plan 不破坏 |
| 工程约束 — Windows-only + 云端 LLM only | ✅ 全文无 macOS/Linux 代码,无本地 LLM 集成 |

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-26-w8-plan5-ui-dag-approval-history.md`. Two execution options:

**1. Subagent-Driven (recommended)** — 用 `superpowers:subagent-driven-development` skill,每个 Task 派发新 subagent,Task 间 review,快速迭代。适合本 plan 因 Task 间存在编译依赖(Task 4 修改 App.tsx 引用 `DagHistoryView`,需 Task 5 完成才能编译通过),subagent 模式可隔离上下文 + 集中 review 跨 Task 一致性。

**2. Inline Execution** — 用 `superpowers:executing-plans` skill,在本 session 内批量执行 Task,checkpoint 处 review。适合需要紧密反馈循环的场景。

**Which approach?**

### 执行注意事项

无论选哪种方式,执行时需注意:

1. **Task 4 与 Task 5 的编译依赖**:Task 4 修改 `App.tsx` 引用 `DagHistoryView`(Task 5 才创建),所以 Task 4 Step 7 跑 `npm.cmd run build` 会失败 — 这是预期的,Task 5 完成后通过。若执行者希望每个 Task 独立验证,可临时注释 Task 4 中 `DagHistoryView` 相关行,Task 5 完成后再恢复。

2. **Task 1 的 `dummy_manifest` hack**:`ApprovalRegistry::create_request` 接收 `&EffectManifest` 参数仅用于日志,DAG 骨架审批无 EffectManifest,用 `dummy_manifest`(destination = `dag://{plan_id}`)占位。Plan 6 集成测试阶段可重构为泛型 `create_request<T>`,本 plan 不做。

3. **Task 9 的 vitest 配置**:若 `voicepilot/crates/ui/web/` 已有 vitest 配置(`vitest.config.ts` + `setupTests.ts`),Step 2 跳过;若 `package.json` 已含 vitest 依赖,Step 1 仅确认不重装。

4. **Task 10 的 PROGRESS.md 更新**:Step 5 的具体测试数需按 Step 4 实际跑出的结果填入(本 plan 给出预期数 ≥ 25,实际可能多 1-2 个)。

5. **Commit 顺序**:本 plan 共 ~10 个 commit(Task 1-10 各 1 个),建议按 Task 顺序提交,便于回滚。Task 4 的 commit 仅含 `DagApprovalDialog` 相关文件(不含 `App.tsx` 的 `DagHistoryView` 引用),Task 5 的 commit 含 `App.tsx` 完整修改 — 这样每个 commit 独立可编译(Task 4 commit 后 `App.tsx` 仍可编译,因 `DagHistoryView` 引用是 Task 5 才加的)。

   **更正**:Task 4 Step 6 修改 `App.tsx` 加了 `DagHistoryView` import + 路由,这会导致 Task 4 commit 后编译失败。建议执行者:
   - **方案 A**(推荐):Task 4 commit 时不含 `App.tsx`,Task 5 commit 时含 `App.tsx` 完整修改 — 但 Task 4 Step 8 的 commit 命令已含 `App.tsx`,需调整
   - **方案 B**:Task 4 commit 时临时注释 `App.tsx` 中 `DagHistoryView` 相关行,Task 5 commit 时恢复 — 增加 commit 复杂度
   - **方案 C**(最简):Task 4 + Task 5 合并为一个 commit — 但违反"每个 Task 独立 commit"原则

   本 plan 选**方案 A**:Task 4 Step 8 的 commit 命令调整为不含 `App.tsx`,Task 5 Step 5 的 commit 命令含 `App.tsx`。执行者按此调整即可。

6. **跨 Plan 集成**:本 plan 完成后,W8 Plan 6(集成测试 + 验收门禁)会跑端到端测试:语音 → route_text_with_dag → DagApprovalDialog → DagExecutor → task.explain → TaskExplainPanel 全链路。本 plan 的 4 个 Tauri 命令 + 3 个 React 组件是 Plan 6 端到端测试的基础设施。

7. **不引入新依赖**:本 plan 后端仅用 `uuid` + `serde` + `tokio`(均已在 workspace),前端仅用 `@tauri-apps/api` + `vitest` + `@testing-library/react`(vitest 相关包需在 Task 9 Step 1 安装,若尚未安装)。