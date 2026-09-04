# 实时快照先行规划 + 实时交互记忆 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 启动即恢复云端 LLM；规划前先拿带新鲜度门禁的实时快照并注入 LLM 上下文；语音每轮封轮记忆，下一轮可回忆上文；classify 结果可复用；人一开口 TTS 即停。

**Architecture:** 启动时从持久化配置重建 serving-applied client；新增 `RealtimeSnapshot`（快照→门禁→上下文块）随 `PlannerInput` 进入规划；新增 `turns` 表做情景记忆（写摘要不写原文音频，privacy 下跳过）；classify 命中进 SQLite 缓存（模型+schema版本+归一化文本为键）；语音 `listen→转写→快照→路由→封轮` 同函数内串行完成，不改 trait 签名、不改 CLI；打断走前端已有的 `cancel_tts_command` 链路。

**Tech Stack:** Rust (tokio current-thread 已有、rusqlite、serde、serde_json、sha2、chrono 均已有)、SQLite migration 幂等模式、wiremock（已有 dev-dep）、chrono（ui 已有）、vitest（web 已有）。

**Non-goals（本次不做）:** 屏幕/活动窗口等新传感器；流式 TTS/流式 LLM；向量/embedding 召回；偏好 KV 写入；Dag→UI 映射修复；LLM 错误透出到 UI（另起计划）。

---

## File map（改动清单，锁死）

- Modify: `voicepilot/crates/trust-kernel/src/planner.rs` —— `RealtimeSnapshot` 类型 + `PlannerInput.snapshot` + 新鲜度门禁 + 上下文注入 + inline 单测。
- Modify: `voicepilot/crates/trust-kernel/tests/planner_pipeline.rs` —— 3 处 `PlannerInput` 构造加 `snapshot: None`。
- Create: `voicepilot/crates/trust-kernel/tests/realtime_snapshot.rs` —— 快照上下文注入 + 过期拒绝的集成测试（wiremock）。
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs` —— 私有 `route_via_pipeline` 加参 + 新增 `route_text_with_snapshot`，旧入口行为不变。
- Create: `voicepilot/crates/trust-kernel/src/migrations/009_turns.sql` —— `turns` 表。
- Modify: `voicepilot/crates/trust-kernel/src/db.rs` —— 注册 `MIGRATION_009`。
- Create: `voicepilot/crates/trust-kernel/src/turns.rs` —— `TurnRecord` + 存/取/清 + inline 单测。
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs` —— 3 个薄包装（插在 `set_llm_client` 之后）。
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs` —— 加 `pub mod turns;`。
- Modify: `voicepilot/crates/ui/src/voice_commands.rs` —— `listen` 重排为转写→快照→路由→封轮；新增 `route_with_snapshot` / `build_snapshot` / `seal_turn` 私有函数；imports 加 3 行。
- Modify: `voicepilot/crates/ui/src/app.rs` —— 启动即调 `startup_rebuild_llm`（Task 0）。
- Modify: `voicepilot/crates/trust-kernel/src/llm/client.rs` —— prompt 按 id 排序 + `ROUTE_TOOL_SCHEMA_VERSION` + inline 单测。
- Create: `voicepilot/crates/trust-kernel/src/migrations/010_llm_route_cache.sql` —— classify 缓存表。
- Create: `voicepilot/crates/trust-kernel/src/llm_cache.rs` —— 归一化/key/存取/TTL清理 + inline 单测。
- Create: `voicepilot/crates/ui/web/src/components/ttsPlayback.ts` + `components/__tests__/ttsPlayback.test.ts` —— 打断 helper + 单测。
- Modify: `voicepilot/crates/ui/web/src/components/MainView.tsx` —— 录音开始先停 TTS；`handleStopTts` 复用 helper。

---

### Task 0: 启动重建 LLM client（根因，先做）

**Files:**

- Modify: `voicepilot/crates/ui/src/settings_commands.rs` —— 新增 `startup_rebuild_llm`（`#[cfg(feature = "llm")]`）。
- Modify: `voicepilot/crates/ui/src/app.rs` —— `run()` 内 `AppState::new` 之后调用。
- Modify: `voicepilot/crates/ui/tests/w7_settings_llm_smoke.rs` —— 加启动等价测试（复用既有 `new_test_state` / `default_update` helper）。

- [ ] **Step 1: 先写失败测试（追加到 `w7_settings_llm_smoke.rs` 末尾）**

```rust
/// 启动路径等价物：只持久化不 rebuild（模拟“配置过但重启后”），
/// 启动重建入口必须让路由重新可用。
#[test]
fn startup_rebuild_from_persisted_settings_enables_llm() {
    use voicepilot_ui::settings_commands::startup_rebuild_llm;
    let state = new_test_state().expect("state");
    let mut update = default_update();
    update.llm_enabled = true;
    update.llm_api_key = Some("sk-test-startup".to_string());
    update_settings(&state, &update).expect("persist");
    assert!(state.kernel.llm_client().is_none());
    startup_rebuild_llm(&state);
    let client = state.kernel.llm_client().expect("client after startup rebuild");
    assert!(client.is_enabled());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p voicepilot-ui --features tauri,llm --test w7_settings_llm_smoke startup_rebuild`（cwd `D:/voicepilot/voicepilot`）
Expected: FAIL（`startup_rebuild_llm` 不存在，编译失败即红灯）

- [ ] **Step 3: 在 `settings_commands.rs` 加函数（放在 `rebuild_llm_client` 调用处附近，`#[cfg(feature = "llm")]`）**

```rust
/// 启动重建：从持久化 KV + SecretStore 重建 serving-applied client。
/// 此前只在设置保存时 rebuild，重启后 LLM 回到 None —— 这是“配了云端还不会聊天”的根因之一。
/// 无配置的新用户走默认分支（disabled），失败不阻断启动由调用方决定。
#[cfg(feature = "llm")]
pub fn startup_rebuild_llm(state: &AppState) {
    let view = get_settings(state).unwrap_or_default();
    state.rebuild_llm_client(&view);
}
```

（`get_settings` 本模块已有；`SettingsView: Default` 已有；`rebuild_llm_client` 内部已处理 privacy/无 key → disabled。）

- [ ] **Step 4: `app.rs` 的 `run()` 内 `let state = AppState::new(kernel);` 之后插入**

```rust
    // Task 0: 启动即从持久化配置重建 LLM client（此前只在设置保存时重建，重启后失效）。
    #[cfg(feature = "llm")]
    crate::settings_commands::startup_rebuild_llm(&state);
```

- [ ] **Step 5: 跑测试**

Run: `cargo test -p voicepilot-ui --features tauri,llm --test w7_settings_llm_smoke`（cwd `D:/voicepilot/voicepilot`）
Expected: 全 PASS（含新增 1 个）

- [ ] **Step 6: Commit**

```bash
git add voicepilot/crates/ui/src/settings_commands.rs voicepilot/crates/ui/src/app.rs voicepilot/crates/ui/tests/w7_settings_llm_smoke.rs
git commit -m "fix: rebuild serving LLM client from persisted settings at startup"
```

---

### Task 1: RealtimeSnapshot 类型 + 新鲜度门禁 + 上下文块

**Files:**

- Modify: `voicepilot/crates/trust-kernel/src/planner.rs`

- [ ] **Step 1: 加类型与常量（`PlannerInput` 定义之前插入）**

