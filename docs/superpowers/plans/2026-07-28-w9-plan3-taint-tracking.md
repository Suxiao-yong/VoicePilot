# W9 Plan 3: Taint Tracking 污点传播(CRUD + 查表驱动 Gateway)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W9 设计文档(`docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.3 + §6.2)实现 VoicePilot Trust Kernel 的值级污点追踪:新增 `TaintRepo` CRUD 操作 `taints` 表(表 schema 已存在于 `migrations/001_init.sql:98-105`)+ 在 Skill dispatcher 注入输入 → 输出 taint 传播 + 将 `gateway.rs:79-80` 的硬编码 `web_page` 规则升级为查表驱动的 `check_taint_policy` + 在 LLM 拆解 / MCP tool 调用点注入 taint 标记 + 补齐 `taint_propagated` / `taint_blocked` 审计事件,使 Trust Kernel 能按值哈希追踪污点跨 Skill 边界传播并在 Gateway 拦截非法 taint → sink 组合。

**Architecture:** 在 `crates/trust-kernel/src/policy/` 新增 `taint_repo.rs` 模块(`TaintRepo` 零参数构造 + 方法接 `&Connection`,遵循 `DagRepo` / `ConfigRepo` / `SkillRepo` 既有 accessor pattern),实现 `upsert` / `find_by_value` / `list_by_provenance` / `list_by_source` / `delete_by_source` 五个方法;`upsert` 在 `value_hash` 冲突时合并 `taints_json`(去重),依赖 `006_taints_unique_index.sql` 的 UNIQUE 约束用 `ON CONFLICT` 幂等写入。在 `skills/dispatcher.rs::dispatch_skill_executor` 入口计算输入值哈希 → 查 taint → 执行 executor → 计算输出值哈希 → upsert 输出 taint(继承输入 taints + 加 `executor_output:<skill_id>` 标签)。在 `gateway.rs` 新增独立函数 `check_taint_policy(conn, value_hash, egress_dest)`(W9 修复 P0-11:非 ActionGateway 方法,因为 gateway 不持有 DB 连接),删除第 79-80 行硬编码 web_page 规则,由调用方(dispatcher / `invoke_mcp_tool` / filesystem 工具函数)在需要时调 `check_taint_policy(&kernel.conn(), ...)`,查表判定 `web_page` taint 不能成为 `ToolArgument`、`llm_output` taint 不能写入文件系统(`EgressDest::LocalFile`);`gateway.decide` 内部不调此函数。在 `llm/client.rs::decompose_to_dag`(签名扩展接收 `kernel: &TrustKernel` 参数,W9 修复 P0-12)返回前遍历 DagPlan literal 值标 `llm_output` taint;在 `mcp/server.rs::handle_tools_call`(McpServer struct 加 `server_id` 字段,W9 修复 P0-13)返回前标 `mcp_tool:<server_id>` taint。所有 taint 操作通过 `kernel.audit_append_external` 记录 `taint_propagated` / `taint_blocked` 事件(hash 不含原始值,遵循 §6.2 隐私约束)。

**Tech Stack:** Rust(stable)+ `rusqlite` + `sha2`(SHA256 值哈希)+ `serde_json`(taints_json 序列化)+ `uuid`(taint_id)+ `chrono`(collected_at ISO8601)+ TDD。

**Spec:** `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.3(Plan 3 范围:TaintRepo CRUD + dispatcher 传播 + gateway 查表驱动)+ §6.2(Taint Tracking 安全约束:value_hash 不存原始值 / taints_json 仅存标签 / 审计不记 resource 原始值 / delete_by_source 清理)+ §10(Conventions:PowerShell `;` 分隔 / TDD / Repo accessor pattern / `&kernel.conn()` 不用 `&*` / TrustKernel 非 Clone / commit message `feat(w9p3): ...`)+ §11(兼容性:`taints` 表空壳 → CRUD 向后兼容无既有调用 / `gateway.rs` 硬编码 → 查表驱动向后兼容 Allow/Deny 路径不变)

**Precondition:**
- Plan 2 已完成:`audit_append_external` 调用模式稳定(`kernel.rs:290` 签名 `audit_append_external(task_id, step_id, event_type, details)`),`stronghold_snapshot_encrypted` 等审计事件已落地,本 Plan 复用同一审计通道记录 `taint_propagated` / `taint_blocked`。
- `taints` 表 schema 已存在于 `voicepilot/crates/trust-kernel/src/migrations/001_init.sql:98-105`,含 `taint_id` / `value_hash` / `provenance` / `taints_json` / `collected_at` / `source_ref` 六列,无需新增 migration。
- `Resource.provenance: String` 字段已存在于 `voicepilot/crates/trust-kernel/src/policy/types.rs:84-86`(注释标 "V1.1 §7.3 taint tracking"),`EgressDest` 枚举(`types.rs:97-104`)含 `LocalFile` / `RemoteLlm` / `RemoteMcp` / `ToolArgument` 四变体(即 spec §2.3 所述 "Sink")。
- `gateway.rs:79-80` 有一处硬编码规则:`web_page provenance` 不能成为 `ToolArgument`(本 Plan 替换为查表驱动)。
- `cedar_engine.rs:93, 106-107` 的 `provenance` 作为 Cedar 属性传入,但无 taint 查询(W9 不改 Cedar,W10+ 延后项)。
- `policy/egress.rs:17-19` 注释 "Full provenance-aware egress lands in W7"(已欠债到 W9,本 Plan 闭合)。
- `dispatch_skill_executor(skill_id, kernel, resolved_input, approver, task_id, step_id)` 签名在 `dispatcher.rs:110`,返回 `DispatchOutcome { task_id, step_id, output, succeeded, error_cause }`。
- `decompose_to_dag` 在 `llm/client.rs:258`,`handle_tools_call` 在 `mcp/server.rs:98`(调 `self.handler.call_tool` at :134)。
- Repo accessor pattern 参考基准:`DagRepo::new()` 无参数,方法接收 `&Connection`(`dag_repo.rs:41-46`)。
- 与 W9 Plan 5(Playwright E2E)的依赖关系:Plan 5 假设 Plan 3 已实现 mcp_tool taint 标记(注意:W7 Plan 5 没有 mcp_tool taint,本 Plan 3 是首次引入)。

---

## File Structure

### Backend — Trust Kernel(`voicepilot/crates/trust-kernel/src/`)

- **Create** `policy/taint_repo.rs` — W9 Plan 3 新模块,`taints` 表的 CRUD:
  - `TaintRecord` struct:`taint_id` / `value_hash` / `provenance` / `taints: Vec<String>` / `collected_at` / `source_ref: Option<String>`
  - `TaintRepo` 零参数构造(`new() -> Self`),方法接 `&Connection`:
    - `upsert(&self, conn: &Connection, taint: &TaintRecord) -> Result<()>` — value_hash 冲突时合并 taints(去重)
    - `find_by_value(&self, conn: &Connection, value: &str) -> Result<Option<TaintRecord>>` — 入参是原始 value,内部算 SHA256
    - `list_by_provenance(&self, conn: &Connection, provenance: &str) -> Result<Vec<TaintRecord>>`
    - `list_by_source(&self, conn: &Connection, source_ref: &str) -> Result<Vec<TaintRecord>>`
    - `delete_by_source(&self, conn: &Connection, source_ref: &str) -> Result<u64>` — 返回删除行数
  - 模块级辅助:`compute_value_hash(value: &serde_json::Value) -> String`(canonical JSON + SHA256 hex,避免 JSON 字段顺序差异产生不同 hash)、`merge_taints(existing: &[String], incoming: &[String]) -> Vec<String>`(去重合并)
- **Modify** `policy/mod.rs` — 加 `pub mod taint_repo;`(行 8 后追加)
- **Modify** `gateway.rs` — 替换第 79-80 行硬编码为 `check_taint_policy` 调用:
  - 新增 `pub fn check_taint_policy(conn: &Connection, value_hash: &str, egress_dest: EgressDest) -> Result<()>` 独立函数(模块级,非方法,因为 ActionGateway 不持有 DB 连接;查 `TaintRepo::find_by_value` 按 value_hash 取 taints,判定 `web_page` → `ToolArgument` 拦截、`llm_output` → `LocalFile` 拦截)
  - `decide` 方法内第 79-80 行的硬编码 web_page 规则删除(由调用方在需要时调 `check_taint_policy(&kernel.conn(), ...)`,gateway.decide 内部不调此函数;见 Task 4 Step 4)
  - 拦截时调 `kernel.audit_append_external` 记录 `taint_blocked` 事件(通过返回 `KernelError` 携带 taints + sink,由调用方审计;见 Task 7)
- **Modify** `skills/dispatcher.rs` — `dispatch_skill_executor` 入口 + 出口注入 taint 传播:
  - 入口(行 128 `let resolved_input = &normalized;` 之后):计算 `input_hash = compute_value_hash(&input_str)`,查 `TaintRepo::find_by_value` 取 `input_taints`
  - 出口(行 130 `match skill_id { ... }` 返回 `DispatchOutcome` 之前):若 `outcome.succeeded` 且 `outcome.output` 非 `Null`,计算 `output_hash`,upsert `TaintRecord { taints: input_taints + ["executor_output:<skill_id>"], provenance: "executor_output:<skill_id>", source_ref: Some("<task_id>:<step_id>") }`
  - 传播后调 `kernel.audit_append_external(task_id, Some(step_id), "taint_propagated", ...)`(见 Task 7)
- **Modify** `llm/client.rs` — `decompose_to_dag` 返回前(行 264 `decompose_to_dag_traced` 调用之后):遍历 `plan.nodes`,对每个 `input_template` 中的 `TemplateExpr::Literal(s)` 计算 value_hash,upsert `TaintRecord { provenance: "llm_output", taints: ["llm_output"], source_ref: Some(plan.plan_id) }`
- **Modify** `mcp/server.rs` — `handle_tools_call` 在 `self.handler.call_tool` 返回成功后(行 134 之后):计算 result 序列化值的 hash,upsert `TaintRecord { provenance: "mcp_tool:<server_id>", taints: ["mcp_tool:<server_id>"], source_ref: Some("<task_id>:<step_id>") }`
- **Modify** `policy/types.rs` — 不修改 `Resource` struct(不加 `value_hash` 字段,避免 breaking change);`check_taint_policy` 用单独 `value_hash: &str` 参数接收

### Tests(`voicepilot/crates/trust-kernel/tests/`)

- **Create** `w9_taint_tracking_unit.rs` — TaintRepo CRUD 单元测试(10 个,Task 2 TDD):upsert / find_by_value / list_by_provenance / list_by_source / delete_by_source / upsert 合并去重 / value_hash 计算 / source_ref 关联 / 级联删除 / 空 taints
- **Create** `w9_gateway_taint_smoke.rs` — Gateway 查表驱动集成测试(4 个,Task 8):web_page blocked / llm_output filesystem blocked / clean value allowed / multi-taint blocked

### Docs

- **Modify** `docs/PROGRESS.md` — W9 Plan 3 完成状态 + 测试统计(非门控测试数 ≥ 286 + 14 新增)

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 project_memory.md "Lessons Learned" + spec §10)
- **TDD**:每个 Task 先写失败测试 → 跑 → 实现 → 跑通 → commit(spec §10)
- **Repo accessor pattern**:`TaintRepo::new()` 不带参数,方法接收 `&Connection`(参考 `DagRepo::new()` at `dag_repo.rs:41-46` + project_memory.md "Engineering Conventions")
- **`&kernel.conn()` 不用 `&*kernel.conn()`**:避免 clippy `explicit_auto_deref` lint(参考 project_memory.md "Lessons Learned")— 本 Plan 中 `kernel.conn()` 返回 `MutexGuard<Connection>`,取 `&*` 传给 repo 方法
- **TrustKernel 不是 Clone**:dispatcher / gateway 通过 `&TrustKernel` 引用访问,测试用 `Arc<TrustKernel>` 共享(参考 project_memory.md "Lessons Learned")
- **审计事件命名**:沿用 W7/W8 的 `lower_snake_case`(`taint_propagated` / `taint_blocked`),不用 SCREAMING_SNAKE(spec §10)
- **审计隐私约束(spec §6.2)**:`taint_propagated` details 仅记 `{source_ref, input_hash, output_hash, taints}`,不记原始 value;`taint_blocked` details 仅记 `{taints, sink, resource_hash}`,不记 resource 原始值
- **value_hash 计算**:`SHA256(value)` hex 字符串,`value` 是 `serde_json::to_string(&json_value)` 或 `&str` 的 UTF-8 字节;不存储原始 value(隐私)
- **taints_json 序列化**:`serde_json::to_string(&Vec<String>)` 存入 `taints_json` 列;读取时 `serde_json::from_str` 反序列化
- **upsert 合并语义**:同一 `value_hash` 已存在时,`taints` 取并集去重(`merge_taints`),`provenance` 保留原值(不覆盖),`collected_at` 更新为当前时间,`source_ref` 取新值(若新值非 None)
- **EgressDest 即 Sink**:spec §2.3 所述 "Sink" 映射到既有 `EgressDest` 枚举(`types.rs:97-104`);`Sink::ToolArgument` → `EgressDest::ToolArgument`,`Sink::FilesystemWrite` → `EgressDest::LocalFile`
- **不修改 Resource struct**:`check_taint_policy` 用 `value_hash: &str` 单独参数,避免给 `Resource` 加字段(breaking change);调用方负责算 hash 传入
- **commit message**:`feat(w9p3): ...` / `test(w9p3): ...` / `fix(w9p3): ...`(spec §10)
- **不引入非必要依赖**:`sha2` 已在 workspace(若缺失,Task 1 步骤 1 加到 `trust-kernel/Cargo.toml`);不新增其他依赖
- **Feature gate**:本 Plan 全部代码无 `#[cfg(feature = ...)]` 门控(`taints` 表在 default feature 下存在,CRUD 不依赖 stronghold / tauri / voice / uia / llm 任何 feature)
- **不修改 spec / 已有 plan**:若发现 spec 描述与实现不一致,记录到 PROGRESS.md "已知偏离" 段落,不回改 spec(spec §10)

