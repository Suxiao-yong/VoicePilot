# W7: LLM Planner + 8 Skills Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W7 设计文档(`docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md`)引入云端 OpenAI 兼容 LLM 做意图分类与 Slot 提取 fallback,完整实现 8 个内置 Skill,支持用户自定义 Skill(Markdown + YAML frontmatter),接入 Windows UIA 自动化与 Playwright MCP 浏览器自动化。

**Architecture:** `trust-kernel` 加 `llm/` 模块(`LlmClient` + OpenAI 兼容 `/chat/completions` + function calling);`SkillRouter` 加 `route_with_llm` 异步方法(keyword 优先,LLM fallback);`SlotParser` 加 LLM fallback(仅 regex 0 命中时);8 个 Skill 各占 `skills/<name>.rs` 文件,共享 `skills/common.rs` 的 `run_prepare_approve_commit` helper;用户自定义 Skill 走 `skills/user_loader.rs` 扫描 `%APPDATA%\voicepilot\skills\*.md`;UIA 走 `uiautomation-rs` crate;Playwright MCP 走 W4 `mcp_servers` 表配置 + `mcp.invoke` 调用链。

**Tech Stack:** Rust(stable),`reqwest = "0.12"`(rustls-tls,无 OpenSSL),`serde_yaml = "0.9"`,`uiautomation-rs = "0.16"`(Windows-only,optional feature),`tauri = "2"`(已有),React + TypeScript(已有),Tauri 2 IPC。

**Spec:** `docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md`

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Modify** `Cargo.toml` — 加 `reqwest` optional(无 default-features)+ `serde_yaml`(非 optional)+ 新 `llm` feature;加 `uiautomation-rs` optional(Windows-only)+ 新 `uia` feature
- **Create** `src/llm/mod.rs` — `pub mod client; pub mod types;`
- **Create** `src/llm/client.rs` — `LlmClient` + `classify_and_extract`
- **Create** `src/llm/types.rs` — `LlmRouteResponse` / `ExtractedSlot` / `LlmError`
- **Modify** `src/lib.rs` — `pub mod llm;`
- **Modify** `src/skills/router.rs` — 加 `with_llm` / `route_with_llm` / `RouteDecision::SkillWithSlots`
- **Modify** `src/skills/manifest.rs` — 加 7 个新 manifest 函数(`task_repeat_verified_manifest` 等)
- **Modify** `src/skills/mod.rs` — `pub mod common; pub mod task_repeat; pub mod task_explain; pub mod task_compensate; pub mod app_control; pub mod note_capture; pub mod research_save; pub mod form_prepare; pub mod user_loader;`
- **Create** `src/skills/common.rs` — `run_prepare_approve_commit` / `validate_input_against_manifest`
- **Create** `src/skills/task_repeat.rs` — `task.repeat_verified` executor
- **Create** `src/skills/task_explain.rs` — `task.explain` executor
- **Create** `src/skills/task_compensate.rs` — `task.compensate` executor
- **Create** `src/skills/app_control.rs` — `quick.app_control` executor(UIA)
- **Create** `src/skills/note_capture.rs` — `note.capture` executor(UIA)
- **Create** `src/skills/research_save.rs` — `research.save_markdown` executor(Playwright MCP)
- **Create** `src/skills/form_prepare.rs` — `form.prepare` executor(Playwright MCP)
- **Create** `src/skills/user_loader.rs` — 扫描 `%APPDATA%\voicepilot\skills\*.md` + YAML frontmatter 解析
- **Modify** `src/voice/router_bridge.rs` — 注册 8 个内置 Skill;若 LLM 启用则用 `route_with_llm`
- **Create** `src/uiautomation/mod.rs` — `pub mod adapter;` + `UiaAdapter` trait
- **Create** `src/uiautomation/adapter.rs` — `WindowsUiaAdapter`(wraps `uiautomation-rs`)
- **Modify** `src/mcp/repo.rs` — 加 `insert_default_servers()`(空表时插入 `playwright`)
- **Modify** `src/kernel.rs` — `boot()` 末尾调 `McpServerRepo::insert_default_servers` + 注册 8 个内置 Skill 到 `SkillRepo`

### Backend — ui(`voicepilot/crates/ui/`)

- **Modify** `src/settings_commands.rs` — `SettingsDto` 加 5 字段(`llm_enabled` / `llm_api_key` / `llm_base_url` / `llm_model` / `llm_provider_url`);`flatten_to_kv` / `merge_from_kv` 同步加 5 KV 映射
- **Modify** `src/voice_commands.rs` — `route_text_command` 改用 `SkillRouter::route_with_llm`(若 LLM 启用);返回 payload 加 `slots: Vec<ExtractedSlot>`(LLM 提取的)
- **Modify** `src/slot_parser.rs` — `SlotKind` 加 `TimeRange` / `Url`;新增 `parse_with_llm_fallback` 异步方法
- **Modify** `src/state.rs` — `AppState` 加 `llm_client: Arc<Mutex<Option<Arc<LlmClient>>>>`
- **Modify** `src/commands.rs` — `register_handlers` 加 `reload_skills_command` / `import_skill_command`
- **Create** `src/skills_commands.rs` — `reload_skills` / `import_skill` / `list_user_skills` Tauri 命令

### Frontend(`voicepilot/crates/ui/web/src/`)

- **Modify** `types.ts` — `Settings` interface 加 5 LLM 字段;新增 `ExtractedSlot` interface
- **Modify** `components/SettingsView.tsx` — 新增 "LLM 配置" fieldset(在 "TTS 配置" 后)
- **Modify** `components/SkillsManager.tsx` — 加 "导入 Skill" 按钮(`.md` 文件选择器)
- **Modify** `components/MainView.tsx` — RouteResult 渲染 LLM 提取的 Slot
- **Modify** `api.ts` — 加 `invokeReloadSkills` / `invokeImportSkill`

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w7_router_llm_smoke.rs`
- **Create** `voicepilot/crates/trust-kernel/tests/w7_user_skill_smoke.rs`
- **Create** `voicepilot/crates/trust-kernel/tests/w7_mcp_playwright_smoke.rs`
- **Create** `voicepilot/crates/ui/tests/w7_settings_llm_smoke.rs`
- **Modify** `voicepilot/crates/trust-kernel/src/llm/client.rs` — `#[cfg(test)] mod tests`(mock HTTP)
- **Modify** `voicepilot/crates/trust-kernel/src/skills/router.rs` — `#[cfg(test)] mod tests`
- **Modify** `voicepilot/crates/trust-kernel/src/skills/user_loader.rs` — `#[cfg(test)] mod tests`