```rust
/// 快照 TTL：超过此时长未规划即视为过期，调用方必须重采。
pub const SNAPSHOT_TTL_MS: u64 = 10_000;
/// 注入 LLM 的上下文块上限（字符数；中文按字计，天然保守）。
pub const CONTEXT_BUDGET_CHARS: usize = 1600;

#[derive(Debug, Clone)]
pub struct SnapshotVoice {
    /// "speech_ended" | "timeout" | "no_speech" | "text"
    pub outcome_kind: &'static str,
    pub stopped_by_vad: bool,
    pub sample_count: usize,
    /// "silero" | "energy"
    pub vad_backend: &'static str,
    /// 首个 voiced chunk 距快照时刻的毫秒数（无语音为 None）
    pub voice_started_ago_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotMemory {
    /// 已渲染摘要 "用户：… → …"，调用方保证每条 ≤200 字符、最多 3 条
    pub prev_turns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RealtimeSnapshot {
    pub taken_at: std::time::SystemTime,
    pub transcript_chars: usize,
    pub voice: SnapshotVoice,
    pub memory: SnapshotMemory,
    /// 采样时刻的 privacy_mode（仅审计，不决定注入；注入与否由调用路径保证）
    pub privacy_mode: bool,
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

impl RealtimeSnapshot {
    pub fn is_fresh_at(&self, now: std::time::SystemTime) -> bool {
        now.duration_since(self.taken_at)
            .map(|d| d.as_millis() as u64 <= SNAPSHOT_TTL_MS)
            .unwrap_or(false)
    }

    /// 渲染注入 LLM user 消息前缀的上下文块。结尾不带用户输入，
    /// 调用方拼 `format!("{}\n{}", snap.context_block(), trimmed)`。
    pub fn context_block(&self) -> String {
        let mut s = String::from(
            "【实时上下文，仅供理解意图；禁止引用其中的路径、数字、专有名词作为槽位值】\n",
        );
        s.push_str(&format!(
            "- 本轮转写长度：{}字；语音后端：{}；结束方式：{}；样本数：{}\n",
            self.transcript_chars,
            self.voice.vad_backend,
            self.voice.outcome_kind,
            self.voice.sample_count
        ));
        if !self.memory.prev_turns.is_empty() {
            s.push_str(&format!("- 上文：{}\n", self.memory.prev_turns.join(" ｜ ")));
        }
        truncate_chars(&s, CONTEXT_BUDGET_CHARS)
    }
}
```

同时文件头加 `use std::time::SystemTime;`（`Instant` 那行保留，`SystemTime` 供签名与门禁用）。

- [ ] **Step 2: 加 inline 单测（文件末尾追加 `#[cfg(test)] mod snapshot_tests`）**

```rust
#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn fresh_snapshot() -> RealtimeSnapshot {
        RealtimeSnapshot {
            taken_at: SystemTime::now(),
            transcript_chars: 12,
            voice: SnapshotVoice {
                outcome_kind: "speech_ended",
                stopped_by_vad: true,
                sample_count: 80000,
                vad_backend: "silero",
                voice_started_ago_ms: Some(1200),
            },
            memory: SnapshotMemory {
                prev_turns: vec!["用户：打开记事本 → routed:quick.app_control".to_string()],
            },
            privacy_mode: false,
        }
    }

    #[test]
    fn fresh_snapshot_passes_and_stale_fails() {
        let snap = fresh_snapshot();
        assert!(snap.is_fresh_at(SystemTime::now()));
        let old = RealtimeSnapshot {
            taken_at: SystemTime::now() - Duration::from_millis(SNAPSHOT_TTL_MS + 1000),
            ..fresh_snapshot()
        };
        assert!(!old.is_fresh_at(SystemTime::now()));
    }

    #[test]
    fn context_block_contains_turns_and_respects_budget() {
        let block = fresh_snapshot().context_block();
        assert!(block.contains("上文"));
        assert!(block.contains("打开记事本"));
        assert!(block.contains("禁止引用"));
        assert!(block.chars().count() <= CONTEXT_BUDGET_CHARS);
    }

    #[test]
    fn context_block_empty_memory_has_no_prev_line() {
        let mut snap = fresh_snapshot();
        snap.memory.prev_turns.clear();
        assert!(!snap.context_block().contains("上文"));
    }
}
```

- [ ] **Step 3: 运行单测**

Run: `cargo test -p trust-kernel --features voice --lib planner::snapshot_tests`（cwd `D:/voicepilot/voicepilot`）
Expected: `3 passed; 0 failed`

- [ ] **Step 4: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/planner.rs
git commit -m "feat: add RealtimeSnapshot with freshness gate and context block"
```

---

### Task 2: PlannerInput.snapshot + 门禁 + 上下文注入

**Files:**

- Modify: `voicepilot/crates/trust-kernel/src/planner.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/planner_pipeline.rs`
- Create: `voicepilot/crates/trust-kernel/tests/realtime_snapshot.rs`

- [ ] **Step 1: `PlannerInput` 加字段**

```rust
/// PlannerPipeline 的输入。
#[derive(Debug, Clone)]
pub struct PlannerInput {
    pub text: String,
    pub source: PlannerSource,
    /// 实时快照；语音路径 Some（须新鲜），文本路径 None（行为与旧版一致）。
    pub snapshot: Option<RealtimeSnapshot>,
}
```

- [ ] **Step 2: `plan()` 加新鲜度门禁（空检查之后、取候选之前插入）**

```rust
if let Some(snap) = &input.snapshot {
    if !snap.is_fresh_at(std::time::SystemTime::now()) {
        return Err(crate::error::KernelError::Skill(
            "stale realtime snapshot: re-sense before planning".to_string(),
        ));
    }
}
```

- [ ] **Step 3: 上下文注入 LLM 两次调用（两个 cfg 的 `plan_with_llm` 都加 `snapshot` 参数）**

llm 版签名改为：

```rust
async fn plan_with_llm(
    &self,
    trimmed: &str,
    manifests: &[crate::skills::manifest::SkillManifest],
    snapshot: Option<&RealtimeSnapshot>,
) -> Result<(PlanResult, PlannerTrace)> {
```

函数体内、LLM 可用性检查通过后、classify 之前插入：

```rust
// 快照上下文只进 LLM（关键词路由仍用原文，避免污染匹配）。
let llm_text = match snapshot {
    Some(s) => format!("{}\n{}", s.context_block(), trimmed),
    None => trimmed.to_string(),
};
```

并把本函数内两处调用的 `trimmed` 换成 `&llm_text`：`llm.classify_and_extract(&llm_text, manifests)` 与 `llm.decompose_to_dag_traced(&llm_text, manifests, &user_slots)`。注意 `trimmed` 仍用于 `resolve_candidate` 后的返回与 Unmatched 回退（保持原文）。

调用点改为 `self.plan_with_llm(trimmed, &manifests, input.snapshot.as_ref()).await`。

非 llm 版签名同步加参（体不变，参数名前加 `_`）：

```rust
async fn plan_with_llm(
    &self,
    trimmed: &str,
    _manifests: &[crate::skills::manifest::SkillManifest],
    _snapshot: Option<&RealtimeSnapshot>,
) -> Result<(PlanResult, PlannerTrace)> {
```

- [ ] **Step 4: 更新既有集成测试的 3 处构造（`tests/planner_pipeline.rs` 内全部 `PlannerInput {`）**

每处在 `source: PlannerSource::Text,`（或 `::Voice`）行后加一行 `snapshot: None,`。共 3 处（keyword 单测 1 处 + text/voice 对比单测 2 处）。

- [ ] **Step 5: 新建集成测试 `tests/realtime_snapshot.rs`（全文）**

```rust
#![cfg(feature = "llm")]

//! 快照门禁 + 上下文注入的集成测试（wiremock 挡掉真实云端）。

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::json;
use trust_kernel::extensions::registry::ExtensionCatalog;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::planner::{
    PlanResult, PlannerInput, PlannerPipeline, PlannerSource, RealtimeSnapshot, SnapshotMemory,
    SnapshotVoice,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fresh_snapshot() -> RealtimeSnapshot {
    RealtimeSnapshot {
        taken_at: SystemTime::now(),
        transcript_chars: 6,
        voice: SnapshotVoice {
            outcome_kind: "speech_ended",
            stopped_by_vad: true,
            sample_count: 80000,
            vad_backend: "energy",
            voice_started_ago_ms: Some(900),
        },
        memory: SnapshotMemory {
            prev_turns: vec!["用户：打开记事本 → routed:quick.app_control".to_string()],
        },
        privacy_mode: false,
    }
}

async fn pipeline_with_mock_llm(server: &MockServer) -> (Arc<TrustKernel>, PlannerPipeline) {
    let kernel = Arc::new(TrustKernel::open_in_memory().expect("open in-memory kernel"));
    kernel.set_llm_client(Some(Arc::new(LlmClient::new(
        &server.uri(),
        "sk-test",
        "test-model",
    ))));
    let catalog = ExtensionCatalog::load(&kernel).expect("load extension catalog");
    let pipeline = PlannerPipeline::new(kernel.clone(), catalog.snapshot());
    (kernel, pipeline)
}

fn mock_classify_hit(server: &MockServer) -> Mock {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "route_skill",
                            "arguments": "{\"matched_skill_id\":\"files.organize\",\"confidence\":0.9,\"slots\":[],\"reasoning\":\"test\"}"
                        }
                    }]
                }
            }]
        })))
}

