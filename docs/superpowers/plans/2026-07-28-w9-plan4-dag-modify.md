# W9 Plan 4: DAG Modify 分支实现(后端 + UI + 重新审批)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W9 设计文档(`docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.4 + §6.3)实现 VoicePilot Trust Kernel + Tauri UI 的 DAG 骨架 Modify 分支:扩展 `Approver` trait 签名为 `approve_dag_skeleton(&DagPlan) -> Result<DagApprovalOutcome>`,新增 `DagApprovalOutcome::Modify { modified_plan: Box<DagPlan> }` 变体,在 `DagExecutor::run` 处理 Modify 分支(审计 + 重新校验 + 第二次审批只允许 Allow/Deny),扩展 `submit_dag_skeleton_approval` 命令接收 `modified_plan: Option<DagPlan>`,激活前端 `DagApprovalDialog` Modify 按钮 + 新增 `NodeEditor` 组件(增删节点 + input_template textarea + risk_ceiling select),使用户能调整 LLM 拆解的 DAG 骨架后重新走完整 skeleton 审批。

**Architecture:** 后端在 `crates/trust-kernel/src/approval/approver.rs` 把 `approve_dag_skeleton` 返回类型从 `ApprovalDecision` 改为新的 `DagApprovalOutcome` 枚举(Allow / Deny / Modify { modified_plan: Box<DagPlan> }),AutoApprover / AutoDenier 适配新签名;在 `crates/trust-kernel/src/error.rs` 加 `KernelError::DagModifyLimitExceeded`;在 `crates/trust-kernel/src/skills/dag_executor.rs` 把 `run` 的 Step 2 拆分:遇到 `DagApprovalOutcome::Modify` 时审计 `dag_skeleton_modified` + 调 `SlotTemplateEngine::validate_dag` 重新校验 + 调 `risk_ceiling` 提权检查 + 调用新方法 `run_modified(&modified_plan)`,后者第二次调 `approve_dag_skeleton` 时只匹配 Allow / Deny,遇到 Modify 返回 `KernelError::DagModifyLimitExceeded` 并审计 `dag_modify_limit_exceeded`。UI 后端在 `crates/ui/src/dag_commands.rs` 扩展 `submit_dag_skeleton_approval` 接收 `modified_plan: Option<DagPlan>` 参数,在 `crates/ui/src/approver.rs` 让 `TauriApprover::approve_dag_skeleton` 改用 `ApprovalRegistry::create_dag_request` + 新结构 `DagApprovalPayload { decision, modified_plan }`(oneshot channel + 5min timeout + 默认 Deny,既有 Tauri IPC 三安全规则不变)。UI 前端在 `crates/ui/web/src/components/DagApprovalDialog.tsx` 激活 Modify 按钮 + 新增 `editingMode` state 切换只读 / 编辑模式,在 `crates/ui/web/src/components/NodeEditor.tsx`(新增)提供增删节点 + input_template textarea + risk_ceiling select 控件,在 `api.ts` + `types.ts` 扩展 `DagApprovalDecision` 类型 + `approveDagSkeleton(approvalRequestId, decision, modifiedPlan?)` API。

**Tech Stack:** Rust(stable)+ `serde` + `thiserror` + `tokio::sync::oneshot` + `uuid`(后端);React 18 + TypeScript 5 + Vite + `@tauri-apps/api`(前端);`vitest`(前端单元测试);TDD。

**Spec:** `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.4(Plan 4 范围:DagApprovalOutcome + dag_executor Modify 分支 + TauriApprover 扩展 + UI 编辑器 + 重新审批闭环)+ §6.3(DAG Modify 安全约束:modified_plan 必须 `SlotTemplateEngine::validate_dag` 重新校验 / Modify 只允许一次防无限递归 / risk_ceiling 不能超过原 plan max risk / approval_request_id 单次使用)+ §10(Conventions:PowerShell `;` 分隔 / TDD / 审计事件 lower_snake_case / TauriApprover constructors `new` vs `with_app` / oneshot + 5min timeout + 默认 Deny / commit message `feat(w9p4): ...`)+ §11(兼容性:`DagApprovalDecision::Modify` 占位 → W9 Plan 4 激活,Allow/Deny 路径不变;AutoApprover / AutoDenier 适配新签名,行为等价 W8)

**Precondition:**
- W8 Plan 5 已完成(commit `8ec814d`):`DagApprovalDecision` 枚举(`Allow` / `Deny` / `Modify`,Modify 带 "W9+ 实现" 注释)在 `voicepilot/crates/ui/src/dag_commands.rs:23-30`;`From<DagApprovalDecision> for ApprovalDecision` 已实现(`dag_commands.rs:32-40`);`submit_dag_skeleton_approval` 逻辑函数(`dag_commands.rs:47-58`)只发 decision,无 modified payload;`DagApprovalRequestPayload`(`approver.rs:39-49`)emit `dag-approval-request` 事件携带 `plan_json` 给 webview;前端 `DagApprovalDialog.tsx:221-229` Modify 按钮 `disabled` + `title="W9+ 实现"`;`parsePlanJson`(`DagApprovalDialog.tsx:16-25`)已能解析 `plan_json`;`renderTemplatePreview`(`DagApprovalDialog.tsx:45-53`)只读渲染 `input_template_json`;TauriApprover 用 oneshot channel + 5min timeout + 默认 Deny(`approver.rs:87-103`);`TauriApprover::new`(tests)+ `with_app`(production)双构造器(`approver.rs:117-132`)。
- W8 Plan 2 已完成:`Approver` trait(`voicepilot/crates/trust-kernel/src/approval/approver.rs:24-42`)含 `approve_dag_skeleton(&self, plan: &DagPlan) -> Result<ApprovalDecision>` 方法;`AutoApprover`(`approver.rs:47-55`)返回 `Ok(ApprovalDecision::Allow)`;`AutoDenier`(`approver.rs:60-68`)返回 `Ok(ApprovalDecision::Deny)`。
- W8 Plan 2 已完成:`DagExecutor::run` 入口在 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs:82`,Step 2 行 121 调 `let decision = self.approver.approve_dag_skeleton(plan)?;`,行 122-131 审计 `dag_skeleton_approved`,行 132 `if matches!(decision, ApprovalDecision::Deny)` 仅处理 Deny 路径,未处理 Modify;行 134 持久化 `DagStatus::Cancelled`,行 136-145 审计 `dag_completed(cancelled)`,行 146 返回 `DagResult::cancelled()`。
- W8 Plan 1 已完成:`SlotTemplateEngine::validate_dag(plan: &DagPlan) -> Result<(), TemplateError>` 在 `voicepilot/crates/trust-kernel/src/skills/template.rs:509`,校验节点 ID 唯一性 + Var 引用 + Filter predicate;`DagPlan` / `DagNode` / `DagEdge` / `DagStatus` / `DagResult` 在 `dag_types.rs`;`ELevel` 在 `policy/types.rs`,含 `E0` / `E1` / `E2` / `E3` 四等级。
- `KernelError` 枚举在 `voicepilot/crates/trust-kernel/src/error.rs:3-47`,已含 `Db` / `Serde` / `Skill` / `Approval` 等变体,本 Plan 追加 `DagModifyLimitExceeded`。
- `ApprovalDecision`(`Allow` / `Deny` / `Modify`,已含 Modify 但仅作占位)在 `voicepilot/crates/trust-kernel/src/approval/types.rs:7-13`,本 Plan 不修改此枚举(保持向后兼容,只在前端 IPC 层用新 `DagApprovalOutcome` 替代)。
- 前端 `web/src/api.ts:202-210` `approveDagSkeleton(approvalRequestId, decision)` + `web/src/types.ts:268` `DagApprovalDecision = "allow" | "deny" | "modify"`,本 Plan 扩展签名加 `modifiedPlan?: DagPlan`。
- 前端 `web/src/types.ts:199-219` `DagNode` 接口含 `node_id` / `skill_id` / `risk_ceiling` / `status` / `input_template_json`,本 Plan 不修改 `DagNode` 结构(只在 `NodeEditor` 组件编辑这些字段);`DagApprovalRequestPayload`(`types.ts:258-265`)含 `plan_json: unknown`,本 Plan 不变。
- Plan 4 与 Plan 1-3 并行(无依赖),但须在 brainstorming 阶段确认 `dag_skeleton_modified` / `dag_modify_limit_exceeded` 审计事件与 Stronghold / Taint 审计事件无字段冲突(都用 `lower_snake_case`,字段名 `plan_id` / `modified_node_count` / `added_count` / `removed_count` 不与 `compensation_id` / `taints` / `value_hash` 冲突,已确认 ✓)。
- W9 Plan 1 已完成(stronghold feature 存在 + StrongholdVault API 可用)— **W9 修复(P2-17):** 仅在 Task 11 Step 1 跑 `cargo check --workspace --features voice,tauri,llm,stronghold` 时需要,Plan 4 本身不依赖 stronghold,但 spec §10 要求 7 套 feature 组合全过。

---

## File Structure

### Backend — Trust Kernel(`voicepilot/crates/trust-kernel/src/`)

- **Modify** `approval/approver.rs` — 扩展 `Approver` trait 签名 + 适配 AutoApprover / AutoDenier:
  - 新增 `pub enum DagApprovalOutcome { Allow, Deny, Modify { modified_plan: Box<DagPlan> } }`(放在 `approver.rs` 顶部,与 `Approver` trait 同模块,便于 `DagExecutor` 直接 `use`)
  - `Approver::approve_dag_skeleton` 签名从 `fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<ApprovalDecision>` 改为 `fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<DagApprovalOutcome>`
  - `AutoApprover::approve_dag_skeleton` 返回 `Ok(DagApprovalOutcome::Allow)`
  - `AutoDenier::approve_dag_skeleton` 返回 `Ok(DagApprovalOutcome::Deny)`
- **Modify** `error.rs` — 在 `KernelError` 枚举追加 `DagModifyLimitExceeded` 变体:
  - `#[error("dag modify limit exceeded: plan_id={plan_id}")] DagModifyLimitExceeded { plan_id: String }`
- **Modify** `policy/types.rs` — ELevel derive 加 `PartialOrd, Ord`(变体声明顺序 E0 < E1 < E2 < E3 已正确,derive Ord 后语义正确)
- **Modify** `skills/dag_executor.rs` — `run` 方法 Step 2 处理 Modify 分支 + 新增 `run_modified` 方法:
  - Step 2 行 121:`let decision = self.approver.approve_dag_skeleton(plan)?;` 改为 `let outcome = self.approver.approve_dag_skeleton(plan)?;`
  - 行 122-131 审计 `dag_skeleton_approved` 的 `decision` 字段从 `format!("{:?}", decision)` 改为 `outcome.as_str()`(顺手闭合 W8 spec §11 兼容性表最后一行,format 字符串 → 标准字符串)
  - 行 132 `if matches!(decision, ApprovalDecision::Deny)` 改为 `match outcome { DagApprovalOutcome::Deny => ..., DagApprovalOutcome::Allow => ..., DagApprovalOutcome::Modify { modified_plan } => ... }`
  - Modify 分支:审计 `dag_skeleton_modified` → 调 `SlotTemplateEngine::validate_dag(&modified_plan)?` → 调 `Self::check_risk_ceiling_no_escalation(plan, &modified_plan)?` → `return self.run_modified(&modified_plan, &root_task_id);`
  - 新增 `fn run_modified(&self, modified_plan: &DagPlan, root_task_id: &str) -> Result<DagResult>`:第二次调 `approve_dag_skeleton`,match Allow / Deny / Modify(Modify 时审计 `dag_modify_limit_exceeded` + 返回 `KernelError::DagModifyLimitExceeded`)
  - 新增 `fn check_risk_ceiling_no_escalation(original: &DagPlan, modified: &DagPlan) -> Result<()>`:遍历 `modified.nodes`,若任一节点 `risk_ceiling` > 原 plan 同 `node_id` 节点的 `risk_ceiling`(或新增节点的 `risk_ceiling` > 原 plan max `risk_ceiling`),返回 `KernelError::Skill(format!("risk ceiling escalated for node {}", node_id))`
  - 新增 `impl DagApprovalOutcome { pub fn as_str(&self) -> &'static str }` 返回 `"allow"` / `"deny"` / `"modify"`(供审计 details 字段使用)
- **Modify** `skills/dag_executor.rs` 模块顶部 `use` — 加 `use crate::approval::approver::DagApprovalOutcome;`(行 27 `use crate::approval::approver::Approver;` 之后追加)
- **不改** `skills/template.rs` — 复用既有 `SlotTemplateEngine::validate_dag(plan: &DagPlan) -> Result<(), TemplateError>`(`template.rs:509`),无新方法
- **不改** `approval/types.rs` — `ApprovalDecision` 枚举保持向后兼容(前端 IPC 层用 `DagApprovalOutcome`,后端单步审批仍用 `ApprovalDecision`)

### Backend — Tauri UI(`voicepilot/crates/ui/src/`)

- **Modify** `dag_commands.rs` — 扩展 `submit_dag_skeleton_approval` + `approve_dag_skeleton_command`:
  - `submit_dag_skeleton_approval(state, approval_request_id, decision, modified_plan: Option<DagPlan>) -> UiResult<bool>`:在 `take_sender` 成功后,构造 `DagApprovalPayload { decision, modified_plan }` 通过 oneshot 投递(需改 `ApprovalRegistry` channel 类型,见下)
  - `approve_dag_skeleton_command` Tauri 命令签名加 `modified_plan: Option<DagPlan>` 参数
  - `DagApprovalDecision` 枚举不变(仍 Allow / Deny / Modify,前端序列化用 `lowercase`)
- **Modify** `approver.rs` — 扩展 `TauriApprover` + `ApprovalRegistry`:
  - 新增 `pub struct DagApprovalPayload { pub decision: ApprovalDecision, pub modified_plan: Option<DagPlan> }`(serde `Serialize` + `Clone`)
  - `ApprovalRegistry` 新增 `dag_senders: Arc<Mutex<HashMap<String, oneshot::Sender<DagApprovalPayload>>>>` 字段(与既有 `senders` 并列,不破坏既有 `prompt` 单步审批语义)
  - `ApprovalRegistry::create_dag_request(&self, plan: &DagPlan) -> (String, oneshot::Receiver<DagApprovalPayload>)` — 新方法,生成 `dag_xxx` 前缀的 approval_request_id 与既有 `apr_xxx` 区分
  - `ApprovalRegistry::take_dag_sender(&self, id: &str) -> Option<oneshot::Sender<DagApprovalPayload>>` — 新方法,从 `dag_senders` 移除并返回
  - `ApprovalRegistry::wait_for_dag_decision(rx, timeout) -> DagApprovalPayload` — 新方法,超时或 sender dropped 返回 `DagApprovalPayload { decision: Deny, modified_plan: None }`
  - `TauriApprover::approve_dag_skeleton` 改返回 `KernelResult<DagApprovalOutcome>`(trait 方法,实现 `Approver`):调 `create_dag_request` → emit `dag-approval-request` 事件(复用既有 `DagApprovalRequestPayload`,不变)→ `wait_for_dag_decision` → match `decision` Allow/Deny/Modify(Modify 时若 `modified_plan` 为 None 返回 `Err(KernelError::Approval("Modify without modified_plan".into()))`,否则返回 `Ok(DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) })`)
  - `TauriApprover` 的 `Approver` trait impl 块:`fn approve_dag_skeleton(&self, plan: &DagPlan) -> KernelResult<DagApprovalOutcome>`(不再调 inherent `approve_dag_skeleton` 返回 `ApprovalDecision`,直接调 inherent `approve_dag_skeleton_outcome` 返回 `DagApprovalOutcome`)
  - 既有 inherent `pub fn approve_dag_skeleton(&self, plan: &DagPlan) -> ApprovalDecision` 重命名为 `pub fn approve_dag_skeleton_outcome(&self, plan: &DagPlan) -> KernelResult<DagApprovalOutcome>`(trait 与 inherent 不重名,避免方法歧义)
  - 既有 `create_dag_approval_request_for_test` 改返回 `(String, oneshot::Receiver<DagApprovalPayload>)`(类型变更,测试需同步更新)
- **Modify** `lib.rs` — 不需改(`pub mod dag_commands;` 已存在)
- **Modify** `commands.rs` — 不需改(`approve_dag_skeleton_command` 已在 `register_handlers` + `register_handlers_with_voice` 注册,签名加 `modified_plan: Option<DagPlan>` 由 Tauri 自动反序列化)

### Frontend — React(`voicepilot/crates/ui/web/src/`)