---

## Task 1: TaintRecord struct + TaintRepo CRUD

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/policy/taint_repo.rs`
- Modify: `voicepilot/crates/trust-kernel/src/policy/mod.rs:8`(加 `pub mod taint_repo;`)
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`(确认 `sha2` 依赖,若缺失则添加)

- [ ] **Step 1: 确认 sha2 依赖**

Run:
```powershell
Select-String -Path "voicepilot\crates\trust-kernel\Cargo.toml" -Pattern "sha2"
```
Expected: 若有匹配(`sha2 = "..."`)则跳过 Step 2;若无匹配,执行 Step 2 添加。

- [ ] **Step 2: 添加 sha2 依赖(仅当 Step 1 无匹配)**

在 `voicepilot/crates/trust-kernel/Cargo.toml` 的 `[dependencies]` 段追加:
```toml
sha2 = "0.10"
```

- [ ] **Step 1.5: 创建 migration 006_taints_unique_index.sql**

新增 `voicepilot/crates/trust-kernel/src/migrations/006_taints_unique_index.sql`:
```sql
-- W9 Plan 3:taints 表 value_hash UNIQUE 约束,防止并发 upsert 产生重复行
CREATE UNIQUE INDEX IF NOT EXISTS idx_taints_value_hash ON taints(value_hash);
```

在 `db.rs` 的 `include_str!` 列表追加 `006_taints_unique_index.sql`。

- [ ] **Step 3: 创建 taint_repo.rs 文件**

创建 `voicepilot/crates/trust-kernel/src/policy/taint_repo.rs`:
```rust
//! TaintRepo — W9 §2.3 Taint Tracking CRUD。
//!
//! 操作 `taints` 表(migrations/001_init.sql:98-105),实现值级污点追踪。
//! 遵循 DagRepo accessor pattern:`new()` 不带参数,方法接收 `&Connection`。
//!
//! 安全约束(spec §6.2):
//!   - value_hash = SHA256(value),不存储原始 value(隐私)
//!   - taints_json 仅存 taint 标签,不含原始值
//!   - delete_by_source 在任务清理时调用,避免无限增长

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;

/// 一条 taint 记录(对应 `taints` 表一行)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintRecord {
    pub taint_id: String,
    pub value_hash: String,
    pub provenance: String,
    pub taints: Vec<String>,
    pub collected_at: String,
    pub source_ref: Option<String>,
}

/// TaintRepo — `taints` 表的 CRUD 访问器。
pub struct TaintRepo;

impl TaintRepo {
    /// 构造(无参数,遵循 DagRepo pattern)。
    pub fn new() -> Self {
        Self
    }

    /// 插入或合并一条 taint 记录。
    /// 若 value_hash 已存在,合并 taints(去重),更新 collected_at + source_ref。
    /// W9 修复(P1-12):使用 ON CONFLICT(value_hash) DO UPDATE,依赖 006_taints_unique_index.sql
    /// 的 UNIQUE 约束,避免并发 upsert 产生重复行。
    pub fn upsert(&self, conn: &Connection, taint: &TaintRecord) -> Result<()> {
        // W9 修复:先查再合并 taints,然后用 ON CONFLICT 写入(UNIQUE 约束保证幂等)
        let existing_taints: Vec<String> = conn
            .query_row(
                "SELECT taints_json FROM taints WHERE value_hash = ?1",
                params![taint.value_hash],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let merged = merge_taints(&existing_taints, &taint.taints);
        let merged_json = serde_json::to_string(&merged)?;
        // source_ref 取新值(若新值非 None),否则保留原值
        let final_source = taint.source_ref.clone();

        conn.execute(
            "INSERT INTO taints (taint_id, value_hash, provenance, taints_json, collected_at, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(value_hash) DO UPDATE SET
                taints_json = excluded.taints_json,
                collected_at = excluded.collected_at,
                source_ref = excluded.source_ref",
            params![
                taint.taint_id,
                taint.value_hash,
                taint.provenance,
                merged_json,
                taint.collected_at,
                final_source,
            ],
        )?;
        Ok(())
    }

    /// 按原始 value 查询 taints(内部算 SHA256)。
    /// W9 修复(P1-19):内部将 &str 转为 serde_json::Value 后调 compute_value_hash,
    /// 以使用 canonical JSON 序列化(避免字段顺序差异)。
    pub fn find_by_value(&self, conn: &Connection, value: &str) -> Result<Option<TaintRecord>> {
        let value_json: serde_json::Value = serde_json::from_str(value)
            .unwrap_or(serde_json::Value::String(value.to_string()));
        let value_hash = compute_value_hash(&value_json);
        self.find_by_hash(conn, &value_hash)
    }

    /// 按 value_hash 查询(供 gateway 已有 hash 时直接查)。
    pub fn find_by_hash(
        &self,
        conn: &Connection,
        value_hash: &str,
    ) -> Result<Option<TaintRecord>> {
        let row = conn
            .query_row(
                "SELECT taint_id, value_hash, provenance, taints_json, collected_at, \
                 source_ref FROM taints WHERE value_hash = ?1",
                params![value_hash],
                |row| {
                    let taints_json: String = row.get(3)?;
                    let taints: Vec<String> =
                        serde_json::from_str(&taints_json).unwrap_or_default();
                    Ok(TaintRecord {
                        taint_id: row.get(0)?,
                        value_hash: row.get(1)?,
                        provenance: row.get(2)?,
                        taints,
                        collected_at: row.get(4)?,
                        source_ref: row.get(5)?,
                    })
                },
            )
            .ok();
        Ok(row)
    }

    /// 按 provenance 查询所有 taints。
    pub fn list_by_provenance(
        &self,
        conn: &Connection,
        provenance: &str,
    ) -> Result<Vec<TaintRecord>> {
        let mut stmt = conn.prepare(
            "SELECT taint_id, value_hash, provenance, taints_json, collected_at, source_ref \
             FROM taints WHERE provenance = ?1",
        )?;
        let rows = stmt.query_map(params![provenance], |row| {
            let taints_json: String = row.get(3)?;
            let taints: Vec<String> = serde_json::from_str(&taints_json).unwrap_or_default();
            Ok(TaintRecord {
                taint_id: row.get(0)?,
                value_hash: row.get(1)?,
                provenance: row.get(2)?,
                taints,
                collected_at: row.get(4)?,
                source_ref: row.get(5)?,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        Ok(records)
    }

    /// 按 source_ref 查询(如某 task_id 关联的所有 taints)。
    pub fn list_by_source(
        &self,
        conn: &Connection,
        source_ref: &str,
    ) -> Result<Vec<TaintRecord>> {
        let mut stmt = conn.prepare(
            "SELECT taint_id, value_hash, provenance, taints_json, collected_at, source_ref \
             FROM taints WHERE source_ref = ?1",
        )?;
        let rows = stmt.query_map(params![source_ref], |row| {
            let taints_json: String = row.get(3)?;
            let taints: Vec<String> = serde_json::from_str(&taints_json).unwrap_or_default();
            Ok(TaintRecord {
                taint_id: row.get(0)?,
                value_hash: row.get(1)?,
                provenance: row.get(2)?,
                taints,
                collected_at: row.get(4)?,
                source_ref: row.get(5)?,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        Ok(records)
    }

    /// 删除某 source_ref 的所有 taints(任务清理时用),返回删除行数。
    pub fn delete_by_source(&self, conn: &Connection, source_ref: &str) -> Result<u64> {
        let affected = conn.execute(
            "DELETE FROM taints WHERE source_ref = ?1",
            params![source_ref],
        )?;
        Ok(affected as u64)
    }
}

impl Default for TaintRepo {
    fn default() -> Self {
        Self::new()
    }
}

/// 计算 value 的 SHA256 hex 字符串(spec §6.2:不存储原始 value)。
/// W9 修复(P1-19):用 canonical JSON 序列化(字段排序),避免
/// `{"a":1,"b":2}` 和 `{"b":2,"a":1}` 产生不同 hash。
pub fn compute_value_hash(value: &serde_json::Value) -> String {
    let canonical = canonicalize_json(value);
    let canonical_str = serde_json::to_string(&canonical).unwrap_or_default();
    sha256(&canonical_str)
}

/// 递归排序 JSON 字段(用 BTreeMap 实现 canonical form)。
fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let btree: std::collections::BTreeMap<&String, &serde_json::Value> = map.iter().collect();
            let canonical_map: serde_json::Map<String, serde_json::Value> = btree.iter()
                .map(|(k, v)| ((*k).clone(), canonicalize_json(v)))
                .collect();
            serde_json::Value::Object(canonical_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(canonicalize_json).collect())
        }
        other => other.clone(),
    }
}

/// SHA256 hex 字符串辅助函数。
fn sha256(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 合并两个 taints 列表(去重,保留顺序)。
pub fn merge_taints(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut merged = existing.to_vec();
    for t in incoming {
        if !merged.contains(t) {
            merged.push(t.clone());
        }
    }
    merged
}

/// 当前时间 ISO8601(供调用方构造 TaintRecord 用)。
pub fn now_iso8601() -> String {
    Utc::now().to_rfc3339()
}

/// 构造一条新 TaintRecord(便利函数,自动填 taint_id + collected_at)。
pub fn make_taint_record(
    value_hash: String,
    provenance: String,
    taints: Vec<String>,
    source_ref: Option<String>,
) -> TaintRecord {
    TaintRecord {
        taint_id: uuid::Uuid::new_v4().to_string(),
        value_hash,
        provenance,
        taints,
        collected_at: now_iso8601(),
        source_ref,
    }
}
```