#[tokio::test]
async fn snapshot_context_is_sent_to_llm() {
    let server = MockServer::start().await;
    mock_classify_hit(&server).mount(&server).await;
    let (_kernel, pipeline) = pipeline_with_mock_llm(&server).await;

    let (plan, _trace) = pipeline
        .plan(PlannerInput {
            text: "还是刚才那个".to_string(),
            source: PlannerSource::Voice,
            snapshot: Some(fresh_snapshot()),
        })
        .await
        .expect("snapshot plan");
    assert!(matches!(plan, PlanResult::Skill { .. }), "got {plan:?}");

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 1, "classify hit must stop before decompose");
    let body = String::from_utf8_lossy(requests[0].body.as_ref()).into_owned();
    assert!(body.contains("上文"), "context block missing: {body}");
    assert!(body.contains("打开记事本"), "prev turn missing: {body}");
    assert!(body.contains("还是刚才那个"), "user text missing: {body}");
}

#[tokio::test]
async fn stale_snapshot_is_rejected_without_llm_call() {
    let server = MockServer::start().await;
    mock_classify_hit(&server).mount(&server).await;
    let (_kernel, pipeline) = pipeline_with_mock_llm(&server).await;

    let mut snap = fresh_snapshot();
    snap.taken_at = SystemTime::now() - Duration::from_secs(60);
    let err = pipeline
        .plan(PlannerInput {
            text: "整理下载目录".to_string(),
            source: PlannerSource::Voice,
            snapshot: Some(snap),
        })
        .await
        .expect_err("stale snapshot must fail");
    assert!(err.to_string().contains("stale realtime snapshot"), "got {err}");
    assert!(
        server.received_requests().await.expect("requests").is_empty(),
        "stale snapshot must not call the LLM"
    );
}
```

- [ ] **Step 6: 运行测试**

Run: `cargo test -p trust-kernel --features voice,llm --test planner_pipeline`（cwd `D:/voicepilot/voicepilot`）
Expected: 既有 2 个单测 PASS（`snapshot: None` 下行为不变）

Run: `cargo test -p trust-kernel --features voice,llm --test realtime_snapshot`
Expected: `2 passed; 0 failed`

Run: `cargo test -p trust-kernel --features voice,llm --lib planner::`
Expected: Task 1 的 3 个 + 本文件无新增失败

- [ ] **Step 7: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/planner.rs voicepilot/crates/trust-kernel/tests/planner_pipeline.rs voicepilot/crates/trust-kernel/tests/realtime_snapshot.rs
git commit -m "feat: gate planning on fresh RealtimeSnapshot and inject context into LLM calls"
```

---

### Task 3: route_text_with_snapshot（路由桥加餐不加价）

**Files:**

- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`

- [ ] **Step 1: 私有 `route_via_pipeline` 加 `snapshot` 参数，两个旧调用点传 `None`**

```rust
async fn route_via_pipeline(
    kernel: Arc<TrustKernel>,
    text: String,
    source: PlannerSource,
    snapshot: Option<crate::planner::RealtimeSnapshot>,
) -> Result<RouteOutcome> {
    let pipeline = PlannerPipeline::new(kernel.clone(), kernel.extension_snapshot());
    let (plan, trace) = pipeline
        .plan(PlannerInput {
            text: text.clone(),
            source,
            snapshot,
        })
        .await?;
```

`route_text` 内调用改为 `route_via_pipeline(kernel, text, PlannerSource::Text, None).await`；`route_text_with_dag` 内调用改为 `route_via_pipeline(kernel.clone_arc(), trimmed.to_string(), PlannerSource::Voice, None).await`。CLI 零改动。

- [ ] **Step 2: 新增公开入口（放在 `route_text_with_dag` 之后）**

```rust
/// 带实时快照的语音路由入口（UI voice 路径用）。
/// 快照由调用方现采现传；`None` 时行为等价 `route_text_with_dag`。
#[cfg(feature = "voice")]
pub async fn route_text_with_snapshot(
    kernel: &TrustKernel,
    text: &str,
    snapshot: Option<crate::planner::RealtimeSnapshot>,
) -> crate::error::Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    route_via_pipeline(
        kernel.clone_arc(),
        trimmed.to_string(),
        PlannerSource::Voice,
        snapshot,
    )
    .await
}
```

- [ ] **Step 3: 编译**

Run: `cargo check -p trust-kernel --features voice,llm`（cwd `D:/voicepilot/voicepilot`）
Expected: `Finished` 无 error（既有 `unused_mut` warning 允许存在，不新增 warning）

- [ ] **Step 4: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/voice/router_bridge.rs
git commit -m "feat: add route_text_with_snapshot, keep legacy route entries unchanged"
```

---

### Task 4: turns 情景记忆持久化（表 + 读写 + 清理）

**Files:**

- Create: `voicepilot/crates/trust-kernel/src/migrations/009_turns.sql`
- Modify: `voicepilot/crates/trust-kernel/src/db.rs`
- Create: `voicepilot/crates/trust-kernel/src/turns.rs`
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`

- [ ] **Step 1: 写 migration（幂等，与 008 同风格）**

```sql
-- turns 表：语音/文本每轮封轮记忆（只存摘要与结果，不存音频与原文长文本）。

