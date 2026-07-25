# W7: LLM Planner + 8 Skills Design Spec

**日期:** 2026-07-25(Asia/Shanghai)
**对应规格:** V1.1.2 §5.1 Skill Router / §5.2 files.organize / §5.3 Skill Manifest / §5.4 LLM Planner(占位)/ §8.3 Skills Manager / §8.4 语音转写快速纠错
**前置:** W6c Fast-Follow 已完成(commit `6cb6ed7`,236+48+78 测试通过)
**范围:** 引入云端 LLM(OpenAI 兼容 API)用于意图分类与 Slot 提取 fallback;完整实现 8 个内置 Skill;支持用户自定义 Skill(Markdown + YAML frontmatter);接入 Windows UIA 自动化与 Playwright MCP 浏览器自动化

---

## 1. 背景

W3b-W6 系列 SkillRouter 维持纯关键词匹配(`intent_examples` 双向 substring + `keywords` substring),SlotParser 维持 regex 提取。W6c 完成后剩余短板:
- 5 个 Skill 占位(`task.repeat_verified` / `task.explain` / `task.compensate` / `note.capture` / `research.save_markdown` / `form.prepare` / `quick.app_control`)未实现
- 关键词匹配无法处理同义表达(如"归档下载目录"不命中 `intent_examples` 但语义属于 `files.organize`)
- regex SlotParser 无法提取无明确语法结构的 Slot(如"昨天的报告" → 时间范围)
- 用户无法自定义 Skill(`SkillManifest` 已支持 `Deserialize` 但无加载入口)
- 无浏览器自动化能力(无法处理"把网页内容存为 Markdown"等场景)
- 无 Windows 应用控制能力(无法处理"打开记事本写 TODO"等场景)

W7 闭合以上短板,引入 OpenAI 兼容 LLM API(默认 DeepSeek,用户可改 base_url 切换到 OpenAI / 通义千问 / Kimi 等),并通过 W4 MCP 框架接入 Playwright MCP。

---

## 2. 范围

W7 共 8 项工作:

| # | 项 | 优先级 |
|---|---|---|
| 1 | LLM 客户端(`crates/trust-kernel/src/llm/`) | P0 |
| 2 | SkillRouter LLM fallback | P0 |
| 3 | SlotParser LLM fallback(仅当 regex 0 命中时) | P0 |
| 4 | 8 Skills 完整实现 | P0 |
| 5 | 用户自定义 Skill(Markdown + YAML frontmatter) | P1 |
| 6 | Windows UIA 自动化(`uiautomation-rs`) | P1 |
| 7 | Playwright MCP 浏览器自动化 | P1 |
| 8 | Settings UI 扩展(LLM 配置 fieldset) | P0 |

### 2.1 LLM 客户端(P0)

**新增文件:** `crates/trust-kernel/src/llm/mod.rs`(及 `crates/trust-kernel/src/llm/client.rs` / `types.rs` 拆分)

**依赖:** `reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }`(已在 workspace;若未启用 `default-features = false`,W7 显式声明以避免 OpenSSL 依赖)

**LlmClient 结构:**

```rust
pub struct LlmClient {
    base_url: String,       // e.g. "https://api.deepseek.com/v1"
    api_key: String,        // 用户在 Settings 中填写
    model: String,          // 默认 "deepseek-chat"
    http: reqwest::Client,
    timeout: Duration,      // 默认 30s
}

impl LlmClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self;
    pub fn disabled() -> Self; // base_url/api_key 为空时返回的 no-op 客户端

    pub fn is_enabled(&self) -> bool;

    /// 意图分类 + Slot 提取(单次 LLM 调用)。
    /// 调用 OpenAI 兼容 `/chat/completions` 端点,
    /// 使用 function calling 强制结构化输出。
    pub async fn classify_and_extract(
        &self,
        text: &str,
        candidate_skills: &[SkillManifest],
    ) -> Result<LlmRouteResponse>;
}

pub struct LlmRouteResponse {
    pub matched_skill_id: Option<String>,  // None = 走 Planner
    pub confidence: f32,                   // [0.0, 1.0]
    pub slots: Vec<ExtractedSlot>,         // LLM 提取的 Slot
    pub reasoning: String,                 // LLM 解释(用于审计)
}

pub struct ExtractedSlot {
    pub kind: String,    // "path" / "app" / "number" / "recipient" / "delete_target" / "time_range" / "url"
    pub raw: String,
    pub high_risk: bool,
}
```

