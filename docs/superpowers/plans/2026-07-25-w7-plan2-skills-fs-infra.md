# W7 Plan 2: Skills 基础设施 + 3 个纯 FS Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W7 设计文档 §2.4 实现 `skills/common.rs` 共享 helper 与 3 个纯文件系统 Skill(`task.repeat_verified` / `task.explain` / `task.compensate`),并在 `manifest.rs` 中提供 7 个新 manifest 函数,注册到 `SkillRouter`。UIA / Playwright MCP 相关 Skill 在 Plan 4/5 实现。

**Architecture:** `skills/common.rs` 抽取 W3b `executor.rs` 的 prepare→approve→commit 核心循环为 `run_prepare_approve_commit` helper,供 8 个 Skill 复用;3 个新 Skill 各占一个文件,通过 `RouteDecision::Skill` 路径触发;manifest 函数在 `manifest.rs` 中以 `pub fn xxx_manifest() -> SkillManifest` 形式提供。

**Tech Stack:** Rust(stable),`trust_kernel::skills::executor::Executor`(W3b 已有),`trust_kernel::policy::transaction`(W2 已有),`trust_kernel::audit`(W1 已有)。

**Spec:** `docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md` §2.4

**Precondition:** Plan 1 已完成(commit `3820d04`)。

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Create** `src/skills/common.rs` — `run_prepare_approve_commit` / `validate_input_against_manifest`
- **Modify** `src/skills/mod.rs` — `pub mod common; pub mod task_repeat; pub mod task_explain; pub mod task_compensate;`
- **Create** `src/skills/task_repeat.rs` — `task.repeat_verified` executor
- **Create** `src/skills/task_explain.rs` — `task.explain` executor
- **Create** `src/skills/task_compensate.rs` — `task.compensate` executor
- **Modify** `src/skills/manifest.rs` — 加 7 个 manifest 函数(`task_repeat_verified_manifest` / `task_explain_manifest` / `task_compensate_manifest` / `app_control_manifest` / `note_capture_manifest` / `research_save_manifest` / `form_prepare_manifest`)
- **Modify** `src/skills/router.rs` — `#[cfg(test)] mod tests` 加 3 个新 Skill 路由测试
- **Modify** `src/voice/router_bridge.rs` — 注册 3 个新 Skill 到 SkillRouter

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w7_plan2_skills_smoke.rs` — 3 个新 Skill 端到端冒烟测试

### Docs

- **Modify** `docs/PROGRESS.md` — Plan 2 完成状态

---

## Conventions

- **TDD**:每个 Skill executor 先写失败测试 → 实现 → 测试通过 → commit
- **prepare→approve→commit**:所有写操作 Skill 必须走 `run_prepare_approve_commit`,不得绕过
- **approval mode**:`task.repeat_verified` / `task.compensate` 用 `PerStep`;`task.explain` 用 `None`(只读)
- **risk_ceiling**:`task.repeat_verified` / `task.compensate` 为 E2;`task.explain` 为 E0
- **Commit message**:`feat(w7p2): ...` / `test(w7p2): ...` / `docs(w7p2): ...`

---

## Task 1: skills/common.rs 共享 helper

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/common.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1:** 创建 `common.rs`,实现 `run_prepare_approve_commit(kernel, input, approver, manifest) -> KernelResult<TaskId>` 抽取 W3b `executor.rs` 的核心循环
- [ ] **Step 2:** 实现 `validate_input_against_manifest(input, manifest) -> Result<()>` 校验 input 满足 `inputs` 约束
- [ ] **Step 3:** 在 `mod.rs` 加 `pub mod common;`
- [ ] **Step 4:** 单元测试:mock kernel + approver 验证 helper 流程
- [ ] **Step 5:** `cargo test -p trust-kernel --lib skills::common::`
- [ ] **Step 6:** commit `feat(w7p2): add skills/common.rs with run_prepare_approve_commit helper`

## Task 2: 7 个 manifest 函数

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`

- [ ] **Step 1:** 加 `task_repeat_verified_manifest()` — risk E1 / approval PerStep / tools `filesystem.search_files` + `filesystem.verify_move`
- [ ] **Step 2:** 加 `task_explain_manifest()` — risk E0 / approval None / tools `audit.read`
- [ ] **Step 3:** 加 `task_compensate_manifest()` — risk E2 / approval PerStep / tools `compensation.auto_reverse`
- [ ] **Step 4:** 加 `app_control_manifest()`(stub,UIA 实现待 Plan 4)— risk E2 / approval PerStep / tools `uiautomation.*`
- [ ] **Step 5:** 加 `note_capture_manifest()`(stub,UIA 实现待 Plan 4)— risk E1 / approval PerStep / tools `uiautomation.*` + `filesystem.write`
- [ ] **Step 6:** 加 `research_save_manifest()`(stub,Playwright 实现待 Plan 5)— risk E2 / approval PerStep / tools `mcp.playwright.*` + `filesystem.write`
- [ ] **Step 7:** 加 `form_prepare_manifest()`(stub,Playwright 实现待 Plan 5)— risk E2 / approval PerStep / tools `mcp.playwright.*`
- [ ] **Step 8:** `cargo check -p trust-kernel`
- [ ] **Step 9:** commit `feat(w7p2): add 7 new skill manifest functions (3 fs + 4 stubs for Plan 4/5)`

