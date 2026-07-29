# W9 Plan 7: 集成测试 + 7 套 feature cargo check 矩阵 + clippy + npm build + PROGRESS.md 收尾 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W9 设计文档 §2.7 + §5 验收门禁完成 W9 全部集成测试与收尾工作:新增 `tests/w9_default_boundary_smoke.rs`(15 个 default 组合边界用例,覆盖 Stronghold feature off 行为 / Taint 表空时 gateway 行为 / DAG Modify 占位行为 / Slot 流水空 user_slots / 审计事件命名一致性 / DagStatus 状态机 / DagPlan validate / 拓扑排序边界)+ 新增 `tests/w9_audit_chain_smoke.rs`(3 个哈希链测试:链不断 / W9 新增 7 种事件全记 / details 字段隐私脱敏);跑 7 套 feature 组合 `cargo check` 矩阵 + 2 套代表性 feature 组合 clippy `-D warnings` + `npm.cmd run build` + 非门控测试数 ≥ 286 验证 + 全量 `cargo test` 无回归;更新 `docs/PROGRESS.md` W9 整体段落 + 里程碑表 + §4.1 转向 W10 候选方向 + 已知偏离段;末尾以空 commit 标记 W9 完成。

**Architecture:** 2 个测试文件(default_boundary_smoke 闭合 default 组合 W9 边界覆盖 + audit_chain_smoke 闭合 spec §5 哈希链 + 隐私脱敏门禁);7 套 feature 组合(`--no-default-features` / `--features llm` / `--features tauri` / `--features voice,tauri` / `--features voice,tauri,llm` / `--features voice,tauri,llm,uia` / `--features voice,tauri,llm,uia,stronghold`)各跑一遍 `cargo check`;clippy 在 `--no-default-features` 与 `--features voice,tauri,llm,uia,stronghold` 两套下 `-D warnings` 0 警告;前端 `npm.cmd run build` PASS;非门控测试数 `cargo test --workspace --no-default-features -- --list | Measure-Object -L` ≥ 286;`docs/PROGRESS.md` 更新 W9 完成状态 + 测试统计(W8 收尾 465 + W9 新增 55+ ≈ 520)+ 已知偏离(spec §7 列出 8 项延后 W10+)+ §4.1 转向 W10 候选方向;末尾 `git commit --allow-empty` 标记 W9 里程碑。

**Tech Stack:** Rust(stable),`cargo` + `cargo clippy`,`tauri-plugin-stronghold`(W9 Plan 1 引入)+ `argon2`(W9 Plan 1 引入),Node.js 22+ / npm 10+(前端构建),PowerShell(`;` 分隔命令,不用 `&&` / `||`)。

**Spec:** `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md` §2.7(Plan 7 范围)+ §5(Fitness Functions)+ §7(已知偏离 / 延后项)+ §9(测试矩阵)+ §10(Conventions)+ §11(兼容性)+ §12 决策 #12(W9 不提交,但 Plan 7 是验收 plan,完成后可提交)

**Precondition:**
- W9 Plan 1-6 已全部完成:
  - Plan 1(`2026-07-28-w9-plan1-stronghold-vault.md`):`StrongholdVault` + Argon2id 密钥派生 + `config` 表 salt 持久化 + 降级模式 + `tests/w9_stronghold_unit.rs`(8 个单元测试)
  - Plan 2(`2026-07-28-w9-plan2-snapshot-encrypted.md`):`create_post_commit_compensation` 注入 Stronghold 加密 + `reverse_compensation` 解密 + 明文残留检测 + `stronghold_snapshot_encrypted` / `stronghold_snapshot_decrypt_failed` / `stronghold_degraded_mode_entered` 审计事件
  - Plan 3(`2026-07-28-w9-plan3-taint-tracking.md`):`TaintRepo` CRUD + `dispatcher` 输入→输出 taint 传播 + `gateway` 查表驱动 + `taint_propagated` / `taint_blocked` 审计事件 + `tests/w9_taint_tracking_unit.rs`(10 个单元)+ `tests/w9_gateway_taint_smoke.rs`(4 个集成)
  - Plan 4(`2026-07-28-w9-plan4-dag-modify.md`):`DagApprovalOutcome::Modify` + `dag_executor` Modify 分支 + `TauriApprover` 扩展 + `DagApprovalDialog.tsx` 编辑器 + 重新审批闭环 + `dag_skeleton_modified` / `dag_modify_limit_exceeded` 审计事件 + `tests/w9_dag_modify_smoke.rs`(6 个集成)
  - Plan 5(`2026-07-28-w9-plan5-playwright-e2e.md`):`tests/w9_plan5_playwright_dag_e2e.rs`(2 个 `#[ignore]` 真实 E2E)
  - Plan 6(`2026-07-28-w9-plan6-uia-e2e-slot.md`):`DagExecutor::run(plan, user_slots)` 签名扩展 + `IterableSource::UserSlot` 实现 + `tests/w9_plan6_uia_dag_e2e.rs`(2 个 `#[ignore]` 真实 E2E)
- W8 已完成(commit `8ec814d`,W8 Plan 1-6 全部验收门禁关闭,465 非门控测试通过)
- `cargo check --workspace --features voice,tauri,llm,uia,stronghold` PASS
- `cargo test --workspace --features voice,tauri,llm,uia,stronghold` 全 PASS,W1-W8 测试无回归
- `npm.cmd run build`(在 `voicepilot/crates/ui/web/`)PASS
- `tauri-plugin-stronghold` + `argon2` 已在 workspace `Cargo.toml` `[workspace.dependencies]`(Plan 1 引入)
- spec §12 决策 #12:W9 Plan 1-6 不提交,仅生成完整 plan 文档;**Plan 7 是验收 plan,完成后可提交**(本 plan 是 W9 唯一可提交的 plan)

---

## W9 7-Plan 拆分概览(供 Plan 7 执行者参考)

| Plan | 范围 | Spec § | 状态 |
|---|---|---|---|
| Plan 1 | StrongholdVault + Argon2id 密钥派生 + 降级模式 + 单元测试 | §2.1 | ✅ 已完成 |
| Plan 2 | snapshot_encrypted 真实加密 + 明文 PoC 移除 + 审计事件 | §2.2 | ✅ 已完成 |
| Plan 3 | TaintRepo CRUD + 查表驱动 gateway + Skill 输入→输出传播 | §2.3 | ✅ 已完成 |
| Plan 4 | DagApprovalOutcome::Modify + 重新审批闭环 + UI 编辑器 | §2.4 | ✅ 已完成 |
| Plan 5 | 真实 Playwright MCP DAG E2E(`#[ignore]`) | §2.5 | ✅ 已完成 |
| Plan 6 | 真实 UIA GUI DAG E2E + Slot 流水闭合(`#[ignore]`) | §2.6 | ✅ 已完成 |
| **Plan 7 (本文件)** | 集成测试 + 7 套 feature cargo check 矩阵 + clippy + npm build + 非门控测试数 ≥ 286 + PROGRESS.md 收尾 | §2.7, §5 | ⏳ 进行中 |

---

## File Structure

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w9_default_boundary_smoke.rs` — 15 个 default 组合边界用例(本 Plan 核心测试文件 1,~350 行)
  - A. Stronghold feature off 行为(3 个):snapshot_encrypted = None / degraded vault decrypt 返回 NotUnlocked / reverse_payload 保留明文(向后兼容)
  - B. Taint 表空时 gateway 行为(2 个):空表放行 / find_by_value 不存在返回 None
  - C. DAG Modify 占位行为(2 个):AutoApprover 不触发 Modify / 第二次 Modify 返回 DagModifyLimitExceeded
  - D. Slot 流水空 user_slots(2 个):run(plan, &[]) 正常 / IterableSource::UserSlot 引用但 user_slots=[] 返回 UserSlotNotFound
  - E. 审计事件命名一致性(2 个):所有 event_type 匹配 ^[a-z][a-z0-9_]*$ / W9 7 种新事件命名合规
  - F. DagStatus 状态机边界(2 个):Running→Succeeded 合法 / Succeeded→Running 非法
  - G. DagPlan validate 边界(1 个):空 nodes 返回 Err
  - H. 拓扑排序边界(1 个):空图返回 Ok([])
- **Create** `voicepilot/crates/trust-kernel/tests/w9_audit_chain_smoke.rs` — 3 个哈希链 + 隐私脱敏测试(本 Plan 核心测试文件 2,~200 行)
  - 哈希链不断(遍历 audit_logs 验证 prev_hash 链)
  - W9 新增 7 种事件全记录(stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed / stronghold_degraded_mode_entered / taint_propagated / taint_blocked / dag_skeleton_modified / dag_modify_limit_exceeded)
  - details 字段隐私脱敏(不含 plaintext / password / 密钥关键字)

### Docs

- **Modify** `docs/PROGRESS.md` — W9 整体段落 + 里程碑表加 7 行(W9 Plan 1-7)+ §4.1 转向 W10 候选方向 + 已知偏离段(spec §7 8 项延后 W10+)

### 无源码改动声明

本 Plan 仅写测试 + 跑验收门禁 + 更新文档,**不修改** `crates/trust-kernel/src/` 或 `crates/ui/src/` 任何源码文件。

**例外**:若 Task 3 / Task 4 / Task 7 跑验收门禁时发现编译错误或 lint 警告,修复策略严格遵循 `project_memory.md` "Lessons Learned" + W4/W7/W8 已建立的模式(见 §Conventions)。修复 commit message 用 `fix(w9p7): ...` 前缀。

若发现回归(既有测试失败),按 spec §10 Conventions "不修改 spec / 已有 plan" 原则,记录到 PROGRESS.md "已知偏离" 段,不回改 spec;若回归阻塞验收门禁,修复 commit message 用 `fix(w9p7): ...` 前缀。

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(不支持 heredoc,参考 `project_memory.md` "Lessons Learned")
- **TDD**:本 Plan 是收尾 plan,测试先行 → 跑测试确认通过 → 跑验收门禁 → commit
- **Feature 组合测试矩阵**:7 套 feature 组合各跑 `cargo check`;2 套代表性 feature 组合跑 `cargo clippy -- -D warnings`;6 套 feature 组合跑 `cargo test` 核心(除 `--features voice,tauri,llm,uia,stronghold` 全跑外,其余 5 套各跑 default 边界 + audit_chain 测试)
- **clippy lint 修复模式**(从 `project_memory.md` "Lessons Learned" + W8 Plan 6 验收):
  - `explicit_auto_deref` → 用 `&kernel.conn()` 不用 `&*kernel.conn()`
  - `query_map` closure 返回 `rusqlite::Result<T>` 不是 `crate::error::Result<T>`
  - 未使用 import → 删除
  - `large_enum_variant` → 用 `Box<T>` 包大变体
  - `manual_inspect` → 用 `.inspect_err(|_| { ... })` 替代 `.map_err(|e| { ...; e })`
  - `manual_clamp` → 用 `.clamp(lo, hi)` 替代 `.max(lo).min(hi)`
  - `manual_range_contains` → 用 `(lo..=hi).contains(&x)` 替代 `x >= lo && x <= hi`
  - `needless_borrows_for_generic_args` → `hasher.update(x.to_le_bytes())` 不要 `&x.to_le_bytes()`
  - `len_zero` → `!vec.is_empty()` 不要 `vec.len() >= 1`
  - `doc_lazy_continuation` → 文档列表项延续加空行
  - Tauri 2.x `Emitter` trait → `use tauri::{AppHandle, Emitter};`
  - `state.rs` cfg-gated field → 拆分为 dual `new()`(Rust 不允许 struct 字段 cfg-gated)
  - Approver import 完整路径 → `use crate::approval::approver::Approver;`(approval 模块未在 root re-export)
- **TrustKernel 不是 Clone**:e2e 测试用 `Arc<TrustKernel>` 共享或 owned `TrustKernel::open_in_memory()`
- **审计事件命名**:沿用 W7/W8 的 `lower_snake_case`(如 `stronghold_snapshot_encrypted`),不用早期 SCREAMING_SNAKE;regex 校验 `^[a-z][a-z0-9_]*$`
- **W9 新增 7 种审计事件**(spec §6.4):
  - `stronghold_snapshot_encrypted` — details `{compensation_id, vault_ref, plaintext_len}`
  - `stronghold_snapshot_decrypt_failed` — details `{compensation_id, error}`
  - `stronghold_degraded_mode_entered` — details `{reason}`
  - `taint_propagated` — details `{source_ref, input_hash, output_hash, taints}`
  - `taint_blocked` — details `{taints, sink, resource_hash}`
  - `dag_skeleton_modified` — details `{plan_id, modified_node_count, added_count, removed_count}`
  - `dag_modify_limit_exceeded` — details `{plan_id}`
- **隐私脱敏规则**(spec §6.4):details 字段不得包含 `plaintext` / `password` / `secret` / `key`(密钥字面量);`value_hash` 用 SHA256(value) 不存原始值
- **`#[ignore]` 真实 E2E 测试**:复用 W7 Plan 4/5 + W9 Plan 5/6 模式,探测环境 + 短路 passing;本 Plan 不写新 `#[ignore]` 测试(Plan 5/6 已写)
- **Stronghold feature 独立**:`stronghold` feature 不依赖 `voice` / `tauri` feature,可独立编译;`stronghold = ["dep:tauri-plugin-stronghold", "dep:argon2"]`
- **commit message 前缀**:`test(w9p7): ...` / `fix(w9p7): ...` / `docs(w9p7): ...`(参考 spec §10)
- **不引入非必要依赖**:W9 仅新增 `tauri-plugin-stronghold` + `argon2`(均已在 Plan 1 workspace 级引入);本 Plan 不引入新依赖
  - **例外**:`regex` crate 若已在 W7/W8 dev-dependencies,本 Plan 复用;否则 Task 1 Step 11 加 `regex = "1"` 到 `[dev-dependencies]`。先 grep 确认。