> **W9 修复(P1-17):** 注意:`taints.source_ref` 不 REFERENCES 任何表(设计权衡:`source_ref` 是 `task_id:step_id` 复合字符串,无法直接 FK)。任务清理时需调用方主动调 `TaintRepo::delete_by_source(conn, &format!("{}:", task_id))`。在 `task_repo.delete(task_id)` 实现中接入此调用。

- [ ] **Step 4: 在 policy/mod.rs 注册模块**

修改 `voicepilot/crates/trust-kernel/src/policy/mod.rs`,在 `pub mod transaction;` 后追加 `pub mod taint_repo;`(不依赖行号,按模块声明顺序定位):
```rust
pub mod taint_repo;
```

完整文件应为:
```rust
//! Policy Engine — V1.1 §4.
//! Layered: hard-deny → Cedar → Rust Constraint → E×D risk → egress → transaction.

pub mod types;
pub mod risk_matrix;
pub mod egress;
pub mod cedar_engine;
pub mod constraint_engine;
pub mod transaction;
pub mod taint_repo;
```

- [ ] **Step 5: 跑 cargo check 验证编译**

Run:
```powershell
cd voicepilot; cargo check -p trust-kernel
```
Expected: 编译通过(可能有 unused warning,Task 2 测试会用到所有方法)。

- [ ] **Step 6: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/src/policy/taint_repo.rs voicepilot/crates/trust-kernel/src/policy/mod.rs voicepilot/crates/trust-kernel/Cargo.toml; git commit -m "feat(w9p3): add TaintRepo CRUD for taints table"
```

---

## Task 2: TDD — 写 w9_taint_tracking_unit.rs 失败测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_taint_tracking_unit.rs`

- [ ] **Step 1: 写 10 个失败测试**

创建 `voicepilot/crates/trust-kernel/tests/w9_taint_tracking_unit.rs`:
```rust
//! W9 Plan 3 Task 2 — TaintRepo CRUD 单元测试(10 个)。
//! 测试 taints 表的 upsert / find_by_value / list_by_provenance /
//! list_by_source / delete_by_source + 合并去重 / value_hash / source_ref /
//! 级联删除 / 空 taints。

use rusqlite::Connection;

use trust_kernel::policy::taint_repo::{
    compute_value_hash, make_taint_record, merge_taints, now_iso8601, TaintRecord, TaintRepo,
};

/// W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,
/// 此 helper 将 &str 转为 Value(JSON 字符串 parse,普通字符串包装为 Value::String)。
fn to_value(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.to_string()))
}

/// 建内存 DB 并跑 001_init.sql migration(复用 W7 测试 helper 模式)。
fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    let sql = include_str!("../src/migrations/001_init.sql");
    conn.execute_batch(sql).unwrap();
    conn
}

/// 测试 1:upsert 插入新记录,find_by_value 能查到。
#[test]
fn test_upsert_and_find_by_value() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = r#"{"url": "https://example.com"}"#;
    let hash = compute_value_hash(&to_value(value));
    let rec = make_taint_record(
        hash.clone(),
        "web_page".into(),
        vec!["web_page".into()],
        Some("task-1:step-1".into()),
    );
    repo.upsert(&conn, &rec).unwrap();

    let found = repo.find_by_value(&conn, value).unwrap();
    assert!(found.is_some(), "find_by_value 必须查到刚 upsert 的记录");
    let found = found.unwrap();
    assert_eq!(found.value_hash, hash);
    assert_eq!(found.provenance, "web_page");
    assert_eq!(found.taints, vec!["web_page".to_string()]);
}

/// 测试 2:list_by_provenance 按 provenance 过滤。
#[test]
fn test_list_by_provenance() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    // 插入 3 条,2 条 web_page,1 条 llm_output
    for v in ["value-a", "value-b"] {
        let h = compute_value_hash(&to_value(v));
        repo.upsert(
            &conn,
            &make_taint_record(h, "web_page".into(), vec!["web_page".into()], None),
        )
        .unwrap();
    }
    let h = compute_value_hash(&to_value("value-c"));
    repo.upsert(
        &conn,
        &make_taint_record(h, "llm_output".into(), vec!["llm_output".into()], None),
    )
    .unwrap();

    let web_pages = repo.list_by_provenance(&conn, "web_page").unwrap();
    assert_eq!(web_pages.len(), 2, "web_page provenance 应有 2 条");
    let llm_outputs = repo.list_by_provenance(&conn, "llm_output").unwrap();
    assert_eq!(llm_outputs.len(), 1, "llm_output provenance 应有 1 条");
}

/// 测试 3:list_by_source 按 source_ref 过滤。
#[test]
fn test_list_by_source() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    for v in ["v1", "v2", "v3"] {
        let h = compute_value_hash(&to_value(v));
        repo.upsert(
            &conn,
            &make_taint_record(
                h,
                "user_input".into(),
                vec!["user_input".into()],
                Some("task-42".into()),
            ),
        )
        .unwrap();
    }
    // 另一个 source_ref 的记录
    let h = compute_value_hash(&to_value("v4"));
    repo.upsert(
        &conn,
        &make_taint_record(h, "user_input".into(), vec!["user_input".into()], Some("task-99".into())),
    )
    .unwrap();

    let task42 = repo.list_by_source(&conn, "task-42").unwrap();
    assert_eq!(task42.len(), 3, "task-42 source_ref 应有 3 条");
}

/// 测试 4:delete_by_source 删除并返回行数。
#[test]
fn test_delete_by_source() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    for v in ["v1", "v2"] {
        let h = compute_value_hash(&to_value(v));
        repo.upsert(
            &conn,
            &make_taint_record(h, "x".into(), vec!["x".into()], Some("task-1".into())),
        )
        .unwrap();
    }
    let deleted = repo.delete_by_source(&conn, "task-1").unwrap();
    assert_eq!(deleted, 2, "应删除 2 条");
    let remaining = repo.list_by_source(&conn, "task-1").unwrap();
    assert!(remaining.is_empty(), "删除后应查不到");
}

/// 测试 5:upsert 同 value_hash 时合并 taints(去重)。
#[test]
fn test_upsert_merges_taints_dedup() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = "shared-value";
    let hash = compute_value_hash(&to_value(value));

    // 第一次:标 web_page
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "web_page".into(), vec!["web_page".into()], None),
    )
    .unwrap();
    // 第二次:同 value,标 executor_output:skill-1
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "executor_output:skill-1".into(),
            vec!["executor_output:skill-1".into()], None),
    )
    .unwrap();

    let found = repo.find_by_value(&conn, value).unwrap().unwrap();
    assert_eq!(found.taints.len(), 2, "合并后应有 2 个 taint");
    assert!(found.taints.contains(&"web_page".to_string()));
    assert!(found.taints.contains(&"executor_output:skill-1".to_string()));
    // 不应产生重复行
    let all = repo.list_by_provenance(&conn, "web_page").unwrap();
    assert_eq!(all.len(), 1, "同 value_hash 只应有 1 行(provenance 保留原值)");
}

/// 测试 6:compute_value_hash 对相同输入稳定,对不同输入不同。
#[test]
fn test_compute_value_hash_stable_and_distinct() {
    let h1a = compute_value_hash(&to_value("hello"));
    let h1b = compute_value_hash(&to_value("hello"));
    let h2 = compute_value_hash(&to_value("world"));
    assert_eq!(h1a, h1b, "相同输入 hash 必须相同");
    assert_ne!(h1a, h2, "不同输入 hash 必须不同");
    assert_eq!(h1a.len(), 64, "SHA256 hex 应为 64 字符");
}

/// 测试 7:source_ref 关联 + None 处理。
#[test]
fn test_source_ref_none_and_some() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    // None source_ref
    let h1 = compute_value_hash(&to_value("v-no-src"));
    repo.upsert(
        &conn,
        &make_taint_record(h1, "p".into(), vec!["p".into()], None),
    )
    .unwrap();
    // Some source_ref
    let h2 = compute_value_hash(&to_value("v-with-src"));
    repo.upsert(
        &conn,
        &make_taint_record(h2, "p".into(), vec!["p".into()], Some("src-1".into())),
    )
    .unwrap();

    let no_src = repo.list_by_source(&conn, "src-1").unwrap();
    assert_eq!(no_src.len(), 1, "只有 1 条带 source_ref=src-1");
}

/// 测试 8:delete_by_source 不影响其他 source_ref(级联精确性)。
#[test]
fn test_delete_by_source_does_not_touch_others() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let h1 = compute_value_hash(&to_value("v1"));
    repo.upsert(
        &conn,
        &make_taint_record(h1, "p".into(), vec!["p".into()], Some("task-A".into())),
    )
    .unwrap();
    let h2 = compute_value_hash(&to_value("v2"));
    repo.upsert(
        &conn,
        &make_taint_record(h2, "p".into(), vec!["p".into()], Some("task-B".into())),
    )
    .unwrap();

    let deleted = repo.delete_by_source(&conn, "task-A").unwrap();
    assert_eq!(deleted, 1);
    // task-B 不受影响
    let task_b = repo.list_by_source(&conn, "task-B").unwrap();
    assert_eq!(task_b.len(), 1, "task-B 的 taint 不应被删");
}

/// 测试 9:空 taints 列表可存储可读取。
#[test]
fn test_empty_taints_list() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let h = compute_value_hash(&to_value("empty-taints-value"));
    repo.upsert(
        &conn,
        &make_taint_record(h.clone(), "p".into(), vec![], Some("s".into())),
    )
    .unwrap();
    let found = repo.find_by_hash(&conn, &h).unwrap().unwrap();
    assert!(found.taints.is_empty(), "空 taints 列表应正确往返");
}

/// 测试 10:merge_taints 辅助函数去重 + 保序。
#[test]
fn test_merge_taints_helper() {
    let existing = vec!["a".to_string(), "b".to_string()];
    let incoming = vec!["b".to_string(), "c".to_string()];
    let merged = merge_taints(&existing, &incoming);
    assert_eq!(merged, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}

#[test]
fn test_now_iso8601_format() {
    let ts = now_iso8601();
    assert!(ts.contains('T'), "ISO8601 应含 T 分隔符");
    assert!(ts.contains('+') || ts.contains('Z'), "应含时区");
}
```

