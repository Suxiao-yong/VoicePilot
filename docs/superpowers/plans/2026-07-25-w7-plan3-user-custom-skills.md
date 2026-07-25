# W7 Plan 3: 用户自定义 Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 按 W7 设计文档 §2.5 实现用户自定义 Skill 加载:扫描 `%APPDATA%\voicepilot\skills\*.md`(Windows),解析 Markdown + YAML frontmatter,反序列化为 `SkillManifest`,注册到 `SkillRouter`(用户自定义可覆盖同名 built-in);UI 端 Skills Manager 加"导入 Skill"按钮。

**Architecture:** `skills/user_loader.rs` 负责目录扫描 + frontmatter 分离 + `serde_yaml` 反序列化 + 注册到 `SkillRepo` / `SkillRouter`;Tauri 端 `skills_commands.rs` 提供 `reload_skills` / `import_skill` / `list_user_skills` 三个命令;前端 `SkillsManager.tsx` 加文件选择器调 `dialog.open()`。

**Tech Stack:** Rust(stable),`serde_yaml = "0.9"`(workspace 已有),Tauri 2 `dialog` 插件,React + TypeScript。

**Spec:** `docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md` §2.5

**Precondition:** Plan 2 已完成。

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Create** `src/skills/user_loader.rs` — `scan_user_skills() -> Vec<SkillManifest>` + `parse_skill_md() -> Result<SkillManifest>`
- **Modify** `src/skills/mod.rs` — `pub mod user_loader;`
- **Modify** `src/skills/manifest.rs` — `SkillManifest` 加 `description_body: Option<String>`(Markdown body 部分)
- **Modify** `src/kernel.rs` — `boot()` 末尾调 `user_loader::scan_user_skills()` + 注册到 `SkillRepo` + `SkillRouter`
- **Modify** `src/skills/router.rs` — `register` 时若已存在同 id,覆盖(built-in 优先级低,用户自定义覆盖)

### Backend — ui(`voicepilot/crates/ui/`)

- **Create** `src/skills_commands.rs` — `reload_skills_command` / `import_skill_command` / `list_user_skills_command`
- **Modify** `src/commands.rs` — `register_handlers` 加 3 个新命令
- **Modify** `src/lib.rs` — `pub mod skills_commands;`

### Frontend(`voicepilot/crates/ui/web/src/`)