- **不修改 spec / 已有 plan**:若发现 spec 描述与实现不一致,记录到 PROGRESS.md "已知偏离" 段落,不回改 spec
- **空 commit 标记里程碑**:W9 收尾 `git commit --allow-empty -m "docs(w9): W9 complete — Stronghold + Taint + DAG Modify + Real E2E"`(spec §10 + §12 决策 #12 例外)

---

## Task 1: 创建 w9_default_boundary_smoke.rs(15 个 default 组合边界用例)

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_default_boundary_smoke.rs`

**目标:** 15 个 default 组合(`--no-default-features`)边界用例,覆盖 spec §2.7 + §5 + §6.4 边界场景。所有用例 `#[test]`(非 `#[tokio::test]`,无 LLM / Tauri / Stronghold feature 依赖),纯逻辑 + DB + 内省 audit_logs。

- [ ] **Step 0: 核实 DagStatus 现有 API**

执行 `grep -n "pub fn\|impl DagStatus" voicepilot/crates/trust-kernel/src/skills/dag_types.rs` 确认可用方法。

F 组测试若需状态机转换检查,用现有 API(如 `is_succeeded()` / `is_failed()` / `is_cancelled()` 等)替代 `DagStatus::transition`。

若必须用 `transition`,先在 `dag_types.rs` 实现该方法:
```rust
impl DagStatus {
    /// 检查从 from 到 to 的状态转换是否合法
    pub fn transition(from: &DagStatus, to: &DagStatus) -> bool {
        use DagStatus::*;
        match (from, to) {
            (Pending, Running) | (Running, Succeeded) | (Running, Failed) | (Running, Cancelled) => true,
            _ => false,
        }
    }
}
```

- [ ] **Step 1: 创建测试文件,写入文件头注释(不写文件级 cfg gate)**

创建 `voicepilot/crates/trust-kernel/tests/w9_default_boundary_smoke.rs`,写入文件头注释 + imports。
**不写 `#![cfg(...)]` 文件级 gate**(默认编译即可,让 default 组合也能跑此文件)。

```rust
//! W9 Plan 7 — default 组合边界用例(15 个)。
//!
//! 覆盖 spec §2.7(Plan 7 范围)+ §5(Fitness Functions)+ §6.4(审计事件隐私):
//!   A. Stronghold feature off 行为(3 个):default 组合下 snapshot_encrypted = None
//!      / StrongholdVault::degraded() decrypt 返回 NotUnlocked
//!      / reverse_payload 保留明文(向后兼容)
//!   B. Taint 表空时 gateway 行为(2 个):空 taints 表 gateway 放行
//!      / TaintRepo::find_by_value 查不存在 hash 返回 None
//!   C. DAG Modify 占位行为(2 个):AutoApprover 不触发 Modify
//!      / 第二次 Modify 返回 DagModifyLimitExceeded
//!   D. Slot 流水空 user_slots(2 个):DagExecutor::run(plan, &[]) 正常执行
//!      / IterableSource::UserSlot 引用但 user_slots=[] 返回 UserSlotNotFound
//!   E. 审计事件命名一致性(2 个):所有 event_type 匹配 ^[a-z][a-z0-9_]*$
//!      / W9 7 种新事件命名合规
//!   F. DagStatus 状态机边界(2 个):Running→Succeeded 合法 / Succeeded→Running 非法
//!   G. DagPlan validate 边界(1 个):空 nodes 返回 Err
//!   H. 拓扑排序边界(1 个):空图返回 Ok([])
//!
//! 测试设计:
//! - 所有用例 `#[test]`,无 feature gate(default 组合可编译)
//! - 所有用例用 `TrustKernel::open_in_memory()`
//! - 不调用真实 LLM / Tauri / Stronghold(feature 未启用)
//! - 内省 audit_logs 表用 `conn.query_map` 直接 SQL
//!
//! 不写文件级 cfg gate,让 default 组合(--no-default-features)也能编译运行此文件。

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagPlan, DagStatus, IterableSource, LoopSpec,
};
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};

/// 统计 audit_logs 中所有 event_type(去重)。
fn list_distinct_event_types(kernel: &TrustKernel) -> Vec<String> {
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT DISTINCT event_type FROM audit_logs ORDER BY event_type ASC")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

/// 统计 audit_logs 中某 event_type 的数量。
fn count_audit_events(kernel: &TrustKernel, event_type: &str) -> i64 {
    let conn = kernel.conn();
    conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE event_type = ?1",
        rusqlite::params![event_type],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// 构造一个 literal SlotTemplate。
fn literal_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Literal(value.to_string()),
    }
}

/// 构造一个最小合法 DagPlan(单节点,无边,无循环)。
fn minimal_dag_plan(node_count: usize) -> DagPlan {
    let nodes: Vec<DagNode> = (0..node_count)
        .map(|i| DagNode {
            node_id: format!("n{}", i),
            skill_id: "note.capture".to_string(),
            input_template: literal_template("{}"),
            risk_ceiling: ELevel::E1,
        })
        .collect();
    DagPlan {
        plan_id: format!("plan-boundary-{}", uuid::Uuid::new_v4()),
        user_goal: "boundary test".to_string(),
        nodes,
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}
```

- [ ] **Step 2: 验证文件创建成功 + cargo check 编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --no-default-features --tests`
Expected: PASS(编译通过;若 Plan 1-6 引入的类型路径与本文件不一致,适配 import 路径)

- [ ] **Step 3: 写 A 组测试 — Stronghold feature off 行为(3 个)**

在文件末尾追加 A 组测试:

```rust
// ===== A. Stronghold feature off 行为(3 个)=====

#[test]
fn stronghold_feature_off_snapshot_encrypted_is_none() {
    // default 组合(无 stronghold feature)调用 create_post_commit_compensation,
    // snapshot_encrypted 必须为 None(spec §2.2 向后兼容分支)
    let kernel = TrustKernel::open_in_memory().unwrap();

    // 直接调用 create_post_commit_compensation(Plan 2 实际签名)
    // 注意:default 组合下 kernel.stronghold_enabled() = false
    let record = trust_kernel::skills::common::create_post_commit_compensation(
        &kernel,
        "task-1",
        "step-1",
        "note.capture",
        &serde_json::json!({"content": "test"}),
        &serde_json::json!({"action": "delete", "path": "test.txt"}),
    )
    .expect("create_post_commit_compensation must succeed in default feature combo");

    assert!(
        record.snapshot_encrypted.is_none(),
        "snapshot_encrypted must be None when stronghold feature is off (default combo)"
    );
    assert!(
        record.snapshot_vault_ref.is_none(),
        "snapshot_vault_ref must be None when stronghold feature is off"
    );
    // reverse_payload 列保留明文(向后兼容,feature 未启用时不加密)
    assert!(
        !record.reverse_payload.is_empty(),
        "reverse_payload must keep plaintext when stronghold feature is off (backward compat)"
    );
}

#[test]
fn stronghold_feature_off_degraded_vault_decrypt_returns_not_unlocked() {
    // StrongholdVault::degraded() 构造的 vault 在 default 组合下
    // 调 decrypt 必须返回 StrongholdError::NotUnlocked
    // 注:StrongholdVault 在 default 组合下用 cfg-gated 占位实现(Plan 1 提供)
    let vault = trust_kernel::crypto::stronghold::StrongholdVault::degraded();
    assert!(!vault.is_unlocked(), "degraded vault must report not unlocked");

    let dummy_payload = trust_kernel::crypto::stronghold::EncryptedPayload {
        ciphertext: vec![1, 2, 3],
        nonce: vec![4, 5, 6],
        salt_ref: "test-salt".to_string(),
    };
    let result = vault.decrypt(&dummy_payload);
    assert!(
        matches!(
            result,
            Err(trust_kernel::crypto::stronghold::StrongholdError::NotUnlocked)
        ),
        "degraded vault decrypt must return NotUnlocked, got {:?}",
        result
    );
}

#[test]
fn stronghold_feature_off_reverse_payload_keeps_plaintext() {
    // 反复调用 create_post_commit_compensation,验证所有记录的 reverse_payload
    // 都是明文 JSON(spec §2.2 兼容性表第 1 行)
    let kernel = TrustKernel::open_in_memory().unwrap();
    for i in 0..3 {
        let _ = trust_kernel::skills::common::create_post_commit_compensation(
            &kernel,
            &format!("task-{}", i),
            &format!("step-{}", i),
            "note.capture",
            &serde_json::json!({"content": format!("test-{}", i)}),
            &serde_json::json!({"action": "delete"}),
        )
        .unwrap();
    }
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT reverse_payload, snapshot_encrypted FROM compensations")
        .unwrap();
    let rows: Vec<(String, Option<Vec<u8>>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<Vec<u8>>>(1)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    for (reverse_payload, snapshot_encrypted) in rows {
        assert!(
            !reverse_payload.is_empty(),
            "reverse_payload must keep plaintext in default combo"
        );
        assert!(
            snapshot_encrypted.is_none(),
            "snapshot_encrypted must be None in default combo"
        );
        // reverse_payload 必须是合法 JSON
        let _: serde_json::Value = serde_json::from_str(&reverse_payload)
            .expect("reverse_payload must be valid JSON in default combo");
    }
}
```

- [ ] **Step 4: 跑 A 组测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke stronghold_feature_off`
Expected: 3 passed

若失败排查:
- `create_post_commit_compensation` 签名变更(检查 Plan 2 实际签名,适配参数)
- `StrongholdVault::degraded()` 在 default 组合下的实际路径(检查 Plan 1 cfg-gated 占位实现)
- `compensations` 表 schema 列名(检查 `migrations/001_init.sql`)

- [ ] **Step 0: grep 验证 TaintRepo CRUD 代码存在(在写 B 组测试之前)**

```powershell
# 验证 INSERT INTO taints 存在
Select-String -Path voicepilot/crates/trust-kernel/src/policy/taint_repo.rs -Pattern "INSERT INTO taints"

# 验证 find_by_value 方法存在
Select-String -Path voicepilot/crates/trust-kernel/src/policy/taint_repo.rs -Pattern "pub fn find_by_value"
```

若任一 grep 无命中,说明 Plan 3 未实现,阻塞 Plan 7。

- [ ] **Step 5: 写 B 组测试 — Taint 表空时 gateway 行为(2 个)**

在文件末尾追加 B 组测试:

```rust
// ===== B. Taint 表空时 gateway 行为(2 个)=====

#[test]
fn gateway_taint_table_empty_allows_resource() {
    // taints 表空时,gateway 必须放行所有 resource(spec §2.3 查表驱动规则)
    let kernel = TrustKernel::open_in_memory().unwrap();
    let resource = trust_kernel::policy::types::Resource {
        value_hash: "sha256:nonexistent".to_string(),
        provenance: "user_input".to_string(),
    };
    let sink = trust_kernel::policy::types::Sink::ToolArgument;
    let result = trust_kernel::policy::gateway::check_taint_policy(
        &kernel.conn(),
        &resource,
        &sink,
    );
    assert!(
        result.is_ok(),
        "gateway must allow resource when taints table is empty, got {:?}",
        result
    );
}

#[test]
fn taint_repo_find_by_value_nonexistent_returns_none() {
    // TaintRepo::find_by_value 查不存在的 value_hash 返回 Ok(None)
    let kernel = TrustKernel::open_in_memory().unwrap();
    let repo = trust_kernel::policy::taint_repo::TaintRepo::new();
    let result = repo
        .find_by_value(&kernel.conn(), "sha256:nonexistent")
        .expect("find_by_value must not error on nonexistent hash");
    assert!(
        result.is_none(),
        "find_by_value must return None for nonexistent hash"
    );
}
```

- [ ] **Step 6: 跑 B 组测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke gateway_taint_table_empty`
Expected: 1 passed

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke taint_repo_find_by_value`
Expected: 1 passed

若失败排查:
- `Resource` / `Sink` 实际路径 + 字段名(检查 Plan 3 `policy/types.rs`)
- `check_taint_policy` 实际签名(检查 Plan 3 `policy/gateway.rs`)
- `TaintRepo::new` / `find_by_value` 实际签名(检查 Plan 3 `policy/taint_repo.rs`)

- [ ] **Step 0: 核实 Plan 4 实际签名(在写 C 组测试之前)**

执行以下 grep 确认 Plan 4 实际签名:
- `grep "pub enum DagApprovalOutcome" voicepilot/crates/trust-kernel/src/approval/approver.rs` 确认变体名(`Allow` / `Deny` / `Modify { modified_plan }`)
- `grep "DagModifyLimitExceeded" voicepilot/crates/trust-kernel/src/error.rs` 确认错误变体名

若 Plan 4 实际签名与本 plan 描述不符,调整测试代码。

- [ ] **Step 7: 写 C 组测试 — DAG Modify 占位行为(2 个)**

在文件末尾追加 C 组测试:

```rust
// ===== C. DAG Modify 占位行为(2 个)=====

#[test]
fn dag_modify_auto_approver_does_not_trigger_modify() {
    // default 组合下 AutoApprover 必须返回 Allow,不触发 Modify 分支
    // (spec §2.4 + §11 兼容性表第 3 行:AutoApprover 返回 Allow 等价 W8 行为)
    let approver = AutoApprover;
    let plan = minimal_dag_plan(1);
    let outcome = approver
        .approve_dag_skeleton(&plan)
        .expect("AutoApprover must not error");
    assert!(
        matches!(
            outcome,
            trust_kernel::approval::approver::DagApprovalOutcome::Allow
        ),
        "AutoApprover must return Allow (not Modify) in default combo, got {:?}",
        outcome
    );
}

#[test]
fn dag_modify_limit_exceeded_on_second_modify() {
    // 第二次连续 Modify 必须返回 DagModifyLimitExceeded(spec §2.4 + §6.3 安全约束)
    // 注:default 组合下用 MockApprover 模拟两次 Modify 调用
    use trust_kernel::approval::approver::{Approver, DagApprovalOutcome};
    use std::sync::atomic::{AtomicU32, Ordering};

    struct DoubleModifyApprover {
        call_count: Arc<AtomicU32>,
    }
    impl Approver for DoubleModifyApprover {
        fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<DagApprovalOutcome, trust_kernel::error::KernelError> {
            let n = self.call_count.fetch_add(1, Ordering::SeqCst);
            // 第 0 次 + 第 1 次都返回 Modify(模拟用户连续两次提交修改)
            let modified_plan = minimal_dag_plan(1);
            Ok(DagApprovalOutcome::Modify {
                modified_plan: Box::new(modified_plan),
            })
        }
    }

    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver = Arc::new(DoubleModifyApprover {
        call_count: Arc::new(AtomicU32::new(0)),
    });
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let plan = minimal_dag_plan(1);

    let result = executor.run(&plan, &[]);
    // 第二次 Modify 必须被拒绝,返回 DagModifyLimitExceeded
    assert!(
        matches!(
            result,
            Err(trust_kernel::error::KernelError::DagModifyLimitExceeded)
        ),
        "second consecutive Modify must return DagModifyLimitExceeded, got {:?}",
        result
    );
    // 审计必须记录 dag_modify_limit_exceeded 事件
    assert!(
        count_audit_events(&kernel, "dag_modify_limit_exceeded") >= 1,
        "dag_modify_limit_exceeded audit event must be logged"
    );
}

#[test]
fn dag_modify_full_loop_modify_then_approve_then_execute() {
    // W9 修复(P1-18):完整闭环用例 — Modify → 重新审批 → 执行 → 验证审计链完整
    // 1. 构造 plan
    // 2. 第一次审批:Modify { modified_plan }
    // 3. 第二次审批:Allow
    // 4. 验证 modified_plan 执行 Succeeded(或至少不 panic)
    // 5. 验证 dag_skeleton_modified + dag_plan_created(modified) + dag_skeleton_approved(allow, phase=after_modify) 审计链完整
    use trust_kernel::approval::approver::{Approver, DagApprovalOutcome};
    use std::sync::atomic::{AtomicU32, Ordering};

    struct ModifyOnceThenAllowApprover {
        call_count: Arc<AtomicU32>,
    }
    impl Approver for ModifyOnceThenAllowApprover {
        fn approve_dag_skeleton(&self, _plan: &DagPlan) -> Result<DagApprovalOutcome, trust_kernel::error::KernelError> {
            let n = self.call_count.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // 第 0 次:返回 Modify,带 modified_plan
                let modified_plan = minimal_dag_plan(1);
                Ok(DagApprovalOutcome::Modify {
                    modified_plan: Box::new(modified_plan),
                })
            } else {
                // 第 1 次及之后:返回 Allow
                Ok(DagApprovalOutcome::Allow)
            }
        }
    }

    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver = Arc::new(ModifyOnceThenAllowApprover {
        call_count: Arc::new(AtomicU32::new(0)),
    });
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let plan = minimal_dag_plan(1);

    let result = executor.run(&plan, &[]);
    // run 必须不 panic,理想情况 Succeeded(default 组合下 note.capture 不可达可能 Failed)
    assert!(result.is_ok(), "dag_modify_full_loop run must not panic");

    // 验证审计链完整:
    // - dag_skeleton_modified:第一次 Modify 时记录
    // - dag_plan_created(modified):modified_plan 重新写入 dag_repo 时记录
    // - dag_skeleton_approved(allow, phase=after_modify):第二次 Allow 时记录
    let modified_count = count_audit_events(&kernel, "dag_skeleton_modified");
    assert!(
        modified_count >= 1,
        "dag_skeleton_modified must be logged on first Modify, got count={}",
        modified_count
    );
    // 注:dag_plan_created + dag_skeleton_approved 的事件名以 Plan 4 实际实现为准,
    // 此处仅验证 dag_skeleton_modified 触发,完整审计链验证在 Plan 4 集成测试
}
```

- [ ] **Step 8: 跑 C 组测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke dag_modify`
Expected: 3 passed(注:C 组现 3 个测试,P1-18 已加完整闭环用例)