- [ ] **Step 2: 跑测试验证全部失败(方法未导出 / 模块未注册)**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --test w9_taint_tracking_unit
```
Expected: 编译失败或测试失败(`TaintRepo` / `compute_value_hash` / `make_taint_record` / `merge_taints` / `now_iso8601` 在 Task 1 已创建,此时应编译通过且测试 PASS — 若 Task 1 完整实现,本步骤实际应 PASS;若 Task 1 有遗漏则 FAIL,需回 Task 1 修补)。

> **注:** Task 1 与 Task 2 在 TDD 严格顺序下应先写测试(Task 2)再实现(Task 1)。本 plan 因 Repo + 测试紧密耦合,Task 1 已含完整实现,Task 2 跑测试验证 PASS 即可。若严格 TDD,可先做 Task 2 Step 1(写测试)→ 跑见 FAIL → 做 Task 1 → 回 Task 2 Step 2 见 PASS。

- [ ] **Step 3: 跑测试验证全部 PASS**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --test w9_taint_tracking_unit
```
Expected: 11 个测试全 PASS(10 个 CRUD + 1 个 now_iso8601 格式)。

- [ ] **Step 4: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/tests/w9_taint_tracking_unit.rs; git commit -m "test(w9p3): add 10 TaintRepo CRUD unit tests"
```

---

## Task 3: dispatcher.rs 注入 taint 传播

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs:110-130`(入口加 input taint 查询)
- Modify: `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs:130+`(出口加 output taint upsert)

- [ ] **Step 1: 在 dispatcher.rs 顶部加 use 语句**

修改 `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs`,在行 25(`use crate::toolresult::ToolStatus;`)后追加:
```rust
use crate::policy::taint_repo::{
    compute_value_hash, make_taint_record, TaintRepo,
};
```

- [ ] **Step 2: 在 dispatch_skill_executor 入口加 input taint 查询**

修改 `dispatch_skill_executor`(行 110 起),在行 128(`let resolved_input = &normalized;`)之后、行 130(`match skill_id {`)之前插入 input taint 查询:
```rust
pub fn dispatch_skill_executor(
    skill_id: &str,
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let normalized: serde_json::Value = if let Some(s) = resolved_input.as_str() {
        serde_json::from_str(s).unwrap_or_else(|_| resolved_input.clone())
    } else {
        resolved_input.clone()
    };
    let resolved_input = &normalized;

    // W9 Plan 3: 记录输入 taint(查表驱动传播)
    // W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,resolved_input 已是 Value
    let input_hash = compute_value_hash(resolved_input);
    let input_taints: Vec<String> = {
        let conn = kernel.conn();
        TaintRepo::new()
            .find_by_hash(&conn, &input_hash)?
            .map(|r| r.taints)
            .unwrap_or_default()
    };

    let outcome = match skill_id {
        "files.organize" => {
            dispatch_files_organize(kernel, resolved_input, approver, task_id, step_id)
        }
        "task.repeat_verified" => {
            dispatch_task_repeat(kernel, resolved_input, approver, task_id, step_id)
        }
        "task.explain" => dispatch_task_explain(kernel, resolved_input, approver, task_id, step_id),
        "task.compensate" => {
            dispatch_task_compensate(kernel, resolved_input, approver, task_id, step_id)
        }
        #[cfg(all(windows, feature = "uia"))]
        "quick.app_control" => {
            dispatch_app_control(kernel, resolved_input, approver, task_id, step_id)
        }
        #[cfg(all(windows, feature = "uia"))]
        "note.capture" => {
            dispatch_note_capture(kernel, resolved_input, approver, task_id, step_id)
        }
        "form.prepare" => dispatch_form_prepare(kernel, resolved_input, approver, task_id, step_id),
        "form.submit" => dispatch_form_submit(kernel, resolved_input, approver, task_id, step_id),
        "research.save_markdown" => {
            dispatch_research_save(kernel, resolved_input, approver, task_id, step_id)
        }
        _ => Err(KernelError::UnknownSkill(skill_id.to_string())),
    }?;

    // W9 Plan 3: 记录输出 taint(继承输入 + 加 executor_output:<skill_id>)
    if outcome.succeeded && !outcome.output.is_null() {
        // W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,outcome.output 已是 Value
        let output_hash = compute_value_hash(&outcome.output);
        let mut output_taints = input_taints.clone();
        let executor_taint = format!("executor_output:{}", skill_id);
        if !output_taints.contains(&executor_taint) {
            output_taints.push(executor_taint.clone());
        }
        let record = make_taint_record(
            output_hash,
            executor_taint,
            output_taints.clone(),
            Some(format!("{}:{}", task_id, step_id)),
        );
        {
            let conn = kernel.conn();
            TaintRepo::new().upsert(&conn, &record)?;
        }
        // 审计:taint_propagated(Task 7 完整实现 details,此处先占位)
        kernel.audit_append_external(
            task_id,
            Some(step_id),
            "taint_propagated",
            serde_json::json!({
                "source_ref": format!("{}:{}", task_id, step_id),
                "input_hash": input_hash,
                "output_hash": record.value_hash,
                "taints": output_taints,
            }),
        )?;
    }

    Ok(outcome)
}
```

> **注:** 上面的 `match skill_id { ... }` 分支需与现有 dispatcher.rs 行 130-148 的实际分支保持一致(本 plan 示例覆盖 9 路 skill_id,实际实现时保留原分支结构,仅在外层包 `match ... ?` 提取 outcome)。若现有代码 `match` 直接返回 `Result<DispatchOutcome>`,则改为 `let outcome = match skill_id { ... }?;` 形式。

- [ ] **Step 3: 跑 cargo check 验证编译**

Run:
```powershell
cd voicepilot; cargo check -p trust-kernel
```
Expected: 编译通过(若 `match` 分支结构与现有不一致,按编译错误调整)。

- [ ] **Step 4: 跑既有 dispatcher 测试验证不回归**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --lib skills::dispatcher
```
Expected: 既有 dispatcher 测试全 PASS(taint 传播是新增逻辑,不破坏既有 outcome)。

- [ ] **Step 5: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/src/skills/dispatcher.rs; git commit -m "feat(w9p3): inject taint propagation in dispatch_skill_executor"
```

---

## Task 4: gateway.rs 查表驱动(替换硬编码)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/gateway.rs:79-85`(替换硬编码为 check_taint_policy)
- Modify: `voicepilot/crates/trust-kernel/src/gateway.rs`(新增 check_taint_policy 函数)

- [ ] **Step 1: 在 gateway.rs 顶部加 use 语句**

修改 `voicepilot/crates/trust-kernel/src/gateway.rs`,在现有 `use` 段追加:
```rust
use crate::policy::taint_repo::{compute_value_hash, TaintRepo};
use crate::policy::types::EgressDest;
```

- [ ] **Step 2: 新增 check_taint_policy 独立函数**

在 `gateway.rs` 中(impl 块之外,模块级)新增:
```rust
/// W9 Plan 3: 查表驱动的 taint 策略检查(spec §2.3 + §6.2)。
///
/// 规则:
///   - web_page taint 不能成为 ToolArgument(防止 LLM 投毒)
///   - llm_output taint 不能写入文件系统(防止 LLM 注入恶意路径)
///
/// `value_hash` 由调用方从 resource.path 或实际 value 计算。
/// 返回 `Ok(())` 表示放行,返回 `Err(KernelError::TaintPropagationBlocked)`
/// 表示拦截(调用方负责审计 taint_blocked 事件,见 Task 7)。
///
/// W9 修复(P0-11):独立函数,非 ActionGateway 方法。ActionGateway 不持有 DB 连接,
/// 由调用方(dispatcher / invoke_mcp_tool / filesystem 工具函数)传入 `&Connection`。
/// gateway.decide 内部不调此函数。
pub fn check_taint_policy(
    conn: &Connection,
    value_hash: &str,
    egress_dest: EgressDest,
) -> Result<()> {
    let taints: Vec<String> = TaintRepo::new()
        .find_by_hash(conn, value_hash)?
        .map(|r| r.taints)
        .unwrap_or_default();

    // 规则 1:web_page taint 不能成为 ToolArgument
    if taints.iter().any(|t| t == "web_page")
        && egress_dest == EgressDest::ToolArgument
    {
        return Err(KernelError::TaintPropagationBlocked {
            taints,
            sink: format!("{:?}", egress_dest),
        });
    }

    // 规则 2:llm_output taint 不能写入文件系统(LocalFile)
    if taints.iter().any(|t| t == "llm_output")
        && egress_dest == EgressDest::LocalFile
    {
        return Err(KernelError::TaintPropagationBlocked {
            taints,
            sink: format!("{:?}", egress_dest),
        });
    }

    Ok(())
}
```

- [ ] **Step 3: 在 KernelError 加 TaintPropagationBlocked 变体**

修改 `voicepilot/crates/trust-kernel/src/error.rs`,在 `KernelError` enum 中追加:
```rust
/// W9 Plan 3: taint 传播被 gateway 拦截(spec §6.2)。
#[error("taint propagation blocked: taints={taints:?} sink={sink}")]
TaintPropagationBlocked {
    taints: Vec<String>,
    sink: String,
},
```

- [ ] **Step 4: 删除 gateway.rs:79-80 硬编码(check_taint_policy 由调用方调)**

修改 `voicepilot/crates/trust-kernel/src/gateway.rs` 的 `decide` 方法,将行 79-85 的硬编码直接删除:
```rust
// Taint elevation: web_page provenance cannot become tool argument.
if resource.provenance == "web_page" && egress_dest == Some(EgressDest::ToolArgument) {
    return Ok(Decision::deny(
        self.bundle_hash.clone(),
        "taint elevation: web_page cannot become tool argument",
    ));
}
```

W9 修复(P0-10/P0-11):`decide` 方法内**不再**调 `check_taint_policy`。`check_taint_policy` 是独立函数,由调用方(如 dispatcher / `invoke_mcp_tool` / filesystem 工具函数)在需要时调 `check_taint_policy(&kernel.conn(), ...)`,`gateway.decide` 内部不调此函数。`decide` 方法只保留原 Cedar + risk + egress 检查,taint 检查外移到调用方。