## Task 3: task.repeat_verified executor

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/task_repeat.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1:** 先写失败测试:验证 `task.repeat_verified` 用 `filesystem.search_files` 找到上一次 prepare 的源文件,然后 `filesystem.verify_move` 校验目标存在
- [ ] **Step 2:** 实现 `pub fn execute_repeat_verified(kernel, input, approver) -> KernelResult<TaskId>`,调 `run_prepare_approve_commit`
- [ ] **Step 3:** 在 `mod.rs` 加 `pub mod task_repeat;`
- [ ] **Step 4:** `cargo test -p trust-kernel --lib skills::task_repeat::`
- [ ] **Step 5:** commit `feat(w7p2): implement task.repeat_verified executor`

## Task 4: task.explain executor

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/task_explain.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1:** 先写失败测试:验证 `task.explain` 读取 `audit_logs` 表 + 返回最近 N 条记录
- [ ] **Step 2:** 实现 `pub fn execute_explain(kernel, input, _approver) -> KernelResult<ExplainResult>`,approval=None 直接读 audit 不走 commit
- [ ] **Step 3:** 在 `mod.rs` 加 `pub mod task_explain;`
- [ ] **Step 4:** `cargo test -p trust-kernel --lib skills::task_explain::`
- [ ] **Step 5:** commit `feat(w7p2): implement task.explain executor (read-only audit log)`

## Task 5: task.compensate executor

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/task_compensate.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1:** 先写失败测试:验证 `task.compensate` 调 `compensation.auto_reverse` 反向执行指定 step
- [ ] **Step 2:** 实现 `pub fn execute_compensate(kernel, input, approver) -> KernelResult<TaskId>`,PerStep approval
- [ ] **Step 3:** 在 `mod.rs` 加 `pub mod task_compensate;`
- [ ] **Step 4:** `cargo test -p trust-kernel --lib skills::task_compensate::`
- [ ] **Step 5:** commit `feat(w7p2): implement task.compensate executor (auto_reverse)`

## Task 6: 注册 3 个新 Skill 到 SkillRouter

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`
- Modify: `voicepilot/crates/ui/src/commands.rs`(route_text 也注册)

- [ ] **Step 1:** 在 `router_bridge.rs` 的 `route_text` 中 `router.register(task_repeat_verified_manifest())` 等 3 行
- [ ] **Step 2:** 在 `commands.rs` 的 `route_text` 中同样注册(plan 1 已留 TODO 注释,取消注释 3 行)
- [ ] **Step 3:** `cargo test -p trust-kernel --lib skills::router::`
- [ ] **Step 4:** commit `feat(w7p2): register 3 new fs skills in SkillRouter`

## Task 7: 端到端冒烟测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w7_plan2_skills_smoke.rs`

- [ ] **Step 1:** 测试 `task.repeat_verified`:模拟上一次 files.organize 的 audit 记录,触发 repeat → 验证新 task 创建
- [ ] **Step 2:** 测试 `task.explain`:预填 audit_logs,触发 explain → 验证返回记录
- [ ] **Step 3:** 测试 `task.compensate`:预填 compensation 记录,触发 compensate → 验证 auto_reverse
- [ ] **Step 4:** `cargo test -p trust-kernel --test w7_plan2_skills_smoke`
- [ ] **Step 5:** commit `test(w7p2): add e2e smoke tests for 3 new fs skills`

## Task 8: 验收 + PROGRESS.md 更新

- [ ] **Step 1:** `cargo check --workspace --features voice,tauri,llm`
- [ ] **Step 2:** `cargo test --workspace --features voice,tauri,llm`
- [ ] **Step 3:** 更新 `docs/PROGRESS.md` Plan 2 完成状态
- [ ] **Step 4:** commit `docs(w7p2): update PROGRESS.md with Plan 2 completion`

---

## Acceptance Gates

- 3 个新 Skill 各至少 1 个单元测试通过
- 端到端冒烟测试 3 个通过
- 全 workspace cargo check + test 无回归
- `run_prepare_approve_commit` 复用率 100%(3 个 Skill 都调,无重复 prepare→approve→commit 代码)