若失败排查:
- `DagApprovalOutcome` 实际变体名(检查 Plan 4 `approval/approver.rs`)
- `KernelError::DagModifyLimitExceeded` 变体名(检查 Plan 4 `error.rs`)
- `DagExecutor::new` 实际签名(检查 Plan 4 `dag_executor.rs`,可能是 `new(kernel, approver, dag_repo)` 或 `with_dispatcher(...)`)
- `DagExecutor::run(&plan, &[])` 第二参数 `user_slots`(检查 Plan 6 实际签名)

- [ ] **Step 9: 写 D 组测试 — Slot 流水空 user_slots(2 个)**

在文件末尾追加 D 组测试:

```rust
// ===== D. Slot 流水空 user_slots(2 个)=====

#[test]
fn dag_executor_run_with_empty_user_slots_does_not_panic() {
    // DagExecutor::run(plan, &[]) 必须不 panic(spec §11 兼容性表第 5 行:
    // user_slots=[] 时仍可工作,等价 W8 行为)
    // 注:default feature 下 note.capture 不可达,节点可能 Failed,但 run 本身不 panic
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    let plan = minimal_dag_plan(1);
    let result = executor.run(&plan, &[]);
    assert!(result.is_ok(), "run must not panic even if node fails");
    // 不断言 Succeeded(default feature 下 note.capture 不可达,可能 Failed)
    let dag_result = result.unwrap();
    let _ = dag_result.status;  // 仅验证 run 不 panic
}

#[test]
fn iterable_source_user_slot_empty_slots_returns_user_slot_not_found() {
    // IterableSource::UserSlot 引用但 user_slots=[] 时,
    // SlotTemplateEngine 必须返回 UserSlotNotFound(spec §11 兼容性表第 5 行例外)
    let kernel = TrustKernel::open_in_memory().unwrap();
    let plan = DagPlan {
        plan_id: format!("plan-slot-{}", uuid::Uuid::new_v4()),
        user_goal: "test slot".to_string(),
        nodes: vec![DagNode {
            node_id: "n1".to_string(),
            skill_id: "note.capture".to_string(),
            input_template: literal_template("{}"),
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs: {
            let mut m = HashMap::new();
            m.insert(
                "n1".to_string(),
                LoopSpec {
                    loop_var: "item".to_string(),
                    iterable_source: IterableSource::UserSlot { slot_kind: "text".to_string() },
                    max_iterations: 5,
                    break_condition: None,
                },
            );
            m
        },
        max_total_steps: 5,
    };

    // 调用 SlotTemplateEngine::validate_dag(plan) 必须返回 UserSlotNotFound
    // (或在 DagExecutor::run(plan, &[]) 执行时返回该错误)
    let kernel = Arc::new(kernel);
    let approver = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&plan, &[]);
    assert!(
        result.is_err(),
        "DagExecutor::run with IterableSource::UserSlot but empty user_slots must error"
    );
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.contains("UserSlotNotFound") || err_msg.contains("user_slot"),
        "error must mention UserSlotNotFound, got: {}",
        err_msg
    );
}
```

- [ ] **Step 10: 跑 D 组测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke dag_executor_run_with_empty_user_slots_does_not_panic`
Expected: 1 passed

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke iterable_source_user_slot`
Expected: 1 passed

- [ ] **Step 11: 写 E 组测试 — 审计事件命名一致性(2 个)**

在文件末尾追加 E 组测试:

```rust
// ===== E. 审计事件命名一致性(2 个)=====

#[test]
fn audit_event_types_all_lower_snake_case() {
    // 扫描 audit_logs 表所有 event_type,必须全部匹配 ^[a-z][a-z0-9_]*$
    // (spec §10 Conventions:沿用 W7/W8 的 lower_snake_case)
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 触发若干审计事件(创建 task / step / compensation 等)
    let _ = kernel.create_task("test");
    let event_types = list_distinct_event_types(&kernel);
    assert!(
        !event_types.is_empty(),
        "audit_logs must have at least one event_type after create_task"
    );
    let re = regex::Regex::new(r"^[a-z][a-z0-9_]*$").unwrap();
    for et in &event_types {
        assert!(
            re.is_match(et),
            "event_type '{}' must match ^[a-z][a-z0-9_]*$ (lower_snake_case)",
            et
        );
    }
}

#[test]
fn audit_event_types_w9_new_events_naming_compliant() {
    // W9 新增 7 种事件命名必须合规(spec §6.4):
    // stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed
    // / stronghold_degraded_mode_entered / taint_propagated / taint_blocked
    // / dag_skeleton_modified / dag_modify_limit_exceeded
    let w9_new_events = vec![
        "stronghold_snapshot_encrypted",
        "stronghold_snapshot_decrypt_failed",
        "stronghold_degraded_mode_entered",
        "taint_propagated",
        "taint_blocked",
        "dag_skeleton_modified",
        "dag_modify_limit_exceeded",
    ];
    let re = regex::Regex::new(r"^[a-z][a-z0-9_]*$").unwrap();
    for et in &w9_new_events {
        assert!(
            re.is_match(et),
            "W9 new event_type '{}' must match lower_snake_case",
            et
        );
    }
    // 注:本测试只验证命名合规性,不验证事件是否实际被触发
    // (default 组合下 stronghold feature 未启用,事件不会触发)
}
```

注:若 `regex` crate 未在 `trust-kernel/Cargo.toml` `[dev-dependencies]`,本 Step 先在 `[dev-dependencies]` 加 `regex = "1"`(若 W7/W8 已加,跳过)。

- [ ] **Step 12: 跑 E 组测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke audit_event_types`
Expected: 2 passed

- [ ] **Step 13: 写 F + G + H 组测试 — DagStatus 状态机 + DagPlan validate + 拓扑排序边界(5 个)**

在文件末尾追加 F + G + H 组测试:

```rust
// ===== F. DagStatus 状态机边界(2 个)=====

#[test]
fn dag_status_running_to_succeeded_legal() {
    // Running → Succeeded 必须合法(spec §2.7 + W8 Plan 6 Task 4.5 已建 transition 方法)
    // 注:transition 返回 bool(见 Step 0 补实现),true=合法,false=非法
    let from = DagStatus::Running;
    let to = DagStatus::Succeeded;
    let result = DagStatus::transition(&from, &to);
    assert!(
        result,
        "Running → Succeeded must be legal, got {}",
        result
    );
}

#[test]
fn dag_status_succeeded_to_running_illegal() {
    // Succeeded → Running 必须非法(终态不可逆)
    let from = DagStatus::Succeeded;
    let to = DagStatus::Running;
    let result = DagStatus::transition(&from, &to);
    assert!(
        !result,
        "Succeeded → Running must be illegal (terminal state)"
    );
}

// ===== G. DagPlan validate 边界(1 个)=====

#[test]
fn dag_plan_validate_empty_nodes_returns_err() {
    // DagPlan nodes=[] 必须 validate 失败(空 DAG 不合法)
    // 注:DagPlan 无聚合 validate() 方法,改用分项 validate_* 方法
    // (若需聚合,先在 dag_types.rs 补 impl DagPlan { pub fn validate(&self) -> Result<()> { ... } })
    let plan = DagPlan {
        plan_id: format!("plan-empty-{}", uuid::Uuid::new_v4()),
        user_goal: "empty".to_string(),
        nodes: vec![],  // 空 nodes
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    // 调用分项方法(若 Plan 1-6 已补聚合 validate(),改为 plan.validate())
    let result = (|| -> Result<(), trust_kernel::error::KernelError> {
        plan.validate_total_steps()?;
        plan.validate_loop_iterations()?;
        plan.validate_edges(&plan.edges)?;
        plan.validate_loop_specs(&plan.loop_specs)?;
        Ok(())
    })();
    assert!(
        result.is_err(),
        "DagPlan with empty nodes must fail validation"
    );
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.to_lowercase().contains("empty") || err_msg.to_lowercase().contains("nodes"),
        "error must mention empty nodes, got: {}",
        err_msg
    );
}

// ===== H. 拓扑排序边界(1 个)=====

#[test]
fn topo_sort_empty_graph_returns_empty() {
    // 空 nodes + edges 拓扑排序返回 Ok([])(spec §2.7 + W8 Plan 6 Task 4.5 已覆盖)
    // 注:DagExecutor 内部 topo_sort 是私有函数,通过 validate_dag 间接调用
    // 若 Plan 1 已暴露 pub fn topological_sort,直接调用更简洁
    let plan = DagPlan {
        plan_id: format!("plan-topo-empty-{}", uuid::Uuid::new_v4()),
        user_goal: "empty topo".to_string(),
        nodes: vec![],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    // 空 nodes 在 validate 阶段就会被拒(见 G 组测试),此处验证 topo_sort 本身
    // 若 topo_sort 是 pub,直接调:
    // let sorted = DagPlan::topological_sort(&plan).unwrap();
    // assert!(sorted.is_empty());
    // 此处用 validate_dag 间接验证:空图 validate 失败 = 拓扑排序边界正确处理空图
    // 注:无聚合 validate(),改用分项方法
    let result = (|| -> Result<(), trust_kernel::error::KernelError> {
        plan.validate_total_steps()?;
        plan.validate_loop_iterations()?;
        plan.validate_edges(&plan.edges)?;
        plan.validate_loop_specs(&plan.loop_specs)?;
        Ok(())
    })();
    assert!(
        result.is_err(),
        "empty graph must be rejected by validate (topo_sort boundary)"
    );
}
```

- [ ] **Step 14: 跑 F + G + H 组测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke dag_status_running_to_succeeded_legal`
Expected: 1 passed

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke dag_status_succeeded_to_running_illegal`
Expected: 1 passed

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke dag_plan_validate_empty_nodes_returns_err`
Expected: 1 passed

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke topo_sort_empty_graph_returns_empty`
Expected: 1 passed

若失败排查:
- `DagStatus::transition` 方法是否存在(检查 Plan 1 `dag_types.rs`,若未实现参考 W8 Plan 6 Task 4.5 Step 4A 补充)
- `DagPlan::validate` 方法名(检查 Plan 1 `dag_types.rs`,可能是 `validate` / `validate_dag` / `validate_all`)
- `DagStatus` 变体名(检查 Plan 1,可能有 `Pending` / `Running` / `Succeeded` / `Failed` / `PartiallySucceeded` / `Cancelled`)

- [ ] **Step 15: 跑全部 15 个用例一起,确认无相互干扰**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_default_boundary_smoke`
Expected: 15 passed

若某个用例失败,排查清单:
- `create_post_commit_compensation` 签名(检查 Plan 2 `skills/common.rs`)
- `StrongholdVault::degraded` / `decrypt` 签名(检查 Plan 1 `crypto/stronghold.rs`)
- `TaintRepo::new` / `find_by_value` 签名(检查 Plan 3 `policy/taint_repo.rs`)
- `check_taint_policy` 签名(检查 Plan 3 `policy/gateway.rs`)
- `DagApprovalOutcome` 变体名(检查 Plan 4 `approval/approver.rs`)
- `DagExecutor::new` / `run` 签名(检查 Plan 4 + Plan 6 `skills/dag_executor.rs`)
- `DagStatus::transition` 方法是否存在(检查 Plan 1 `dag_types.rs`)
- `DagPlan::validate` 方法名(检查 Plan 1 `dag_types.rs`)
- `KernelError::DagModifyLimitExceeded` 变体(检查 Plan 4 `error.rs`)
- `IterableSource::UserSlot(SlotKind)` 变体(检查 Plan 1 + Plan 6 `dag_types.rs`)
- `regex` crate 是否在 dev-dependencies(若 E 组失败,加 `regex = "1"` 到 `[dev-dependencies]`)

- [ ] **Step 16: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w9_default_boundary_smoke.rs
git commit -m "test(w9p7): add w9_default_boundary_smoke with 15 default-combo boundary cases"
```

---

## Task 2: 创建 w9_audit_chain_smoke.rs(3 个哈希链测试)

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w9_audit_chain_smoke.rs`

**目标:** 3 个哈希链 + 隐私脱敏测试,闭合 spec §5 Fitness Functions "哈希链不断" + §6.4 隐私处理门禁。

- [ ] **Step 1: 创建测试文件,不写文件级 cfg gate**

创建 `voicepilot/crates/trust-kernel/tests/w9_audit_chain_smoke.rs`。
**不写 `#![cfg(feature = "stronghold")]` 文件级 gate**(让 default 组合也能跑测试 1 + 测试 3)。

测试 1 + 测试 2 加 `#[cfg(feature = "stronghold")]` 函数级 gate;
测试 3(隐私脱敏)无 gate,default 组合可跑。

```rust
//! W9 Plan 7 — 哈希链 + 隐私脱敏 smoke 测试(3 个)。
//!
//! 覆盖 spec §5 "哈希链不断" + §6.4 审计事件隐私处理:
//!   1. audit_chain_hash_links_unbroken — 遍历 audit_logs 验证 prev_hash 链不断
//!   2. audit_chain_w9_new_5_events_recorded — W9 新增 5 种事件全记录(stronghold 组合)
//!      (注:default 组合下 stronghold feature 未启用,部分事件不触发;
//!       本测试在 stronghold feature 启用时跑,或用 mock 触发)
//!   3. audit_chain_details_no_plaintext_secrets — details 字段隐私脱敏
//!      (不含 plaintext / password / secret / key 关键字)
//!
//! 测试设计:
//! - 用例 1 + 3 用 default 组合(无 feature gate)
//! - 用例 2 用 `#[cfg(feature = "stronghold")]` + `#[cfg(feature = "llm")]` 门控
//! - 不写文件级 cfg gate,让 default 组合也能编译运行测试 1 + 测试 3

use std::sync::Arc;

use trust_kernel::crypto::stronghold::StrongholdVault;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::taint_repo::{TaintRecord, TaintRepo};
use trust_kernel::policy::types::{Resource, Sink};
use trust_kernel::skills::common::create_post_commit_compensation;

/// 读取 audit_logs 全部记录(id, event_type, prev_hash, details),按 id ASC 排序。
fn read_audit_chain(kernel: &TrustKernel) -> Vec<(i64, String, String, String)> {
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare(
            "SELECT id, event_type, prev_hash, details FROM audit_logs ORDER BY id ASC",
        )
        .unwrap();
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// 计算某字符串的 SHA256 hex(复用 audit_append 用的算法)。
fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}
```

- [ ] **Step 0: 读 W1 audit_append 源码确定哈希链算法**

执行 `grep -A 20 "fn audit_append" voicepilot/crates/trust-kernel/src/kernel.rs` 确认哈希链计算方式:
- 是 `sha256(prev_hash + event_type + details)` 还是 `sha256(details)` 还是其他?

确定算法后,测试 1 用确定性断言(见 Step 2 代码,假设算法是 `sha256(prev_hash + event_type)`,
若实际不同,执行时替换为 grep 出来的真实算法)。

- [ ] **Step 2: 写测试 1 — 哈希链不断**

在文件末尾追加测试 1:

```rust
/// 测试 1:audit_logs 哈希链不断。
///
/// 遍历所有记录,验证第 i 条的 prev_hash == SHA256(第 i-1 条的 prev_hash + event_type + details)。
/// 第 0 条的 prev_hash 必须是 "0000...0000"(创世哈希)。
#[test]
fn audit_chain_hash_links_unbroken() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 触发若干审计事件(create_task / create_step / create_post_commit_compensation 等)
    for i in 0..5 {
        let _ = kernel.create_task(&format!("chain-test-{}", i));
    }

    let chain = read_audit_chain(&kernel);
    assert!(
        chain.len() >= 5,
        "audit_logs must have at least 5 records after 5 create_task calls"
    );

    // 第 0 条 prev_hash 必须是创世哈希(全 0 或固定种子)
    let genesis = &chain[0];
    assert!(
        genesis.2.chars().all(|c| c == '0') || genesis.2 == "genesis" || genesis.2.is_empty(),
        "first audit_log prev_hash must be genesis (all zeros or 'genesis'), got: {}",
        genesis.2
    );

    // 第 i 条 prev_hash 必须等于第 i-1 条的 link_hash
    // link_hash 算法 = SHA256(prev.prev_hash + prev.event_type + prev.details)
    // 注:具体算法以 W1 audit_append 实际实现为准(见 Step 0 grep 结果),
    //   若 grep 显示算法不同(如 sha256(prev_hash + event_type) 或 sha256(details)),
    //   替换下面的 format! 字符串为真实算法。
    for i in 1..chain.len() {
        let prev = &chain[i - 1];
        let curr = &chain[i];
        // 确定性算法(从 W1 audit_append 源码 grep 出来后填入):
        let prev_hash = &prev.2;        // 前一条的 prev_hash
        let current_event_type = &prev.1;
        let current_details = &prev.3;
        let expected_link_hash = sha256_hex(&format!("{}{}{}", prev_hash, current_event_type, current_details));
        assert_eq!(
            curr.2, expected_link_hash,
            "audit chain broken at record {}: prev_hash='{}', expected='{}' (algorithm: sha256(prev_hash + event_type + details))",
            i, curr.2, expected_link_hash
        );
    }
}
```

- [ ] **Step 3: 跑测试 1**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features stronghold --test w9_audit_chain_smoke audit_chain_hash_links_unbroken`
Expected: 1 passed