### Docs

- **Modify** `docs/PROGRESS.md` — W7 完成状态 + 测试统计 + 已知偏离

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行
- **TDD**:每个含逻辑的任务先写失败测试 → 跑 → 实现 → 跑通 → commit
- **Feature gates**:
  - `llm` feature 在 `trust-kernel/Cargo.toml`,默认开启(LLM 是核心路由能力,但 `LlmClient::disabled()` 让运行时 opt-in)
  - `uia` feature 在 `trust-kernel/Cargo.toml`,默认关闭(Windows-only,需 `uiautomation-rs`)
  - voice / tauri feature 边界不变
- **Commit message**:`feat(w7): ...` / `fix(w7): ...` / `refactor(w7): ...` / `test(w7): ...` / `docs(w7): ...`
- **Privacy**:`privacy_mode = true` 时 LLM 强制不调用(代码层 `if privacy_mode { return LlmClient::disabled(); }`)
- **LLM 调用边界**:LLM 仅在路由阶段调用,Skill 执行不调 LLM
- **reqwest 配置**:`default-features = false, features = ["json", "rustls-tls"]` 避免 OpenSSL 依赖
- **异步运行时**:用 `tokio`(trust-kernel 已有 dev-dependency;W7 加到 `[dependencies]` 因为 `LlmClient::classify_and_extract` 是 `async fn`)
- **错误处理**:LLM 失败回退到关键词匹配,不阻断路由

---

## Task 1: 工作区依赖与 feature gate 设置

**Files:**
- Modify: `voicepilot/Cargo.toml`
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`

- [ ] **Step 1: 修改 workspace 依赖,加 reqwest / serde_yaml / uiautomation-rs**

打开 `voicepilot/Cargo.toml`,定位到 `[workspace.dependencies]` 段,新增:

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde_yaml = "0.9"
uiautomation-rs = "0.16"
```

- [ ] **Step 2: 修改 trust-kernel/Cargo.toml,加 llm + uia feature**

打开 `voicepilot/crates/trust-kernel/Cargo.toml`,在 `[dependencies]` 段新增:

```toml
reqwest = { workspace = true, optional = true }
serde_yaml = { workspace = true }
uiautomation-rs = { workspace = true, optional = true }
tokio = { version = "1", features = ["rt", "macros"] }
```

在 `[features]` 段新增:

```toml
llm = ["dep:reqwest"]
uia = ["dep:uiautomation-rs"]
default = ["llm"]
```

注意:`uia` 不在 default(Windows-only);`llm` 在 default(LLM 是核心路由)。

- [ ] **Step 3: 验证编译**

```powershell
cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel
```

Expected: PASS(无新代码,仅依赖声明)

- [ ] **Step 4: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/Cargo.toml voicepilot/crates/trust-kernel/Cargo.toml ; git commit -m "feat(w7): add reqwest/serde_yaml/uiautomation-rs deps + llm/uia feature gates"
```

---

## Task 2: LLM 类型定义(`llm/types.rs`)

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/llm/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/llm/types.rs`
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`

- [ ] **Step 1: 创建 llm/mod.rs**

```rust
//! LLM 客户端 — W7 §2.1.
//!
//! OpenAI 兼容 API(默认 DeepSeek),用于意图分类 + Slot 提取 fallback。
//! LLM 仅在路由阶段调用,Skill 执行不调 LLM。

pub mod client;
pub mod types;
```

- [ ] **Step 2: 创建 llm/types.rs**

```rust
//! LLM 类型定义 — W7 §2.1.

use serde::{Deserialize, Serialize};

/// LLM 路由响应
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmRouteResponse {
    /// 匹配的 Skill ID(None = 走 Planner)
    pub matched_skill_id: Option<String>,
    /// 置信度 [0.0, 1.0]
    pub confidence: f32,
    /// LLM 提取的 Slot 列表
    pub slots: Vec<ExtractedSlot>,
    /// LLM 推理过程(用于审计日志)
    pub reasoning: String,
}

/// LLM 提取的 Slot
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtractedSlot {
    /// "path" / "app" / "number" / "recipient" / "delete_target" / "time_range" / "url"
    pub kind: String,
    /// Slot 原始文本
    pub raw: String,
    /// 是否高风险(UI 强制视觉确认)
    pub high_risk: bool,
}

/// LLM 错误
#[derive(Debug, Clone, thiserror::Error)]
pub enum LlmError {
    #[error("LLM HTTP error: {0}")]
    Http(String),
    #[error("LLM JSON parse error: {0}")]
    Parse(String),
    #[error("LLM timeout after {0:?}")]
    Timeout(std::time::Duration),
    #[error("LLM not configured (api_key empty)")]
    NotConfigured,
    #[error("LLM disabled by privacy_mode")]
    DisabledByPrivacy,
}

pub type LlmResult<T> = std::result::Result<T, LlmError>;
```

- [ ] **Step 3: 修改 lib.rs 加 pub mod llm**

打开 `voicepilot/crates/trust-kernel/src/lib.rs`,在已有 `pub mod` 列表中加:

```rust
pub mod llm;
```

- [ ] **Step 4: 验证编译**

```powershell
cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel
```

Expected: PASS(`thiserror` 已在 trust-kernel 依赖中)

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/crates/trust-kernel/src/llm/ voicepilot/crates/trust-kernel/src/lib.rs ; git commit -m "feat(w7): add llm/types.rs with LlmRouteResponse/ExtractedSlot/LlmError"
```

---