- **Modify** `components/DagApprovalDialog.tsx` — 激活 Modify 按钮 + 编辑模式切换:
  - 新增 state:`editingMode: boolean`(默认 `false`)、`editedNodes: DagNode[]`(默认 `parsePlanJson(plan_json).nodes`)
  - Modify 按钮从 `disabled` 改为 `onClick={() => setEditingMode(true)}`,移除 `title="W9+ 实现"`
  - `editingMode = true` 时渲染 `<NodeEditor>` 列表 + "添加节点" 按钮 + "提交修改" / "取消" 按钮
  - 新增 `handleModifySubmit`:`modifiedPlan = { ...original_plan, nodes: editedNodes }` → `approveDagSkeleton(approval_request_id, "modify", modifiedPlan)` → `onDismiss()`
  - `decide("modify")` 路径只在 `editingMode = false` 时不触发(用户必须先点 Modify 进入编辑模式,编辑后点 "提交修改")
  - 卸载时自动 Deny 逻辑不变(`useEffect` cleanup `approveDagSkeleton(approval_request_id, "deny")`)
- **Create** `components/NodeEditor.tsx` — W9 Plan 4 新组件,单节点编辑器:
  - Props:`{ node: DagNode; onChange(updated: DagNode): void; onDelete(): void }`
  - 渲染:`node_id` 文本输入框(只读,改 node_id 需重建节点)、`skill_id` 文本输入框、`risk_ceiling` `<select>`(E0/E1/E2/E3)、`input_template_json` `<textarea>`(行高 6,monospace 字体)、"删除节点" 按钮
  - `onChange` 触发条件:任一字段值变化时回调父组件,父组件更新 `editedNodes` 数组
- **Modify** `api.ts` — 扩展 `approveDagSkeleton`:
  - 签名从 `approveDagSkeleton(approvalRequestId, decision)` 改为 `approveDagSkeleton(approvalRequestId, decision, modifiedPlan?)`
  - `invoke<boolean>("approve_dag_skeleton_command", { approvalRequestId, decision, modifiedPlan })`(Tauri 自动把 camelCase → snake_case)
- **Modify** `types.ts` — 扩展 `DagApprovalDecision` + 新增 `DagPlanFull` 类型:
  - `DagApprovalDecision` 仍为 `"allow" | "deny" | "modify"`(后端 `lowercase` 序列化,无需改)
  - 新增 `export interface DagPlanFull { plan_id: string; user_goal: string; nodes: DagNode[]; edges: DagEdge[]; loop_specs: Record<string, unknown>; max_total_steps: number; }`(供 `handleModifySubmit` 构造完整 `modified_plan` 时用)
  - `DagApprovalRequestPayload.plan_json` 类型从 `unknown` 收紧为 `DagPlanFull`(可选,利于 `parsePlanJson` 类型推导)

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs` — W9 Plan 4 集成测试(6 个,Task 9 TDD):
  1. `modify_then_approve_allow_runs_modified_plan` — 用户 Modify → 第二次 Allow → 执行 modified_plan
  2. `modify_then_deny_cancels_dag` — 用户 Modify → 第二次 Deny → DagStatus::Cancelled
  3. `second_modify_returns_dag_modify_limit_exceeded` — 用户 Modify → 第二次仍 Modify → `KernelError::DagModifyLimitExceeded` + 审计 `dag_modify_limit_exceeded`
  4. `modify_with_invalid_modified_plan_fails_validation` — modified_plan 引用未知 node_id → `SlotTemplateEngine::validate_dag` 返回 `TemplateError::UnknownNodeId`
  5. `modify_with_escalated_risk_ceiling_rejected` — 原 plan max risk = E2,modified_plan 含 E3 节点 → `KernelError::Skill("risk ceiling escalated ...")`
  6. `modify_emits_complete_audit_events` — 验证审计链:`dag_plan_created` → `dag_skeleton_approved(modify)` → `dag_skeleton_modified` → `dag_skeleton_approved(allow/deny)` → 节点审计 → `dag_completed`
- **Create** `voicepilot/crates/ui/web/src/components/__tests__/NodeEditor.test.tsx` — vitest 组件测试(4 个,Task 10):
  1. `renders_node_fields_correctly` — 渲染 node_id / skill_id / risk_ceiling select / input_template textarea
  2. `add_node_via_dag_dialog` — 在 DagApprovalDialog 中点击 "添加节点" → editedNodes 长度 +1
  3. `delete_node_via_node_editor` — NodeEditor 点击 "删除节点" → editedNodes 长度 -1
  4. `submit_modified_plan_calls_approve_dag_skeleton_with_modify` — 点击 "提交修改" → `approveDagSkeleton` 被调用,参数含 `modifiedPlan`
- **Modify** `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs` — 既有 `submit_dag_skeleton_approval` 测试加 `modified_plan: None` 参数(向后兼容,既有的 Allow/Deny 测试不变)
- **Modify** `voicepilot/crates/ui/tests/approver_unit.rs` — 既有 `TauriApprover::approve_dag_skeleton` 测试适配新签名(返回 `DagApprovalOutcome` 而非 `ApprovalDecision`)+ 加 1 个 Modify 路径测试(`take_dag_sender` 投递 `DagApprovalPayload { decision: Modify, modified_plan: Some(...) }` → `approve_dag_skeleton` 返回 `Ok(DagApprovalOutcome::Modify { ... })`)

### Docs

- **Modify** `docs/PROGRESS.md` — W9 Plan 4 完成状态 + 测试统计(非门控测试数 ≥ 286 + 11 新增 = 6 集成 + 4 vitest + 1 approver_unit)

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 project_memory.md "Lessons Learned" + spec §10)
- **TDD**:每个 Task 先写失败测试 → 跑 → 实现 → 跑通 → commit(spec §10)
- **TauriApprover constructors**:`new` for tests(不 emit 事件,只阻塞等待),`with_app` for production(emit `dag-approval-request` 事件给 webview)(参考 `approver.rs:117-132` + project_memory.md)
- **TauriApprover oneshot + 5min timeout**:复用 `ApprovalRegistry` 既有模式,超时默认 Deny(参考 `approver.rs:87-103` + project_memory.md "Hard Constraints")。本 Plan 新增 `dag_senders` HashMap 与 `dag_senders` 平行,不破坏既有 `prompt` 单步审批(`approval-request` 事件)的 `senders` HashMap
- **禁止 `#[tokio::test]` 调 `TauriApprover`**(W9 修复 P0-5):`wait_for_dag_decision` 已用 `Handle::try_current()` 检测避免嵌套 runtime panic,但测试仍建议用 `#[test]`(同步上下文)调 TauriApprover,避免 async 复杂性。若必须用 `#[tokio::test]`,确保 `wait_for_dag_decision` 的 `Handle::try_current()` 分支被覆盖。
- **Tauri IPC 三安全规则**(参考 project_memory.md "Hard Constraints" + spec §6.3):
  1. WebView 不能直接访问 filesystem(本 Plan NodeEditor 只编辑内存中的 `editedNodes`,不读写文件)
  2. UI 不能直接调用 MCP(DAG Modify 不触发 MCP,只更新 plan_json)
  3. `approval_request_id` 单次使用(用后即焚,防重放)— `ApprovalRegistry::take_dag_sender` 移除 sender,与既有 `take_sender` 语义一致
- **审计事件命名**:沿用 W7/W8 的 `lower_snake_case`(`dag_skeleton_modified` / `dag_modify_limit_exceeded`),不用 SCREAMING_SNAKE(spec §10)
- **审计隐私约束(spec §6.3 + §6.4)**:`dag_skeleton_modified` details 仅记 `{plan_id, modified_node_count, added_count, removed_count}`,**不记录 input_template 内容**(可能含敏感数据);`dag_modify_limit_exceeded` details 仅记 `{plan_id}`
- **DagApprovalOutcome 与 ApprovalDecision 共存**:后端单步审批(`Approver::prompt`)仍用 `ApprovalDecision`(Allow/Deny/Modify 三变体),DAG 骨架审批(`approve_dag_skeleton`)改用 `DagApprovalOutcome`(Allow/Deny/Modify { modified_plan });两者不冲突,因 `prompt` 的 Modify 是占位(W8 未实现,本 Plan 不动),`approve_dag_skeleton` 的 Modify 是真实 payload
- **`Box<DagPlan>` 而非 `DagPlan`**:`DagApprovalOutcome::Modify { modified_plan: Box<DagPlan> }` 用 Box 避免枚举 size 爆炸(`DagPlan` 含 `Vec<DagNode>` + `HashMap<String, LoopSpec>`,栈上 size 大)
- **Modify 一次语义**:第二次 `approve_dag_skeleton` 返回 `Modify` 时,`run_modified` 返回 `KernelError::DagModifyLimitExceeded`(不递归调用 `run_modified`),防止无限递归(spec §6.3 第二条)
- **risk_ceiling 提权检查**:遍历 `modified_plan.nodes`,对每个节点:
  - 若 `node_id` 在原 plan 中存在:比较 `modified.risk_ceiling` 与 `original.risk_ceiling`,modified > original → 拒绝
  - 若 `node_id` 是新增:比较 `modified.risk_ceiling` 与 `original.nodes.iter().map(|n| n.risk_ceiling).max()`,modified > original_max → 拒绝
  - `ELevel` 实现 `Ord`(`policy/types.rs` 已实现,本 Plan 不改)— 若未实现,Task 3 步骤 3 加 `impl Ord for ELevel` 或转 `u8` 比较
- **`&kernel.conn()` 不用 `&*kernel.conn()`**:避免 clippy `explicit_auto_deref` lint(参考 project_memory.md "Lessons Learned")
- **TrustKernel 不是 Clone**:`DagExecutor` 持 `Arc<TrustKernel>`,本 Plan 不改;e2e 测试用 `Arc<TrustKernel>` 共享
- **Repo accessor pattern**:`DagRepo::new()` 不带参数,方法接收 `&Connection`(参考 project_memory.md "Engineering Conventions")— 本 Plan 不新增 Repo
- **Tauri feature `#[cfg(feature = "tauri")]` 门控**:`dag_commands.rs` + `approver.rs` 的 TauriApprover 整体 `#[cfg(feature = "tauri")]` 门控;trust-kernel 侧的 `DagApprovalOutcome` / `DagExecutor::run_modified` 无 feature gate(default feature 下编译)
- **npm.cmd run build**(Windows):不用 `npm run build`(参考 project_memory.md)
- **Engineering Console UI aesthetic**(参考 project_memory.md "Engineering Conventions"):
  - 深海军蓝背景(`--bg-deep: #0a0e1a` / `--bg-base: #111827`)+ 暖琥珀色强调(`--accent: #f59e0b`)
  - IBM Plex Mono(代码 / 数字)+ IBM Plex Sans(正文)
  - 4px 直角(`border-radius: 0`),不用圆角
  - WCAG A 可访问性:`NodeEditor` 的 textarea / select / button 都有 `label` / `htmlFor`;`aria-label` 描述节点编辑操作
  - 不用 emoji,不用 gradient / heavy shadow
- **风险徽章颜色编码**(spec §6 安全约束):
  - E0 = 灰色(`--text-muted`) / E1 = 蓝色(`--info: #3b82f6`) / E2 = 琥珀色(`--accent`) / E3 = 红色(`--danger`)
- **commit message**:`feat(w9p4): ...` / `test(w9p4): ...` / `fix(w9p4): ...` / `refactor(w9p4): ...`(spec §10)
- **不引入非必要依赖**:本 Plan 仅用 Tauri 2 + React 18 + TypeScript + `@tauri-apps/api`(均已在 `web/package.json`),不加新 npm 包;后端仅用 `serde` + `thiserror` + `tokio` + `uuid`(均已在 workspace)
- **不修改 spec / 已有 plan**:若发现 spec 描述与实现不一致,记录到 PROGRESS.md "已知偏离" 段落,不回改 spec(spec §10)
- **空 commit 标记里程碑**:W9 Plan 4 收尾不需要(整个 W9 收尾在 Plan 7 用空 commit)

---

## Task 1: 后端 — DagApprovalOutcome 枚举 + Approver trait 签名扩展 + AutoApprover/AutoDenier 适配

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/approval/approver.rs`(扩展 `Approver` trait + 新增 `DagApprovalOutcome` + 适配 AutoApprover / AutoDenier)
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(行 27 `use` 加 `DagApprovalOutcome`;行 121-147 `run` 方法 Step 2 临时适配 — Task 2 完整实现 Modify 分支)

**目标:** 把 `Approver::approve_dag_skeleton` 返回类型从 `ApprovalDecision` 改为 `DagApprovalOutcome`,新增 `DagApprovalOutcome` 枚举(Allow / Deny / Modify { modified_plan: Box<DagPlan> }),AutoApprover / AutoDenier 适配新签名。本 Task 只动 trait 签名 + AutoApprover / AutoDenier,`dag_executor.rs` 的 `run` 方法仅做最小适配(把 `decision` 改为 `outcome`,Modify 暂时 fallthrough 到 Allow 路径,Task 2 完整实现)。

- [ ] **Step 1: 在 `approver.rs` 顶部新增 `DagApprovalOutcome` 枚举**

打开 `voicepilot/crates/trust-kernel/src/approval/approver.rs`,在 `use crate::approval::types::ApprovalDecision;`(行 15)之后追加:

```rust
/// W9 Plan 4:DAG 骨架审批的完整决策结果。
///
/// 与 `ApprovalDecision` 区别:`ApprovalDecision` 用于单步 prepare→commit
/// 审批(`Approver::prompt`),Modify 是占位(W8 未实现 payload 回传);
/// `DagApprovalOutcome` 用于 DAG 骨架审批(`Approver::approve_dag_skeleton`),
/// Modify 携带完整 `modified_plan: Box<DagPlan>`(W9 Plan 4 实现)。
///
/// 用 `Box<DagPlan>` 避免枚举 size 爆炸(`DagPlan` 含 Vec + HashMap,栈上 size 大)。
#[derive(Debug, Clone)]
pub enum DagApprovalOutcome {
    Allow,
    Deny,
    /// 用户调整 DAG 骨架后回传的 modified_plan。
    /// 由 `DagExecutor::run` 处理:审计 `dag_skeleton_modified` →
    /// `SlotTemplateEngine::validate_dag` 重新校验 → `run_modified` 第二次审批。
    Modify { modified_plan: Box<DagPlan> },
}