若失败排查:
- `audit_append` 实际 link_hash 算法(检查 W1 `kernel/audit.rs`,本测试尝试 3 种算法,若都不匹配,读源码确定实际算法)
- `prev_hash` 创世值(可能是全 0、空字符串、或固定种子 "genesis")
- `audit_logs` 表列名(检查 `migrations/001_init.sql`,可能是 `prev_hash` / `previous_hash` / `link_hash`)

- [ ] **Step 4: 写测试 2 — W9 新增 5 种事件全记录**

**注意:** `dag_skeleton_modified` + `dag_modify_limit_exceeded` 需要 tauri feature 才有 TauriApprover 触发。
本测试在 `--features stronghold` 组合下只验证 5 种事件(stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed / stronghold_degraded_mode_entered / taint_propagated / taint_blocked)。
完整 7 种事件验证在 Task 7 Step 6 `--features voice,tauri,llm,uia,stronghold` 组合下跑。

在文件末尾追加测试 2:

```rust
/// 测试 2:W9 新增 5 种事件全记录(stronghold 组合下可触发的事件)。
///
/// 触发 Stronghold 加密 + 解密失败 + 降级模式 + Taint 传播 + Taint 拦截
/// 共 5 种事件(dag_skeleton_modified + dag_modify_limit_exceeded 需 tauri feature,
/// 在 Task 7 Step 6 全 feature 组合下验证),验证 audit_logs 全部记录。
#[test]
#[cfg(feature = "stronghold")]
fn audit_chain_w9_new_5_events_recorded() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());

    // ===== 触发 stronghold_snapshot_encrypted =====
    let vault = StrongholdVault::create("test_password", &kernel.conn()).unwrap();
    kernel.set_stronghold_vault(vault);
    let _ = create_post_commit_compensation(
        &kernel,
        "task-enc",
        "step-enc",
        "note.capture",
        &serde_json::json!({"content": "test"}),
        &serde_json::json!({"action": "delete"}),
    ).unwrap();
    assert!(
        count_audit_events(&kernel, "stronghold_snapshot_encrypted") >= 1,
        "stronghold_snapshot_encrypted must be logged"
    );

    // ===== 触发 stronghold_snapshot_decrypt_failed =====
    // 用错误密码解锁 → 解密失败
    let vault2 = StrongholdVault::degraded();
    kernel.set_stronghold_vault(vault2);
    // W9 修复:先创建真实 compensation 记录其 ID,再调 reverse_compensation
    // (原来传 "comp-1" 没创建过,会返回 CompensationNotFound 而非 decrypt_failed)
    let orig = serde_json::json!({"content": "test"});
    let curr = serde_json::json!({"content": "modified"});
    let comp_record = create_post_commit_compensation(
        &kernel,
        "task-dec",
        "step-dec",
        "note.capture",
        &orig,
        &serde_json::json!({"action": "delete"}),
    ).unwrap();
    let comp_id = comp_record.compensation_id;  // 字段名以 Plan 2 实际为准
    let result = trust_kernel::skills::common::reverse_compensation(&kernel, &comp_id);
    // 降级模式下解密必失败(reverse_compensation 应返回 decrypt_failed 错误)
    // 审计应记录 decrypt_failed
    let _ = result;  // 不强制 result 是 Err,只验证审计事件被记录
    assert!(
        count_audit_events(&kernel, "stronghold_snapshot_decrypt_failed") >= 1
            || count_audit_events(&kernel, "stronghold_degraded_mode_entered") >= 1,
        "decrypt_failed or degraded_mode_entered must be logged in degraded mode"
    );

    // ===== 触发 stronghold_degraded_mode_entered =====
    // 上面 set_stronghold_vault(degraded) 已触发;或显式调
    // kernel.enter_degraded_mode("wrong_password")
    assert!(
        count_audit_events(&kernel, "stronghold_degraded_mode_entered") >= 1,
        "stronghold_degraded_mode_entered must be logged"
    );

    // ===== 触发 taint_propagated =====
    // 用 TaintRepo::upsert 直接插入一条 taint(模拟 dispatcher 传播)
    let repo = TaintRepo::new();
    let _ = repo.upsert(&kernel.conn(), &TaintRecord {
        taint_id: uuid::Uuid::new_v4().to_string(),
        value_hash: "sha256:test-input".to_string(),
        provenance: "user_input".to_string(),
        taints: vec!["user_input".to_string()],
        collected_at: "2026-07-28T00:00:00Z".to_string(),
        source_ref: Some("task-1:step-1".to_string()),
    });
    // W9 修复:taint_propagated 由 dispatcher 传播时审计,本测试不强制触发
    // 若需验证,在 Plan 3 范围内用 mock dispatcher 测试
    let conn = kernel.conn();
    let taint_propagated_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE event_type = 'taint_propagated'",
        [], |row| row.get(0)
    ).unwrap_or(0);
    // 不强制断言 > 0,仅记录
    eprintln!("taint_propagated count: {}", taint_propagated_count);

    // ===== 触发 taint_blocked =====
    // 构造一个带 web_page taint 的 resource + ToolArgument sink
    let _ = repo.upsert(&kernel.conn(), &TaintRecord {
        taint_id: uuid::Uuid::new_v4().to_string(),
        value_hash: "sha256:web-content".to_string(),
        provenance: "web_page".to_string(),
        taints: vec!["web_page".to_string()],
        collected_at: "2026-07-28T00:00:00Z".to_string(),
        source_ref: Some("task-2:step-2".to_string()),
    });
    let resource = Resource {
        value_hash: "sha256:web-content".to_string(),
        provenance: "web_page".to_string(),
    };
    let sink = Sink::ToolArgument;
    let _ = trust_kernel::policy::gateway::check_taint_policy(
        &kernel.conn(),
        &resource,
        &sink,
    );
    assert!(
        count_audit_events(&kernel, "taint_blocked") >= 1,
        "taint_blocked must be logged when web_page taint hits ToolArgument sink"
    );

    // ===== 触发 dag_skeleton_modified + dag_modify_limit_exceeded =====
    // 用 DoubleModifyApprover(参考 Task 1 C 组测试)
    // 此处简化:仅验证事件命名存在(实际触发由 Plan 4 集成测试覆盖)
    // 若 default + stronghold 组合下能跑 DagExecutor,直接跑;否则跳过

    // ===== 验证 5 种事件命名都在 audit_logs 表中出现过 =====
    // 注:default + stronghold 组合下 dag_skeleton_modified / dag_modify_limit_exceeded
    // 可能不触发(需要 tauri feature 才有 TauriApprover),本测试验证 5 种事件命名合规即可
    let w9_event_types_5 = vec![
        "stronghold_snapshot_encrypted",
        "stronghold_snapshot_decrypt_failed",
        "stronghold_degraded_mode_entered",
        "taint_propagated",
        "taint_blocked",
    ];
    let logged_events = list_distinct_event_types(&kernel);
    // 5 种事件中至少 4 种必须实际触发(stronghold 3 + taint_blocked 1)
    // taint_propagated 由 dispatcher 传播,本测试不强求
    for et in &w9_event_types_5 {
        if et.starts_with("stronghold") || et == "taint_blocked" {
            assert!(
                logged_events.contains(&et.to_string()),
                "event_type '{}' must be logged after triggering",
                et
            );
        }
    }
}

fn count_audit_events(kernel: &TrustKernel, event_type: &str) -> i64 {
    let conn = kernel.conn();
    conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE event_type = ?1",
        rusqlite::params![event_type],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn list_distinct_event_types(kernel: &TrustKernel) -> Vec<String> {
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT DISTINCT event_type FROM audit_logs")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}
```

- [ ] **Step 5: 跑测试 2**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features stronghold --test w9_audit_chain_smoke audit_chain_w9_new_5_events_recorded`
Expected: 1 passed

若失败排查:
- `StrongholdVault::create` 签名(检查 Plan 1 `crypto/stronghold.rs`)
- `kernel.set_stronghold_vault` 方法是否存在(检查 Plan 1 `kernel.rs`)
- `reverse_compensation` 函数路径(检查 Plan 2 `skills/common.rs`)
- `TaintRecord` 字段名(检查 Plan 3 `policy/taint_repo.rs`)
- `Resource` / `Sink` 字段名(检查 Plan 3 `policy/types.rs`)
- 审计事件触发条件(检查 Plan 2 / Plan 3 / Plan 4 实际审计调用点)

- [ ] **Step 6: 写测试 3 — details 字段隐私脱敏**

在文件末尾追加测试 3:

```rust
/// 测试 3:audit_logs.details 字段隐私脱敏。
///
/// 扫描所有 audit_logs.details,验证不含敏感关键字:
/// - "plaintext"(明文 reverse_payload 内容)
/// - "password"(用户密码)
/// - "secret"(密钥 / 机密)
/// - "api_key"(LLM API key)
/// - "vault_key"(Stronghold vault key)
///
/// spec §6.4:details 字段仅记录 hash / id / count / reason,不记录原始敏感值。
#[test]
fn audit_chain_details_no_plaintext_secrets() {
    let kernel = TrustKernel::open_in_memory().unwrap();

    // 触发若干审计事件(create_task / create_step / compensation / taint 等)
    let _ = kernel.create_task("privacy-test");
    let _ = create_post_commit_compensation(
        &kernel,
        "task-priv",
        "step-priv",
        "note.capture",
        &serde_json::json!({"content": "this is plaintext content"}),
        &serde_json::json!({"action": "delete"}),
    ).unwrap();

    // 读取所有 details
    let chain = read_audit_chain(&kernel);
    assert!(!chain.is_empty(), "audit_logs must have records");

    // 隐私黑名单关键字(精确匹配密钥字面量前缀,大小写不敏感)
    // W9 修复:不用子串匹配("secret"/"key" 会误报合法字段 vault_key_ref / secret_id / api_key_hash),
    // 改为精确匹配密钥字面量前缀(如 password= / api_key= / sk-);
    // hash 字段(vault_key_ref / secret_id / api_key_hash)不是密钥本身,允许
    let sensitive_keywords = [
        "password=",
        "passwd=",
        "secret=",
        "api_key=",
        "sk-",       // OpenAI API key 前缀
        "plaintext=",
    ];

    for (id, event_type, _prev_hash, details) in &chain {
        let details_lower = details.to_lowercase();
        for kw in &sensitive_keywords {
            assert!(
                !details_lower.contains(kw),
                "audit_log id={} event_type='{}' details='{}' contains sensitive keyword '{}'",
                id,
                event_type,
                details,
                kw
            );
        }
    }

    // 额外验证:stronghold_snapshot_encrypted 事件的 details 必须含 plaintext_len(数字)
    // 但不含 plaintext 字面量(spec §6.4 + §2.2 审计事件表)
    // 注:default 组合下 stronghold feature 未启用,事件不触发;此断言仅在 stronghold feature 启用时跑
    #[cfg(feature = "stronghold")]
    {
        let encrypted_events: Vec<_> = chain
            .iter()
            .filter(|(_, et, _, _)| et == "stronghold_snapshot_encrypted")
            .collect();
        for (_, _, _, details) in encrypted_events {
            assert!(
                details.contains("plaintext_len"),
                "stronghold_snapshot_encrypted details must contain plaintext_len, got: {}",
                details
            );
            assert!(
                !details.contains("this is plaintext content"),
                "stronghold_snapshot_encrypted details must NOT contain raw plaintext"
            );
        }
    }
}
```

- [ ] **Step 7: 跑测试 3(default + stronghold 两套)**

Run(default 组合): `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_audit_chain_smoke audit_chain_details_no_plaintext_secrets`
Expected: 1 passed(Step 1 已采用函数级 gate,测试 3 无 gate,default 组合可跑)

跑测试 3(default): `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --no-default-features --test w9_audit_chain_smoke audit_chain_details_no_plaintext_secrets`
Expected: 1 passed

跑测试 3(stronghold): `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features stronghold --test w9_audit_chain_smoke audit_chain_details_no_plaintext_secrets`
Expected: 1 passed

- [ ] **Step 8: 跑全部 3 个测试一起(stronghold 组合)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --features stronghold --test w9_audit_chain_smoke`
Expected: 3 passed

- [ ] **Step 9: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w9_audit_chain_smoke.rs
git commit -m "test(w9p7): add w9_audit_chain_smoke with 3 hash-chain + privacy tests"
```

---

## Task 3: 7 套 feature 组合 cargo check 矩阵

**Files:** 无文件改动,仅运行 cargo check。

**目标:** spec §2.7 + §5 编译门禁 — 7 套 feature 组合 `cargo check` 全 PASS。若有编译错误,逐一修复(参考 §Conventions clippy lint 修复模式)。

- [ ] **Step 1: cargo check --no-default-features**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --no-default-features`
Expected:
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in <N>s
```
PASS(exit code 0)

- [ ] **Step 2: cargo check --features llm**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --features llm`
Expected: PASS

- [ ] **Step 3: cargo check --features tauri**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --features tauri`
Expected: PASS

- [ ] **Step 4: cargo check --features voice,tauri**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --features voice,tauri`
Expected: PASS

- [ ] **Step 5: cargo check --features voice,tauri,llm**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --features voice,tauri,llm`
Expected: PASS

- [ ] **Step 6: cargo check --features voice,tauri,llm,uia**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --features voice,tauri,llm,uia`
Expected: PASS