**Function calling schema(发给 LLM):**

```json
{
  "name": "route_skill",
  "parameters": {
    "type": "object",
    "properties": {
      "matched_skill_id": { "type": ["string", "null"] },
      "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
      "slots": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "kind": { "type": "string" },
            "raw": { "type": "string" },
            "high_risk": { "type": "boolean" }
          }
        }
      },
      "reasoning": { "type": "string" }
    },
    "required": ["matched_skill_id", "confidence", "slots", "reasoning"]
  }
}
```

**System prompt 要点(中文):**
- 你是 VoicePilot 的意图分类器,从用户语音转写文本中识别要执行的 Skill
- 候选 Skill 列表(每个含 id / title / description / intent_examples / inputs)
- 若没有匹配的 Skill,返回 `matched_skill_id: null` + `confidence < 0.7`
- Slot 提取遵循 `inputs` 中的 `input_type` 约束
- 不得执行任何动作,只返回路由决策

**错误处理:**
- HTTP 失败 / JSON 解析失败 → 返回 `LlmResponse::Error(reason)`,SkillRouter 回退到关键词匹配结果
- 超时(30s)→ 同上
- API key 无效(401)→ 同上,UI 在 Settings 中提示

**Privacy:**
- LLM 调用是 opt-in(`llm_enabled = false` 默认)
- `privacy_mode = true` 时禁止 LLM 调用(强制本地 fallback)
- 转写文本 + 候选 Skill id/title/description 发给 LLM,不发用户敏感文件路径

### 2.2 SkillRouter LLM fallback(P0)

**改动文件:** `crates/trust-kernel/src/skills/router.rs`

**新签名:**

```rust
pub struct SkillRouter {
    skills: Vec<SkillManifest>,
    llm: Option<Arc<LlmClient>>,   // None = 无 LLM(W6 行为)
}

impl SkillRouter {
    pub fn new() -> Self;
    pub fn with_llm(llm: Arc<LlmClient>) -> Self;
    pub fn register(&mut self, manifest: SkillManifest);

    /// 同步路由(W6 行为,关键词匹配)
    pub fn route(&self, user_goal: &str) -> RouteDecision;

    /// 异步路由(关键词优先,LLM fallback)
    pub async fn route_with_llm(&self, user_goal: &str) -> RouteDecision;
}
```

**`route_with_llm` 算法:**

1. 计算 keyword_score:`matches_intent_examples` 命中 → score = 1.0;`matches_keywords` 命中 → score = 0.5;否则 score = 0.0
2. 若 score >= 0.5 → 直接返回 `RouteDecision::Skill`(不调 LLM,省钱)
3. 若 score < 0.5 且 LLM 启用 → 调 `llm.classify_and_extract`,confidence >= 0.7 时返回 LLM 匹配的 Skill
4. 否则 → `RouteDecision::Planner`

**RouteDecision 扩展(新增字段):**

```rust
pub enum RouteDecision {
    Skill(Box<SkillManifest>),
    SkillWithSlots(Box<SkillManifest>, Vec<ExtractedSlot>),  // W7 新增:LLM 提取了 Slot
    Planner,
}
```

`SkillWithSlots` 携带 LLM 提取的 Slot,经 UI 反馈给用户修改 / Apply 后再执行。

### 2.3 SlotParser LLM fallback(P0)

**改动文件:** `crates/ui/src/slot_parser.rs`(或迁移到 trust-kernel 共享模块)