CREATE TABLE IF NOT EXISTS turns (
    turn_id TEXT PRIMARY KEY,
    started_at_ms INTEGER NOT NULL,
    source TEXT NOT NULL,
    transcript TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    plan_id TEXT NOT NULL DEFAULT '',
    latency_ms INTEGER NOT NULL DEFAULT 0,
    sensitive INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_turns_started ON turns(started_at_ms);
```

- [ ] **Step 2: 注册（`db.rs` 照 008 抄两行）**

```rust
const MIGRATION_009: &str = include_str!("migrations/009_turns.sql");
```

```rust
conn.execute_batch(MIGRATION_009)?;
```

分别紧随 `MIGRATION_008` 的 `include_str!` 行与 `execute_batch(MIGRATION_008)?;` 行之后。另在 `db.rs` 既有 `migration_008_*` 单测旁加：

```rust
#[test]
fn migration_009_creates_turns_table() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turns'",
            [],
            |row| row.get(0),
        )
        .expect("turns table must exist after migration 009");
    assert_eq!(count, 1);
}
```

（`Connection` 与 `run_migrations` 在该测试模块已有 import；若无则照 008 单测的 import 抄。）

- [ ] **Step 3: 新建 `turns.rs`（全文，default-gated，仿 voice_latency 纯 DB 风格）**

```rust
//! turns 情景记忆：每轮封轮一条摘要（结果可回忆，原始音频永不落盘）。

use crate::error::Result;
use rusqlite::{params, Connection};

/// outcome 取值：`routed:<skill_id>` | `dag` | `unmatched` | `empty` | `nospeech` | `error`。
/// transcript 截断到 500 字符后存入；privacy_mode 下调用方不得调用本模块。
#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub turn_id: String,
    pub started_at_ms: i64,
    pub source: String,
    pub transcript: String,
    pub outcome: String,
    pub plan_id: String,
    pub latency_ms: i64,
    pub sensitive: bool,
}

pub fn record_turn(conn: &Connection, rec: &TurnRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO turns (turn_id, started_at_ms, source, transcript, outcome, plan_id, latency_ms, sensitive)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(turn_id) DO UPDATE SET
           started_at_ms = excluded.started_at_ms, source = excluded.source,
           transcript = excluded.transcript, outcome = excluded.outcome,
           plan_id = excluded.plan_id, latency_ms = excluded.latency_ms,
           sensitive = excluded.sensitive",
        params![
            rec.turn_id,
            rec.started_at_ms,
            rec.source,
            rec.transcript,
            rec.outcome,
            rec.plan_id,
            rec.latency_ms,
            rec.sensitive as i32
        ],
    )?;
    Ok(())
}

/// 取最近 N 轮（时间倒序）。回忆时调用方自行反转为正序渲染。
pub fn recent_turns(conn: &Connection, limit: usize) -> Result<Vec<TurnRecord>> {
    let mut stmt = conn.prepare(
        "SELECT turn_id, started_at_ms, source, transcript, outcome, plan_id, latency_ms, sensitive
         FROM turns ORDER BY started_at_ms DESC, rowid DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(TurnRecord {
            turn_id: row.get(0)?,
            started_at_ms: row.get(1)?,
            source: row.get(2)?,
            transcript: row.get(3)?,
            outcome: row.get(4)?,
            plan_id: row.get(5)?,
            latency_ms: row.get(6)?,
            sensitive: row.get::<_, i32>(7)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
}

/// TTL 清理：删除早于 now_ms - days*86400*1000 的轮次，返回删除行数。
pub fn prune_turns_older_than(conn: &Connection, days: u32, now_ms: i64) -> Result<usize> {
    let cutoff = now_ms - days as i64 * 86_400_000;
    let deleted = conn.execute("DELETE FROM turns WHERE started_at_ms < ?1", params![cutoff])?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;

    fn migrated() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn sample(id: &str, started_at_ms: i64) -> TurnRecord {
        TurnRecord {
            turn_id: id.to_string(),
            started_at_ms,
            source: "voice".to_string(),
            transcript: "打开记事本".to_string(),
            outcome: "routed:quick.app_control".to_string(),
            plan_id: String::new(),
            latency_ms: 320,
            sensitive: false,
        }
    }

    #[test]
    fn record_and_recent_return_newest_first() {
        let conn = migrated();
        record_turn(&conn, &sample("t1", 1000)).unwrap();
        record_turn(&conn, &sample("t2", 2000)).unwrap();
        let rows = recent_turns(&conn, 3).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].turn_id, "t2");
        assert_eq!(rows[1].turn_id, "t1");
    }

    #[test]
    fn prune_removes_only_expired_turns() {
        let conn = migrated();
        record_turn(&conn, &sample("old", 1000)).unwrap();
        record_turn(&conn, &sample("new", 1000 + 31 * 86_400_000)).unwrap();
        let deleted = prune_turns_older_than(&conn, 30, 1000 + 31 * 86_400_000).unwrap();
        assert_eq!(deleted, 1);
        let rows = recent_turns(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].turn_id, "new");
    }
}
```

- [ ] **Step 4: `kernel.rs` 加 3 个薄包装（插在 `set_llm_client` 方法的闭合 brace 之后、`// ===== Wave 3 Task 3.1` 注释之前）**

```rust
/// 记录一轮封轮摘要（turns 情景记忆）。privacy_mode 下调用方不得调用。
pub fn record_turn(&self, rec: &crate::turns::TurnRecord) -> Result<()> {
    let conn = self.conn();
    crate::turns::record_turn(&conn, rec)
}

/// 取最近 N 轮（时间倒序），供快照回忆渲染。
pub fn recent_turns(&self, limit: usize) -> Result<Vec<crate::turns::TurnRecord>> {
    let conn = self.conn();
    crate::turns::recent_turns(&conn, limit)
}

/// 按天数清理过期轮次，返回删除行数（封轮时 piggyback 调用，默认 30 天）。
pub fn prune_turns_older_than(&self, days: u32, now_ms: i64) -> Result<usize> {
    let conn = self.conn();
    crate::turns::prune_turns_older_than(&conn, days, now_ms)
}
```

（`Result` 指 kernel.rs 已有的 `crate::error::Result` 别名；`self.conn()` 与 `set_llm_api_key` 内用法一致。）

- [ ] **Step 5: `lib.rs` 注册模块（`pub mod voice_latency;` 附近，照抄注释风格）**

```rust
// turns 情景记忆表读写（纯 DB 操作，default-gated，与 voice_latency 同级）。
pub mod turns;
```

- [ ] **Step 6: 运行测试**

Run: `cargo test -p trust-kernel --features voice turns::`（cwd `D:/voicepilot/voicepilot`）
Expected: `record_and_recent…` + `prune_removes…` PASS

Run: `cargo test -p trust-kernel --features voice --lib db::`
Expected: 既有 migration 单测 + 新增 `migration_009…` PASS

- [ ] **Step 7: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/migrations/009_turns.sql voicepilot/crates/trust-kernel/src/db.rs voicepilot/crates/trust-kernel/src/turns.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/src/lib.rs
git commit -m "feat: add turns episodic memory table with TTL prune"
```

---

### Task 5: 语音 listen 内串起“转写→快照→路由→封轮”

**Files:**

- Modify: `voicepilot/crates/ui/src/voice_commands.rs`

- [ ] **Step 1: imports 加 3 行（紧随现有 `trust_kernel::voice::vad` 那行之后）**

```rust
use trust_kernel::voice::listener::ListenTimings;
use trust_kernel::planner::{RealtimeSnapshot, SnapshotMemory, SnapshotVoice};
use trust_kernel::turns::TurnRecord;
```

- [ ] **Step 2: `listen()` 改为“先拿 outcome+timings，再收尾”（替换现有 outcome 匹配段）**

现有 `listen()` 内：

```rust
let outcome = match partial_cb.as_ref() {
    Some(cb) => listener.listen_with_cancel_and_partial(cancel, Some(cb))?,
    None => listener.listen_with_cancel(cancel)?,
};