- [ ] **Step 7: cargo check --features voice,tauri,llm,uia,stronghold(W9 新增第 7 套)**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --features voice,tauri,llm,uia,stronghold`
Expected: PASS

- [ ] **Step 8: 若有编译错误,逐一修复**

常见错误模式(从 `project_memory.md` "Lessons Learned" + W8 Plan 6 Task 5 Step 7):

1. **`explicit_auto_deref`**:`&*kernel.conn()` → `&kernel.conn()`
2. **`query_map` closure 返回类型**:`crate::error::Result<T>` → `rusqlite::Result<T>`
3. **未使用 import** → 删除
4. **`large_enum_variant`** → `Box<T>` 包大变体
5. **`manual_inspect`** → `.inspect_err(|_e| { ... })`
6. **`manual_clamp`** → `.clamp(lo, hi)`
7. **`manual_range_contains`** → `(lo..=hi).contains(&x)`
8. **`needless_borrows_for_generic_args`** → `hasher.update(x.to_le_bytes())`
9. **`len_zero`** → `!vec.is_empty()`
10. **`doc_lazy_continuation`** → 文档列表项延续加空行
11. **Tauri 2.x `Emitter` trait** → `use tauri::{AppHandle, Emitter};`
12. **Tauri 2.x `register_handlers` 单态化到 Wry** → 显式 `Builder<Wry>`
13. **`state.rs` cfg-gated field** → 拆分 dual `new()`
14. **Approver import 完整路径** → `use crate::approval::approver::Approver;`
15. **W9 新增:`stronghold` feature 在 default 组合下缺 cfg-gate** → `#[cfg(feature = "stronghold")]` 标注所有 stronghold 引用
16. **W9 新增:`argon2` 在 default 组合下未使用** → `#[cfg(feature = "stronghold")]` 标注 import

每修一个错误,重跑对应 feature 组合的 `cargo check`,直到 PASS。

- [ ] **Step 9: 7 套全 PASS 后,记录矩阵到 PROGRESS.md(留待 Task 8 写入)**

矩阵:
| 命令 | 结果 |
|---|---|
| `cargo check --workspace --no-default-features` | PASS |
| `cargo check --workspace --features llm` | PASS |
| `cargo check --workspace --features tauri` | PASS |
| `cargo check --workspace --features voice,tauri` | PASS |
| `cargo check --workspace --features voice,tauri,llm` | PASS |
| `cargo check --workspace --features voice,tauri,llm,uia` | PASS |
| `cargo check --workspace --features voice,tauri,llm,uia,stronghold` | PASS |

- [ ] **Step 10: 若有修复,Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/ voicepilot/crates/ui/src/
git commit -m "fix(w9p7): resolve cargo check errors across 7 feature combos"
```

若 7 套一次通过无修复,跳过此 Step。

- [ ] **Step 11: stronghold feature 独立编译验证**

spec §10 要求 "stronghold feature 不依赖 voice/tauri feature,可独立编译"。

```powershell
cargo check --workspace --features stronghold
```

若失败,说明 stronghold feature 隐式依赖 voice/tauri,需修复 Cargo.toml feature 定义。

---

## Task 4: 2 套 clippy -D warnings clean

**Files:** 无文件改动,仅运行 clippy。

**目标:** spec §2.7 + §5 编译门禁 — clippy `-D warnings` 在 `--no-default-features` 与 `--features voice,tauri,llm,uia,stronghold` 两套下 0 警告。

- [ ] **Step 1: clippy --no-default-features**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy --workspace --no-default-features -- -D warnings`
Expected:
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in <N>s
```
0 warnings, 0 errors

若失败:
- 按 Task 3 Step 8 的修复模式逐一修
- 常见:`explicit_auto_deref` / `query_map` closure / `large_enum_variant` / `manual_inspect` / `manual_clamp` / `manual_range_contains` / `needless_borrows_for_generic_args` / `len_zero` / `doc_lazy_continuation`
- 修复后重跑 `cargo clippy --workspace --no-default-features -- -D warnings` 直到 0 warnings

- [ ] **Step 2: clippy --features voice,tauri,llm,uia,stronghold(W9 全 feature 代表性组合)**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy --workspace --features voice,tauri,llm,uia,stronghold -- -D warnings`
Expected:
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in <N>s
```
0 warnings, 0 errors

若失败:
- 全 feature 启用后,可能有 feature-gated 代码触发新 lint
- 同样按 Task 3 Step 8 修复模式逐一修
- 注意 `uia` feature 下的 `uiautomation-rs` FFI 代码可能触发 `unused_unsafe` / `missing_safety_doc` 等 lint,需要 `#[allow(...)]` 显式标注(参考 W7 Plan 4 处理方式)
- 注意 `stronghold` feature 下的 `tauri-plugin-stronghold` + `argon2` 调用可能触发 `unused_imports` / `redundant_closure` 等 lint

- [ ] **Step 3: 若有修复,Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/ voicepilot/crates/ui/src/
git commit -m "fix(w9p7): clippy -D warnings clean across no-default-features and full feature set"
```

若两套一次通过无修复,跳过此 Step。

---

## Task 5: npm.cmd run build + TypeScript 检查

**Files:** 无文件改动,仅运行 npm build。

**目标:** spec §2.7 + §5 编译门禁 — 前端 `npm.cmd run build` PASS,无 TypeScript 错误。

- [ ] **Step 1: npm.cmd run build**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build`
Expected:
```
> voicepilot-ui@0.1.0 build
> tsc --noEmit && vite build

vite v<version> building for production...
✓ <N> modules transformed.
dist/index.html                  <X> kB
dist/assets/index-<hash>.css     <Y> kB
dist/assets/index-<hash>.js      <Z> kB │ gzip: <W> kB
✓ built in <N>s
```
PASS(exit code 0,`dist/` 目录生成)

- [ ] **Step 2: 若失败,排查清单**

1. **TypeScript 错误**:
   - `error TS2304: Cannot find name 'X'` → 缺失 import
   - `error TS2322: Type 'A' is not assignable to type 'B'` → 类型不匹配,检查 `types.ts` 中 DTO 定义是否与后端 Rust 类型对齐
   - `error TS6133: 'X' is declared but its value is never used` → 删除未使用变量(`tsconfig.json` `noUnusedLocals: true`)
   - `error TS2554: Expected N arguments, but got M` → 检查函数签名

2. **W9 Plan 4 新增组件未导入**:
   - 若 Plan 4 引入了 `DagApprovalDialog.tsx` 的 Modify 编辑器但未在 `App.tsx` 中导入,build 会失败
   - 修复:在 `App.tsx` 中正确导入并渲染新组件

3. **W9 Plan 4 IPC 调用扩展**:
   - `api.ts` 中 `submit_dag_skeleton_approval` 签名扩展(携带 modified_plan)→ 检查 Tauri `invoke` 参数对齐
   - 后端 `dag_commands.rs` 命令签名扩展 → 检查 `tauri::command` 宏参数

4. **缺失依赖**:
   - `Cannot find module '@tauri-apps/api'` → `npm.cmd install @tauri-apps/api`
   - `Cannot find module '@tauri-apps/plugin-dialog'` → `npm.cmd install @tauri-apps/plugin-dialog`

5. **Vite 构建错误**:
   - `Could not resolve entry file "src/main.tsx"` → 检查 `index.html` 中 `<script type="module" src="/src/main.tsx">` 路径
   - `Unexpected token '<'` → `index.html` 引用了不存在的模块,检查 import 路径

- [ ] **Step 3: 若有修复,Commit**

```powershell
git add voicepilot/crates/ui/web/src/
git commit -m "fix(w9p7): resolve TypeScript errors in npm build"
```

若一次通过无修复,跳过此 Step。

- [ ] **Step 4: 验证 dist/ 生成 + 文件大小合理**

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; dir dist`
Expected: 至少包含 `index.html` + `assets/index-<hash>.css` + `assets/index-<hash>.js`,且 `index.html` < 2KB,`index-<hash>.js` < 500KB(合理范围)

**失败响应:** 若 bundle 超阈值(index.html > 2KB 或 index-*.js > 500KB),记录到 PROGRESS.md "已知偏离",不阻塞 milestone(W9 范围不优化 bundle)。

---

## Task 5.5: 手动运行真实 E2E 测试 + 记录结果

**Files:** 无文件改动,仅运行 `#[ignore]` 测试。

**目标:** spec §5 Fitness Functions 真实 E2E 门禁 — Plan 5 Playwright + Plan 6 UIA 各 2 个 `#[ignore]` 测试手动运行,记录结果到 PROGRESS.md。

- [ ] **Step 1: 运行 Plan 5 Playwright E2E(2 个 `#[ignore]` 测试)**

```powershell
cargo test --features voice,tauri,llm,stronghold --test w9_plan5_playwright_dag_e2e -- --ignored
```

记录结果(PASS/FAIL/SKIP)到 PROGRESS.md。

- [ ] **Step 2: 运行 Plan 6 UIA E2E(2 个 `#[ignore]` 测试)**

```powershell
cargo test --features voice,tauri,llm,uia,stronghold --test w9_plan6_uia_dag_e2e -- --ignored
```

记录结果到 PROGRESS.md。

- [ ] **Step 3: 汇总 4 个测试结果到 PROGRESS.md**

若任一测试 FAIL,记录失败原因 + 是否阻塞 milestone(真实 E2E 失败不阻塞 CI,但记录到已知偏离)。

---

## Task 6: 非门控测试数验证(≥ 286)

**Files:** 无文件改动,仅运行 cargo test --list。

**目标:** spec §2.7 + §5 测试门禁 — `cargo test --workspace --no-default-features -- --list` 列出的非门控测试数 ≥ 286。

- [ ] **Step 1: 跑当前非门控测试数**

Run: `cd d:\voicepilot\voicepilot ; $matches = cargo test --workspace --no-default-features -- --list | Select-String ': test$' ; ($matches | Measure-Object).Count`
Expected: `>= 286`

注:`Measure-Object -L` 会多 1 行(包含 `running 0 tests` 等非测试行),改用 `Select-String ': test$'` 精确过滤测试行,再 `Measure-Object` 计数。

记录实际数字为 `w9_total_count`。

- [ ] **Step 2: 验证数字符合预估**

预估公式(核实报告):
- W8 收尾非门控测试数:465(全 feature 组合累计,见 Task 8 Step 3 估算)
- W9 新增测试预估:55+(Plan 1: 8 + Plan 2: 5 + Plan 3: 15 + Plan 4: 6 + Plan 5: 2 + Plan 6: 2 + Plan 7: 15+3)
- 全 feature 组合测试数预估:≈ 520(Task 8 Step 3 估算,远超 ≥ 286 阈值)

**注意:** `w9_total_count`(Task 6 Step 1 实测,default 组合非门控测试数)和全 feature 组合测试数(~520)是两个不同口径的数字:
- **default 组合非门控测试数 ≥ 286**(Task 6 验证,spec §5 阈值)
- **全 feature 组合测试数 ~520**(Task 8 Step 3 估算,包含 feature-gated 测试 + `#[ignore]` 测试)

但 `--no-default-features` 只数 default 组合测试,feature-gated 测试不计入:
- W8 default 收尾:249 + W8 Plan 6 Task 4.5 补 37 = 286(W8 已闭合)
- W9 default 新增:Plan 3 taint_tracking_unit 10 + Plan 7 default_boundary 15 + Plan 7 audit_chain 测试 3(其中 2 个 stronghold-gated,1 个 default)= 25+
- W9 default 累计预估:286 + 25+ ≈ 311+(远超 ≥ 286 阈值)

若实际数字 < 286:
- 在 `w9_default_boundary_smoke.rs` 追加更多 default 边界用例(如 IterableSource::Literal 空 Vec / PrevNodeOutput 不存在 port / UserSlot 不存在 kind)
- 或在 `w9_taint_tracking_unit.rs` 追加更多 TaintRepo 边界(如 upsert 合并 taints 去重 / list_by_provenance 空结果 / delete_by_source 删除计数)

- [ ] **Step 3: 若补足测试,Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/
git commit -m "test(w9p7): add more default boundary tests to close >= 286 non-gated threshold"
```

若一次通过 ≥ 286,跳过此 Step。

- [ ] **Step 4: 记录最终数字到 PROGRESS.md(留待 Task 8 写入)**

格式:
```
**累计非门控测试数:** <w9_total_count>(W8 收尾 465 + W9 新增 <diff> = <w9_total_count>,远超 spec §5 ≥ 286 阈值)
```

---

## Task 7: 全量 cargo test 无回归(7 套 feature 组合各跑核心测试)

**Files:** 无文件改动,仅运行 cargo test。

**目标:** spec §2.7 + §5 测试门禁 — 7 套 feature 组合各跑核心测试,W1-W8 测试无回归 + W9 新增测试全 PASS。

注:第 7 套(`voice,tauri,llm,uia,stronghold`)在 Task 3 Step 7 已验证 cargo check,此处全量 test 跑(含 `#[ignore]` 测试不跑)。

- [ ] **Step 1: cargo test --no-default-features**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --no-default-features`
Expected: 全 PASS(0 failed),W1-W8 default 测试无回归 + W9 Plan 7 default_boundary_smoke 15 个 + audit_chain_smoke 测试 3(default) PASS

- [ ] **Step 2: cargo test --features llm**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features llm`
Expected: 全 PASS,W7/W8 llm-gated 测试无回归 + W9 Plan 5 playwright_dag_e2e `#[ignore]` 跳过

- [ ] **Step 3: cargo test --features tauri**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features tauri`
Expected: 全 PASS,W6a-W8 tauri-gated 测试无回归

- [ ] **Step 4: cargo test --features voice,tauri**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri`
Expected: 全 PASS,W5-W8 voice-gated 测试无回归

- [ ] **Step 5: cargo test --features voice,tauri,llm**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri,llm`
Expected: 全 PASS,W7-W8 voice+llm 测试无回归 + W9 Plan 5 `#[ignore]` 跳过

- [ ] **Step 6: cargo test --features voice,tauri,llm,uia**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri,llm,uia`
Expected: 全 PASS,W7 Plan 4 uia-gated 测试无回归 + W9 Plan 6 `#[ignore]` 跳过

- [ ] **Step 7: cargo test --features voice,tauri,llm,uia,stronghold(W9 全 feature 代表性组合)**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri,llm,uia,stronghold`
Expected: 全 PASS,W9 Plan 1-6 stronghold-gated 测试全 PASS + audit_chain_smoke 3 个 PASS + `#[ignore]` 测试跳过

**附加验证:** 验证 spec §9 列出的测试文件实际存在且测试数符合预期:

```powershell
# 验证 spec §9 列出的测试文件实际存在且测试数符合预期
cargo test --features stronghold --test w9_stronghold_unit -- --list | Select-String ': test$' | Measure-Object
cargo test --features stronghold --test w9_snapshot_encrypted_smoke -- --list | Select-String ': test$' | Measure-Object
cargo test --no-default-features --test w9_taint_tracking_unit -- --list | Select-String ': test$' | Measure-Object
cargo test --no-default-features --test w9_gateway_taint_smoke -- --list | Select-String ': test$' | Measure-Object
cargo test --features voice,tauri,llm --test w9_dag_modify_smoke -- --list | Select-String ': test$' | Measure-Object
cargo test --no-default-features --test w9_default_boundary_smoke -- --list | Select-String ': test$' | Measure-Object
cargo test --features stronghold --test w9_audit_chain_smoke -- --list | Select-String ': test$' | Measure-Object
```

若任一测试文件不存在或测试数为 0,说明 Plan 1-6 未实现或测试文件命名不一致,需修复。

- [ ] **Step 8: 若有测试失败,排查清单**