- **Modify** `types.ts` — 加 `UserSkill` interface(id / title / description / source_path)
- **Modify** `api.ts` — 加 `invokeReloadSkills` / `invokeImportSkill` / `invokeListUserSkills`
- **Modify** `components/SkillsManager.tsx` — 加"导入 Skill"按钮 + 用户自定义 Skill 列表区
- **Modify** `components/SettingsView.tsx` — 加"用户 Skill 目录"提示(`%APPDATA%\voicepilot\skills\`)

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w7_plan3_user_skill_smoke.rs`
- **Modify** `voicepilot/crates/trust-kernel/src/skills/user_loader.rs` — `#[cfg(test)] mod tests`

### Docs

- **Modify** `docs/PROGRESS.md` — Plan 3 完成状态

---

## Conventions

- **YAML frontmatter 分隔符**:`---`(三横线),首次出现到第二次出现之间为 frontmatter,其余为 Markdown body
- **错误处理**:单个 Skill 文件解析失败 → 跳过该文件 + 日志 `skill_load_error: <path>`,不阻断其他 Skill 加载
- **覆盖语义**:用户自定义 Skill 同 id 覆盖 built-in(用户优先);UI 在 Skills Manager 中标识"用户自定义" vs "内置"
- **文件路径**:`%APPDATA%\voicepilot\skills\`(Windows),用 `dirs::data_dir()` 拼接
- **Commit message**:`feat(w7p3): ...` / `test(w7p3): ...`

---

## Task 1: user_loader.rs 解析与扫描

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/user_loader.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1:** 实现 `parse_skill_md(content: &str) -> Result<(SkillManifest, String)>`,分离 frontmatter + body
- [ ] **Step 2:** 实现 `scan_user_skills(dir: &Path) -> Vec<SkillManifest>`,遍历 `*.md` + 调 `parse_skill_md` + 错误跳过
- [ ] **Step 3:** 在 `mod.rs` 加 `pub mod user_loader;`
- [ ] **Step 4:** 单元测试:合法 md / 缺字段 / 非法 YAML / 无 frontmatter 四种 case
- [ ] **Step 5:** `cargo test -p trust-kernel --lib skills::user_loader::`
- [ ] **Step 6:** commit `feat(w7p3): add user_loader with YAML frontmatter parsing`

## Task 2: SkillManifest 加 description_body 字段

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`

- [ ] **Step 1:** 加 `pub description_body: Option<String>` 字段(默认 `None`,serde `#[serde(default)]`)
- [ ] **Step 2:** `parse_skill_md` 把 body 写入 `description_body`
- [ ] **Step 3:** 单元测试:验证 `description_body` 被正确填充
- [ ] **Step 4:** `cargo test -p trust-kernel --lib skills::manifest::`
- [ ] **Step 5:** commit `feat(w7p3): add description_body field to SkillManifest for markdown body`

## Task 3: SkillRouter 覆盖语义

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/router.rs`

- [ ] **Step 1:** `register` 改为:若同 id 已存在,替换;否则 push
- [ ] **Step 2:** 单元测试:built-in 先注册,用户自定义后注册同 id,验证 `route` 返回用户版本
- [ ] **Step 3:** `cargo test -p trust-kernel --lib skills::router::`
- [ ] **Step 4:** commit `feat(w7p3): SkillRouter register now overrides same-id skills (user > built-in)`

## Task 4: kernel.rs boot 集成

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`

- [ ] **Step 1:** `boot()` 末尾加 `let user_skills = user_loader::scan_user_skills(&skills_dir);`
- [ ] **Step 2:** 对每个 user_skill 调 `SkillRepo::upsert` + `SkillRouter::register`
- [ ] **Step 3:** `skills_dir` 用 `dirs::data_dir().join("voicepilot/skills")`,不存在则创建
- [ ] **Step 4:** `cargo check -p trust-kernel`
- [ ] **Step 5:** commit `feat(w7p3): kernel boot scans and registers user skills`

## Task 5: skills_commands.rs Tauri 命令

**Files:**
- Create: `voicepilot/crates/ui/src/skills_commands.rs`
- Modify: `voicepilot/crates/ui/src/commands.rs`
- Modify: `voicepilot/crates/ui/src/lib.rs`

- [ ] **Step 1:** 实现 `reload_skills_command(state) -> Result<Vec<UserSkillDto>, String>`:重扫目录 + 重建 SkillRouter + 返回列表
- [ ] **Step 2:** 实现 `import_skill_command(state, source_path) -> Result<UserSkillDto, String>`:`dialog.open()` 选 `.md` → 复制到 skills 目录 → reload
- [ ] **Step 3:** 实现 `list_user_skills_command(state) -> Result<Vec<UserSkillDto>, String>`:返回当前用户自定义 Skill 列表
- [ ] **Step 4:** 在 `commands.rs` `register_handlers` 注册 3 个命令
- [ ] **Step 5:** `cargo check -p voicepilot-ui --features tauri`
- [ ] **Step 6:** commit `feat(w7p3): add reload/import/list user skills Tauri commands`

## Task 6: 前端 SkillsManager 加导入按钮

**Files:**
- Modify: `voicepilot/crates/ui/web/src/components/SkillsManager.tsx`
- Modify: `voicepilot/crates/ui/web/src/types.ts`
- Modify: `voicepilot/crates/ui/web/src/api.ts`

- [ ] **Step 1:** `types.ts` 加 `UserSkill` interface
- [ ] **Step 2:** `api.ts` 加 `invokeReloadSkills` / `invokeImportSkill` / `invokeListUserSkills`
- [ ] **Step 3:** `SkillsManager.tsx` 加"导入 Skill"按钮(调 `invokeImportSkill`) + 用户自定义 Skill 列表区(显示 id/title/description/source_path)
- [ ] **Step 4:** `npm.cmd run build` 通过
- [ ] **Step 5:** commit `feat(w7p3): SkillsManager UI with import button and user skill list`

## Task 7: 端到端冒烟测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w7_plan3_user_skill_smoke.rs`

- [ ] **Step 1:** 测试:tempdir 创建 `my-test.md`(合法 frontmatter)→ `scan_user_skills` 返回 1 个 → `SkillRouter::register` 后 `route` 命中
- [ ] **Step 2:** 测试:tempdir 创建 `bad.md`(非法 YAML)→ `scan_user_skills` 跳过 + 日志 + 返回 0 个
- [ ] **Step 3:** 测试:用户自定义覆盖 built-in(同 id,先注册 built-in,后注册用户)→ `route` 返回用户版本
- [ ] **Step 4:** `cargo test -p trust-kernel --test w7_plan3_user_skill_smoke`
- [ ] **Step 5:** commit `test(w7p3): add e2e smoke tests for user skill loading`

## Task 8: 验收 + PROGRESS.md

- [ ] **Step 1:** `cargo check --workspace --features voice,tauri,llm`
- [ ] **Step 2:** `cargo test --workspace --features voice,tauri,llm`
- [ ] **Step 3:** `npm.cmd run build`
- [ ] **Step 4:** 更新 `docs/PROGRESS.md` Plan 3 完成状态
- [ ] **Step 5:** commit `docs(w7p3): update PROGRESS.md with Plan 3 completion`

---

## Acceptance Gates

- 用户在 `%APPDATA%\voicepilot\skills\` 放入 `.md` 后,Skills Manager 中可见
- 同 id 用户自定义 Skill 覆盖 built-in
- 非法 YAML 文件不阻断其他 Skill 加载
- 全 workspace cargo check + test 无回归