**算法:**
1. 调 `SlotParser::parse(text)`(regex 提取)
2. 若 regex 提取到 ≥ 1 个 Slot → 直接返回(regex 优先,延迟低)
3. 若 regex 0 命中且 LLM 启用 → 调 `llm.classify_and_extract(text, &[matched_skill_manifest])` 提取 Slot
4. 若 LLM 也未提取到 Slot → 返回空 Vec(用户可手动 Chip 修改)

**SlotKind 扩展:**
- 新增 `TimeRange`(W7 LLM 提取,如"昨天" → `{"start": "2026-07-24", "end": "2026-07-24"}`)
- 新增 `Url`(W7 浏览器自动化场景)

### 2.4 8 Skills 完整实现(P0)

| Skill ID | 实现文件 | tools | risk | approval |
|---|---|---|---|---|
| `files.organize` | `skills/executor.rs`(W3b 已实现) | filesystem.* | E2 | BatchOnce |
| `task.repeat_verified` | `skills/task_repeat.rs`(W7 新增) | filesystem.search_files + verify_move | E1 | PerStep |
| `task.explain` | `skills/task_explain.rs`(W7 新增) | audit.read | E0 | None |
| `task.compensate` | `skills/task_compensate.rs`(W7 新增) | compensation.auto_reverse | E2 | PerStep |
| `quick.app_control` | `skills/app_control.rs`(W7 新增,UIA) | uiautomation.* | E2 | PerStep |
| `note.capture` | `skills/note_capture.rs`(W7 新增,UIA) | uiautomation.* + filesystem.write | E1 | PerStep |
| `research.save_markdown` | `skills/research_save.rs`(W7 新增,Playwright MCP) | mcp.playwright.* + filesystem.write | E2 | PerStep |
| `form.prepare` | `skills/form_prepare.rs`(W7 新增,Playwright MCP) | mcp.playwright.* | E2 | PerStep |

**8 个 manifest 函数** 在 `skills/manifest.rs` 中以 `pub fn xxx_manifest() -> SkillManifest` 形式提供,在 `RouterBridge::route_text` 中注册。

**Skill 之间共享的 helper(放 `skills/common.rs`):**
- `run_prepare_approve_commit(kernel, input, approver)` —— 把 W3b `executor.rs` 的核心循环抽出复用
- `validate_input_against_manifest(input, manifest)` —— 校验 input 满足 `inputs` 约束

### 2.5 用户自定义 Skill(P1)

**新增文件:** `crates/trust-kernel/src/skills/user_loader.rs`

**Skill 文件目录:** `%APPDATA%\voicepilot\skills\*.md`(Windows)

**文件格式(Markdown + YAML frontmatter):**

```markdown
---
id: my-project.deploy
version: "1.0.0"
title: 部署项目
description: 把当前项目部署到生产环境
intent_examples:
  - 部署项目
  - 上线
keywords:
  - 部署
  - 上线
inputs:
  target:
    input_type: enum
    required: true
    allowed_values: [staging, production]
    default: staging
risk_ceiling: E3
data_class_ceiling: D2
egress: local_only
max_steps: 3
tools:
  - filesystem.search_files
  - filesystem.prepare_move
approval:
  mode: per_step
  required_for: both
  show_effect_manifest: true
  max_approval_scope: 1
compensation:
  level: strong
  ttl_seconds: 3600
  conflict_policy: require_confirmation
verifier:
  strategy: strong
  recheck_after_seconds: 0
failure_policy:
  max_retries: 0
  allow_replan: false
  on_fail: ask_user
---

# 部署项目 Skill

用户语音"部署到生产"时触发。会先在项目目录搜索构建产物,
然后 prepare_move 到部署目录,等待用户审批后 commit。
```

**解析流程:**
1. 启动时 / Settings 触发 → 扫描 `%APPDATA%\voicepilot\skills\*.md`
2. 用 `yaml-front-matter` crate(或手写分隔器)分离 frontmatter 与 body
3. frontmatter 用 `serde_yaml::from_str` 反序列化为 `SkillManifest`
4. body 存入 `SkillManifest.description` 后置(可选,UI 展示)
5. 注册到 `SkillRouter`(built-in 优先,用户自定义可覆盖同名 built-in)

