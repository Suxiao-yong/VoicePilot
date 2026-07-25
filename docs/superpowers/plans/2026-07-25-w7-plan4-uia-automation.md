# W7 Plan 4: Windows UIA 自动化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 按 W7 设计文档 §2.6 实现 Windows UIA 自动化适配器(`uiautomation-rs` crate)+ 2 个 UIA Skill(`quick.app_control` / `note.capture`),并通过 `uia` cargo feature gate 控制(默认关闭,Windows-only)。

**Architecture:** `uiautomation/mod.rs` 定义 `UiaAdapter` trait;`uiautomation/adapter.rs` 实现 `WindowsUiaAdapter`(wraps `uiautomation-rs`);2 个 Skill executor 各占一个文件,通过 `RouteDecision::Skill` 触发,调 `UiaAdapter` 方法;`allowed_apps` 白名单约束启动范围,超出时 PerStep approval。

**Tech Stack:** Rust(stable),`uiautomation-rs = "0.16"`(Windows-only,纯 Rust 无 SDK 依赖),`trust_kernel::approval::approver`(W6a 已有)。

**Spec:** `docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md` §2.6

**Precondition:** Plan 2 已完成(`app_control_manifest` / `note_capture_manifest` stub 已存在)。

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Modify** `Cargo.toml` — 加 `uiautomation-rs = { version = "0.16", optional = true }` + 新 `uia` feature(默认关闭)
- **Modify** `src/lib.rs` — `#[cfg(feature = "uia")] pub mod uiautomation;`
- **Create** `src/uiautomation/mod.rs` — `pub mod adapter;` + `UiaAdapter` trait + `UiaElementHandle` 类型 + `UiaSelector` enum
- **Create** `src/uiautomation/adapter.rs` — `WindowsUiaAdapter`(wraps `uiautomation-rs`)
- **Create** `src/skills/app_control.rs` — `quick.app_control` executor
- **Create** `src/skills/note_capture.rs` — `note.capture` executor
- **Modify** `src/skills/mod.rs` — `#[cfg(feature = "uia")] pub mod app_control; pub mod note_capture;`
- **Modify** `src/skills/manifest.rs` — `app_control_manifest` / `note_capture_manifest` 加完整 `inputs` 定义(Plan 2 stub 升级)
- **Modify** `src/voice/router_bridge.rs` — `#[cfg(feature = "uia")]` 注册 2 个 UIA Skill
- **Modify** `src/kernel.rs` — 加 `allowed_apps: Vec<String>` 配置(默认 `["notepad", "explorer", "calc"]`)

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w7_plan4_uia_smoke.rs` — `#[ignore]` 真实 GUI 测试 + 普通 mock 测试
- **Modify** `voicepilot/crates/trust-kernel/src/uiautomation/adapter.rs` — `#[cfg(test)] mod tests`

### Docs

- **Modify** `docs/PROGRESS.md` — Plan 4 完成状态
- **Modify** `docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md` — 标注 UIA 已实现

---

## Conventions

- **Feature gate**:`uia` feature 在 `trust-kernel/Cargo.toml`,默认关闭;`uiautomation-rs` 为 optional 依赖
- **平台门控**:`uiautomation-rs` 仅 Windows 编译;`Cargo.toml` 加 `[target.'cfg(windows)'.dependencies]`
- **allowed_apps 白名单**:默认 `["notepad", "explorer", "calc"]`,Settings 可扩展;超出白名单的 `launch_app` 需要 PerStep approval
- **set_text 边界**:仅对窗口标题在白名单内的元素生效(防伪造窗口)
- **screenshot**:返回 PNG 字节流,写入 audit_logs(供事后审计)
- **Commit message**:`feat(w7p4): ...` / `test(w7p4): ...`

---

## Task 1: uia feature gate + uiautomation-rs 依赖

**Files:**
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`

- [ ] **Step 1:** 加 `[features] uia = ["uiautomation-rs"]`
- [ ] **Step 2:** 加 `[target.'cfg(windows)'.dependencies] uiautomation-rs = { version = "0.16", optional = true }`
- [ ] **Step 3:** `cargo check -p trust-kernel --features uia`(Windows)
- [ ] **Step 4:** `cargo check -p trust-kernel`(无 uia,验证 default 不依赖)
- [ ] **Step 5:** commit `feat(w7p4): add uia feature gate + uiautomation-rs optional dep (Windows-only)`

## Task 2: UiaAdapter trait + WindowsUiaAdapter 实现

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/uiautomation/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/uiautomation/adapter.rs`
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`

- [ ] **Step 1:** `mod.rs` 定义 `UiaAdapter` trait:`launch_app` / `find_window` / `find_element` / `click` / `set_text` / `get_text` / `screenshot`
- [ ] **Step 2:** `mod.rs` 定义 `UiaElementHandle`(opaque wrapper)与 `UiaSelector` enum(ById / ByName / ByRole)
- [ ] **Step 3:** `adapter.rs` 实现 `WindowsUiaAdapter`,内部 wrap `uiautomation::UIAutomation`
- [ ] **Step 4:** `lib.rs` 加 `#[cfg(feature = "uia")] pub mod uiautomation;`
- [ ] **Step 5:** 单元测试:mock `UiaAdapter`(用 trait 对象 + `Box<dyn UiaAdapter>`)验证 selector / handle 流程
- [ ] **Step 6:** `cargo test -p trust-kernel --features uia --lib uiautomation::`
- [ ] **Step 7:** commit `feat(w7p4): implement UiaAdapter trait + WindowsUiaAdapter`