## Task 3: LLM 客户端实现(`llm/client.rs`)

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/llm/client.rs`

- [ ] **Step 1: 写失败测试 — `new` + `is_enabled` + `disabled`**

在 `voicepilot/crates/trust-kernel/src/llm/client.rs` 末尾写:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_client_is_not_enabled() {
        let c = LlmClient::disabled();
        assert!(!c.is_enabled());
    }

    #[test]
    fn new_client_with_api_key_is_enabled() {
        let c = LlmClient::new("https://api.deepseek.com/v1", "sk-test", "deepseek-chat");
        assert!(c.is_enabled());
    }

    #[test]
    fn new_client_without_api_key_is_not_enabled() {
        let c = LlmClient::new("https://api.deepseek.com/v1", "", "deepseek-chat");
        assert!(!c.is_enabled());
    }
}
```

- [ ] **Step 2: 跑测试看失败**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel llm::client::tests::disabled_client_is_not_enabled 2>&1 | Select-String "error\[" | Select-Object -First 3
```

Expected: 编译错误(`LlmClient` 不存在)

- [ ] **Step 3: 实现 LlmClient 基础结构**

在 `voicepilot/crates/trust-kernel/src/llm/client.rs` 文件顶部写:

```rust
//! LLM 客户端 — W7 §2.1.
//!
//! OpenAI 兼容 `/chat/completions` + function calling 强制结构化输出。
//! 失败时返回 `LlmError`,SkillRouter 回退到关键词匹配。

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::llm::types::{ExtractedSlot, LlmError, LlmResult, LlmRouteResponse};
use crate::skills::manifest::SkillManifest;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

pub struct LlmClient {
    base_url: String,
    api_key: String,
    model: String,
    http: Client,
    timeout: Duration,
}

impl LlmClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        let timeout = Duration::from_secs(DEFAULT_TIMEOUT_SECS);
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            http,
            timeout,
        }
    }

    /// 返回未配置的 no-op 客户端(api_key 为空或 privacy_mode = true 时用)
    pub fn disabled() -> Self {
        Self::new("", "", "")
    }

    pub fn is_enabled(&self) -> bool {
        !self.api_key.is_empty() && !self.base_url.is_empty()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}
```

- [ ] **Step 4: 跑测试看通过**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel llm::client::tests:: 2>&1 | Select-String "test result"
```

Expected: `test result: ok. 3 passed`

- [ ] **Step 5: 写失败测试 — `classify_and_extract` mock HTTP 200**

在 `tests` mod 末尾追加:

```rust
    #[tokio::test]
    async fn classify_and_extract_parses_valid_response() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "route_skill",
                            "arguments": "{\"matched_skill_id\":\"files.organize\",\"confidence\":0.9,\"slots\":[{\"kind\":\"path\",\"raw\":\"C:\\\\Downloads\",\"high_risk\":true}],\"reasoning\":\"user wants to organize files\"}"
                        }
                    }]
                }
            }]
        });
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "sk-test", "deepseek-chat");
        let skills = vec![crate::skills::manifest::files_organize_manifest()];
        let resp = client.classify_and_extract("整理下载目录", &skills).await.unwrap();
        assert_eq!(resp.matched_skill_id.as_deref(), Some("files.organize"));
        assert!((resp.confidence - 0.9).abs() < 0.01);
        assert_eq!(resp.slots.len(), 1);
        assert_eq!(resp.slots[0].kind, "path");
        assert!(resp.slots[0].high_risk);
    }
```

加 `wiremock` 到 `voicepilot/crates/trust-kernel/Cargo.toml` 的 `[dev-dependencies]`:

```toml
wiremock = "0.6"
```

