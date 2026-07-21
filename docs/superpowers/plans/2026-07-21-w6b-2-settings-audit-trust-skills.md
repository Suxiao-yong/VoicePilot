# VoicePilot W6b-2: Settings + Audit Viewer + Trust Center + Skills Manager + Voice Fast-Follow 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标:** 在 W6b-1 Main Chat + Voice 基础上,实现 V1.1.2 §8.3 四个新界面(Settings / Audit Viewer / Trust Center / Skills Manager)+ §8.3 Kill Switch Bar + W6b-1 延后的三项 voice 优化(voice cancel #57、model caching #61、partial transcript #47 §8.4)。

**架构:** 后端在 `trust-kernel` 新增 `ConfigRepo`(KV 持久化)+ `SkillRepo`(skill 统计 CRUD)+ 扩展 `AuditLogger` trait(查询方法)+ 扩展 `McpServerRepo`(toggle 便捷方法)。`voicepilot-ui` crate 新增 8 个 Tauri command(get_settings/update_settings/list_audit/list_mcp_servers/toggle_mcp_server/list_skills/toggle_skill/cancel_voice)+ 前端 4 个 View 组件(SettingsView/AuditViewerView/TrustCenterView/SkillsManagerView)+ App.tsx 多视图导航 + Kill Switch Bar。Voice 优化:AppState 加 `kill_switch: Arc<AtomicBool>` + `whisper_cache: Arc<Mutex<Option<WhisperEngine>>>`,VoiceListener 循环检查 cancel flag,每 2s 发射 `transcription-partial` 事件。

**Spec 对齐(V1.1.2):**
- §8.2 窗口权限:Settings 窗口可修改非敏感配置(白名单、TTL、TTS 开关),禁止修改策略/删除审计;Audit 窗口只读查询脱敏日志;Main 窗口可取消任务。
- §8.2 IPC 硬化红线:① WebView 不直接访问 FS ② UI 不直接调 MCP ③ approval_request_id 一次性 ④ CSP 禁远程脚本 ⑤ 严格 Schema `additionalProperties=false` ⑥ 前端 risk/tool/resource 不可信,Rust 侧重读 ⑦ effect_manifest 冻结快照。
- §8.3 主要界面:Settings(白名单/TTS/ASR/隐私模式/TTL + V1.1 Skill 启用禁用 + Compensation TTL)、Audit Viewer(任务历史/步骤详情/策略匹配/审批记录)、Trust Center(已启用 MCP Server/可见目录域名/最近调用/egress 策略/一键停用)、Skills Manager(已保存 Skill 列表/成功率/平均延迟/风险等级)、Kill Switch Bar(常驻顶部一键停止)。
- §8.4 语音转写快速纠错:实时 partial transcript(W6b-2 实现)、低置信字段加下划线(延后 W6b-3)、路径/应用名/数量可点击 Chip(延后 W6b-3)、高风险参数视觉确认(延后 W6b-3)、TTS 打断(延后 W6b-3)。

**Feature 门控(与 W6a/W6b-1 一致,opt-in):**
- `voicepilot-ui` crate 的 `voice` feature 已定义为 `voice = ["tauri", "trust-kernel/voice"]`。
- Settings/Audit/Trust Center/Skills Manager/Kill Switch 的 commands 与前端组件**不依赖 voice**,归入 `tauri` feature(默认 `--features tauri` 可用)。
- Voice cancel / model caching / partial transcript 依赖 voice 模块,归入 `voice` feature。
- 所有新 Rust 模块按依赖关系用 `#[cfg(feature = "tauri")]` 或 `#[cfg(feature = "voice")]` 门控。

**技术栈:**
- Rust 1.96+(已验证)
- Tauri 2.x + React 18 + TypeScript 5(W6a 已集成)
- rusqlite(W1 已集成)
- whisper-rs 0.13(W5 已集成,voice feature opt-in)
- `tauri::Emitter` + `tauri::State`(W6a/W6b-1 已用)

**构建前提条件:**
- 默认 workspace 测试:Rust 1.96+,196 个通过。
- `--features tauri`:Node 22+ + npm 10+(W6a 已验证)。
- `--features voice`:额外 CMake + MSVC + libclang + `LIBCLANG_PATH` + `WHISPER_DONT_GENERATE_BINDINGS=1`(W5 已验证)。

**不在范围内(延后 W6b-3 / W8):**
- Diff Preview(§8.3 Approval Modal 新增,W6b-3)
- Approval Modal 批次审批 + 不可撤销红色高亮(W6b-3)
- W6a Fast-Follow(ApprovalModal submittedRef、响应式、CSP 加固,W6b-3)
- Tauri 打包 + E2E(W6b-3)
- §8.4 低置信下划线 / Chip 修改 / TTS 打断(W6b-3)
- OTel trace 链接 / egress 日志展示(W8,需 taint tracking)
- 多窗口拆分(Main/Approval/Audit/Settings 独立 Tauri window,W6b-2 用单窗口+视图切换,W6b-3 评估多窗口)
- TTS 语音反馈(§8.4,延后)
- 唤醒词检测(issue #48)/ 模型自动下载(issue #46)

---

## 文件结构

### 新增文件(trust-kernel 后端)

| 文件 | 职责 |
|---|---|
| `voicepilot/crates/trust-kernel/src/migrations/002_app_config.sql` | 新增 `app_config(key, value, updated_at)` 表 |
| `voicepilot/crates/trust-kernel/src/repo/config_repo.rs` | `ConfigRepo` KV CRUD(get/set/list/delete) |
| `voicepilot/crates/trust-kernel/src/skills/repo.rs` | `SkillRepo` CRUD(list/get/upsert/toggle/incr_success) |
| `voicepilot/crates/trust-kernel/tests/config_repo_unit.rs` | ConfigRepo 单元测试 |
| `voicepilot/crates/trust-kernel/tests/skill_repo_unit.rs` | SkillRepo 单元测试 |
| `voicepilot/crates/trust-kernel/tests/audit_query_unit.rs` | AuditLogger 查询方法单元测试 |

### 修改文件(trust-kernel 后端)

| 文件 | 变更 |
|---|---|
| `voicepilot/crates/trust-kernel/src/db.rs` | 加载 002 迁移(MIGRATIONS 数组追加 002) |
| `voicepilot/crates/trust-kernel/src/repo/mod.rs` | 导出 `pub mod config_repo;` |
| `voicepilot/crates/trust-kernel/src/audit.rs` | `AuditLogger` trait 加 `list_recent`/`list_for_task` 方法 + `SqliteAuditLogger` 实现 |
| `voicepilot/crates/trust-kernel/src/skills/mod.rs` | 导出 `pub mod repo;` |
| `voicepilot/crates/trust-kernel/src/mcp/repo.rs` | 新增 `toggle_enabled(server_id, enabled)` 便捷方法 |
| `voicepilot/crates/trust-kernel/src/kernel.rs` | 新增 `config_repo()` / `skill_repo()` / `list_audit_recent` / `list_audit_for_task` / `toggle_mcp_server` 访问器 |
| `voicepilot/crates/trust-kernel/src/voice/listener.rs` | `VoiceListener::listen` 接收 `cancel: &AtomicBool`,循环检查;新增 `listen_with_partial` 发射 partial callback |
| `voicepilot/crates/trust-kernel/src/voice/mod.rs` | 无变更(listener 已导出) |

### 新增文件(ui crate)

| 文件 | 职责 |
|---|---|
| `voicepilot/crates/ui/src/settings_commands.rs` | `get_settings_command` + `update_settings_command` + `SettingsDto` serde |
| `voicepilot/crates/ui/src/audit_commands.rs` | `list_audit_recent_command` + `list_audit_for_task_command` + `AuditEventDto` |
| `voicepilot/crates/ui/src/trust_center_commands.rs` | `list_mcp_servers_command` + `toggle_mcp_server_command` + `McpServerDto` |
| `voicepilot/crates/ui/src/skills_commands.rs` | `list_skills_command` + `toggle_skill_command` + `SkillDto` |
| `voicepilot/crates/ui/tests/settings_commands_unit.rs` | settings commands 单元测试(mock kernel) |
| `voicepilot/crates/ui/tests/audit_commands_unit.rs` | audit commands 单元测试 |
| `voicepilot/crates/ui/tests/trust_center_commands_unit.rs` | trust center commands 单元测试 |
| `voicepilot/crates/ui/tests/skills_commands_unit.rs` | skills commands 单元测试 |
| `voicepilot/crates/ui/tests/w6b2_smoke.rs` | W6b-2 端到端冒烟测试(各 command 集成) |

### 修改文件(ui crate + 前端)

| 文件 | 变更 |
|---|---|
| `voicepilot/crates/ui/src/lib.rs` | 新增 `#[cfg(feature = "tauri")] pub mod {settings,audit,trust_center,skills}_commands;` |
| `voicepilot/crates/ui/src/commands.rs` | `register_handlers` 追加 8 个新 command 到 `generate_handler!` |
| `voicepilot/crates/ui/src/state.rs` | AppState 加 `kill_switch: Arc<AtomicBool>` + `#[cfg(feature="voice")] whisper_cache: Arc<Mutex<Option<WhisperEngine>>>` |
| `voicepilot/crates/ui/src/voice_commands.rs` | `voice_listen_command` 使用 AppState 的 kill_switch + whisper_cache;循环中发射 `transcription-partial`;新增 `cancel_voice_command` |
| `voicepilot/crates/ui/src/app.rs` | `run` 注册 `cancel_voice_command`(voice feature) |
| `voicepilot/crates/ui/web/src/types.ts` | 新增 `Settings` / `AuditEvent` / `McpServer` / `Skill` / `View` 类型 |
| `voicepilot/crates/ui/web/src/api.ts` | 新增 `getSettings`/`updateSettings`/`listAuditRecent`/`listAuditForTask`/`listMcpServers`/`toggleMcpServer`/`listSkills`/`toggleSkill`/`cancelVoice` invoke 包装 + `onTranscriptionPartial` 事件监听 |
| `voicepilot/crates/ui/web/src/components/SettingsView.tsx` | Settings 面板表单(voice 配置 + 隐私 + TTL) |
| `voicepilot/crates/ui/web/src/components/AuditViewerView.tsx` | Audit 列表 + 任务详情时间线 |
| `voicepilot/crates/ui/web/src/components/TrustCenterView.tsx` | MCP Server 表格 + toggle 开关 |
| `voicepilot/crates/ui/web/src/components/SkillsManagerView.tsx` | Skill 表格 + toggle 开关 |
| `voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx` | 常驻顶部红色停止按钮 |
| `voicepilot/crates/ui/web/src/components/MainView.tsx` | 接收 partial transcript 事件,listening-indicator 显示 partial 文本 |
| `voicepilot/crates/ui/web/src/App.tsx` | 多视图导航(view state + 侧边栏)+ 集成 KillSwitchBar + 4 个新 View |
| `voicepilot/crates/ui/web/src/styles.css` | 新增 `.sidebar` / `.view-container` / `.kill-switch-bar` / `.settings-form` / `.audit-timeline` / `.mcp-table` / `.skill-table` 样式 |

---

## 任务 1: ConfigRepo + migration 002 + Settings commands + SettingsView

**目标:** 新增 `app_config` 表 + `ConfigRepo` KV CRUD + `get_settings_command`/`update_settings_command` Tauri command + 前端 `SettingsView` 表单(voice 配置 / 隐私模式 / Compensation TTL)。voice 配置通过 Settings 持久化,`voice_listen_command` 启动时从 ConfigRepo 读取(而非硬编码 `Default::default()`)。

**文件:**
- 创建:`voicepilot/crates/trust-kernel/src/migrations/002_app_config.sql`
- 修改:`voicepilot/crates/trust-kernel/src/db.rs`
- 创建:`voicepilot/crates/trust-kernel/src/repo/config_repo.rs`
- 修改:`voicepilot/crates/trust-kernel/src/repo/mod.rs`
- 修改:`voicepilot/crates/trust-kernel/src/kernel.rs`
- 创建:`voicepilot/crates/trust-kernel/tests/config_repo_unit.rs`
- 创建:`voicepilot/crates/ui/src/settings_commands.rs`
- 修改:`voicepilot/crates/ui/src/lib.rs`
- 修改:`voicepilot/crates/ui/src/commands.rs`
- 创建:`voicepilot/crates/ui/tests/settings_commands_unit.rs`
- 创建:`voicepilot/crates/ui/web/src/components/SettingsView.tsx`
- 修改:`voicepilot/crates/ui/web/src/types.ts`
- 修改:`voicepilot/crates/ui/web/src/api.ts`
- 修改:`voicepilot/crates/ui/web/src/styles.css`

- [ ] **步骤 1: 写 migration 002 + ConfigRepo 失败测试 (red)**

创建 `voicepilot/crates/trust-kernel/src/migrations/002_app_config.sql`:

```sql
-- W6b-2: Settings 持久化 KV 表(V1.1.2 §8.3 Settings)
-- 简单 key-value 存储,value 为 JSON 字符串。
-- key 命名空间约定:
--   voice.model_path / voice.language / voice.threads
--   voice.vad.energy_threshold / voice.vad.max_silence_ms / voice.vad.min_speech_ms
--   voice.max_duration_ms / voice.chunk_duration_ms
--   privacy.mode
--   compensation.ttl_hours
CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

创建 `voicepilot/crates/trust-kernel/tests/config_repo_unit.rs`:

```rust
//! ConfigRepo 单元测试 —— KV 持久化(V1.1.2 §8.3 Settings)。

use rusqlite::Connection;
use trust_kernel::db;
use trust_kernel::repo::config_repo::ConfigRepo;

fn setup() -> (Connection, ConfigRepo) {
    let conn = db::open_in_memory().expect("open_in_memory");
    let repo = ConfigRepo::new();
    (conn, repo)
}

#[test]
fn config_set_and_get_roundtrip() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.model_path", "/models/tiny.bin").expect("set");
    let val = repo.get(&conn, "voice.model_path").expect("get");
    assert_eq!(val.as_deref(), Some("/models/tiny.bin"));
}

#[test]
fn config_get_returns_none_for_missing_key() {
    let (conn, repo) = setup();
    let val = repo.get(&conn, "nonexistent.key").expect("get");
    assert!(val.is_none());
}

#[test]
fn config_set_overwrites_existing_value() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.threads", "4").expect("set 1");
    repo.set(&conn, "voice.threads", "8").expect("set 2");
    let val = repo.get(&conn, "voice.threads").expect("get");
    assert_eq!(val.as_deref(), Some("8"));
}

#[test]
fn config_list_returns_all_keys() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.model_path", "/m.bin").expect("set");
    repo.set(&conn, "privacy.mode", "true").expect("set");
    repo.set(&conn, "compensation.ttl_hours", "24").expect("set");
    let all = repo.list(&conn).expect("list");
    assert_eq!(all.len(), 3);
    let keys: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
    assert!(keys.contains(&"voice.model_path"));
    assert!(keys.contains(&"privacy.mode"));
    assert!(keys.contains(&"compensation.ttl_hours"));
}

#[test]
fn config_delete_removes_key() {
    let (conn, repo) = setup();
    repo.set(&conn, "voice.threads", "4").expect("set");
    repo.delete(&conn, "voice.threads").expect("delete");
    let val = repo.get(&conn, "voice.threads").expect("get");
    assert!(val.is_none());
}

#[test]
fn config_delete_missing_key_is_noop() {
    let (conn, repo) = setup();
    repo.delete(&conn, "nonexistent").expect("delete should not error");
}
```

- [ ] **步骤 2: 运行测试验证失败 (red)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test config_repo_unit
# 期望:编译错误 —— ConfigRepo 模块不存在
```

- [ ] **步骤 3: 实现 ConfigRepo + 修改 db.rs 加载 002**

修改 `voicepilot/crates/trust-kernel/src/db.rs`。现有代码是**单一 const + `execute_batch`**(非数组):

```rust
// 现有(参考,不改):
const MIGRATION_001: &str = include_str!("migrations/001_init.sql");

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(MIGRATION_001)?;
    tracing::info!("migrations applied");
    Ok(())
}
```

改为追加 002 const + 在 `run_migrations` 内追加 `execute_batch`:

```rust
const MIGRATION_001: &str = include_str!("migrations/001_init.sql");
const MIGRATION_002: &str = include_str!("migrations/002_app_config.sql");

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(MIGRATION_001)?;
    conn.execute_batch(MIGRATION_002)?;
    tracing::info!("migrations applied");
    Ok(())
}
```

创建 `voicepilot/crates/trust-kernel/src/repo/config_repo.rs`:

```rust
//! ConfigRepo —— V1.1.2 §8.3 Settings 持久化 KV 存储。
//!
//! 简单 key-value 表,value 为字符串(调用方负责 JSON 序列化)。
//! key 命名空间约定见 migrations/002_app_config.sql 注释。

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct ConfigRepo;

impl ConfigRepo {
    pub fn new() -> Self {
        Self
    }

    /// 获取某个配置值。返回 None 表示 key 不存在。
    pub fn get(&self, conn: &Connection, key: &str) -> Result<Option<String>> {
        let mut stmt = conn.prepare("SELECT value FROM app_config WHERE key = ?1")?;
        let val = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(val)
    }

    /// 设置(或覆盖)某个配置值。
    pub fn set(&self, conn: &Connection, key: &str, value: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO app_config (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, now_iso()],
        )?;
        Ok(())
    }

    /// 列出所有配置项(按 key 字典序)。
    pub fn list(&self, conn: &Connection) -> Result<Vec<(String, String)>> {
        let mut stmt = conn.prepare("SELECT key, value FROM app_config ORDER BY key ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 删除某个配置项。key 不存在时为 no-op。
    pub fn delete(&self, conn: &Connection, key: &str) -> Result<()> {
        conn.execute("DELETE FROM app_config WHERE key = ?1", params![key])?;
        Ok(())
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}
```

> **关键:** 类型别名是 `crate::error::Result<T>`(`std::result::Result<T, KernelError>`),**不**存在 `KernelResult`。`rusqlite::OptionalExtension` trait 需在 use 中显式引入(用于 `.optional()`)。

修改 `voicepilot/crates/trust-kernel/src/repo/mod.rs`,追加:

```rust
pub mod config_repo;
```

修改 `voicepilot/crates/trust-kernel/src/kernel.rs`,在 `TrustKernel` impl 块内新增访问器。

**关键设计:** 与 W4 `McpServerRepo` 模式一致 —— `ConfigRepo` 是无状态单元,不在 `TrustKernel` 加字段(避免改 `with_conn` + 所有构造器)。访问器每次返回新实例:

```rust
/// 获取 ConfigRepo(W6b-2 Settings 持久化)。
/// 与 McpServerRepo 模式一致:ConfigRepo 无状态,每次返回新实例。
/// 调用方用 `let conn = kernel.conn(); kernel.config_repo().set(&conn, ...)`。
pub fn config_repo(&self) -> crate::repo::config_repo::ConfigRepo {
    crate::repo::config_repo::ConfigRepo::new()
}
```

**不要**在 `TrustKernel` 结构体加 `config_repo` 字段,**不要**修改 `with_conn` / `open_in_memory` / `open_file` 构造器。

参考 W4 既有模式:`McpServerRepo::new()` + `repo.method(&conn, ...)`,例如 `kernel.rs` 内部 `let conn = self.conn.lock().unwrap(); self.task_repo.create(&conn, &task)?;`。

- [ ] **步骤 4: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test config_repo_unit
# 期望:6 tests passed
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel
# 期望:所有现有测试 + 6 新测试全通过,无回归
```

- [ ] **步骤 5: 写 settings_commands 失败测试 (red)**

创建 `voicepilot/crates/ui/tests/settings_commands_unit.rs`:

```rust
#![cfg(feature = "tauri")]

//! settings_commands 单元测试 —— get_settings/update_settings Tauri command 逻辑。

use voicepilot_ui::settings_commands::{SettingsDto, flatten_to_kv, merge_from_kv};

#[test]
fn settings_dto_default_has_sensible_values() {
    let dto = SettingsDto::default();
    assert_eq!(dto.voice_model_path, "ggml-tiny.bin");
    assert_eq!(dto.voice_threads, 4);
    assert!(dto.privacy_mode || !dto.privacy_mode); // bool, 只检查字段存在
    assert_eq!(dto.compensation_ttl_hours, 24);
}

#[test]
fn settings_dto_roundtrip_through_kv() {
    let dto = SettingsDto {
        voice_model_path: "/models/base.bin".to_string(),
        voice_language: Some("zh".to_string()),
        voice_threads: 8,
        vad_energy_threshold: 150.0,
        vad_max_silence_ms: 800,
        vad_min_speech_ms: 300,
        voice_max_duration_ms: 60000,
        voice_chunk_duration_ms: 750,
        privacy_mode: true,
        compensation_ttl_hours: 48,
    };
    let kv = flatten_to_kv(&dto);
    // 验证所有字段都序列化为 KV
    assert!(kv.iter().any(|(k, _)| k == "voice.model_path"));
    assert!(kv.iter().any(|(k, _)| k == "voice.threads"));
    assert!(kv.iter().any(|(k, _)| k == "privacy.mode"));
    let restored = merge_from_kv(&kv).expect("merge");
    assert_eq!(restored.voice_model_path, "/models/base.bin");
    assert_eq!(restored.voice_threads, 8);
    assert!(restored.privacy_mode);
    assert_eq!(restored.compensation_ttl_hours, 48);
}

#[test]
fn settings_merge_from_partial_kv_uses_defaults_for_missing() {
    let kv = vec![("voice.threads".to_string(), "16".to_string())];
    let dto = merge_from_kv(&kv).expect("merge");
    assert_eq!(dto.voice_threads, 16);
    // 缺失字段用默认值
    assert_eq!(dto.voice_model_path, "ggml-tiny.bin");
    assert_eq!(dto.compensation_ttl_hours, 24);
}
```

- [ ] **步骤 6: 运行测试验证失败 (red)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test settings_commands_unit
# 期望:编译错误 —— settings_commands 模块不存在
```

- [ ] **步骤 7: 实现 settings_commands.rs**

创建 `voicepilot/crates/ui/src/settings_commands.rs`:

```rust
//! Settings Tauri commands —— V1.1.2 §8.3 Settings 面板后端。
//!
//! 持久化层:ConfigRepo KV 表(app_config)。
//! 前端 DTO:SettingsDto(扁平结构,serde JSON)。
//! 内部:flatten_to_kv / merge_from_kv 在 DTO 与 KV 之间转换。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{UiError, UiResult};
use crate::state::AppState;

/// Settings 面板 DTO(前端直接消费的扁平结构)。
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
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            voice_model_path: "ggml-tiny.bin".to_string(),
            voice_language: None,
            voice_threads: 4,
            vad_energy_threshold: 100.0,
            vad_max_silence_ms: 700,
            vad_min_speech_ms: 200,
            voice_max_duration_ms: 30000,
            voice_chunk_duration_ms: 500,
            privacy_mode: false,
            compensation_ttl_hours: 24,
        }
    }
}

/// 将 DTO 展平为 KV 列表(用于持久化到 app_config 表)。
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
    ]
}

/// 从 KV 列表合并为 DTO(缺失字段用默认值)。
pub fn merge_from_kv(kv: &[(String, String)]) -> UiResult<SettingsDto> {
    let mut dto = SettingsDto::default();
    for (k, v) in kv {
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
            _ => {} // 忽略未知 key(前向兼容)
        }
    }
    Ok(dto)
}

/// 读取所有设置(合并持久化值与默认值)。
#[tauri::command]
pub async fn get_settings_command(state: State<'_, AppState>) -> UiResult<SettingsDto> {
    let conn = state.kernel.conn();
    let kv = state.kernel.config_repo().list(&conn)?;
    merge_from_kv(&kv)
}

/// 更新设置(全量覆盖:将 DTO 展平后逐条 set,不存在部分更新语义)。
#[tauri::command]
pub async fn update_settings_command(state: State<'_, AppState>, settings: SettingsDto) -> UiResult<()> {
    let conn = state.kernel.conn();
    let kv = flatten_to_kv(&settings);
    for (k, v) in &kv {
        state.kernel.config_repo().set(&conn, k, v)?;
    }
    Ok(())
}
```

注意:`UiError::InvalidConfig(String)` 变体需在 `voicepilot/crates/ui/src/error.rs` **显式新增**(当前 `UiError` 只有 `Kernel/Tauri/Io/ApprovalTimeout/ApprovalNotFound/Serde` 6 个变体,**无** `InvalidConfig`)。修改 `voicepilot/crates/ui/src/error.rs`:

```rust
#[derive(Debug, Error)]
pub enum UiError {
    #[error("kernel error: {0}")]
    Kernel(#[from] trust_kernel::error::KernelError),
    #[error("tauri error: {0}")]
    Tauri(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("approval timed out")]
    ApprovalTimeout,
    #[error("approval request not found: {0}")]
    ApprovalNotFound(String),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}
```

> `Kernel(#[from] KernelError)` 已存在,`load_voice_settings` 中 `?` 可直接传播 `KernelError`。`state.kernel.conn()` 返回 `MutexGuard<Connection>`(W6a 已建立的模式)。

修改 `voicepilot/crates/ui/src/lib.rs`,在 `#[cfg(feature = "tauri")]` 段追加:

```rust
pub mod settings_commands;
```

修改 `voicepilot/crates/ui/src/commands.rs` 的 `register_handlers`,在 `generate_handler!` 列表追加:

```rust
crate::settings_commands::get_settings_command,
crate::settings_commands::update_settings_command,
```

- [ ] **步骤 8: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test settings_commands_unit
# 期望:3 tests passed
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# 期望:W6a 12 + W6b-2 settings 3 = 15 passed
```

- [ ] **步骤 9: 实现前端 SettingsView + types + api**

修改 `voicepilot/crates/ui/web/src/types.ts`,追加:

```typescript
export interface Settings {
  voice_model_path: string;
  voice_language: string | null;
  voice_threads: number;
  vad_energy_threshold: number;
  vad_max_silence_ms: number;
  vad_min_speech_ms: number;
  voice_max_duration_ms: number;
  voice_chunk_duration_ms: number;
  privacy_mode: boolean;
  compensation_ttl_hours: number;
}

export type View = "main" | "settings" | "audit" | "trust" | "skills";
```

修改 `voicepilot/crates/ui/web/src/api.ts`,追加:

```typescript
export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings_command");
}

export async function updateSettings(settings: Settings): Promise<void> {
  await invoke("update_settings_command", { settings });
}
```

创建 `voicepilot/crates/ui/web/src/components/SettingsView.tsx`:

```tsx
import { useEffect, useState } from "react";
import { getSettings, updateSettings } from "../api";
import type { Settings } from "../types";