**新增依赖:** `serde_yaml = "0.9"`(workspace 已有,确认即可)

**Skills Manager UI 扩展:** 现有 `SkillsManager` 组件加"导入 Skill"按钮,调用 Tauri `dialog.open()` 选 `.md` 文件 → 复制到 `%APPDATA%\voicepilot\skills\` → 触发 `reload_skills` 命令。

### 2.6 Windows UIA 自动化(P1)

**新增依赖:** `uiautomation-rs = "0.16"`(纯 Rust,无 Windows SDK 依赖)

**新增文件:** `crates/trust-kernel/src/uiautomation/mod.rs` + `uiautomation/adapter.rs`

**适配器接口:**

```rust
pub trait UiaAdapter {
    fn launch_app(&self, app_name: &str) -> Result<UiaElementHandle>;
    fn find_window(&self, title_contains: &str) -> Result<Option<UiaElementHandle>>;
    fn find_element(&self, root: &UiaElementHandle, selector: &UiaSelector) -> Result<Option<UiaElementHandle>>;
    fn click(&self, element: &UiaElementHandle) -> Result<()>;
    fn set_text(&self, element: &UiaElementHandle, text: &str) -> Result<()>;
    fn get_text(&self, element: &UiaElementHandle) -> Result<String>;
    fn screenshot(&self, element: &UiaElementHandle) -> Result<Vec<u8>>;
}

pub struct WindowsUiaAdapter { /* wraps uiautomation-rs */ }
```

**已实现 Skill:**
- `quick.app_control`:启动 / 切换 / 关闭应用(`launch_app` / `find_window` + close)
- `note.capture`:打开记事本 → `set_text` 写入内容 → 保存到指定路径

**安全约束:**
- `allowed_apps` 白名单(默认 `notepad` / `explorer` / `calc`,用户可在 Settings 扩展)
- 超出白名单的 app 启动需要 PerStep approval
- `set_text` 仅对窗口标题在白名单内的元素生效

### 2.7 Playwright MCP 浏览器自动化(P1)

**配置:** 通过 W4 `mcp_servers` 表插入 `playwright` 服务器记录(`mcp_servers` 表已存在)

```sql
INSERT INTO mcp_servers (server_id, command, args, env, enabled, allowed_paths)
VALUES ('playwright', 'npx', '["-y", "@playwright/mcp@latest"]', '{}', 1, '[]');
```

**新增 Skill 调用 MCP 工具:**
- `research.save_markdown`:调 `playwright.navigate` → `playwright.snapshot` 获取 accessibility tree → `playwright.eval` 提取主要内容 → `filesystem.write` 保存为 `.md`
- `form.prepare`:调 `playwright.navigate` → `playwright.snapshot` → `playwright.fill` 填充表单 → 不点击 submit(用户审批后调 `playwright.click`)

**改动:** `crates/trust-kernel/src/mcp/repo.rs` 加 `insert_default_servers()` 方法,启动时若 `mcp_servers` 表为空则插入 `playwright`(用户可在 Settings 关闭)。

**端到端约束:** Playwright MCP 需要 Node.js + npx,W7 不打包 Node;用户首次使用时弹提示"请安装 Node.js ≥ 18"。

### 2.8 Settings UI 扩展(P0)

**改动文件:**
- `voicepilot/crates/ui/src/settings_commands.rs`: `SettingsDto` 加 4 字段
- `voicepilot/crates/ui/web/src/types.ts`: `Settings` interface 同步加 4 字段
- `voicepilot/crates/ui/web/src/components/SettingsView.tsx`: 新增"LLM 配置" fieldset

**SettingsDto 新增字段:**

```rust
pub struct SettingsDto {
    // ... 现有字段 ...
    pub llm_enabled: bool,
    pub llm_api_key: String,
    pub llm_base_url: String,    // 默认 "https://api.deepseek.com/v1"
    pub llm_model: String,       // 默认 "deepseek-chat"
    pub llm_provider_url: String, // 默认 "https://platform.deepseek.com/api_keys"
}
```

**KV 映射:** `llm.enabled` / `llm.api_key` / `llm.base_url` / `llm.model` / `llm.provider_url`

**UI:**
- "LLM 配置" fieldset 在"TTS 配置"后
- `llm_enabled` checkbox(label "启用云端 LLM(用于意图分类与 Slot 提取)")
- `llm_api_key` password input
- `llm_base_url` text input(placeholder 显示默认值)
- `llm_model` text input
- "获取 API Key" 链接(`llm_provider_url`,target="_blank" rel="noopener noreferrer")
- 隐私提示:`privacy_mode = true` 时禁用 LLM 配置区,显示 "隐私模式已启用,LLM 不可用"

**provider 切换 helper(可选,V1):** 不实现复杂 provider 选择;用户改 `llm_base_url` 即可切换。文档提供常见 provider 配置表(DeepSeek / OpenAI / 通义 / Kimi)。

---

## 3. 数据流

### 3.1 路由 + Slot 提取流程(W7 增强)

```
[语音转写文本]
      ↓