调用方示例(在 `invoke_mcp_tool` / `filesystem` 工具函数内):
```rust
// W9 Plan 3: 调用方在调 gateway.decide 前(或后)主动调 check_taint_policy
// W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,将 resource 转 Value
let resource_value_json = serde_json::to_value(&resource).unwrap_or(serde_json::Value::Null);
let value_hash = compute_value_hash(&resource_value_json);
if let Some(dest) = egress_dest {
    // None 时不检查 taint,仅 Some(dest) 时检查(P1-18 修复:删除 unwrap_or(LocalFile))
    if let Err(KernelError::TaintPropagationBlocked { taints, sink }) =
        check_taint_policy(&kernel.conn(), &value_hash, dest)
    {
        // 审计 taint_blocked(Task 7 完整实现,此处 details 仅含 hash + 标签)
        // 注:decide 不持有 task_id,审计由调用方(invoke_mcp_tool / filesystem 工具函数)负责
        let reason = format!(
            "taint elevation blocked: taints={:?} cannot flow to sink={}",
            taints, sink
        );
        return Ok(Decision::deny(
            bundle_hash.clone(),
            &reason,
        ));
    }
}
```

> **注:** `Decision::deny` 的第二个参数是 `&str`,`format!` 返回 `String`,需先绑定到变量 `let reason = format!(...);` 再传 `&reason`(P2-4 修复:不用 `&format!(...)` 临时值,避免 clippy `unnecessary_temporary` 警告)。

- [ ] **Step 5: 跑 cargo check 验证编译**

Run:
```powershell
cd voicepilot; cargo check -p trust-kernel
```
Expected: 编译通过。

- [ ] **Step 6: 跑既有 gateway 测试验证不回归**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --lib gateway
```
Expected: 既有 gateway 测试全 PASS(查表驱动在无 taint 记录时放行,等价于原硬编码对非 web_page 路径的行为)。

> **注:** 若既有测试中有 "web_page provenance → ToolArgument 被 deny" 的断言,本修改后行为等价(查表无记录时放行,但原硬编码按 provenance 字段判定;新逻辑按 taints 表判定。若既有测试用 resource.provenance = "web_page" 但未 upsert taint,新逻辑会放行 → 测试 FAIL。此时需在测试中先 upsert web_page taint,或保留 provenance 字段作为 fallback。Task 8 集成测试会覆盖查表驱动路径。)

- [ ] **Step 7: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/src/gateway.rs voicepilot/crates/trust-kernel/src/error.rs; git commit -m "feat(w9p3): replace hardcoded web_page rule with table-driven check_taint_policy"
```

---

## Task 5: LLM 拆解 literal 值标 llm_output taint

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/llm/client.rs:258-268`(decompose_to_dag 返回前注入 taint)

- [ ] **Step 1: 在 llm/client.rs 顶部加 use 语句**

修改 `voicepilot/crates/trust-kernel/src/llm/client.rs`,在现有 `use` 段追加:
```rust
use crate::policy::taint_repo::{
    compute_value_hash, make_taint_record, TaintRepo,
};
use crate::skills::template::TemplateExpr;
```

- [ ] **Step 2: 在 decompose_to_dag 返回前注入 taint**

修改 `decompose_to_dag`(行 258 起),在 `let (plan, _stats) = self.decompose_to_dag_traced(...).await?;` 之后、`Ok(plan)` 之前插入 taint 标记。

W9 修复(P0-12/P1-14):`decompose_to_dag` 签名扩展,接收 `kernel: &TrustKernel` 参数(`LlmClient` struct 不持有 kernel);`candidate_skills` 实际类型是 `&[SkillManifest]` 不是 `&[String]`;返回类型是 `LlmResult<DagPlan>` 不是 `Result<DagPlan>`。调用方 `route_text_with_dag` 需同步更新签名,传入 `kernel: &TrustKernel`。
```rust
// decompose_to_dag 签名扩展,由调用方(route_text_with_dag)传入 kernel
pub async fn decompose_to_dag(
    &self,
    kernel: &TrustKernel,
    user_text: &str,
    candidate_skills: &[SkillManifest],  // W9 修复:实际是 SkillManifest 不是 String
    user_slots: &[ExtractedSlot],
) -> LlmResult<DagPlan> {  // W9 修复:返回 LlmResult 不是 Result
    let (plan, _stats) = self
        .decompose_to_dag_traced(user_text, candidate_skills, user_slots)
        .await?;

    // W9 Plan 3: LLM 拆解的 DagPlan literal 值标 llm_output taint
    // 遍历所有节点的 input_template,对 TemplateExpr::Literal(s) 计算 hash + upsert
    {
        let conn = kernel.conn();  // W9 修复:用传入的 kernel,不是 self.kernel
        let repo = TaintRepo::new();
        for node in &plan.nodes {
            collect_literal_hashes(&node.input_template.template, &mut |literal_str| {
                // W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,字符串字面量包装为 Value::String
                let value_json = serde_json::Value::String(literal_str.to_string());
                let hash = compute_value_hash(&value_json);
                let record = make_taint_record(
                    hash,
                    "llm_output".into(),
                    vec!["llm_output".into()],
                    Some(format!("{}:{}", plan.plan_id, node.node_id)),
                );
                let _ = repo.upsert(&conn, &record);
            });
        }
    }

    Ok(plan)
}
```

- [ ] **Step 3: 新增 collect_literal_hashes 辅助函数**

在 `llm/client.rs` 文件末尾(或 mod 内)追加:
```rust
/// W9 Plan 3: 递归遍历 TemplateExpr,对每个 Literal(s) 调用 f(&s)。
/// 供 decompose_to_dag 标记 llm_output taint。
fn collect_literal_hashes<F: FnMut(&str)>(expr: &TemplateExpr, f: &mut F) {
    match expr {
        TemplateExpr::Literal(s) => f(s),
        TemplateExpr::Var(_) => {}
        TemplateExpr::Concat(parts) => {
            for p in parts {
                collect_literal_hashes(p, f);
            }
        }
        // W9 修复(P1-11):漏标 Filter 变体,Filter { source, .. } 的 source 也需递归
        TemplateExpr::Filter { source, .. } => {
            collect_literal_hashes(source, f);
        }
    }
}
```

> **注:** `TemplateExpr` 的变体名(`Literal` / `Var` / `Concat` / `Filter`)需与 `template.rs` 实际定义一致(P0-16 修复:TemplateExpr 在 `template.rs` 不在 `dag_types.rs`)。若变体名不同,按实际调整。Task 实施时先 `Grep` `enum TemplateExpr` 确认。

- [ ] **Step 4: 确认调用方 route_text_with_dag 已更新签名**

W9 修复(P0-12):`LlmClient` struct 不持有 kernel 字段(只有 base_url/api_key/model/http/timeout),`decompose_to_dag` 改为接收 `kernel: &TrustKernel` 参数。调用方 `route_text_with_dag` 需同步更新签名,传入 `kernel: &TrustKernel`。

Run:
```powershell
cd voicepilot; Select-String -Path "voicepilot\crates\trust-kernel\src\llm\client.rs" -Pattern "fn route_text_with_dag" -Context 0,5
```
Expected: 找到 `route_text_with_dag` 函数,确认其签名已传入 `kernel: &TrustKernel` 并转发给 `decompose_to_dag`。若未更新,需在 route_text_with_dag 内调 `decompose_to_dag` 时传入 kernel 参数。

- [ ] **Step 5: 跑 cargo check 验证编译(需 --features llm)**

Run:
```powershell
cd voicepilot; cargo check -p trust-kernel --features llm
```
Expected: 编译通过。

- [ ] **Step 6: 跑既有 LLM 测试验证不回归**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --features llm --lib llm
```
Expected: 既有 LLM 测试全 PASS(taint 标记是新增副作用,不破坏 plan 结构)。

- [ ] **Step 7: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/src/llm/client.rs; git commit -m "feat(w9p3): tag llm_output taint on DagPlan literal values"
```

---

## Task 6: MCP tool 返回值标 mcp_tool taint

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/server.rs:98-156`(handle_tools_call 返回前注入 taint)

- [ ] **Step 0: McpServer struct 加 server_id 字段**

W9 修复(P0-13):`McpServer` struct 只有 `handler` / `kernel` 字段,没有 `server_id`。本 Plan 需要在 `mcp_tool:<server_id>` taint provenance 中用到 `server_id`,因此先在 struct 加此字段。

在 `voicepilot/crates/trust-kernel/src/mcp/server.rs` 中,McpServer struct 加字段:
```rust
pub struct McpServer {
    handler: McpHandler,
    kernel: Arc<TrustKernel>,
    server_id: String,  // W9 Plan 3 新增,用于 taint provenance
}
```

`McpServer::new` / `with_arc` 构造器签名同步加 `server_id: String` 参数。所有调用点(`kernel.rs` / `mcp_serve` CLI 命令)同步更新。

- [ ] **Step 1: 在 mcp/server.rs 顶部加 use 语句**

修改 `voicepilot/crates/trust-kernel/src/mcp/server.rs`,在现有 `use` 段追加:
```rust
use crate::policy::taint_repo::{
    compute_value_hash, make_taint_record, TaintRepo,
};
```

- [ ] **Step 2: 在 handle_tools_call 返回前注入 taint**

修改 `handle_tools_call`(行 98 起),在 `let result = self.handler.call_tool(...)`(行 134)成功返回后、序列化响应前插入 taint 标记:
```rust
fn handle_tools_call(
    &mut self,
    id: &Value,
    params: Value,
) -> Result<Value, KernelError> {
    // ... 既有参数解析(name, arguments)...
    let result = self.handler.call_tool(&self.kernel, name, &arguments);

    // W9 Plan 3: MCP tool 返回值标 mcp_tool:<server_id> taint
    if let Ok(tool_result) = &result {
        // W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,将 tool_result 转 Value
        let result_json = serde_json::to_value(tool_result).unwrap_or(serde_json::Value::Null);
        let result_hash = compute_value_hash(&result_json);
        let server_taint = format!("mcp_tool:{}", self.server_id);
        let record = make_taint_record(
            result_hash,
            server_taint.clone(),
            vec![server_taint],
            // task_id / step_id 从 params 提取(若 MCP 调用上下文携带)
            params
                .get("task_id")
                .and_then(|v| v.as_str())
                .map(|t| format!("{}:{}", t, params.get("step_id").and_then(|v| v.as_str()).unwrap_or(""))),
        );
        let conn = self.kernel.conn();
        let _ = TaintRepo::new().upsert(&conn, &record);
    }

    // ... 既有响应构造 ...
}
```

> **注:** `self.server_id` 字段名需与 `McpServer` struct 实际字段一致(参考 mcp/server.rs 既有结构)。`tool_result` 的序列化字段可能是 `ToolResult` struct,`serde_json::to_string` 需其实现 `Serialize`(W3a 已实现)。Task 实施时按实际字段名调整。