impl DagApprovalOutcome {
    /// 返回标准字符串(供审计 details 字段使用,替换 W8 的 `format!("{:?}", decision)`)。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Modify { .. } => "modify",
        }
    }
}
```

- [ ] **Step 2: 修改 `Approver` trait 签名**

在 `voicepilot/crates/trust-kernel/src/approval/approver.rs` 中,把 `Approver::approve_dag_skeleton` 签名(行 41)从:

```rust
fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<ApprovalDecision>;
```

改为:

```rust
fn approve_dag_skeleton(&self, plan: &DagPlan) -> Result<DagApprovalOutcome>;
```

同时更新方法注释(行 29-40),把 "返回 `Result<ApprovalDecision>` 而非 `ApprovalDecision`" 改为 "返回 `Result<DagApprovalOutcome>`(W9 Plan 4:含 Modify payload)而非 `ApprovalDecision`(单步审批用)"。

- [ ] **Step 3: 适配 `AutoApprover`**

在 `voicepilot/crates/trust-kernel/src/approval/approver.rs` 中,把 `AutoApprover::approve_dag_skeleton`(行 52-54)从:

```rust
fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<ApprovalDecision> {
    Ok(ApprovalDecision::Allow)
}
```

改为:

```rust
fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<DagApprovalOutcome> {
    Ok(DagApprovalOutcome::Allow)
}
```

- [ ] **Step 4: 适配 `AutoDenier`**

在 `voicepilot/crates/trust-kernel/src/approval/approver.rs` 中,把 `AutoDenier::approve_dag_skeleton`(行 65-67)从:

```rust
fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<ApprovalDecision> {
    Ok(ApprovalDecision::Deny)
}
```

改为:

```rust
fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<DagApprovalOutcome> {
    Ok(DagApprovalOutcome::Deny)
}
```

- [ ] **Step 5: 在 `dag_executor.rs` 顶部 `use` 加 `DagApprovalOutcome`**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,在行 27 `use crate::approval::approver::Approver;` 之后追加:

```rust
use crate::approval::approver::DagApprovalOutcome;
```

- [ ] **Step 6: 在 `dag_executor.rs` 行 121 临时适配(最小改动,Task 2 完整实现 Modify 分支)**

把行 121:

```rust
let decision = self.approver.approve_dag_skeleton(plan)?;
```

改为:

```rust
let outcome = self.approver.approve_dag_skeleton(plan)?;
```

把行 122-131 审计 `dag_skeleton_approved` 的 `decision` 字段:

```rust
"decision": format!("{:?}", decision),
```

改为(W8 spec §11 顺手闭合 format→字符串):

```rust
"decision": outcome.as_str(),
```

把行 132:

```rust
if matches!(decision, ApprovalDecision::Deny) {
```

改为(临时:Deny 走 Cancelled,Allow / Modify 都走执行路径,Task 2 完整实现 Modify):

```rust
if matches!(outcome, DagApprovalOutcome::Deny) {
```

注意:此时 `ApprovalDecision` import 可能变成未使用 — 若 clippy 报 `unused_imports`,在行 28 `use crate::approval::types::ApprovalDecision;` 加 `#[allow(unused_imports)]` 或直接删除(Task 2 不再用 `ApprovalDecision`)。**临时保留删除注释**:在 Task 2 完成后,行 28 `use crate::approval::types::ApprovalDecision;` 整行删除。

- [ ] **Step 7: 跑 `cargo check` 验证编译**

Run:
```powershell
cd voicepilot; cargo check --workspace --features voice,tauri,llm
```
Expected: 编译通过(可能有 `unused_imports` 警告,Step 6 已说明处理)。若 `TauriApprover` 在 `crates/ui/src/approver.rs` 报 trait 方法签名不匹配(`approve_dag_skeleton` 返回 `ApprovalDecision` 而非 `DagApprovalOutcome`),Task 5 修复;本 Task 范围内只确保 trust-kernel crate 编译通过,ui crate 在 Task 5 修复。

若 ui crate 编译失败阻塞 trust-kernel 测试,临时在 `crates/ui/src/approver.rs` 的 `TauriApprover` trait impl 块加 `#[allow(unused_variables)]` + 把 `approve_dag_skeleton` 返回 `Ok(DagApprovalOutcome::Allow)` 占位,Task 5 完整实现。

- [ ] **Step 8: 跑既有 W8 测试验证不回归**

Run:
```powershell
cd voicepilot; cargo test --features voice,tauri,llm -p trust-kernel --test w8_plan2_dag_executor
```
Expected: 全部 PASS(AutoApprover / AutoDenier 适配新签名后,Allow / Deny 路径行为等价 W8)。

若测试失败,检查测试代码是否直接断言 `ApprovalDecision::Allow` / `Deny`(W8 测试可能在 `dag_executor` 测试中 `assert!(matches!(decision, ApprovalDecision::Allow))`),改为断言 `DagApprovalOutcome::Allow` / `Deny`。

- [ ] **Step 9: Commit**

```powershell
cd voicepilot; git add crates/trust-kernel/src/approval/approver.rs crates/trust-kernel/src/skills/dag_executor.rs; git commit -m "feat(w9p4): extend Approver trait with DagApprovalOutcome for Modify payload"
```

---

## Task 2: 后端 — dag_executor.rs 处理 Modify 分支 + run_modified + DagModifyLimitExceeded

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/error.rs`(加 `DagModifyLimitExceeded` 变体)
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`(完整实现 Modify 分支 + `run_modified` + `check_risk_ceiling_no_escalation`)
- Test: `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs`(Task 9 完整 TDD,本 Task 先写最小冒烟测试)

**目标:** 在 `DagExecutor::run` Step 2 完整处理 `DagApprovalOutcome::Modify` 分支:审计 `dag_skeleton_modified` → 调 `SlotTemplateEngine::validate_dag(&modified_plan)` 重新校验 → 调 `check_risk_ceiling_no_escalation` 防提权 → 调用新方法 `run_modified(&modified_plan, &root_task_id)` 走第二次审批。第二次 `approve_dag_skeleton` 只匹配 Allow / Deny,遇到 Modify 返回 `KernelError::DagModifyLimitExceeded` + 审计 `dag_modify_limit_exceeded`。

**W9 修复(P2-19):** 本 Task 引用的行号(如 "行 121"、"行 149"、"行 117-118")均基于 W8 commit `8ec814d` 的 `dag_executor.rs` 快照。Task 2 Step 6 把 `run` 方法节点执行循环抽为 `execute_nodes` 后,原行号会失效 — 实现时需重新定位(用 `grep -n "topological_sort" crates/trust-kernel/src/skills/dag_executor.rs` 等命令查找当前行号)。

- [ ] **Step 1: 在 `error.rs` 加 `DagModifyLimitExceeded` + 在 `policy/types.rs` 加 ELevel Ord derive(必做项)**

打开 `voicepilot/crates/trust-kernel/src/error.rs`,在 `KernelError` 枚举最后一个变体 `Uia(String)`(行 46)之后追加:

```rust
    /// W9 Plan 4:DAG Modify 超过单次上限(第二次 Modify 被拒绝,防无限递归)。
    #[error("dag modify limit exceeded: plan_id={plan_id}")]
    DagModifyLimitExceeded { plan_id: String },
```

**W9 修复(P0-1):** 同时打开 `voicepilot/crates/trust-kernel/src/policy/types.rs`,把 `ELevel` 的 derive 从 `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` 改为 `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]`。变体声明顺序 E0 < E1 < E2 < E3 已正确,derive Ord 后语义正确。本 Step 为必做项(不是 conditional),供 Task 2 Step 7 `check_risk_ceiling_no_escalation` 的 `ceiling > original_node.risk_ceiling` 比较使用 — 若不 derive Ord,该行编译失败。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ELevel {
    E0,
    E1,
    E2,
    E3,
}
```

- [ ] **Step 2: 写最小冒烟测试(先失败)**

创建 `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs`(本 Task 只写 1 个最小测试,Task 9 补齐 6 个):

```rust
//! W9 Plan 4:DAG Modify 闭环集成测试。
//!
//! 测试 `DagExecutor::run` 处理 `DagApprovalOutcome::Modify` 分支:
//! 审计 `dag_skeleton_modified` + 重新校验 + `run_modified` 第二次审批。
//! Modify 只允许一次,第二次 Modify 返回 `KernelError::DagModifyLimitExceeded`。

#![cfg(test)]

use std::sync::{Arc, Mutex};

use trust_kernel::approval::approver::{Approver, AutoApprover, DagApprovalOutcome};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

/// 可编程 Approver:按预设序列返回决策(第一次 Modify → 第二次 Allow)。
struct ScriptedApprover {
    outcomes: Mutex<Vec<DagApprovalOutcome>>,
}

impl ScriptedApprover {
    fn new(outcomes: Vec<DagApprovalOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes),
        }
    }
}

impl Approver for ScriptedApprover {
    fn prompt(&self, _manifest: &trust_kernel::policy::transaction::EffectManifest) -> trust_kernel::approval::types::ApprovalDecision {
        trust_kernel::approval::types::ApprovalDecision::Allow
    }

    fn approve_dag_skeleton(&self, _plan: &DagPlan) -> trust_kernel::error::Result<DagApprovalOutcome> {
        let mut guard = self.outcomes.lock().unwrap();
        if guard.is_empty() {
            return Ok(DagApprovalOutcome::Deny);
        }
        Ok(guard.remove(0))
    }
}

fn literal_text_node(node_id: &str, skill_id: &str, text: &str) -> DagNode {
    DagNode {
        node_id: node_id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal(text.into()),
        },
        risk_ceiling: trust_kernel::policy::types::ELevel::E1,
    }
}

fn one_node_plan(plan_id: &str, node: DagNode) -> DagPlan {
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "W9 Plan 4 测试".into(),
        nodes: vec![node],
        edges: vec![],
        loop_specs: std::collections::HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn modify_then_approve_allow_runs_modified_plan() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    let original_node = literal_text_node("n1", "note.capture", "原始 TODO");
    let original_plan = one_node_plan("w9p4-modify-allow", original_node);

    let modified_node = literal_text_node("n1", "note.capture", "修改后 TODO");
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    // 第一次 Modify → 第二次 Allow
    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan.clone()) },
        DagApprovalOutcome::Allow,
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan).unwrap();

    // 验证:DagStatus::Succeeded(modified_plan 被执行)
    assert!(matches!(result.status, trust_kernel::skills::dag_types::DagStatus::Succeeded),
        "expected Succeeded, got {:?}", result.status);
}
```

- [ ] **Step 3: 跑测试验证失败**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke modify_then_approve_allow_runs_modified_plan
```
Expected: FAIL,因为 Task 1 的 Step 6 临时把 Modify fallthrough 到 Allow 路径,直接执行 original_plan 而非 modified_plan(虽然测试可能 PASS,但因 `run_modified` 未实现,Modify 分支没走第二次审批 — 需补完整 Task 2 实现后跑)。

若测试 PASS 但行为错误(没走 Modify 分支),改为更严格断言:在 `ScriptedApprover::approve_dag_skeleton` 加 `println!("call #{}", call_count)` 验证被调用 2 次。

- [ ] **Step 4: 在 `dag_executor.rs` 实现 Modify 分支**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,把 Step 2(行 120-147)从:

```rust
// Step 2: 全局审批 — DAG 骨架 Allow/Deny(决策 #2)
let outcome = self.approver.approve_dag_skeleton(plan)?;
// 审计 — dag_skeleton_approved(无论 Allow/Deny 都记录)
self.kernel.audit_append_external(
    &root_task_id,
    None,
    "dag_skeleton_approved",
    serde_json::json!({
        "plan_id": plan.plan_id,
        "decision": outcome.as_str(),
    }),
)?;
if matches!(outcome, DagApprovalOutcome::Deny) {
    // Deny → 0 节点执行 + DagStatus=Cancelled
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
```

改为:

```rust
// Step 2: 全局审批 — DAG 骨架 Allow/Deny/Modify(决策 #2 + W9 Plan 4 Modify 分支)
let outcome = self.approver.approve_dag_skeleton(plan)?;
// 审计 — dag_skeleton_approved(无论 Allow/Deny/Modify 都记录)
self.kernel.audit_append_external(
    &root_task_id,
    None,
    "dag_skeleton_approved",
    serde_json::json!({
        "plan_id": plan.plan_id,
        "decision": outcome.as_str(),
        "phase": "initial",
    }),
)?;

let effective_plan: DagPlan = match outcome {
    DagApprovalOutcome::Allow => plan.clone(),
    DagApprovalOutcome::Deny => {
        // Deny → 0 节点执行 + DagStatus=Cancelled
        self.persist_dag_status(plan, &DagStatus::Cancelled)?;
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
    DagApprovalOutcome::Modify { modified_plan } => {
        // W9 Plan 4:审计 dag_skeleton_modified + 重新校验 + run_modified(第二次审批)
        let modified_node_count = modified_plan.nodes.len();
        let added_count = modified_plan
            .nodes
            .iter()
            .filter(|n| !plan.nodes.iter().any(|o| o.node_id == n.node_id))
            .count();
        let removed_count = plan
            .nodes
            .iter()
            .filter(|o| !modified_plan.nodes.iter().any(|n| n.node_id == o.node_id))
            .count();
        self.kernel.audit_append_external(
            &root_task_id,
            None,
            "dag_skeleton_modified",
            serde_json::json!({
                "plan_id": plan.plan_id,
                "modified_node_count": modified_node_count,
                "added_count": added_count,
                "removed_count": removed_count,
            }),
        )?;

        // 重新校验 modified_plan(SlotTemplateEngine::validate_dag)
        if let Err(e) = SlotTemplateEngine::validate_dag(&modified_plan) {
            return Err(KernelError::Skill(format!("modified_plan validate_dag failed: {}", e)));
        }

        // risk_ceiling 提权检查(spec §6.3 第三条)
        Self::check_risk_ceiling_no_escalation(plan, &modified_plan)?;

        // W9 修复(P0-2):对 modified_plan 做环检测(前置 fail-fast,避免 execute_nodes 内才报错)
        topological_sort(&modified_plan.nodes, &modified_plan.edges)?;

        // 第二次审批(只允许 Allow / Deny)
        return self.run_modified(&modified_plan, &root_task_id);
    }
};
```

**W9 修复(P0-2):** 保留 W8 既有对原 plan 的 `topological_sort` 环检测(在 `dag_skeleton_approved` 之前,前置 fail-fast)。**不要**把 `topological_sort` 后移到 `effective_plan` 之后 — 否则原 plan 有环时审计链断(Modify 分支审计 `dag_skeleton_modified` 后才报环错,而非 audit 之前就 fail-fast)。

Modify 分支额外对 `modified_plan` 调一次 `topological_sort` 做环检测(仅校验,不消费 order)— 已在上方 Modify 分支代码末尾加 `topological_sort(&modified_plan.nodes, &modified_plan.edges)?;`(在 `run_modified` 之前)。

后续行 149 起的 `for node_id in &order` 循环保留用 Step 1 对原 plan 的 `order`,但 `execute_nodes`(Step 6 抽出)内部会对 `effective_plan` 重新调 `topological_sort` 拿 order(因 modified_plan 节点可能变化)。原 Step 1 位置(行 117-118)保留不变 — 但因 `run` 方法 Step 2 之后改为调 `execute_nodes`(`order` 不再被 `run` 直接使用),Step 1 行可改为 `topological_sort(&plan.nodes, &plan.edges)?;`(去掉 `let order =`)以避免 `unused variable` 警告,仅保留 fail-fast 环检测语义。

```rust
// Step 1(保留 W8 既有位置,仅做 fail-fast 环检测,不消费 order):
// W9 修复(P0-2):不要后移到 effective_plan 之后,保留前置 fail-fast 语义
topological_sort(&plan.nodes, &plan.edges)?;
```

- [ ] **Step 5: 在 `dag_executor.rs` 实现 `run_modified` 方法**

在 `DagExecutor` impl 块中(`run` 方法之后)追加:

**W9 修复(P0-3):** modified_plan 用新 `plan_id`(如 `format!("{}_modified", modified_plan.plan_id)`),单独走 `dag_plan_created` 审计 + `persist_dag_status(modified_plan, Pending)`。原 plan 的 `dag_plan_created` 已在 `run` 方法 Step 0 写过一次,modified_plan 复用原 plan_id 会导致审计缺失 + 节点列表未持久化(数据库 `dag_status` 表只有原 plan_id 的记录,modified_plan 节点无法追踪)。

```rust
/// W9 Plan 4:第二次审批 — 只允许 Allow / Deny,Modify 返回 `DagModifyLimitExceeded`。
///
/// spec §6.3 第二条:Modify 只允许一次,防止无限递归。
/// W9 修复(P0-3):modified_plan 用新 plan_id,单独走完整审计链
/// (dag_plan_created + persist_dag_status(Pending) + dag_skeleton_approved + ...)。
fn run_modified(&self, modified_plan: &DagPlan, root_task_id: &str) -> Result<DagResult> {
    // W9 修复(P0-3):modified_plan 用新 plan_id,单独走完整审计链
    let modified_plan_with_id = DagPlan {
        plan_id: format!("{}_modified", modified_plan.plan_id),
        ..modified_plan.clone()
    };
    self.kernel.audit_append_external(
        root_task_id,
        None,
        "dag_plan_created",
        serde_json::json!({
            "plan_id": modified_plan_with_id.plan_id,
            "source": "modify",
            "original_plan_id": modified_plan.plan_id,
        }),
    )?;
    self.persist_dag_status(&modified_plan_with_id, &DagStatus::Pending)?;

    let outcome = self.approver.approve_dag_skeleton(&modified_plan_with_id)?;
    self.kernel.audit_append_external(
        root_task_id,
        None,
        "dag_skeleton_approved",
        serde_json::json!({
            "plan_id": modified_plan_with_id.plan_id,
            "decision": outcome.as_str(),
            "phase": "after_modify",
        }),
    )?;

    match outcome {
        DagApprovalOutcome::Allow => {
            // 第二次 Allow → 用 modified_plan_with_id 走完整执行路径
            self.execute_nodes(&modified_plan_with_id, root_task_id)
        }
        DagApprovalOutcome::Deny => {
            self.persist_dag_status(&modified_plan_with_id, &DagStatus::Cancelled)?;
            self.kernel.audit_append_external(
                root_task_id,
                None,
                "dag_completed",
                serde_json::json!({
                    "plan_id": modified_plan_with_id.plan_id,
                    "final_status": "cancelled",
                    "succeeded_count": 0,
                }),
            )?;
            Ok(DagResult::cancelled())
        }
        DagApprovalOutcome::Modify { .. } => {
            // 第二次 Modify → 拒绝(spec §6.3 第二条)
            self.kernel.audit_append_external(
                root_task_id,
                None,
                "dag_modify_limit_exceeded",
                serde_json::json!({
                    "plan_id": modified_plan_with_id.plan_id,
                }),
            )?;
            Err(KernelError::DagModifyLimitExceeded {
                plan_id: modified_plan_with_id.plan_id,
            })
        }
    }
}
```

- [ ] **Step 6: 重构 `run` 把节点执行循环抽为 `execute_nodes`**

为了 `run_modified` 第二次 Allow 时能复用节点执行逻辑,把 `run` 方法行 149 起的 `for node_id in &order` 循环抽为独立方法 `execute_nodes(&self, plan: &DagPlan, root_task_id: &str) -> Result<DagResult>`:

```rust
/// W9 Plan 4:节点执行循环(原 run 方法 Step 3-4 抽出,供 run_modified 复用)。
///
/// 输入:`plan`(effective_plan,可能是原 plan 或 modified_plan)+ `root_task_id`
/// 输出:DagResult(Succeeded / Failed / PartiallySucceeded)
fn execute_nodes(&self, plan: &DagPlan, root_task_id: &str) -> Result<DagResult> {
    let order = topological_sort(&plan.nodes, &plan.edges)?;
    let mut node_outputs: HashMap<String, serde_json::Value> = HashMap::new();
    let mut node_results: HashMap<String, DagNodeStatus> = HashMap::new();
    let mut prev_node_id: Option<String> = None;

    for node_id in &order {
        // ... 原 run 方法行 154-280 的节点循环逻辑(不变)
        // 把 self.kernel / self.dag_repo 引用保持,把 plan 引用改为 effective_plan = plan
    }

    // Step 4:全部成功 → DagStatus::Succeeded
    self.persist_dag_status(plan, &DagStatus::Succeeded)?;
    self.kernel.audit_append_external(
        root_task_id,
        None,
        "dag_completed",
        serde_json::json!({
            "plan_id": plan.plan_id,
            "final_status": "succeeded",
            "succeeded_count": order.len(),
        }),
    )?;
    Ok(DagResult::succeeded(node_results))
}
```

然后 `run` 方法 Step 2 之后的代码改为:

```rust
// Step 3-4:节点执行(抽为 execute_nodes,W9 Plan 4 重构)
self.execute_nodes(&effective_plan, &root_task_id)
```

注意:`execute_nodes` 内部原有 `run_simple_node` / `run_loop_node` 调用不变;`run` 方法行 154 之后的代码全部移到 `execute_nodes` 中。

- [ ] **Step 7: 在 `dag_executor.rs` 实现 `check_risk_ceiling_no_escalation`**

在 `DagExecutor` impl 块中追加:

```rust
/// W9 Plan 4:检查 modified_plan 的 risk_ceiling 没有超过原 plan 的 max risk。
///
/// spec §6.3 第三条:用户不能通过 Modify 提权。
/// 规则:
///   - 既有节点:modified.risk_ceiling ≤ original.risk_ceiling(同 node_id)
///   - 新增节点:modified.risk_ceiling ≤ max(original.nodes.risk_ceiling)
///
/// `ELevel` 实现 `Ord`(`policy/types.rs`),直接比较。
fn check_risk_ceiling_no_escalation(original: &DagPlan, modified: &DagPlan) -> Result<()> {
    use crate::policy::types::ELevel;
    let original_max: ELevel = original
        .nodes
        .iter()
        .map(|n| n.risk_ceiling)
        .max()
        .unwrap_or(ELevel::E0);

    for modified_node in &modified.nodes {
        let ceiling = modified_node.risk_ceiling;
        if let Some(original_node) = original.nodes.iter().find(|n| n.node_id == modified_node.node_id) {
            // 既有节点:不能超过原 ceiling
            if ceiling > original_node.risk_ceiling {
                return Err(KernelError::Skill(format!(
                    "risk ceiling escalated for node {}: {:?} > {:?}",
                    modified_node.node_id, ceiling, original_node.risk_ceiling
                )));
            }
        } else {
            // 新增节点:不能超过原 plan max ceiling
            if ceiling > original_max {
                return Err(KernelError::Skill(format!(
                    "risk ceiling escalated for new node {}: {:?} > original max {:?}",
                    modified_node.node_id, ceiling, original_max
                )));
            }
        }
    }
    Ok(())
}
```

注意:`ELevel` 的 `PartialOrd, Ord` derive 已在 Task 2 Step 1 作为必做项完成(W9 修复 P0-1,非 conditional),`ceiling > original_node.risk_ceiling` 比较直接可用,Step 8 跑 `cargo check` 不会因 ELevel Ord 报错。

- [ ] **Step 8: 跑 `cargo check` 验证编译**

Run:
```powershell
cd voicepilot; cargo check --workspace --features voice,tauri,llm
```
Expected: 编译通过。若 `ELevel` 未实现 `Ord` 报错,按 Step 7 注意说明加 `Ord` derive。

- [ ] **Step 9: 跑 Step 2 的冒烟测试**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke modify_then_approve_allow_runs_modified_plan
```
Expected: PASS。

- [ ] **Step 10: 跑既有 W8 测试验证不回归**

Run:
```powershell
cd voicepilot; cargo test --features voice,tauri,llm -p trust-kernel --test w8_plan2_dag_executor; cargo test --features voice,tauri,llm -p trust-kernel --test w8_plan2_dag_e2e
```
Expected: 全部 PASS。

- [ ] **Step 11: Commit**

```powershell
cd voicepilot; git add crates/trust-kernel/src/error.rs crates/trust-kernel/src/skills/dag_executor.rs crates/trust-kernel/src/policy/types.rs crates/trust-kernel/tests/w9_dag_modify_smoke.rs; git commit -m "feat(w9p4): implement DagExecutor Modify branch + run_modified + risk ceiling check"
```

---

## Task 3: 后端 — SlotTemplateEngine::validate_dag 重新校验 + risk_ceiling 提权检查

**Files:**
- 不改 `voicepilot/crates/trust-kernel/src/skills/template.rs`(复用既有 `validate_dag`)
- Modify: `voicepilot/crates/trust-kernel/src/policy/types.rs`(确认 `ELevel` 实现 `Ord`,若未实现则加 derive)
- Test: `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs`(Task 9 补齐 `modify_with_invalid_modified_plan_fails_validation` + `modify_with_escalated_risk_ceiling_rejected`)

**目标:** 确认 `SlotTemplateEngine::validate_dag` 既有实现能校验 modified_plan 的节点 ID 唯一性 + Var 引用 + Filter predicate(spec §6.3 第一条),`ELevel` 实现 `Ord` 以支持 `check_risk_ceiling_no_escalation` 比较(spec §6.3 第三条)。本 Task 主要是验证 + 测试,代码改动最小。

- [ ] **Step 1: 验证 `SlotTemplateEngine::validate_dag` 既有实现**

打开 `voicepilot/crates/trust-kernel/src/skills/template.rs`,定位行 509 `pub fn validate_dag(plan: &DagPlan) -> Result<(), TemplateError>`,确认其校验逻辑:
- 节点 ID 集合 `node_ids: HashSet<&str>`(行 510-511)
- 遍历 `plan.nodes` 调 `Self::validate_expr` 校验 `Var.scope = Step(id)` 时 `id` 在 `node_ids` 中(行 442-450)
- 校验 `${prev.output.xxx}` 仅在节点有前驱时有效(行 437-441)
- 校验 `${item}` 仅在循环节点中有效(行 934-957)

Expected: 既有实现已满足 spec §6.3 第一条(modified_plan 必须通过 `SlotTemplateEngine::validate_dag` 重新校验)。**本 Task 不改 `template.rs`**。

- [ ] **Step 2: 验证 `ELevel` 实现 `Ord`**

打开 `voicepilot/crates/trust-kernel/src/policy/types.rs`,定位 `ELevel` enum 定义,确认 derive 列表。

Run:
```powershell
Select-String -Path "voicepilot\crates\trust-kernel\src\policy\types.rs" -Pattern "enum ELevel" -Context 0,2
```
Expected: 显示 `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`(或类似)+ `pub enum ELevel { ... }`。

- [ ] **Step 3: ELevel Ord derive 已在 Task 2 Step 1 完成**

W9 修复(P0-1):`ELevel` 的 `PartialOrd, Ord` derive 已在 Task 2 Step 1 作为必做项完成(非 conditional),本 Step 无需再改 `policy/types.rs`。Step 2 的验证仅用于确认 derive 列表含 `Ord`(若 W8 已实现 Ord 则无变化,若 W8 未实现则 Task 2 Step 1 已追加)。

注意:Rust enum derive `Ord` 按变体声明顺序比较(`E0 < E1 < E2 < E3`),与风险等级语义一致。

- [ ] **Step 4: 写 `modify_with_invalid_modified_plan_fails_validation` 测试(先失败)**

在 `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs` 追加(Task 9 完善断言):

```rust
#[test]
fn modify_with_invalid_modified_plan_fails_validation() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    let original_node = literal_text_node("n1", "note.capture", "原始");
    let original_plan = one_node_plan("w9p4-invalid-modify", original_node);

    // modified_plan 引用未知 node_id n99(违反 validate_dag)
    let bad_expr = trust_kernel::skills::template::SlotTemplateEngine::parse("${n99.output.path}").unwrap();
    let modified_node = DagNode {
        node_id: "n1".into(),
        skill_id: "note.capture".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: bad_expr,
        },
        risk_ceiling: trust_kernel::policy::types::ELevel::E1,
    };
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) },
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan);

    // 验证:返回 Err(KernelError::Skill("modified_plan validate_dag failed: ..."))
    assert!(result.is_err(), "expected Err for invalid modified_plan");
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("validate_dag failed"), "got: {}", err_msg);
}
```

- [ ] **Step 5: 跑测试验证通过**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke modify_with_invalid_modified_plan_fails_validation
```
Expected: PASS(Task 2 Step 4 的 `SlotTemplateEngine::validate_dag(&modified_plan)?` 已实现校验)。

- [ ] **Step 6: 写 `modify_with_escalated_risk_ceiling_rejected` 测试(先失败)**

在 `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs` 追加:

```rust
#[test]
fn modify_with_escalated_risk_ceiling_rejected() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    // 原 plan:n1 = E1
    let original_node = literal_text_node("n1", "note.capture", "原始");
    let original_plan = one_node_plan("w9p4-escalation", original_node);

    // modified_plan:n1 = E3(提权)
    let modified_node = DagNode {
        node_id: "n1".into(),
        skill_id: "note.capture".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("修改后".into()),
        },
        risk_ceiling: trust_kernel::policy::types::ELevel::E3, // 提权 E1 → E3
    };
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) },
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan);

    // 验证:返回 Err(KernelError::Skill("risk ceiling escalated ..."))
    assert!(result.is_err(), "expected Err for escalated risk_ceiling");
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("risk ceiling escalated"), "got: {}", err_msg);
}
```

- [ ] **Step 7: 跑测试验证通过**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke modify_with_escalated_risk_ceiling_rejected
```
Expected: PASS(Task 2 Step 7 的 `check_risk_ceiling_no_escalation` 已实现)。

- [ ] **Step 8: 跑 `cargo check` 全 workspace**

Run:
```powershell
cd voicepilot; cargo check --workspace
```
Expected: 编译通过(无 `ELevel` Ord 报错)。

- [ ] **Step 9: Commit**

```powershell
cd voicepilot; git add crates/trust-kernel/src/policy/types.rs crates/trust-kernel/tests/w9_dag_modify_smoke.rs; git commit -m "test(w9p4): add validate_dag + risk ceiling escalation tests for modified_plan"
```

---

## Task 4: 后端 — 审计事件 dag_skeleton_modified + dag_modify_limit_exceeded

**Files:**
- 不改 `voicepilot/crates/trust-kernel/src/audit.rs`(复用 `audit_append_external`)
- Test: `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs`(Task 9 补齐 `modify_emits_complete_audit_events` + `second_modify_returns_dag_modify_limit_exceeded`)

**目标:** 验证 Task 2 Step 4 + Step 5 实现的 `dag_skeleton_modified` / `dag_modify_limit_exceeded` 审计事件按 spec §6.4 字段约束写入 `audit_logs` 表,字段命名 `lower_snake_case`,不记录 `input_template` 内容(隐私约束)。

- [ ] **Step 1: 确认 `dag_skeleton_modified` 审计调用**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,定位 Task 2 Step 4 加的 `dag_skeleton_modified` 审计调用:

```rust
self.kernel.audit_append_external(
    &root_task_id,
    None,
    "dag_skeleton_modified",
    serde_json::json!({
        "plan_id": plan.plan_id,
        "modified_node_count": modified_node_count,
        "added_count": added_count,
        "removed_count": removed_count,
    }),
)?;
```

确认 details 字段:
- `plan_id`:String
- `modified_node_count`:usize
- `added_count`:usize
- `removed_count`:usize
- **不含 `input_template` 内容**(spec §6.4 隐私约束)

Expected: 字段与 spec §6.4 表格一致。

- [ ] **Step 2: 确认 `dag_modify_limit_exceeded` 审计调用**

定位 Task 2 Step 5 加的 `dag_modify_limit_exceeded` 审计调用:

```rust
self.kernel.audit_append_external(
    root_task_id,
    None,
    "dag_modify_limit_exceeded",
    serde_json::json!({
        "plan_id": modified_plan.plan_id,
    }),
)?;
```

确认 details 字段:
- `plan_id`:String
- **不含其他字段**(spec §6.4 表格仅 `plan_id`)

Expected: 字段与 spec §6.4 表格一致。

- [ ] **Step 3: 写 `second_modify_returns_dag_modify_limit_exceeded` 测试**

在 `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs` 追加:

```rust
#[test]
fn second_modify_returns_dag_modify_limit_exceeded() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    let original_node = literal_text_node("n1", "note.capture", "原始");
    let original_plan = one_node_plan("w9p4-double-modify", original_node);

    let modified_plan_v1 = original_plan.clone();
    let modified_plan_v2 = original_plan.clone();

    // 第一次 Modify → 第二次仍 Modify → 应返回 DagModifyLimitExceeded
    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan_v1) },
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan_v2) },
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan);

    assert!(result.is_err(), "expected Err for second Modify");
    match result.unwrap_err() {
        trust_kernel::error::KernelError::DagModifyLimitExceeded { plan_id } => {
            assert_eq!(plan_id, "w9p4-double-modify");
        }
        other => panic!("expected DagModifyLimitExceeded, got {:?}", other),
    }

    // 验证审计:dag_modify_limit_exceeded 事件存在
    let conn = kernel.conn();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE event_type = 'dag_modify_limit_exceeded'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "dag_modify_limit_exceeded audit event must be emitted");
}
```

- [ ] **Step 4: 跑测试验证通过**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke second_modify_returns_dag_modify_limit_exceeded
```
Expected: PASS。

- [ ] **Step 5: 写 `modify_emits_complete_audit_events` 测试**

在 `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs` 追加:

```rust
#[test]
fn modify_emits_complete_audit_events() {
    use trust_kernel::repo::step_repo::StepRepo;

    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    let original_node = literal_text_node("n1", "note.capture", "原始");
    let original_plan = one_node_plan("w9p4-audit-chain", original_node);

    let modified_node = literal_text_node("n1", "note.capture", "修改后");
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) },
        DagApprovalOutcome::Allow,
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let _ = executor.run(&original_plan).unwrap();

    // 验证审计链(顺序)
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT event_type FROM audit_logs ORDER BY created_at ASC")
        .unwrap();
    let event_types: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    // 期望序列:dag_plan_created → dag_skeleton_approved(decision=modify) →
    //          dag_skeleton_modified → dag_skeleton_approved(decision=allow, phase=after_modify) →
    //          dag_node_started → dag_node_succeeded → dag_completed
    assert!(event_types.iter().any(|e| e == "dag_plan_created"), "missing dag_plan_created");
    assert!(event_types.iter().any(|e| e == "dag_skeleton_modified"), "missing dag_skeleton_modified");
    assert!(event_types.iter().any(|e| e == "dag_skeleton_approved"), "missing dag_skeleton_approved");

    // 验证 dag_skeleton_modified details 不含 input_template 内容(隐私约束)
    let mut stmt2 = conn
        .prepare("SELECT details FROM audit_logs WHERE event_type = 'dag_skeleton_modified'")
        .unwrap();
    let details_str: String = stmt2
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .next()
        .unwrap();
    // W9 修复(P1-9):解析 JSON 后检查字段,避免字符串 contains 误判(如 modified_node_count 中含 "node" 子串)
    let details: serde_json::Value = serde_json::from_str(&details_str).unwrap();
    assert!(details.get("input_template").is_none(), "details must not contain input_template field, got: {}", details_str);
    assert!(details.get("nodes").is_none(), "details must not contain nodes field (may contain input_template in nodes), got: {}", details_str);
    assert!(details.get("modified_node_count").is_some(), "details must contain modified_node_count, got: {}", details_str);
}
```

- [ ] **Step 6: 跑测试验证通过**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke modify_emits_complete_audit_events
```
Expected: PASS。若 `dag_skeleton_modified` details 含 `input_template` 字符串,回 Task 2 Step 4 检查 `serde_json::json!` 宏字段。

- [ ] **Step 7: 跑全部 w9_dag_modify_smoke 测试**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke
```
Expected: 5 个测试全 PASS(Task 9 补齐第 6 个 `modify_then_deny_cancels_dag`)。

- [ ] **Step 8: Commit**