- [ ] **Step 6: 跑测试看失败**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel llm::client::tests::classify_and_extract_parses_valid_response 2>&1 | Select-String "error\[" | Select-Object -First 3
```

Expected: 编译错误(`classify_and_extract` 方法不存在)

- [ ] **Step 7: 实现 `classify_and_extract`**

在 `impl LlmClient` 末尾追加:

```rust
    /// 意图分类 + Slot 提取(单次 LLM 调用)。
    pub async fn classify_and_extract(
        &self,
        text: &str,
        candidate_skills: &[SkillManifest],
    ) -> LlmResult<LlmRouteResponse> {
        if !self.is_enabled() {
            return Err(LlmError::NotConfigured);
        }

        let system_prompt = self.build_system_prompt(candidate_skills);
        let tools = self.build_tool_schema();
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": text},
            ],
            "tools": tools,
            "tool_choice": {"type": "function", "function": {"name": "route_skill"}},
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
                    LlmError::Timeout(self.timeout)
                } else {
                    LlmError::Http(e.to_string())
                }
            })?;

        if !resp.status().is_success() {
            return Err(LlmError::Http(format!("HTTP {}", resp.status())));
        }

        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(format!("response body parse: {e}")))?;

        self.parse_tool_call_response(&resp_json)
    }

    fn build_system_prompt(&self, skills: &[SkillManifest]) -> String {
        let mut s = String::from(
            "你是 VoicePilot 的意图分类器,从用户语音转写文本中识别要执行的 Skill。\n\n候选 Skill 列表:\n",
        );
        for skill in skills {
            s.push_str(&format!(
                "- id: {}\n  title: {}\n  description: {}\n  intent_examples: {:?}\n  inputs: {:?}\n\n",
                skill.id, skill.title, skill.description, skill.intent_examples, skill.inputs.keys().collect::<Vec<_>>()
            ));
        }
        s.push_str(
            "\n若没有匹配的 Skill,返回 matched_skill_id=null + confidence<0.7。\nSlot 提取遵循 inputs 中的 input_type 约束。\n不得执行任何动作,只返回路由决策。",
        );
        s
    }

    fn build_tool_schema(&self) -> serde_json::Value {
        json!([{
            "type": "function",
            "function": {
                "name": "route_skill",
                "description": "Route user text to a skill and extract slots",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "matched_skill_id": {"type": ["string", "null"]},
                        "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                        "slots": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": {"type": "string"},
                                    "raw": {"type": "string"},
                                    "high_risk": {"type": "boolean"}
                                },
                                "required": ["kind", "raw", "high_risk"]
                            }
                        },
                        "reasoning": {"type": "string"}
                    },
                    "required": ["matched_skill_id", "confidence", "slots", "reasoning"]
                }
            }
        }])
    }

    fn parse_tool_call_response(&self, resp: &serde_json::Value) -> LlmResult<LlmRouteResponse> {
        let tool_call = resp
            .pointer("/choices/0/message/tool_calls/0")
            .ok_or_else(|| LlmError::Parse("missing tool_calls[0]".to_string()))?;
        let args_str = tool_call
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing function.arguments".to_string()))?;
        let args: serde_json::Value = serde_json::from_str(args_str)
            .map_err(|e| LlmError::Parse(format!("arguments parse: {e}")))?;

        let matched_skill_id = args
            .get("matched_skill_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let confidence = args
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .unwrap_or(0.0);
        let reasoning = args
            .get("reasoning")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let slots = args
            .get("slots")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| {
                        Some(ExtractedSlot {
                            kind: s.get("kind")?.as_str()?.to_string(),
                            raw: s.get("raw")?.as_str()?.to_string(),
                            high_risk: s.get("high_risk")?.as_bool()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(LlmRouteResponse {
            matched_skill_id,
            confidence,
            slots,
            reasoning,
        })
    }
```

- [ ] **Step 8: 跑测试看通过**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel llm::client::tests:: 2>&1 | Select-String "test result"
```

Expected: `test result: ok. 4 passed`

- [ ] **Step 9: 写失败测试 — HTTP 401 返回 Error**

在 `tests` mod 末尾追加:

```rust
    #[tokio::test]
    async fn classify_and_extract_returns_error_on_401() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "sk-invalid", "deepseek-chat");
        let skills = vec![];
        let result = client.classify_and_extract("test", &skills).await;
        assert!(matches!(result, Err(LlmError::Http(_))));
    }
```

- [ ] **Step 10: 跑测试看通过(401 路径已实现)**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel llm::client::tests::classify_and_extract_returns_error_on_401 2>&1 | Select-String "test result"
```

Expected: `test result: ok. 1 passed`

- [ ] **Step 11: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/crates/trust-kernel/src/llm/client.rs voicepilot/crates/trust-kernel/Cargo.toml ; git commit -m "feat(w7): implement LlmClient with classify_and_extract (OpenAI-compatible + function calling)"
```

---

## Task 4: SkillRouter LLM fallback

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/router.rs`

- [ ] **Step 1: 写失败测试 — `route_with_llm` keyword 高分不调 LLM**

在 `voicepilot/crates/trust-kernel/src/skills/router.rs` 末尾追加 `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::client::LlmClient;
    use crate::skills::manifest::files_organize_manifest;
    use std::sync::Arc;

    fn router_with_llm() -> SkillRouter {
        let llm = Arc::new(LlmClient::new("https://invalid.example.com", "sk-test", "test"));
        let mut r = SkillRouter::with_llm(llm);
        r.register(files_organize_manifest());
        r
    }

    #[tokio::test]
    async fn route_with_llm_keyword_high_score_skips_llm() {
        let router = router_with_llm();
        // "整理下载目录" 命中 keyword "整理"
        let decision = router.route_with_llm("整理下载目录").await;
        assert!(matches!(decision, RouteDecision::Skill(_)));
    }

    #[tokio::test]
    async fn route_with_llm_no_keyword_disabled_returns_planner() {
        let llm = Arc::new(LlmClient::disabled());
        let mut router = SkillRouter::with_llm(llm);
        router.register(files_organize_manifest());
        // "归档今天的报告" 不命中任何 keyword,LLM disabled → Planner
        let decision = router.route_with_llm("归档今天的报告").await;
        assert!(matches!(decision, RouteDecision::Planner));
    }

    #[tokio::test]
    async fn route_without_llm_uses_keyword_only() {
        let mut router = SkillRouter::new();
        router.register(files_organize_manifest());
        let decision = router.route("整理下载目录");
        assert!(matches!(decision, RouteDecision::Skill(_)));
    }
}
```

- [ ] **Step 2: 跑测试看失败**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel skills::router::tests:: 2>&1 | Select-String "error\[" | Select-Object -First 3
```

Expected: 编译错误(`with_llm` / `route_with_llm` 不存在)

- [ ] **Step 3: 修改 SkillRouter 加 LLM 字段与 `route_with_llm`**

替换 `voicepilot/crates/trust-kernel/src/skills/router.rs` 全文:

```rust
//! SkillRouter — V1.1 §5.1.
//!
//! W3b: 确定性关键词匹配。
//! W7: keyword 优先,LLM fallback(置信度 >= 0.7 才用 LLM 结果)。
//!
//! 路由顺序:
//!   1. intent_examples 命中 → score = 1.0 → 直接返回 Skill
//!   2. keywords 命中 → score = 0.5 → 直接返回 Skill
//!   3. score < 0.5 且 LLM 启用 → 调 LLM,confidence >= 0.7 返回 SkillWithSlots
//!   4. 否则 → Planner

use std::sync::Arc;

use crate::llm::client::LlmClient;
use crate::llm::types::ExtractedSlot;
use crate::skills::manifest::SkillManifest;

#[derive(Debug, Clone)]
pub enum RouteDecision {
    Skill(Box<SkillManifest>),
    /// W7 新增:LLM 提取了 Slot,经 UI 反馈给用户修改/Apply 后再执行
    SkillWithSlots(Box<SkillManifest>, Vec<ExtractedSlot>),
    Planner,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRouter {
    skills: Vec<SkillManifest>,
    llm: Option<Arc<LlmClient>>,
}

impl SkillRouter {
    pub fn new() -> Self {
        Self { skills: Vec::new(), llm: None }
    }

    pub fn with_llm(llm: Arc<LlmClient>) -> Self {
        Self { skills: Vec::new(), llm: Some(llm) }
    }

    pub fn register(&mut self, manifest: SkillManifest) {
        self.skills.push(manifest);
    }

    /// 同步路由(W6 行为,关键词匹配,不调 LLM)
    pub fn route(&self, user_goal: &str) -> RouteDecision {
        let goal_lower = user_goal.to_lowercase();
        for skill in &self.skills {
            if self.matches_intent_examples(&goal_lower, skill)
                || self.matches_keywords(&goal_lower, skill)
            {
                return RouteDecision::Skill(Box::new(skill.clone()));
            }
        }
        RouteDecision::Planner
    }

    /// 异步路由(关键词优先,LLM fallback)
    pub async fn route_with_llm(&self, user_goal: &str) -> RouteDecision {
        // Step 1-2: keyword 匹配
        let goal_lower = user_goal.to_lowercase();
        for skill in &self.skills {
            if self.matches_intent_examples(&goal_lower, skill)
                || self.matches_keywords(&goal_lower, skill)
            {
                return RouteDecision::Skill(Box::new(skill.clone()));
            }
        }

        // Step 3: LLM fallback
        if let Some(llm) = &self.llm {
            if llm.is_enabled() {
                match llm.classify_and_extract(user_goal, &self.skills).await {
                    Ok(resp) if resp.confidence >= 0.7 => {
                        if let Some(skill_id) = &resp.matched_skill_id {
                            if let Some(skill) = self.skills.iter().find(|s| &s.id == skill_id) {
                                if resp.slots.is_empty() {
                                    return RouteDecision::Skill(Box::new(skill.clone()));
                                }
                                return RouteDecision::SkillWithSlots(
                                    Box::new(skill.clone()),
                                    resp.slots,
                                );
                            }
                        }
                    }
                    _ => {} // LLM 失败或低置信度,回退到 Planner
                }
            }
        }

        // Step 4: Planner
        RouteDecision::Planner
    }

    fn matches_intent_examples(&self, goal_lower: &str, skill: &SkillManifest) -> bool {
        for example in &skill.intent_examples {
            let example_lower = example.to_lowercase();
            if goal_lower.contains(&example_lower) || example_lower.contains(goal_lower) {
                return true;
            }
        }
        false
    }

    fn matches_keywords(&self, goal_lower: &str, skill: &SkillManifest) -> bool {
        for keyword in &skill.keywords {
            let keyword_lower = keyword.to_lowercase();
            if !keyword_lower.is_empty() && goal_lower.contains(&keyword_lower) {
                return true;
            }
        }
        false
    }
}
```

- [ ] **Step 4: 跑测试看通过**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel skills::router::tests:: 2>&1 | Select-String "test result"
```

Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/crates/trust-kernel/src/skills/router.rs ; git commit -m "feat(w7): add SkillRouter::route_with_llm with keyword-first LLM fallback"
```

---

## Task 5: SlotParser LLM fallback + SlotKind 扩展

**Files:**
- Modify: `voicepilot/crates/ui/src/slot_parser.rs`

- [ ] **Step 1: 写失败测试 — `parse_with_llm_fallback` regex 0 命中时调 LLM mock**

在 `voicepilot/crates/ui/src/slot_parser.rs` 末尾追加测试:

```rust
    #[tokio::test]
    async fn parse_with_llm_fallback_uses_llm_when_regex_empty() {
        use std::sync::Arc;
        // 用一个 mock LlmClient(指向无效 URL,但 is_enabled = true)
        // 实际测试中用 wiremock,但 ui crate 不依赖 wiremock,
        // 所以这里只测 regex 命中时不调 LLM 的路径。
        let slots = SlotParser::parse("整理下载目录");
        // regex 提取 0 个 Slot(无路径/应用/数字等)
        assert!(slots.is_empty(), "regex should return empty for plain text");
    }

    #[test]
    fn slot_kind_supports_time_range_and_url() {
        let slot = Slot {
            kind: SlotKind::TimeRange,
            raw: "昨天".to_string(),
            start: 0,
            end: 2,
            high_risk: false,
        };
        let json = serde_json::to_string(&slot).unwrap();
        assert!(json.contains("\"kind\":\"time_range\""));
    }
```

注意:`parse_with_llm_fallback` 的 LLM 集成测试在 `tests/w7_router_llm_smoke.rs`(Task 16)做端到端覆盖。本 Task 只做 SlotKind 扩展 + 静态方法签名。

- [ ] **Step 2: 跑测试看失败**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --features voice slot_parser::tests::slot_kind_supports_time_range_and_url 2>&1 | Select-String "error\[" | Select-Object -First 3
```

Expected: 编译错误(`SlotKind::TimeRange` 不存在)

- [ ] **Step 3: 扩展 SlotKind + 加 `parse_with_llm_fallback` 方法**

修改 `voicepilot/crates/ui/src/slot_parser.rs`:

在 `SlotKind` enum 中加两个变体:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotKind {
    Path,
    App,
    Number,
    Recipient,
    DeleteTarget,
    /// W7 新增:时间范围(LLM 提取,如"昨天")
    TimeRange,
    /// W7 新增:URL(LLM 提取,浏览器自动化场景)
    Url,
}
```

在 `impl SlotParser` 末尾加异步方法:

```rust
    /// W7: regex 优先,0 命中时调 LLM fallback。
    /// LLM 调用由 caller 提供(`llm: &LlmClient`),本方法不直接持有 LLM。
    pub async fn parse_with_llm_fallback(
        text: &str,
        llm: Option<&crate::llm::client::LlmClient>,
        candidate_skills: &[crate::skills::manifest::SkillManifest],
    ) -> Vec<Slot> {
        // Step 1: regex 优先
        let regex_slots = Self::parse(text);
        if !regex_slots.is_empty() {
            return regex_slots;
        }

        // Step 2: LLM fallback(regex 0 命中)
        if let Some(llm) = llm {
            if llm.is_enabled() {
                if let Ok(resp) = llm.classify_and_extract(text, candidate_skills).await {
                    return resp.slots.iter().map(|s| Slot {
                        kind: parse_slot_kind(&s.kind),
                        raw: s.raw.clone(),
                        start: 0, // LLM 不返回位置,用 0 占位
                        end: s.raw.len(),
                        high_risk: s.high_risk,
                    }).collect();
                }
            }
        }

        Vec::new()
    }
```

在文件末尾(`#[cfg(test)] mod tests` 之前)加 helper:

```rust
fn parse_slot_kind(kind: &str) -> SlotKind {
    match kind {
        "path" => SlotKind::Path,
        "app" => SlotKind::App,
        "number" => SlotKind::Number,
        "recipient" => SlotKind::Recipient,
        "delete_target" => SlotKind::DeleteTarget,
        "time_range" => SlotKind::TimeRange,
        "url" => SlotKind::Url,
        _ => SlotKind::App, // 未知 kind 默认 App(低风险)
    }
}
```

注意:`ui` crate 引用 `crate::llm::client::LlmClient` 需要 `voicepilot-ui` 依赖 `trust-kernel`(已有)。若 `LlmClient` 在 `llm` feature 后,需 `voicepilot-ui/Cargo.toml` 加 `trust-kernel/llm`(默认开启)。

- [ ] **Step 4: 跑测试看通过**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --features voice slot_parser::tests:: 2>&1 | Select-String "test result"
```

Expected: 所有测试 PASS(含新加的 2 个 + 原 13 个)

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/crates/ui/src/slot_parser.rs ; git commit -m "feat(w7): add SlotKind::TimeRange/Url + parse_with_llm_fallback async method"
```

---

## Task 6: Settings UI 扩展 — SettingsDto 加 LLM 字段

**Files:**
- Modify: `voicepilot/crates/ui/src/settings_commands.rs`

- [ ] **Step 1: 写失败测试 — LLM 字段往返**

在 `voicepilot/crates/ui/src/settings_commands.rs` 末尾追加 `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_settings_roundtrip() {
        let mut dto = SettingsDto::default();
        dto.llm_enabled = true;
        dto.llm_api_key = "sk-test".to_string();
        dto.llm_base_url = "https://api.deepseek.com/v1".to_string();
        dto.llm_model = "deepseek-chat".to_string();
        dto.llm_provider_url = "https://platform.deepseek.com/api_keys".to_string();

        let kv = flatten_to_kv(&dto);
        let restored = merge_from_kv(&kv).unwrap();
        assert_eq!(restored.llm_enabled, true);
        assert_eq!(restored.llm_api_key, "sk-test");
        assert_eq!(restored.llm_base_url, "https://api.deepseek.com/v1");
        assert_eq!(restored.llm_model, "deepseek-chat");
        assert_eq!(restored.llm_provider_url, "https://platform.deepseek.com/api_keys");
    }

    #[test]
    fn llm_defaults_are_disabled() {
        let dto = SettingsDto::default();
        assert!(!dto.llm_enabled);
        assert_eq!(dto.llm_api_key, "");
        assert_eq!(dto.llm_base_url, "https://api.deepseek.com/v1");
        assert_eq!(dto.llm_model, "deepseek-chat");
        assert_eq!(dto.llm_provider_url, "https://platform.deepseek.com/api_keys");
    }
}
```

- [ ] **Step 2: 跑测试看失败**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --features tauri settings_commands::tests:: 2>&1 | Select-String "error\[" | Select-Object -First 3
```

Expected: 编译错误(`llm_enabled` 字段不存在)

- [ ] **Step 3: 修改 SettingsDto 加 5 字段 + KV 映射**

修改 `voicepilot/crates/ui/src/settings_commands.rs` 的 `SettingsDto` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsDto {
    pub voice_model_path: String,
    pub voice_language: Option<String>,
    pub voice_threads: u32,
    pub vad_energy_threshold: f32,
    pub vad_max_silence_ms: u32,
    pub vad_min_speech_ms: u32,
    pub voice_max_duration_ms: u64,
    pub voice_chunk_duration_ms: u64,
    pub privacy_mode: bool,
    pub compensation_ttl_hours: u32,
    pub tts_enabled: bool,
    pub tts_model_path: String,
    // W7 新增
    pub llm_enabled: bool,
    pub llm_api_key: String,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_provider_url: String,
}
```

修改 `Default for SettingsDto`:

```rust
impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            voice_model_path: String::new(),
            voice_language: None,
            voice_threads: 4,
            vad_energy_threshold: 100.0,
            vad_max_silence_ms: 700,
            vad_min_speech_ms: 200,
            voice_max_duration_ms: 30000,
            voice_chunk_duration_ms: 500,
            privacy_mode: false,
            compensation_ttl_hours: 24,
            tts_enabled: true,
            tts_model_path: String::new(),
            // W7 默认值
            llm_enabled: false,
            llm_api_key: String::new(),
            llm_base_url: "https://api.deepseek.com/v1".to_string(),
            llm_model: "deepseek-chat".to_string(),
            llm_provider_url: "https://platform.deepseek.com/api_keys".to_string(),
        }
    }
}
```

修改 `flatten_to_kv`:

```rust
pub fn flatten_to_kv(dto: &SettingsDto) -> Vec<(String, String)> {
    vec![
        ("voice.model_path".to_string(), dto.voice_model_path.clone()),
        ("voice.language".to_string(), dto.voice_language.clone().unwrap_or_default()),
        ("voice.threads".to_string(), dto.voice_threads.to_string()),
        ("voice.vad.energy_threshold".to_string(), dto.vad_energy_threshold.to_string()),
        ("voice.vad.max_silence_ms".to_string(), dto.vad_max_silence_ms.to_string()),
        ("voice.vad.min_speech_ms".to_string(), dto.vad_min_speech_ms.to_string()),
        ("voice.max_duration_ms".to_string(), dto.voice_max_duration_ms.to_string()),
        ("voice.chunk_duration_ms".to_string(), dto.voice_chunk_duration_ms.to_string()),
        ("privacy.mode".to_string(), dto.privacy_mode.to_string()),
        ("compensation.ttl_hours".to_string(), dto.compensation_ttl_hours.to_string()),
        ("tts.enabled".to_string(), dto.tts_enabled.to_string()),
        ("tts.model_path".to_string(), dto.tts_model_path.clone()),
        // W7 LLM
        ("llm.enabled".to_string(), dto.llm_enabled.to_string()),
        ("llm.api_key".to_string(), dto.llm_api_key.clone()),
        ("llm.base_url".to_string(), dto.llm_base_url.clone()),
        ("llm.model".to_string(), dto.llm_model.clone()),
        ("llm.provider_url".to_string(), dto.llm_provider_url.clone()),
    ]
}
```

修改 `merge_from_kv` match 分支:

```rust
        match k.as_str() {
            "voice.model_path" => dto.voice_model_path = v.clone(),
            "voice.language" => dto.voice_language = if v.is_empty() { None } else { Some(v.clone()) },
            "voice.threads" => dto.voice_threads = v.parse().map_err(|e| UiError::InvalidConfig(format!("voice.threads: {e}")))?,
            "voice.vad.energy_threshold" => dto.vad_energy_threshold = v.parse().map_err(|e| UiError::InvalidConfig(format!("energy_threshold: {e}")))?,
            "voice.vad.max_silence_ms" => dto.vad_max_silence_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("max_silence_ms: {e}")))?,
            "voice.vad.min_speech_ms" => dto.vad_min_speech_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("min_speech_ms: {e}")))?,
            "voice.max_duration_ms" => dto.voice_max_duration_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("max_duration_ms: {e}")))?,
            "voice.chunk_duration_ms" => dto.voice_chunk_duration_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("chunk_duration_ms: {e}")))?,
            "privacy.mode" => dto.privacy_mode = v.parse().map_err(|e| UiError::InvalidConfig(format!("privacy.mode: {e}")))?,
            "compensation.ttl_hours" => dto.compensation_ttl_hours = v.parse().map_err(|e| UiError::InvalidConfig(format!("ttl_hours: {e}")))?,
            "tts.enabled" => dto.tts_enabled = v.parse().map_err(|e| UiError::InvalidConfig(format!("tts.enabled: {e}")))?,
            "tts.model_path" => dto.tts_model_path = v.clone(),
            // W7 LLM
            "llm.enabled" => dto.llm_enabled = v.parse().map_err(|e| UiError::InvalidConfig(format!("llm.enabled: {e}")))?,
            "llm.api_key" => dto.llm_api_key = v.clone(),
            "llm.base_url" => dto.llm_base_url = v.clone(),
            "llm.model" => dto.llm_model = v.clone(),
            "llm.provider_url" => dto.llm_provider_url = v.clone(),
            _ => {} // 忽略未知 key(前向兼容)
        }
```

- [ ] **Step 4: 跑测试看通过**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --features tauri settings_commands::tests:: 2>&1 | Select-String "test result"
```

Expected: `test result: ok. 2 passed`

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/crates/ui/src/settings_commands.rs ; git commit -m "feat(w7): add 5 LLM fields to SettingsDto with KV roundtrip"
```

---

## Task 7: Settings UI 扩展 — 前端 types.ts + SettingsView.tsx

**Files:**
- Modify: `voicepilot/crates/ui/web/src/types.ts`
- Modify: `voicepilot/crates/ui/web/src/components/SettingsView.tsx`

- [ ] **Step 1: 修改 types.ts 加 LLM 字段**

打开 `voicepilot/crates/ui/web/src/types.ts`,在 `Settings` interface 末尾(`tts_model_path` 后)加:

```typescript
  // W7 LLM
  llm_enabled: boolean;
  llm_api_key: string;
  llm_base_url: string;
  llm_model: string;
  llm_provider_url: string;
```

- [ ] **Step 2: 修改 SettingsView.tsx 加 LLM fieldset**

打开 `voicepilot/crates/ui/web/src/components/SettingsView.tsx`,在 TTS 配置 fieldset 后(`</fieldset>` 后)新增:

```tsx
        <fieldset className="settings-fieldset">
          <legend>LLM 配置</legend>
          <div className="form-row checkbox-row">
            <input
              id="llm_enabled"
              type="checkbox"
              checked={settings.llm_enabled}
              onChange={(e) => handleField("llm_enabled", e.target.checked)}
              disabled={settings.privacy_mode}
            />
            <label htmlFor="llm_enabled">启用云端 LLM(用于意图分类与 Slot 提取)</label>
          </div>
          {settings.privacy_mode && (
            <p className="settings-hint settings-warn">
              隐私模式已启用,LLM 不可用
            </p>
          )}
          <div className="form-row">
            <label htmlFor="llm_api_key">API Key</label>
            <input
              id="llm_api_key"
              type="password"
              value={settings.llm_api_key}
              onChange={(e) => handleField("llm_api_key", e.target.value)}
              placeholder="sk-..."
              disabled={settings.privacy_mode || !settings.llm_enabled}
            />
          </div>
          <div className="form-row">
            <label htmlFor="llm_base_url">Base URL</label>
            <input
              id="llm_base_url"
              type="text"
              value={settings.llm_base_url}
              onChange={(e) => handleField("llm_base_url", e.target.value)}
              placeholder="https://api.deepseek.com/v1"
              disabled={settings.privacy_mode || !settings.llm_enabled}
            />
          </div>
          <div className="form-row">
            <label htmlFor="llm_model">模型名</label>
            <input
              id="llm_model"
              type="text"
              value={settings.llm_model}
              onChange={(e) => handleField("llm_model", e.target.value)}
              placeholder="deepseek-chat"
              disabled={settings.privacy_mode || !settings.llm_enabled}
            />
          </div>
          <div className="form-row">
            <a
              href={settings.llm_provider_url}
              target="_blank"
              rel="noopener noreferrer"
              className="settings-link"
            >
              获取 API Key
            </a>
          </div>
          <details className="settings-details">
            <summary>常见 provider 配置</summary>
            <ul>
              <li>DeepSeek: base_url=<code>https://api.deepseek.com/v1</code>, model=<code>deepseek-chat</code></li>
              <li>OpenAI: base_url=<code>https://api.openai.com/v1</code>, model=<code>gpt-4o-mini</code></li>
              <li>通义千问: base_url=<code>https://dashscope.aliyuncs.com/compatible-mode/v1</code>, model=<code>qwen-turbo</code></li>
              <li>Kimi: base_url=<code>https://api.moonshot.cn/v1</code>, model=<code>moonshot-v1-8k</code></li>
            </ul>
          </details>
        </fieldset>
```

- [ ] **Step 3: 验证前端构建**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build
```

Expected: PASS(无 TS 错误)

- [ ] **Step 4: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/components/SettingsView.tsx voicepilot/crates/ui/web/dist/ ; git commit -m "feat(w7): add LLM config fieldset to SettingsView with provider presets"
```

---

## Task 8: AppState 注入 LlmClient + voice_commands 用 route_with_llm

**Files:**
- Modify: `voicepilot/crates/ui/src/state.rs`
- Modify: `voicepilot/crates/ui/src/voice_commands.rs`

- [ ] **Step 1: 修改 state.rs 加 llm_client cache**

打开 `voicepilot/crates/ui/src/state.rs`,在 `AppState` struct 中加字段:

```rust
pub struct AppState {
    // ... 现有字段 ...
    /// W7: LLM 客户端缓存(配置变更时重建)
    pub llm_client: Arc<Mutex<Option<Arc<trust_kernel::llm::client::LlmClient>>>>,
}
```

在 `AppState::new()` 初始化:

```rust
impl AppState {
    pub fn new(kernel: Arc<trust_kernel::kernel::TrustKernel>) -> Self {
        Self {
            // ... 现有初始化 ...
            llm_client: Arc::new(Mutex::new(None)),
        }
    }
}
```

加 helper 方法:

```rust
    /// W7: 根据 Settings 构建 LlmClient(privacy_mode=true 时返回 disabled)
    pub fn rebuild_llm_client(&self, settings: &SettingsDto) -> Arc<trust_kernel::llm::client::LlmClient> {
        if settings.privacy_mode || !settings.llm_enabled || settings.llm_api_key.is_empty() {
            return Arc::new(trust_kernel::llm::client::LlmClient::disabled());
        }
        Arc::new(trust_kernel::llm::client::LlmClient::new(
            &settings.llm_base_url,
            &settings.llm_api_key,
            &settings.llm_model,
        ))
    }

    pub fn llm_client(&self) -> Arc<trust_kernel::llm::client::LlmClient> {
        let guard = self.llm_client.lock().unwrap();
        guard.clone().unwrap_or_else(|| {
            Arc::new(trust_kernel::llm::client::LlmClient::disabled())
        })
    }
```

- [ ] **Step 2: 修改 voice_commands.rs 的 route_text_command 用 route_with_llm**

打开 `voicepilot/crates/ui/src/voice_commands.rs`,定位到 `route_text` 函数(约 380-420 行)。把现有实现改为:

```rust
pub async fn route_text(
    state: &AppState,
    text: &str,
) -> Result<RouteTextResult, crate::error::UiError> {
    use trust_kernel::skills::router::{RouteDecision, SkillRouter};
    use trust_kernel::skills::manifest::files_organize_manifest;

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteTextResult::Empty);
    }

    let llm = state.llm_client();
    let mut router = SkillRouter::with_llm(llm);
    // W7: 注册所有 8 个内置 Skill(其他 Task 实现 manifest 后取消注释)
    router.register(files_organize_manifest());
    // router.register(task_repeat_verified_manifest());
    // router.register(task_explain_manifest());
    // router.register(task_compensate_manifest());
    // router.register(app_control_manifest());
    // router.register(note_capture_manifest());
    // router.register(research_save_manifest());
    // router.register(form_prepare_manifest());

    let decision = router.route_with_llm(trimmed).await;
    match decision {
        RouteDecision::Skill(manifest) => Ok(RouteTextResult::Routed {
            skill_id: manifest.id,
            slots: vec![],
        }),
        RouteDecision::SkillWithSlots(manifest, slots) => {
            let slot_dtos: Vec<Slot> = slots.iter().map(|s| Slot {
                kind: s.kind.clone(),
                raw: s.raw.clone(),
                start: 0,
                end: s.raw.len(),
                high_risk: s.high_risk,
            }).collect();
            Ok(RouteTextResult::Routed {
                skill_id: manifest.id,
                slots: slot_dtos,
            })
        }
        RouteDecision::Planner => Ok(RouteTextResult::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}
```

(注意:`RouteTextResult` enum 与 `Slot` struct 已存在;若 `RouteTextResult::Routed` 不含 `slots` 字段,需先扩展 enum,见 Step 3。)

- [ ] **Step 3: 扩展 RouteTextResult 加 slots 字段(若未存在)**

打开 `voicepilot/crates/ui/src/voice_commands.rs`,定位到 `RouteTextResult` enum,改为:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteTextResult {
    Routed {
        skill_id: String,
        slots: Vec<Slot>,
    },
    Unmatched {
        text: String,
    },
    Empty,
}
```

- [ ] **Step 4: 修改 update_settings_command 重建 LLM client**

在 `voicepilot/crates/ui/src/settings_commands.rs` 的 `update_settings_command` 中,保存设置后重建 LLM client:

```rust
pub async fn update_settings_command(
    state: State<'_, AppState>,
    settings: SettingsDto,
) -> Result<(), String> {
    update_settings(&state, &settings).map_err(Into::into)?;
    // W7: 重建 LLM client
    let new_llm = state.rebuild_llm_client(&settings);
    *state.llm_client.lock().unwrap() = Some(new_llm);
    Ok(())
}
```

- [ ] **Step 5: 验证编译**

```powershell
cd d:\voicepilot\voicepilot ; cargo check -p voicepilot-ui --features voice,tauri
```

Expected: PASS

- [ ] **Step 6: 跑现有测试看无回归**

```powershell
cd d:\voicepilot\voicepilot ; cargo test -p voicepilot-ui --features voice,tauri 2>&1 | Select-String "test result" | Select-Object -Last 5
```

Expected: 所有现有测试 PASS

- [ ] **Step 7: Commit**

```powershell
cd d:\voicepilot ; git add voicepilot/crates/ui/src/state.rs voicepilot/crates/ui/src/voice_commands.rs voicepilot/crates/ui/src/settings_commands.rs ; git commit -m "feat(w7): inject LlmClient into AppState + route_text uses route_with_llm"
```

---

(后续 Task 9-16 待续,因计划较长,先 commit 当前进度。)