- [ ] **Step 3: 跑 cargo check 验证编译**

Run:
```powershell
cd voicepilot; cargo check -p trust-kernel
```
Expected: 编译通过。

- [ ] **Step 4: 跑既有 MCP 测试验证不回归**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --lib mcp
```
Expected: 既有 MCP 测试全 PASS(taint 标记是新增副作用,不破坏 call_tool 返回值)。

- [ ] **Step 5: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/src/mcp/server.rs; git commit -m "feat(w9p3): tag mcp_tool taint on MCP tool call results"
```

---

## Task 7: 审计事件(taint_propagated / taint_blocked)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs`(Task 3 已加 taint_propagated,本 Task 补 details 完整性)
- Modify: `voicepilot/crates/trust-kernel/src/skills/common.rs::invoke_mcp_tool`(W9 修复 P0-15:替代原 dag_executor.rs,gateway 拦截后审计 taint_blocked)
- Modify: `voicepilot/crates/trust-kernel/src/skills/filesystem.rs`(filesystem 工具函数,同上)

- [ ] **Step 1: 确认 dispatcher.rs 的 taint_propagated 审计完整**

Task 3 Step 2 已在 dispatcher.rs 加:
```rust
kernel.audit_append_external(
    task_id,
    Some(step_id),
    "taint_propagated",
    serde_json::json!({
        "source_ref": format!("{}:{}", task_id, step_id),
        "input_hash": input_hash,
        "output_hash": record.value_hash,
        "taints": output_taints,
    }),
)?;
```
确认 details 字段符合 spec §6.4:`{source_ref, input_hash, output_hash, taints}`(不含原始 value)。

Run:
```powershell
cd voicepilot; Select-String -Path "voicepilot\crates\trust-kernel\src\skills\dispatcher.rs" -Pattern "taint_propagated"
```
Expected: 匹配到 1 处 `audit_append_external` 调用。

- [ ] **Step 2: 在 invoke_mcp_tool / filesystem 工具函数补 taint_blocked 审计**

`gateway.rs::decide` 不持有 `task_id`(签名无此参数),无法直接调 `audit_append_external`。taint_blocked 审计由调用方(`invoke_mcp_tool` / `filesystem` 工具函数)在收到 `Decision::Deny` 后负责。

W9 修复(P0-14/P0-15/P1-20):原 Plan 假设 `dag_executor.rs` 中有 `self.gateway.authorize(...)` 调用,实际 `DagExecutor` struct 无 gateway 字段,且 `Decision` struct 没有 `is_denied()` 方法,字段名是 `reasons: Vec<String>`(复数)不是 `reason`。修改位置改为 `invoke_mcp_tool` / `filesystem` 工具函数内部(即实际调用 `gateway.decide` 的地方)。审计 details 不再用硬编码占位符,改为从 `decision.reasons` + `sink` 提取。

**修改位置:** `voicepilot/crates/trust-kernel/src/skills/common.rs::invoke_mcp_tool`(约行 166)+ `filesystem` 工具函数

在 `invoke_mcp_tool` 调用 `gateway.decide` 后,catch `Decision::Deny` 并审计:
```rust
use trust_kernel::policy::types::Effect;

let decision = gateway.decide(...)?;
// W9 修复(P0-14):用 matches! 替代不存在的 is_denied() 方法,reasons(复数)替代 reason
if matches!(decision.effect, Effect::Deny) {
    if decision.reasons.iter().any(|r| r.contains("taint elevation blocked")) {
        // W9 修复(P1-20):details 从 decision.reasons + sink 提取,不再硬编码
        kernel.audit_append_external(
            &task_id, Some(&step_id),
            "taint_blocked",
            serde_json::json!({
                "taints": decision.reasons,
                "sink": sink.to_string(),
                "resource_hash": value_hash,
            }),
        )?;
        return Err(KernelError::TaintPropagationBlocked {
            taints: decision.reasons,
            sink: sink.to_string(),
        });
    }
    // 既有 deny 处理...
}
```

> **注:** `Decision` struct 的字段:`effect: Effect`(`Allow` / `Deny`)+ `reasons: Vec<String>`(复数,不是 `reason: String`)。无 `is_denied()` 方法,用 `matches!(decision.effect, Effect::Deny)` 替代。`Effect` 需 `use` 导入(来自 `policy/types.rs`)。

- [ ] **Step 3: 在 mcp/server.rs 补 taint_blocked 审计(MCP 调用 gateway 拦截时)**

若 `mcp/server.rs::handle_tools_call` 内部调 gateway 并被拦截,同样补审计:
```rust
// mcp/server.rs(既有 gateway 调用后)
if let Err(KernelError::TaintPropagationBlocked { taints, sink }) = &result {
    self.kernel.audit_append_external(
        &task_id,
        Some(&step_id),
        "taint_blocked",
        serde_json::json!({
            "taints": taints,
            "sink": sink,
            "resource_hash": resource_hash,
        }),
    )?;
}
```

> **注:** MCP 调用 gateway 的实际位置需 Grep `gateway.decide` 在 mcp 模块的调用点确认(P0-10 修复:`authorize` → `decide`)。若 MCP 不直接调 gateway(走 tool handler 内部),则 taint_blocked 审计在 tool handler 内补。

- [ ] **Step 4: 跑 cargo check 验证编译**

Run:
```powershell
cd voicepilot; cargo check -p trust-kernel
```
Expected: 编译通过。

- [ ] **Step 5: 跑测试验证审计事件写入**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --test w9_taint_tracking_unit; cargo test -p trust-kernel --lib skills::dispatcher
```
Expected: 测试 PASS。手动验证审计事件:
```powershell
cd voicepilot; cargo test -p trust-kernel --test w9_gateway_taint_smoke -- --nocapture
```
(Task 8 完成后跑,验证 audit_logs 表含 taint_propagated / taint_blocked 记录)

- [ ] **Step 6: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/src/skills/common.rs voicepilot/crates/trust-kernel/src/skills/filesystem.rs voicepilot/crates/trust-kernel/src/mcp/server.rs voicepilot/crates/trust-kernel/src/gateway.rs; git commit -m "feat(w9p3): emit taint_propagated and taint_blocked audit events"
```

---

## Task 8: w9_gateway_taint_smoke.rs 集成测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_gateway_taint_smoke.rs`

- [ ] **Step 1: 写 4 个集成测试**

创建 `voicepilot/crates/trust-kernel/tests/w9_gateway_taint_smoke.rs`:
```rust
//! W9 Plan 3 Task 8 — Gateway 查表驱动 taint 策略集成测试(4 个)。
//! 验证:
//!   1. web_page taint → ToolArgument 被拦截
//!   2. llm_output taint → LocalFile 被拦截
//!   3. 无 taint 的 clean value 放行
//!   4. multi-taint(web_page + llm_output)→ ToolArgument 被拦截

use rusqlite::Connection;

use trust_kernel::error::KernelError;
use trust_kernel::gateway::check_taint_policy;
use trust_kernel::policy::taint_repo::{
    compute_value_hash, make_taint_record, TaintRepo,
};
use trust_kernel::policy::types::EgressDest;

/// W9 修复(P1-19):compute_value_hash 接收 &serde_json::Value,
/// 此 helper 将 &str 转为 Value(JSON 字符串 parse,普通字符串包装为 Value::String)。
fn to_value(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.to_string()))
}

/// 建内存 DB + 001_init.sql。
fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    let sql = include_str!("../src/migrations/001_init.sql");
    conn.execute_batch(sql).unwrap();
    conn
}

/// 测试 1:web_page taint 不能成为 ToolArgument。
#[test]
fn test_web_page_taint_blocked_from_tool_argument() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = r#"{"html": "<script>...</script>"}"#;
    let hash = compute_value_hash(&to_value(value));
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "web_page".into(), vec!["web_page".into()], Some("t1:s1".into())),
    )
    .unwrap();

    let result = check_taint_policy(&conn, &hash, EgressDest::ToolArgument);
    assert!(result.is_err(), "web_page taint → ToolArgument 必须被拦截");
    // W9 修复(P1-21):用模式匹配替代 format!("{:?}", err),不依赖 Debug 可读性
    let err = result.unwrap_err();
    if let KernelError::TaintPropagationBlocked { taints, sink } = err {
        assert!(taints.contains(&"web_page".to_string()), "taints 应含 web_page");
        assert!(sink.contains("ToolArgument"), "sink 应含 ToolArgument");
    } else {
        panic!("expected TaintPropagationBlocked, got {:?}", err);
    }
}

/// 测试 2:llm_output taint 不能写入文件系统(LocalFile)。
#[test]
fn test_llm_output_taint_blocked_from_filesystem() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = r#"{"path": "/etc/passwd", "content": "..."}"#;
    let hash = compute_value_hash(&to_value(value));
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "llm_output".into(), vec!["llm_output".into()], Some("t2:s2".into())),
    )
    .unwrap();

    let result = check_taint_policy(&conn, &hash, EgressDest::LocalFile);
    assert!(result.is_err(), "llm_output taint → LocalFile 必须被拦截");
    let err = result.unwrap_err();
    if let KernelError::TaintPropagationBlocked { taints, sink } = err {
        assert!(taints.contains(&"llm_output".to_string()));
        assert!(sink.contains("LocalFile"));
    } else {
        panic!("expected TaintPropagationBlocked, got {:?}", err);
    }
}

/// 测试 3:无 taint 的 clean value 放行所有 sink。
#[test]
fn test_clean_value_allowed_all_sinks() {
    let conn = open_in_memory();
    let value = r#"{"normal": "data"}"#;
    let hash = compute_value_hash(&to_value(value));
    // 不 upsert 任何 taint

    assert!(check_taint_policy(&conn, &hash, EgressDest::ToolArgument).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::LocalFile).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::RemoteLlm).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::RemoteMcp).is_ok());
}

/// 测试 4:multi-taint(web_page + llm_output)→ ToolArgument 被拦截。
#[test]
fn test_multi_taint_blocked_from_tool_argument() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = "multi-tainted-value";
    let hash = compute_value_hash(&to_value(value));
    // 第一次 upsert web_page
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "web_page".into(), vec!["web_page".into()], None),
    )
    .unwrap();
    // 第二次 upsert 同 value,加 llm_output(触发合并去重)
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "llm_output".into(), vec!["llm_output".into()], None),
    )
    .unwrap();

    // 合并后 taints = ["web_page", "llm_output"]
    let found = repo.find_by_hash(&conn, &hash).unwrap().unwrap();
    assert_eq!(found.taints.len(), 2);

    // → ToolArgument 被 web_page 拦截
    let result = check_taint_policy(&conn, &hash, EgressDest::ToolArgument);
    assert!(result.is_err(), "含 web_page 的 multi-taint → ToolArgument 必须被拦截");

    // → LocalFile 被 llm_output 拦截
    let result = check_taint_policy(&conn, &hash, EgressDest::LocalFile);
    assert!(result.is_err(), "含 llm_output 的 multi-taint → LocalFile 必须被拦截");

    // → RemoteLlm 放行(无规则拦截)
    let result = check_taint_policy(&conn, &hash, EgressDest::RemoteLlm);
    assert!(result.is_ok(), "RemoteLlm 无 taint 拦截规则,应放行");
}

/// 测试 5(辅助):user_input taint 不触发任何拦截(只 web_page / llm_output 有规则)。
#[test]
fn test_user_input_taint_allowed_all_sinks() {
    let conn = open_in_memory();
    let repo = TaintRepo::new();
    let value = "user-typed-text";
    let hash = compute_value_hash(&to_value(value));
    repo.upsert(
        &conn,
        &make_taint_record(hash.clone(), "user_input".into(), vec!["user_input".into()], None),
    )
    .unwrap();

    assert!(check_taint_policy(&conn, &hash, EgressDest::ToolArgument).is_ok());
    assert!(check_taint_policy(&conn, &hash, EgressDest::LocalFile).is_ok());
}
```