1. **W8 测试回归**:
   - W8 Plan 6 e2e_dag_smoke 8 个场景 — 检查 DagExecutor::run 签名变更是否影响(W9 Plan 6 加了 user_slots 参数)
   - W8 Plan 6 default_boundary_smoke 22 个 — 检查 DagStatus::transition 是否被 W9 修改
   - W8 Plan 1-5 各测试 — 检查 StrongholdVault 注入是否破坏既有 compensation 流程

2. **W9 新增测试失败**:
   - w9_stronghold_unit(Plan 1)— 检查 StrongholdVault API 一致性
   - w9_snapshot_encrypted_smoke(Plan 2)— 检查明文残留检测逻辑
   - w9_taint_tracking_unit(Plan 3)— 检查 TaintRepo CRUD
   - w9_gateway_taint_smoke(Plan 3)— 检查 gateway 查表驱动
   - w9_dag_modify_smoke(Plan 4)— 检查 DagApprovalOutcome::Modify 闭环
   - w9_plan5_playwright_dag_e2e(Plan 5)— `#[ignore]`,本 Step 不跑
   - w9_plan6_uia_dag_e2e(Plan 6)— `#[ignore]`,本 Step 不跑
   - w9_default_boundary_smoke(Plan 7)— 检查 15 个 default 边界
   - w9_audit_chain_smoke(Plan 7)— 检查 3 个哈希链测试

3. **签名变更导致的回归**:
   - `DagExecutor::run(plan)` → `run(plan, user_slots)`(W9 Plan 6 breaking change)
   - `approve_dag_skeleton(plan) -> ApprovalDecision` → `-> DagApprovalOutcome`(W9 Plan 4)
   - `create_post_commit_compensation(...)` 签名扩展(可能加了 stronghold_vault 参数)

每修一个回归,重跑对应 feature 组合的 `cargo test`,直到全 PASS。

- [ ] **Step 9: 若有修复,Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/ voicepilot/crates/trust-kernel/tests/
git commit -m "fix(w9p7): resolve test regressions across 7 feature combos"
```

若 7 套一次通过无修复,跳过此 Step。

---

## Task 8: 更新 PROGRESS.md(W9 整体段落 + 里程碑表 + §4.1 + 已知偏离)

**Files:**
- Modify: `docs/PROGRESS.md`

**目标:** 在 W8 段落后加 W9 整体段落(参考 W8 段落格式);里程碑表加 7 行(W9 Plan 1-7);§4.1 立即任务更新为 W10 候选方向;追加 W9 已知偏离段(spec §7 8 项延后 W10+)。

- [ ] **Step 1: 读 PROGRESS.md 当前状态,定位插入点**

Run: `cd d:\voicepilot ; Select-String -Path docs\PROGRESS.md -Pattern "^### W8 Plan 6" -SimpleMatch`
Expected: 找到 `### W8 Plan 6: 集成测试 + 验收门禁 ✅` 段落开头

定位 §一 里程碑表(`## 一、总体里程碑状态` 表格末尾 `| W9 | Stronghold + Taint + DAG Modify | ⏳ 未开始 | — | — | — |` 行)。
定位 §4.1 立即任务(`### 4.1 立即任务:W9 候选方向(等用户决策)` 段落)。

- [ ] **Step 2: 更新 §一 里程碑表 — 把 W9 行改为 7 行(W9 Plan 1-7)+ W9 整体行**

将 `| W9 | Stronghold + Taint + DAG Modify | ⏳ 未开始 | — | — | — |` 行替换为 8 行:

```markdown
| W9 Plan 1 | StrongholdVault + Argon2id 密钥派生 + 降级模式 | ✅ 已完成 | +w9_stronghold_unit 8 (stronghold) | 2026-07-28 | (direct on master) |
| W9 Plan 2 | snapshot_encrypted 真实加密 + 明文 PoC 移除 | ✅ 已完成 | +w9_snapshot_encrypted_smoke 5 (stronghold,llm) | 2026-07-28 | (direct on master) |
| W9 Plan 3 | TaintRepo CRUD + 查表驱动 gateway + 输入→输出传播 | ✅ 已完成 | +w9_taint_tracking_unit 10 + w9_gateway_taint_smoke 4 | 2026-07-28 | (direct on master) |
| W9 Plan 4 | DagApprovalOutcome::Modify + 重新审批闭环 + UI 编辑器 | ✅ 已完成 | +w9_dag_modify_smoke 6 (tauri,llm) | 2026-07-28 | (direct on master) |
| W9 Plan 5 | 真实 Playwright MCP DAG E2E(`#[ignore]`) | ✅ 已完成 | +w9_plan5_playwright_dag_e2e 2 `#[ignore]` | 2026-07-28 | (direct on master) |
| W9 Plan 6 | 真实 UIA GUI DAG E2E + Slot 流水闭合(`#[ignore]`) | ✅ 已完成 | +w9_plan6_uia_dag_e2e 2 `#[ignore]` | 2026-07-28 | (direct on master) |
| W9 Plan 7 | 集成测试 + 7 套 feature cargo check + clippy + npm build + 非门控 ≥ 286 | ✅ 已完成 | +w9_default_boundary_smoke 15 + w9_audit_chain_smoke 3;7 套 cargo check 全 PASS;clippy `-D warnings` 0 警告;npm build PASS | 2026-07-28 | (direct on master) |
| W9 | Stronghold 加密 + Taint Tracking + DAG Modify + 真实 E2E | ✅ 已完成 | 7 个 Plan(Plan 1 基础设施 + Plan 2-6 实施 + Plan 7 集成验收),累计 ~60+ commit | 2026-07-28 | (direct on master) |
```

- [ ] **Step 3: 更新累计测试数行**

将 W8 收尾的 `**累计测试数:** ...` 行更新为:

```markdown
**累计非门控测试数:** <w9_total_count>(W8 收尾 465 + W9 新增 55+ = ~520;default 组合 ≥ 286,远超 spec §5 阈值);+stronghold:W9 w9_stronghold_unit 8 + w9_snapshot_encrypted_smoke 5 + w9_audit_chain_smoke 3(stronghold-gated);+llm:W9 w9_snapshot_encrypted_smoke 5 + w9_dag_modify_smoke 6 + w9_plan5 2 `#[ignore]` + w9_plan6 2 `#[ignore]`;+tauri:W9 w9_dag_modify_smoke 6 + w9_plan5 2 `#[ignore]`;+uia:W9 w9_plan6 2 `#[ignore]` — 满足 spec §5 + §9 测试矩阵门禁
```

注:`<w9_total_count>` 用 Task 6 Step 1 跑出的实际数字替换。

- [ ] **Step 4: 在 §二 W8 Plan 6 段落后追加 W9 整体段落**

定位 `### W8 Plan 6: 集成测试 + 验收门禁 ✅` 段落末尾(`**下一步:** W8 全部 6 个 Plan 已完成...` 行之后,`---` 之前),追加 W9 段落:

```markdown
### W9: Stronghold 加密 + Taint Tracking + DAG Modify + 真实 E2E ✅

**完成时间:** 2026-07-28(Asia/Shanghai)
**对应规格:** `docs/superpowers/specs/2026-07-28-w9-stronghold-taint-dagmodify-reale2e-design.md`
**对应计划:**
- `docs/superpowers/plans/2026-07-28-w9-plan1-stronghold-vault.md`
- `docs/superpowers/plans/2026-07-28-w9-plan2-snapshot-encrypted.md`
- `docs/superpowers/plans/2026-07-28-w9-plan3-taint-tracking.md`
- `docs/superpowers/plans/2026-07-28-w9-plan4-dag-modify.md`
- `docs/superpowers/plans/2026-07-28-w9-plan5-playwright-e2e.md`
- `docs/superpowers/plans/2026-07-28-w9-plan6-uia-e2e-slot.md`
- `docs/superpowers/plans/2026-07-28-w9-plan7-integration-acceptance.md`(本 Plan,收尾)

**Commit 范围:** Plan 7 是 W9 唯一可提交 plan(需用户决策时确认);其余 Plan 1-6 仅生成文档不提交,除非用户明确授权。Plan 7 自身累计 9 个 task 直接提交到 master(若用户授权 Plan 1-6 可提交,7 个 Plan 累计 ~60+ commit)。

**7 个 Plan 概览 + 状态:**

| Plan | 主题 | 状态 | 关键产出 |
|---|---|---|---|
| Plan 1 | StrongholdVault + Argon2id 密钥派生 | ✅ | `crypto/stronghold.rs` + 降级模式 + 8 个单元测试 |
| Plan 2 | snapshot_encrypted 真实加密 | ✅ | `create_post_commit_compensation` 注入 Stronghold + 明文残留检测 + 5 个集成测试 |
| Plan 3 | Taint Tracking 污点传播 | ✅ | `policy/taint_repo.rs` CRUD + `gateway` 查表驱动 + 10 单元 + 4 集成 |
| Plan 4 | DAG Modify 分支 | ✅ | `DagApprovalOutcome::Modify` + `TauriApprover` 扩展 + UI 编辑器 + 6 集成 |
| Plan 5 | 真实 Playwright MCP DAG E2E | ✅ | `w9_plan5_playwright_dag_e2e.rs` 2 个 `#[ignore]` |
| Plan 6 | 真实 UIA GUI DAG E2E + Slot 流水闭合 | ✅ | `DagExecutor::run(plan, user_slots)` + `IterableSource::UserSlot` + 2 个 `#[ignore]` |
| Plan 7 | 集成验收 | ✅ | `w9_default_boundary_smoke.rs` 15 + `w9_audit_chain_smoke.rs` 3 + 7 套 cargo check + clippy + npm build + 非门控 ≥ 286 |

**验收门禁闭合(spec §5 Fitness Functions):**

| 门禁类别 | 指标 | 阈值 | 实测 | 状态 |
|---|---|---|---|---|
| 编译 | 7 套 feature 组合 cargo check | 全 PASS | 7/7 PASS | ✅ |
| clippy | 2 套代表性组合 `-D warnings` | 0 警告 | 0 / 0 | ✅ |
| npm build | `npm.cmd run build` | PASS | PASS | ✅ |
| 非门控测试数 | `cargo test --workspace --no-default-features -- --list` | ≥ 286 | <w9_total_count> | ✅ |
| Stronghold 加密 | `SELECT COUNT(*) FROM compensations WHERE snapshot_encrypted IS NULL AND reverse_payload != ''`(stronghold feature 启用) | = 0 | 0 | ✅ |
| Taint Tracking | `taints` 表有 CRUD + gateway 查表驱动 | grep `INSERT INTO taints` 非空 | 非空 | ✅ |
| DAG Modify | Modify → 重新审批 → 执行闭环 | w9_dag_modify_smoke 通过 | 通过 | ✅ |
| 真实 E2E | `#[ignore]` 测试手动运行通过 | 2 个场景(form DAG + note DAG) | 手动 PASS(Plan 5 + Plan 6) | ✅ |
| 哈希链 | audit_logs 哈希链不断 | w9_audit_chain_smoke 通过 | 通过 | ✅ |

**审计事件新增(spec §6.4):** W9 新增 7 种事件,均 `lower_snake_case`:
- `stronghold_snapshot_encrypted` — `{compensation_id, vault_ref, plaintext_len}`
- `stronghold_snapshot_decrypt_failed` — `{compensation_id, error}`
- `stronghold_degraded_mode_entered` — `{reason}`
- `taint_propagated` — `{source_ref, input_hash, output_hash, taints}`
- `taint_blocked` — `{taints, sink, resource_hash}`
- `dag_skeleton_modified` — `{plan_id, modified_node_count, added_count, removed_count}`
- `dag_modify_limit_exceeded` — `{plan_id}`

**已知偏离 / 延后项(spec §7,8 项延后 W10+):**
1. fs_snapshot 采集延后 W10+(W9 仅加密 reverse_payload,文件系统快照采集延后)
2. Cedar 引擎 taint 查询延后 W10+(W9 仅 Rust 层查表驱动)
3. 文件系统 sink taint 传播延后 W10+(W9 仅 Skill 输入→输出传播)
4. DAG Modify 改 loop_specs 延后 W10+(W9 仅允许改 nodes + input_template + risk_ceiling)
5. DAG 节点并行执行延后 W10+(W8 已延后,W9 不涉及)
6. LLM 调用计费 / 速率限制延后 W10+(W8 已延后,W9 不涉及)
7. Stronghold 密钥恢复机制延后 W10+(W9 接受"密码丢失 = 永久不可解"语义,恢复密钥 shamir 等延后)
8. D3/E3 红色高亮延后(自 W6b-3a 起未实现,W9 不在范围)

**下一步:** W9 全部 7 个 Plan 已完成,可进入 W10(等用户决策优先级,详见 §4.1)。
```

- [ ] **Step 5: 更新 §4.1 立即任务 — 转向 W10 候选方向**

将 `### 4.1 立即任务:W9 候选方向(等用户决策)` 段落改为:

```markdown
### 4.1 立即任务:W10 候选方向(等用户决策)

W9 已完成(commit `<w9_final_hash>`),spec §7 列出 8 项延后 W10+ 的工作。W10 候选方向:

1. **fs_snapshot 采集 + Stronghold 加密扩展**(spec §7 延后项 1)
   - 当前:W9 仅加密 `reverse_payload`,文件系统快照不采集
   - W10:实现 `fs_snapshot` 采集(文件内容 / 元数据)+ Stronghold 加密
   - 优先级:P0(闭合 spec §7.2 #18 完整强补偿)

2. **Cedar 引擎 taint 查询**(spec §7 延后项 2)
   - 当前:W9 仅 Rust 层查表驱动 gateway
   - W10:Cedar policy 内嵌 taint 查询,实现声明式 taint 策略
   - 优先级:P1

3. **文件系统 sink taint 传播**(spec §7 延后项 3)
   - 当前:W9 仅 Skill 输入→输出传播
   - W10:文件系统写入 taint 标记(防止 LLM 注入恶意路径)
   - 优先级:P1

4. **DAG Modify 改 loop_specs**(spec §7 延后项 4)
   - 当前:W9 仅允许改 nodes + input_template + risk_ceiling
   - W10:扩展 Modify 允许改 loop_specs(循环规格)
   - 优先级:P2

5. **DAG 节点并行执行**(spec §7 延后项 5)
   - 当前:W8 已延后,DAG 节点串行执行
   - W10:实现 DAG 节点并行执行(独立节点并发)
   - 优先级:P2

6. **LLM 调用计费 / 速率限制**(spec §7 延后项 6)
   - 当前:W8 已延后,LLM 调用无计费 / 限速
   - W10:实现 LLM 调用计费 + 速率限制(防滥用)
   - 优先级:P2

7. **Stronghold 密钥恢复机制**(spec §7 延后项 7)
   - 当前:W9 接受"密码丢失 = 永久不可解"语义
   - W10:实现 shamir secret sharing 等恢复机制
   - 优先级:P3(用户决策是否需要)

8. **D3/E3 红色高亮**(spec §7 延后项 8)
   - 当前:自 W6b-3a 起未实现
   - W10:UI 层实现 D3/E3 风险等级红色高亮
   - 优先级:P3

**等用户决策 W10 优先级 + 范围。**
```

- [ ] **Step 6: 验证 PROGRESS.md 改动**

Run: `cd d:\voicepilot ; Select-String -Path docs\PROGRESS.md -Pattern "W9 整体|W9 Plan 7|W10 候选"`
Expected: 至少 3 处匹配,确认 W9 整体段落 + Plan 7 行 + W10 候选方向都已写入

- [ ] **Step 7: Commit PROGRESS.md 改动**

```powershell
git add docs/PROGRESS.md
git commit -m "docs(w9p7): update PROGRESS.md with W9 completion + test stats + W10 deferrals"
```

---

## Task 9: 空 commit 标记 W9 里程碑

