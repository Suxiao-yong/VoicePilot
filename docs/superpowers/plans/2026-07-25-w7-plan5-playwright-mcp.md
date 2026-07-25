# W7 Plan 5: Playwright MCP 浏览器自动化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 按 W7 设计文档 §2.7 实现 Playwright MCP 浏览器自动化:在 `mcp_servers` 表插入 `playwright` 默认记录(W4 框架已支持),实现 2 个浏览器 Skill(`research.save_markdown` / `form.prepare`),通过 `mcp.invoke` 调用链驱动 Playwright MCP stdio server。

**Architecture:** `mcp/repo.rs` 加 `insert_default_servers()` 方法,首次启动时若 `mcp_servers` 表为空则插入 `playwright`;2 个 Skill executor 各占一个文件,调 `McpClient::invoke_tool` 触发 `playwright.navigate` / `playwright.snapshot` / `playwright.fill` / `playwright.eval` / `playwright.click`;不点击 submit(用户审批后调)。

**Tech Stack:** Rust(stable),`trust_kernel::mcp::client`(W4 已有),Tauri 2,Node.js + `@playwright/mcp`(用户侧依赖,不打包)。

**Spec:** `docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md` §2.7

**Precondition:** Plan 2 已完成(`research_save_manifest` / `form_prepare_manifest` stub 已存在)。

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Modify** `src/mcp/repo.rs` — 加 `insert_default_servers(conn) -> Result<()>`(空表时插入 `playwright`)
- **Modify** `src/kernel.rs` — `boot()` 末尾调 `McpServerRepo::insert_default_servers`
- **Create** `src/skills/research_save.rs` — `research.save_markdown` executor
- **Create** `src/skills/form_prepare.rs` — `form.prepare` executor
- **Modify** `src/skills/mod.rs` — `pub mod research_save; pub mod form_prepare;`
- **Modify** `src/skills/manifest.rs` — `research_save_manifest` / `form_prepare_manifest` 加完整 `inputs` 定义(Plan 2 stub 升级)
- **Modify** `src/voice/router_bridge.rs` — 注册 2 个浏览器 Skill
- **Modify** `src/skills/common.rs` — 加 `invoke_mcp_tool(kernel, server_id, tool_name, args) -> ToolResult` helper

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w7_plan5_mcp_playwright_smoke.rs` — mock MCP stdio server
- **Modify** `voicepilot/crates/trust-kernel/src/skills/research_save.rs` — `#[cfg(test)] mod tests`
- **Modify** `voicepilot/crates/trust-kernel/src/skills/form_prepare.rs` — `#[cfg(test)] mod tests`

### Frontend(`voicepilot/crates/ui/web/src/`)

- **Modify** `components/SettingsView.tsx` — 加"Playwright MCP"配置提示("需 Node.js ≥ 18,首次使用 `npx -y @playwright/mcp@latest` 会自动下载")
- **Modify** `components/SkillsManager.tsx` — 标识 `research.save_markdown` / `form.prepare` 需 Playwright MCP 启用

### Docs

- **Modify** `docs/PROGRESS.md` — Plan 5 完成状态
- **Create** `docs/playwright-mcp-setup.md` — Node.js 安装 + Playwright MCP 配置指南

---

## Conventions

- **MCP 调用边界**:Skill 通过 `McpClient::invoke_tool` 调 `playwright.*` tool,不直接 spawn npx
- **不点击 submit**:`form.prepare` 仅 fill 表单,不 click submit;用户审批后单独调 `playwright.click`
- **allowed_paths 为空**:Playwright MCP 不直接访问文件系统,所有写操作走 `filesystem.write`
- **错误处理**:MCP 不可用 → Skill 返回 `ToolStatus::Failed` + `error_code = "mcp_playwright_unavailable"`
- **Node 依赖提示**:首次调用时若 MCP server 启动失败,UI 弹"请安装 Node.js ≥ 18"
- **Commit message**:`feat(w7p5): ...` / `test(w7p5): ...`

---

## Task 1: mcp/repo.rs 加 insert_default_servers

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/repo.rs`

- [ ] **Step 1:** 实现 `pub fn insert_default_servers(conn: &Connection) -> Result<()>`:查 `mcp_servers` 表为空 → 插入 `playwright` 记录(`server_id='playwright', command='npx', args='["-y", "@playwright/mcp@latest"]', enabled=1`)
- [ ] **Step 2:** 单元测试:空表 → 插入后查到 1 条;非空表 → 不插入
- [ ] **Step 3:** `cargo test -p trust-kernel --lib mcp::repo::`
- [ ] **Step 4:** commit `feat(w7p5): add insert_default_servers (playwright on empty mcp_servers table)`

## Task 2: kernel.rs boot 集成

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`

- [ ] **Step 1:** `boot()` 末尾调 `McpServerRepo::insert_default_servers(&conn)?`
- [ ] **Step 2:** `cargo check -p trust-kernel`
- [ ] **Step 3:** 集成测试:新 kernel 实例 → `mcp_servers` 表有 `playwright` 记录
- [ ] **Step 4:** commit `feat(w7p5): kernel boot inserts default playwright MCP server`

## Task 3: skills/common.rs 加 invoke_mcp_tool helper

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/common.rs`

- [ ] **Step 1:** 实现 `pub fn invoke_mcp_tool(kernel: &TrustKernel, server_id: &str, tool_name: &str, args: serde_json::Value) -> KernelResult<ToolResult>`
- [ ] **Step 2:** 单元测试:mock `McpClient` 验证 args 传递 + ToolResult 解析
- [ ] **Step 3:** `cargo test -p trust-kernel --lib skills::common::`
- [ ] **Step 4:** commit `feat(w7p5): add invoke_mcp_tool helper in skills/common.rs`

## Task 4: research.save_markdown executor

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/research_save.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`