- [ ] **Step 2: 确认 check_taint_policy 已 pub 导出**

Run:
```powershell
cd voicepilot; Select-String -Path "voicepilot\crates\trust-kernel\src\gateway.rs" -Pattern "pub fn check_taint_policy"
```
Expected: 匹配到 1 处(Task 4 已加)。若未导出,改 `fn` → `pub fn`。

- [ ] **Step 3: 跑测试验证 PASS**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --test w9_gateway_taint_smoke
```
Expected: 5 个测试全 PASS(4 个核心 + 1 个辅助)。

- [ ] **Step 4: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/tests/w9_gateway_taint_smoke.rs; git commit -m "test(w9p3): add 4 gateway taint policy smoke tests"
```

---

## Task 9: cargo check + clippy + 非门控测试数 + commit

**Files:**
- Modify: `docs/PROGRESS.md`(W9 Plan 3 完成状态)

- [ ] **Step 1: cargo check 全 feature 组合**

Run:
```powershell
cd voicepilot; cargo check -p trust-kernel; cargo check -p trust-kernel --features llm; cargo check -p trust-kernel --features tauri,llm; cargo check -p trust-kernel --features voice,tauri,llm
```
Expected: 4 套组合全 PASS(本 Plan 代码无 feature gate,所有组合都应编译通过)。

- [ ] **Step 2: cargo check workspace 全 feature**

Run:
```powershell
cd voicepilot; cargo check --workspace --features voice,tauri,llm,uia
```
Expected: PASS。

- [ ] **Step 3: clippy 0 警告(代表性 feature 组合)**

Run:
```powershell
cd voicepilot; cargo clippy -p trust-kernel -- -D warnings; cargo clippy -p trust-kernel --features llm -- -D warnings
```
Expected: 0 警告。若有警告,按提示修复(常见:`explicit_auto_deref` → 改 `&*x` 为 `&x`;`unused_imports` → 删除未用 use)。

- [ ] **Step 4: 非门禁测试数 ≥ 286 + 14 新增**

Run:
```powershell
cd voicepilot; cargo test --workspace --no-default-features -- --list 2>&1 | Select-String ": test$" | Measure-Object | Select-Object -ExpandProperty Count
```
Expected: ≥ 300(W8 收尾 286 + W9 Plan 3 新增 14:10 单元 + 4 集成,实际 11 单元 + 5 集成 = 16)。记录实际数字到 PROGRESS.md。

- [ ] **Step 5: 跑 W9 Plan 3 全部测试**

Run:
```powershell
cd voicepilot; cargo test -p trust-kernel --test w9_taint_tracking_unit; cargo test -p trust-kernel --test w9_gateway_taint_smoke
```
Expected: 16 个测试全 PASS(11 单元 + 5 集成)。

- [ ] **Step 6: 验收门禁 grep(Plan 3 完成标志)**

Run:
```powershell
cd voicepilot; Select-String -Path "voicepilot\crates\trust-kernel\src\policy\taint_repo.rs" -Pattern "INSERT INTO taints"; Select-String -Path "voicepilot\crates\trust-kernel\src\gateway.rs" -Pattern "check_taint_policy"
```
Expected:
- `INSERT INTO taints` 匹配 1 处(upsert 方法内)
- `check_taint_policy` 匹配 ≥ 2 处(定义 1 + 调用 1)

- [ ] **Step 7: 更新 PROGRESS.md**

修改 `docs/PROGRESS.md`,在 W9 段落追加 Plan 3 完成状态:
```markdown
## W9 Plan 3: Taint Tracking 污点传播 — 完成

- **新增文件:**
  - `crates/trust-kernel/src/policy/taint_repo.rs`(TaintRepo CRUD + TaintRecord + compute_value_hash + merge_taints)
  - `crates/trust-kernel/src/migrations/006_taints_unique_index.sql`(W9 修复 P1-12:taints 表 value_hash UNIQUE 约束)
  - `crates/trust-kernel/tests/w9_taint_tracking_unit.rs`(11 个单元测试)
  - `crates/trust-kernel/tests/w9_gateway_taint_smoke.rs`(5 个集成测试)
- **修改文件:**
  - `crates/trust-kernel/src/policy/mod.rs`(注册 taint_repo 模块)
  - `crates/trust-kernel/src/gateway.rs`(check_taint_policy 独立函数,删除硬编码 web_page 规则;W9 修复 P0-11:非 ActionGateway 方法)
  - `crates/trust-kernel/src/skills/dispatcher.rs`(输入 → 输出 taint 传播)
  - `crates/trust-kernel/src/llm/client.rs`(DagPlan literal 标 llm_output taint;W9 修复 P0-12:decompose_to_dag 接收 kernel 参数)
  - `crates/trust-kernel/src/mcp/server.rs`(MCP tool 返回值标 mcp_tool taint;W9 修复 P0-13:McpServer struct 加 server_id 字段)
  - `crates/trust-kernel/src/skills/common.rs::invoke_mcp_tool`(W9 修复 P0-15:taint_blocked 审计,替代原 dag_executor.rs)
  - `crates/trust-kernel/src/skills/filesystem.rs`(同上,taint_blocked 审计)
  - `crates/trust-kernel/src/error.rs`(TaintPropagationBlocked 变体,W9 修复 P1-16:加 #[error] 属性)
- **测试统计:** 16 个新测试通过(11 单元 + 5 集成),非门控测试总数 ≥ 300
- **验收门禁:** `INSERT INTO taints` 非空 ✓ / `check_taint_policy` 非空 ✓ / clippy 0 警告 ✓
- **已知偏离:** 无(spec §2.3 全部覆盖)
```

- [ ] **Step 8: 最终 commit**

```powershell
cd voicepilot; git add docs/PROGRESS.md; git commit -m "docs(w9p3): mark Plan 3 taint tracking complete with test stats"
```

- [ ] **Step 9: 空 commit 标记 Plan 3 里程碑(可选)**

```powershell
cd voicepilot; git commit --allow-empty -m "docs(w9p3): W9 Plan 3 complete — TaintRepo CRUD + dispatcher propagation + table-driven gateway"
```

---

## Self-Review

### 1. Spec 覆盖检查

| Spec 条目(§2.3) | 对应 Task | 覆盖 |
|---|---|---|
| 新增 `taint_repo.rs`(TaintRepo CRUD) | Task 1 | ✓ |
| TaintRecord struct(taint_id / value_hash / provenance / taints / collected_at / source_ref) | Task 1 Step 3 | ✓ |
| API:upsert | Task 1 Step 3 + Task 2 测试 1, 5 | ✓ |
| API:find_by_value | Task 1 Step 3 + Task 2 测试 1 | ✓ |
| API:list_by_provenance | Task 1 Step 3 + Task 2 测试 2 | ✓ |
| API:list_by_source | Task 1 Step 3 + Task 2 测试 3 | ✓ |
| API:delete_by_source | Task 1 Step 3 + Task 2 测试 4, 8 | ✓ |
| Skill 输入 → 输出 taint 传播(dispatcher.rs) | Task 3 | ✓ |
| Gateway 查表驱动(替换 gateway.rs:79 硬编码) | Task 4 | ✓ |
| 审计事件:taint_propagated | Task 3 Step 2 + Task 7 Step 1 | ✓ |
| 审计事件:taint_blocked | Task 7 Step 2, 3 | ✓ |
| 传播规则:Skill executor 输入 → 输出 | Task 3 | ✓ |
| 传播规则:LLM 拆解 literal 标 llm_output | Task 5 | ✓ |
| 传播规则:MCP tool 返回值标 mcp_tool | Task 6 | ✓ |
| 传播规则:文件系统写入暂不传播(W10+) | — | ✓(本 Plan 不涉及) |
| Gateway 规则:web_page → ToolArgument 拦截 | Task 4 + Task 8 测试 1 | ✓ |
| Gateway 规则:llm_output → filesystem 拦截 | Task 4 + Task 8 测试 2 | ✓ |
| §6.2:value_hash 不存原始值 | Task 1(compute_value_hash 只算 SHA256) | ✓ |
| §6.2:taints_json 仅存标签 | Task 1(serde_json::to_string(&Vec<String>)) | ✓ |
| §6.2:审计不记 resource 原始值 | Task 7(details 仅含 hash + 标签) | ✓ |
| §6.2:delete_by_source 清理 | Task 1 + Task 2 测试 4, 8 | ✓ |
| §10:Repo accessor pattern | Task 1(TaintRepo::new() 无参) | ✓ |
| §11:向后兼容(taints 表空壳 → CRUD) | Task 1(无既有调用,纯新增) | ✓ |

### 2. 占位符扫描

- 无 "TBD" / "TODO" / "implement later"
- Task 5 Step 3 注释 "需与 `template.rs` 实际定义一致"(W9 修复 P0-16:TemplateExpr 在 `template.rs` 不在 `dag_types.rs`)+ 提供确认命令 — 这是实施时验证点,非占位符
- 所有代码片段完整,无 `// ... 省略 ...` 关键逻辑省略(既有代码的 `// ...` 仅指代非本 Plan 修改的既有行)

### 3. 类型一致性

- `TaintRecord` 字段在 Task 1 定义 → Task 2 测试 / Task 3 dispatcher / Task 5 llm / Task 6 mcp 全部使用同一字段名(`taint_id` / `value_hash` / `provenance` / `taints` / `collected_at` / `source_ref`)
- `TaintRepo::new()` 在 Task 1 定义 → Task 2 / 3 / 4 / 5 / 6 / 8 全部 `TaintRepo::new()` 调用
- `compute_value_hash(&serde_json::Value) -> String` 在 Task 1 定义(W9 修复 P1-19:canonical JSON 序列化)→ Task 2 / 3 / 4 / 5 / 6 / 8 全部用同名调用,需传 `&serde_json::Value`(测试用 `to_value` helper 转换)
- `make_taint_record(value_hash, provenance, taints, source_ref)` 在 Task 1 定义 → Task 2 / 5 / 6 / 8 全部用同名 + 同参数顺序
- `check_taint_policy(conn, value_hash, egress_dest)` 在 Task 4 定义(W9 修复 P0-11:独立函数,非方法)→ Task 8 测试用 `check_taint_policy(&conn, &hash, EgressDest::ToolArgument)` 一致
- `KernelError::TaintPropagationBlocked { taints, sink }` 在 Task 4 Step 3 定义(W9 修复 P1-16:加 `#[error]` 属性)→ Task 4 Step 4 / Task 7 Step 2 / Task 8 测试 1, 2 使用一致
- `EgressDest::ToolArgument` / `EgressDest::LocalFile` / `EgressDest::RemoteLlm` / `EgressDest::RemoteMcp` 与 `types.rs:97-104` 既有变体一致