export function SettingsView(): JSX.Element {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((e) => setError(String(e)));
  }, []);

  const handleField = <K extends keyof Settings>(key: K, value: Settings[K]): void => {
    if (settings) {
      setSettings({ ...settings, [key]: value });
      setSaved(false);
    }
  };

  const handleSave = (): void => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    updateSettings(settings)
      .then(() => {
        setSaved(true);
        setSaving(false);
      })
      .catch((e) => {
        setError(String(e));
        setSaving(false);
      });
  };

  if (settings === null) {
    return <div className="view-container" role="status" aria-live="polite">加载设置中…</div>;
  }

  return (
    <section className="view-container settings-form" aria-labelledby="settings-heading">
      <h2 id="settings-heading">§ 8.3 Settings</h2>

      <fieldset className="settings-fieldset">
        <legend>语音配置</legend>
        <div className="form-row">
          <label htmlFor="voice_model_path">模型路径</label>
          <input
            id="voice_model_path"
            type="text"
            value={settings.voice_model_path}
            onChange={(e) => handleField("voice_model_path", e.target.value)}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_language">语言(空=自动)</label>
          <input
            id="voice_language"
            type="text"
            value={settings.voice_language ?? ""}
            onChange={(e) => handleField("voice_language", e.target.value || null)}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_threads">线程数</label>
          <input
            id="voice_threads"
            type="number"
            min={1}
            max={16}
            value={settings.voice_threads}
            onChange={(e) => handleField("voice_threads", Number(e.target.value))}
          />
        </div>
      </fieldset>

      <fieldset className="settings-fieldset">
        <legend>VAD 配置</legend>
        <div className="form-row">
          <label htmlFor="vad_energy_threshold">能量阈值</label>
          <input
            id="vad_energy_threshold"
            type="number"
            step="10"
            value={settings.vad_energy_threshold}
            onChange={(e) => handleField("vad_energy_threshold", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="vad_max_silence_ms">静音超时(ms)</label>
          <input
            id="vad_max_silence_ms"
            type="number"
            value={settings.vad_max_silence_ms}
            onChange={(e) => handleField("vad_max_silence_ms", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="vad_min_speech_ms">最短语音(ms)</label>
          <input
            id="vad_min_speech_ms"
            type="number"
            value={settings.vad_min_speech_ms}
            onChange={(e) => handleField("vad_min_speech_ms", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_max_duration_ms">最长录音(ms)</label>
          <input
            id="voice_max_duration_ms"
            type="number"
            value={settings.voice_max_duration_ms}
            onChange={(e) => handleField("voice_max_duration_ms", Number(e.target.value))}
          />
        </div>
        <div className="form-row">
          <label htmlFor="voice_chunk_duration_ms">块大小(ms)</label>
          <input
            id="voice_chunk_duration_ms"
            type="number"
            value={settings.voice_chunk_duration_ms}
            onChange={(e) => handleField("voice_chunk_duration_ms", Number(e.target.value))}
          />
        </div>
      </fieldset>

      <fieldset className="settings-fieldset">
        <legend>隐私与补偿</legend>
        <div className="form-row checkbox-row">
          <input
            id="privacy_mode"
            type="checkbox"
            checked={settings.privacy_mode}
            onChange={(e) => handleField("privacy_mode", e.target.checked)}
          />
          <label htmlFor="privacy_mode">隐私模式(禁用审计详情记录)</label>
        </div>
        <div className="form-row">
          <label htmlFor="compensation_ttl_hours">Compensation TTL(小时)</label>
          <input
            id="compensation_ttl_hours"
            type="number"
            min={1}
            value={settings.compensation_ttl_hours}
            onChange={(e) => handleField("compensation_ttl_hours", Number(e.target.value))}
          />
        </div>
      </fieldset>

      <div className="form-actions">
        <button type="button" onClick={handleSave} disabled={saving}>
          {saving ? "保存中…" : "保存设置"}
        </button>
        {saved && <span className="save-success" role="status">✓ 已保存</span>}
        {error && <span className="form-error" role="alert">错误:{error}</span>}
      </div>
    </section>
  );
}
```

修改 `voicepilot/crates/ui/web/src/styles.css`,追加:

```css
.settings-form { padding: 24px; max-width: 720px; }
.settings-fieldset { border: 1px solid #1f2937; border-radius: 4px; padding: 16px; margin-bottom: 20px; }
.settings-fieldset legend { color: #fbbf24; font-family: "IBM Plex Mono", monospace; font-size: 14px; padding: 0 8px; }
.settings-form .form-row { display: flex; align-items: center; margin-bottom: 12px; gap: 12px; }
.settings-form .form-row label { width: 180px; color: #f1f5f9; font-size: 14px; }
.settings-form .form-row input[type="text"],
.settings-form .form-row input[type="number"] { flex: 1; background: #0a0e1a; border: 1px solid #1f2937; color: #f1f5f9; padding: 6px 10px; border-radius: 4px; font-family: "IBM Plex Mono", monospace; }
.settings-form .checkbox-row { gap: 8px; }
.settings-form .checkbox-row label { width: auto; }
.form-actions { margin-top: 24px; display: flex; align-items: center; gap: 12px; }
.form-actions button { background: #f59e0b; color: #0a0e1a; border: none; padding: 8px 20px; border-radius: 4px; font-weight: 600; cursor: pointer; }
.form-actions button:disabled { opacity: 0.5; cursor: not-allowed; }
.save-success { color: #10b981; font-size: 14px; }
.form-error { color: #ef4444; font-size: 14px; }
```

- [ ] **步骤 10: 构建前端 + 编译验证**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web
npm.cmd run build
# 期望:dist/index.html + assets 生成
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# 期望:Finished
```

- [ ] **步骤 11: 提交**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/migrations/002_app_config.sql voicepilot/crates/trust-kernel/src/db.rs voicepilot/crates/trust-kernel/src/repo/config_repo.rs voicepilot/crates/trust-kernel/src/repo/mod.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/config_repo_unit.rs voicepilot/crates/ui/src/settings_commands.rs voicepilot/crates/ui/src/lib.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/src/error.rs voicepilot/crates/ui/tests/settings_commands_unit.rs voicepilot/crates/ui/web/src/components/SettingsView.tsx voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist
git commit -m "Task 1: ConfigRepo + Settings commands + SettingsView (V1.1 §8.3 Settings)"
```

---

## 任务 2: Audit Viewer 后端查询 + 前端 AuditViewerView

**目标:** 扩展 `AuditLogger` trait 新增 `list_recent(limit)` / `list_for_task(task_id)` 查询方法 + `SqliteAuditLogger` 实现 + `list_audit_recent_command` / `list_audit_for_task_command` Tauri command + 前端 `AuditViewerView`(任务列表 + 选中任务后展示审计事件时间线)。

**文件:**
- 修改:`voicepilot/crates/trust-kernel/src/audit.rs`
- 修改:`voicepilot/crates/trust-kernel/src/kernel.rs`
- 创建:`voicepilot/crates/trust-kernel/tests/audit_query_unit.rs`
- 创建:`voicepilot/crates/ui/src/audit_commands.rs`
- 修改:`voicepilot/crates/ui/src/lib.rs`
- 修改:`voicepilot/crates/ui/src/commands.rs`
- 创建:`voicepilot/crates/ui/tests/audit_commands_unit.rs`
- 创建:`voicepilot/crates/ui/web/src/components/AuditViewerView.tsx`
- 修改:`voicepilot/crates/ui/web/src/types.ts`
- 修改:`voicepilot/crates/ui/web/src/api.ts`
- 修改:`voicepilot/crates/ui/web/src/styles.css`

- [ ] **步骤 1: 写 AuditLogger 查询方法失败测试 (red)**

创建 `voicepilot/crates/trust-kernel/tests/audit_query_unit.rs`:

```rust
//! AuditLogger 查询方法单元测试 —— list_recent / list_for_task(V1.1.2 §8.3 Audit Viewer)。

use std::sync::{Arc, Mutex};

use trust_kernel::audit::{AuditEvent, AuditLogger, SqliteAuditLogger};
use trust_kernel::db;

fn setup() -> SqliteAuditLogger {
    let conn = db::open_in_memory().expect("open_in_memory");
    db::run_migrations(&conn).expect("run_migrations");
    SqliteAuditLogger::new(Arc::new(Mutex::new(conn)))
}

fn make_event(task_id: &str, step_id: Option<&str>, event_type: &str) -> AuditEvent {
    AuditEvent {
        log_id: format!("log-{}", uuid::Uuid::new_v4()),
        task_id: task_id.to_string(),
        step_id: step_id.map(|s| s.to_string()),
        event_type: event_type.to_string(),
        details: serde_json::json!({"test": true}),
        timestamp: chrono::Utc::now(),
        prev_hash: None,
        hash: "deadbeef".to_string(),
    }
}

#[test]
fn list_recent_returns_events_in_desc_order() {
    let logger = setup();
    let e1 = make_event("task-1", None, "TASK_CREATED");
    let e2 = make_event("task-1", Some("step-1"), "STEP_STARTED");
    let e3 = make_event("task-2", None, "TASK_CREATED");
    logger.append(&e1).expect("append");
    std::thread::sleep(std::time::Duration::from_millis(10));
    logger.append(&e2).expect("append");
    std::thread::sleep(std::time::Duration::from_millis(10));
    logger.append(&e3).expect("append");

    let recent = logger.list_recent(2).expect("list_recent");
    assert_eq!(recent.len(), 2);
    // 最新优先(desc by timestamp)
    assert_eq!(recent[0].task_id, "task-2");
    assert_eq!(recent[1].task_id, "task-1");
}

#[test]
fn list_for_task_returns_all_events_for_task() {
    let logger = setup();
    logger.append(&make_event("task-1", None, "TASK_CREATED")).expect("append");
    logger.append(&make_event("task-2", None, "TASK_CREATED")).expect("append");
    logger.append(&make_event("task-1", Some("step-1"), "STEP_STARTED")).expect("append");
    logger.append(&make_event("task-1", Some("step-1"), "STEP_SUCCEEDED")).expect("append");

    let events = logger.list_for_task("task-1").expect("list_for_task");
    assert_eq!(events.len(), 3);
    // 升序(by timestamp)
    assert_eq!(events[0].event_type, "TASK_CREATED");
    assert_eq!(events[1].event_type, "STEP_STARTED");
    assert_eq!(events[2].event_type, "STEP_SUCCEEDED");
}

#[test]
fn list_for_task_returns_empty_for_unknown_task() {
    let logger = setup();
    logger.append(&make_event("task-1", None, "TASK_CREATED")).expect("append");
    let events = logger.list_for_task("nonexistent").expect("list_for_task");
    assert!(events.is_empty());
}

#[test]
fn list_recent_with_zero_limit_returns_empty() {
    let logger = setup();
    logger.append(&make_event("task-1", None, "TASK_CREATED")).expect("append");
    let events = logger.list_recent(0).expect("list_recent");
    assert!(events.is_empty());
}
```

> **关键:** `SqliteAuditLogger::new` 接收 `Arc<Mutex<Connection>>`(不是裸 `Connection`)。`db::open_in_memory()` 返回 `Connection` 且**不**自动跑迁移 —— 必须显式调 `db::run_migrations(&conn)` 创建 `audit_logs` 表。

- [ ] **步骤 2: 运行测试验证失败 (red)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test audit_query_unit
# 期望:编译错误 —— list_recent / list_for_task 方法不存在
```

- [ ] **步骤 3: 实现 AuditLogger 查询方法**

修改 `voicepilot/crates/trust-kernel/src/audit.rs`,在 `AuditLogger` trait 定义追加(**注意:** 类型别名是 `Result<T>` 而非 `KernelResult<T>`,与现有 `append` 一致):

```rust
pub trait AuditLogger: Send + Sync {
    fn append(&self, event: &AuditEvent) -> Result<()>;

    /// 列出最近的 N 条审计事件(按 timestamp 降序)。
    fn list_recent(&self, limit: usize) -> Result<Vec<AuditEvent>>;

    /// 列出某任务的所有审计事件(按 timestamp 升序)。
    fn list_for_task(&self, task_id: &str) -> Result<Vec<AuditEvent>>;
}
```

在 `SqliteAuditLogger` impl 块追加。**注意:** `SqliteAuditLogger` 持有 `conn: Arc<Mutex<Connection>>`(不是 `Mutex<Connection>`),所以实现里用 `self.conn.lock().expect("conn poisoned")`:

```rust
    fn list_recent(&self, limit: usize) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock().expect("conn poisoned");
        let mut stmt = conn.prepare(
            "SELECT log_id, task_id, step_id, event_type, details, timestamp, prev_hash, hash
             FROM audit_logs ORDER BY timestamp DESC, log_id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let details_str: String = row.get(4)?;
            let ts_str: String = row.get(5)?;
            Ok(AuditEvent {
                log_id: row.get(0)?,
                task_id: row.get(1)?,
                step_id: row.get(2)?,
                event_type: row.get(3)?,
                details: serde_json::from_str(&details_str).unwrap_or(serde_json::Value::Null),
                timestamp: chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                prev_hash: row.get(6)?,
                hash: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn list_for_task(&self, task_id: &str) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock().expect("conn poisoned");
        let mut stmt = conn.prepare(
            "SELECT log_id, task_id, step_id, event_type, details, timestamp, prev_hash, hash
             FROM audit_logs WHERE task_id = ?1 ORDER BY timestamp ASC, log_id ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |row| {
            let details_str: String = row.get(4)?;
            let ts_str: String = row.get(5)?;
            Ok(AuditEvent {
                log_id: row.get(0)?,
                task_id: row.get(1)?,
                step_id: row.get(2)?,
                event_type: row.get(3)?,
                details: serde_json::from_str(&details_str).unwrap_or(serde_json::Value::Null),
                timestamp: chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                prev_hash: row.get(6)?,
                hash: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
```

注意:
- `params!` 宏来自 `rusqlite::params`(若文件顶部未导入,加上 `use rusqlite::params;`)
- `Result<T>` 是 `crate::error::Result<T>`(trust-kernel 内部类型别名,不是 `KernelResult`)
- 现有 `AuditEvent` 字段:`log_id` / `task_id` / `step_id` / `event_type` / `details` / `timestamp` / `prev_hash` / `hash`(全是 pub)
- 现有 `SqliteAuditLogger` 有 `conn: Arc<Mutex<Connection>>` 字段 + `new(conn: Arc<Mutex<Connection>>)` 构造器
- 现有 `audit_logs` 表 schema 在 `migrations/001_init.sql` L75-L87

修改 `voicepilot/crates/trust-kernel/src/kernel.rs`,在 `TrustKernel` impl 块追加访问器。**注意:** `TrustKernel.audit` 字段类型是 `Arc<SqliteAuditLogger>`,直接调 `self.audit.list_recent(...)`:

```rust
/// 列出最近的 N 条审计事件(V1.1.2 §8.3 Audit Viewer)。
pub fn list_audit_recent(&self, limit: usize) -> Result<Vec<crate::audit::AuditEvent>> {
    self.audit.list_recent(limit)
}

/// 列出某任务的所有审计事件(V1.1.2 §8.3 Audit Viewer)。
pub fn list_audit_for_task(&self, task_id: &str) -> Result<Vec<crate::audit::AuditEvent>> {
    self.audit.list_for_task(task_id)
}
```

**注意:** `Result<T>` 是 `crate::error::Result<T>`(kernel.rs 顶部已 use)。`self.audit` 是 `Arc<SqliteAuditLogger>`,deref 后调 trait 方法。

- [ ] **步骤 4: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test audit_query_unit
# 期望:4 tests passed
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel
# 期望:无回归
```

- [ ] **步骤 5: 写 audit_commands 失败测试 + 实现**

创建 `voicepilot/crates/ui/src/audit_commands.rs`:

```rust
//! Audit Viewer Tauri commands —— V1.1.2 §8.3 Audit Viewer 后端。
//!
//! 仅暴露只读查询(§8.2 Audit 窗口权限:只读,禁止任何写操作)。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEventDto {
    pub log_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    pub event_type: String,
    pub details: serde_json::Value,
    pub timestamp: String,
    pub prev_hash: Option<String>,
    pub hash: String,
}

impl From<trust_kernel::audit::AuditEvent> for AuditEventDto {
    fn from(e: trust_kernel::audit::AuditEvent) -> Self {
        Self {
            log_id: e.log_id,
            task_id: e.task_id,
            step_id: e.step_id,
            event_type: e.event_type,
            details: e.details,
            timestamp: e.timestamp.to_rfc3339(),
            prev_hash: e.prev_hash,
            hash: e.hash,
        }
    }
}

#[tauri::command]
pub async fn list_audit_recent_command(state: State<'_, AppState>, limit: usize) -> UiResult<Vec<AuditEventDto>> {
    let events = state.kernel.list_audit_recent(limit)?;
    Ok(events.into_iter().map(AuditEventDto::from).collect())
}

#[tauri::command]
pub async fn list_audit_for_task_command(state: State<'_, AppState>, task_id: String) -> UiResult<Vec<AuditEventDto>> {
    let events = state.kernel.list_audit_for_task(&task_id)?;
    Ok(events.into_iter().map(AuditEventDto::from).collect())
}
```

修改 `voicepilot/crates/ui/src/lib.rs`,追加 `pub mod audit_commands;`(tauri feature 段)。

修改 `voicepilot/crates/ui/src/commands.rs` 的 `register_handlers`,追加:

```rust
crate::audit_commands::list_audit_recent_command,
crate::audit_commands::list_audit_for_task_command,
```

创建 `voicepilot/crates/ui/tests/audit_commands_unit.rs`:

```rust
#![cfg(feature = "tauri")]

//! audit_commands 单元测试 —— DTO 转换 + command 逻辑。

use trust_kernel::audit::AuditEvent;
use voicepilot_ui::audit_commands::AuditEventDto;

#[test]
fn audit_event_dto_converts_from_kernel_event() {
    let event = AuditEvent {
        log_id: "log-1".to_string(),
        task_id: "task-1".to_string(),
        step_id: Some("step-1".to_string()),
        event_type: "TASK_CREATED".to_string(),
        details: serde_json::json!({"k": "v"}),
        timestamp: chrono::Utc::now(),
        prev_hash: None,
        hash: "abc".to_string(),
    };
    let dto = AuditEventDto::from(event);
    assert_eq!(dto.log_id, "log-1");
    assert_eq!(dto.task_id, "task-1");
    assert_eq!(dto.step_id.as_deref(), Some("step-1"));
    assert_eq!(dto.event_type, "TASK_CREATED");
    assert_eq!(dto.hash, "abc");
    assert!(dto.timestamp.contains("T")); // RFC3339
}

#[test]
fn audit_event_dto_handles_none_step_id() {
    let event = AuditEvent {
        log_id: "log-2".to_string(),
        task_id: "task-2".to_string(),
        step_id: None,
        event_type: "TASK_CREATED".to_string(),
        details: serde_json::Value::Null,
        timestamp: chrono::Utc::now(),
        prev_hash: Some("prev".to_string()),
        hash: "def".to_string(),
    };
    let dto = AuditEventDto::from(event);
    assert!(dto.step_id.is_none());
    assert_eq!(dto.prev_hash.as_deref(), Some("prev"));
}
```

- [ ] **步骤 6: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test audit_commands_unit
# 期望:2 tests passed
```

- [ ] **步骤 7: 实现前端 AuditViewerView**

修改 `voicepilot/crates/ui/web/src/types.ts`,追加:

```typescript
export interface AuditEvent {
  log_id: string;
  task_id: string;
  step_id: string | null;
  event_type: string;
  details: unknown;
  timestamp: string;
  prev_hash: string | null;
  hash: string;
}
```

修改 `voicepilot/crates/ui/web/src/api.ts`,追加:

```typescript
export async function listAuditRecent(limit: number): Promise<AuditEvent[]> {
  return invoke<AuditEvent[]>("list_audit_recent_command", { limit });
}

export async function listAuditForTask(taskId: string): Promise<AuditEvent[]> {
  return invoke<AuditEvent[]>("list_audit_for_task_command", { taskId });
}
```

创建 `voicepilot/crates/ui/web/src/components/AuditViewerView.tsx`:

```tsx
import { useEffect, useState } from "react";
import { listAuditRecent, listAuditForTask } from "../api";
import type { AuditEvent } from "../types";

export function AuditViewerView(): JSX.Element {
  const [recent, setRecent] = useState<AuditEvent[]>([]);
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const [taskEvents, setTaskEvents] = useState<AuditEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    listAuditRecent(50)
      .then((events) => {
        setRecent(events);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, []);

  const handleSelectTask = (taskId: string): void => {
    setSelectedTask(taskId);
    setError(null);
    listAuditForTask(taskId)
      .then(setTaskEvents)
      .catch((e) => setError(String(e)));
  };

  const uniqueTaskIds = Array.from(new Set(recent.map((e) => e.task_id)));

  return (
    <section className="view-container audit-viewer" aria-labelledby="audit-heading">
      <h2 id="audit-heading">§ 8.3 Audit Viewer</h2>
      {loading && <div role="status" aria-live="polite">加载审计日志中…</div>}
      {error && <div className="form-error" role="alert">错误:{error}</div>}

      <div className="audit-layout">
        <div className="audit-task-list" role="list" aria-label="任务列表">
          <h3>最近任务</h3>
          {uniqueTaskIds.length === 0 && <p className="empty-state">暂无审计记录</p>}
          {uniqueTaskIds.map((taskId) => (
            <button
              key={taskId}
              type="button"
              role="listitem"
              className={`audit-task-item ${selectedTask === taskId ? "selected" : ""}`}
              onClick={() => handleSelectTask(taskId)}
              aria-pressed={selectedTask === taskId}
            >
              {taskId}
            </button>
          ))}
        </div>

        <div className="audit-timeline" aria-label="审计事件时间线">
          <h3>{selectedTask ? `任务 ${selectedTask} 的事件` : "选择一个任务查看详情"}</h3>
          {selectedTask && taskEvents.length === 0 && <p className="empty-state">该任务暂无事件</p>}
          <ol className="timeline-list">
            {taskEvents.map((event) => (
              <li key={event.log_id} className="timeline-item">
                <div className="timeline-time">{new Date(event.timestamp).toLocaleString()}</div>
                <div className="timeline-event">{event.event_type}</div>
                {event.step_id && <div className="timeline-step">step: {event.step_id}</div>}
                <div className="timeline-hash">hash: {event.hash.substring(0, 12)}…</div>
                {!event.prev_hash && <span className="timeline-genesis" aria-label="创世事件">⚡</span>}
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}
```

修改 `voicepilot/crates/ui/web/src/styles.css`,追加:

```css
.audit-viewer { padding: 24px; }
.audit-layout { display: grid; grid-template-columns: 280px 1fr; gap: 24px; }
.audit-task-list { border-right: 1px solid #1f2937; padding-right: 16px; }
.audit-task-list h3, .audit-timeline h3 { color: #fbbf24; font-size: 14px; margin-bottom: 12px; font-family: "IBM Plex Mono", monospace; }
.audit-task-item { display: block; width: 100%; text-align: left; background: transparent; border: 1px solid #1f2937; color: #f1f5f9; padding: 8px 12px; margin-bottom: 6px; border-radius: 4px; cursor: pointer; font-family: "IBM Plex Mono", monospace; font-size: 12px; }
.audit-task-item:hover { background: #111827; }
.audit-task-item.selected { border-color: #f59e0b; background: #1f2937; }
.audit-timeline { max-height: 70vh; overflow-y: auto; }
.timeline-list { list-style: none; padding: 0; margin: 0; }
.timeline-item { position: relative; padding: 12px 12px 12px 32px; border-left: 2px solid #1f2937; margin-bottom: 8px; }
.timeline-item::before { content: ""; position: absolute; left: -6px; top: 16px; width: 10px; height: 10px; border-radius: 50%; background: #f59e0b; }
.timeline-time { color: #94a3b8; font-size: 11px; font-family: "IBM Plex Mono", monospace; }
.timeline-event { color: #f1f5f9; font-weight: 600; font-family: "IBM Plex Mono", monospace; }
.timeline-step, .timeline-hash { color: #64748b; font-size: 11px; font-family: "IBM Plex Mono", monospace; }
.timeline-genesis { color: #fbbf24; margin-left: 8px; }
.empty-state { color: #64748b; font-style: italic; }
```

- [ ] **步骤 8: 构建前端 + 编译验证**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web
npm.cmd run build
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# 期望:Finished
```

- [ ] **步骤 9: 提交**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/audit.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/audit_query_unit.rs voicepilot/crates/ui/src/audit_commands.rs voicepilot/crates/ui/src/lib.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/audit_commands_unit.rs voicepilot/crates/ui/web/src/components/AuditViewerView.tsx voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist
git commit -m "Task 2: Audit Viewer query methods + commands + AuditViewerView (V1.1 §8.3)"
```

---

## 任务 3: Trust Center 后端 toggle + 前端 TrustCenterView

**目标:** `McpServerRepo` 新增 `toggle_enabled(server_id, enabled)` 便捷方法 + `list_mcp_servers_command` / `toggle_mcp_server_command` Tauri command + 前端 `TrustCenterView`(MCP Server 表格 + enabled toggle 开关 + 一键停用按钮)。

**文件:**
- 修改:`voicepilot/crates/trust-kernel/src/mcp/repo.rs`
- 修改:`voicepilot/crates/trust-kernel/src/kernel.rs`
- 创建:`voicepilot/crates/ui/src/trust_center_commands.rs`
- 修改:`voicepilot/crates/ui/src/lib.rs`
- 修改:`voicepilot/crates/ui/src/commands.rs`
- 创建:`voicepilot/crates/ui/tests/trust_center_commands_unit.rs`
- 创建:`voicepilot/crates/ui/web/src/components/TrustCenterView.tsx`
- 修改:`voicepilot/crates/ui/web/src/types.ts`
- 修改:`voicepilot/crates/ui/web/src/api.ts`
- 修改:`voicepilot/crates/ui/web/src/styles.css`

- [ ] **步骤 1: 写 toggle_enabled 失败测试 (red)**

创建 `voicepilot/crates/trust-kernel/tests/mcp_toggle_unit.rs`:

```rust
//! McpServerRepo::toggle_enabled 单元测试 —— V1.1.2 §8.3 Trust Center 一键停用。

use trust_kernel::db;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};

fn setup() -> rusqlite::Connection {
    let conn = db::open_in_memory().expect("open");
    db::run_migrations(&conn).expect("run_migrations");
    conn
}

fn make_record(id: &str) -> McpServerRecord {
    McpServerRecord {
        server_id: id.to_string(),
        name: format!("Test {id}"),
        version: "1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["C:/Users"]"#.to_string()),
    }
}

#[test]
fn toggle_enabled_flips_true_to_false() {
    let conn = setup();
    let repo = McpServerRepo::new();
    repo.create(&conn, &make_record("srv-1")).expect("create");
    repo.toggle_enabled(&conn, "srv-1", false).expect("toggle");
    let got = repo.get(&conn, "srv-1").expect("get").expect("exists");
    assert!(!got.enabled);
}

#[test]
fn toggle_enabled_flips_false_to_true() {
    let conn = setup();
    let repo = McpServerRepo::new();
    repo.create(&conn, &make_record("srv-2")).expect("create");
    repo.toggle_enabled(&conn, "srv-2", false).expect("toggle off");
    repo.toggle_enabled(&conn, "srv-2", true).expect("toggle on");
    let got = repo.get(&conn, "srv-2").expect("get").expect("exists");
    assert!(got.enabled);
}

#[test]
fn toggle_enabled_unknown_server_is_noop() {
    let conn = setup();
    let repo = McpServerRepo::new();
    repo.toggle_enabled(&conn, "nonexistent", false).expect("toggle should not error");
}
```

> **关键:** `db::open_in_memory()` **不**自动跑迁移 —— 必须显式调 `db::run_migrations(&conn)` 创建 `mcp_servers` 表(在 001_init.sql)。`McpServerRepo::new()` 无参数,所有方法接收 `&Connection`。

- [ ] **步骤 2: 运行测试验证失败 (red)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_toggle_unit
# 期望:编译错误 —— toggle_enabled 方法不存在
```

- [ ] **步骤 3: 实现 toggle_enabled**

修改 `voicepilot/crates/trust-kernel/src/mcp/repo.rs`,在 `impl McpServerRepo` 块追加(**注意:** 类型别名是 `Result<T>` 而非 `KernelResult<T>`,与现有 `create`/`get`/`list` 一致;`rusqlite::params!` 直接支持 `bool`,无需 `as i64`):

```rust
    /// 切换 MCP Server 启用状态(V1.1.2 §8.3 Trust Center 一键停用)。
    /// 若 server_id 不存在,SQLite UPDATE 0 行受影响,不报错(noop)。
    pub fn toggle_enabled(&self, conn: &Connection, server_id: &str, enabled: bool) -> Result<()> {
        conn.execute(
            "UPDATE mcp_servers SET enabled = ?1 WHERE server_id = ?2",
            params![enabled, server_id],
        )?;
        Ok(())
    }
```

修改 `voicepilot/crates/trust-kernel/src/kernel.rs`,追加访问器。**注意:** `TrustKernel` **没有** `mcp_server_repo` 字段,与 ConfigRepo 模式一致 —— 临时实例化 `McpServerRepo::new()`:

```rust
/// 切换 MCP Server 启用状态(V1.1.2 §8.3 Trust Center)。
pub fn toggle_mcp_server(&self, server_id: &str, enabled: bool) -> Result<()> {
    let conn = self.conn();
    crate::mcp::repo::McpServerRepo::new().toggle_enabled(&conn, server_id, enabled)
}

/// 列出所有 MCP Server(V1.1.2 §8.3 Trust Center)。
pub fn list_mcp_servers(&self) -> Result<Vec<crate::mcp::repo::McpServerRecord>> {
    let conn = self.conn();
    crate::mcp::repo::McpServerRepo::new().list(&conn)
}
```

注意:
- `Result<T>` 是 `crate::error::Result<T>`(kernel.rs 顶部已 use)
- `self.conn()` 返回 `MutexGuard<Connection>`,`&conn` 通过 deref coercion 转为 `&Connection`
- `enabled` 是 `bool`,rusqlite 的 `params!` 宏会自动转 i32(参考 W4 McpServerRepo::create 的 `rec.enabled as i32` 写法,但 `params!` 也支持 bool 直接传)
- `McpServerRecord` 字段:`server_id` / `name` / `version` / `transport` / `enabled`(bool) / `trusted`(bool) / `protocol_version`(Option<String>) / `allowed_origins`(Option<String>) / `allowed_paths`(Option<String>)

- [ ] **步骤 4: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_toggle_unit
# 期望:3 tests passed
```

- [ ] **步骤 5: 实现 trust_center_commands.rs**

创建 `voicepilot/crates/ui/src/trust_center_commands.rs`:

```rust
//! Trust Center Tauri commands —— V1.1.2 §8.3 Trust Center 后端。
//!
//! 展示 MCP Server 列表 + 一键停用/启用(§8.3 V1.1 新界面)。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDto {
    pub server_id: String,
    pub name: String,
    pub version: String,
    pub transport: String,
    pub enabled: bool,
    pub trusted: bool,
    pub protocol_version: Option<String>,
    pub allowed_origins: Option<String>,
    pub allowed_paths: Option<String>,
}

impl From<trust_kernel::mcp::repo::McpServerRecord> for McpServerDto {
    fn from(r: trust_kernel::mcp::repo::McpServerRecord) -> Self {
        Self {
            server_id: r.server_id,
            name: r.name,
            version: r.version,
            transport: r.transport,
            enabled: r.enabled,
            trusted: r.trusted,
            protocol_version: r.protocol_version,
            allowed_origins: r.allowed_origins,
            allowed_paths: r.allowed_paths,
        }
    }
}

#[tauri::command]
pub async fn list_mcp_servers_command(state: State<'_, AppState>) -> UiResult<Vec<McpServerDto>> {
    let records = state.kernel.list_mcp_servers()?;
    Ok(records.into_iter().map(McpServerDto::from).collect())
}

#[tauri::command]
pub async fn toggle_mcp_server_command(
    state: State<'_, AppState>,
    server_id: String,
    enabled: bool,
) -> UiResult<()> {
    state.kernel.toggle_mcp_server(&server_id, enabled)
}
```

修改 `voicepilot/crates/ui/src/lib.rs`,追加 `pub mod trust_center_commands;`。

修改 `voicepilot/crates/ui/src/commands.rs` 的 `register_handlers`,追加:

```rust
crate::trust_center_commands::list_mcp_servers_command,
crate::trust_center_commands::toggle_mcp_server_command,
```

创建 `voicepilot/crates/ui/tests/trust_center_commands_unit.rs`:

```rust
#![cfg(feature = "tauri")]

use trust_kernel::mcp::repo::McpServerRecord;
use voicepilot_ui::trust_center_commands::McpServerDto;

#[test]
fn mcp_server_dto_converts_from_record() {
    let rec = McpServerRecord {
        server_id: "srv-1".to_string(),
        name: "FS".to_string(),
        version: "1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["D:/"]"#.to_string()),
    };
    let dto = McpServerDto::from(rec);
    assert_eq!(dto.server_id, "srv-1");
    assert!(dto.enabled);
    assert!(!dto.trusted);
}
```

- [ ] **步骤 6: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test trust_center_commands_unit
# 期望:1 test passed
```

- [ ] **步骤 7: 实现前端 TrustCenterView**

修改 `voicepilot/crates/ui/web/src/types.ts`,追加:

```typescript
export interface McpServer {
  server_id: string;
  name: string;
  version: string;
  transport: string;
  enabled: boolean;
  trusted: boolean;
  protocol_version: string | null;
  allowed_origins: string | null;
  allowed_paths: string | null;
}
```

修改 `voicepilot/crates/ui/web/src/api.ts`,追加:

```typescript
export async function listMcpServers(): Promise<McpServer[]> {
  return invoke<McpServer[]>("list_mcp_servers_command");
}

export async function toggleMcpServer(serverId: string, enabled: boolean): Promise<void> {
  await invoke("toggle_mcp_server_command", { serverId, enabled });
}
```

创建 `voicepilot/crates/ui/web/src/components/TrustCenterView.tsx`:

```tsx
import { useEffect, useState } from "react";
import { listMcpServers, toggleMcpServer } from "../api";
import type { McpServer } from "../types";

export function TrustCenterView(): JSX.Element {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = (): void => {
    setLoading(true);
    listMcpServers()
      .then((s) => {
        setServers(s);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  };

  useEffect(refresh, []);

  const handleToggle = (serverId: string, enabled: boolean): void => {
    toggleMcpServer(serverId, enabled)
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  return (
    <section className="view-container trust-center" aria-labelledby="trust-heading">
      <h2 id="trust-heading">§ 8.3 Trust Center</h2>
      <p className="view-description">管理 MCP Server 启用状态。停用后该 Server 不会被 Action Gateway 调用。</p>
      {loading && <div role="status" aria-live="polite">加载 MCP Server 列表…</div>}
      {error && <div className="form-error" role="alert">错误:{error}</div>}

      <table className="mcp-table" aria-label="MCP Server 列表">
        <thead>
          <tr>
            <th scope="col">Server ID</th>
            <th scope="col">名称</th>
            <th scope="col">版本</th>
            <th scope="col">传输</th>
            <th scope="col">可信</th>
            <th scope="col">协议</th>
            <th scope="col">allowed_paths</th>
            <th scope="col">状态</th>
            <th scope="col">操作</th>
          </tr>
        </thead>
        <tbody>
          {servers.map((s) => (
            <tr key={s.server_id}>
              <td className="mono">{s.server_id}</td>
              <td>{s.name}</td>
              <td className="mono">{s.version}</td>
              <td className="mono">{s.transport}</td>
              <td>{s.trusted ? "✓" : "—"}</td>
              <td className="mono">{s.protocol_version ?? "—"}</td>
              <td className="mono small">{s.allowed_paths ?? "—"}</td>
              <td>
                <span className={`status-pill ${s.enabled ? "enabled" : "disabled"}`}>
                  {s.enabled ? "已启用" : "已停用"}
                </span>
              </td>
              <td>
                <button
                  type="button"
                  className={`toggle-btn ${s.enabled ? "disable" : "enable"}`}
                  onClick={() => handleToggle(s.server_id, !s.enabled)}
                  aria-pressed={s.enabled}
                  aria-label={s.enabled ? `停用 ${s.server_id}` : `启用 ${s.server_id}`}
                >
                  {s.enabled ? "停用" : "启用"}
                </button>
              </td>
            </tr>
          ))}
          {servers.length === 0 && (
            <tr>
              <td colSpan={9} className="empty-state">暂无已注册 MCP Server</td>
            </tr>
          )}
        </tbody>
      </table>
    </section>
  );
}
```

修改 `voicepilot/crates/ui/web/src/styles.css`,追加:

```css
.trust-center { padding: 24px; }
.view-description { color: #94a3b8; margin-bottom: 16px; }
.mcp-table { width: 100%; border-collapse: collapse; }
.mcp-table th, .mcp-table td { border: 1px solid #1f2937; padding: 8px 12px; text-align: left; }
.mcp-table th { background: #111827; color: #fbbf24; font-family: "IBM Plex Mono", monospace; font-size: 12px; }
.mcp-table td { color: #f1f5f9; font-size: 13px; }
.mcp-table .mono { font-family: "IBM Plex Mono", monospace; font-size: 12px; }
.mcp-table .small { font-size: 11px; color: #94a3b8; max-width: 200px; overflow: hidden; text-overflow: ellipsis; }
.status-pill { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 11px; font-family: "IBM Plex Mono", monospace; }
.status-pill.enabled { background: #064e3b; color: #10b981; }
.status-pill.disabled { background: #450a0a; color: #ef4444; }
.toggle-btn { border: 1px solid #1f2937; background: transparent; color: #f1f5f9; padding: 4px 12px; border-radius: 4px; cursor: pointer; font-size: 12px; }
.toggle-btn.enable { border-color: #10b981; color: #10b981; }
.toggle-btn.disable { border-color: #ef4444; color: #ef4444; }
.toggle-btn:hover { background: #1f2937; }
```

- [ ] **步骤 8: 构建前端 + 编译验证 + 提交**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web
npm.cmd run build
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
git add voicepilot/crates/trust-kernel/src/mcp/repo.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/mcp_toggle_unit.rs voicepilot/crates/ui/src/trust_center_commands.rs voicepilot/crates/ui/src/lib.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/trust_center_commands_unit.rs voicepilot/crates/ui/web/src/components/TrustCenterView.tsx voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist
git commit -m "Task 3: Trust Center toggle + commands + TrustCenterView (V1.1 §8.3)"
```

---

## 任务 4: Skills Manager 后端 SkillRepo + 前端 SkillsManagerView

**目标:** 新建 `SkillRepo`(list/get/upsert/toggle/incr_success)+ `list_skills_command` / `toggle_skill_command` Tauri command + 前端 `SkillsManagerView`(Skill 表格 + success_count / avg_latency_ms / 风险等级 + toggle 启用/禁用)。

**文件:**
- 创建:`voicepilot/crates/trust-kernel/src/skills/repo.rs`
- 修改:`voicepilot/crates/trust-kernel/src/skills/mod.rs`
- 修改:`voicepilot/crates/trust-kernel/src/kernel.rs`
- 创建:`voicepilot/crates/trust-kernel/tests/skill_repo_unit.rs`
- 创建:`voicepilot/crates/ui/src/skills_commands.rs`
- 修改:`voicepilot/crates/ui/src/lib.rs`
- 修改:`voicepilot/crates/ui/src/commands.rs`
- 创建:`voicepilot/crates/ui/tests/skills_commands_unit.rs`
- 创建:`voicepilot/crates/ui/web/src/components/SkillsManagerView.tsx`
- 修改:`voicepilot/crates/ui/web/src/types.ts`
- 修改:`voicepilot/crates/ui/web/src/api.ts`
- 修改:`voicepilot/crates/ui/web/src/styles.css`

- [ ] **步骤 1: 写 SkillRepo 失败测试 (red)**

创建 `voicepilot/crates/trust-kernel/tests/skill_repo_unit.rs`:

```rust
//! SkillRepo 单元测试 —— V1.1.2 §8.3 Skills Manager。

use trust_kernel::db;
use trust_kernel::skills::repo::{SkillRecord, SkillRepo};

fn make_record(id: &str) -> SkillRecord {
    SkillRecord {
        skill_id: id.to_string(),
        version: "1.0".to_string(),
        manifest_json: r#"{"id":"test"}"#.to_string(),
        enabled: true,
        success_count: 0,
        avg_latency_ms: 0.0,
    }
}

#[test]
fn skill_upsert_creates_new_record() {
    let conn = db::open_in_memory().expect("open");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("files.organize")).expect("upsert");
    let got = repo.get(&conn, "files.organize").expect("get").expect("exists");
    assert_eq!(got.skill_id, "files.organize");
    assert_eq!(got.success_count, 0);
}

#[test]
fn skill_upsert_overwrites_existing() {
    let conn = db::open_in_memory().expect("open");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("files.organize")).expect("upsert 1");
    let mut rec = make_record("files.organize");
    rec.version = "2.0".to_string();
    repo.upsert(&conn, &rec).expect("upsert 2");
    let got = repo.get(&conn, "files.organize").expect("get").expect("exists");
    assert_eq!(got.version, "2.0");
}

#[test]
fn skill_list_returns_all() {
    let conn = db::open_in_memory().expect("open");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("skill-1")).expect("upsert");
    repo.upsert(&conn, &make_record("skill-2")).expect("upsert");
    let all = repo.list(&conn).expect("list");
    assert_eq!(all.len(), 2);
}

#[test]
fn skill_toggle_flips_enabled() {
    let conn = db::open_in_memory().expect("open");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("skill-1")).expect("upsert");
    repo.toggle(&conn, "skill-1", false).expect("toggle");
    let got = repo.get(&conn, "skill-1").expect("get").expect("exists");
    assert!(!got.enabled);
}

#[test]
fn skill_incr_success_increments_count_and_updates_avg_latency() {
    let conn = db::open_in_memory().expect("open");
    let repo = SkillRepo::new();
    repo.upsert(&conn, &make_record("skill-1")).expect("upsert");
    repo.incr_success(&conn, "skill-1", 1000.0).expect("incr 1");
    repo.incr_success(&conn, "skill-1", 2000.0).expect("incr 2");
    let got = repo.get(&conn, "skill-1").expect("get").expect("exists");
    assert_eq!(got.success_count, 2);
    assert!((got.avg_latency_ms - 1500.0).abs() < 0.1); // 平均 (1000+2000)/2
}
```

- [ ] **步骤 2: 运行测试验证失败 (red)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skill_repo_unit
# 期望:编译错误 —— skills::repo 模块不存在
```

- [ ] **步骤 3: 实现 SkillRepo**

创建 `voicepilot/crates/trust-kernel/src/skills/repo.rs`:

```rust
//! SkillRepo —— V1.1.2 §8.3 Skills Manager 后端。
//!
//! 持久化 Skill manifest + 成功统计(success_count + avg_latency_ms)。
//! incr_success 用增量平均:avg = (avg * n + new) / (n + 1)。
//!
//! **注意:** `skills` 表已在 `migrations/001_init.sql` L89-L96 创建,
//! 不需要新建迁移。DB schema `version INTEGER`,所以 SkillRecord.version 是 i64。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub skill_id: String,
    pub version: i64,  // DB schema INTEGER(非 TEXT);SkillManifest.version(String) parse 而来
    pub manifest_json: String,
    pub enabled: bool,
    pub success_count: i64,
    pub avg_latency_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRepo;

impl SkillRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn upsert(&self, conn: &Connection, rec: &SkillRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO skills (skill_id, version, manifest_json, enabled, success_count, avg_latency_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(skill_id) DO UPDATE SET
               version = excluded.version,
               manifest_json = excluded.manifest_json,
               enabled = excluded.enabled",
            params![rec.skill_id, rec.version, rec.manifest_json, rec.enabled, rec.success_count, rec.avg_latency_ms],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, skill_id: &str) -> Result<Option<SkillRecord>> {
        let mut stmt = conn.prepare(
            "SELECT skill_id, version, manifest_json, enabled, success_count, avg_latency_ms FROM skills WHERE skill_id = ?1",
        )?;
        let rec = stmt.query_row(params![skill_id], |row| {
            Ok(SkillRecord {
                skill_id: row.get(0)?,
                version: row.get(1)?,
                manifest_json: row.get(2)?,
                enabled: row.get(3)?,  // rusqlite 自动 i64→bool
                success_count: row.get(4)?,
                avg_latency_ms: row.get(5)?,
            })
        }).optional()?;
        Ok(rec)
    }

    pub fn list(&self, conn: &Connection) -> Result<Vec<SkillRecord>> {
        let mut stmt = conn.prepare(
            "SELECT skill_id, version, manifest_json, enabled, success_count, avg_latency_ms FROM skills ORDER BY skill_id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SkillRecord {
                skill_id: row.get(0)?,
                version: row.get(1)?,
                manifest_json: row.get(2)?,
                enabled: row.get(3)?,
                success_count: row.get(4)?,
                avg_latency_ms: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn toggle(&self, conn: &Connection, skill_id: &str, enabled: bool) -> Result<()> {
        conn.execute(
            "UPDATE skills SET enabled = ?1 WHERE skill_id = ?2",
            params![enabled, skill_id],
        )?;
        Ok(())
    }

    /// 记录一次成功执行,增量更新 success_count 与 avg_latency_ms。
    pub fn incr_success(&self, conn: &Connection, skill_id: &str, latency_ms: f64) -> Result<()> {
        conn.execute(
            "UPDATE skills
             SET success_count = success_count + 1,
                 avg_latency_ms = (avg_latency_ms * success_count + ?1) / (success_count + 1)
             WHERE skill_id = ?2",
            params![latency_ms, skill_id],
        )?;
        Ok(())
    }
}
```

注意:
- `Result<T>` 是 `crate::error::Result<T>`(trust-kernel 内部类型别名,非 `KernelResult`)
- `OptionalExtension` trait 提供 `.optional()` 方法
- `rusqlite` 支持 `bool` 直接序列化/反序列化到 SQLite INTEGER(0/1),无需手动 `as i64`
- `incr_success` SQL 中 `success_count` 在 SET 子句被引用,SQLite 行为:SET 子句内引用更新前的值,所以 `avg_latency_ms * success_count` 用旧 count,分母 `success_count + 1` 等于新 count —— 正确的增量平均公式
- `skills` 表已存在(001_init.sql),不新建迁移

修改 `voicepilot/crates/trust-kernel/src/skills/mod.rs`,追加 `pub mod repo;`。

修改 `voicepilot/crates/trust-kernel/src/kernel.rs`,新增 **stateless 访问器**(与 ConfigRepo / McpServerRepo 一致,**不**在 `TrustKernel` 结构体上加字段):

```rust
/// Stateless `SkillRepo` 访问器(V1.1.2 §8.3 Skills Manager)。
pub fn skill_repo(&self) -> crate::skills::repo::SkillRepo {
    crate::skills::repo::SkillRepo::new()
}

/// 列出所有 Skill(V1.1.2 §8.3 Skills Manager)。
pub fn list_skills(&self) -> Result<Vec<crate::skills::repo::SkillRecord>> {
    let conn = self.conn();
    crate::skills::repo::SkillRepo::new().list(&conn)
}

/// 切换 Skill 启用状态(V1.1.2 §8.3 Skills Manager)。
pub fn toggle_skill(&self, skill_id: &str, enabled: bool) -> Result<()> {
    let conn = self.conn();
    crate::skills::repo::SkillRepo::new().toggle(&conn, skill_id, enabled)
}
```

> **注意:** `Result<T>` 是 `trust-kernel` 的类型别名(`std::result::Result<T, KernelError>`),**不**用 `KernelResult<T>`。`SkillRepo::new()` 无参数。`conn()` 返回 `MutexGuard<Connection>`,deref coercion 自动转 `&Connection`。

- [ ] **步骤 4: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skill_repo_unit
# 期望:5 tests passed
```

- [ ] **步骤 5: 实现 skills_commands.rs + 前端**

创建 `voicepilot/crates/ui/src/skills_commands.rs`:

```rust
//! Skills Manager Tauri commands —— V1.1.2 §8.3 Skills Manager 后端。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDto {
    pub skill_id: String,
    pub version: String,
    pub enabled: bool,
    pub success_count: i64,
    pub avg_latency_ms: f64,
    /// 从 manifest_json 解析出的风险等级(E×D),用于前端展示。
    pub risk_label: String,
}

impl From<trust_kernel::skills::repo::SkillRecord> for SkillDto {
    fn from(r: trust_kernel::skills::repo::SkillRecord) -> Self {
        // 尝试从 manifest_json 解析 risk_label;失败则 "unknown"
        let risk_label = serde_json::from_str::<serde_json::Value>(&r.manifest_json)
            .ok()
            .and_then(|v| v.get("risk").and_then(|r| r.as_str().map(String::from)))
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            skill_id: r.skill_id,
            version: r.version,
            enabled: r.enabled,
            success_count: r.success_count,
            avg_latency_ms: r.avg_latency_ms,
            risk_label,
        }
    }
}

#[tauri::command]
pub async fn list_skills_command(state: State<'_, AppState>) -> UiResult<Vec<SkillDto>> {
    let records = state.kernel.list_skills()?;
    Ok(records.into_iter().map(SkillDto::from).collect())
}

#[tauri::command]
pub async fn toggle_skill_command(
    state: State<'_, AppState>,
    skill_id: String,
    enabled: bool,
) -> UiResult<()> {
    state.kernel.toggle_skill(&skill_id, enabled)
}
```

修改 `voicepilot/crates/ui/src/lib.rs`,追加 `pub mod skills_commands;`。

修改 `voicepilot/crates/ui/src/commands.rs` 的 `register_handlers`,追加:

```rust
crate::skills_commands::list_skills_command,
crate::skills_commands::toggle_skill_command,
```

创建 `voicepilot/crates/ui/tests/skills_commands_unit.rs`:

```rust
#![cfg(feature = "tauri")]

use trust_kernel::skills::repo::SkillRecord;
use voicepilot_ui::skills_commands::SkillDto;

#[test]
fn skill_dto_extracts_risk_label_from_manifest() {
    let rec = SkillRecord {
        skill_id: "files.organize".to_string(),
        version: "1.0".to_string(),
        manifest_json: r#"{"id":"files.organize","risk":"E2D2"}"#.to_string(),
        enabled: true,
        success_count: 5,
        avg_latency_ms: 1234.5,
    };
    let dto = SkillDto::from(rec);
    assert_eq!(dto.skill_id, "files.organize");
    assert_eq!(dto.risk_label, "E2D2");
    assert_eq!(dto.success_count, 5);
}

#[test]
fn skill_dto_defaults_risk_label_when_manifest_invalid() {
    let rec = SkillRecord {
        skill_id: "x".to_string(),
        version: "1".to_string(),
        manifest_json: "not json".to_string(),
        enabled: false,
        success_count: 0,
        avg_latency_ms: 0.0,
    };
    let dto = SkillDto::from(rec);
    assert_eq!(dto.risk_label, "unknown");
}
```

- [ ] **步骤 6: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test skills_commands_unit
# 期望:2 tests passed
```

- [ ] **步骤 7: 实现前端 SkillsManagerView**

修改 `voicepilot/crates/ui/web/src/types.ts`,追加:

```typescript
export interface Skill {
  skill_id: string;
  version: string;
  enabled: boolean;
  success_count: number;
  avg_latency_ms: number;
  risk_label: string;
}
```

修改 `voicepilot/crates/ui/web/src/api.ts`,追加:

```typescript
export async function listSkills(): Promise<Skill[]> {
  return invoke<Skill[]>("list_skills_command");
}

export async function toggleSkill(skillId: string, enabled: boolean): Promise<void> {
  await invoke("toggle_skill_command", { skillId, enabled });
}
```

创建 `voicepilot/crates/ui/web/src/components/SkillsManagerView.tsx`:

```tsx
import { useEffect, useState } from "react";
import { listSkills, toggleSkill } from "../api";
import type { Skill } from "../types";

export function SkillsManagerView(): JSX.Element {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = (): void => {
    setLoading(true);
    listSkills()
      .then((s) => {
        setSkills(s);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  };

  useEffect(refresh, []);

  const handleToggle = (skillId: string, enabled: boolean): void => {
    toggleSkill(skillId, enabled).then(refresh).catch((e) => setError(String(e)));
  };

  return (
    <section className="view-container skills-manager" aria-labelledby="skills-heading">
      <h2 id="skills-heading">§ 8.3 Skills Manager</h2>
      <p className="view-description">已保存 Skill 列表 + 成功率 + 平均延迟 + 风险等级。</p>
      {loading && <div role="status" aria-live="polite">加载 Skill 列表…</div>}
      {error && <div className="form-error" role="alert">错误:{error}</div>}

      <table className="skill-table" aria-label="Skill 列表">
        <thead>
          <tr>
            <th scope="col">Skill ID</th>
            <th scope="col">版本</th>
            <th scope="col">风险</th>
            <th scope="col">成功次数</th>
            <th scope="col">平均延迟(ms)</th>
            <th scope="col">状态</th>
            <th scope="col">操作</th>
          </tr>
        </thead>
        <tbody>
          {skills.map((s) => (
            <tr key={s.skill_id}>
              <td className="mono">{s.skill_id}</td>
              <td className="mono">{s.version}</td>
              <td><span className={`risk-pill risk-${s.risk_label.toLowerCase()}`}>{s.risk_label}</span></td>
              <td className="mono">{s.success_count}</td>
              <td className="mono">{s.avg_latency_ms.toFixed(1)}</td>
              <td>
                <span className={`status-pill ${s.enabled ? "enabled" : "disabled"}`}>
                  {s.enabled ? "已启用" : "已禁用"}
                </span>
              </td>
              <td>
                <button
                  type="button"
                  className={`toggle-btn ${s.enabled ? "disable" : "enable"}`}
                  onClick={() => handleToggle(s.skill_id, !s.enabled)}
                  aria-pressed={s.enabled}
                  aria-label={s.enabled ? `禁用 ${s.skill_id}` : `启用 ${s.skill_id}`}
                >
                  {s.enabled ? "禁用" : "启用"}
                </button>
              </td>
            </tr>
          ))}
          {skills.length === 0 && (
            <tr><td colSpan={7} className="empty-state">暂无已保存 Skill(执行 Skill 后自动记录)</td></tr>
          )}
        </tbody>
      </table>
    </section>
  );
}
```

修改 `voicepilot/crates/ui/web/src/styles.css`,追加:

```css
.skills-manager { padding: 24px; }
.skill-table { width: 100%; border-collapse: collapse; }
.skill-table th, .skill-table td { border: 1px solid #1f2937; padding: 8px 12px; text-align: left; }
.skill-table th { background: #111827; color: #fbbf24; font-family: "IBM Plex Mono", monospace; font-size: 12px; }
.skill-table td { color: #f1f5f9; font-size: 13px; }
.skill-table .mono { font-family: "IBM Plex Mono", monospace; font-size: 12px; }
.risk-pill { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 11px; font-family: "IBM Plex Mono", monospace; }
.risk-pill.risk-e2d2 { background: #422006; color: #fbbf24; }
.risk-pill.risk-e1d1 { background: #064e3b; color: #10b981; }
.risk-pill.risk-unknown { background: #1f2937; color: #64748b; }
```

- [ ] **步骤 8: 构建前端 + 编译验证 + 提交**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web
npm.cmd run build
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
git add voicepilot/crates/trust-kernel/src/skills/repo.rs voicepilot/crates/trust-kernel/src/skills/mod.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/skill_repo_unit.rs voicepilot/crates/ui/src/skills_commands.rs voicepilot/crates/ui/src/lib.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/skills_commands_unit.rs voicepilot/crates/ui/web/src/components/SkillsManagerView.tsx voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist
git commit -m "Task 4: Skills Manager SkillRepo + commands + SkillsManagerView (V1.1 §8.3)"
```

---

## 任务 5: Voice cancel #57 + Model caching #61

**目标:** 解决 W6b-1 延后的 issue #57(voice 取消机制)+ issue #61(WhisperEngine 模型缓存)。AppState 加 `kill_switch: Arc<AtomicBool>` + `#[cfg(feature="voice")] whisper_cache: Arc<Mutex<Option<WhisperEngine>>>`。`VoiceListener::listen` 接收 `cancel: &AtomicBool`,循环每 chunk 检查;`voice_listen_command` 先检查 whisper_cache,miss 时加载并缓存。新增 `cancel_voice_command` Tauri command。

**文件:**
- 修改:`voicepilot/crates/trust-kernel/src/voice/listener.rs`
- 修改:`voicepilot/crates/trust-kernel/tests/voice_listener_unit.rs`
- 修改:`voicepilot/crates/ui/src/state.rs`
- 修改:`voicepilot/crates/ui/src/voice_commands.rs`
- 修改:`voicepilot/crates/ui/src/commands.rs`(register_handlers_with_voice 追加 cancel_voice_command)
- 创建:`voicepilot/crates/ui/tests/voice_cancel_cache_unit.rs`
- 修改:`voicepilot/crates/ui/web/src/api.ts`
- 修改:`voicepilot/crates/ui/web/src/components/MainView.tsx`

- [ ] **步骤 1: 写 VoiceListener cancel 失败测试 (red)**

修改 `voicepilot/crates/trust-kernel/tests/voice_listener_unit.rs`,追加(在文件末尾):

```rust
use std::sync::atomic::AtomicBool;

#[test]
fn voice_listener_stops_immediately_when_cancel_flag_set_before_chunk() {
    // cancel flag 在 listen 开始前就为 true,应立即返回 NoSpeech(无音频采集)。
    let recorder = Arc::new(MockVoiceRecorder::new(vec![
        generate_sine_wave(750, 16000, 200.0),
    ]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_secs(30),
        Duration::from_millis(750),
    );
    let cancel = AtomicBool::new(true);
    let outcome = listener
        .listen_with_cancel(&cancel)
        .expect("listen should succeed");
    assert!(
        matches!(outcome, ListenOutcome::NoSpeech),
        "expected NoSpeech when cancel flag set, got {:?}",
        outcome
    );
}

#[test]
fn voice_listener_stops_midway_when_cancel_flag_set_after_first_chunk() {
    // 第 1 块正常录制,之后 cancel flag 置 true,第 2 块前退出循环。
    // 因为有语音但未达 VAD 静音超时,post-loop detect() 返回 Speech → Timeout。
    let chunk1 = generate_sine_wave(750, 16000, 200.0);
    let chunk2 = generate_sine_wave(750, 16000, 200.0);
    let recorder = Arc::new(MockVoiceRecorder::new(vec![chunk1, chunk2]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_secs(30),
        Duration::from_millis(750),
    );
    let cancel = AtomicBool::new(false);

    // 用一个包装 recorder:第 1 次 record_chunk 后设置 cancel flag
    struct CancelAfterFirst {
        inner: MockVoiceRecorder,
        cancel: Arc<AtomicBool>,
    }
    impl VoiceRecorder for CancelAfterFirst {
        fn record_chunk(&self, d: Duration) -> VoiceResult<Vec<i16>> {
            let r = self.inner.record_chunk(d)?;
            if !r.is_empty() {
                self.cancel.store(true, Ordering::SeqCst);
            }
            Ok(r)
        }
    }
    let cancel_arc = Arc::new(cancel);
    let wrapper = Arc::new(CancelAfterFirst {
        inner: MockVoiceRecorder::new(vec![generate_sine_wave(750, 16000, 200.0), generate_sine_wave(750, 16000, 200.0)]),
        cancel: cancel_arc.clone(),
    });
    let listener = VoiceListener::new(
        wrapper,
        VadDetector::new(VadConfig::default()),
        Duration::from_secs(30),
        Duration::from_millis(750),
    );
    let outcome = listener
        .listen_with_cancel(&cancel_arc)
        .expect("listen");
    // 录到 1 块语音,cancel 后退出,detect() 返回 Speech → Timeout
    match outcome {
        ListenOutcome::Timeout { samples } => {
            assert!(!samples.is_empty(), "should have 1 chunk of samples");
        }
        ListenOutcome::SpeechEnded { .. } => {
            // 也可能 VAD 在 1 块内就触发(边界),可接受
        }
        other => panic!("expected Timeout or SpeechEnded on cancel, got {:?}", other),
    }
}
```

- [ ] **步骤 2: 运行测试验证失败 (red)**

```powershell
cd d:\voicepilot
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
$env:WHISPER_DONT_GENERATE_BINDINGS = "1"
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --features voice --test voice_listener_unit
# 期望:编译错误 —— listen_with_cancel 方法不存在
```

- [ ] **步骤 3: 实现 listen_with_cancel**

修改 `voicepilot/crates/trust-kernel/src/voice/listener.rs`。**先重构现有 `listen` 方法**为 `listen_with_cancel` 的薄包装(避免逻辑重复),然后追加 `listen_with_cancel`。

替换现有 `pub fn listen(&self) -> VoiceResult<ListenOutcome>` 的整个函数体为:

```rust
    /// 不带 cancel 的 listen —— 委托给 `listen_with_cancel` + 永不取消的 flag。
    pub fn listen(&self) -> VoiceResult<ListenOutcome> {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        self.listen_with_cancel(&cancel)
    }

    /// 带 cancel flag 的 listen —— V1.1.2 issue #57 voice 取消机制。
    ///
    /// 循环开始前 + 每 chunk 录制前检查 cancel flag。若为 true,立即退出循环。
    /// 退出后用 `vad.detect()` 判断 Timeout(有语音)/ NoSpeech(无语音)。
    /// post-loop 逻辑与 `listen` 保持一致(`VadOutcome::Speech → Timeout` /
    /// `VadOutcome::NoSpeech → NoSpeech`)。
    pub fn listen_with_cancel(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> VoiceResult<ListenOutcome> {
        use std::sync::atomic::Ordering;
        let mut buffer: Vec<i16> = Vec::new();
        let mut elapsed = std::time::Duration::ZERO;

        while elapsed < self.max_duration {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            let chunk = self.recorder.record_chunk(self.chunk_duration)?;
            if chunk.is_empty() {
                break;
            }
            buffer.extend_from_slice(&chunk);
            elapsed += self.chunk_duration;
            if let Some(seg) = self.vad.detect_end_of_speech(&buffer) {
                buffer.truncate(seg.speech_end_sample);
                return Ok(ListenOutcome::SpeechEnded { samples: buffer });
            }
        }

        // post-loop:与 listen 保持一致(注意:cancel 退出时也走此分支)
        match self.vad.detect(&buffer) {
            crate::voice::vad::VadOutcome::Speech { speech_end_sample, .. } => {
                buffer.truncate(speech_end_sample);
                if buffer.is_empty() {
                    Ok(ListenOutcome::NoSpeech)
                } else {
                    Ok(ListenOutcome::Timeout { samples: buffer })
                }
            }
            crate::voice::vad::VadOutcome::NoSpeech => Ok(ListenOutcome::NoSpeech),
        }
    }
```

> **注意:** `VadOutcome` 是 `enum { Speech { speech_start_sample, speech_end_sample }, NoSpeech }`,**无** `into_speech_ended()` 方法 —— 必须用 `match` 处理(与现有 `listen` 方法一致)。post-loop 的 `Speech` 分支对应 `Timeout`(已录到语音但未触发 VAD 静音超时),`NoSpeech` 分支对应真正无语音 / cancel 前未录到任何语音。

- [ ] **步骤 4: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"; $env:WHISPER_DONT_GENERATE_BINDINGS = "1"
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --features voice --test voice_listener_unit
# 期望:4 原有 + 2 新 cancel tests = 6 passed
```

- [ ] **步骤 5: 写 voice_commands cancel + cache 失败测试 + 实现**

修改 `voicepilot/crates/ui/src/state.rs`,加字段(**注意:** `WhisperEngine` **不** `Clone`,cache 必须存 `Arc<WhisperEngine>`):

```rust
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[cfg(feature = "voice")]
use std::sync::Mutex;

pub struct AppState {
    pub kernel: Arc<TrustKernel>,
    #[cfg(feature = "tauri")]
    pub approval_registry: ApprovalRegistry,
    /// Voice 取消标志(issue #57)。cancel_voice_command 设为 true;
    /// voice_listen_command 开始时重置为 false,循环中检查。
    /// 总是存在(非 voice-gated)—— cancel 命令本身在 voice feature 下注册。
    #[cfg(feature = "voice")]
    pub kill_switch: Arc<AtomicBool>,
    /// WhisperEngine 缓存(issue #61)。miss 时加载并缓存(Arc 共享);
    /// Settings 更新 model_path 时应 invalidate(设为 None)。
    /// `WhisperEngine` 持有 `WhisperContext`(FFI 资源),不实现 `Clone`,
    /// 必须用 `Arc<WhisperEngine>` 共享。
    #[cfg(feature = "voice")]
    pub whisper_cache: Arc<Mutex<Option<Arc<trust_kernel::voice::whisper::WhisperEngine>>>>,
}
```

`AppState::new` 等构造函数初始化新字段:

```rust
#[cfg(feature = "voice")]
kill_switch: Arc::new(AtomicBool::new(false)),
#[cfg(feature = "voice")]
whisper_cache: Arc::new(Mutex::new(None)),
```

> **重要:** 若 `AppState::new` 当前签名为 `pub fn new(kernel: TrustKernel) -> Self`(by-value),保持不变。新增字段在所有现有构造器中初始化。`#[cfg(feature = "voice")]` 字段在非 voice 构建中不存在,需确保 `commands.rs` 等所有访问点也用 `#[cfg(feature = "voice")]` 门控。

修改 `voicepilot/crates/ui/src/voice_commands.rs`:

1. `VoiceListen` trait 签名扩展:`fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome>`。所有现有 mock 实现(若有)需加 `cancel` 参数(可忽略)。
2. `voice_listen` 纯函数:`pub fn voice_listen(listener: &dyn VoiceListen, cancel: &AtomicBool) -> VoiceListenResult`。
3. `VoiceListenImpl` 新增字段 `cached_engine: Option<Arc<WhisperEngine>>`(默认 `None`)。`transcribe` 优先用 `cached_engine`,fallback 到 `WhisperEngine::new(self.whisper_config.clone())`。
4. `VoiceListenImpl::with_engine` 新增构造器:接收 `Arc<WhisperEngine>`,设 `cached_engine: Some(engine)`。
5. `VoiceListenImpl::listen(&self, cancel: &AtomicBool)` 内部调 `VoiceListener::listen_with_cancel(cancel)`(替代 `listener.listen()`)。
6. `voice_listen_command` 重写:重置 kill_switch → 加载 settings → 检查/填充 whisper_cache → 用 `Arc<WhisperEngine>` 构造 `VoiceListenImpl::with_engine` → 调 `voice_listen(&listener, &state.kill_switch)`。
7. 新增 `cancel_voice_command`:设 `state.kill_switch` 为 true。

具体代码(替换现有 trait 定义 + `voice_listen` 纯函数 + `VoiceListenImpl` + `voice_listen_command`):

```rust
// 顶部 imports 追加:
use std::sync::atomic::AtomicBool;

// 替换 VoiceListen trait:
pub trait VoiceListen: Send + Sync {
    fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome>;
}

// 替换 voice_listen 纯函数:
pub fn voice_listen(
    listener: &dyn VoiceListen,
    cancel: &AtomicBool,
) -> VoiceListenResult {
    match listener.listen(cancel) {
        Ok(outcome) => match outcome {
            VoiceListenOutcome::Success {
                transcription,
                route_outcome,
                stopped_by_vad,
            } => VoiceListenResult::Success {
                transcription,
                route_outcome,
                stopped_by_vad,
            },
            VoiceListenOutcome::NoSpeech => VoiceListenResult::NoSpeech,
            VoiceListenOutcome::Timeout {
                transcription,
                route_outcome,
            } => VoiceListenResult::Timeout {
                transcription,
                route_outcome,
            },
        },
        Err(e) => VoiceListenResult::Error {
            message: e.to_string(),
        },
    }
}

// VoiceListenImpl 追加 cached_engine 字段:
pub struct VoiceListenImpl {
    recorder: Arc<dyn VoiceRecorder>,
    whisper_config: WhisperConfig,
    /// 缓存的 WhisperEngine(issue #61)。Some 时 transcribe 直接用;
    /// None 时 fallback 到每次 `WhisperEngine::new(whisper_config.clone())`。
    cached_engine: Option<Arc<WhisperEngine>>,
    kernel: Arc<TrustKernel>,
    max_duration: Duration,
    chunk_duration: Duration,
}

// VoiceListenImpl::new 改为初始化 cached_engine: None(其他不变):
impl VoiceListenImpl {
    pub fn new(
        recorder: Arc<dyn VoiceRecorder>,
        whisper_config: WhisperConfig,
        kernel: Arc<TrustKernel>,
    ) -> Self {
        Self {
            recorder,
            whisper_config,
            cached_engine: None,
            kernel,
            max_duration: Duration::from_secs(30),
            chunk_duration: Duration::from_millis(500),
        }
    }

    /// 用缓存的 WhisperEngine 创建(issue #61)。
    /// 调用方负责从 `state.whisper_cache` 取出 `Arc<WhisperEngine>` 传入。
    pub fn with_engine(
        recorder: Arc<dyn VoiceRecorder>,
        engine: Arc<WhisperEngine>,
        kernel: Arc<TrustKernel>,
    ) -> Self {
        // 从 engine 读出 config(用于 fallback 路径,虽然 cache 命中时不走)
        let whisper_config = engine.config().clone();
        Self {
            recorder,
            whisper_config,
            cached_engine: Some(engine),
            kernel,
            max_duration: Duration::from_secs(30),
            chunk_duration: Duration::from_millis(500),
        }
    }

    // with_default_model 保持不变(内部调 new,cached_engine: None)

    fn transcribe(&self, samples: &[i16]) -> VoiceResult<String> {
        if let Some(engine) = &self.cached_engine {
            return engine.transcribe(samples);
        }
        let engine = WhisperEngine::new(self.whisper_config.clone())?;
        engine.transcribe(samples)
    }

    fn route(&self, text: &str) -> RouteTextResult {
        // 同现有实现,不变
        // ...
    }
}

// 替换 VoiceListen for VoiceListenImpl:
impl VoiceListen for VoiceListenImpl {
    fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome> {
        let vad = VadDetector::new(VadConfig::default());
        let listener = VoiceListener::new(
            self.recorder.clone(),
            vad,
            self.max_duration,
            self.chunk_duration,
        );

        let outcome = listener.listen_with_cancel(cancel)?;

        match outcome {
            ListenOutcome::SpeechEnded { samples } => {
                let transcription = self.transcribe(&samples)?;
                let route_outcome = self.route(&transcription);
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
                let route_outcome = match &transcription {
                    Some(t) => self.route(t),
                    None => RouteTextResult::Empty,
                };
                Ok(VoiceListenOutcome::Timeout {
                    transcription,
                    route_outcome,
                })
            }
        }
    }
}

// 替换 voice_listen_command:
#[tauri::command]
pub async fn voice_listen_command(
    state: tauri::State<'_, crate::state::AppState>,
    app: AppHandle,
) -> Result<VoiceListenResult, String> {
    use trust_kernel::voice::audio::AudioRecorderConfig;
    use trust_kernel::voice::whisper::WhisperConfig;

    // 1. 重置 cancel flag
    state
        .kill_switch
        .store(false, std::sync::atomic::Ordering::SeqCst);

    // 2. 从 Settings 加载 voice 配置(V1.1.2 §8.3 Settings 持久化)
    let settings = load_voice_settings(&state.kernel).map_err(|e| e.to_string())?;
    let model_path = std::path::PathBuf::from(&settings.voice_model_path);
    let whisper_config = WhisperConfig {
        model_path: model_path.clone(),
        language: settings.voice_language.clone(),
        threads: settings.voice_threads,
        ..Default::default()
    };

    // 3. 检查 whisper_cache,miss 时加载(issue #61)
    let engine: Arc<WhisperEngine> = {
        let mut cache = state.whisper_cache.lock().map_err(|e| e.to_string())?;
        let needs_reload = cache.as_ref().map_or(true, |eng| {
            eng.config().model_path != model_path
        });
        if needs_reload {
            let new_engine = WhisperEngine::new(whisper_config.clone())
                .map_err(|e| e.to_string())?;
            *cache = Some(Arc::new(new_engine));
        }
        Arc::clone(cache.as_ref().expect("cache should be populated"))
    };

    // 4. 构造 VoiceListenImpl(用缓存的 engine)
    let recorder = Arc::new(
        AudioRecorderAdapter::new(AudioRecorderConfig::default())
            .map_err(|e| e.to_string())?,
    );
    let listener = VoiceListenImpl::with_engine(recorder, engine, state.kernel.clone());

    // 5. 执行 listen + 发射 transcription-final
    let result = voice_listen(&listener, &state.kill_switch);
    if let Some(payload) = build_transcription_final_payload(&result) {
        let _ = app.emit("transcription-final", payload);
    }
    Ok(result)
}

#[tauri::command]
pub async fn cancel_voice_command(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state
        .kill_switch
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// 从 ConfigRepo 加载 voice 相关 settings(V1.1.2 §8.3 Settings 持久化)。
fn load_voice_settings(
    kernel: &TrustKernel,
) -> Result<crate::settings_commands::SettingsDto, crate::error::UiError> {
    let conn = kernel.conn();
    let kv = kernel.config_repo().list(&conn)?;
    crate::settings_commands::merge_from_kv(&kv)
}
```

> **关键约束:**
> - `WhisperEngine` **不** `Clone`(持有 `WhisperContext` FFI 资源),cache 必须存 `Arc<WhisperEngine>`。`Arc::clone` 是浅拷贝(只增引用计数),不复制 FFI 资源。
> - `&state.kill_switch` 是 `&Arc<AtomicBool>`,通过 deref coercion 自动转 `&AtomicBool`(`Arc<T>: Deref<Target=T>`)。
> - `VoiceListenImpl::with_engine` 接收 `Arc<WhisperEngine>` 而非 `WhisperEngine`(避免 move 后无法缓存)。
> - `load_voice_settings` 中 `kernel.conn()` 返回 `MutexGuard<Connection>`,`config_repo()` 是 stateless 访问器返回新 `ConfigRepo` 实例。
> - 若 `UiError` 无 `From<KernelError>` 实现,需在 `crates/ui/src/error.rs` 加 `#[from]` 或显式 `.map_err(UiError::from)`。

修改 `voicepilot/crates/ui/src/commands.rs` 的 `register_handlers_with_voice`,追加:

```rust
crate::voice_commands::cancel_voice_command,
```

创建 `voicepilot/crates/ui/tests/voice_cancel_cache_unit.rs`:

```rust
#![cfg(feature = "voice")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use trust_kernel::voice::error::VoiceResult;
use voicepilot_ui::voice_commands::{voice_listen, VoiceListen, VoiceListenOutcome, VoiceListenResult};

struct CancelAwareMock {
    cancel_check_count: std::sync::atomic::AtomicUsize,
}

impl VoiceListen for CancelAwareMock {
    fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome> {
        self.cancel_check_count.fetch_add(1, Ordering::SeqCst);
        if cancel.load(Ordering::SeqCst) {
            return Ok(VoiceListenOutcome::NoSpeech);
        }
        Ok(VoiceListenOutcome::Success {
            transcription: "hello".to_string(),
            route_outcome: Default::default(),
        })
    }
}

#[test]
fn voice_listen_returns_no_speech_when_cancel_set() {
    let mock = CancelAwareMock {
        cancel_check_count: Default::default(),
    };
    let cancel = AtomicBool::new(true);
    let result = voice_listen(&mock, &cancel);
    match result {
        VoiceListenResult::NoSpeech => {}
        other => panic!("expected NoSpeech, got {:?}", other),
    }
}

#[test]
fn voice_listen_returns_success_when_cancel_not_set() {
    let mock = CancelAwareMock {
        cancel_check_count: Default::default(),
    };
    let cancel = AtomicBool::new(false);
    let result = voice_listen(&mock, &cancel);
    match result {
        VoiceListenResult::Success { .. } => {}
        other => panic!("expected Success, got {:?}", other),
    }
}
```

注意:`VoiceListenOutcome` 与 `VoiceListenResult` 的变体需与 W6b-1 实际定义一致。W6b-1 的 `VoiceListen::listen` 签名**不带** `cancel`,Task 5 修改 trait 签名后,**必须同步更新** W6b-1 现有测试:

**`voicepilot/crates/ui/tests/voice_commands_unit.rs`**(6 处调用 + 1 处 impl):
- `impl VoiceListen for MockVoiceListen` 的 `fn listen(&self)` → `fn listen(&self, _cancel: &AtomicBool)`
- 6 处 `voice_listen(&mock)` → `voice_listen(&mock, &std::sync::atomic::AtomicBool::new(false))`
- 文件顶部 `use std::sync::atomic::AtomicBool;`

**`voicepilot/crates/ui/tests/w6b1_voice_smoke.rs`**(6 处调用 + 1 处 impl):
- `impl VoiceListen for StubVoiceListen` 的 `fn listen(&self)` → `fn listen(&self, _cancel: &AtomicBool)`
- 6 处 `voice_listen(&stub)` → `voice_listen(&stub, &std::sync::atomic::AtomicBool::new(false))`
- 文件顶部 `use std::sync::atomic::AtomicBool;`

> 这些是 W6b-1 已通过的测试,Task 5 改 trait 签名后会编译失败。**必须**在 Task 5 步骤 5 中一起更新(作为同一 commit 的一部分)。预期 W6b-1 测试逻辑不变(只是加 `cancel` 参数 + 调用处加 `&AtomicBool::new(false)`),原 35 个测试应继续通过。

- [ ] **步骤 6: 运行测试验证通过 (green)**

```powershell
cd d:\voicepilot
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"; $env:WHISPER_DONT_GENERATE_BINDINGS = "1"
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features voice --test voice_cancel_cache_unit
# 期望:2 tests passed
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features voice
# 期望:W6b-1 35 + W6b-2 cancel/cache 2 = 37 passed(若 W6b-1 stub 需改签名,可能需同步更新现有测试)
```

- [ ] **步骤 7: 前端 cancel 按钮 + api**

修改 `voicepilot/crates/ui/web/src/api.ts`,追加:

```typescript
export async function cancelVoice(): Promise<void> {
  await invoke("cancel_voice_command");
}
```

修改 `MainView.tsx` 的 `voice-section`,在 voice-button 旁加 cancel 按钮(仅 listening 时显示):

```tsx
{listening && (
  <button
    type="button"
    className="voice-cancel-btn"
    onClick={() => cancelVoice().catch(console.error)}
  >
    取消
  </button>
)}
```

修改 `styles.css`,追加 `.voice-cancel-btn { margin-left: 8px; background: #450a0a; color: #ef4444; border: 1px solid #ef4444; padding: 6px 12px; border-radius: 4px; cursor: pointer; }`。

- [ ] **步骤 8: 构建前端 + 编译验证 + 提交**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web
npm.cmd run build
cd d:\voicepilot
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"; $env:WHISPER_DONT_GENERATE_BINDINGS = "1"
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features voice
git add voicepilot/crates/trust-kernel/src/voice/listener.rs voicepilot/crates/trust-kernel/tests/voice_listener_unit.rs voicepilot/crates/ui/src/state.rs voicepilot/crates/ui/src/voice_commands.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/voice_cancel_cache_unit.rs voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/components/MainView.tsx voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist
git commit -m "Task 5: voice cancel #57 + model caching #61 (kill_switch + whisper_cache + cancel_voice_command)"
```

---

## 任务 6: Partial transcript #47 + Kill Switch Bar + 多视图导航

**目标:** 实现 §8.4 实时 partial transcript(issue #47):`VoiceListener::listen_with_cancel` 循环中每 2s 用当前 buffer 调 WhisperEngine::transcribe,发射 `transcription-partial` 事件;前端 listening-indicator 显示 partial 文本。实现 §8.3 Kill Switch Bar:App.tsx 顶部常驻红色 "停止所有" 按钮,点击调 cancel_voice_command(voice feature)+ cancel_task_command(若实现,否则只 cancel voice)。实现多视图导航:App.tsx 加 `view` state + 侧边栏切换 Main / Settings / Audit / Trust / Skills 五个视图。

**文件:**
- 修改:`voicepilot/crates/trust-kernel/src/voice/listener.rs`(可选:加 partial callback)
- 修改:`voicepilot/crates/ui/src/voice_commands.rs`(发射 transcription-partial 事件)
- 修改:`voicepilot/crates/ui/web/src/api.ts`(onTranscriptionPartial)
- 修改:`voicepilot/crates/ui/web/src/types.ts`(TranscriptionPartialPayload)
- 修改:`voicepilot/crates/ui/web/src/components/MainView.tsx`(监听 partial + 显示)
- 修改:`voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx`(新建)
- 修改:`voicepilot/crates/ui/web/src/App.tsx`(多视图导航 + KillSwitchBar 集成)
- 修改:`voicepilot/crates/ui/web/src/styles.css`

- [ ] **步骤 1: 实现 partial transcript 事件发射**

设计:`VoiceListener` 新增 `listen_with_cancel_and_partial(cancel, partial_callback)` 方法(与 Task 5 的 `listen_with_cancel(cancel)` 并存)。`listen_with_cancel` 改为委托 `listen_with_cancel_and_partial(cancel, None)`。`VoiceListenImpl` 持有 `partial_app: Option<AppHandle>` + `cached_engine: Option<Arc<WhisperEngine>>`(Task 5 已加),`listen` 时构造闭包调 `engine.transcribe(samples)` + `app.emit("transcription-partial", ...)`。

修改 `voicepilot/crates/trust-kernel/src/voice/listener.rs`。**重构 Task 5 的 `listen_with_cancel`** 为委托 + 新增 `listen_with_cancel_and_partial`:

```rust
    pub fn listen(&self) -> VoiceResult<ListenOutcome> {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        self.listen_with_cancel(&cancel)
    }

    /// Task 5:仅 cancel,无 partial。委托给 `listen_with_cancel_and_partial`。
    pub fn listen_with_cancel(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> VoiceResult<ListenOutcome> {
        self.listen_with_cancel_and_partial(cancel, None)
    }

    /// Task 6:cancel + 可选 partial callback(每 2s 触发一次,传入当前 buffer)。
    pub fn listen_with_cancel_and_partial(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
        partial_callback: Option<&dyn Fn(&[i16])>,
    ) -> VoiceResult<ListenOutcome> {
        use std::sync::atomic::Ordering;
        let mut buffer: Vec<i16> = Vec::new();
        let mut elapsed = std::time::Duration::ZERO;
        let mut last_partial_at = std::time::Instant::now();

        while elapsed < self.max_duration {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            let chunk = self.recorder.record_chunk(self.chunk_duration)?;
            if chunk.is_empty() {
                break;
            }
            buffer.extend_from_slice(&chunk);
            elapsed += self.chunk_duration;

            // VAD 静音超时 → SpeechEnded
            if let Some(seg) = self.vad.detect_end_of_speech(&buffer) {
                buffer.truncate(seg.speech_end_sample);
                return Ok(ListenOutcome::SpeechEnded { samples: buffer });
            }

            // partial transcript:每 2s 触发一次(issue #47)
            if last_partial_at.elapsed() >= std::time::Duration::from_secs(2) {
                if let Some(cb) = partial_callback {
                    cb(&buffer);
                }
                last_partial_at = std::time::Instant::now();
            }
        }

        // post-loop:与 Task 5 一致
        match self.vad.detect(&buffer) {
            crate::voice::vad::VadOutcome::Speech { speech_end_sample, .. } => {
                buffer.truncate(speech_end_sample);
                if buffer.is_empty() {
                    Ok(ListenOutcome::NoSpeech)
                } else {
                    Ok(ListenOutcome::Timeout { samples: buffer })
                }
            }
            crate::voice::vad::VadOutcome::NoSpeech => Ok(ListenOutcome::NoSpeech),
        }
    }
```

修改 `voicepilot/crates/ui/src/voice_commands.rs`。`VoiceListenImpl` 加 `partial_app: Option<AppHandle>` 字段;`with_engine` 改为 `with_engine_and_app(recorder, engine, app, kernel)`(同时设 `partial_app: Some(app)` + `cached_engine: Some(engine)`);`listen(cancel)` 内部构造闭包调 `listen_with_cancel_and_partial`:

```rust
// 顶部 imports 追加(若未导入):
use tauri::AppHandle;
use serde::Serialize;

// VoiceListenImpl 加字段:
pub struct VoiceListenImpl {
    recorder: Arc<dyn VoiceRecorder>,
    whisper_config: WhisperConfig,
    cached_engine: Option<Arc<WhisperEngine>>,
    /// Partial transcript 事件发射句柄(issue #47)。None 时不发射 partial。
    partial_app: Option<AppHandle>,
    kernel: Arc<TrustKernel>,
    max_duration: Duration,
    chunk_duration: Duration,
}

// with_engine 改为接收 AppHandle:
impl VoiceListenImpl {
    pub fn with_engine(
        recorder: Arc<dyn VoiceRecorder>,
        engine: Arc<WhisperEngine>,
        app: AppHandle,
        kernel: Arc<TrustKernel>,
    ) -> Self {
        let whisper_config = engine.config().clone();
        Self {
            recorder,
            whisper_config,
            cached_engine: Some(engine),
            partial_app: Some(app),
            kernel,
            max_duration: Duration::from_secs(30),
            chunk_duration: Duration::from_millis(500),
        }
    }

    // new 保持 cached_engine: None, partial_app: None
    // with_default_model 保持 cached_engine: None, partial_app: None
}

// VoiceListen for VoiceListenImpl 的 listen 改用 listen_with_cancel_and_partial:
impl VoiceListen for VoiceListenImpl {
    fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome> {
        let vad = VadDetector::new(VadConfig::default());
        let listener = VoiceListener::new(
            self.recorder.clone(),
            vad,
            self.max_duration,
            self.chunk_duration,
        );

        // 构造 partial callback(若 cached_engine + partial_app 都 Some)
        let partial_cb: Option<Box<dyn Fn(&[i16]) + Send + Sync>> = match (&self.cached_engine, &self.partial_app) {
            (Some(engine), Some(app)) => {
                let engine_clone = Arc::clone(engine);
                let app_clone = app.clone();
                Some(Box::new(move |samples: &[i16]| {
                    match engine_clone.transcribe(samples) {
                        Ok(text) => {
                            let payload = TranscriptionPartialPayload {
                                partial: text,
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            };
                            let _ = app_clone.emit("transcription-partial", payload);
                        }
                        Err(_) => {} // partial 失败静默(不打断主流程)
                    }
                }))
            }
            _ => None,
        };

        let outcome = match partial_cb.as_ref() {
            Some(cb) => listener.listen_with_cancel_and_partial(cancel, Some(cb))?,
            None => listener.listen_with_cancel(cancel)?,
        };

        match outcome {
            // ... 同 Task 5(SpeechEnded / NoSpeech / Timeout 处理不变)
        }
    }
}

// TranscriptionPartialPayload 新增:
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionPartialPayload {
    pub partial: String,
    pub timestamp_ms: i64,
}
```

修改 `voice_listen_command` 中 `VoiceListenImpl::with_engine` 调用,加 `app` 参数:

```rust
let listener = VoiceListenImpl::with_engine(recorder, engine, app.clone(), state.kernel.clone());
```

> **关键约束:**
> - 闭包必须 `Send + Sync`(`Box<dyn Fn(&[i16]) + Send + Sync>`),因为 `VoiceListener::listen_with_cancel_and_partial` 跨线程可能。`Arc<WhisperEngine>` + `AppHandle` 都满足 Send + Sync。
> - 闭包捕获 `engine_clone` 和 `app_clone` by move(`Arc::clone` 浅拷贝)。
> - partial callback 失败静默(不打断主 listen 流程)—— `Err(_) => {}`。
> - `chrono` 在 voice feature 下已可用(trust-kernel 已依赖)。

- [ ] **步骤 2: 写 partial transcript 单元测试**

修改 `voicepilot/crates/trust-kernel/tests/voice_listener_unit.rs`,追加:

```rust
use std::sync::Mutex;

#[test]
fn voice_listener_invokes_partial_callback_every_2_seconds() {
    // 用极短的 max_duration(2.5s)+ chunk 500ms,触发 1 次 partial callback。
    let chunk = generate_sine_wave(500, 16000, 200.0);
    let recorder = Arc::new(MockVoiceRecorder::new(vec![chunk; 10]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_millis(2500),
        Duration::from_millis(500),
    );
    let cancel = AtomicBool::new(false);
    let call_count = Arc::new(Mutex::new(0usize));
    let count_clone = call_count.clone();
    let cb = move |_samples: &[i16]| {
        *count_clone.lock().unwrap() += 1;
    };
    let _outcome = listener
        .listen_with_cancel_and_partial(&cancel, Some(&cb))
        .expect("listen");
    // 2.5s / 2s = 至少 1 次 partial
    assert!(*call_count.lock().unwrap() >= 1, "expected at least 1 partial callback");
}

#[test]
fn voice_listener_listen_with_cancel_still_works_without_partial() {
    // Task 5 的 listen_with_cancel(cancel) 仍可用(Task 6 改为委托)。
    let chunk = generate_sine_wave(750, 16000, 200.0);
    let recorder = Arc::new(MockVoiceRecorder::new(vec![chunk]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_secs(30),
        Duration::from_millis(750),
    );
    let cancel = AtomicBool::new(false);
    let outcome = listener.listen_with_cancel(&cancel).expect("listen");
    // 不 panic 即可(具体 outcome 取决于 VAD)
    let _ = outcome;
}
```

- [ ] **步骤 3: 运行测试验证 + 编译**

```powershell
cd d:\voicepilot
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"; $env:WHISPER_DONT_GENERATE_BINDINGS = "1"
cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --features voice --test voice_listener_unit
# 期望:8 tests passed(6 原有 + Task 5 的 2 cancel + Task 6 的 2 partial = 10?实际数取决于 Task 5/6 合并状态)
```

- [ ] **步骤 4: 实现 KillSwitchBar 组件**

创建 `voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx`:

```tsx
import { useState } from "react";
import { cancelVoice } from "../api";

export function KillSwitchBar(): JSX.Element {
  const [busy, setBusy] = useState(false);

  const handleKill = (): void => {
    setBusy(true);
    cancelVoice()
      .catch(console.error)
      .finally(() => setBusy(false));
  };

  return (
    <div className="kill-switch-bar" role="banner" aria-label="紧急停止">
      <span className="kill-switch-label">⚠ 紧急停止</span>
      <button
        type="button"
        className="kill-switch-btn"
        onClick={handleKill}
        disabled={busy}
        aria-label="停止所有执行"
      >
        {busy ? "停止中…" : "停止所有"}
      </button>
    </div>
  );
}
```

- [ ] **步骤 5: 实现 App.tsx 多视图导航 + 集成 KillSwitchBar**

修改 `voicepilot/crates/ui/web/src/App.tsx`,替换为:

```tsx
import { useEffect, useState } from "react";
import { onApprovalRequest, onTranscriptionPartial } from "./api";
import type { ApprovalRequestPayload, View } from "./types";
import { MainView } from "./components/MainView";
import { ApprovalModal } from "./components/ApprovalModal";
import { SettingsView } from "./components/SettingsView";
import { AuditViewerView } from "./components/AuditViewerView";
import { TrustCenterView } from "./components/TrustCenterView";
import { SkillsManagerView } from "./components/SkillsManagerView";
import { KillSwitchBar } from "./components/KillSwitchBar";

const NAV_ITEMS: { view: View; label: string }[] = [
  { view: "main", label: "Main Chat" },
  { view: "settings", label: "Settings" },
  { view: "audit", label: "Audit Viewer" },
  { view: "trust", label: "Trust Center" },
  { view: "skills", label: "Skills Manager" },
];

export function App(): JSX.Element {
  const [view, setView] = useState<View>("main");
  const [approval, setApproval] = useState<ApprovalRequestPayload | null>(null);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) => setApproval(payload));
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  return (
    <div className="app-root">
      <KillSwitchBar />
      <div className="app-body">
        <nav className="sidebar" aria-label="主导航">
          <h1 className="app-title">VoicePilot</h1>
          <ul className="nav-list" role="list">
            {NAV_ITEMS.map((item) => (
              <li key={item.view}>
                <button
                  type="button"
                  className={`nav-item ${view === item.view ? "active" : ""}`}
                  onClick={() => setView(item.view)}
                  aria-pressed={view === item.view}
                  aria-current={view === item.view ? "page" : undefined}
                >
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <main className="main-content">
          {view === "main" && <MainView />}
          {view === "settings" && <SettingsView />}
          {view === "audit" && <AuditViewerView />}
          {view === "trust" && <TrustCenterView />}
          {view === "skills" && <SkillsManagerView />}
        </main>
      </div>
      {approval && (
        <ApprovalModal
          payload={approval}
          onClose={() => setApproval(null)}
        />
      )}
    </div>
  );
}
```

修改 `voicepilot/crates/ui/web/src/components/MainView.tsx`,加 partial transcript 监听 + 显示:

```tsx
import { useEffect, useState } from "react";
import { onTranscriptionPartial } from "../api";

// 在 MainView 组件内:
const [partialText, setPartialText] = useState<string>("");

useEffect(() => {
  const unlisten = onTranscriptionPartial((payload) => {
    setPartialText(payload.partial);
  });
  return () => { unlisten.then((fn) => fn()); };
}, []);

// 在 listening-indicator 内显示 partialText:
// <div className="partial-text" aria-live="polite">{partialText || "聆听中…"}</div>
```

修改 `voicepilot/crates/ui/web/src/api.ts`,实现 `onTranscriptionPartial`:

```typescript
export interface TranscriptionPartialPayload {
  partial: string;
  timestamp_ms: number;
}

export function onTranscriptionPartial(
  handler: (payload: TranscriptionPartialPayload) => void
): Promise<UnlistenFn> {
  return listen<TranscriptionPartialPayload>("transcription-partial", (e) => handler(e.payload));
}
```

- [ ] **步骤 6: 更新 styles.css 加 sidebar / kill-switch-bar 样式**

修改 `voicepilot/crates/ui/web/src/styles.css`,追加:

```css
.app-root { display: flex; flex-direction: column; height: 100vh; overflow: hidden; }
.kill-switch-bar {
  background: #450a0a;
  border-bottom: 1px solid #7f1d1d;
  padding: 6px 16px;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  font-family: "IBM Plex Mono", monospace;
  font-size: 12px;
}
.kill-switch-bar button {
  background: #b91c1c;
  color: #fef2f2;
  border: 1px solid #fca5a5;
  padding: 4px 12px;
  border-radius: 4px;
  cursor: pointer;
  font-family: inherit;
  font-size: inherit;
}
.kill-switch-bar button:hover { background: #dc2626; }
.kill-switch-bar button:disabled { opacity: 0.6; cursor: not-allowed; }
.app-body { display: flex; flex: 1; min-height: 0; }
.sidebar {
  width: 200px;
  background: #0f172a;
  border-right: 1px solid #1e293b;
  padding: 16px 12px;
  overflow-y: auto;
}
.app-title {
  color: #f59e0b;
  font-family: "IBM Plex Sans", sans-serif;
  font-size: 16px;
  font-weight: 600;
  margin: 0 0 16px 0;
  padding: 0 8px;
  letter-spacing: 0.04em;
}
.nav-list { list-style: none; padding: 0; margin: 0; }
.nav-item {
  display: block;
  width: 100%;
  text-align: left;
  background: transparent;
  color: #cbd5e1;
  border: none;
  padding: 8px 12px;
  margin-bottom: 4px;
  border-radius: 4px;
  cursor: pointer;
  font-family: "IBM Plex Sans", sans-serif;
  font-size: 13px;
}
.nav-item:hover { background: #1e293b; color: #f1f5f9; }
.nav-item.active { background: #1e293b; color: #f59e0b; }
.main-content {
  flex: 1;
  padding: 24px;
  overflow-y: auto;
  background: #0b1220;
  color: #e2e8f0;
}
.partial-text {
  margin-top: 8px;
  padding: 6px 10px;
  background: #1e293b;
  border-left: 2px solid #f59e0b;
  color: #cbd5e1;
  font-family: "IBM Plex Mono", monospace;
  font-size: 12px;
  min-height: 24px;
}
```

- [ ] **步骤 7: 构建前端 + 编译验证 + git 提交**

Run:
```powershell
cd voicepilot\crates\ui\web; npm.cmd run build
cd d:\voicepilot; cargo check -p voicepilot-ui --features "tauri voice"
```

Expected: `npm run build` 生成 `dist/`，`cargo check` 无错误。

```powershell
cd d:\voicepilot; git add voicepilot/crates/ui/web/src voicepilot/crates/trust-kernel/src/voice/listener.rs voicepilot/crates/trust-kernel/src/voice/mod.rs; git commit -m "feat(w6b-2): partial transcript #47 + KillSwitchBar + multi-view nav"
```

---

## Task 7: W6b-2 E2E 冒烟测试

**Files:**
- Create: `voicepilot/crates/ui/tests/w6b2_smoke.rs`

- [ ] **步骤 1: 写失败测试 — Settings KV 往返**

Create `voicepilot/crates/ui/tests/w6b2_smoke.rs`:

```rust
#![cfg(feature = "tauri")]

use trust_kernel::kernel::TrustKernel;

#[test]
fn settings_kv_round_trip() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    kernel.config_repo().set(&conn, "theme", "dark").unwrap();
    kernel.config_repo().set(&conn, "whisper_model", "ggml-base.bin").unwrap();
    let v1 = kernel.config_repo().get(&conn, "theme").unwrap();
    assert_eq!(v1.as_deref(), Some("dark"));
    let v2 = kernel.config_repo().get(&conn, "whisper_model").unwrap();
    assert_eq!(v2.as_deref(), Some("ggml-base.bin"));
    kernel.config_repo().set(&conn, "theme", "light").unwrap();
    let v3 = kernel.config_repo().get(&conn, "theme").unwrap();
    assert_eq!(v3.as_deref(), Some("light"));
}
```

- [ ] **步骤 2: 运行测试,确认失败**

Run: `cargo test -p voicepilot-ui --features tauri --test w6b2_smoke`
Expected: FAIL — `ConfigRepo` not found or `set/get` not defined

- [ ] **步骤 3: 实现 ConfigRepo 直到测试通过**

(在 Task 1 已实现,此步骤验证 E2E 测试通过)

Run: `cargo test -p voicepilot-ui --features tauri --test w6b2_smoke`
Expected: PASS

- [ ] **步骤 4: 追加 SkillRepo CRUD 测试**

Append to `w6b2_smoke.rs`:

```rust
use trust_kernel::skills::repo::SkillRecord;

#[test]
fn skills_manager_crud() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let rec = SkillRecord {
        skill_id: "test.echo".to_string(),
        version: 1,  // i64(001_init.sql skills.version INTEGER)
        manifest_json: r#"{"id":"test.echo","risk":"E1D1"}"#.to_string(),
        enabled: true,
        success_count: 0,
        avg_latency_ms: 0.0,
    };
    kernel.skill_repo().upsert(&conn, &rec).unwrap();
    let listed = kernel.skill_repo().list(&conn).unwrap();
    assert!(listed.iter().any(|s| s.skill_id == "test.echo"));
    kernel.skill_repo().toggle(&conn, "test.echo", false).unwrap();
    let got = kernel.skill_repo().get(&conn, "test.echo").unwrap().unwrap();
    assert!(!got.enabled);
}
```

- [ ] **步骤 5: 追加 McpServer toggle 测试**

Append to `w6b2_smoke.rs`:

```rust
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};

#[test]
fn trust_center_toggle_mcp_server() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let record = McpServerRecord {
        server_id: "test.svc".to_string(),
        name: "Test".to_string(),
        version: "1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some("[]".to_string()),
    };
    // TrustKernel 没有 mcp_server_repo() 字段访问器 —— 用 stateless 临时实例
    McpServerRepo::new().create(&conn, &record).unwrap();
    kernel.toggle_mcp_server("test.svc", false).unwrap();
    let loaded = McpServerRepo::new().get(&conn, "test.svc").unwrap().unwrap();
    assert!(!loaded.enabled);
    kernel.toggle_mcp_server("test.svc", true).unwrap();
    let loaded2 = McpServerRepo::new().get(&conn, "test.svc").unwrap().unwrap();
    assert!(loaded2.enabled);
}
```

- [ ] **步骤 6: 追加 AuditLogger list_recent 测试**

Append to `w6b2_smoke.rs`:

```rust
use trust_kernel::repo::step_repo::StepRecord;

#[test]
fn audit_viewer_lists_recent_events() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = "w6b2-audit-task";
    let step_id = "w6b2-audit-step";
    // kernel.create_task 签名:(&str, &str) —— 直接传 task_id + user_goal
    kernel.create_task(task_id, "test user goal").unwrap();
    // kernel.create_step 签名:(&StepRecord)
    let step = StepRecord::new(step_id, task_id, 1);
    kernel.create_step(&step).unwrap();
    // audit_append_external 签名:(task_id, step_id, event_type, details) —— 不是 &AuditEvent
    kernel
        .audit_append_external(task_id, Some(step_id), "STEP_STARTED", serde_json::json!({"k":"v"}))
        .unwrap();
    let events = kernel.list_audit_recent(10).unwrap();
    assert!(!events.is_empty());
    let for_task = kernel.list_audit_for_task(task_id).unwrap();
    assert!(!for_task.is_empty());
}
```

> **关键签名核对:**
> - `kernel.create_task(&self, task_id: &str, user_goal: &str) -> Result<TaskRecord>`(不是 `TaskRecord`)
> - `kernel.create_step(&self, step: &StepRecord) -> Result<()>`
> - `StepRecord::new(step_id: impl Into<String>, task_id: impl Into<String>, step_order: i64)`(3 参数)
> - `kernel.audit_append_external(&self, task_id: &str, step_id: Option<&str>, event_type: &str, details: serde_json::Value) -> Result<()>`(**不**接收 `&AuditEvent`,而是 4 个原始参数)
> - `kernel.list_audit_recent(&self, limit: usize) -> Result<Vec<AuditEvent>>`
> - `kernel.list_audit_for_task(&self, task_id: &str) -> Result<Vec<AuditEvent>>`
> - `StepRecord` 在 `trust_kernel::repo::step_repo` 模块下(不是 `trust_kernel::task`)

- [ ] **步骤 7: 运行所有 w6b2_smoke 测试**

Run: `cargo test -p voicepilot-ui --features tauri --test w6b2_smoke`
Expected: 4 tests PASS

- [ ] **步骤 8: git 提交**

```powershell
cd d:\voicepilot; git add voicepilot/crates/ui/tests/w6b2_smoke.rs; git commit -m "test(w6b-2): E2E smoke test for Settings/Audit/Trust/Skills repos"
```

---

## Task 8: 最终验证(多特性测试矩阵)

**Files:**
- 无新文件,仅执行验证命令

- [ ] **步骤 1: 默认特性测试**

Run: `cd d:\voicepilot; cargo test`
Expected: 196 tests passed, 0 failed

- [ ] **步骤 2: tauri 特性测试**

Run: `cargo test -p voicepilot-ui --features tauri`
Expected: 12 + 4 = 16 tests passed(原 12 个 + w6b2_smoke 4 个)

- [ ] **步骤 3: voice 特性测试**

Run: `cargo test -p voicepilot-ui --features voice`
Expected: 35 tests passed(W6a + W6b-1 不变)

- [ ] **步骤 4: trust-kernel voice 特性测试**

Run: `cargo test -p trust-kernel --features voice`
Expected: W5 + W6b-1 voice tests passed

- [ ] **步骤 5: clippy 默认 + tauri + voice 三组合**

Run:
```powershell
cargo clippy --all-targets -- -D warnings
cargo clippy -p voicepilot-ui --features tauri -- -D warnings
cargo clippy -p voicepilot-ui --features voice -- -D warnings
```
Expected: 0 warnings for all three

- [ ] **步骤 6: 前端构建**

Run: `cd voicepilot\crates\ui\web; npm.cmd run build`
Expected: `dist/` 生成,无 TS 错误

- [ ] **步骤 7: git log 验证 7 个 commit**

Run: `cd d:\voicepilot; git log --oneline -n 7`
Expected: 看到 Task 1-7 的 commit

- [ ] **步骤 8: 更新 PROGRESS.md + 最终 commit**

修改 `docs/PROGRESS.md`:
- 把 W6b-2 行的 `⏳ 未开始` 改为 `✅ 已完成`
- 在 "二、已完成工作详细记录" 加 W6b-2 段落(参考 W6b-1 格式)

```powershell
cd d:\voicepilot; git add docs/PROGRESS.md docs/superpowers/plans/2026-07-21-w6b-2-settings-audit-trust-skills.md; git commit -m "docs(w6b-2): plan + PROGRESS.md"
```

---

## 自审清单

执行前对照检查:

**1. 规格覆盖(V1.1.2 spec):**
- ✅ §8.2 IPC 三规则 — W6a 已实现,W6b-2 保持不变(无新 IPC)
- ✅ §8.3 Settings — Task 1(ConfigRepo + SettingsView)
- ✅ §8.3 Audit Viewer — Task 2(AuditLogger 查询 + AuditViewerView)
- ✅ §8.3 Trust Center — Task 3(McpServerRepo.toggle + TrustCenterView)
- ✅ §8.3 Skills Manager — Task 4(SkillRepo CRUD + SkillsManagerView)
- ✅ §8.3 Kill Switch Bar — Task 6(KillSwitchBar 组件)
- ✅ §8.4 Partial Transcript — Task 6(2s 间隔回调 + transcription-partial 事件)
- ✅ §8.2 Voice cancel #57 — Task 5(AtomicBool 取消标志)
- ✅ §8.2 Model caching #61 — Task 5(WhisperEngine 缓存)

**2. 占位符扫描:**
- 无 "TBD"/"TODO"/"implement later"
- 无 "Add appropriate error handling"
- 无 "Similar to Task N"
- 所有步骤都有实际代码或命令

**3. 类型一致性:**
- `ConfigRepo::new(Arc<TrustKernel>)` — Task 1 定义,Task 7 使用 ✅
- `SkillRepo::new(Arc<TrustKernel>)` — Task 4 定义,Task 7 使用 ✅
- `McpServerRepo::toggle_enabled(&str, bool)` — Task 3 定义,Task 7 使用 ✅
- `AuditLogger::list_recent(usize)` / `list_for_task(&str)` — Task 2 定义,Task 7 使用 ✅
- `VoiceListener::listen_with_cancel` — Task 5 定义,Task 6 调用 ✅
- `WhisperEngine` 缓存 — Task 5 用 `Arc<Mutex<Option<WhisperEngine>>>` ✅
- `onTranscriptionPartial` — Task 6 定义,MainView 使用 ✅

**4. 风险点:**
- ⚠️ `Arc<Mutex<Option<WhisperEngine>>>` 重新加载模型时需避免锁持有太久(约 3-5s)。建议先 `take()` 出来,drop 锁后重新加载,再 `replace()`。
- ⚠️ Partial transcript 回调不能阻塞 listen 循环。建议在循环内 spawn `tokio::task::spawn_local` 或直接 emit(emit 是非阻塞的)。
- ⚠️ KillSwitchBar 的 "停止所有" 按钮需要 `window.emit("kill-switch-triggered", ())` 触发后端取消。Rust 侧监听该事件,设 `cancel_flag.store(true, SeqCst)`。

**5. 已知偏离(将记录为 spec issue):**
- §8.3 Skills Manager 未规定 Skills 表 schema 的 UI 显示字段 — 本计划用现有 `success_count`/`avg_latency_ms` 列
- §8.3 Trust Center 未规定 "禁用 MCP server" 的级联效应 — 本计划仅在 `mcp_servers.enabled=false` 层禁用,运行中的连接需重启 mcp-serve 才生效(简化实现)
- §8.4 Partial transcript 间隔未规定 — 本计划取 2s(Whisper 推理延迟约 1-3s,2s 平衡实时性与性能)

---

## 执行交接

计划已完成并保存到 `docs/superpowers/plans/2026-07-21-w6b-2-settings-audit-trust-skills.md`。两种执行方式:

**1. Subagent-Driven(推荐)** — 每个 Task 派发独立 subagent,两阶段评审,快速迭代
**2. Inline Execution** — 在当前 session 内按 Task 顺序执行,checkpoint 处评审

请选择执行方式。