## Task 3: quick.app_control executor

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/app_control.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`

- [ ] **Step 1:** `manifest.rs` 升级 `app_control_manifest`:`inputs` 加 `app_name: { input_type: string, required: true }` + `action: { input_type: enum, allowed_values: [launch, focus, close], default: launch }`
- [ ] **Step 2:** `app_control.rs` 实现 `pub fn execute_app_control(kernel, input, approver, adapter) -> KernelResult<TaskId>`
  - `action=launch` → 校验 `app_name` 在 `allowed_apps` 白名单 → 不在则 PerStep approval → `adapter.launch_app`
  - `action=focus` → `adapter.find_window` + `click`
  - `action=close` → `adapter.find_window` + send close event
- [ ] **Step 3:** `mod.rs` 加 `#[cfg(feature = "uia")] pub mod app_control;`
- [ ] **Step 4:** 单元测试:mock adapter 验证 launch / focus / close 三个 action
- [ ] **Step 5:** `cargo test -p trust-kernel --features uia --lib skills::app_control::`
- [ ] **Step 6:** commit `feat(w7p4): implement quick.app_control executor (launch/focus/close)`

## Task 4: note.capture executor

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/note_capture.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`

- [ ] **Step 1:** `manifest.rs` 升级 `note_capture_manifest`:`inputs` 加 `content: { input_type: string, required: true }` + `save_path: { input_type: path, required: true, allowed_roots: ["%USERPROFILE%"] }`
- [ ] **Step 2:** `note_capture.rs` 实现 `execute_note_capture(kernel, input, approver, adapter) -> KernelResult<TaskId>`
  - `adapter.launch_app("notepad")` → 等待窗口
  - `adapter.set_text(element, content)`
  - 调 `filesystem.write` 保存到 `save_path`
  - PerStep approval 在 launch_app 后触发
- [ ] **Step 3:** `mod.rs` 加 `#[cfg(feature = "uia")] pub mod note_capture;`
- [ ] **Step 4:** 单元测试:mock adapter 验证 set_text + filesystem.write 调用链
- [ ] **Step 5:** `cargo test -p trust-kernel --features uia --lib skills::note_capture::`
- [ ] **Step 6:** commit `feat(w7p4): implement note.capture executor (notepad + set_text + save)`

## Task 5: allowed_apps 白名单 + Settings 配置

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`
- Modify: `voicepilot/crates/ui/src/settings_commands.rs`
- Modify: `voicepilot/crates/ui/web/src/types.ts`
- Modify: `voicepilot/crates/ui/web/src/components/SettingsView.tsx`

- [ ] **Step 1:** `kernel.rs` `TrustKernel` 加 `allowed_apps: Arc<Mutex<Vec<String>>>`,默认 `["notepad", "explorer", "calc"]`
- [ ] **Step 2:** `settings_commands.rs` `SettingsDto` 加 `uia_allowed_apps: Vec<String>` + KV 映射 `uia.allowed_apps`(JSON 数组序列化)
- [ ] **Step 3:** `types.ts` `Settings` 加 `uia_allowed_apps: string[]`
- [ ] **Step 4:** `SettingsView.tsx` 加"UIA 白名单"输入区(逗号分隔)+ `#[cfg(feature = "uia")]` 标识
- [ ] **Step 5:** `cargo check -p voicepilot-ui --features tauri,uia`
- [ ] **Step 6:** commit `feat(w7p4): add allowed_apps whitelist + Settings UI for UIA`

## Task 6: 注册 UIA Skill 到 SkillRouter

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`
- Modify: `voicepilot/crates/ui/src/commands.rs`

- [ ] **Step 1:** `router_bridge.rs` `#[cfg(feature = "uia")] { router.register(app_control_manifest()); router.register(note_capture_manifest()); }`
- [ ] **Step 2:** `commands.rs` `route_text` 同样注册
- [ ] **Step 3:** `cargo test -p trust-kernel --features uia --lib skills::router::`
- [ ] **Step 4:** commit `feat(w7p4): register 2 UIA skills in SkillRouter (cfg-gated)`

## Task 7: 端到端冒烟测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w7_plan4_uia_smoke.rs`

- [ ] **Step 1:** 测试(mock):`quick.app_control` launch notepad → mock adapter 验证 launch_app 调用
- [ ] **Step 2:** 测试(mock):`note.capture` 写入"hello" → mock adapter 验证 set_text + filesystem.write
- [ ] **Step 3:** `#[ignore]` 真实测试:启动 notepad + set_text + 关闭(需 GUI,CI 跳过)
- [ ] **Step 4:** `cargo test -p trust-kernel --features uia --test w7_plan4_uia_smoke`
- [ ] **Step 5:** commit `test(w7p4): add e2e smoke tests (mock + #[ignore] real GUI)`

## Task 8: 验收 + PROGRESS.md

- [ ] **Step 1:** `cargo check --workspace --features voice,tauri,llm,uia`
- [ ] **Step 2:** `cargo test --workspace --features voice,tauri,llm,uia`
- [ ] **Step 3:** `cargo check --workspace`(验证 default 不依赖 uia)
- [ ] **Step 4:** 更新 `docs/PROGRESS.md` Plan 4 完成状态
- [ ] **Step 5:** commit `docs(w7p4): update PROGRESS.md with Plan 4 completion`

---

## Acceptance Gates

- `uia` feature 默认关闭,默认构建无 `uiautomation-rs` 依赖
- 2 个 UIA Skill 各至少 1 个 mock 单元测试通过
- `allowed_apps` 白名单约束生效(超出时 PerStep approval)
- 真实 GUI 测试标 `#[ignore]`,CI 不跑但本地可手动验证
- 全 workspace cargo check + test 无回归(含 uia 与不含 uia 两套)