```powershell
cd voicepilot; git add crates/trust-kernel/tests/w9_dag_modify_smoke.rs; git commit -m "test(w9p4): verify dag_skeleton_modified + dag_modify_limit_exceeded audit events"
```

---

## Task 5: UI 后端 — submit_dag_skeleton_approval 扩展 modified_plan 参数 + TauriApprover 携带 payload

**Files:**
- Modify: `voicepilot/crates/ui/src/approver.rs`(扩展 `ApprovalRegistry` + `TauriApprover` 适配新 trait 签名)
- Modify: `voicepilot/crates/ui/src/dag_commands.rs`(扩展 `submit_dag_skeleton_approval` + `approve_dag_skeleton_command`)
- Modify: `voicepilot/crates/ui/tests/approver_unit.rs`(适配新签名 + 加 Modify 路径测试)
- Modify: `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs`(既有测试加 `modified_plan: None` 参数)

**目标:** UI 后端扩展 IPC 链路:`submit_dag_skeleton_approval` 接收 `modified_plan: Option<DagPlan>` 参数,通过新 `DagApprovalPayload { decision, modified_plan }` 结构经 oneshot channel 投递;`TauriApprover::approve_dag_skeleton` 改返回 `KernelResult<DagApprovalOutcome>`,匹配 `decision` Allow / Deny / Modify(Modify 时取 `modified_plan` 字段,若 None 返回 `Err`)。

- [ ] **Step 1: 在 `approver.rs` 加 `DagApprovalPayload` 结构 + `ApprovalRegistry` 扩展**

打开 `voicepilot/crates/ui/src/approver.rs`,在 `DagApprovalRequestPayload` 结构(行 39-49)之后追加:

```rust
/// W9 Plan 4:携带 modified_plan 的 DAG 审批决策 payload。
///
/// 通过 oneshot channel 从 `submit_dag_skeleton_approval` 命令投递到
/// `TauriApprover::approve_dag_skeleton`(阻塞等待中)。
/// `decision = Modify` 时 `modified_plan` 必须为 `Some`,否则 TauriApprover
/// 返回 `Err(KernelError::Approval("Modify without modified_plan"))`。
// W9 修复(P1-14):仅 Serialize(从 UI 后端投递到 TauriApprover,无需 Deserialize)
#[derive(Debug, Clone, serde::Serialize)]
pub struct DagApprovalPayload {
    pub decision: ApprovalDecision,
    pub modified_plan: Option<DagPlan>,
}
```

然后在 `ApprovalRegistry` 结构(行 51-54)加 `dag_senders` 字段:

```rust
#[derive(Clone)]
pub struct ApprovalRegistry {
    senders: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
    /// W9 Plan 4:DAG 骨架审批专用 senders(与 `senders` 平行,不破坏既有 `prompt` 语义)。
    dag_senders: Arc<Mutex<HashMap<String, oneshot::Sender<DagApprovalPayload>>>>,
}
```

把 `ApprovalRegistry::new`(行 57-61)改为:

```rust
pub fn new() -> Self {
    Self {
        senders: Arc::new(Mutex::new(HashMap::new())),
        dag_senders: Arc::new(Mutex::new(HashMap::new())),
    }
}
```

**W9 修复(P0-6)测试 hook 要求:** `TauriApprover` 需暴露 `latest_approval_id() -> Option<String>` 和 `registry() -> &ApprovalRegistry` 测试方法(仅 `#[cfg(test)]`),用于测试协调时序(spawn 线程调 `approve_dag_skeleton_outcome`,主线程通过 `latest_approval_id()` 取 approval_id 后 `take_dag_sender` 投递 payload)。`TauriApprover` 需实现 `Clone`(或测试用 `Arc<TauriApprover>`)以便 spawn 线程移动所有权。示例:

```rust
#[cfg(test)]
impl TauriApprover {
    /// 返回最近一次 create_dag_request 生成的 approval_id(测试协调用)。
    pub fn latest_approval_id(&self) -> Option<String> {
        self.latest_dag_approval_id
            .lock()
            .unwrap()
            .clone()
    }

    /// 暴露 registry 引用(测试 take_dag_sender 用)。
    pub fn registry(&self) -> &ApprovalRegistry {
        &self.registry
    }
}
```

(在 `TauriApprover` 结构加 `latest_dag_approval_id: Arc<Mutex<Option<String>>>` 字段,`create_dag_request` 调用后更新此字段;`approve_dag_skeleton_outcome` 内 `let (approval_id, rx) = self.registry.create_dag_request();` 之后加 `*self.latest_dag_approval_id.lock().unwrap() = Some(approval_id.clone());`)

- [ ] **Step 2: 在 `ApprovalRegistry` impl 块加 DAG 专用方法**

在 `ApprovalRegistry` impl 块(行 56-104)`take_sender` 方法(行 81-83)之后追加:

```rust
    /// W9 Plan 4:创建 DAG 骨架审批请求(专用 oneshot channel)。
    ///
    /// 与 `create_request` 区别:
    /// - 用 `dag_xxx` 前缀的 approval_request_id(与 `apr_xxx` 区分)
    /// - 投递 `DagApprovalPayload`(含 modified_plan)而非 `ApprovalDecision`
    // W9 修复(P1-15):删除未使用的 plan 参数(原 plan 仅用于日志占位,不影响 channel 语义)
    pub fn create_dag_request(
        &self,
    ) -> (String, oneshot::Receiver<DagApprovalPayload>) {
        let approval_id = format!("dag_{}", Uuid::new_v4());
        let (tx, rx) = oneshot::channel::<DagApprovalPayload>();
        self.dag_senders
            .lock()
            .unwrap()
            .insert(approval_id.clone(), tx);
        (approval_id, rx)
    }

    /// W9 Plan 4:取出 DAG 骨架审批的 sender(由 `submit_dag_skeleton_approval` 调用)。
    pub fn take_dag_sender(&self, approval_id: &str) -> Option<oneshot::Sender<DagApprovalPayload>> {
        self.dag_senders.lock().unwrap().remove(approval_id)
    }

    /// W9 Plan 4:阻塞等待 DAG 审批决策,超时或 sender dropped 返回 Deny(默认安全)。
    // W9 修复(P0-5):用 Handle::try_current() 检测当前是否在 tokio runtime 内,
    // 避免 #[tokio::test] 上下文触发 "runtime within runtime" panic。
    pub fn wait_for_dag_decision(
        &self,
        rx: oneshot::Receiver<DagApprovalPayload>,
        timeout: Duration,
    ) -> DagApprovalPayload {
        let wait_fn = || async move {
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(payload)) => payload,
                Ok(Err(_)) => DagApprovalPayload {
                    decision: ApprovalDecision::Deny,
                    modified_plan: None,
                }, // sender dropped
                Err(_) => DagApprovalPayload {
                    decision: ApprovalDecision::Deny,
                    modified_plan: None,
                }, // timeout
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                // 已在 runtime 内(如 #[tokio::test]),用 handle.block_on 避免嵌套 runtime panic
                handle.block_on(wait_fn())
            }
            Err(_) => {
                // 不在 runtime 内(如 #[test] 同步上下文),新建 current_thread runtime
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .expect("failed to build tokio runtime");
                rt.block_on(wait_fn())
            }
        }
    }
```

- [ ] **Step 3: 重构 `TauriApprover::approve_dag_skeleton` 返回 `DagApprovalOutcome`**

在 `voicepilot/crates/ui/src/approver.rs` 中,把既有 inherent `pub fn approve_dag_skeleton(&self, plan: &DagPlan) -> ApprovalDecision`(行 181-206)重命名为 `pub fn approve_dag_skeleton_outcome(&self, plan: &DagPlan) -> KernelResult<DagApprovalOutcome>`,并改为:

```rust
    /// W9 Plan 4:DAG 骨架审批入口(实现 `Approver::approve_dag_skeleton` 逻辑)。
    ///
    /// 创建 oneshot channel + approval_request_id,emit `dag-approval-request`
    /// 事件给 webview,阻塞等待决策(5min timeout,默认 Deny)。
    /// 返回 `DagApprovalOutcome`(含 Modify payload)。
    pub fn approve_dag_skeleton_outcome(&self, plan: &DagPlan) -> KernelResult<DagApprovalOutcome> {
        use trust_kernel::approval::approver::DagApprovalOutcome;
        use trust_kernel::error::KernelError;

        // W9 修复(P1-15):create_dag_request 不再接收 plan 参数
        let (approval_id, rx) = self.registry.create_dag_request();

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

        let dag_payload = self.registry.wait_for_dag_decision(rx, DEFAULT_APPROVAL_TIMEOUT);
        match dag_payload.decision {
            ApprovalDecision::Allow => Ok(DagApprovalOutcome::Allow),
            ApprovalDecision::Deny => Ok(DagApprovalOutcome::Deny),
            ApprovalDecision::Modify => {
                let modified_plan = dag_payload.modified_plan.ok_or_else(|| {
                    KernelError::Approval("Modify without modified_plan".into())
                })?;
                Ok(DagApprovalOutcome::Modify {
                    modified_plan: Box::new(modified_plan),
                })
            }
        }
    }
```

- [ ] **Step 4: 更新 `Approver` trait impl 块**

把 `TauriApprover` 的 `Approver` trait impl 块中(行 161-163)`approve_dag_skeleton` 方法从:

```rust
fn approve_dag_skeleton(&self, plan: &DagPlan) -> KernelResult<ApprovalDecision> {
    Ok(TauriApprover::approve_dag_skeleton(self, plan))
}
```

改为:

```rust
fn approve_dag_skeleton(&self, plan: &DagPlan) -> KernelResult<DagApprovalOutcome> {
    TauriApprover::approve_dag_skeleton_outcome(self, plan)
}
```

并在文件顶部 `use` 段加:

```rust
use trust_kernel::approval::approver::DagApprovalOutcome;
```

- [ ] **Step 5: 更新 `create_dag_approval_request_for_test` 返回类型**

把既有 `create_dag_approval_request_for_test`(行 210-221)从返回 `oneshot::Receiver<ApprovalDecision>` 改为返回 `oneshot::Receiver<DagApprovalPayload>`:

```rust
    /// 测试用:不 emit 事件,直接返回 approval_request_id + receiver。
    /// 单元测试调 `take_dag_sender(id).send(DagApprovalPayload { decision, modified_plan })` 模拟用户决策。
    // W9 修复(P1-15):删除未使用的 plan 参数(委托 create_dag_request,后者已删 plan 参数)
    pub fn create_dag_approval_request_for_test(
        &self,
    ) -> (String, oneshot::Receiver<DagApprovalPayload>) {
        self.registry.create_dag_request()
    }
```

- [ ] **Step 6: 扩展 `submit_dag_skeleton_approval` 逻辑函数**

**W9 修复(P1-16):** 实现前先核实 `DagApprovalDecision` 的 serde 属性:在 `voicepilot/crates/ui/src/dag_commands.rs` grep `DagApprovalDecision` 的 `#[serde(...)]`,确认是 `rename_all = "lowercase"`(前端 `types.ts` 用 `"allow" | "deny" | "modify"`)。若不是 `lowercase`,前端类型需对应调整(如 `PascalCase` → `"Allow" | "Deny" | "Modify"`)。

打开 `voicepilot/crates/ui/src/dag_commands.rs`,把 `submit_dag_skeleton_approval`(行 47-58)从:

```rust
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
```

改为:

```rust
use trust_kernel::skills::dag_types::DagPlan;
use crate::approver::DagApprovalPayload;

/// 提交 DAG 骨架审批决策(W9 Plan 4:支持 modified_plan payload)。
///
/// 由 webview `DagApprovalDialog` 在用户点击 Allow/Deny/Modify 后调用:
/// - Allow / Deny:`modified_plan = None`
/// - Modify:`modified_plan = Some(DagPlan)`(用户编辑后的 plan)
///
/// 通过 `ApprovalRegistry::take_dag_sender` 取出 oneshot sender,投递 `DagApprovalPayload`。
/// 返回 true = 投递成功,false = 请求已被消费 / 已过期 / 不存在(一次性语义)。
pub fn submit_dag_skeleton_approval(
    state: &AppState,
    approval_request_id: &str,
    decision: DagApprovalDecision,
    modified_plan: Option<DagPlan>,
) -> UiResult<bool> {
    let sender = match state.approval_registry.take_dag_sender(approval_request_id) {
        Some(s) => s,
        None => return Ok(false),
    };
    let payload = DagApprovalPayload {
        decision: decision.into(),
        modified_plan,
    };
    let _ = sender.send(payload);
    Ok(true)
}
```

- [ ] **Step 7: 扩展 `approve_dag_skeleton_command` Tauri 命令**

**W9 修复(P1-12):** 核实 `#[tauri::command(rename_all = "camelCase")]` 或确认 W8 既有 `approveDagSkeleton` 调用已正常工作 — 若 W8 工作则 camelCase 转换已生效,Plan 4 沿用即可(前端 `approveDagSkeleton` 传 `{ approvalRequestId, decision, modifiedPlan }`,Tauri 自动转为 `approval_request_id` / `decision` / `modified_plan`)。若 W8 未工作,需在 `approve_dag_skeleton_command` 上加 `#[tauri::command(rename_all = "camelCase")]`。

把 `approve_dag_skeleton_command`(行 60-68)从:

```rust
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

改为:

```rust
#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn approve_dag_skeleton_command(
    state: State<'_, AppState>,
    approval_request_id: String,
    decision: DagApprovalDecision,
    modified_plan: Option<DagPlan>,
) -> Result<bool, String> {
    submit_dag_skeleton_approval(&state, &approval_request_id, decision, modified_plan).map_err(Into::into)
}
```

- [ ] **Step 8: 适配既有测试(全量盘点)**

**W9 修复(P0-4):** inherent `approve_dag_skeleton` 已重命名为 `approve_dag_skeleton_outcome`,W8 既有测试若直接调 inherent 会编译失败。执行 `grep -r "approve_dag_skeleton" voicepilot/crates/` 全 workspace,更新所有调用点:

1. `crates/ui/tests/approver_unit.rs` — 所有 `approver.approve_dag_skeleton(&plan)` 调用改为 `approver.approve_dag_skeleton_outcome(&plan)` 或通过 trait 调用
2. `crates/ui/tests/w8_dag_commands_unit.rs` — 同上(若有直接调 inherent)
3. `crates/trust-kernel/tests/e2e_dag_smoke.rs` — 同上
4. `crates/trust-kernel/tests/w8_plan2_dag_e2e.rs` — 同上
5. 任何其他 grep 命中的文件

`create_dag_approval_request_for_test` 的 receiver 类型从 `oneshot::Receiver<ApprovalDecision>` 改为 `oneshot::Receiver<DagApprovalPayload>`,所有调用方同步更新(W9 修复 P1-15:已删除 plan 参数,调用方不再传 `&plan`)。

打开 `voicepilot/crates/ui/tests/approver_unit.rs`,把所有 `TauriApprover::approve_dag_skeleton` 调用的断言从 `ApprovalDecision` 改为 `DagApprovalOutcome`。例如:

```rust
// 旧:
let decision = approver.approve_dag_skeleton(&plan);
assert_eq!(decision, ApprovalDecision::Allow);

// 新:
let outcome = approver.approve_dag_skeleton_outcome(&plan).unwrap();
assert!(matches!(outcome, DagApprovalOutcome::Allow));
```

并加 1 个 Modify 路径测试(**W9 修复 P0-6:** 用 spawn 线程协调时序,主线程 take_dag_sender 投递 payload — 原版先创建 approval_id + receiver,投递 payload 到第一个 sender,但 approve_dag_skeleton 内部创建第二个 approval_id,5min 超时返回 Deny):

```rust
#[test]
fn approve_dag_skeleton_modify_returns_outcome_with_modified_plan() {
    use trust_kernel::approval::approver::DagApprovalOutcome;
    use trust_kernel::skills::dag_types::{DagNode, DagPlan};
    use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

    let registry = ApprovalRegistry::new();
    let approver = TauriApprover::new(registry.clone());
    let plan = DagPlan {
        plan_id: "test-modify".into(),
        user_goal: "测试 Modify".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "note.capture".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("原始".into()),
            },
            risk_ceiling: trust_kernel::policy::types::ELevel::E1,
        }],
        edges: vec![],
        loop_specs: std::collections::HashMap::new(),
        max_total_steps: 5,
    };

    // W9 修复(P0-6):用 spawn 线程调 approve_dag_skeleton_outcome,主线程 take_dag_sender 投递 payload
    // TauriApprover 需 Clone(若不 Clone,用 Arc),plan 也需 Clone
    let approver_clone = approver.clone();
    let plan_clone = plan.clone();
    let handle = std::thread::spawn(move || {
        approver_clone.approve_dag_skeleton_outcome(&plan_clone)
    });

    // 主线程等待 approval_request 创建后投递 payload
    // 用 poll 方式取 approval_id(或 TauriApprover 暴露 latest_approval_id() 测试 hook)
    std::thread::sleep(std::time::Duration::from_millis(100));
    let approval_id = approver.latest_approval_id().expect("approval_id must be created");
    let sender = approver.registry().take_dag_sender(&approval_id).unwrap();
    sender.send(crate::approver::DagApprovalPayload {
        decision: ApprovalDecision::Modify,
        modified_plan: Some(plan.clone()),
    }).unwrap();

    let outcome = handle.join().unwrap().unwrap();
    match outcome {
        DagApprovalOutcome::Modify { modified_plan: mp } => {
            assert_eq!(mp.plan_id, "test-modify");
            assert_eq!(mp.nodes.len(), plan.nodes.len());
        }
        other => panic!("expected Modify, got {:?}", other),
    }
}
```

- [ ] **Step 9: 适配既有 `w8_dag_commands_unit.rs` 测试**

打开 `voicepilot/crates/ui/tests/w8_dag_commands_unit.rs`,把所有 `submit_dag_skeleton_approval` 调用加 `modified_plan: None` 参数。例如:

```rust
// 旧:
let ok = submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Allow).unwrap();