match outcome {
```

改为：

```rust
let (outcome, timings) = match partial_cb.as_ref() {
    Some(cb) => listener.listen_with_cancel_partial_and_timings(cancel, Some(cb))?,
    None => {
        let outcome = listener.listen_with_cancel(cancel)?;
        (outcome, ListenTimings::default())
    }
};

self.finish_listen(outcome, timings)
```

并把原 `match outcome { SpeechEnded… / NoSpeech / Timeout… }` 整段搬进新私有函数（体不变，只改签名与结尾）：

```rust
/// 收尾：转写 → 快照 → 路由 → 封轮。NoSpeech 不封轮（无信息量）。
fn finish_listen(
    &self,
    outcome: ListenOutcome,
    timings: ListenTimings,
) -> VoiceResult<VoiceListenOutcome> {
    match outcome {
        ListenOutcome::SpeechEnded { samples } => {
            let transcription = self.transcribe(&samples)?;
            let snapshot = self.build_snapshot(
                &transcription,
                &timings,
                samples.len(),
                "speech_ended",
                true,
            );
            let route_outcome = self.route_with_snapshot(&transcription, snapshot.as_ref());
            self.seal_turn(&transcription, &route_outcome, &timings, "voice");
            Ok(VoiceListenOutcome::Success {
                transcription,
                route_outcome,
                stopped_by_vad: true,
            })
        }
        ListenOutcome::NoSpeech => Ok(VoiceListenOutcome::NoSpeech),
        ListenOutcome::Timeout { samples } => {
            let transcription = if samples.is_empty() {
                None
            } else {
                self.transcribe(&samples).ok()
            };
            let (route_outcome, sealed) = match &transcription {
                Some(t) => {
                    let snapshot =
                        self.build_snapshot(t, &timings, samples.len(), "timeout", false);
                    let r = self.route_with_snapshot(t, snapshot.as_ref());
                    self.seal_turn(t, &r, &timings, "voice");
                    (r, true)
                }
                None => (RouteTextResult::Empty, false),
            };
            let _ = sealed;
            Ok(VoiceListenOutcome::Timeout {
                transcription,
                route_outcome,
            })
        }
    }
}
```

- [ ] **Step 3: 新增 `route_with_snapshot`（放在现有 `route()` 之后，原 `route()` 不动）**

```rust
/// 带快照的路由（voice 路径用）。快照为 None 时等价 `route()`。
fn route_with_snapshot(
    &self,
    text: &str,
    snapshot: Option<&RealtimeSnapshot>,
) -> RouteTextResult {
    use trust_kernel::voice::router_bridge::route_text_with_snapshot;
    let kernel = self.kernel.clone();
    let text_for_closure = text.to_string();
    let snapshot = snapshot.cloned();
    let outcome = block_on_planner(async move {
        route_text_with_snapshot(&kernel, &text_for_closure, snapshot).await
    });
    match outcome {
        Ok(RouteOutcome::Routed { skill_id }) => RouteTextResult::Routed {
            skill_id,
            slots: vec![],
        },
        Ok(RouteOutcome::Unmatched { text }) => RouteTextResult::Unmatched { text },
        Ok(RouteOutcome::Empty) => RouteTextResult::Empty,
        #[cfg(feature = "llm")]
        Ok(RouteOutcome::DagPlan(_)) => RouteTextResult::Unmatched {
            text: text.to_string(),
        },
        Err(_) => RouteTextResult::Empty,
    }
}
```

（映射臂与现有 `route()` 逐行一致；Dag 防御性映射保持不变，本计划不碰。）

- [ ] **Step 4: 新增 `build_snapshot` + `seal_turn`（放在 `route_with_snapshot` 之后）**

```rust
/// 现采快照：final 转写 + timings + VAD 后端探测 + 最近 3 轮回忆。
/// privacy_mode 下返回 None（无快照、无记忆、无注入）。
fn build_snapshot(
    &self,
    transcription: &str,
    timings: &ListenTimings,
    sample_count: usize,
    outcome_kind: &'static str,
    stopped_by_vad: bool,
) -> Option<RealtimeSnapshot> {
    if self.kernel.privacy_mode() {
        return None;
    }
    let prev_turns = self
        .kernel
        .recent_turns(3)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .map(|t| {
            let short: String = t.transcript.chars().take(200).collect();
            format!("用户：{} → {}", short, t.outcome)
        })
        .collect();
    let vad_backend = if VadDetector::new(VadConfig::default()).is_silero() {
        "silero"
    } else {
        "energy"
    };
    let now = std::time::SystemTime::now();
    Some(RealtimeSnapshot {
        taken_at: now,
        transcript_chars: transcription.chars().count(),
        voice: SnapshotVoice {
            outcome_kind,
            stopped_by_vad,
            sample_count,
            vad_backend,
            voice_started_ago_ms: timings.voice_started_at.and_then(|t0| {
                now.duration_since(t0).ok().map(|d| d.as_millis() as u64)
            }),
        },
        memory: SnapshotMemory { prev_turns },
        privacy_mode: false,
    })
}

/// 封轮：摘要落 turns 表并 piggyback 30 天 TTL。privacy 下跳过；失败只记日志不阻断返回。
fn seal_turn(
    &self,
    transcription: &str,
    route_outcome: &RouteTextResult,
    timings: &ListenTimings,
    source: &str,
) {
    if self.kernel.privacy_mode() {
        return;
    }
    let outcome = match route_outcome {
        RouteTextResult::Routed { skill_id, .. } => format!("routed:{skill_id}"),
        RouteTextResult::Unmatched { .. } => "unmatched".to_string(),
        RouteTextResult::Empty => "empty".to_string(),
    };
    let latency_ms = match (timings.voice_started_at, timings.first_partial_at) {
        (Some(a), Some(b)) => b
            .duration_since(a)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        _ => 0,
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    let transcript: String = transcription.chars().take(500).collect();
    let rec = TurnRecord {
        turn_id: format!(
            "turn-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(now_ms * 1_000_000)
        ),
        started_at_ms: now_ms,
        source: source.to_string(),
        transcript,
        outcome,
        plan_id: String::new(),
        latency_ms,
        sensitive: false,
    };
    if let Err(e) = self.kernel.record_turn(&rec) {
        tracing::warn!("seal_turn record failed: {e}");
    }
    let _ = self.kernel.prune_turns_older_than(30, now_ms);
}
```

（`tracing` 在本文件是否已 import 未核实：若编译报 `unresolved import`，改用既有风格的 `eprintln!("[voice] seal_turn record failed: {e}")`——本文件已有 `eprintln!("[voice] listen begin…")` 先例。）

- [ ] **Step 5: 编译 + 存量测试**

Run: `cargo check -p voicepilot-ui --features voice,custom-protocol`（cwd `D:/voicepilot/voicepilot`）
Expected: `Finished` 无 error

Run: `cargo test -p voicepilot-ui --features voice,custom-protocol voice`（cwd `D:/voicepilot/voicepilot`）
Expected: 既有 voice 单测全 PASS（trait 签名未变，mock 无需改）

- [ ] **Step 6: Commit**

```bash
git add voicepilot/crates/ui/src/voice_commands.rs
git commit -m "feat: sense-before-route with snapshot and seal each voice turn"
```

---

### Task 6: prompt 前缀稳定化 + classify 结果缓存

**Files:**

- Modify: `voicepilot/crates/trust-kernel/src/llm/client.rs` —— 双 builder 按 id 排序 + `ROUTE_TOOL_SCHEMA_VERSION` + inline 单测。
- Create: `voicepilot/crates/trust-kernel/src/migrations/010_llm_route_cache.sql`。
- Modify: `voicepilot/crates/trust-kernel/src/db.rs` —— 注册 `MIGRATION_010` + 单测。
- Create: `voicepilot/crates/trust-kernel/src/llm_cache.rs` —— 归一化/key/存取/TTL清理 + inline 单测。
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs` —— 3 个薄包装。
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs` —— 加 `pub mod llm_cache;`。
- Modify: `voicepilot/crates/trust-kernel/src/planner.rs` —— 查/存缓存（只缓存高置信 Skill 命中）。
- Modify: `voicepilot/crates/trust-kernel/tests/realtime_snapshot.rs` —— 缓存命中集成测试。

- [ ] **Step 1: 双 builder 按 id 排序（`build_system_prompt` 与 `build_decompose_system_prompt` 内各自改同一处）**

两处把 `for skill in skills {` 改为：

```rust
// Task 6:候选按 id 排序后再拼 prompt —— 同一候选集无论注册顺序如何，
// prompt 前缀字节一致，provider 侧前缀缓存才能命中。
let mut ordered: Vec<&SkillManifest> = skills.iter().collect();
ordered.sort_by(|a, b| a.id.cmp(&b.id));
for skill in ordered {
```

- [ ] **Step 2: schema 版本常量（放在 `build_tool_schema` 定义之前）**

```rust
/// classify 工具 schema 版本：改 `build_tool_schema` 必 bump，否则 `llm_route_cache` 串味。
pub const ROUTE_TOOL_SCHEMA_VERSION: u32 = 1;
```

- [ ] **Step 3: prompt 顺序单测（`client.rs` 既有 `#[cfg(test)]` 模块内追加，需 `files_organize_manifest` 已在该模块 import）**

```rust
#[test]
fn system_prompts_are_stable_regardless_of_manifest_order() {
    let mut second = files_organize_manifest();
    second.id = "zzz.second".to_string();
    let first = files_organize_manifest();
    let client = LlmClient::new("https://x", "sk-test", "m");
    let p1 = client.build_system_prompt(&[second.clone(), first.clone()]);
    let p2 = client.build_system_prompt(&[first, second]);
    assert_eq!(p1, p2);
    assert!(p1.find("files.organize").unwrap() < p1.find("zzz.second").unwrap());
}
```

- [ ] **Step 4: 写 migration `010_llm_route_cache.sql`（幂等）**

```sql
-- llm_route_cache 表：classify 高置信 Skill 命中的结果缓存。
-- key = sha256(模型 + schema版本 + 归一化文本)，含上下文时天然带上文；
-- 只存 Skill 命中（Dag/Unmatched 不进缓存，计划与候选清单会变）；
-- privacy 路径到不了写入点（上游已返回 Unmatched），无需额外门禁。

CREATE TABLE IF NOT EXISTS llm_route_cache (
    cache_key TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL,
    slots_json TEXT NOT NULL DEFAULT '[]',
    confidence REAL NOT NULL,
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_route_cache_expiry ON llm_route_cache(expires_at_ms);
```

- [ ] **Step 5: `db.rs` 注册（照 Task 4 的 009 抄两行）+ 单测照抄改名**

```rust
const MIGRATION_010: &str = include_str!("migrations/010_llm_route_cache.sql");
```

```rust
conn.execute_batch(MIGRATION_010)?;
```

```rust
#[test]
fn migration_010_creates_llm_route_cache_table() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='llm_route_cache'",
            [],
            |row| row.get(0),
        )
        .expect("llm_route_cache table must exist after migration 010");
    assert_eq!(count, 1);
}
```

- [ ] **Step 6: 新建 `llm_cache.rs`（全文，default-gated）**

```rust
//! classify 结果缓存：同模型 + 同 schema 版本 + 归一化文本 → 高置信 Skill 命中。
//!
//! 不调模型最省：重复指令（如“打开记事本”）一次命中省掉整包云端往返。
//! 归一化只做空白折叠 + 小写（中文不受影响）；槽位原样存取，不参与 key。

use crate::error::Result;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

/// 缓存 TTL：24h。候选清单/模型变化由 key 隔离（snapshot_id 不进 key，
/// 因为 skill 上下线会改变同一文本的正确路由 —— 误命中比 miss 更贵，
/// 跨清单复用一律不做）。
pub const ROUTE_CACHE_TTL_DAYS: u32 = 1;

#[derive(Debug, Clone)]
pub struct CachedRoute {
    pub skill_id: String,
    pub slots_json: String,
    pub confidence: f32,
}

pub fn normalize_text(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub fn route_cache_key(model: &str, schema_version: u32, text: &str) -> String {
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0u8]);
    h.update(schema_version.to_le_bytes());
    h.update([0u8]);
    h.update(normalize_text(text).as_bytes());
    format!("{:x}", h.finalize())
}

/// 命中且未过期才返回 Some；过期行视为 miss（由 prune 异步清理）。
pub fn lookup(conn: &Connection, key: &str, now_ms: i64) -> Result<Option<CachedRoute>> {
    let mut stmt = conn.prepare(
        "SELECT skill_id, slots_json, confidence FROM llm_route_cache
         WHERE cache_key = ?1 AND expires_at_ms > ?2",
    )?;
    let row = stmt
        .query_row(params![key, now_ms], |row| {
            Ok(CachedRoute {
                skill_id: row.get(0)?,
                slots_json: row.get(1)?,
                confidence: row.get(2)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn store(
    conn: &Connection,
    key: &str,
    skill_id: &str,
    slots_json: &str,
    confidence: f32,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO llm_route_cache
         (cache_key, skill_id, slots_json, confidence, created_at_ms, expires_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(cache_key) DO UPDATE SET
           skill_id = excluded.skill_id, slots_json = excluded.slots_json,
           confidence = excluded.confidence, created_at_ms = excluded.created_at_ms,
           expires_at_ms = excluded.expires_at_ms",
        params![
            key,
            skill_id,
            slots_json,
            confidence,
            now_ms,
            now_ms + ROUTE_CACHE_TTL_DAYS as i64 * 86_400_000
        ],
    )?;
    Ok(())
}

pub fn prune_expired(conn: &Connection, now_ms: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM llm_route_cache WHERE expires_at_ms <= ?1",
        params![now_ms],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;

    fn migrated() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn normalize_ignores_case_and_whitespace() {
        assert_eq!(normalize_text("  打开 记事本\n"), normalize_text("打开记事本"));
    }

    #[test]
    fn store_lookup_roundtrip_and_expiry() {
        let conn = migrated();
        let key = route_cache_key("m", 1, "打开记事本");
        assert!(lookup(&conn, &key, 1000).unwrap().is_none());
        store(&conn, &key, "quick.app_control", "[]", 0.9, 1000).unwrap();
        let hit = lookup(&conn, &key, 2000).unwrap().expect("hit");
        assert_eq!(hit.skill_id, "quick.app_control");
        assert!(lookup(&conn, &key, 1000 + 86_400_000 + 1).unwrap().is_none());
    }

    #[test]
    fn prune_expired_removes_only_expired_rows() {
        let conn = migrated();
        store(&conn, "k-old", "s", "[]", 0.9, 1000).unwrap();
        store(&conn, "k-new", "s", "[]", 0.9, 1000 + 86_400_000).unwrap();
        let deleted = prune_expired(&conn, 1000 + 86_400_000 + 1).unwrap();
        assert_eq!(deleted, 1);
    }
}
```

（`optional()` 需要 `rusqlite::OptionalExtension`：若编译报找不到，给文件头加 `use rusqlite::OptionalExtension;` —— `config_repo.rs` 的 `get` 已用同写法，照抄其 import。）

- [ ] **Step 7: `kernel.rs` 加 3 个薄包装（紧随 Task 4 的 turns 包装之后）+ `lib.rs` 加 `pub mod llm_cache;`**

```rust
/// classify 缓存查询（Task 6）。key 含模型+schema版本+归一化文本；过期由 lookup 过滤。
pub fn lookup_route_cache(&self, key: &str, now_ms: i64) -> Result<Option<crate::llm_cache::CachedRoute>> {
    let conn = self.conn();
    crate::llm_cache::lookup(&conn, key, now_ms)
}

/// 存一次高置信 Skill 命中（Task 6）。只在 planner 命中路径调用。
pub fn record_route_cache(&self, key: &str, skill_id: &str, slots_json: &str, confidence: f32, now_ms: i64) -> Result<()> {
    let conn = self.conn();
    crate::llm_cache::store(&conn, key, skill_id, slots_json, confidence, now_ms)
}

/// 清理过期缓存行（封轮/命中路径 piggyback 调用，失败不阻断）。
pub fn prune_route_cache_expired(&self, now_ms: i64) -> Result<usize> {
    let conn = self.conn();
    crate::llm_cache::prune_expired(&conn, now_ms)
}
```

- [ ] **Step 8: `planner.rs` 接入（llm 版 `plan_with_llm` 内，Task 2 的 `llm_text` 构造之后、`let started` 之前插入查，命中分支内加存）**

```rust
// Task 6: classify 缓存。命中且候选仍有效 → 零 LLM 开销直接返回。
let now_ms = chrono::Utc::now().timestamp_millis();
let cache_key = crate::llm_cache::route_cache_key(
    llm.model(),
    crate::llm::client::ROUTE_TOOL_SCHEMA_VERSION,
    &llm_text,
);
let cached_hit = {
    let conn = self.kernel.conn();
    crate::llm_cache::lookup(&conn, &cache_key, now_ms).unwrap_or(None)
};
if let Some(hit) = cached_hit {
    if self.snapshot.resolve_candidate(&hit.skill_id).is_some() {
        let slots = serde_json::from_str(&hit.slots_json).unwrap_or_default();
        return Ok((
            PlanResult::Skill {
                extension_id: hit.skill_id,
                slots,
            },
            trace,
        ));
    }
    // 候选已变（如 skill 下线）：当 miss 继续走 LLM。
}
```

并把 Task 2 已有 Skill 返回分支内、 `return Ok((` 之前加存：

```rust
let slots_json =
    serde_json::to_string(&resp.slots).unwrap_or_else(|_| "[]".to_string());
let conn = self.kernel.conn();
let _ = crate::llm_cache::store(
    &conn,
    &cache_key,
    skill_id,
    &slots_json,
    resp.confidence,
    now_ms,
);
```

（`kernel.conn()` 的 guard 用法与 `config_repo().list(&conn)` 一致；`chrono` / `serde_json` 用全路径，无需新 import；`trace.used_llm` 保持只在真实调用后置 true —— 缓存命中路径不碰 trace。）

- [ ] **Step 9: 集成测试追加到 `tests/realtime_snapshot.rs` 末尾**

```rust
#[tokio::test]
async fn repeated_query_hits_classify_cache() {
    let server = MockServer::start().await;
    mock_classify_hit(&server).mount(&server).await;
    let (_kernel, pipeline) = pipeline_with_mock_llm(&server).await;
    for _ in 0..2 {
        let (plan, _) = pipeline
            .plan(PlannerInput {
                text: "还是刚才那个".to_string(),
                source: PlannerSource::Voice,
                snapshot: None,
            })
            .await
            .expect("cached plan");
        assert!(matches!(plan, PlanResult::Skill { .. }));
    }
    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 1, "second identical query must hit cache, got {}", requests.len());
}
```

- [ ] **Step 10: 运行测试**

Run: `cargo test -p trust-kernel --features voice,llm --lib llm:: llm_cache:: db::`（cwd `D:/voicepilot/voicepilot`）
Expected: prompt 顺序单测 + 缓存 3 单测 + migration_010 单测 PASS

Run: `cargo test -p trust-kernel --features voice,llm --test realtime_snapshot`（cwd `D:/voicepilot/voicepilot`）
Expected: 3 passed（含新增缓存命中）

- [ ] **Step 11: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/llm/client.rs voicepilot/crates/trust-kernel/src/migrations/010_llm_route_cache.sql voicepilot/crates/trust-kernel/src/db.rs voicepilot/crates/trust-kernel/src/llm_cache.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/src/planner.rs voicepilot/crates/trust-kernel/tests/realtime_snapshot.rs
git commit -m "feat: stabilize prompt prefix and cache classify hits"
```

---

### Task 7: VAD 联动 TTS 打断（人一开口音箱就停）

**Files:**

- Create: `voicepilot/crates/ui/web/src/components/ttsPlayback.ts`
- Create: `voicepilot/crates/ui/web/src/components/__tests__/ttsPlayback.test.ts`
- Modify: `voicepilot/crates/ui/web/src/components/MainView.tsx` —— 录音开始先停 TTS；`handleStopTts` 复用 helper。

背景（已核实，无需再探）：TTS 实际由前端 `<audio>` 播放（`ttsPlaying` + `audioRef`），后端 `cancel_tts_command` 只负责停合成+清 cooldown；打断只能由前端发起。`handleStopTts`（326–332 行）与 `onVoiceListen` 重入守卫（203–206 行）是两个精确锚点。

- [ ] **Step 1: 新建 `ttsPlayback.ts`（全文，无 tauri 依赖，单测零风险）**

```ts
/** 停止 TTS 播放并取消后端合成。自动打断与手动停止共用。 */
export function stopTtsPlayback(
  audio: { pause: () => void } | null,
  setTtsPlaying: (v: boolean) => void,
  cancelTts: () => Promise<unknown>,
): void {
  if (audio) audio.pause();
  cancelTts().catch(() => {});
  setTtsPlaying(false);
}
```

- [ ] **Step 2: 新建单测 `__tests__/ttsPlayback.test.ts`（全文）**

```ts
import { describe, expect, it, vi } from "vitest";
import { stopTtsPlayback } from "../ttsPlayback";

describe("stopTtsPlayback", () => {
  it("pauses audio, cancels backend synthesis, and clears playing flag", () => {
    const pause = vi.fn();
    const setTtsPlaying = vi.fn();
    let cancelCalled = false;
    stopTtsPlayback({ pause }, setTtsPlaying, async () => {
      cancelCalled = true;
    });
    expect(pause).toHaveBeenCalledTimes(1);
    expect(cancelCalled).toBe(true);
    expect(setTtsPlaying).toHaveBeenCalledWith(false);
  });

  it("works when nothing is playing", () => {
    const setTtsPlaying = vi.fn();
    stopTtsPlayback(null, setTtsPlaying, async () => {});
    expect(setTtsPlaying).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 3: `MainView.tsx` 加 import（`import { convertFileSrc } ...` 行之后）**

```tsx
import { stopTtsPlayback } from "./ttsPlayback";
```

- [ ] **Step 4: `onVoiceListen` 重入守卫后先打断（锚点 203–206 行）**

```tsx
    // 3-2:重入守卫——PTT start 事件与 mic 按钮可能相邻触发,只放一个进后端
    if (listeningRef.current) return;
    listeningRef.current = true;
    // Task 7 打断：新一轮录音开始即停掉正在播的 TTS（人一开口音箱就停）。
    // mic 按钮与 PTT 都走本函数，单漏斗全覆盖。
    if (audioRef.current || ttsPlaying) {
      stopTtsPlayback(audioRef.current, setTtsPlaying, invokeCancelTts);
      audioRef.current = null;
    }
    setListening(true);
```

- [ ] **Step 5: `handleStopTts` 复用 helper（锚点 326–332 行，行为等价：取消改为 best-effort 静默，卸载路径本就如此）**

```tsx
  const handleStopTts = (): void => {
    stopTtsPlayback(audioRef.current, setTtsPlaying, invokeCancelTts);
    audioRef.current = null;
```

（其后 `};` 保留不动。）

- [ ] **Step 6: 运行测试与类型检查**（cwd `D:/voicepilot/voicepilot/crates/ui/web`）

Run: `npm test -- ttsPlayback`
Expected: `2 passed`

Run: `npx tsc --noEmit`
Expected: 零 error

- [ ] **Step 7: Commit**

```bash
git add voicepilot/crates/ui/web/src/components/ttsPlayback.ts voicepilot/crates/ui/web/src/components/__tests__/ttsPlayback.test.ts voicepilot/crates/ui/web/src/components/MainView.tsx
git commit -m "feat: stop TTS playback when a new voice listen starts"
```

---

### Task 8: 验证（回归 + 手工端到端）

**Files:** 无（只跑命令、记录结果）

- [ ] **Step 1: 全量相关回归**

Run: `cargo test -p trust-kernel --features voice,llm --lib planner:: turns:: db:: llm_cache:: llm::`（cwd `D:/voicepilot/voicepilot`）
Expected: Task 1/2/4/6 的全部单测 PASS，0 failed

Run: `cargo test -p trust-kernel --features voice,llm --test planner_pipeline --test realtime_snapshot --test w8_plan4_router_bridge_dag`（cwd `D:/voicepilot/voicepilot`）
Expected: 全 PASS（含旧有 2 个 planner 单测，证明 `snapshot: None` 行为不变；`realtime_snapshot` 3 个含缓存命中）

Run: `cargo test -p voicepilot-ui --features tauri,llm --test w7_settings_llm_smoke`（cwd `D:/voicepilot/voicepilot`）
Expected: 全 PASS（含 Task 0 启动重建单测）

Run: `npm test -- ttsPlayback`（cwd `D:/voicepilot/voicepilot/crates/ui/web`）
Expected: `2 passed`

Run: `npx tsc --noEmit`（cwd `D:/voicepilot/voicepilot/crates/ui/web`）
Expected: 零 error

Run: `cargo build -p voicepilot-ui --features voice,custom-protocol`（cwd `D:/voicepilot/voicepilot`）
Expected: `Finished`，`target/debug/voicepilot-ui.exe` mtime 更新

- [ ] **Step 2: 手工端到端（安静房间 + 麦克风）**

1. 启动新编 `voicepilot-ui.exe`，设置页确认 LLM 已配置（Task 0 已修启动重建：重启后无需重存，直连可用；若仍不可用再进设置页检查）。
2. 第一句说“打开记事本”（关键词命中，不耗 LLM）。
3. 第二句说“还是刚才那个”（无关键词，LLM classify 带上文 `打开记事本`）。
Expected: 第二句不再是干巴巴的 Unmatched，而是能关联上文的路由；`turns` 表新增 2 行（用 CLI `show` 或 DB 查看确认，transcript ≤500 字）。
4. 打开 `privacy_mode` 重复一次。
Expected: 功能降级为关键词/Unmatched，且 `turns` 表行数不变（无记忆写入）。

- [ ] **Step 3: 记录验证结论到本计划末尾（追加 `## Verification log`，日期+通过项+残留）后 Commit**

```bash
git add docs/superpowers/plans/2026-09-04-realtime-snapshot-memory.md
git commit -m "docs: record verification log for realtime snapshot memory plan"
```

---

## Self-review（已执行）

1. **Spec 覆盖**：拿实时数据再规划→Task 1/2/3/5（快照类型、门禁、注入、现采）；实时记忆→Task 4/5（turns 表、封轮、回忆）；启动根因→Task 0；调用效率→Task 6（前缀稳定+缓存）；打断→Task 7；全部需求均有任务对应。
2. **占位符扫描**：无 TBD/TODO/“适当处理”；唯一条件分支是 Step 4 的 `tracing` import 回退，给了确切替代写法（`eprintln!` 先例在本文件存在）。
3. **类型一致性**：`RealtimeSnapshot` 在 planner.rs 定义，router_bridge 用全路径 `crate::planner::RealtimeSnapshot`，ui 用 `trust_kernel::planner::{…}`；`ListenTimings` 由 `listener` 模块导出（listener.rs 已定义）；`TurnRecord` 字段与建表列一一对应；`PlannerInput` 新增字段后 `tests/planner_pipeline.rs` 内 3 处旧构造（Task 2 Step 4 已逐处列出）全部加 `snapshot: None`；`CachedRoute` 字段与 `llm_route_cache` 列一一对应（`confidence REAL` ↔ f32，rusqlite 支持）；`ExtractedSlot: Serialize + Deserialize` 已有 derive，缓存存取 JSON 无需新 bound；`commands.rs` 的文本路径与 CLI 路径不构造快照、签名不动，无需改；前端 helper 与单测同目录、同 import 风格（`vitest` + 相对路径），与既有 `__tests__` 一致。

---

## Verification log（2026-09-04，执行 Task 8 时追加）

- 通过：`trust-kernel --lib` 全量 279 passed / 0 failed / 1 ignored（含新增 snapshot 3、turns 2、db 2、llm_cache 3、prompt 顺序 1）。
- 通过：`--test planner_pipeline` 2/2、`--test realtime_snapshot` 3/3（含缓存命中：第二次同句零 HTTP）、`--test w8_plan4_router_bridge_dag` 8/8、`w7_settings_llm_smoke` 5/5（含启动重建）。
- 通过：`voicepilot-ui --lib voice` 15/15、`voice_commands_unit` 12/12、`voice_cancel_cache_unit` 2/2；`npm test -- ttsPlayback` 2/2；`npx tsc --noEmit` 零 error。
- 通过：`cargo build -p voicepilot-ui --features voice,custom-protocol` 成功，exe 已更新。
- 残留（执行中发现并已处理）：① 全量多 target 并发链接偶发 rlib 缺失，改逐 target 跑即过，属环境抖动；② `cargo test` 只接受单个 filter，计划内多 filter 命令执行时已拆分；③ router_bridge 原有重复 pipeline 调用已删除（重复烧一次 LLM）；④ MainView 历史 `as unknown as` 补 SAFETY 注释。
- 待用户手工：安静房间两句有指代的语音端到端 + privacy 对照（Task 8 Step 2）。

## Review loop Round 1（parent 编排，3 reviewer 并行 + fix worker 收尾）

- Round 1 结论：无 P0；7 项值得修的已全部收掉；defer：放宽 scenario_6/7 期望、缓存聊天答案、流式。
- 修：语音流渲染回答正文、桌宠气泡展示答案、`selectSpeakText` 抽取+单测、`turns.rs` 文档补 `chat`、chat prompt 拼功能清单防幻觉、聊天改走无 guard 的 `chat_context()`、scenario_7 panic 文案改诚实。
- 验证：`trust-kernel --lib` 286/0、`realtime_snapshot` 4/4（含兜底）、`w8_e2e_dag_smoke` 8/8、UI voice 15+12+2、`npm test` 24 passed 1 skipped（旧 ts 合并 stub）、`tsc` 零 error；clippy 仅历史 warning。