[SlotParser::parse]  ← regex 优先
      ↓ (0 命中?)
      ↓ 是               ↓ 否
[LLM fallback]      [直接返回 regex Slot]
      ↓
[SkillRouter::route_with_llm]
      ↓ keyword_score >= 0.5?
      ↓ 是               ↓ 否
[返回 Skill]        [LLM classify_and_extract]
      ↓                  ↓ confidence >= 0.7?
      ↓                  ↓ 是           ↓ 否
      ↓             [SkillWithSlots]  [Planner]
      ↓                  ↓
[UI: Chip 修改 / Apply]
      ↓
[Skill::execute(kernel, input, approver)]
      ↓ prepare → approve → commit → verify → compensation
[ToolResult V2]
```

### 3.2 LLM 调用边界

- LLM 仅在路由阶段调用,**不**在 Skill 执行阶段调用(LLM 不直接驱动文件操作)
- LLM 调用结果路由决策 + Slot 提取 → 经 UI 反馈给用户 → 用户 Apply 后才执行 Skill
- LLM 不接触用户文件内容(只接触转写文本 + 候选 Skill manifest 的 id/title/description)

---

## 4. 数据库迁移

**新增迁移文件:** `crates/trust-kernel/src/migrations/002_llm_and_skills.sql`

```sql
-- W7: LLM 配置已通过 app_config KV 表存储,无需新表。

-- W7: mcp_servers 表已存在(W4 创建),仅插入默认 playwright 记录。
-- 通过代码层 `insert_default_servers()` 在首次启动时插入(若表为空)。