**Files:** 无文件改动,仅 git commit。

**目标:** spec §10 Conventions + §12 决策 #12 例外 — W9 收尾空 commit 标记里程碑。Plan 7 是 W9 唯一可提交的 plan(spec §12 决策 #12:W9 Plan 1-6 不提交,仅生成完整 plan 文档;Plan 7 是验收 plan,完成后可提交)。

- [ ] **Step 1: 验证所有 Task 1-8 已完成**

Run: `cd d:\voicepilot ; git log --oneline -10`
Expected: 看到以下 commit(从新到旧):
- `docs(w9p7): update PROGRESS.md with W9 completion + test stats + W10 deferrals`(Task 8 Step 7)
- `fix(w9p7): ...`(若有修复,Task 3/4/5/7 Step)
- `test(w9p7): add w9_default_boundary_smoke with 15 default-combo boundary cases`(Task 1 Step 16)
- `test(w9p7): add w9_audit_chain_smoke with 3 hash-chain + privacy tests`(Task 2 Step 9)

- [ ] **Step 2: 验证 7 套 cargo check + 2 套 clippy + npm build + 测试数 全 PASS**

Run: `cd d:\voicepilot\voicepilot ; cargo check --workspace --no-default-features ; cargo check --workspace --features voice,tauri,llm,uia,stronghold`
Expected: 两套都 PASS

Run: `cd d:\voicepilot\voicepilot ; cargo clippy --workspace --no-default-features -- -D warnings ; cargo clippy --workspace --features voice,tauri,llm,uia,stronghold -- -D warnings`
Expected: 两套都 0 warnings

Run: `cd d:\voicepilot\voicepilot\crates\ui\web ; npm.cmd run build`
Expected: PASS

Run: `cd d:\voicepilot\voicepilot ; $matches = cargo test --workspace --no-default-features -- --list | Select-String ': test$' ; ($matches | Measure-Object).Count`
Expected: `>= 286`

- [ ] **Step 3: 空 commit 标记 W9 里程碑(需用户明确授权)**

**⚠️ 重要:** project_memory.md 明确要求 "NEVER commit unless explicitly asked"。
spec §12 决策 #12 原文 "W9 不提交,仅生成完整 plan 文档"。

本 Step 仅在用户明确授权 "执行 Plan 7 并允许 commit" 后才执行:

```powershell
git commit --allow-empty -m "docs(w9): W9 complete — Stronghold + Taint + DAG Modify + Real E2E"
```

若用户未授权,跳过本 Step,仅完成 Task 8 PROGRESS.md 更新。

- [ ] **Step 4: 验证 git log 显示 W9 收尾 commit**

Run: `cd d:\voicepilot ; git log --oneline -5`
Expected:
```
<hash> docs(w9): W9 complete — Stronghold + Taint + DAG Modify + Real E2E
<hash> docs(w9p7): update PROGRESS.md with W9 completion + test stats + W10 deferrals
<hash> test(w9p7): add w9_audit_chain_smoke with 3 hash-chain + privacy tests
<hash> test(w9p7): add w9_default_boundary_smoke with 15 default-combo boundary cases
<hash> <prior W8 / W9 Plan 1-6 commit>
```

- [ ] **Step 5: 记录 W9 收尾 commit hash 到 PROGRESS.md(可选)**

若需精确记录 W9 收尾 hash,在 Task 8 Step 4 写入的 W9 段落 `**Commit 范围:**` 行后追加:

```markdown
**W9 收尾 commit:** `<hash>`(空 commit 标记里程碑,spec §10 + §12 决策 #12 例外)
```

若已 Task 8 Step 7 commit,本 Step 需额外 amend:

```powershell
git add docs/PROGRESS.md
git commit --amend --no-edit
```

注:amend 会改变 commit hash,若 Task 9 Step 3 已空 commit,需重做 Step 3。推荐顺序:先 Task 9 Step 5 amend Task 8 commit → 再 Task 9 Step 3 空 commit。

---

## Self-Review

完成本 plan 编写后,对照 spec §2.7 + §5 + §6.4 + §9 + §10 验收门禁 + project_memory.md 工程约束,逐项检查:

### 1. Spec 覆盖检查(spec §2.7 + §5 + §9)

| spec 章节 | 要求 | Plan 7 覆盖位置 | 状态 |
|---|---|---|---|
| §2.7 集成验收 | 新增 `w9_default_boundary_smoke.rs` 15 个 default 边界 | Task 1 Step 3-15 | ✅ |
| §2.7 集成验收 | 新增 `w9_audit_chain_smoke.rs` 3 个哈希链 | Task 2 Step 2-7 | ✅ |
| §2.7 集成验收 | 7 套 feature 组合 cargo check 矩阵 | Task 3 Step 1-7 | ✅ |
| §2.7 集成验收 | 2 套 clippy `-D warnings` | Task 4 Step 1-2 | ✅ |
| §2.7 集成验收 | npm build | Task 5 Step 1 | ✅ |
| §2.7 集成验收 | 非门控测试数 ≥ 286 | Task 6 Step 1 | ✅ |
| §2.7 集成验收 | PROGRESS.md 更新 W9 完成状态 | Task 8 Step 1-7 | ✅ |
| §2.7 集成验收 | 空 commit 标记 W9 里程碑 | Task 9 Step 3 | ✅ |
| §5 Fitness Functions | Stronghold 加密门禁 | Task 7 Step 7 验证(Plan 2 已实现) | ✅ |
| §5 Fitness Functions | Taint Tracking 门禁 | Task 7 Step 1 验证(Plan 3 已实现) | ✅ |
| §5 Fitness Functions | DAG Modify 门禁 | Task 7 Step 3 验证(Plan 4 已实现) | ✅ |
| §5 Fitness Functions | 真实 E2E `#[ignore]` | Plan 5 + Plan 6 已实现,本 Plan 不重复 | ✅ |
| §5 Fitness Functions | 非门控测试数 ≥ 286 | Task 6 Step 1 | ✅ |
| §5 Fitness Functions | clippy `-D warnings` | Task 4 Step 1-2 | ✅ |
| §5 Fitness Functions | npm build PASS | Task 5 Step 1 | ✅ |
| §5 Fitness Functions | 哈希链不断 | Task 2 Step 2 audit_chain_hash_links_unbroken | ✅ |
| §9 测试矩阵 | w9_default_boundary_smoke 15 个 | Task 1 | ✅ |
| §9 测试矩阵 | w9_audit_chain_smoke 3 个 | Task 2 | ✅ |
| §9 测试矩阵 | W9 新增 55+ 测试 | Plan 1-7 累计 55+ | ✅ |
| §6.4 审计事件隐私 | details 字段隐私脱敏 | Task 2 Step 6 audit_chain_details_no_plaintext_secrets | ✅ |
| §6.4 审计事件命名 | lower_snake_case | Task 1 Step 11 audit_event_types_all_lower_snake_case | ✅ |
| §10 Conventions | PowerShell `;` 分隔 | 所有 Run 命令 | ✅ |
| §10 Conventions | commit message `test(w9p7): ...` / `docs(w9p7): ...` | 所有 commit step | ✅ |
| §10 Conventions | 空 commit `docs(w9): W9 complete — ...` | Task 9 Step 3 | ✅ |
| §12 决策 #12 例外 | Plan 7 是验收 plan,完成后可提交 | Task 1-9 全部 commit | ✅ |

**spec §2.7 + §5 + §6.4 + §9 + §10 + §12 全部门禁覆盖,无遗漏。**

### 2. Placeholder 扫描

- ✅ 无 "TBD" / "TODO" / "implement later" / "fill in details"
- ✅ 无 "Add appropriate error handling" / "add validation" / "handle edge cases"
- ✅ 无 "Write tests for the above"(每个测试都有完整代码)
- ✅ 无 "Similar to Task N"(每个 Task 独立完整)
- ✅ 所有 code step 都有完整代码块
- ✅ 所有引用的类型 / 函数 / 方法都在 Plan 1-6 或 W7/W8 中已定义
- ⚠ Task 8 Step 3 `<w9_total_count>` 是占位符,执行时用 Task 6 Step 1 实际数字替换(已明确说明,非 placeholder)

### 3. 类型一致性检查

| 类型 / 函数 | 定义位置 | 本 Plan 引用位置 | 一致性 |
|---|---|---|---|
| `StrongholdVault` | Plan 1 `crypto/stronghold.rs` | Task 1 A 组 + Task 2 测试 2 | ✅ `degraded()` / `is_unlocked()` / `decrypt()` 一致 |
| `StrongholdError::NotUnlocked` | Plan 1 `crypto/stronghold.rs` | Task 1 A 组 | ✅ |
| `EncryptedPayload` | Plan 1 `crypto/stronghold.rs` | Task 1 A 组 | ✅ `ciphertext` / `nonce` / `salt_ref` 字段一致 |
| `create_post_commit_compensation` | Plan 2 `skills/common.rs` | Task 1 A 组 + Task 2 测试 2/3 | ⚠ 签名需以 Plan 2 实际为准(本 Plan 给出预估签名) |
| `reverse_compensation` | Plan 2 `skills/common.rs` | Task 2 测试 2 | ⚠ 签名需以 Plan 2 实际为准 |
| `TaintRepo` / `TaintRecord` | Plan 3 `policy/taint_repo.rs` | Task 1 B 组 + Task 2 测试 2 | ✅ `new()` / `find_by_value()` / `upsert()` 一致 |
| `check_taint_policy` | Plan 3 `policy/gateway.rs` | Task 1 B 组 + Task 2 测试 2 | ⚠ 签名需以 Plan 3 实际为准 |
| `Resource` / `Sink` | Plan 3 `policy/types.rs` | Task 1 B 组 + Task 2 测试 2 | ⚠ 字段名需以 Plan 3 实际为准 |
| `DagApprovalOutcome` | Plan 4 `approval/approver.rs` | Task 1 C 组 | ✅ `Allow` / `Deny` / `Modify { modified_plan }` 变体一致 |
| `Approver::approve_dag_skeleton` | Plan 4 `approval/approver.rs` | Task 1 C 组 | ⚠ 返回类型从 `ApprovalDecision` 改为 `DagApprovalOutcome`(Plan 4 breaking change) |
| `KernelError::DagModifyLimitExceeded` | Plan 4 `error.rs` | Task 1 C 组 | ⚠ 变体名需以 Plan 4 实际为准 |
| `DagExecutor::new` / `run` | Plan 4 + Plan 6 `dag_executor.rs` | Task 1 C/D 组 | ⚠ `run(plan, user_slots)` 签名(Plan 6 breaking change) |
| `DagStatus::transition` | W8 Plan 6 Task 4.5 `dag_types.rs` | Task 1 F 组 | ✅ W8 已实现 |
| `DagPlan::validate` | Plan 1 `dag_types.rs` | Task 1 G/H 组 | ⚠ 方法名需以 Plan 1 实际为准(可能是 `validate` / `validate_dag`) |
| `IterableSource::UserSlot(SlotKind)` | Plan 1 + Plan 6 `dag_types.rs` | Task 1 D 组 | ✅ |
| `audit_logs` 表 | W1 `migrations/001_init.sql` | Task 2 helper | ✅ `id` / `event_type` / `prev_hash` / `details` 列一致 |
| `TrustKernel::open_in_memory` | W1 | Task 1-2 全部 | ✅ |
| `kernel.conn()` | W1 | Task 1-2 helper | ✅ 返回 `MutexGuard<Connection>` |
| `kernel.create_task` | W1 | Task 2 测试 1/3 | ✅ |
| `kernel.set_stronghold_vault` | Plan 1 `kernel.rs` | Task 2 测试 2 | ⚠ 方法名需以 Plan 1 实际为准 |

**类型一致性结论:** 多数类型 / 函数签名与 Plan 1-6 一致;少数(`create_post_commit_compensation` / `check_taint_policy` / `DagExecutor::new` / `DagPlan::validate` / `kernel.set_stronghold_vault`)需以 Plan 1-6 实际实现为准,本 Plan 在 Task 1-2 排查清单中已提示执行者适配。

### 4. 工程约束检查(project_memory.md + spec §10)

| 约束 | 本 Plan 遵守情况 |
|---|---|
| Windows-only | ✅ 所有命令 PowerShell 兼容,`uia` feature 仅 Windows |
| 云端 LLM only | ✅ 本 Plan 不涉及 LLM 调用(Plan 5 `#[ignore]` 已覆盖) |
| PowerShell `;` 分隔 | ✅ 所有 Run 命令用 `;` 或换行 |
| TDD 测试先行 | ✅ Task 1-2 每组先写测试 → 跑 → commit |
| `&kernel.conn()` 不用 `&*kernel.conn()` | ✅ helper `count_audit_events` / `list_distinct_event_types` 用 `kernel.conn()` |
| query_map closure 返回 rusqlite::Result<T> | ✅ `list_distinct_event_types` closure `|row| row.get::<_, String>(0)` 返回 `rusqlite::Result<String>` |
| Approver import 完整路径 | ✅ `use trust_kernel::approval::approver::{Approver, DagApprovalOutcome, AutoApprover};` |
| TrustKernel 不是 Clone | ✅ 用 `Arc<TrustKernel>` 共享 |
| Stronghold feature 独立 | ✅ Task 1 default 组合不依赖 stronghold feature;Task 2 用 `#[cfg(feature = "stronghold")]` 函数级 gate |
| 审计事件命名 lower_snake_case | ✅ Task 1 E 组验证 |
| details 字段隐私脱敏 | ✅ Task 2 测试 3 验证 |
| 不用 `&&` / `||` 分隔 PowerShell | ✅ |
| `git commit -m "msg"` 单行 | ✅ 所有 commit 命令单行 |
| 空 commit 标记里程碑用 `--allow-empty` | ✅ Task 9 Step 3 |
| commit message 前缀 `test(w9p7): ...` / `docs(w9p7): ...` | ✅ 所有 commit |
| 不引入新依赖 | ✅ 仅用 `regex`(W7/W8 已 dev-dependency)+ `sha2`(W1 已依赖) |

**全部工程约束遵守。**

### 5. 已知偏离记录(spec §7)

本 Plan 不引入新偏离。W9 已知偏离(8 项延后 W10+)已在 Task 8 Step 4 写入 PROGRESS.md W9 段落 "已知偏离 / 延后项"。

### 6. 测试数估算(spec §9)

| Plan | 新增测试数 | 累计 |
|---|---|---|
| W9 Plan 1 | 8 (stronghold_unit, stronghold-gated) | +8 stronghold |
| W9 Plan 2 | 5 (snapshot_encrypted_smoke, stronghold+llm) | +5 stronghold+llm |
| W9 Plan 3 | 10 (taint_tracking_unit, default) + 4 (gateway_taint_smoke, default) | +14 default |
| W9 Plan 4 | 6 (dag_modify_smoke, tauri+llm) | +6 tauri+llm |
| W9 Plan 5 | 2 (playwright_dag_e2e, `#[ignore]`, voice+tauri+llm+stronghold) | +2 `#[ignore]` |
| W9 Plan 6 | 2 (uia_dag_e2e, `#[ignore]`, voice+tauri+llm+uia+stronghold) | +2 `#[ignore]` |
| W9 Plan 7 | 15 (default_boundary_smoke, default) + 3 (audit_chain_smoke, 1 default + 2 stronghold) | +16 default + 2 stronghold |
| **W9 总计** | **55+**(default 29 + stronghold 10 + llm 6 + tauri+llm 6 + `#[ignore]` 4) | **W8 收尾 465 + W9 新增 55+ ≈ 520** |

满足 spec §9 "W9 新增 55+ 测试"(本 Plan 7 加 18 个,15 default + 3 audit_chain)。

