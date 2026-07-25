# W7 Plan 6: 集成测试 + 验收门禁 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 按 W7 设计文档 §6 + §7 完成 W7 全部集成测试与验收门禁:补 4 个端到端冒烟测试文件,跑全 feature 组合 cargo check + clippy + test,更新 `docs/PROGRESS.md` W7 完成状态。

**Architecture:** 4 个 E2E 测试文件分别覆盖 LLM 路由链 / 用户自定义 Skill / Playwright MCP / Settings LLM UI;全 feature 组合(`--no-default-features` / `--features voice` / `--features tauri` / `--features voice,tauri` / `--features voice,tauri,llm` / `--features voice,tauri,llm,uia`)各跑一遍;clippy `-D warnings` 0 警告。

**Tech Stack:** Rust(stable),`cargo` + `cargo clippy`,`wiremock`(LLM mock,已 dev-dependency),Tauri 2 测试框架。

**Spec:** `docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md` §6 + §7

**Precondition:** Plan 2/3/4/5 已完成。

---

## File Structure

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w7_router_llm_smoke.rs` — LLM 路由链 E2E
- **Create** `voicepilot/crates/ui/tests/w7_settings_llm_smoke.rs` — Settings LLM UI E2E
- **Modify** `voicepilot/crates/trust-kernel/tests/w7_user_skill_smoke.rs`(Plan 3 已建,本 Plan 补充断言)
- **Modify** `voicepilot/crates/trust-kernel/tests/w7_mcp_playwright_smoke.rs`(Plan 5 已建,本 Plan 补充断言)

### Docs

- **Modify** `docs/PROGRESS.md` — W7 完成状态 + 测试统计 + 已知偏离

---

## Conventions

- **Feature 组合测试矩阵**:6 套 feature 组合各跑 `cargo check` + `cargo test`
- **clippy**:`cargo clippy --workspace --no-default-features -- -D warnings` 0 警告
- **测试统计**:报告总测试数(目标 ≥ 256 default / +58 tauri / +78 voice)
- **commit**:`test(w7p6): ...` / `docs(w7p6): ...`

---

## Task 1: w7_router_llm_smoke.rs LLM 路由链 E2E

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w7_router_llm_smoke.rs`

- [ ] **Step 1:** 测试:keyword 命中(files.organize)→ 不调 LLM → 返回 `Skill`
- [ ] **Step 2:** 测试:keyword 未命中 + LLM 高 confidence → 返回 `SkillWithSlots`
- [ ] **Step 3:** 测试:keyword 未命中 + LLM 低 confidence → 返回 `Planner`
- [ ] **Step 4:** 测试:LLM disabled → 全走 keyword 路径
- [ ] **Step 5:** 测试:`privacy_mode=true` → LLM 不被调用(用 mock HTTP server 计数)
- [ ] **Step 6:** `cargo test -p trust-kernel --features llm --test w7_router_llm_smoke`
- [ ] **Step 7:** commit `test(w7p6): add w7_router_llm_smoke E2E (5 LLM routing scenarios)`

## Task 2: w7_settings_llm_smoke.rs Settings LLM UI E2E

**Files:**
- Create: `voicepilot/crates/ui/tests/w7_settings_llm_smoke.rs`

- [ ] **Step 1:** 测试:SettingsDto LLM 5 字段 flatten → KV → merge 往返一致
- [ ] **Step 2:** 测试:`update_settings_command` 持久化后 `state.llm_client()` 反映新配置(用 mock `LlmClient::is_enabled()`)
- [ ] **Step 3:** 测试:`privacy_mode=true` 时 `rebuild_llm_client` 返回 `disabled`(is_enabled=false)
- [ ] **Step 4:** 测试:LLM 配置 UI 渲染(可选用 Tauri 测试框架,或仅验证 SettingsDto 序列化)
- [ ] **Step 5:** `cargo test -p voicepilot-ui --features tauri,llm --test w7_settings_llm_smoke`
- [ ] **Step 6:** commit `test(w7p6): add w7_settings_llm_smoke E2E (4 LLM settings scenarios)`

## Task 3: 补充 w7_user_skill_smoke + w7_mcp_playwright_smoke 断言

**Files:**
- Modify: `voicepilot/crates/trust-kernel/tests/w7_user_skill_smoke.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/w7_mcp_playwright_smoke.rs`

- [ ] **Step 1:** `w7_user_skill_smoke.rs` 补:用户自定义 Skill 在 `route_with_llm` 中作为候选发给 LLM
- [ ] **Step 2:** `w7_user_skill_smoke.rs` 补:用户自定义覆盖 built-in 后,LLM 返回的 `matched_skill_id` 命中用户版本
- [ ] **Step 3:** `w7_mcp_playwright_smoke.rs` 补:`form.prepare` 调用链不含 `playwright.click`(submit 不点击)
- [ ] **Step 4:** `w7_mcp_playwright_smoke.rs` 补:MCP 不可用时 Skill 返回 `error_code = "mcp_playwright_unavailable"`
- [ ] **Step 5:** `cargo test -p trust-kernel --features llm --test w7_user_skill_smoke`
- [ ] **Step 6:** `cargo test -p trust-kernel --test w7_mcp_playwright_smoke`
- [ ] **Step 7:** commit `test(w7p6): strengthen w7_user_skill + w7_mcp_playwright assertions`