-- W7: skills 表已存在(W6b-2 创建),W7 新增 8 个内置 Skill 的 upsert 在启动时执行。
-- 用户自定义 Skill 也 upsert 到 skills 表,manifest_json 字段存完整 YAML 反序列化后的 JSON。
```

**回滚:** 删除 `002_llm_and_skills.sql`(W7 不修改 schema,仅数据填充)

---

## 5. 错误处理

| 场景 | 处理 |
|---|---|
| LLM HTTP 401 | UI Settings 提示 "API Key 无效",自动 disable `llm_enabled` |
| LLM 超时 | 回退到关键词匹配,日志记录 `llm_timeout` |
| LLM 返回非 JSON | 回退到关键词匹配,日志记录 `llm_parse_error` |
| 用户自定义 Skill YAML 解析失败 | 跳过该文件,日志记录 `skill_load_error: <path>`,其他 Skill 正常加载 |
| UIA 启动 app 失败 | Skill 返回 `ToolStatus::Failed`,`error_code = "uia_launch_failed"` |
| Playwright MCP 不可用 | Skill 返回 `ToolStatus::Failed`,`error_code = "mcp_playwright_unavailable"` |
| `privacy_mode = true` 时 LLM 被调用 | 强制回退到关键词匹配(代码层 `if privacy_mode { return None; }`) |

**审计日志:** 每次 LLM 调用记录到 `audit_logs` 表,包含 `request_text` / `response_json` / `latency_ms` / `success`。

---

## 6. 测试策略

### 6.1 单元测试

- `llm/client.rs`:mock HTTP server(`wiremock` crate)测试 `classify_and_extract`
  - 200 + 合法 JSON → 解析为 `LlmRouteResponse`
  - 401 → 返回 `Error`
  - 超时 → 返回 `Error`
- `skills/router.rs`:`route_with_llm` 4 个分支(keyword 高分 / keyword 低分 + LLM 高置信 / keyword 低分 + LLM 低置信 / LLM disabled)
- `skills/user_loader.rs`:解析样例 `.md` 文件,校验 frontmatter + body 分离
- `slot_parser.rs`:regex 0 命中时调 LLM mock 提取 Slot
- 8 个 Skill executor 各 1-3 个单元测试(prepare / approve / commit 流程)

### 6.2 集成测试

- `voicepilot/crates/trust-kernel/tests/w7_router_llm_smoke.rs`:端到端路由(关键词命中 + LLM 命中 + LLM fallback)
- `voicepilot/crates/trust-kernel/tests/w7_user_skill_smoke.rs`:创建临时 `%APPDATA%\voicepilot\skills\test.md`,启动后验证加载
- `voicepilot/crates/trust-kernel/tests/w7_mcp_playwright_smoke.rs`:mock MCP stdio server,验证 `research.save_markdown` 调用链
- `voicepilot/crates/ui/tests/w7_settings_llm_smoke.rs`:Settings LLM 配置 UI E2E(写入 + 读取 + 路由生效)

### 6.3 手动验证项(不写自动测试)

- Windows UIA 启动 notepad + 写入文本(需 GUI 环境,标 `#[ignore]`)
- 真实 Playwright MCP 抓取网页(需 Node + 网络,标 `#[ignore]`)
- 真实 LLM 调用(需 API key,标 `#[ignore]`)

### 6.4 现有测试回归

- W6c 236 + 48 + 78 测试全部 PASS
- 新增 LLM mock 测试不依赖网络
- W7 不修改 voice / tauri feature 边界

---

## 7. 验收门禁

### 7.1 编译门禁

- `cargo check --workspace --no-default-features` PASS
- `cargo check --workspace --features voice` PASS
- `cargo check --workspace --features tauri` PASS
- `cargo check --workspace --features voice,tauri` PASS
- `cargo clippy --workspace --no-default-features -- -D warnings` 0 warnings
- `npm.cmd run build` PASS

### 7.2 测试门禁

- 现有 236 + 48 + 78 测试全部 PASS(无回归)
- 新增 W7 测试 ≥ 20 个:
  - LLM client mock(trust-kernel): 4
  - Router LLM fallback(trust-kernel): 4
  - SlotParser LLM fallback(trust-kernel): 3
  - 8 Skill executors(trust-kernel): 8(每个至少 1 个,files.organize 复用 W3b 已有测试)
  - User Skill loader(trust-kernel): 3
  - E2E smoke(trust-kernel × 3 + ui × 1): 4
- 总测试数 ≥ 256(default, +20) / +58(tauri, +10 Settings LLM E2E) / +78 voice(不变,W7 不改 voice 模块)

### 7.3 功能门禁