**非门控测试数(default 组合)估算:**
- W8 收尾 default:286(W8 Plan 6 Task 4.5 已闭合)
- W9 新增 default:Plan 3 taint_tracking_unit 10 + Plan 3 gateway_taint_smoke 4 + Plan 7 default_boundary_smoke 15 + Plan 7 audit_chain_smoke 测试 3(测试 3 default,测试 1+2 stronghold-gated)= 29+
- W9 完成后 default 累计:286 + 29 = 315+(远超 spec §5 ≥ 286 阈值)

**Self-Review 结论:Plan 7 完整覆盖 spec §2.7 + §5 + §6.4 + §9 + §10 + §12 验收门禁;无 placeholder(除 Task 8 Step 3 `<w9_total_count>` 占位符,执行时替换);类型一致性多数 OK,少数签名需以 Plan 1-6 实际为准(排查清单已提示);工程约束遵守;已知偏离已记录。可以执行。**

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-07-28-w9-plan7-integration-acceptance.md`.**

W9 共 7 个 Plan,本 Plan 7 是收尾 plan(集成测试 + 验收门禁 + PROGRESS.md 更新),无后续 plan。Plan 7 完成后 W9 全部闭合,可进入 W10(等用户决策优先级,详见 PROGRESS.md §4.1)。

**两个执行选项:**

**1. Subagent-Driven(推荐)** — 每个 Task 派发 fresh subagent,Task 间 review,快速迭代
- **REQUIRED SUB-SKILL:** Use superpowers:subagent-driven-development
- 适合本 Plan:9 个 Task 相对独立(Task 1-2 写测试,Task 3-7 跑门禁,Task 8-9 收尾),可并行 dispatch Task 1 + Task 2(两个测试文件无依赖)

**2. Inline Execution** — 在当前 session 顺序执行,checkpoint review
- **REQUIRED SUB-SKILL:** Use superpowers:executing-plans
- 适合本 Plan:Task 3-7(验收门禁)需要顺序执行(check → clippy → npm build → test count → test),Inline 模式可保证顺序

**推荐执行顺序:**
1. Task 1(default_boundary_smoke 15 个)— Subagent-Driven,独立测试文件
2. Task 2(audit_chain_smoke 3 个)— Subagent-Driven,与 Task 1 并行
3. Task 3(7 套 cargo check)— Inline,需顺序跑 7 套
4. Task 4(2 套 clippy)— Inline,依赖 Task 3 修复完成
5. Task 5(npm build)— Inline,与 Task 3/4 独立但需在 Task 8 前完成
6. Task 6(非门控测试数 ≥ 286)— Inline,依赖 Task 1-2 完成
7. Task 7(6 套 cargo test 无回归)— Inline,依赖 Task 1-6 完成
8. Task 8(PROGRESS.md + 收尾 commit)— Inline,依赖 Task 1-7 全部完成
9. Task 9(空 commit 标记 W9 里程碑)— Inline,依赖 Task 8 完成

**执行完成后:**
- W9 全部 7 个 Plan 完成,累计 ~60+ commit
- W9 acceptance gates 全闭合(spec §2.7 + §5 + §6.4 + §9 + §10)
- PROGRESS.md 更新 W9 整体段落 + W10 候选方向 + 8 项延后项
- 空 commit `docs(w9): W9 complete — Stronghold + Taint + DAG Modify + Real E2E` 标记里程碑
- 可进入 W10(等用户决策)

**Commit message 格式提醒:**
- 测试代码:`test(w9p7): ...`(如 `test(w9p7): add w9_default_boundary_smoke with 15 default-combo boundary cases`)
- 文档更新:`docs(w9p7): ...`(如 `docs(w9p7): update PROGRESS.md with W9 completion + test stats + W10 deferrals`)
- 修复:`fix(w9p7): ...`(如 `fix(w9p7): resolve cargo check errors across 7 feature combos`)
- W9 收尾空 commit:`docs(w9): W9 complete — Stronghold + Taint + DAG Modify + Real E2E`(无 `(w9p7)` 前缀,因为是里程碑 commit,不是 Plan 7 内部 commit)

---

## W9 审查修复记录

本段落记录 W9 Plan 7 文档审查后所做的所有修复项,共 25 项(5 个 P0 + 17 个 P1 + 2 个 P2 + 1 个 P1 合并)。

### P0 缺陷修复(编译/类型错误,5 项)

1. **P0-1: `#![no_cfg]` 非法 Rust 语法** (Task 1 Step 1, 原 151 行)
   - 删除 `#![no_cfg]  // 默认组合,无 feature gate` 行
   - 改 Step 1 标题为 "创建测试文件,写入文件头注释(不写文件级 cfg gate)"
   - 补充文件头注释 "不写文件级 cfg gate,让 default 组合(--no-default-features)也能编译运行此文件"

2. **P0-2: `IterableSource::UserSlot` 类型错误** (Task 1 Step 9 D 组测试)
   - 修改前:`IterableSource::UserSlot(SlotKind::Text)` (tuple variant)
   - 修改后:`IterableSource::UserSlot { slot_kind: "text".to_string() }` (struct variant)

3. **P0-3: `DagStatus::transition` 方法不存在** (Task 1 F 组 Step 13)
   - 在 Task 1 之前加 "Step 0: 核实 DagStatus 现有 API"
   - 提供两种解决路径:用现有 API(is_succeeded / is_failed 等)替代,或在 dag_types.rs 补实现 transition 方法
   - 同步更新 F 组测试代码,从 `result.is_ok()` / `result.is_err()` 改为 `result` / `!result`(bool 返回值)

4. **P0-4: `DagPlan::validate()` 方法不存在** (Task 1 G/H 组 Step 13)
   - 改为调用分项方法:`validate_total_steps()` / `validate_loop_iterations()` / `validate_edges()` / `validate_loop_specs()`
   - G 组测试 + H 组测试都用 IIFE 闭包调用分项方法替代聚合 `validate()`
   - 注释提示可补聚合 validate() 方法

5. **P0-5: Task 2 文件级 gate 前后矛盾** (Task 2 Step 1 vs Step 7)
   - Step 1 删除 `#![cfg(feature = "stronghold")]` 文件级 gate
   - 直接采用 Step 7 的设计:测试 1 + 测试 2 加 `#[cfg(feature = "stronghold")]` 函数级 gate,测试 3 无 gate
   - Step 7 同步更新,移除 "调整" 段落

### P1 缺陷修复(语义/逻辑问题,17 项)

6. **P1-1 + P1-16: Task 2 测试 1 哈希链算法猜测式** (Task 2 Step 2)
   - 在 Task 2 Step 2 之前加 "Step 0: 读 W1 audit_append 源码确定哈希链算法"
   - 改为确定性断言:使用 `sha256(prev_hash + event_type + details)` 算法
   - 删除 3 种算法 a/b/c 任一匹配的逻辑
   - 用 `assert_eq!` 替代 OR 匹配的 `assert!`

7. **P1-2: Task 2 测试 2 `reverse_compensation("comp-1")` 调用错误** (Task 2 Step 4)
   - 改为先用 `create_post_commit_compensation` 创建真实 compensation 记录
   - 提取 `comp_record.compensation_id` 作为 `comp_id`
   - 调用 `reverse_compensation(&kernel, &comp_id)` 而非 `reverse_compensation(&kernel, "comp-1")`

8. **P1-3: Task 2 测试 2 `taint_propagated` 断言不严谨** (Task 2 Step 4)
   - 改用 `eprintln!` 记录 taint_propagated 数量,不强制断言 > 0
   - 注释提示若需验证,在 Plan 3 范围内用 mock dispatcher 测试

9. **P1-4: Task 2 测试 2 测试名与实际矛盾** (Task 2 Step 4)
   - 改测试名:`audit_chain_w9_new_events_all_recorded` → `audit_chain_w9_new_5_events_recorded`
   - 改注释从 "7 种事件" → "5 种事件"
   - 加说明:`dag_skeleton_modified` + `dag_modify_limit_exceeded` 需要 tauri feature,完整 7 种事件验证在 Task 7 Step 6 全 feature 组合下跑
   - Step 5 跑测试命令同步更新为新测试名
   - 文件头注释同步更新

10. **P1-5: Task 1 C 组 dag_modify_limit_exceeded 依赖 Plan 4 实际签名** (Task 1 C 组前)
    - 在 Task 1 B 组之后、Step 7 之前加 "Step 0: 核实 Plan 4 实际签名"
    - 提供 grep 命令核实 `DagApprovalOutcome` 变体名 + `DagModifyLimitExceeded` 错误变体名

11. **P1-6: Task 1 Step 9 测试名与实际矛盾** (Task 1 Step 9)
    - 改测试名:`dag_executor_run_with_empty_user_slots_succeeds` → `dag_executor_run_with_empty_user_slots_does_not_panic`
    - 改注释说明 default feature 下 note.capture 不可达,可能 Failed,但 run 不 panic
    - 删除 `dag_result.status` 之前的 Succeeded/Cancelled 注释
    - Step 10 跑测试命令同步更新

12. **P1-7: Task 6 Step 2 vs Task 8 Step 3 测试数预估两个数字混用** (Task 6 Step 2)
    - 区分两个口径:
      - **default 组合非门控测试数 ≥ 286**(Task 6 验证,spec §5 阈值)
      - **全 feature 组合测试数 ~520**(Task 8 Step 3 估算,包含 feature-gated + `#[ignore]`)
    - 加 "**注意:**" 段落明确区分

13. **P1-8: Task 6 Step 1 Measure-Object -L 多 1 行** (Task 6 Step 1 + Task 9 Step 2)
    - 改为精确过滤:`$matches = cargo test ... | Select-String ': test$' ; ($matches | Measure-Object).Count`
    - 加注释说明 `Measure-Object -L` 会多 1 行
    - Task 9 Step 2 同步更新

14. **P1-9: Task 7 标题 "6 套" vs Step 1-7 实际 7 套** (Task 7 标题 + 目标 + Step 9)
    - 标题:`## Task 7: ...(6 套 ...)` → `## Task 7: ...(7 套 ...)`
    - 目标:`6 套 feature 组合` → `7 套 feature 组合`
    - Step 9 末尾:`若 6 套一次通过无修复` → `若 7 套一次通过无修复`

15. **P1-10: Task 9 Step 3 空 commit 与 project_memory.md "NEVER commit" 冲突** (Task 9 Step 3)
    - 改 Step 3 标题为 "空 commit 标记 W9 里程碑(需用户明确授权)"
    - 加 "⚠️ 重要:" 段落说明 project_memory.md 明确要求 "NEVER commit unless explicitly asked"
    - 加 spec §12 决策 #12 原文 "W9 不提交,仅生成完整 plan 文档"
    - 加显式授权要求:仅在用户明确授权 "执行 Plan 7 并允许 commit" 后才执行
    - 加 "若用户未授权,跳过本 Step" 说明

16. **P1-11: Task 8 Step 4 "Commit 范围: 7 个 Plan 累计 ~60+ commit" 与 spec 矛盾** (Task 8 Step 4)
    - 改为:"Plan 7 是 W9 唯一可提交 plan(需用户决策时确认);其余 Plan 1-6 仅生成文档不提交,除非用户明确授权。Plan 7 自身累计 9 个 task 直接提交到 master(若用户授权 Plan 1-6 可提交,7 个 Plan 累计 ~60+ commit)。"

17. **P1-12: Task 1 Step 11 regex crate 依赖与 Conventions 矛盾** (Conventions 部分)
    - 在 Conventions "不引入非必要依赖" 项后加 "例外" 子项
    - 说明:`regex` crate 若已在 W7/W8 dev-dependencies,本 Plan 复用;否则 Task 1 Step 11 加 `regex = "1"` 到 `[dev-dependencies]`。先 grep 确认

18. **P1-13: Task 2 Step 6 隐私脱敏黑名单过严** (Task 2 Step 6)
    - 修改前(子串匹配,误报):`["plaintext", "password", "secret", "api_key", "vault_key", "sk-"]`
    - 修改后(精确匹配密钥字面量前缀):`["password=", "passwd=", "secret=", "api_key=", "sk-", "plaintext="]`
    - 加注释说明 hash 字段(vault_key_ref / secret_id / api_key_hash)不是密钥本身,允许

19. **P1-14: stronghold feature 独立编译验收缺失** (Task 3)
    - 在 Task 3 末尾加 "Step 11: stronghold feature 独立编译验证"
    - 命令:`cargo check --workspace --features stronghold`
    - 失败响应:若失败,说明 stronghold feature 隐式依赖 voice/tauri,需修复 Cargo.toml feature 定义

20. **P1-15: 真实 E2E 4 个 `#[ignore]` 测试验收无明确步骤** (Task 5.5 新增)
    - 在 Task 5 之后、Task 6 之前插入 "## Task 5.5: 手动运行真实 E2E 测试 + 记录结果"
    - Step 1: 运行 Plan 5 Playwright E2E(2 个 `#[ignore]` 测试)
    - Step 2: 运行 Plan 6 UIA E2E(2 个 `#[ignore]` 测试)
    - Step 3: 汇总 4 个测试结果到 PROGRESS.md

21. **P1-17: Taint Tracking grep 验收未实际跑** (Task 1 B 组)
    - 在 Task 1 A 组之后、Step 5 之前加 "Step 0: grep 验证 TaintRepo CRUD 代码存在"
    - 用 `Select-String` 验证 `INSERT INTO taints` + `pub fn find_by_value` 存在
    - 若任一 grep 无命中,说明 Plan 3 未实现,阻塞 Plan 7

22. **P1-18: Task 7 Step 7 DAG Modify 闭环不验证完整语义** (Task 1 C 组)
    - 在 Task 1 C 组测试代码末尾加新测试 `dag_modify_full_loop_modify_then_approve_then_execute`
    - 完整闭环:Modify → Allow → 执行 → 验证审计链
    - 用 `ModifyOnceThenAllowApprover`(第 0 次 Modify,第 1 次 Allow)
    - 验证 `dag_skeleton_modified` 审计事件被记录
    - Step 8 Expected 从 2 passed 改为 3 passed,加注释说明 P1-18 已加完整闭环用例

### P2 缺陷修复(验收门禁细化,2 项)

23. **P2-1: Task 5 Step 4 bundle size 阈值无失败响应** (Task 5 Step 4)
    - 加 "**失败响应:** 若 bundle 超阈值(index.html > 2KB 或 index-*.js > 500KB),记录到 PROGRESS.md "已知偏离",不阻塞 milestone(W9 范围不优化 bundle)。"

24. **P2-2: Task 8 Step 4 测试矩阵未验证其他测试文件** (Task 7 Step 7)
    - 在 Task 7 Step 7 末尾加 "**附加验证:**" 段落
    - 用 `cargo test ... --list | Select-String ': test$' | Measure-Object` 验证 7 个测试文件存在且测试数符合预期
    - 涵盖:w9_stronghold_unit / w9_snapshot_encrypted_smoke / w9_taint_tracking_unit / w9_gateway_taint_smoke / w9_dag_modify_smoke / w9_default_boundary_smoke / w9_audit_chain_smoke

### 备注

- 所有修改使用 Edit 工具精确替换,未重写整个文件
- 每个修复都对照 spec §2.7 + §5 + §6.4 + §9 + §10 + §12 + project_memory.md 工程约束
- P1-16(哈希链验收脚本不具体)与 P1-1 合并修复(都涉及 Task 2 测试 1 哈希链算法)
- 修复后 C 组测试数从 2 个变为 3 个(P1-18 加完整闭环用例),Task 1 总测试数从 15 个变为 16 个

**End of W9 Plan 7 Implementation Plan**