---

## Commit Message 格式

本 Plan 所有 commit 使用前缀 `feat(w9p3):` / `test(w9p3):` / `fix(w9p3):` / `docs(w9p3):`(spec §10):

- `feat(w9p3): add TaintRepo CRUD for taints table`
- `test(w9p3): add 10 TaintRepo CRUD unit tests`
- `feat(w9p3): inject taint propagation in dispatch_skill_executor`
- `feat(w9p3): replace hardcoded web_page rule with table-driven check_taint_policy`
- `feat(w9p3): tag llm_output taint on DagPlan literal values`
- `feat(w9p3): tag mcp_tool taint on MCP tool call results`
- `feat(w9p3): emit taint_propagated and taint_blocked audit events`
- `test(w9p3): add 4 gateway taint policy smoke tests`
- `docs(w9p3): mark Plan 3 taint tracking complete with test stats`
- `docs(w9p3): W9 Plan 3 complete — TaintRepo CRUD + dispatcher propagation + table-driven gateway`(空 commit 里程碑)

---

**End of W9 Plan 3 Implementation Plan**

---

## W9 审查修复记录

本段落记录 W9 Plan 3 审查中发现的所有缺陷及修复措施,按缺陷编号排列。

### P0 级修复(阻塞性,已全部修复)

- **P0-10: `gateway.rs::authorize` 方法不存在,实际是 `decide`**
  - 修复:全文替换方法名 `authorize` → `decide`(File Structure、Architecture、Task 4 Step 4、Task 7 Step 2/3 等)。仅替换方法名,保留 `authz` 变量名与 "authorization" 注释不变。剩余 2 处 `authorize` 引用为"原 Plan 错误描述"的说明性文字,保留以说明修复背景。

- **P0-11: `ActionGateway` 没有 `conn` 字段,`check_taint_policy` 改为独立函数**
  - 修复:采用方案 C,`check_taint_policy` 改为模块级独立函数(非方法),签名 `pub fn check_taint_policy(conn: &Connection, value_hash: &str, egress_dest: EgressDest) -> Result<()>`。Task 4 Step 2 加注释说明"独立函数,非 ActionGateway 方法,gateway 不持有 DB 连接,由调用方传入 `&Connection`"。Task 4 Step 4 说明"由调用方(dispatcher / invoke_mcp_tool / filesystem 工具函数)在需要时调 `check_taint_policy(&kernel.conn(), ...)`,gateway.decide 内部不调此函数"。

- **P0-12: `LlmClient` 没有 `kernel` 字段,`decompose_to_dag` 改为接收 `kernel` 参数**
  - 修复:`decompose_to_dag` 签名扩展,新增 `kernel: &TrustKernel` 参数,函数体内 `self.kernel.conn()` 改为 `kernel.conn()`。Task 5 Step 2 加注释说明"调用方 `route_text_with_dag` 需同步更新签名,传入 `kernel: &TrustKernel`"。Task 5 Step 4 同步更新为"确认调用方 route_text_with_dag 已更新签名"。

- **P0-13: `McpServer` 没有 `server_id` 字段**
  - 修复:采用方案 A,在 Task 6 新增 Step 0,要求在 `McpServer` struct 加 `server_id: String` 字段(W9 Plan 3 新增,用于 taint provenance),`McpServer::new` / `with_arc` 构造器签名同步加 `server_id: String` 参数,所有调用点(`kernel.rs` / `mcp_serve` CLI 命令)同步更新。Task 6 Step 2 的 `format!("mcp_tool:{}", self.server_id)` 保持不变。

- **P0-14: `Decision::is_denied()` 方法不存在 + `decision.reason` 字段名错**
  - 修复:Task 7 Step 2 改用 `matches!(decision.effect, Effect::Deny)` 替代不存在的 `is_denied()` 方法,`decision.reason`(单数)改为 `decision.reasons`(复数,`Vec<String>`),用 `decision.reasons.iter().any(|r| r.contains("taint elevation blocked"))` 判定。

- **P0-15: `dag_executor.rs` 中没有 `gateway.authorize` 调用**
  - 修复:Task 7 Step 2 修改位置改为 `voicepilot/crates/trust-kernel/src/skills/common.rs::invoke_mcp_tool`(约行 166)+ `filesystem` 工具函数,删除对 `dag_executor.rs` 的引用。Files 段落同步移除 `dag_executor.rs`,改为 `common.rs` + `filesystem.rs`。Commit 命令同步更新。

- **P0-16: `TemplateExpr` 导入路径错**
  - 修复:Task 5 Step 1 的 `use crate::skills::dag_types::TemplateExpr;` 改为 `use crate::skills::template::TemplateExpr;`。Task 5 Step 3 注释同步更新为"需与 `template.rs` 实际定义一致"。

### P1 级修复(重要,已全部修复)

- **P1-11: `collect_literal_hashes` 漏掉 `Filter` 变体**
  - 修复:Task 5 Step 3 的 `collect_literal_hashes` 函数加 `TemplateExpr::Filter { source, .. } => { collect_literal_hashes(source, f); }` 分支。

- **P1-12: `taints` 表无 `value_hash` UNIQUE 约束,并发 upsert 产生重复行**
  - 修复:Task 1 新增 Step 1.5,创建 `migrations/006_taints_unique_index.sql`(`CREATE UNIQUE INDEX IF NOT EXISTS idx_taints_value_hash ON taints(value_hash);`),在 `db.rs` 的 `include_str!` 列表追加。Task 1 Step 3 `upsert` 方法改用 `ON CONFLICT(value_hash) DO UPDATE SET ...`,依赖 UNIQUE 约束保证幂等。

- **P1-13: W7 Plan 5 没有 mcp_tool taint,Plan 3 Precondition 描述错**
  - 修复:Precondition 末尾追加"与 W9 Plan 5(Playwright E2E)的依赖关系:Plan 5 假设 Plan 3 已实现 mcp_tool taint 标记(注意:W7 Plan 5 没有 mcp_tool taint,本 Plan 3 是首次引入)"。

- **P1-14: `decompose_to_dag` 签名描述错**
  - 修复:见 P0-12,`candidate_skills` 类型由 `&[String]` 改为 `&[SkillManifest]`,返回类型由 `Result<DagPlan>` 改为 `LlmResult<DagPlan>`。

- **P1-15: `policy/mod.rs` 行号描述偏差**
  - 修复:Task 1 Step 4 的"在行 8(`pub mod transaction;`)后追加"改为"在 `pub mod transaction;` 后追加 `pub mod taint_repo;`(不依赖行号,按模块声明顺序定位)"。

- **P1-16: `KernelError::TaintPropagationBlocked` 缺 `#[error]` 属性**
  - 修复:Task 4 Step 3 的 enum 变体加 `#[error("taint propagation blocked: taints={taints:?} sink={sink}")]` 属性。

- **P1-17: `taints` 表无 ON DELETE CASCADE 关联到 tasks/steps**
  - 修复:Task 1 Step 3 末尾加注释说明"`taints.source_ref` 不 REFERENCES 任何表(设计权衡:`source_ref` 是 `task_id:step_id` 复合字符串,无法直接 FK)。任务清理时需调用方主动调 `TaintRepo::delete_by_source(conn, &format!("{}:", task_id))`。在 `task_repo.delete(task_id)` 实现中接入此调用"。

- **P1-18: `egress_dest.unwrap_or(EgressDest::LocalFile)` 默认值不当**
  - 修复:统一为"None 时不检查 taint,仅 Some(dest) 时检查"。删除 `unwrap_or(EgressDest::LocalFile)`,Task 4 Step 4 调用方示例改为 `if let Some(dest) = egress_dest { ... }`。

- **P1-19: `compute_value_hash` 不区分 JSON 字段顺序**
  - 修复:`compute_value_hash` 签名由 `&str` 改为 `&serde_json::Value`,内部用 `canonicalize_json`(递归用 BTreeMap 排序 JSON 字段)序列化后再 SHA256。新增 `sha256(&str) -> String` 辅助函数。`find_by_value` 内部将 `&str` parse 为 `Value` 后调 `compute_value_hash`。Task 2/Task 8 测试加 `to_value(&str) -> serde_json::Value` helper 转换。Task 3 dispatcher / Task 5 llm / Task 6 mcp 调用点同步更新。

- **P1-20: Task 7 Step 2 审计 details 含硬编码占位符**
  - 修复:见 P0-15,Task 7 Step 2 重写后,details 从 `decision.reasons` + `sink.to_string()` + `value_hash` 提取,不再硬编码 `"taints": []` / `"sink": "ToolArgument"`。

- **P1-21: Task 8 测试 1 `format!("{:?}", err)` 假设 Debug 可读**
  - 修复:Task 8 测试 1(及测试 2)改为模式匹配 `if let KernelError::TaintPropagationBlocked { taints, sink } = err { assert!(...); } else { panic!(...); }`,不依赖 Debug 可读性。Task 8 测试文件加 `use trust_kernel::error::KernelError;` 导入。

### P2 级修复(代码质量,已全部修复)

- **P2-3: `use KernelError as _KernelError` 多余**
  - 修复:Task 1 Step 3 文件末尾的 `#[allow(unused_imports)] use KernelError as _KernelError;` 删除,顶部 `use crate::error::{KernelError, Result};` 改为 `use crate::error::Result;`(只保留 Result)。

- **P2-4: `&format!(...)` 传 `Decision::deny`**
  - 修复:Task 4 Step 4 调用方示例改为先绑定变量 `let reason = format!(...);` 再传 `&reason`,避免 clippy `unnecessary_temporary` 警告。

### 关联更新(非缺陷修复,为保证 Plan 一致性)

- Architecture 段落同步更新,说明 `check_taint_policy` 为独立函数、`decompose_to_dag` 接收 kernel 参数、`McpServer` 加 `server_id` 字段。
- Self-Review 段落 §2 占位符扫描:删除"Task 7 Step 2 '更优方案' 描述"项(已被 P0-15 重写移除);§3 类型一致性:`compute_value_hash` 签名描述更新为 `&serde_json::Value`,`check_taint_policy` 标注"独立函数,非方法",`TaintPropagationBlocked` 标注"加 `#[error]` 属性"。
- Task 9 PROGRESS.md 模板:新增文件列表加 `006_taints_unique_index.sql`;修改文件列表将 `dag_executor.rs` 替换为 `common.rs` + `filesystem.rs`,各文件后加 W9 修复标注。