- 用户在 Settings 中启用 LLM + 填入 API key 后,语音输入"打开记事本写 TODO" → 命中 `note.capture` Skill
- 关闭 LLM 时,同语音输入 → 命中关键词 "打开" + `quick.app_control`(但参数不足,UI 提示补 Slot)
- 用户在 `%APPDATA%\voicepilot\skills\` 放入自定义 `.md` 后,重启应用 → Skills Manager 中可见
- `research.save_mark_markdown` 在 Playwright MCP 启用时,语音"把这个网页存为 Markdown" + URL Slot → 执行抓取并保存

### 7.4 安全门禁

- `privacy_mode = true` 时 LLM 不被调用(单元测试覆盖)
- LLM 调用审计日志完整(`audit_logs` 表新增 `llm_call` 类型记录)
- UIA 限制 `allowed_apps` 白名单(超出时 PerStep approval)
- Playwright MCP `allowed_paths` 为空(不直接访问文件系统,所有写操作走 `filesystem.write`)

---

## 8. 已知偏离 / 延后项

- **本地 LLM 路径不实现:** V1.1 §5.4 提及"LLM Planner",但未规定本地 vs 云端。W7 选云端 OpenAI 兼容 API(用户决策),本地 LLM(ollama / llama.cpp)延后 W8+(性能 < 2s 端到端门槛在当前硬件下不可达)。
- **Skill 之间不组合:** W7 LLM Planner 仅做单 Skill 路由,不做 Skill 编排(如"打开记事本写 TODO 然后保存到桌面" 拆分为 `note.capture` + `files.move` 两步)。Skill 编排延后 W8+(需 DAG 调度器)。
- **LLM 不直接驱动文件操作:** LLM 仅做路由 + Slot 提取,Skill 执行仍走 prepare→approve→commit 强约束。LLM 不可绕过审批。
- **`task.explain` 不接 LLM:** 当前 `task.explain` 仅展示 audit log + 步骤状态,不让 LLM 解释"为什么这一步失败"(避免 LLM 幻觉)。延后 W8+ 接 LLM 解释。
- **macOS / Linux UIA:** `uiautomation-rs` 仅支持 Windows。macOS 用 AXUIElement / Linux 用 AT-SPI 延后 W8+。
- **Playwright MCP Node 依赖:** 不打包 Node.js,用户首次使用时弹提示。延后 W8+ 探讨打包内置 Node runtime。
- **用户自定义 Skill 的 inputs 校验:** W7 仅做 `serde_yaml` 反序列化校验,不做 `allowed_roots` / `allowed_values` 的运行时校验(W6b-2 已有 schema,运行时校验延后 W8)。
- **LLM 调用计费 / 速率限制:** W7 不实现 token 计数 / 速率限制(用户在 LLM provider 侧管理)。延后 W8+ 探讨本地速率限制。
- **Skill 版本管理:** W7 仅 `version` 字段记录,不做版本升级 / 回滚。延后 W8+。
- **D3/E3 红色高亮:** 仍延后(自 W6b-3a 起未实现),W7 不在范围。

---

## 9. 实现顺序建议(供 writing-plans 参考)

1. **Task 1-2:** LLM client + Router LLM fallback(基础设施,其他 Task 依赖)
2. **Task 3:** SlotParser LLM fallback
3. **Task 4:** Settings UI 扩展(让用户能配置 LLM)
4. **Task 5-12:** 8 个 Skill 实现(可并行)
5. **Task 13:** 用户自定义 Skill 加载
6. **Task 14:** Windows UIA 适配器 + 2 个 UIA Skill
7. **Task 15:** Playwright MCP 配置 + 2 个浏览器 Skill
8. **Task 16:** 集成测试 + 验收门禁复跑

---

## 10. 参考

- V1.1.2 规格相关章节:§5.1 / §5.2 / §5.3 / §5.4 / §8.3 / §8.4 / §11.1
- W3b `files.organize` 实现:`crates/trust-kernel/src/skills/executor.rs`
- W4 MCP 框架:`crates/trust-kernel/src/mcp/`
- W6b-2 Skills Manager UI:`crates/ui/web/src/components/SkillsManager.tsx`
- W6c SlotParser:`crates/ui/src/slot_parser.rs`
- OpenAI API 参考:https://platform.openai.com/docs/api-reference/chat
- DeepSeek API 参考:https://platform.deepseek.com/api-docs/
- uiautomation-rs:https://github.com/leexgone/uiautomation-rs
- Playwright MCP:https://github.com/microsoft/playwright-mcp