// 新:
let ok = submit_dag_skeleton_approval(&state, &approval_id, DagApprovalDecision::Allow, None).unwrap();
```

- [ ] **Step 10: 跑 `cargo check` + `cargo test`**

Run:
```powershell
cd voicepilot; cargo check --workspace --features voice,tauri,llm; cargo test --features voice,tauri,llm -p ui --test approver_unit; cargo test --features voice,tauri,llm -p ui --test w8_dag_commands_unit
```
Expected: 编译通过 + 测试 PASS。

- [ ] **Step 11: Commit**

```powershell
cd voicepilot; git add crates/ui/src/approver.rs crates/ui/src/dag_commands.rs crates/ui/tests/approver_unit.rs crates/ui/tests/w8_dag_commands_unit.rs; git commit -m "feat(w9p4): extend TauriApprover + submit_dag_skeleton_approval with modified_plan payload"
```

---

## Task 6: UI 前端 — DagApprovalDialog 激活 Modify 按钮 + 编辑模式切换

**Files:**
- Modify: `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx`(激活 Modify 按钮 + `editingMode` state + 编辑模式渲染)

**目标:** 把前端 `DagApprovalDialog` 的 Modify 按钮从 `disabled` 改为可点击,点击后切换到编辑模式,渲染 `NodeEditor` 列表(Task 7 实现)+ "添加节点" / "提交修改" / "取消" 按钮。用户编辑后点 "提交修改" 调 `approveDagSkeleton(approval_request_id, "modify", modifiedPlan)`。

- [ ] **Step 1: 在 `DagApprovalDialog.tsx` 加 `editingMode` + `editedNodes` state**

打开 `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx`,在 `export function DagApprovalDialog` 内(行 55-68 之间)加 state:

```tsx
const [editingMode, setEditingMode] = useState(false);
const [editedNodes, setEditedNodes] = useState<DagNode[]>(() => parsePlanJson(plan_json).nodes);
```

注意:`editedNodes` 初始化用 `parsePlanJson(plan_json).nodes`(parsePlanJson 在行 16-25 已存在);`useState` lazy initializer 避免每次渲染重算。

- [ ] **Step 2: 实现 `handleModifySubmit` 函数**

在 `decide` 函数(行 70-83)之后追加:

```tsx
async function handleModifySubmit(): Promise<void> {
  setSubmitting(true);
  setError(null);
  try {
    // 构造 modified_plan(完整 DagPlan,复用原 plan 的 plan_id / user_goal / edges / loop_specs / max_total_steps)
    const originalPlan = parsePlanJson(plan_json);
    const modifiedPlan = {
      plan_id,
      user_goal,
      nodes: editedNodes,
      edges: originalPlan.edges,
      loop_specs: {}, // W9 不支持改 loop_specs,保留空(spec §7 延后项)
      max_total_steps,
    };
    await approveDagSkeleton(approval_request_id, "modify", modifiedPlan);
    // W9 修复(P1-13):成功后才设 submittedRef.current = true,失败时保持 false
    // — 这样失败后若用户关闭对话框,cleanup useEffect 仍能发 Deny(避免 approval_request 悬空)
    submittedRef.current = true;
    onDismiss();
  } catch (e) {
    // W9 修复(P1-13):submittedRef.current 不设,允许 cleanup 在 unmount 时发 Deny
    setError(e instanceof Error ? e.message : String(e));
    console.error(e);
  } finally {
    setSubmitting(false);
  }
}
```

注意:`modifiedPlan.loop_specs` 设为 `{}`(空对象),因前端 `DagApprovalRequestPayload.plan_json` 不含 `loop_specs` 详细信息(spec §7 延后项,W10+ 实现);后端 `SlotTemplateEngine::validate_dag` 对 `loop_specs = {}` 不报错(既有实现兼容)。

- [ ] **Step 3: 激活 Modify 按钮 + 加编辑模式渲染**

把行 211-240 的 `<div className="modal-footer">` 内容改为:

```tsx
<div className="modal-footer">
  {!editingMode ? (
    <>
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
        onClick={() => setEditingMode(true)}
        disabled={submitting}
        aria-label="调整 DAG 节点"
      >
        调整(Modify)
      </button>
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
    </>
  ) : (
    <>
      <button
        type="button"
        className="btn"
        onClick={() => setEditingMode(false)}
        disabled={submitting}
        aria-label="取消编辑"
      >
        取消
      </button>
      <button
        type="button"
        className="btn btn-primary"
        onClick={handleModifySubmit}
        disabled={submitting}
        aria-label="提交修改后的 DAG"
      >
        提交修改
      </button>
    </>
  )}
</div>
```

注意:移除原 Modify 按钮的 `disabled` + `title="W9+ 实现"` + `aria-disabled="true"`(三处)。

- [ ] **Step 4: 在 `modal-body` 加编辑模式节点列表**

把行 148-179 的 `<div className="dag-nodes-grid">...</div>` 内容改为(只读 + 编辑模式分别渲染):

```tsx
<div className="dag-nodes-grid" role="group" aria-label="DAG 节点列表">
  <h3 className="dag-section-title">
    节点({editingMode ? editedNodes.length : nodes.length})
  </h3>
  <div className="dag-nodes-list">
    {!editingMode
      ? nodes.map((node) => (
          <div key={node.node_id} className="dag-node-card">
            <div className="dag-node-header">
              <span className="dag-node-id mono">{node.node_id}</span>
              <span className="dag-node-skill mono">{node.skill_id}</span>
              <span
                className={riskBadgeClass(node.risk_ceiling)}
                aria-label={`风险等级 ${node.risk_ceiling}`}
              >
                {node.risk_ceiling}
              </span>
            </div>
            <div className="dag-node-body">
              <details className="dag-template-preview">
                <summary className="dag-summary">输入模板</summary>
                <pre className="dag-template-code mono">
                  {renderTemplatePreview(node.input_template_json)}
                </pre>
              </details>
            </div>
          </div>
        ))
      : editedNodes.map((node) => (
          <NodeEditor
            key={node.node_id}
            node={node}
            onChange={(updated) =>
              setEditedNodes((prev) =>
                prev.map((n) => (n.node_id === updated.node_id ? updated : n))
              )
            }
            onDelete={() =>
              setEditedNodes((prev) =>
                prev.filter((n) => n.node_id !== node.node_id)
              )
            }
          />
        ))}
    {editingMode && (
      <button
        type="button"
        className="btn btn-secondary dag-add-node-btn"
        onClick={() =>
          setEditedNodes((prev) => [
            ...prev,
            {
              node_id: `n${prev.length + 1}_${Date.now()}`,
              skill_id: "note.capture",
              risk_ceiling: "E1",
              status: "pending",
              input_template_json: JSON.stringify({
                kind: "text",
                template: { Literal: "" },
              }),
            },
          ])
        }
        aria-label="添加新节点"
      >
        + 添加节点
      </button>
    )}
  </div>
</div>
```

注意:`NodeEditor` 组件在 Task 7 实现,本 Step 先 import 占位(Step 5)。

- [ ] **Step 5: import `NodeEditor`**

在 `DagApprovalDialog.tsx` 顶部(行 1-8)import 段加:

```tsx
import { NodeEditor } from "./NodeEditor";
```

- [ ] **Step 6: 跑 vitest 验证不回归**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run test -- --run DagApprovalDialog
```
Expected: 既有 5 个 DagApprovalDialog 测试 PASS(若失败因 `NodeEditor` 未创建,先跑 Task 7 创建组件,再回本 Task 跑测试)。

- [ ] **Step 7: Commit**

```powershell
cd voicepilot; git add crates/ui/web/src/components/DagApprovalDialog.tsx; git commit -m "feat(w9p4): activate Modify button + editing mode in DagApprovalDialog"
```

---

## Task 7: UI 前端 — NodeEditor 组件(增删节点 + input_template textarea + risk_ceiling select)

**Files:**
- Create: `voicepilot/crates/ui/web/src/components/NodeEditor.tsx`(新组件)
- Create: `voicepilot/crates/ui/web/src/components/__tests__/NodeEditor.test.tsx`(vitest 测试,Task 10 完整 TDD)

**目标:** 实现 `NodeEditor` 组件,提供单节点编辑界面:`node_id` 文本输入(只读,改 node_id 需重建)、`skill_id` 文本输入、`risk_ceiling` `<select>`(E0/E1/E2/E3)、`input_template_json` `<textarea>`(行高 6,monospace)、"删除节点" 按钮。任一字段变化时调 `onChange(updated)`,删除按钮调 `onDelete()`。

- [ ] **Step 1: 写最小 vitest 测试(先失败)**

创建 `voicepilot/crates/ui/web/src/components/__tests__/NodeEditor.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { NodeEditor } from "../NodeEditor";
import type { DagNode } from "../../types";

const sampleNode: DagNode = {
  node_id: "n1",
  skill_id: "note.capture",
  risk_ceiling: "E1",
  status: "pending",
  input_template_json: JSON.stringify({
    kind: "text",
    template: { Literal: "原始 TODO" },
  }),
};

describe("NodeEditor", () => {
  it("renders_node_fields_correctly", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    expect(screen.getByDisplayValue("n1")).toBeInTheDocument();
    expect(screen.getByDisplayValue("note.capture")).toBeInTheDocument();
    expect(screen.getByDisplayValue("E1")).toBeInTheDocument();
    expect(screen.getByText(/原始 TODO/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /删除节点/ })).toBeInTheDocument();
  });

  it("delete_node_calls_onDelete", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    fireEvent.click(screen.getByRole("button", { name: /删除节点/ }));
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("change_risk_ceiling_calls_onChange", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    const select = screen.getByDisplayValue("E1");
    fireEvent.change(select, { target: { value: "E2" } });
    expect(onChange).toHaveBeenCalledWith({ ...sampleNode, risk_ceiling: "E2" });
  });

  it("change_input_template_calls_onChange", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    const textarea = screen.getByRole("textbox", { name: /输入模板/ });
    fireEvent.change(textarea, { target: { value: '{"kind":"text","template":{"Literal":"新值"}}' } });
    expect(onChange).toHaveBeenCalledWith({
      ...sampleNode,
      input_template_json: '{"kind":"text","template":{"Literal":"新值"}}',
    });
  });
});
```

- [ ] **Step 2: 跑测试验证失败**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run test -- --run NodeEditor
```
Expected: FAIL,因 `NodeEditor` 组件不存在。

- [ ] **Step 3: 创建 `NodeEditor.tsx` 组件**

**W9 修复(P1-11):** `input_template_json` textarea 加 JSON 校验 — onChange 内 `try { JSON.parse(value); setInvalid(false) } catch { setInvalid(true) }`,invalid 时 textarea 红色边框 + 错误提示 "Invalid JSON",并通过 `onValidityChange` 回调让父组件(DagApprovalDialog)禁用 "提交修改" 按钮。

创建 `voicepilot/crates/ui/web/src/components/NodeEditor.tsx`:

```tsx
import { useState } from "react";
import type { DagNode } from "../types";

interface Props {
  node: DagNode;
  onChange(updated: DagNode): void;
  onDelete(): void;
  /** W9 修复(P1-11):JSON 校验结果变化时通知父组件(用于禁用提交按钮)。 */
  onValidityChange?(valid: boolean): void;
}

/** W9 Plan 4:单节点编辑器(供 DagApprovalDialog 编辑模式使用)。 */
export function NodeEditor({ node, onChange, onDelete, onValidityChange }: Props): JSX.Element {
  // W9 修复(P1-11):input_template_json JSON 校验状态
  const [invalid, setInvalid] = useState(false);

  const handleTemplateChange = (value: string) => {
    const updated = { ...node, input_template_json: value };
    onChange(updated);
    try {
      JSON.parse(value);
      setInvalid(false);
      onValidityChange?.(true);
    } catch {
      setInvalid(true);
      onValidityChange?.(false);
    }
  };

  return (
    <div className="dag-node-editor" role="group" aria-label={`节点 ${node.node_id} 编辑器`}>
      <div className="dag-node-editor-header">
        <label htmlFor={`node-id-${node.node_id}`} className="dag-label">
          节点 ID
        </label>
        <input
          id={`node-id-${node.node_id}`}
          type="text"
          className="mono"
          value={node.node_id}
          readOnly
          aria-readonly="true"
        />
        <button
          type="button"
          className="btn btn-danger dag-delete-node-btn"
          onClick={onDelete}
          aria-label={`删除节点 ${node.node_id}`}
        >
          删除节点
        </button>
      </div>

      <div className="dag-node-editor-field">
        <label htmlFor={`skill-id-${node.node_id}`} className="dag-label">
          Skill ID
        </label>
        <input
          id={`skill-id-${node.node_id}`}
          type="text"
          className="mono"
          value={node.skill_id}
          onChange={(e) => onChange({ ...node, skill_id: e.target.value })}
        />
      </div>

      <div className="dag-node-editor-field">
        <label htmlFor={`risk-ceiling-${node.node_id}`} className="dag-label">
          风险上限
        </label>
        <select
          id={`risk-ceiling-${node.node_id}`}
          value={node.risk_ceiling}
          onChange={(e) => onChange({ ...node, risk_ceiling: e.target.value })}
        >
          <option value="E0">E0(无风险)</option>
          <option value="E1">E1(低风险)</option>
          <option value="E2">E2(中风险)</option>
          <option value="E3">E3(高风险)</option>
        </select>
      </div>

      <div className="dag-node-editor-field">
        <label htmlFor={`input-template-${node.node_id}`} className="dag-label">
          输入模板(JSON)
        </label>
        <textarea
          id={`input-template-${node.node_id}`}
          className={`mono dag-template-textarea${invalid ? " dag-template-invalid" : ""}`}
          rows={6}
          value={node.input_template_json}
          onChange={(e) => handleTemplateChange(e.target.value)}
          aria-label={`节点 ${node.node_id} 输入模板`}
          aria-invalid={invalid}
          spellCheck={false}
        />
        {invalid && (
          <span className="dag-template-error" role="alert">
            Invalid JSON
          </span>
        )}
      </div>
    </div>
  );
}
```

**W9 修复(P1-11)续:** DagApprovalDialog 需跟踪所有 NodeEditor 的 validity,任一 invalid 时 "提交修改" 按钮 disabled。在 `DagApprovalDialog.tsx` 加 state:

```tsx
const [invalidNodeIds, setInvalidNodeIds] = useState<Set<string>>(new Set());
```

NodeEditor 渲染处加 `onValidityChange`:
```tsx
<NodeEditor
  key={node.node_id}
  node={node}
  onChange={(updated) => setEditedNodes((prev) => prev.map((n) => (n.node_id === updated.node_id ? updated : n)))}
  onDelete={() => setEditedNodes((prev) => prev.filter((n) => n.node_id !== node.node_id))}
  onValidityChange={(valid) => setInvalidNodeIds((prev) => {
    const next = new Set(prev);
    if (valid) next.delete(node.node_id); else next.add(node.node_id);
    return next;
  })}
/>
```

"提交修改" 按钮 `disabled={submitting || invalidNodeIds.size > 0}`。

CSS 加 `.dag-template-invalid { border-color: var(--danger); } .dag-template-error { color: var(--danger); font-size: 12px; }`(Task 7 Step 5 CSS 段落追加)。

- [ ] **Step 4: 跑测试验证通过**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run test -- --run NodeEditor
```
Expected: 4 个测试全 PASS。

- [ ] **Step 5: 加 CSS 样式**

打开 `voicepilot/crates/ui/web/src/styles.css`,在末尾追加:

```css
/* W9 Plan 4: NodeEditor 样式 */
.dag-node-editor {
  border: 1px solid var(--border);
  padding: 12px;
  margin-bottom: 8px;
  background: var(--bg-deep);
}

.dag-node-editor-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.dag-node-editor-header input {
  flex: 1;
  padding: 4px 8px;
  background: var(--bg-base);
  border: 1px solid var(--border);
  color: var(--text-muted);
}

.dag-node-editor-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 8px;
}

.dag-node-editor-field select,
.dag-node-editor-field input {
  padding: 4px 8px;
  background: var(--bg-base);
  border: 1px solid var(--border);
  color: var(--text);
}

.dag-template-textarea {
  width: 100%;
  font-family: var(--font-mono);
  font-size: 13px;
  resize: vertical;
  background: var(--bg-base);
  border: 1px solid var(--border);
  color: var(--text);
  padding: 8px;
}

.dag-add-node-btn {
  margin-top: 8px;
}

.dag-delete-node-btn {
  padding: 4px 8px;
  font-size: 12px;
}
```

- [ ] **Step 6: 跑 DagApprovalDialog 测试验证集成**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run test -- --run DagApprovalDialog
```
Expected: PASS(既有 5 个测试 + Task 10 新增 NodeEditor 集成测试)。

- [ ] **Step 7: Commit**

```powershell
cd voicepilot; git add crates/ui/web/src/components/NodeEditor.tsx crates/ui/web/src/components/__tests__/NodeEditor.test.tsx crates/ui/web/src/styles.css; git commit -m "feat(w9p4): add NodeEditor component for editing DAG nodes"
```

---

## Task 8: UI 前端 — api.ts + types.ts 扩展(修改 DagApprovalDecision 类型 + approveDagSkeleton API)

**Files:**
- Modify: `voicepilot/crates/ui/web/src/api.ts`(扩展 `approveDagSkeleton` 签名加 `modifiedPlan?`)
- Modify: `voicepilot/crates/ui/web/src/types.ts`(新增 `DagPlanFull` 接口 + `DagApprovalRequestPayload.plan_json` 类型收紧)

**目标:** 前端 `approveDagSkeleton` API 加可选 `modifiedPlan?: DagPlanFull` 参数,与后端 `approve_dag_skeleton_command` 的 `modified_plan: Option<DagPlan>` 对齐;新增 `DagPlanFull` 接口供 `handleModifySubmit` 构造完整 modified_plan。

- [ ] **Step 1: 在 `types.ts` 新增 `DagPlanFull` 接口**

打开 `voicepilot/crates/ui/web/src/types.ts`,在 `DagApprovalRequestPayload` 接口(行 258-265)之后追加:

```ts
/** W9 Plan 4:完整 DagPlan(供 Modify 时构造 modified_plan 用)。 */
export interface DagPlanFull {
  plan_id: string;
  user_goal: string;
  nodes: DagNode[];
  edges: DagEdge[];
  loop_specs: Record<string, unknown>;
  max_total_steps: number;
}
```

注意:`loop_specs` 用 `Record<string, unknown>` 因前端不编辑循环规格(spec §7 延后项),保留为空对象即可。

- [ ] **Step 2: 收紧 `DagApprovalRequestPayload.plan_json` 类型**

把 `DagApprovalRequestPayload`(行 258-265)的 `plan_json: unknown` 改为 `plan_json: DagPlanFull`:

```ts
export interface DagApprovalRequestPayload {
  approval_request_id: string;
  plan_id: string;
  user_goal: string;
  max_total_steps: number;
  node_count: number;
  plan_json: DagPlanFull;
}
```

注意:此改动可能让 `DagApprovalDialog.tsx` 的 `parsePlanJson(plan_json: unknown)` 类型不匹配 — 把 `parsePlanJson` 参数类型改为 `DagPlanFull`,直接返回 `{ nodes: plan_json.nodes, edges: plan_json.edges }`,无需 `as` 强转。

- [ ] **Step 3: 调整 `parsePlanJson` 签名**

**W9 修复(P1-10):** `DagPlanFull | unknown` 等价于 `unknown`(TS 联合类型含 unknown 时收窄为 unknown),原签名无类型保护。改为用 type guard 校验 `nodes` 字段存在,签名保持 `unknown` 但内部 throw 异常而非静默返回空数组(让调用方感知错误):

在 `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx` 中,把 `parsePlanJson`(行 16-25)从:

```tsx
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
```

改为:

```tsx
// W9 修复(P1-10):用 type guard 校验 nodes 字段,签名保持 unknown(因 DagPlanFull | unknown 等价 unknown)
function parsePlanJson(planJson: unknown): { nodes: DagNode[]; edges: DagEdge[] } {
  if (typeof planJson !== "object" || planJson === null || !("nodes" in planJson)) {
    throw new Error("Invalid plan_json: missing nodes");
  }
  return planJson as { nodes: DagNode[]; edges: DagEdge[] };
}
```

并在顶部 import 加 `DagPlanFull`(供 `handleModifySubmit` 构造 modifiedPlan 用,Step 2 已收紧 `plan_json: DagPlanFull`):

```tsx
import type {
  DagApprovalDecision,
  DagApprovalRequestPayload,
  DagEdge,
  DagNode,
  DagPlanFull,
} from "../types";
```

- [ ] **Step 4: 扩展 `approveDagSkeleton` API 签名**

打开 `voicepilot/crates/ui/web/src/api.ts`,把 `approveDagSkeleton`(行 202-210)从:

```tsx
export async function approveDagSkeleton(
  approvalRequestId: string,
  decision: DagApprovalDecision
): Promise<boolean> {
  return invoke<boolean>("approve_dag_skeleton_command", {
    approvalRequestId,
    decision,
  });
}
```

改为:

```tsx
export async function approveDagSkeleton(
  approvalRequestId: string,
  decision: DagApprovalDecision,
  modifiedPlan?: DagPlanFull
): Promise<boolean> {
  return invoke<boolean>("approve_dag_skeleton_command", {
    approvalRequestId,
    decision,
    modifiedPlan,
  });
}
```

并在 `api.ts` 顶部 import 段加 `DagPlanFull`:

```tsx
import type {
  // ... 既有 imports
  DagPlanFull,
  // ...
} from "./types";
```

- [ ] **Step 5: 跑 `npm.cmd run build` 验证 TypeScript 编译**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run build
```
Expected: PASS(无 TypeScript 错误)。若报 `DagPlanFull` 未导入 / `modifiedPlan` 未声明等,按错误信息修复。

- [ ] **Step 6: 跑 vitest 验证不回归**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run test -- --run
```
Expected: 既有 + 新增测试全 PASS。

- [ ] **Step 7: Commit**

```powershell
cd voicepilot; git add crates/ui/web/src/api.ts crates/ui/web/src/types.ts crates/ui/web/src/components/DagApprovalDialog.tsx; git commit -m "feat(w9p4): extend approveDagSkeleton API with optional modifiedPlan parameter"
```

---

## Task 9: TDD — w9_dag_modify_smoke.rs 集成测试(6 个完整场景)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs`(补齐 6 个测试)

**目标:** 在 Task 2-4 已写的 5 个测试基础上,补齐第 6 个 `modify_then_deny_cancels_dag`,并验证 6 个测试全 PASS。

- [ ] **Step 1: 补齐 `modify_then_deny_cancels_dag` 测试**

打开 `voicepilot/crates/trust-kernel/tests/w9_dag_modify_smoke.rs`,追加:

```rust
#[test]
fn modify_then_deny_cancels_dag() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    let original_node = literal_text_node("n1", "note.capture", "原始");
    let original_plan = one_node_plan("w9p4-modify-deny", original_node);

    let modified_plan = original_plan.clone();

    // 第一次 Modify → 第二次 Deny
    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) },
        DagApprovalOutcome::Deny,
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan).unwrap();

    // 验证:DagStatus::Cancelled
    assert!(matches!(result.status, trust_kernel::skills::dag_types::DagStatus::Cancelled),
        "expected Cancelled, got {:?}", result.status);

    // 验证审计:dag_completed(final_status=cancelled) 存在
    let conn = kernel.conn();
    let count: i64 = conn
        .query_row(
            // W9 修复(P2-20):用 json_extract 替代 details LIKE,避免 JSON 字段顺序/转义导致 LIKE 失败
            "SELECT COUNT(*) FROM audit_logs WHERE event_type = 'dag_completed' AND json_extract(details, '$.final_status') = 'cancelled'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "dag_completed(cancelled) audit event must be emitted");
}
```

- [ ] **Step 2: 跑全部 6 个测试**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke
```
Expected: 6 个测试全 PASS:
1. `modify_then_approve_allow_runs_modified_plan` ✓
2. `modify_then_deny_cancels_dag` ✓
3. `second_modify_returns_dag_modify_limit_exceeded` ✓
4. `modify_with_invalid_modified_plan_fails_validation` ✓
5. `modify_with_escalated_risk_ceiling_rejected` ✓
6. `modify_emits_complete_audit_events` ✓

- [ ] **Step 3: 跑 clippy 验证无警告**

Run:
```powershell
cd voicepilot; cargo clippy --workspace --features voice,tauri,llm -- -D warnings
```
Expected: 0 警告。若有 `unused_imports` 等,按提示修复。

- [ ] **Step 4: Commit**

```powershell
cd voicepilot; git add crates/trust-kernel/tests/w9_dag_modify_smoke.rs; git commit -m "test(w9p4): complete 6 dag_modify_smoke integration tests"
```

---

## Task 10: 前端 vitest 测试(NodeEditor 渲染 / 增删节点 / input_template 编辑 / 提交 modified_plan)

**Files:**
- Modify: `voicepilot/crates/ui/web/src/components/__tests__/NodeEditor.test.tsx`(Task 7 已写 4 个,本 Task 补齐集成测试)
- Create: `voicepilot/crates/ui/web/src/components/__tests__/DagApprovalDialog.modify.test.tsx`(DagApprovalDialog Modify 路径集成测试)

**目标:** 验证 NodeEditor 组件渲染 / 增删节点 / input_template 编辑 / 提交 modified_plan 完整闭环。

- [ ] **Step 1: 创建 DagApprovalDialog Modify 路径测试**

创建 `voicepilot/crates/ui/web/src/components/__tests__/DagApprovalDialog.modify.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { DagApprovalDialog } from "../DagApprovalDialog";
import type { DagApprovalRequestPayload } from "../../types";

vi.mock("../../api", () => ({
  approveDagSkeleton: vi.fn().mockResolvedValue(true),
}));

import { approveDagSkeleton } from "../../api";

const samplePayload: DagApprovalRequestPayload = {
  approval_request_id: "dag_test_123",
  plan_id: "w9p4-test",
  user_goal: "测试 Modify 路径",
  max_total_steps: 5,
  node_count: 1,
  plan_json: {
    plan_id: "w9p4-test",
    user_goal: "测试 Modify 路径",
    nodes: [
      {
        node_id: "n1",
        skill_id: "note.capture",
        risk_ceiling: "E1",
        status: "pending",
        input_template_json: JSON.stringify({
          kind: "text",
          template: { Literal: "原始" },
        }),
      },
    ],
    edges: [],
    loop_specs: {},
    max_total_steps: 5,
  },
};