- [ ] **Step 1:** `manifest.rs` 升级 `research_save_manifest`:`inputs` 加 `url: { input_type: url, required: true }` + `save_path: { input_type: path, required: true, allowed_roots: ["%USERPROFILE%"] }`
- [ ] **Step 2:** `research_save.rs` 实现 `execute_research_save(kernel, input, approver) -> KernelResult<TaskId>`:
  - `invoke_mcp_tool(kernel, "playwright", "navigate", {"url": input.url})`
  - `invoke_mcp_tool(kernel, "playwright", "snapshot", {})` 获取 accessibility tree
  - `invoke_mcp_tool(kernel, "playwright", "eval", {"script": "document.querySelector('main')?.innerText || document.body.innerText"})` 提取主要内容
  - 调 `filesystem.write` 保存为 `.md`
  - PerStep approval 在 navigate 前触发
- [ ] **Step 3:** `mod.rs` 加 `pub mod research_save;`
- [ ] **Step 4:** 单元测试:mock MCP 返回 snapshot + eval → 验证 filesystem.write 调用
- [ ] **Step 5:** `cargo test -p trust-kernel --lib skills::research_save::`
- [ ] **Step 6:** commit `feat(w7p5): implement research.save_markdown executor (navigate + snapshot + eval + write)`

## Task 5: form.prepare executor

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/form_prepare.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`

- [ ] **Step 1:** `manifest.rs` 升级 `form_prepare_manifest`:`inputs` 加 `url: { input_type: url, required: true }` + `fields: { input_type: object, required: true }`(JSON 对象,selector → value)
- [ ] **Step 2:** `form_prepare.rs` 实现 `execute_form_prepare(kernel, input, approver) -> KernelResult<TaskId>`:
  - `invoke_mcp_tool(kernel, "playwright", "navigate", {"url": input.url})`
  - `invoke_mcp_tool(kernel, "playwright", "snapshot", {})` 获取表单元素 selector
  - 遍历 `input.fields` 调 `invoke_mcp_tool(kernel, "playwright", "fill", {"selector": ..., "value": ...})`
  - **不**调 `playwright.click` submit
  - PerStep approval 在 navigate + 每个 fill 前触发
- [ ] **Step 3:** `mod.rs` 加 `pub mod form_prepare;`
- [ ] **Step 4:** 单元测试:mock MCP 验证 navigate + fill 调用链(无 click)
- [ ] **Step 5:** `cargo test -p trust-kernel --lib skills::form_prepare::`
- [ ] **Step 6:** commit `feat(w7p5): implement form.prepare executor (navigate + fill, no submit)`

## Task 6: 注册 2 个浏览器 Skill 到 SkillRouter

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`
- Modify: `voicepilot/crates/ui/src/commands.rs`

- [ ] **Step 1:** `router_bridge.rs` `router.register(research_save_manifest()); router.register(form_prepare_manifest());`
- [ ] **Step 2:** `commands.rs` `route_text` 同样注册(取消 Plan 1 TODO 注释)
- [ ] **Step 3:** `cargo test -p trust-kernel --lib skills::router::`
- [ ] **Step 4:** commit `feat(w7p5): register 2 browser skills in SkillRouter`

## Task 7: 端到端冒烟测试 + Node 依赖提示

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w7_plan5_mcp_playwright_smoke.rs`
- Modify: `voicepilot/crates/ui/web/src/components/SettingsView.tsx`
- Modify: `voicepilot/crates/ui/web/src/components/SkillsManager.tsx`

- [ ] **Step 1:** 测试(mock MCP stdio):`research.save_markdown` mock navigate + snapshot + eval → 验证 `.md` 文件生成
- [ ] **Step 2:** 测试(mock MCP stdio):`form.prepare` mock navigate + snapshot + fill × N → 验证无 click submit
- [ ] **Step 3:** `#[ignore]` 测试:真实 Playwright MCP 抓 `https://example.com`(需 Node + 网络)
- [ ] **Step 4:** `SettingsView.tsx` 加 Playwright MCP 配置提示
- [ ] **Step 5:** `SkillsManager.tsx` 标识 2 个浏览器 Skill 需 Playwright MCP 启用
- [ ] **Step 6:** `cargo test -p trust-kernel --test w7_plan5_mcp_playwright_smoke`
- [ ] **Step 7:** commit `test(w7p5): add e2e smoke tests (mock + #[ignore] real) + UI hints`

## Task 8: 文档 + 验收

**Files:**
- Create: `docs/playwright-mcp-setup.md`
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1:** 写 `docs/playwright-mcp-setup.md`:Node.js ≥ 18 安装 + 首次 `npx -y @playwright/mcp@latest` 自动下载说明 + 故障排查
- [ ] **Step 2:** `cargo check --workspace --features voice,tauri,llm`
- [ ] **Step 3:** `cargo test --workspace --features voice,tauri,llm`
- [ ] **Step 4:** 更新 `docs/PROGRESS.md` Plan 5 完成状态
- [ ] **Step 5:** commit `docs(w7p5): add playwright-mcp-setup guide + update PROGRESS.md`

---

## Acceptance Gates

- `mcp_servers` 表首次启动有 `playwright` 记录
- 2 个浏览器 Skill 各至少 1 个 mock 单元测试通过
- `form.prepare` 不调用 `playwright.click` submit(单元测试断言)
- 真实 Playwright MCP 测试标 `#[ignore]`,本地可手动验证
- 全 workspace cargo check + test 无回归