## Task 4: 全 feature 组合 cargo check 矩阵

- [ ] **Step 1:** `cargo check --workspace --no-default-features`
- [ ] **Step 2:** `cargo check --workspace --features voice`
- [ ] **Step 3:** `cargo check --workspace --features tauri`
- [ ] **Step 4:** `cargo check --workspace --features voice,tauri`
- [ ] **Step 5:** `cargo check --workspace --features voice,tauri,llm`
- [ ] **Step 6:** `cargo check --workspace --features voice,tauri,llm,uia`(Windows)
- [ ] **Step 7:** 记录所有组合通过,失败则修复
- [ ] **Step 8:** commit `test(w7p6): verify all 6 feature combos pass cargo check`(如有修复)

## Task 5: clippy + 全 feature test

- [ ] **Step 1:** `cargo clippy --workspace --no-default-features -- -D warnings`
- [ ] **Step 2:** `cargo clippy --workspace --features voice,tauri,llm -- -D warnings`
- [ ] **Step 3:** `cargo clippy --workspace --features voice,tauri,llm,uia -- -D warnings`(Windows)
- [ ] **Step 4:** 修复所有 clippy 警告
- [ ] **Step 5:** `cargo test --workspace --features voice,tauri,llm`
- [ ] **Step 6:** `cargo test --workspace --features voice,tauri,llm,uia`(Windows)
- [ ] **Step 7:** 记录总测试数,对比 §7.2 验收门禁
- [ ] **Step 8:** commit `test(w7p6): clippy clean + full test suite passes`(如有修复)

## Task 6: npm build + 前端验收

- [ ] **Step 1:** `cd voicepilot/crates/ui/web ; npm.cmd run build`
- [ ] **Step 2:** 验证 `dist/` 生成 + 无 TypeScript 错误
- [ ] **Step 3:** 手动验证(记录到 PROGRESS.md):
  - 启用 LLM + API key → 语音"打开记事本写 TODO" → 命中 `note.capture`
  - 关闭 LLM → 同语音 → 命中 keyword `quick.app_control`(Slot 不足提示)
  - 放自定义 `.md` 到 `%APPDATA%\voicepilot\skills\` → 重启 → Skills Manager 可见
  - Playwright MCP 启用 → 语音"把这个网页存为 Markdown" + URL → 执行抓取保存
- [ ] **Step 4:** commit `docs(w7p6): record frontend build + manual verification results`

## Task 7: PROGRESS.md W7 完成状态

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1:** 更新 W7 章节:8 项工作全部完成
- [ ] **Step 2:** 加测试统计:总数 + 按 crate 分布
- [ ] **Step 3:** 加已知偏离(参照 spec §8):本地 LLM / Skill 编排 / macOS UIA / Node 打包等延后项
- [ ] **Step 4:** 加 commit hash 列表(Plan 1-6 各自的 head commit)
- [ ] **Step 5:** commit `docs(w7p6): update PROGRESS.md with W7 completion + test stats + deferrals`

## Task 8: W7 收尾 commit + tag(可选)

- [ ] **Step 1:** `git log --oneline --grep="w7" | head -50` 列出所有 W7 commit
- [ ] **Step 2:** 验证 W7 commit 链完整(Plan 1 8 任务 + Plan 2 8 任务 + Plan 3 8 任务 + Plan 4 8 任务 + Plan 5 8 任务 + Plan 6 8 任务)
- [ ] **Step 3:** (可选)`git tag w7-complete`(用户确认后)
- [ ] **Step 4:** commit `docs(w7p6): W7 complete`(空 commit 标记里程碑,`git commit --allow-empty`)

---

## Acceptance Gates(对应 spec §7)

### 7.1 编译门禁
- 6 套 feature 组合 `cargo check` 全 PASS
- `cargo clippy --workspace --no-default-features -- -D warnings` 0 warnings
- `npm.cmd run build` PASS

### 7.2 测试门禁
- 现有 236 + 48 + 78 测试全 PASS(无回归)
- 新增 W7 测试 ≥ 20 个
- 总测试数 ≥ 256(default) / +58(tauri) / +78 voice

### 7.3 功能门禁
- LLM 启用 → `note.capture` 命中
- LLM 关闭 → `quick.app_control` 命中(keyword,Slot 不足提示)
- 用户自定义 `.md` → Skills Manager 可见
- Playwright MCP 启用 → `research.save_markdown` 执行

### 7.4 安全门禁
- `privacy_mode=true` 时 LLM 不被调用(单元测试覆盖)
- LLM 调用审计日志完整
- UIA `allowed_apps` 白名单约束
- Playwright MCP `allowed_paths` 为空