describe("DagApprovalDialog Modify path", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("click_modify_enters_editing_mode", () => {
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={samplePayload} onDismiss={onDismiss} />);

    // 点击 "调整(Modify)" 按钮
    fireEvent.click(screen.getByRole("button", { name: /调整/ }));
    // 应该看到 "提交修改" / "取消" / "+ 添加节点" 按钮
    expect(screen.getByRole("button", { name: /提交修改/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /取消/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /添加节点/ })).toBeInTheDocument();
  });

  it("add_node_increases_editedNodes_length", () => {
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={samplePayload} onDismiss={onDismiss} />);

    fireEvent.click(screen.getByRole("button", { name: /调整/ }));
    fireEvent.click(screen.getByRole("button", { name: /添加节点/ }));

    // 应该有 2 个 NodeEditor(原 n1 + 新增)
    const editors = screen.getAllByRole("group", { name: /节点.*编辑器/ });
    expect(editors.length).toBe(2);
  });

  it("delete_node_decreases_editedNodes_length", () => {
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={samplePayload} onDismiss={onDismiss} />);

    fireEvent.click(screen.getByRole("button", { name: /调整/ }));
    fireEvent.click(screen.getByRole("button", { name: /删除节点 n1/ }));

    // 应该 0 个 NodeEditor
    const editors = screen.queryAllByRole("group", { name: /节点.*编辑器/ });
    expect(editors.length).toBe(0);
  });

  it("submit_modified_plan_calls_approveDagSkeleton_with_modify", async () => {
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={samplePayload} onDismiss={onDismiss} />);

    // 进入编辑模式
    fireEvent.click(screen.getByRole("button", { name: /调整/ }));
    // 修改 risk_ceiling
    fireEvent.change(screen.getByDisplayValue("E1"), { target: { value: "E2" } });
    // 提交修改
    fireEvent.click(screen.getByRole("button", { name: /提交修改/ }));

    await waitFor(() => {
      expect(approveDagSkeleton).toHaveBeenCalledWith(
        "dag_test_123",
        "modify",
        expect.objectContaining({
          plan_id: "w9p4-test",
          nodes: expect.arrayContaining([
            expect.objectContaining({ risk_ceiling: "E2" }),
          ]),
        })
      );
    });
    expect(onDismiss).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: 跑 vitest 验证通过**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run test -- --run DagApprovalDialog.modify
```
Expected: 4 个测试全 PASS。

- [ ] **Step 3: 跑全部 vitest 验证不回归**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run test -- --run
```
Expected: 既有 + 新增(NodeEditor 4 个 + DagApprovalDialog.modify 4 个)全 PASS。

- [ ] **Step 4: Commit**

```powershell
cd voicepilot; git add crates/ui/web/src/components/__tests__/DagApprovalDialog.modify.test.tsx; git commit -m "test(w9p4): add DagApprovalDialog Modify path vitest integration tests"
```

---

## Task 11: cargo check + clippy + npm build + commit

**Files:**
- 不改源码(仅跑验收命令)
- Modify: `docs/PROGRESS.md`(W9 Plan 4 完成状态)

**目标:** 跑完整验收门禁:6 套 feature 组合 `cargo check` + 2 套代表性 `clippy -D warnings` + `npm.cmd run build` + 测试统计。更新 PROGRESS.md。

- [ ] **Step 1: 6 套 feature 组合 cargo check**

Run:
```powershell
cd voicepilot; cargo check --workspace --no-default-features; cargo check --workspace --features llm; cargo check --workspace --features voice,tauri; cargo check --workspace --features voice,tauri,llm; cargo check --workspace --features voice,tauri,llm,uia; cargo check --workspace --features voice,tauri,llm,stronghold
```
Expected: 全部 PASS。若 `stronghold` feature 组合失败,确认 Plan 1 是否已完成(本 Plan 不依赖 stronghold,但 spec §10 要求 7 套组合全过)。

注意:本 Plan 4 不引入 `stronghold` feature 依赖,故 `stronghold` feature 组合的 cargo check 不应因 Plan 4 改动而失败;若失败,检查 Plan 1 状态。

- [ ] **Step 2: 2 套 clippy `-D warnings`**

Run:
```powershell
cd voicepilot; cargo clippy --workspace --no-default-features -- -D warnings; cargo clippy --workspace --features voice,tauri,llm -- -D warnings
```
Expected: 0 警告。若有警告,按提示修复(常见:`unused_imports` / `explicit_auto_deref` / `redundant_clone`)。

- [ ] **Step 3: npm build**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run build
```
Expected: PASS(`vite build` 成功,无 TypeScript 错误)。

- [ ] **Step 4: 非门控测试数统计**

Run:
```powershell
cd voicepilot; cargo test --workspace --no-default-features -- --list 2>&1 | Select-String "test$" | Measure-Object | Select-Object -ExpandProperty Count
```
Expected: ≥ 286(W8 收尾 465 + W9 Plan 4 新增约 10 = 475+,远超阈值)。

- [ ] **Step 5: 跑全部 w9_dag_modify_smoke + vitest 验证最终状态**

Run:
```powershell
cd voicepilot; cargo test --features llm -p trust-kernel --test w9_dag_modify_smoke; cd crates\ui\web; npm.cmd run test -- --run
```
Expected: 全 PASS。

- [ ] **Step 6: 更新 PROGRESS.md**

打开 `docs/PROGRESS.md`,在 W9 段落(若不存在则在 W8 段落之后追加 W9 段落)加 Plan 4 完成状态:

```markdown
## W9 Plan 4:DAG Modify 分支实现(后端 + UI + 重新审批)

**状态:** 已完成(commit `<待 git log 获取>`)
**测试统计:**
- 集成测试:`w9_dag_modify_smoke.rs` 6 个全 PASS
- 前端测试:`NodeEditor.test.tsx` 4 个 + `DagApprovalDialog.modify.test.tsx` 4 个全 PASS
- 既有 `approver_unit.rs` / `w8_dag_commands_unit.rs` 适配新签名,无回归
**验收门禁:**
- 6 套 feature `cargo check` PASS
- 2 套 `clippy -D warnings` 0 警告
- `npm.cmd run build` PASS
- 非门控测试数 ≥ 286(实际 ~475+)
**关键改动:**
- `Approver::approve_dag_skeleton` 签名 `Result<ApprovalDecision>` → `Result<DagApprovalOutcome>`
- 新增 `DagApprovalOutcome::Modify { modified_plan: Box<DagPlan> }` 变体
- 新增 `KernelError::DagModifyLimitExceeded`
- `DagExecutor::run` 处理 Modify 分支(审计 + 重新校验 + `run_modified`)
- `submit_dag_skeleton_approval` 扩展 `modified_plan: Option<DagPlan>` 参数
- `TauriApprover` 用 `DagApprovalPayload` + `dag_senders` 平行 oneshot channel
- 前端 `DagApprovalDialog` 激活 Modify 按钮 + `editingMode` state
- 新增 `NodeEditor` 组件(增删节点 + input_template textarea + risk_ceiling select)
- 审计事件:`dag_skeleton_modified` / `dag_modify_limit_exceeded`(spec §6.4)
```

- [ ] **Step 7: Commit**

```powershell
cd voicepilot; git add docs/PROGRESS.md; git commit -m "docs(w9p4): update PROGRESS.md with Plan 4 completion status"
```

- [ ] **Step 8: 验证 git log 干净**

Run:
```powershell
cd voicepilot; git log --oneline -10
```
Expected: 看到 6 个 W9 Plan 4 commit(trait 扩展 / Modify 分支 / 测试 / UI 后端 / UI 前端 / PROGRESS.md)。

---

## Self-Review

**1. Spec 覆盖(spec §2.4 + §6.3 + §10 + §11):**

- §2.4 后端:`Approver` trait 签名扩展 ✓(Task 1)+ `DagApprovalOutcome` 枚举 ✓(Task 1)+ `dag_executor.rs` 处理 Modify 分支 ✓(Task 2)+ `run_modified` ✓(Task 2)+ `DagModifyLimitExceeded` ✓(Task 2 Step 1)
- §2.4 UI 后端:`submit_dag_skeleton_approval` 接收 `modified_plan: Option<DagPlan>` ✓(Task 5 Step 6)+ `TauriApprover` emit `dag-approval-request` 携带完整 plan ✓(Task 5 Step 3 复用既有 `DagApprovalRequestPayload`)
- §2.4 UI 前端:激活 Modify 按钮 ✓(Task 6 Step 3)+ `NodeEditor` 组件(增删节点 + input_template textarea + risk_ceiling select)✓(Task 7)
- §2.4 审计事件:`dag_skeleton_modified` ✓(Task 2 Step 4)+ `dag_modify_limit_exceeded` ✓(Task 2 Step 5)
- §6.3 安全约束:
  - modified_plan 通过 `SlotTemplateEngine::validate_dag` 重新校验 ✓(Task 2 Step 4 + Task 3)
  - Modify 只允许一次 ✓(Task 2 Step 5 `run_modified` 第二次 Modify 返回 `DagModifyLimitExceeded`)
  - risk_ceiling 不能超过原 plan max risk ✓(Task 2 Step 7 `check_risk_ceiling_no_escalation` + Task 3 Step 6 测试)
  - approval_request_id 单次使用 ✓(Task 5 Step 2 `take_dag_sender` 移除 sender)
- §10 Conventions:PowerShell `;` 分隔 ✓ / TDD ✓ / 审计事件 lower_snake_case ✓ / TauriApprover constructors `new` vs `with_app` ✓(Task 5 Step 3-5 保留)/ oneshot + 5min timeout + 默认 Deny ✓(Task 5 Step 2 `wait_for_dag_decision`)/ commit message `feat(w9p4): ...` ✓
- §11 兼容性:`DagApprovalDecision::Modify` 占位 → W9 Plan 4 激活 ✓ / Allow/Deny 路径不变 ✓(Task 1 Step 8 验证 W8 测试不回归)/ `DagExecutor::run` 签名未改(本 Plan 不动 user_slots,W9 Plan 6 改)✓

**2. Placeholder 扫描:**

- 无 "TBD" / "TODO" / "implement later" / "fill in details"
- 所有代码步骤含完整代码块
- 所有 PowerShell 命令含 expected output 描述
- Task 6 Step 4 `NodeEditor` import 在 Task 7 创建组件后才能跑通,已在 Step 6 注释说明

**3. 类型一致性:**

- `DagApprovalOutcome` 在 Task 1 定义,Task 2 / Task 5 使用,签名一致(`Allow` / `Deny` / `Modify { modified_plan: Box<DagPlan> }`)
- `DagApprovalPayload` 在 Task 5 Step 1 定义,Task 5 Step 3 + Task 5 Step 8 测试使用,字段一致(`decision: ApprovalDecision` + `modified_plan: Option<DagPlan>`)
- `KernelError::DagModifyLimitExceeded { plan_id: String }` 在 Task 2 Step 1 定义,Task 2 Step 5 使用,Task 9 Step 1 测试断言一致
- `DagPlanFull`(前端)在 Task 8 Step 1 定义,Task 6 Step 2 `handleModifySubmit` + Task 8 Step 4 `approveDagSkeleton` 使用,字段一致
- `ApprovalRegistry::create_dag_request` / `take_dag_sender` / `wait_for_dag_decision` 在 Task 5 Step 2 定义,Task 5 Step 3 + Task 5 Step 8 测试使用,签名一致
- `check_risk_ceiling_no_escalation(original, modified)` 在 Task 2 Step 7 定义,Task 2 Step 4 调用,参数顺序一致
- `run_modified(modified_plan, root_task_id)` 在 Task 2 Step 5 定义,Task 2 Step 4 调用,参数一致
- `approve_dag_skeleton_outcome(plan) -> KernelResult<DagApprovalOutcome>` 在 Task 5 Step 3 定义,Task 5 Step 4 trait impl 调用,签名一致

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-28-w9-plan4-dag-modify.md`. Two execution options:

**1. Subagent-Driven (recommended)** - 每个 Task 派发独立 subagent,Task 之间 review,迭代快

**2. Inline Execution** - 在当前会话批量执行,checkpoint review

**commit message 格式(spec §10):**

- `feat(w9p4): ...` — 新功能(如 `feat(w9p4): extend Approver trait with DagApprovalOutcome for Modify payload`)
- `test(w9p4): ...` — 测试(如 `test(w9p4): complete 6 dag_modify_smoke integration tests`)
- `fix(w9p4): ...` — 修复(如 `fix(w9p4): handle ELevel Ord derive for risk ceiling comparison`)
- `refactor(w9p4): ...` — 重构(如 `refactor(w9p4): extract execute_nodes from run for run_modified reuse`)
- `docs(w9p4): ...` — 文档(如 `docs(w9p4): update PROGRESS.md with Plan 4 completion status`)

**End of W9 Plan 4 Implementation Plan**

---

## W9 审查修复记录

本段落记录 W9 Plan 4 审查阶段发现并修复的所有缺陷。修复方式为 Edit 工具精确替换(不重写整个文件),每项修复在对应 Task / Step 加 `**W9 修复(Px-x):**` 注释标记。

### P0 级缺陷(阻塞性,必修复)

- **P0-1: ELevel Ord 缺失导致 check_risk_ceiling_no_escalation 编译失败**
  - 位置:File Structure + Task 2 Step 1 + Task 2 Step 7 + Task 3 Step 3
  - 修复:在 File Structure 段落加 `policy/types.rs` Modify 条目;把 Task 2 Step 1 改为必做项(同时处理 `error.rs` DagModifyLimitExceeded + `policy/types.rs` ELevel Ord derive);Task 2 Step 7 末尾的 conditional 注意改为"已在 Task 2 Step 1 完成";Task 3 Step 3 标题改为"ELevel Ord derive 已在 Task 2 Step 1 完成",删除 conditional 措辞。

- **P0-2: topological_sort 后移破坏 W8 既有审计顺序 + 失败诊断能力**
  - 位置:Task 2 Step 4
  - 修复:在 Task 2 Step 4 加注释保留 W8 既有对原 plan 的 `topological_sort` 环检测(在 `dag_skeleton_approved` 之前,前置 fail-fast),不要后移到 `effective_plan` 之后;Modify 分支额外对 `modified_plan` 调一次 `topological_sort` 做环检测(仅校验,不消费 order);原 Step 1 位置保留(去掉 `let order =` 避免 unused 警告)。

- **P0-3: run_modified 第二次 Allow 路径绕过 dag_plan_created 审计 + 持久化**
  - 位置:Task 2 Step 5
  - 修复:modified_plan 用新 plan_id(`format!("{}_modified", modified_plan.plan_id)`),单独走完整审计链(`dag_plan_created` + `persist_dag_status(Pending)` + `dag_skeleton_approved(phase=after_modify)` + Allow/Deny/Modify 分支处理)。

- **P0-4: TauriApprover inherent 方法重命名 + trait impl 重构破坏既有测试调用链**
  - 位置:Task 5 Step 8
  - 修复:在 Task 5 Step 8 显式列出所有需更新的测试文件(approver_unit.rs / w8_dag_commands_unit.rs / e2e_dag_smoke.rs / w8_plan2_dag_e2e.rs + grep 命中的其他文件),`create_dag_approval_request_for_test` receiver 类型变更同步更新。

- **P0-5: wait_for_dag_decision 在 #[tokio::test] 上下文触发 "runtime within runtime" panic**
  - 位置:Task 5 Step 2 + §Conventions
  - 修复:`wait_for_dag_decision` 用 `tokio::runtime::Handle::try_current()` 检测当前是否在 runtime 内 — 已在 runtime 用 `handle.block_on`,不在 runtime 新建 current_thread runtime;§Conventions 加"禁止 `#[tokio::test]` 调 `TauriApprover`"规则。

- **P0-6: Task 5 Step 8 测试 approve_dag_skeleton_modify_returns_outcome_with_modified_plan 流程逻辑错误**
  - 位置:Task 5 Step 8 + Task 5 Step 1
  - 修复:测试改为 spawn 线程调 `approve_dag_skeleton_outcome`,主线程通过 `latest_approval_id()` 测试 hook 取 approval_id 后 `take_dag_sender` 投递 payload;Task 5 Step 1 加测试 hook 要求(`latest_approval_id() -> Option<String>` + `registry() -> &ApprovalRegistry`,仅 `#[cfg(test)]`)。

### P1 级缺陷(重要,应修复)

- **P1-7: dag_skeleton_approved 审计 phase 字段非对称**
  - 位置:Task 2 Step 4
  - 修复:第一次审计 `dag_skeleton_approved` details 加 `"phase": "initial"`,与第二次 `run_modified` 内的 `"phase": "after_modify"` 对称。

- **P1-9: dag_skeleton_modified details 隐私断言过于宽松**
  - 位置:Task 4 Step 5
  - 修复:测试断言改为解析 JSON 后检查 `details.get("input_template").is_none()` + `details.get("nodes").is_none()` + `details.get("modified_node_count").is_some()`,避免字符串 contains 误判。

- **P1-10: parsePlanJson 类型签名 DagPlanFull | unknown 等价 unknown**
  - 位置:Task 8 Step 3
  - 修复:签名保持 `unknown`(因 `DagPlanFull | unknown` 等价 unknown),内部用 type guard 校验 `nodes` 字段存在,throw `Error("Invalid plan_json: missing nodes")` 而非静默返回空数组。

- **P1-11: NodeEditor textarea 无 JSON 校验**
  - 位置:Task 7 Step 3
  - 修复:NodeEditor 加 `invalid` state + `onValidityChange?` prop,textarea onChange 内 `try { JSON.parse(value); setInvalid(false) } catch { setInvalid(true) }`,invalid 时红色边框(`.dag-template-invalid`)+ "Invalid JSON" 提示(`.dag-template-error`);DagApprovalDialog 跟踪 `invalidNodeIds: Set<string>`,"提交修改" 按钮 `disabled={submitting || invalidNodeIds.size > 0}`。

- **P1-12: approveDagSkeleton Tauri IPC camelCase 转换未确认**
  - 位置:Task 5 Step 7
  - 修复:在 Task 5 Step 7 加注释核实 `#[tauri::command(rename_all = "camelCase")]` 或确认 W8 既有 `approveDagSkeleton` 调用已正常工作 — 若 W8 工作则 camelCase 转换已生效,Plan 4 沿用即可。

- **P1-13: DagApprovalDialog 卸载 cleanup 与 handleModifySubmit 的 submittedRef 时序**
  - 位置:Task 6 Step 2
  - 修复:`handleModifySubmit` 的 `submittedRef.current = true` 仅在 `approveDagSkeleton` 成功后设,catch 块不设(保持 false),允许 cleanup 在 unmount 时发 Deny。

- **P1-14: DagApprovalPayload serde Deserialize 无实际用途**
  - 位置:Task 5 Step 1
  - 修复:移除 `serde::Deserialize`,仅保留 `Serialize` + `Clone` + `Debug`(从 UI 后端投递到 TauriApprover,单向序列化)。

- **P1-15: ApprovalRegistry::create_dag_request 的 plan: &DagPlan 参数未使用**
  - 位置:Task 5 Step 2
  - 修复:删除 plan 参数,签名改为 `create_dag_request(&self) -> (String, oneshot::Receiver<DagApprovalPayload>)`;调用方 `approve_dag_skeleton_outcome` 直接 `self.registry.create_dag_request()`;`create_dag_approval_request_for_test` 同步删 plan 参数。

- **P1-16: DagApprovalDecision 前端类型与后端 serde 序列化不一致风险**
  - 位置:Task 5 Step 6
  - 修复:在 Task 5 Step 6 加注释核实 `DagApprovalDecision` 的 serde 属性(grep `#[serde(...)]`),确认是 `rename_all = "lowercase"`(前端 `types.ts` 用 `"allow" | "deny" | "modify"`)。若不是,前端类型对应调整。

### P2 级缺陷(改进性,可选修复)

- **P2-17: Plan 4 Precondition 未列 W9 Plan 1 完成依赖**
  - 位置:§Precondition
  - 修复:§Precondition 加"W9 Plan 1 已完成(stronghold feature 存在 + StrongholdVault API 可用)— 仅在 Task 11 Step 1 跑 `--features stronghold` 时需要"。

- **P2-19: execute_nodes 抽取后 run 方法行号引用全部失效**
  - 位置:Task 2(目标段落之后)
  - 修复:在 Task 2 目字段后加注释"行号基于 W8 commit `8ec814d`,Task 2 Step 6 重构后需重新定位(用 `grep -n` 查找当前行号)"。

- **P2-20: Task 9 Step 1 SQL details LIKE 脆弱**
  - 位置:Task 9 Step 1
  - 修复:SQL 改为 `json_extract(details, '$.final_status') = 'cancelled'`,替代 `details LIKE '%"final_status":"cancelled"%'`,避免 JSON 字段顺序/转义导致 LIKE 失败。

### 修复统计

- P0 级:6 项(全部修复)
- P1 级:10 项(全部修复)
- P2 级:3 项(全部修复)
- 总计:19 项缺陷全部修复

### 修复影响范围

- **后端 Trust Kernel:** `policy/types.rs`(ELevel Ord)、`error.rs`(DagModifyLimitExceeded)、`skills/dag_executor.rs`(Modify 分支 + run_modified + check_risk_ceiling_no_escalation + execute_nodes 抽取 + topological_sort 环检测)
- **后端 UI:** `approver.rs`(DagApprovalPayload + ApprovalRegistry + TauriApprover + wait_for_dag_decision runtime 检测 + 测试 hook)、`dag_commands.rs`(submit_dag_skeleton_approval + approve_dag_skeleton_command)
- **前端:** `DagApprovalDialog.tsx`(editingMode + handleModifySubmit submittedRef 时序 + parsePlanJson type guard)、`NodeEditor.tsx`(JSON 校验 + onValidityChange)、`api.ts` / `types.ts`(DagPlanFull + approveDagSkeleton modifiedPlan)
- **测试:** `w9_dag_modify_smoke.rs`(SQL json_extract + 隐私断言 JSON 解析)、`approver_unit.rs`(spawn 线程测试 + latest_approval_id hook)、`w8_dag_commands_unit.rs` / `e2e_dag_smoke.rs` / `w8_plan2_dag_e2e.rs`(适配新签名)
- **文档:** §Precondition(W9 Plan 1 依赖)、§Conventions(禁止 #[tokio::test] 调 TauriApprover)、File Structure(policy/types.rs 条目)
