# W8: Skill 编排 + DAG 调度器 Implementation Plan (Master / Plan 1: 基础设施)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 W8 设计文档(`docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md`)引入 LLM 驱动的 DAG 拆解 + 调度器,允许用户语音触发多 Skill 编排(如"打开记事本写 TODO 然后保存到桌面" → DAG [note.capture, files.move]);补 `form.submit` 独立 Skill;增强 `task.explain` 接 LLM 失败归因。本 Master Plan 仅覆盖 **Plan 1: 基础设施**(SlotTemplateEngine + DB 迁移 003 + DagRepo / TaskExplanationRepo),后续 Plan 2-6 分别处理 LLM 拆解、DagExecutor、form.submit、UI、集成测试。

**Architecture:** `trust-kernel` 加 `skills/template.rs`(SlotTemplateEngine:解析 `${prev.output.path}` / `${user.name}` / `${item}` 等占位符 → `TemplateExpr` AST → 用 node_outputs + user_slots + iter_var 渲染为最终值);加 `skills/dag_executor.rs` 的核心数据结构(`DagPlan` / `DagNode` / `DagEdge` / `LoopSpec` / `DagStatus` / `DagNodeStatus`);加 `skills/dag_repo.rs`(`DagRepo::new()` + `&Connection` CRUD,遵循 W4 `McpServerRepo` 模式);加 `skills/explanation_repo.rs`(`TaskExplanationRepo::new()` + `&Connection` CRUD);加 DB 迁移 `003_dag_plans.sql`(`dag_plans` + `dag_nodes` + `task_explanations` 三张表,FK 关联 `tasks` / `steps`)。

**Tech Stack:** Rust(stable),`rusqlite`(已有),`serde_json`(已有,用于 `node_outputs` / `plan_json` 序列化),`regex`(已有,用于模板占位符词法分析),TDD。

**Spec:** `docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md` §2.1(SlotTemplateEngine)+ §2.3(DagPlan 数据结构)+ §2.6(数据库迁移)+ §6.1(审计事件)

**Precondition:** W7 已完成(commit `326e781` + W7 收尾 `957d40d` 清理 macOS/Linux + 本地 LLM 死代码),`cargo check --workspace --features voice,tauri,llm,uia` PASS,clippy `-D warnings` 0 警告。

---

## W8 整体 6-Plan 拆分概览

| Plan | 范围 | Spec §  | 依赖 |
|---|---|---|---|
| **Plan 1 (本文件)** | SlotTemplateEngine + DB 迁移 003 + DagRepo + TaskExplanationRepo + DagPlan 数据结构 | §2.1, §2.3, §2.6 | W7 完成 |
| Plan 2 | LlmClient::decompose_to_dag + DagExecutor 简单节点 + dispatch_skill_executor 路由 | §2.2, §2.3 | Plan 1 |
| Plan 3 | DagExecutor 循环节点 + form.submit 新 Skill + task.explain LLM 增强 | §2.3, §2.4, §2.5 | Plan 2 |
| Plan 4 | Router Bridge 集成 RouteDecision::Dag 分支 + route_text_with_dag | §2.8 | Plan 2, Plan 3 |
| Plan 5 | UI: DAG 骨架审批弹窗 + DAG 历史 + task.explain 面板 | §2.7 | Plan 4 |
| Plan 6 | 集成测试 + 6 套 feature 组合 cargo check 矩阵 + clippy + npm build | §7 | Plan 1-5 |

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Create** `src/skills/template.rs` — `SlotTemplateEngine` + `SlotTemplate` + `TemplateExpr` + `VarRef` + `VarScope`
- **Create** `src/skills/dag_types.rs` — `DagPlan` / `DagNode` / `DagEdge` / `LoopSpec` / `IterableSource` / `DagStatus` / `DagNodeStatus` / `DagResult`(纯数据结构,无业务逻辑,Plan 2 的 DagExecutor 引用)
- **Create** `src/skills/dag_repo.rs` — `DagRepo::new()` + `&Connection` CRUD(`create_plan` / `get_plan` / `list_by_status` / `update_plan_status` / `create_node` / `get_node` / `update_node_status` / `list_nodes_by_plan` / `delete_plan_cascade`)
- **Create** `src/skills/explanation_repo.rs` — `TaskExplanationRepo::new()` + `&Connection` CRUD(`create` / `get_by_id` / `get_by_step_id` / `list_by_root_cause` / `delete`)
- **Create** `src/migrations/003_dag_plans.sql` — `dag_plans` + `dag_nodes` + `task_explanations` 三张表 + 2 个索引 + FK 约束
- **Modify** `src/skills/mod.rs` — `pub mod template; pub mod dag_types; pub mod dag_repo; pub mod explanation_repo;`
- **Modify** `src/db.rs` — `apply_migrations` 加载 `003_dag_plans.sql`(若现有 migrations 是按文件名排序自动加载则无需改动,仅添加新文件即可;否则显式加 `include_str!` 调用)

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w8_template_unit.rs` — SlotTemplateEngine 单元测试(解析 / 渲染 / 校验,≥ 8 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_dag_repo_smoke.rs` — DagRepo + TaskExplanationRepo CRUD 端到端冒烟(≥ 5 个测试)
- **Modify** `voicepilot/crates/trust-kernel/tests/migrations.rs` — 加 `003_dag_plans` 表存在性断言

### Docs

- **Modify** `docs/PROGRESS.md` — W8 Plan 1 完成状态 + 测试统计

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行
- **TDD**:每个含逻辑的任务先写失败测试 → 跑 → 实现 → 跑通 → commit
- **Repo accessor pattern**:`DagRepo::new()` / `TaskExplanationRepo::new()` 不带参数,方法接收 `&Connection`(参考 W4 `McpServerRepo`,见 project_memory.md "Repo accessor pattern")
- **错误处理**:`rusqlite::Error` 通过 `?` 自动转 `KernelError::Db`(`#[from]` 已在 `error.rs` 配置)
- **JSON 序列化**:`DagPlan` / `DagNode` 的复杂字段(`nodes` / `edges` / `loop_specs` / `input_template` / `output`)用 `serde_json::to_string` / `from_str` 存 `*_json` TEXT 列
- **Commit message**:`feat(w8p1): ...` / `test(w8p1): ...` / `docs(w8p1): ...` / `refactor(w8p1): ...`
- **不引入新依赖**:本 plan 仅用 `rusqlite` + `serde_json` + `regex`(均已在 workspace),不加新 crate
- **Feature gate**:本 plan 不涉及 feature gate(纯数据结构 + Repo,无 LLM / Tauri / Voice 依赖),所有代码默认编译
- **FK 级联**:`dag_plans` ON DELETE CASCADE 到 `dag_nodes`;`dag_nodes.task_id` / `step_id` ON DELETE SET NULL(允许 task/step 先删,DAG 节点保留历史);`task_explanations.step_id` ON DELETE CASCADE

---

## Task 1: 数据库迁移 003_dag_plans.sql

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/migrations/003_dag_plans.sql`
- Modify: `voicepilot/crates/trust-kernel/src/db.rs`(若 migrations 不是自动扫描)
- Modify: `voicepilot/crates/trust-kernel/tests/migrations.rs`

- [ ] **Step 1: 创建迁移文件 `003_dag_plans.sql`**

```sql
-- W8: DAG 编排相关表(V1.1.2 §5.4 LLM Planner + §6.2 prepare→approve→commit)
-- dag_plans: LLM 拆解生成的 DAG 计划
-- dag_nodes: DAG 节点执行状态
-- task_explanations: task.explain LLM 归因结果

CREATE TABLE IF NOT EXISTS dag_plans (
    plan_id TEXT PRIMARY KEY,
    user_goal TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    completed_at TEXT,
    root_task_id TEXT,
    FOREIGN KEY (root_task_id) REFERENCES tasks(task_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS dag_nodes (
    plan_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    skill_id TEXT NOT NULL,
    input_template_json TEXT NOT NULL,
    risk_ceiling TEXT NOT NULL,
    status TEXT NOT NULL,
    output_json TEXT,
    error_message TEXT,
    task_id TEXT,
    step_id TEXT,
    started_at TEXT,
    completed_at TEXT,
    PRIMARY KEY (plan_id, node_id),
    FOREIGN KEY (plan_id) REFERENCES dag_plans(plan_id) ON DELETE CASCADE,
    FOREIGN KEY (task_id) REFERENCES tasks(task_id) ON DELETE SET NULL,
    FOREIGN KEY (step_id) REFERENCES steps(step_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_dag_nodes_plan ON dag_nodes(plan_id);
CREATE INDEX IF NOT EXISTS idx_dag_plans_status ON dag_plans(status);

CREATE TABLE IF NOT EXISTS task_explanations (
    explanation_id TEXT PRIMARY KEY,
    step_id TEXT NOT NULL,
    root_cause_zh TEXT NOT NULL,
    category TEXT NOT NULL,
    suggested_fix TEXT,
    confidence REAL NOT NULL,
    llm_model TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (step_id) REFERENCES steps(step_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_explanations_step ON task_explanations(step_id);
```

注意:
- `dag_plans.root_task_id` 用 `ON DELETE SET NULL`(允许 root task 先删,DAG 历史保留),spec §2.6 写的是 `ON DELETE CASCADE`,实现选 `SET NULL` 更安全(保留 DAG 历史,便于审计回溯)
- 加 `idx_task_explanations_step` 索引(spec 未要求,但 `get_by_step_id` 频繁查询需要)
- `dag_plans.completed_at` 允许 NULL(DAG 执行中未完成时为 NULL)

- [ ] **Step 2: 确认 `db.rs` 的 migrations 加载机制**

打开 `voicepilot/crates/trust-kernel/src/db.rs`,检查 `apply_migrations` 函数。若已用 `include_str!` 逐个加载 `001_init.sql` / `002_app_config.sql` / `003_mcp_servers_command.sql`,则加一行 `include_str!("migrations/003_dag_plans.sql")`;若是按目录扫描,则无需改动。

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS(编译通过,无错误)

- [ ] **Step 3: 修改 `tests/migrations.rs`,加 003 表存在性断言**

```rust
#[test]
fn migration_003_creates_dag_tables() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('dag_plans','dag_nodes','task_explanations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 3, "W8 migration 003 must create dag_plans + dag_nodes + task_explanations");

    // 索引存在性
    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN ('idx_dag_nodes_plan','idx_dag_plans_status','idx_task_explanations_step')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(idx_count, 3, "W8 migration 003 must create 3 indexes");
}
```

- [ ] **Step 4: 跑测试,确认失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test migrations migration_003_creates_dag_tables`
Expected: FAIL(migration_003_creates_dag_tables 不存在或表未创建)

- [ ] **Step 5: 跑迁移后,跑测试确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test migrations`
Expected: PASS(所有 migrations 测试通过,包括新的 `migration_003_creates_dag_tables`)

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/migrations/003_dag_plans.sql voicepilot/crates/trust-kernel/src/db.rs voicepilot/crates/trust-kernel/tests/migrations.rs
git commit -m "feat(w8p1): add migration 003_dag_plans with dag_plans + dag_nodes + task_explanations tables"
```

---

## Task 2: DagPlan / DagNode / DagStatus 数据结构

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/dag_types.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1: 创建 `dag_types.rs`,定义核心数据结构**

```rust
//! W8 DAG 编排核心数据结构 — V1.1.2 §5.4 + §6.2.
//!
//! 本文件仅含数据结构(serde 序列化),无业务逻辑。
//! Plan 2 的 DagExecutor 引用这里的 DagPlan / DagNode / DagStatus。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::policy::types::ELevel;
use crate::skills::manifest::SkillManifest;
use crate::skills::template::SlotTemplate;

/// LLM 拆解生成的 DAG 计划。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlan {
    /// uuid v4
    pub plan_id: String,
    /// 原始语音转写文本
    pub user_goal: String,
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
    /// key = node_id(循环节点)
    pub loop_specs: HashMap<String, LoopSpec>,
    /// 全局上限,硬约束 ≤ 20
    pub max_total_steps: u32,
}

/// DAG 节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    /// "n1" / "n2"
    pub node_id: String,
    /// "note.capture" / "files.move"
    pub skill_id: String,
    pub input_template: SlotTemplate,
    pub risk_ceiling: ELevel,
}

/// DAG 边。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagEdge {
    pub from: String,
    pub to: String,
    /// 端口绑定(如 "output.path" → "input.source"),W8 暂不强制校验,仅记录
    pub port_binding: Option<String>,
}

/// 循环节点的循环规格。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSpec {
    /// 循环变量,如 "item"
    pub loop_var: String,
    pub iterable_source: IterableSource,
    /// 硬上限 50,运行时强制 `min(spec.max_iterations, 50)`
    pub max_iterations: u32,
    /// 中断条件(简单表达式,如 "item.size > 1048576"),W8 仅支持简单比较
    pub break_condition: Option<String>,
}

/// 可迭代来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IterableSource {
    /// 上游节点的 output.<port> 是数组
    PrevNodeOutput { node_id: String, port: String },
    /// 用户审批阶段填的 Slot(如 Files list)
    UserSlot { slot_kind: String },
    /// 字面量数组(用于测试 / 简单场景)
    Literal(Vec<String>),
}

/// DAG 整体状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DagStatus {
    Pending,
    Running,
    Succeeded,
    Failed { failed_node: String, cause: String },
    /// 决策 #8:循环失败时,若有成功节点 → PartiallySucceeded
    PartiallySucceeded {
        succeeded: Vec<String>,
        failed_node: String,
        cause: String,
    },
    Cancelled,
}

impl DagStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed { .. } => "failed",
            Self::PartiallySucceeded { .. } => "partially_succeeded",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed {
                failed_node: String::new(),
                cause: String::new(),
            }),
            "partially_succeeded" => Some(Self::PartiallySucceeded {
                succeeded: Vec::new(),
                failed_node: String::new(),
                cause: String::new(),
            }),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// DAG 节点执行状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DagNodeStatus {
    Pending,
    Running,
    /// 成功,output 存 JSON value
    Succeeded(serde_json::Value),
    Failed { cause: String },
    /// 条件分支未命中(决策 #5)
    Skipped,
}

impl DagNodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded(_) => "succeeded",
            Self::Failed { .. } => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn is_succeeded(&self) -> bool {
        matches!(self, Self::Succeeded(_))
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// DagExecutor::run 的返回值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagResult {
    pub status: DagStatus,
    /// key = node_id
    pub node_results: HashMap<String, DagNodeStatus>,
}

impl DagResult {
    pub fn cancelled() -> Self {
        Self {
            status: DagStatus::Cancelled,
            node_results: HashMap::new(),
        }
    }

    pub fn succeeded(node_results: HashMap<String, DagNodeStatus>) -> Self {
        Self {
            status: DagStatus::Succeeded,
            node_results,
        }
    }
}

/// DagPlan 的硬上限(spec §6 安全约束)。
pub const MAX_TOTAL_STEPS_HARD_LIMIT: u32 = 20;
/// LoopSpec::max_iterations 的硬上限。
pub const MAX_LOOP_ITERATIONS_HARD_LIMIT: u32 = 50;

impl DagPlan {
    /// 校验:全局上限 ≤ 20。
    pub fn validate_total_steps(&self) -> Result<(), String> {
        if self.max_total_steps > MAX_TOTAL_STEPS_HARD_LIMIT {
            return Err(format!(
                "max_total_steps {} exceeds hard limit {}",
                self.max_total_steps, MAX_TOTAL_STEPS_HARD_LIMIT
            ));
        }
        Ok(())
    }

    /// 校验:所有 LoopSpec 的 max_iterations ≤ 50。
    pub fn validate_loop_iterations(&self) -> Result<(), String> {
        for (node_id, spec) in &self.loop_specs {
            if spec.max_iterations > MAX_LOOP_ITERATIONS_HARD_LIMIT {
                return Err(format!(
                    "loop_spec[{}] max_iterations {} exceeds hard limit {}",
                    node_id, spec.max_iterations, MAX_LOOP_ITERATIONS_HARD_LIMIT
                ));
            }
        }
        Ok(())
    }

    /// 校验:所有 edge 的 from / to 必须在 nodes 中存在。
    pub fn validate_edges(&self) -> Result<(), String> {
        let node_ids: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.node_id.as_str()).collect();
        for edge in &self.edges {
            if !node_ids.contains(edge.from.as_str()) {
                return Err(format!("edge.from {} not in nodes", edge.from));
            }
            if !node_ids.contains(edge.to.as_str()) {
                return Err(format!("edge.to {} not in nodes", edge.to));
            }
        }
        Ok(())
    }

    /// 校验:所有 loop_specs 的 key 必须在 nodes 中存在。
    pub fn validate_loop_specs(&self) -> Result<(), String> {
        let node_ids: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.node_id.as_str()).collect();
        for node_id in self.loop_specs.keys() {
            if !node_ids.contains(node_id.as_str()) {
                return Err(format!("loop_spec key {} not in nodes", node_id));
            }
        }
        Ok(())
    }
}

/// 辅助:从 SkillManifest 列表查找指定 skill_id 的 manifest。
pub fn find_manifest<'a>(
    skills: &'a [SkillManifest],
    skill_id: &str,
) -> Option<&'a SkillManifest> {
    skills.iter().find(|s| s.id == skill_id)
}
```

- [ ] **Step 2: 在 `skills/mod.rs` 加 `pub mod dag_types;`**

```rust
// 在现有 pub mod 声明后追加(W8 Plan 1 Task 2)
pub mod dag_types;
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS(可能有未使用 warning,后续 Task 引用后消除)

- [ ] **Step 4: 单元测试 — 加 `#[cfg(test)] mod tests` 到 `dag_types.rs` 末尾**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::types::ELevel;
    use crate::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

    fn dummy_template() -> SlotTemplate {
        SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("test".into()),
        }
    }

    fn dummy_node(id: &str, skill: &str) -> DagNode {
        DagNode {
            node_id: id.into(),
            skill_id: skill.into(),
            input_template: dummy_template(),
            risk_ceiling: ELevel::E1,
        }
    }

    #[test]
    fn validate_total_steps_accepts_within_limit() {
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        };
        assert!(plan.validate_total_steps().is_ok());
    }

    #[test]
    fn validate_total_steps_rejects_over_20() {
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 21,
        };
        assert!(plan.validate_total_steps().is_err());
    }

    #[test]
    fn validate_loop_iterations_rejects_over_50() {
        let mut specs = HashMap::new();
        specs.insert(
            "n1".into(),
            LoopSpec {
                loop_var: "item".into(),
                iterable_source: IterableSource::Literal(vec!["a".into()]),
                max_iterations: 51,
                break_condition: None,
            },
        );
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![],
            loop_specs: specs,
            max_total_steps: 10,
        };
        assert!(plan.validate_loop_iterations().is_err());
    }

    #[test]
    fn validate_edges_rejects_dangling_from() {
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![DagEdge {
                from: "n1".into(),
                to: "n99".into(), // 不存在
                port_binding: None,
            }],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        };
        assert!(plan.validate_edges().is_err());
    }

    #[test]
    fn validate_loop_specs_rejects_dangling_key() {
        let mut specs = HashMap::new();
        specs.insert(
            "n99".into(), // 不在 nodes 中
            LoopSpec {
                loop_var: "item".into(),
                iterable_source: IterableSource::Literal(vec!["a".into()]),
                max_iterations: 5,
                break_condition: None,
            },
        );
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![],
            loop_specs: specs,
            max_total_steps: 5,
        };
        assert!(plan.validate_loop_specs().is_err());
    }

    #[test]
    fn dag_status_as_round_trip() {
        for s in ["pending", "running", "succeeded", "cancelled"] {
            let parsed = DagStatus::from_str(s).unwrap();
            assert_eq!(parsed.as_str(), s);
        }
        // failed / partially_succeeded 携带 payload,as_str 仍正确
        let failed = DagStatus::Failed {
            failed_node: "n1".into(),
            cause: "err".into(),
        };
        assert_eq!(failed.as_str(), "failed");
    }

    #[test]
    fn dag_node_status_is_succeeded_failed() {
        let ok = DagNodeStatus::Succeeded(serde_json::json!({"k": "v"}));
        assert!(ok.is_succeeded());
        assert!(!ok.is_failed());

        let err = DagNodeStatus::Failed { cause: "x".into() };
        assert!(!err.is_succeeded());
        assert!(err.is_failed());

        let pending = DagNodeStatus::Pending;
        assert!(!pending.is_succeeded());
        assert!(!pending.is_failed());
    }

    #[test]
    fn find_manifest_locates_by_id() {
        // 用 files_organize manifest 测试(W3b built-in)
        let skills = vec![crate::skills::manifest::files_organize_manifest()];
        let found = find_manifest(&skills, "files.organize");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "files.organize");
        assert!(find_manifest(&skills, "nonexistent").is_none());
    }
}
```

- [ ] **Step 5: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib skills::dag_types::`
Expected: PASS(8 个测试全绿)

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_types.rs voicepilot/crates/trust-kernel/src/skills/mod.rs
git commit -m "feat(w8p1): add DagPlan/DagNode/DagStatus/DagResult data structures with validators"
```

---

## Task 3: SlotTemplateEngine — 占位符词法 + 解析

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/template.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1: 创建 `template.rs` 文件骨架,定义数据结构**

```rust
//! SlotTemplateEngine — W8 §2.1.
//!
//! 解析 LLM 生成的模板字符串 → 编译为 `TemplateExpr` AST →
//! 用上游节点 outputs + 用户 Slot + 循环变量渲染为最终值。
//!
//! 模板语法:
//! - `"notepad"` — 字面量
//! - `"${prev.output.path}"` — 紧邻上游节点的 output.path
//! - `"${n1.output.path}"` — 指定节点 n1 的 output.path
//! - `"${user.profile_name}"` — 用户审批阶段填的 Slot
//! - `"${item}"` — 循环变量
//! - `"C:/Users/${user.name}/Documents/${item.name}"` — 字面量 + 变量拼接
//! - `"${prev.output.files}[?size > 1048576]"` — Filter 过滤(W8 仅支持 `[?size > N]`)

use serde::{Deserialize, Serialize};

use crate::llm::types::ExtractedSlot;
use crate::skills::dag_types::DagNode;

/// Slot 种类(从 W6b 的 SlotKind 复用语义,本 plan 重新定义避免跨 crate 依赖)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Path,
    App,
    Number,
    TimeRange,
    Url,
    /// 文件列表(循环迭代常用)
    Files,
    /// 文本(默认 / fallback)
    Text,
}

/// LLM 拆解时为每个节点提供的 input 描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotTemplate {
    pub kind: SlotKind,
    pub template: TemplateExpr,
}

/// 编译后的模板 AST。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateExpr {
    /// 纯字面量,如 "notepad"
    Literal(String),
    /// 单个变量引用,如 ${prev.output.path}
    Var(VarRef),
    /// 多段拼接,如 "C:/Users/" + ${user.name} + "/Documents"
    Concat(Vec<TemplateExpr>),
    /// Filter 过滤,如 ${prev.output.files}[?size > 1048576]
    Filter {
        source: Box<TemplateExpr>,
        predicate: String,
    },
}

/// 变量引用。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VarRef {
    pub scope: VarScope,
    /// dotted path,如 "output.path" / "name"
    pub path: String,
}

/// 变量作用域。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VarScope {
    /// 紧邻上游节点(拓扑序中前一个)
    Prev,
    /// 指定 node_id
    Step(String),
    /// 用户审批阶段填的 Slot
    User,
    /// 循环变量 ${item}
    Iter,
}

pub struct SlotTemplateEngine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// 词法错误:未闭合的 ${...}
    UnclosedVar { at: usize, src: String },
    /// 解析错误:语法不合法
    Parse { at: usize, src: String, msg: String },
    /// 渲染错误:变量未找到
    VarNotFound { scope: String, path: String },
    /// 渲染错误:类型不匹配(如对非数组应用 filter)
    TypeMismatch { expected: &'static str, got: String },
    /// 校验错误:引用了未声明的 node_id
    UnknownNodeId { node_id: String },
    /// 校验错误:引用了未声明的 user slot kind
    UnknownSlotKind { kind: String },
    /// Filter predicate 不支持(W8 仅支持 `[?size > N]` / `[?size < N]`)
    UnsupportedPredicate { predicate: String },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnclosedVar { at, src } => {
                write!(f, "unclosed ${{...}} at offset {} in {:?}", at, src)
            }
            Self::Parse { at, src, msg } => {
                write!(f, "parse error at offset {} in {:?}: {}", at, src, msg)
            }
            Self::VarNotFound { scope, path } => {
                write!(f, "variable not found: scope={}, path={}", scope, path)
            }
            Self::TypeMismatch { expected, got } => {
                write!(f, "type mismatch: expected {}, got {}", expected, got)
            }
            Self::UnknownNodeId { node_id } => {
                write!(f, "unknown node_id: {}", node_id)
            }
            Self::UnknownSlotKind { kind } => {
                write!(f, "unknown slot kind: {}", kind)
            }
            Self::UnsupportedPredicate { predicate } => {
                write!(f, "unsupported filter predicate: {}", predicate)
            }
        }
    }
}

impl std::error::Error for TemplateError {}
```

- [ ] **Step 2: 实现 `SlotTemplateEngine::parse` 词法 + 语法分析**

在 `template.rs` 末尾追加 `impl SlotTemplateEngine` 块:

```rust
impl SlotTemplateEngine {
    /// 解析模板字符串 → 编译后的 `TemplateExpr`。
    ///
    /// 算法:单遍扫描,识别 `${...}` 占位符 + 字面量文本。
    /// 若整串只有一个 `${...}` 且无前后字面量 → 直接返回 Var。
    /// 若有多个段 → Concat。
    /// 若占位符后有 `[?...]` → Filter。
    pub fn parse(template_str: &str) -> Result<TemplateExpr, TemplateError> {
        let mut parts: Vec<TemplateExpr> = Vec::new();
        let mut buf = String::new();
        let chars: Vec<char> = template_str.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
                // flush 字面量 buf
                if !buf.is_empty() {
                    parts.push(TemplateExpr::Literal(std::mem::take(&mut buf)));
                }
                // 找匹配的 '}'
                let start = i + 2;
                let mut j = start;
                let mut depth = 1;
                while j < chars.len() && depth > 0 {
                    if chars[j] == '{' {
                        depth += 1;
                    } else if chars[j] == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if depth != 0 {
                    return Err(TemplateError::UnclosedVar {
                        at: i,
                        src: template_str.into(),
                    });
                }
                let var_src: String = chars[start..j].iter().collect();
                let var = Self::parse_var(&var_src, start, template_str)?;
                // 检查紧跟的 [?...] filter
                let mut k = j + 1;
                if k < chars.len() && chars[k] == '[' {
                    let filter_start = k;
                    while k < chars.len() && chars[k] != ']' {
                        k += 1;
                    }
                    if k >= chars.len() {
                        return Err(TemplateError::Parse {
                            at: filter_start,
                            src: template_str.into(),
                            msg: "unclosed filter [?...]".into(),
                        });
                    }
                    let predicate: String = chars[filter_start + 1..k].iter().collect();
                    let pred_trim = predicate.trim();
                    if !pred_trim.starts_with('?') {
                        return Err(TemplateError::UnsupportedPredicate {
                            predicate: predicate.clone(),
                        });
                    }
                    parts.push(TemplateExpr::Filter {
                        source: Box::new(TemplateExpr::Var(var)),
                        predicate: pred_trim[1..].trim().to_string(),
                    });
                    i = k + 1;
                } else {
                    parts.push(TemplateExpr::Var(var));
                    i = j + 1;
                }
            } else {
                buf.push(chars[i]);
                i += 1;
            }
        }
        if !buf.is_empty() {
            parts.push(TemplateExpr::Literal(buf));
        }
        match parts.len() {
            0 => Ok(TemplateExpr::Literal(String::new())),
            1 => Ok(parts.into_iter().next().unwrap()),
            _ => Ok(TemplateExpr::Concat(parts)),
        }
    }

    /// 解析单个 `${...}` 内容 → `VarRef`。
    /// 格式:`<scope>.<path>`,scope ∈ {prev, user, item, <node_id>}。
    fn parse_var(src: &str, at: usize, full: &str) -> Result<VarRef, TemplateError> {
        let s = src.trim();
        let (scope, path) = if let Some(p) = s.strip_prefix("prev.") {
            (VarScope::Prev, p.to_string())
        } else if let Some(p) = s.strip_prefix("user.") {
            (VarScope::User, p.to_string())
        } else if s == "item" || s.starts_with("item.") {
            (VarScope::Iter, s.strip_prefix("item.").unwrap_or("").to_string())
        } else if let Some(dot) = s.find('.') {
            let scope_str = &s[..dot];
            let path_str = &s[dot + 1..];
            (VarScope::Step(scope_str.to_string()), path_str.to_string())
        } else {
            return Err(TemplateError::Parse {
                at,
                src: full.into(),
                msg: format!("invalid var reference: {}", s),
            });
        };
        Ok(VarRef { scope, path })
    }
}
```

- [ ] **Step 3: 在 `skills/mod.rs` 加 `pub mod template;`**

```rust
// 在现有 pub mod 声明后追加(W8 Plan 1 Task 3)
pub mod template;
```

- [ ] **Step 4: 写失败测试 — 加 `#[cfg(test)] mod tests` 到 `template.rs` 末尾**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_literal() {
        let expr = SlotTemplateEngine::parse("notepad").unwrap();
        assert_eq!(expr, TemplateExpr::Literal("notepad".into()));
    }

    #[test]
    fn parse_empty_string() {
        let expr = SlotTemplateEngine::parse("").unwrap();
        assert_eq!(expr, TemplateExpr::Literal(String::new()));
    }

    #[test]
    fn parse_prev_var() {
        let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Prev,
                path: "output.path".into(),
            })
        );
    }

    #[test]
    fn parse_step_var() {
        let expr = SlotTemplateEngine::parse("${n1.output.path}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Step("n1".into()),
                path: "output.path".into(),
            })
        );
    }

    #[test]
    fn parse_user_var() {
        let expr = SlotTemplateEngine::parse("${user.profile_name}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::User,
                path: "profile_name".into(),
            })
        );
    }

    #[test]
    fn parse_iter_var() {
        let expr = SlotTemplateEngine::parse("${item}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Iter,
                path: String::new(),
            })
        );
    }

    #[test]
    fn parse_iter_with_path() {
        let expr = SlotTemplateEngine::parse("${item.name}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Iter,
                path: "name".into(),
            })
        );
    }

    #[test]
    fn parse_concat() {
        let expr =
            SlotTemplateEngine::parse("C:/Users/${user.name}/Documents/${item.name}").unwrap();
        match expr {
            TemplateExpr::Concat(parts) => {
                assert_eq!(parts.len(), 5);
                assert_eq!(parts[0], TemplateExpr::Literal("C:/Users/".into()));
                assert_eq!(
                    parts[1],
                    TemplateExpr::Var(VarRef {
                        scope: VarScope::User,
                        path: "name".into(),
                    })
                );
            }
            other => panic!("expected Concat, got {:?}", other),
        }
    }

    #[test]
    fn parse_filter() {
        let expr =
            SlotTemplateEngine::parse("${prev.output.files}[?size > 1048576]").unwrap();
        match expr {
            TemplateExpr::Filter { source, predicate } => {
                assert_eq!(
                    *source,
                    TemplateExpr::Var(VarRef {
                        scope: VarScope::Prev,
                        path: "output.files".into(),
                    })
                );
                assert_eq!(predicate, "size > 1048576");
            }
            other => panic!("expected Filter, got {:?}", other),
        }
    }

    #[test]
    fn parse_unclosed_var_errors() {
        let err = SlotTemplateEngine::parse("${prev.output.path").unwrap_err();
        assert!(matches!(err, TemplateError::UnclosedVar { .. }));
    }

    #[test]
    fn parse_invalid_var_errors() {
        let err = SlotTemplateEngine::parse("${nopdot}").unwrap_err();
        assert!(matches!(err, TemplateError::Parse { .. }));
    }
}
```

- [ ] **Step 5: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib skills::template::tests::parse`
Expected: PASS(11 个 parse 测试全绿)

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/template.rs voicepilot/crates/trust-kernel/src/skills/mod.rs
git commit -m "feat(w8p1): add SlotTemplateEngine parser for \${prev}/${user}/${item} placeholders"
```

---

## Task 4: SlotTemplateEngine — resolve 渲染

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/template.rs`

- [ ] **Step 1: 实现 `resolve` 方法**

在 `impl SlotTemplateEngine` 块中追加:

```rust
    /// 执行模板:用 node_outputs + user_slots + iter_var 渲染最终值。
    ///
    /// - `node_outputs`:key = node_id,value = 节点 output(serde_json::Value)
    /// - `user_slots`:用户审批阶段填的 Slot 列表
    /// - `iter_var`:循环变量当前值(Some 时表示在循环体内)
    pub fn resolve(
        expr: &TemplateExpr,
        node_outputs: &std::collections::HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        iter_var: Option<&serde_json::Value>,
        prev_node_id: Option<&str>,
    ) -> Result<serde_json::Value, TemplateError> {
        match expr {
            TemplateExpr::Literal(s) => Ok(serde_json::Value::String(s.clone())),
            TemplateExpr::Var(var) => Self::resolve_var(var, node_outputs, user_slots, iter_var, prev_node_id),
            TemplateExpr::Concat(parts) => {
                let mut s = String::new();
                for p in parts {
                    let v = Self::resolve(p, node_outputs, user_slots, iter_var, prev_node_id)?;
                    match v {
                        serde_json::Value::String(t) => s.push_str(&t),
                        other => s.push_str(&other.to_string()),
                    }
                }
                Ok(serde_json::Value::String(s))
            }
            TemplateExpr::Filter { source, predicate } => {
                let src_val = Self::resolve(source, node_outputs, user_slots, iter_var, prev_node_id)?;
                Self::apply_filter(&src_val, predicate)
            }
        }
    }

    fn resolve_var(
        var: &VarRef,
        node_outputs: &std::collections::HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        iter_var: Option<&serde_json::Value>,
        prev_node_id: Option<&str>,
    ) -> Result<serde_json::Value, TemplateError> {
        match &var.scope {
            VarScope::Prev => {
                let nid = prev_node_id.ok_or_else(|| TemplateError::VarNotFound {
                    scope: "prev".into(),
                    path: var.path.clone(),
                })?;
                let node_out = node_outputs.get(nid).ok_or_else(|| TemplateError::VarNotFound {
                    scope: format!("prev({})", nid),
                    path: var.path.clone(),
                })?;
                Self::extract_path(node_out, &var.path)
            }
            VarScope::Step(step_id) => {
                let node_out = node_outputs.get(step_id).ok_or_else(|| TemplateError::VarNotFound {
                    scope: format!("step({})", step_id),
                    path: var.path.clone(),
                })?;
                Self::extract_path(node_out, &var.path)
            }
            VarScope::User => {
                // user_slots 中查找匹配 path 的 slot(raw 字段)
                let slot = user_slots.iter().find(|s| {
                    let kind_str = format!("{:?}", s.kind).to_lowercase();
                    kind_str == var.path.to_lowercase() || s.raw.contains(&var.path)
                });
                let slot = slot.ok_or_else(|| TemplateError::VarNotFound {
                    scope: "user".into(),
                    path: var.path.clone(),
                })?;
                Ok(serde_json::Value::String(slot.raw.clone()))
            }
            VarScope::Iter => {
                let val = iter_var.ok_or_else(|| TemplateError::VarNotFound {
                    scope: "item".into(),
                    path: var.path.clone(),
                })?;
                if var.path.is_empty() {
                    Ok(val.clone())
                } else {
                    Self::extract_path(val, &var.path)
                }
            }
        }
    }

    /// 从 JSON value 中按 dotted path 提取(如 "output.path" → obj["output"]["path"])。
    fn extract_path(val: &serde_json::Value, path: &str) -> Result<serde_json::Value, TemplateError> {
        if path.is_empty() {
            return Ok(val.clone());
        }
        let mut current = val;
        for key in path.split('.') {
            current = match current {
                serde_json::Value::Object(map) => map
                    .get(key)
                    .ok_or_else(|| TemplateError::VarNotFound {
                        scope: "json_path".into(),
                        path: format!("{}.{}", path, key),
                    })?,
                _ => {
                    return Err(TemplateError::TypeMismatch {
                        expected: "object",
                        got: current.to_string(),
                    })
                }
            };
        }
        Ok(current.clone())
    }

    /// W8 仅支持 `[?size > N]` / `[?size < N]` / `[?size >= N]` / `[?size <= N]`。
    /// 完整 JSONPath filter(如 `[?@.type == 'image']`)延后 W9+(spec §8)。
    fn apply_filter(src: &serde_json::Value, predicate: &str) -> Result<serde_json::Value, TemplateError> {
        let arr = match src {
            serde_json::Value::Array(a) => a,
            _ => {
                return Err(TemplateError::TypeMismatch {
                    expected: "array",
                    got: src.to_string(),
                })
            }
        };
        // 解析 "size > N" / "size < N" / "size >= N" / "size <= N"
        let p = predicate.trim();
        let (op, n_str) = if let Some(rest) = p.strip_prefix("size >=") {
            (">=", rest.trim())
        } else if let Some(rest) = p.strip_prefix("size <=") {
            ("<=", rest.trim())
        } else if let Some(rest) = p.strip_prefix("size >") {
            (">", rest.trim())
        } else if let Some(rest) = p.strip_prefix("size <") {
            ("<", rest.trim())
        } else {
            return Err(TemplateError::UnsupportedPredicate {
                predicate: predicate.into(),
            });
        };
        let threshold: u64 = n_str.parse().map_err(|_| {
            TemplateError::UnsupportedPredicate {
                predicate: predicate.into(),
            }
        })?;
        let filtered: Vec<serde_json::Value> = arr
            .iter()
            .filter(|item| {
                let size = item
                    .get("size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                match op {
                    ">" => size > threshold,
                    "<" => size < threshold,
                    ">=" => size >= threshold,
                    "<=" => size <= threshold,
                    _ => false,
                }
            })
            .cloned()
            .collect();
        Ok(serde_json::Value::Array(filtered))
    }
```

注意:`ExtractedSlot` 的字段定义在 `crates/trust-kernel/src/llm/types.rs`,需先确认其 `kind` / `raw` 字段名(若不同需调整 `resolve_var` 中的 User 分支匹配逻辑)。

- [ ] **Step 2: 写 resolve 测试**

在 `template.rs` 的 `#[cfg(test)] mod tests` 块中追加:

```rust
    use std::collections::HashMap;

    fn make_node_outputs() -> HashMap<String, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert(
            "n1".into(),
            serde_json::json!({
                "output": {
                    "path": "C:/Users/test/Documents/file.txt",
                    "files": [
                        {"name": "a.txt", "size": 500},
                        {"name": "b.txt", "size": 2_000_000}
                    ]
                }
            }),
        );
        m
    }

    #[test]
    fn resolve_literal() {
        let expr = TemplateExpr::Literal("hello".into());
        let v = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            None,
            None,
        )
        .unwrap();
        assert_eq!(v, serde_json::Value::String("hello".into()));
    }

    #[test]
    fn resolve_prev_var() {
        let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
        assert_eq!(v, serde_json::Value::String("C:/Users/test/Documents/file.txt".into()));
    }

    #[test]
    fn resolve_step_var() {
        let expr = SlotTemplateEngine::parse("${n1.output.path}").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
        assert_eq!(v, serde_json::Value::String("C:/Users/test/Documents/file.txt".into()));
    }

    #[test]
    fn resolve_concat() {
        let expr =
            SlotTemplateEngine::parse("Path: ${n1.output.path}").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
        assert_eq!(v, serde_json::Value::String("Path: C:/Users/test/Documents/file.txt".into()));
    }

    #[test]
    fn resolve_iter_var() {
        let expr = SlotTemplateEngine::parse("${item}").unwrap();
        let v = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            Some(&serde_json::json!("item_value")),
            None,
        )
        .unwrap();
        assert_eq!(v, serde_json::Value::String("item_value".into()));
    }

    #[test]
    fn resolve_iter_with_path() {
        let expr = SlotTemplateEngine::parse("${item.name}").unwrap();
        let v = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            Some(&serde_json::json!({"name": "x.txt", "size": 100})),
            None,
        )
        .unwrap();
        assert_eq!(v, serde_json::Value::String("x.txt".into()));
    }

    #[test]
    fn resolve_filter_gt() {
        let expr =
            SlotTemplateEngine::parse("${prev.output.files}[?size > 1048576]").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
        match v {
            serde_json::Value::Array(a) => {
                assert_eq!(a.len(), 1, "only b.txt > 1MB");
                assert_eq!(a[0]["name"], "b.txt");
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn resolve_filter_lt() {
        let expr =
            SlotTemplateEngine::parse("${prev.output.files}[?size < 1048576]").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
        match v {
            serde_json::Value::Array(a) => {
                assert_eq!(a.len(), 1, "only a.txt < 1MB");
                assert_eq!(a[0]["name"], "a.txt");
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn resolve_var_not_found_errors() {
        let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
        let err = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            None,
            None, // no prev_node_id
        )
        .unwrap_err();
        assert!(matches!(err, TemplateError::VarNotFound { .. }));
    }

    #[test]
    fn resolve_filter_on_non_array_errors() {
        let expr =
            SlotTemplateEngine::parse("${prev.output.path}[?size > 100]").unwrap();
        let outs = make_node_outputs();
        let err = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap_err();
        assert!(matches!(err, TemplateError::TypeMismatch { .. }));
    }
```

- [ ] **Step 3: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib skills::template::tests::resolve`
Expected: PASS(10 个 resolve 测试全绿)

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/template.rs
git commit -m "feat(w8p1): implement SlotTemplateEngine::resolve with var/concat/filter rendering"
```

---

## Task 5: SlotTemplateEngine — validate 校验

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/template.rs`

- [ ] **Step 1: 实现 `validate` + `validate_dag` 方法**

在 `impl SlotTemplateEngine` 块中追加:

```rust
    /// 校验:模板中所有变量引用都能在 DAG 上下文中找到绑定。
    ///
    /// - `dag_nodes`:DAG 中所有节点(用于校验 VarScope::Step 引用)
    /// - `user_slot_kinds`:用户审批阶段可填的 Slot kind 列表(用于校验 VarScope::User)
    /// - `loop_node_ids`:循环节点 id 列表(用于校验 VarScope::Iter 仅在循环节点内使用)
    pub fn validate(
        expr: &TemplateExpr,
        dag_nodes: &[DagNode],
        user_slot_kinds: &[&str],
        loop_node_ids: &[&str],
    ) -> Result<(), TemplateError> {
        let node_ids: std::collections::HashSet<&str> =
            dag_nodes.iter().map(|n| n.node_id.as_str()).collect();
        Self::validate_expr(expr, &node_ids, user_slot_kinds, loop_node_ids)
    }

    fn validate_expr(
        expr: &TemplateExpr,
        node_ids: &std::collections::HashSet<&str>,
        user_slot_kinds: &[&str],
        loop_node_ids: &[&str],
    ) -> Result<(), TemplateError> {
        match expr {
            TemplateExpr::Literal(_) => Ok(()),
            TemplateExpr::Var(var) => match &var.scope {
                VarScope::Prev => {
                    // Prev 在运行时绑定到拓扑序前驱,编译期无法校验,
                    // 仅当 DAG 无前驱时失败(由 validate_dag 在节点级校验)
                    Ok(())
                }
                VarScope::Step(step_id) => {
                    if node_ids.contains(step_id.as_str()) {
                        Ok(())
                    } else {
                        Err(TemplateError::UnknownNodeId {
                            node_id: step_id.clone(),
                        })
                    }
                }
                VarScope::User => {
                    // user_slot_kinds 列表校验:path 必须匹配某个 kind
                    let path_lower = var.path.to_lowercase();
                    if user_slot_kinds
                        .iter()
                        .any(|k| k.to_lowercase() == path_lower)
                    {
                        Ok(())
                    } else {
                        Err(TemplateError::UnknownSlotKind {
                            kind: var.path.clone(),
                        })
                    }
                }
                VarScope::Iter => {
                    // Iter 仅在循环节点内合法;此处仅做弱校验
                    // (节点是否为循环节点由 validate_dag 在节点级校验)
                    if loop_node_ids.is_empty() {
                        // 非循环节点却引用 ${item} — 在 validate_dag 节点级校验中报错
                        // 这里返回 Ok,让节点级校验精确报错
                        Ok(())
                    } else {
                        Ok(())
                    }
                }
            },
            TemplateExpr::Concat(parts) => {
                for p in parts {
                    Self::validate_expr(p, node_ids, user_slot_kinds, loop_node_ids)?;
                }
                Ok(())
            }
            TemplateExpr::Filter { source, predicate } => {
                Self::validate_expr(source, node_ids, user_slot_kinds, loop_node_ids)?;
                Self::validate_filter_predicate(predicate)?;
                Ok(())
            }
        }
    }

    /// W8 仅支持 `[?size > N]` / `[?size < N]` / `[?size >= N]` / `[?size <= N]`。
    fn validate_filter_predicate(predicate: &str) -> Result<(), TemplateError> {
        let p = predicate.trim();
        let rest = if let Some(r) = p.strip_prefix("size >=") {
            r
        } else if let Some(r) = p.strip_prefix("size <=") {
            r
        } else if let Some(r) = p.strip_prefix("size >") {
            r
        } else if let Some(r) = p.strip_prefix("size <") {
            r
        } else {
            return Err(TemplateError::UnsupportedPredicate {
                predicate: predicate.into(),
            });
        };
        rest.trim()
            .parse::<u64>()
            .map(|_| ())
            .map_err(|_| TemplateError::UnsupportedPredicate {
                predicate: predicate.into(),
            })
    }

    /// 校验整个 DAG:遍历所有节点,校验其 input_template.template。
    /// 同时校验:只有 loop_specs 中标记为循环节点的才能引用 ${item}。
    pub fn validate_dag(plan: &crate::skills::dag_types::DagPlan) -> Result<(), TemplateError> {
        let node_ids: std::collections::HashSet<&str> =
            plan.nodes.iter().map(|n| n.node_id.as_str()).collect();
        let loop_node_ids: Vec<&str> = plan
            .loop_specs
            .keys()
            .map(|s| s.as_str())
            .collect();
        // 收集 user_slot_kinds — 从所有节点的 SlotTemplate.kind 推断
        // (W8 简化:Slot.kind 直接当作 user_slot kind,不做跨节点 cross-check)
        let user_slot_kinds: Vec<&str> = plan
            .nodes
            .iter()
            .map(|n| match n.input_template.kind {
                SlotKind::Path => "path",
                SlotKind::App => "app",
                SlotKind::Number => "number",
                SlotKind::TimeRange => "time_range",
                SlotKind::Url => "url",
                SlotKind::Files => "files",
                SlotKind::Text => "text",
            })
            .collect();

        for node in &plan.nodes {
            let is_loop = plan.loop_specs.contains_key(&node.node_id);
            let loop_ids: Vec<&str> = if is_loop {
                vec![node.node_id.as_str()]
            } else {
                vec![]
            };
            // 校验模板表达式
            Self::validate_expr(
                &node.input_template.template,
                &node_ids,
                &user_slot_kinds,
                &loop_ids,
            )?;
            // 若非循环节点却引用 ${item} → 报错
            if !is_loop {
                if Self::references_iter(&node.input_template.template) {
                    return Err(TemplateError::VarNotFound {
                        scope: "item".into(),
                        path: format!(
                            "node {} is not a loop node but references ${{item}}",
                            node.node_id
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    /// 递归检查表达式是否引用 VarScope::Iter。
    fn references_iter(expr: &TemplateExpr) -> bool {
        match expr {
            TemplateExpr::Literal(_) => false,
            TemplateExpr::Var(var) => matches!(var.scope, VarScope::Iter),
            TemplateExpr::Concat(parts) => parts.iter().any(Self::references_iter),
            TemplateExpr::Filter { source, .. } => Self::references_iter(source),
        }
    }
```

- [ ] **Step 2: 写 validate 测试**

在 `template.rs` 的 `#[cfg(test)] mod tests` 块中追加:

```rust
    use crate::policy::types::ELevel;
    use crate::skills::dag_types::{DagEdge, DagNode, DagPlan, LoopSpec, IterableSource};
    use crate::skills::template::{SlotKind, SlotTemplate};

    fn tpl(kind: SlotKind, expr: TemplateExpr) -> SlotTemplate {
        SlotTemplate { kind, template: expr }
    }

    fn node(id: &str, expr: TemplateExpr) -> DagNode {
        DagNode {
            node_id: id.into(),
            skill_id: "test.skill".into(),
            input_template: tpl(SlotKind::Text, expr),
            risk_ceiling: ELevel::E1,
        }
    }

    fn plan(nodes: Vec<DagNode>) -> DagPlan {
        DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes,
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        }
    }

    #[test]
    fn validate_literal_ok() {
        let expr = TemplateExpr::Literal("notepad".into());
        let p = plan(vec![node("n1", expr)]);
        assert!(SlotTemplateEngine::validate_dag(&p).is_ok());
    }

    #[test]
    fn validate_step_ref_known_node_ok() {
        let expr = SlotTemplateEngine::parse("${n1.output.path}").unwrap();
        let p = plan(vec![
            node("n1", TemplateExpr::Literal("a".into())),
            node("n2", expr),
        ]);
        assert!(SlotTemplateEngine::validate_dag(&p).is_ok());
    }

    #[test]
    fn validate_step_ref_unknown_node_errors() {
        let expr = SlotTemplateEngine::parse("${n99.output.path}").unwrap();
        let p = plan(vec![node("n1", expr)]);
        let err = SlotTemplateEngine::validate_dag(&p).unwrap_err();
        assert!(matches!(err, TemplateError::UnknownNodeId { .. }));
    }

    #[test]
    fn validate_iter_in_loop_node_ok() {
        let mut specs = HashMap::new();
        specs.insert(
            "n1".into(),
            LoopSpec {
                loop_var: "item".into(),
                iterable_source: IterableSource::Literal(vec!["a".into()]),
                max_iterations: 5,
                break_condition: None,
            },
        );
        let expr = SlotTemplateEngine::parse("${item}").unwrap();
        let mut p = plan(vec![node("n1", expr)]);
        p.loop_specs = specs;
        assert!(SlotTemplateEngine::validate_dag(&p).is_ok());
    }

    #[test]
    fn validate_iter_in_non_loop_node_errors() {
        let expr = SlotTemplateEngine::parse("${item}").unwrap();
        let p = plan(vec![node("n1", expr)]);
        let err = SlotTemplateEngine::validate_dag(&p).unwrap_err();
        assert!(matches!(err, TemplateError::VarNotFound { .. }));
    }

    #[test]
    fn validate_filter_supported_predicate_ok() {
        let expr =
            SlotTemplateEngine::parse("${prev.output.files}[?size > 1048576]").unwrap();
        let p = plan(vec![node("n1", expr)]);
        assert!(SlotTemplateEngine::validate_dag(&p).is_ok());
    }

    #[test]
    fn validate_filter_unsupported_predicate_errors() {
        let expr =
            SlotTemplateEngine::parse("${prev.output.files}[?@.type == 'image']").unwrap();
        let p = plan(vec![node("n1", expr)]);
        let err = SlotTemplateEngine::validate_dag(&p).unwrap_err();
        assert!(matches!(err, TemplateError::UnsupportedPredicate { .. }));
    }
```

- [ ] **Step 3: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib skills::template::tests::validate`
Expected: PASS(7 个 validate 测试全绿)

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/template.rs
git commit -m "feat(w8p1): add SlotTemplateEngine::validate + validate_dag with iter/step/filter checks"
```

---

## Task 6: DagRepo — CRUD 实现

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/dag_repo.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1: 创建 `dag_repo.rs`,实现 DagRepo**

```rust
//! DagRepo — W8 §2.6.
//!
//! CRUD for `dag_plans` + `dag_nodes` tables.
//! 遵循 W4 McpServerRepo 模式:`new()` 不带参数,方法接收 `&Connection`。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::skills::dag_types::{DagNode, DagNodeStatus, DagPlan, DagStatus};

/// DB 持久化形态(序列化字段用 JSON)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlanRecord {
    pub plan_id: String,
    pub user_goal: String,
    pub plan_json: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub root_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNodeRecord {
    pub plan_id: String,
    pub node_id: String,
    pub skill_id: String,
    pub input_template_json: String,
    pub risk_ceiling: String,
    pub status: String,
    pub output_json: Option<String>,
    pub error_message: Option<String>,
    pub task_id: Option<String>,
    pub step_id: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

pub struct DagRepo;

impl DagRepo {
    pub fn new() -> Self {
        Self
    }

    /// 创建 DAG plan 记录。
    pub fn create_plan(&self, conn: &Connection, plan: &DagPlan, status: &DagStatus, root_task_id: Option<&str>) -> Result<()> {
        let plan_json = serde_json::to_string(plan)
            .map_err(|e| crate::error::KernelError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
        let now = now_iso();
        conn.execute(
            r#"INSERT INTO dag_plans (plan_id, user_goal, plan_json, status, created_at, completed_at, root_task_id)
               VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)"#,
            params![
                plan.plan_id,
                plan.user_goal,
                plan_json,
                status.as_str(),
                now,
                root_task_id,
            ],
        )?;
        Ok(())
    }

    pub fn get_plan(&self, conn: &Connection, plan_id: &str) -> Result<Option<DagPlanRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT plan_id, user_goal, plan_json, status, created_at, completed_at, root_task_id
               FROM dag_plans WHERE plan_id = ?1"#,
        )?;
        let mut rows = stmt.query(params![plan_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(DagPlanRecord {
                plan_id: row.get(0)?,
                user_goal: row.get(1)?,
                plan_json: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                completed_at: row.get(5)?,
                root_task_id: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// 按 status 列表查询 plan(W8 UI 显示"运行中" / "已完成" DAG)。
    pub fn list_plans_by_status(&self, conn: &Connection, status: &str) -> Result<Vec<DagPlanRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT plan_id, user_goal, plan_json, status, created_at, completed_at, root_task_id
               FROM dag_plans WHERE status = ?1 ORDER BY created_at DESC"#,
        )?;
        let rows = stmt.query_map(params![status], |row| {
            Ok(DagPlanRecord {
                plan_id: row.get(0)?,
                user_goal: row.get(1)?,
                plan_json: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                completed_at: row.get(5)?,
                root_task_id: row.get(6)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 更新 DAG plan 状态(DagExecutor 执行后调用)。
    pub fn update_plan_status(
        &self,
        conn: &Connection,
        plan_id: &str,
        status: &DagStatus,
        completed: bool,
    ) -> Result<()> {
        let now = if completed { Some(now_iso()) } else { None };
        conn.execute(
            r#"UPDATE dag_plans SET status = ?1, completed_at = COALESCE(?2, completed_at) WHERE plan_id = ?3"#,
            params![status.as_str(), now, plan_id],
        )?;
        Ok(())
    }

    /// 创建 DAG 节点记录。
    pub fn create_node(&self, conn: &Connection, plan_id: &str, node: &DagNode) -> Result<()> {
        let input_template_json = serde_json::to_string(&node.input_template)
            .map_err(|e| crate::error::KernelError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
        let now = now_iso();
        conn.execute(
            r#"INSERT INTO dag_nodes (plan_id, node_id, skill_id, input_template_json, risk_ceiling, status, output_json, error_message, task_id, step_id, started_at, completed_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL, ?7, NULL)"#,
            params![
                plan_id,
                node.node_id,
                node.skill_id,
                input_template_json,
                format!("{:?}", node.risk_ceiling),
                DagNodeStatus::Pending.as_str(),
                now,
            ],
        )?;
        Ok(())
    }

    /// 更新 DAG 节点状态(DagExecutor 节点执行后调用)。
    pub fn update_node_status(
        &self,
        conn: &Connection,
        plan_id: &str,
        node_id: &str,
        status: &DagNodeStatus,
        task_id: Option<&str>,
        step_id: Option<&str>,
    ) -> Result<()> {
        let (output_json, error_msg) = match status {
            DagNodeStatus::Succeeded(out) => (
                Some(serde_json::to_string(out).unwrap_or_default()),
                None,
            ),
            DagNodeStatus::Failed { cause } => (None, Some(cause.clone())),
            _ => (None, None),
        };
        let completed = matches!(
            status,
            DagNodeStatus::Succeeded(_) | DagNodeStatus::Failed { .. } | DagNodeStatus::Skipped
        );
        let now = if completed { Some(now_iso()) } else { None };
        conn.execute(
            r#"UPDATE dag_nodes SET status = ?1, output_json = ?2, error_message = ?3, task_id = COALESCE(?4, task_id), step_id = COALESCE(?5, step_id), completed_at = COALESCE(?6, completed_at) WHERE plan_id = ?7 AND node_id = ?8"#,
            params![
                status.as_str(),
                output_json,
                error_msg,
                task_id,
                step_id,
                now,
                plan_id,
                node_id,
            ],
        )?;
        Ok(())
    }

    /// 列出某 DAG plan 的所有节点(按 started_at 排序)。
    pub fn list_nodes_by_plan(&self, conn: &Connection, plan_id: &str) -> Result<Vec<DagNodeRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT plan_id, node_id, skill_id, input_template_json, risk_ceiling, status, output_json, error_message, task_id, step_id, started_at, completed_at
               FROM dag_nodes WHERE plan_id = ?1 ORDER BY started_at ASC"#,
        )?;
        let rows = stmt.query_map(params![plan_id], |row| {
            Ok(DagNodeRecord {
                plan_id: row.get(0)?,
                node_id: row.get(1)?,
                skill_id: row.get(2)?,
                input_template_json: row.get(3)?,
                risk_ceiling: row.get(4)?,
                status: row.get(5)?,
                output_json: row.get(6)?,
                error_message: row.get(7)?,
                task_id: row.get(8)?,
                step_id: row.get(9)?,
                started_at: row.get(10)?,
                completed_at: row.get(11)?,
            })
        })?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        Ok(v)
    }

    /// 级联删除 DAG plan + 其所有节点。
    /// (FK ON DELETE CASCADE 会自动删 dag_nodes,但显式删便于审计)
    pub fn delete_plan_cascade(&self, conn: &Connection, plan_id: &str) -> Result<()> {
        conn.execute(r#"DELETE FROM dag_nodes WHERE plan_id = ?1"#, params![plan_id])?;
        conn.execute(r#"DELETE FROM dag_plans WHERE plan_id = ?1"#, params![plan_id])?;
        Ok(())
    }
}

impl Default for DagRepo {
    fn default() -> Self {
        Self::new()
    }
}

fn now_iso() -> String {
    // 与 W1 audit.rs 一致的时间格式(UTC ISO 8601)
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}", dur.as_secs())
}
```

- [ ] **Step 2: 在 `skills/mod.rs` 加 `pub mod dag_repo;`**

```rust
// 在现有 pub mod 声明后追加(W8 Plan 1 Task 6)
pub mod dag_repo;
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_repo.rs voicepilot/crates/trust-kernel/src/skills/mod.rs
git commit -m "feat(w8p1): add DagRepo with dag_plans/dag_nodes CRUD + cascade delete"
```

---

## Task 7: TaskExplanationRepo — CRUD 实现

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/explanation_repo.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`

- [ ] **Step 1: 创建 `explanation_repo.rs`**

```rust
//! TaskExplanationRepo — W8 §2.5 / §2.6.
//!
//! CRUD for `task_explanations` table.
//! Stores LLM failure analysis results from `task.explain` Skill.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExplanationRecord {
    pub explanation_id: String,
    pub step_id: String,
    pub root_cause_zh: String,
    pub category: String,
    pub suggested_fix: Option<String>,
    pub confidence: f32,
    pub llm_model: Option<String>,
    pub created_at: String,
}

/// 失败分类(W8 §2.5)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    McpUnavailable,
    PathNotAllowed,
    ApprovalDenied,
    NetworkError,
    Unknown,
}

impl FailureCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::McpUnavailable => "mcp_unavailable",
            Self::PathNotAllowed => "path_not_allowed",
            Self::ApprovalDenied => "approval_denied",
            Self::NetworkError => "network_error",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "mcp_unavailable" => Some(Self::McpUnavailable),
            "path_not_allowed" => Some(Self::PathNotAllowed),
            "approval_denied" => Some(Self::ApprovalDenied),
            "network_error" => Some(Self::NetworkError),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

pub struct TaskExplanationRepo;

impl TaskExplanationRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(
        &self,
        conn: &Connection,
        rec: &TaskExplanationRecord,
    ) -> Result<()> {
        let now = now_iso();
        conn.execute(
            r#"INSERT INTO task_explanations (explanation_id, step_id, root_cause_zh, category, suggested_fix, confidence, llm_model, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
            params![
                rec.explanation_id,
                rec.step_id,
                rec.root_cause_zh,
                rec.category,
                rec.suggested_fix,
                rec.confidence,
                rec.llm_model,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_by_id(
        &self,
        conn: &Connection,
        explanation_id: &str,
    ) -> Result<Option<TaskExplanationRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT explanation_id, step_id, root_cause_zh, category, suggested_fix, confidence, llm_model, created_at
               FROM task_explanations WHERE explanation_id = ?1"#,
        )?;
        let mut rows = stmt.query(params![explanation_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    /// W8 §2.5:按 step_id 查询最新归因(UI 显示 task.explain 结果)。
    pub fn get_by_step_id(
        &self,
        conn: &Connection,
        step_id: &str,
    ) -> Result<Option<TaskExplanationRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT explanation_id, step_id, root_cause_zh, category, suggested_fix, confidence, llm_model, created_at
               FROM task_explanations WHERE step_id = ?1 ORDER BY created_at DESC LIMIT 1"#,
        )?;
        let mut rows = stmt.query(params![step_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn delete(&self, conn: &Connection, explanation_id: &str) -> Result<()> {
        conn.execute(
            r#"DELETE FROM task_explanations WHERE explanation_id = ?1"#,
            params![explanation_id],
        )?;
        Ok(())
    }
}

impl Default for TaskExplanationRepo {
    fn default() -> Self {
        Self::new()
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskExplanationRecord> {
    Ok(TaskExplanationRecord {
        explanation_id: row.get(0)?,
        step_id: row.get(1)?,
        root_cause_zh: row.get(2)?,
        category: row.get(3)?,
        suggested_fix: row.get(4)?,
        confidence: row.get(5)?,
        llm_model: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}", dur.as_secs())
}
```

- [ ] **Step 2: 在 `skills/mod.rs` 加 `pub mod explanation_repo;`**

```rust
// 在现有 pub mod 声明后追加(W8 Plan 1 Task 7)
pub mod explanation_repo;
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 4: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/explanation_repo.rs voicepilot/crates/trust-kernel/src/skills/mod.rs
git commit -m "feat(w8p1): add TaskExplanationRepo with task_explanations CRUD + FailureCategory"
```

---

## Task 8: DagRepo + TaskExplanationRepo 端到端冒烟测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w8_dag_repo_smoke.rs`

- [ ] **Step 1: 创建冒烟测试文件**

```rust
//! W8 Plan 1 Task 8: DagRepo + TaskExplanationRepo 端到端冒烟测试.

use std::collections::HashMap;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagNodeStatus, DagPlan, DagStatus,
};
use trust_kernel::skills::explanation_repo::{FailureCategory, TaskExplanationRecord, TaskExplanationRepo};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

fn make_plan(plan_id: &str) -> DagPlan {
    let n1 = DagNode {
        node_id: "n1".into(),
        skill_id: "note.capture".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("notepad".into()),
        },
        risk_ceiling: ELevel::E1,
    };
    let n2 = DagNode {
        node_id: "n2".into(),
        skill_id: "files.move".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Path,
            template: TemplateExpr::Var(trust_kernel::skills::template::VarRef {
                scope: trust_kernel::skills::template::VarScope::Prev,
                path: "output.path".into(),
            }),
        },
        risk_ceiling: ELevel::E2,
    };
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "打开记事本写 TODO 然后保存到桌面".into(),
        nodes: vec![n1, n2],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: Some("output.path -> input.source".into()),
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn dag_repo_create_and_get_plan() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-001");

    repo.create_plan(&conn, &plan, &DagStatus::Pending, None).unwrap();

    let got = repo.get_plan(&conn, "plan-001").unwrap().unwrap();
    assert_eq!(got.plan_id, "plan-001");
    assert_eq!(got.status, "pending");
    assert_eq!(got.root_task_id, None);
    assert!(got.plan_json.contains("note.capture"));
    assert!(got.plan_json.contains("files.move"));
}

#[test]
fn dag_repo_update_plan_status() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-002");

    repo.create_plan(&conn, &plan, &DagStatus::Pending, None).unwrap();
    repo.update_plan_status(&conn, "plan-002", &DagStatus::Succeeded, true).unwrap();

    let got = repo.get_plan(&conn, "plan-002").unwrap().unwrap();
    assert_eq!(got.status, "succeeded");
    assert!(got.completed_at.is_some());
}

#[test]
fn dag_repo_list_plans_by_status() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    repo.create_plan(&conn, &make_plan("p-a"), &DagStatus::Pending, None).unwrap();
    repo.create_plan(&conn, &make_plan("p-b"), &DagStatus::Succeeded, None).unwrap();
    repo.create_plan(&conn, &make_plan("p-c"), &DagStatus::Pending, None).unwrap();

    let pending = repo.list_plans_by_status(&conn, "pending").unwrap();
    assert_eq!(pending.len(), 2);
    let succeeded = repo.list_plans_by_status(&conn, "succeeded").unwrap();
    assert_eq!(succeeded.len(), 1);
}

#[test]
fn dag_repo_create_and_update_nodes() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-003");

    repo.create_plan(&conn, &plan, &DagStatus::Running, None).unwrap();
    for node in &plan.nodes {
        repo.create_node(&conn, &plan.plan_id, node).unwrap();
    }

    // 模拟 n1 执行成功,n2 执行失败
    repo.update_node_status(
        &conn,
        "plan-003",
        "n1",
        &DagNodeStatus::Succeeded(serde_json::json!({"output": {"path": "C:/test.txt"}})),
        Some("task-001"),
        Some("step-001"),
    )
    .unwrap();
    repo.update_node_status(
        &conn,
        "plan-003",
        "n2",
        &DagNodeStatus::Failed {
            cause: "permission denied".into(),
        },
        None,
        None,
    )
    .unwrap();

    let nodes = repo.list_nodes_by_plan(&conn, "plan-003").unwrap();
    assert_eq!(nodes.len(), 2);
    let n1_rec = nodes.iter().find(|n| n.node_id == "n1").unwrap();
    assert_eq!(n1_rec.status, "succeeded");
    assert!(n1_rec.output_json.is_some());
    assert_eq!(n1_rec.task_id.as_deref(), Some("task-001"));
    let n2_rec = nodes.iter().find(|n| n.node_id == "n2").unwrap();
    assert_eq!(n2_rec.status, "failed");
    assert_eq!(n2_rec.error_message.as_deref(), Some("permission denied"));
}

#[test]
fn dag_repo_cascade_delete() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-004");
    repo.create_plan(&conn, &plan, &DagStatus::Pending, None).unwrap();
    for node in &plan.nodes {
        repo.create_node(&conn, &plan.plan_id, node).unwrap();
    }
    // 确认节点已创建
    let nodes_before = repo.list_nodes_by_plan(&conn, "plan-004").unwrap();
    assert_eq!(nodes_before.len(), 2);

    repo.delete_plan_cascade(&conn, "plan-004").unwrap();

    // 确认 plan + nodes 全删
    assert!(repo.get_plan(&conn, "plan-004").unwrap().is_none());
    let nodes_after = repo.list_nodes_by_plan(&conn, "plan-004").unwrap();
    assert_eq!(nodes_after.len(), 0);
}

#[test]
fn task_explanation_repo_create_and_get() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = TaskExplanationRepo::new();

    // 必须先创建 parent step(FK 约束)
    // 用 W1 task_repo + step_repo 先插入 task + step
    use trust_kernel::repo::task_repo::TaskRepo;
    use trust_kernel::repo::step_repo::StepRepo;
    use trust_kernel::state::TaskState;
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    task_repo
        .create(&conn, "task-explain-test", "test goal", TaskState::Pending)
        .unwrap();
    step_repo
        .create(&conn, "step-001", "task-explain-test", 0, "test step")
        .unwrap();

    let rec = TaskExplanationRecord {
        explanation_id: "exp-001".into(),
        step_id: "step-001".into(),
        root_cause_zh: "MCP 服务器未启动".into(),
        category: FailureCategory::McpUnavailable.as_str().into(),
        suggested_fix: Some("请在 Settings 中启动 Playwright MCP".into()),
        confidence: 0.85,
        llm_model: Some("gpt-4o-mini".into()),
        created_at: String::new(),
    };
    repo.create(&conn, &rec).unwrap();

    let got = repo.get_by_id(&conn, "exp-001").unwrap().unwrap();
    assert_eq!(got.step_id, "step-001");
    assert_eq!(got.root_cause_zh, "MCP 服务器未启动");
    assert_eq!(got.category, "mcp_unavailable");
    assert!(got.confidence - 0.85 < 0.001);
    assert_eq!(got.llm_model.as_deref(), Some("gpt-4o-mini"));

    let by_step = repo.get_by_step_id(&conn, "step-001").unwrap().unwrap();
    assert_eq!(by_step.explanation_id, "exp-001");
}

#[test]
fn task_explanation_repo_delete() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = TaskExplanationRepo::new();
    use trust_kernel::repo::task_repo::TaskRepo;
    use trust_kernel::repo::step_repo::StepRepo;
    use trust_kernel::state::TaskState;
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    task_repo
        .create(&conn, "task-del-test", "test", TaskState::Pending)
        .unwrap();
    step_repo
        .create(&conn, "step-del", "task-del-test", 0, "s")
        .unwrap();
    let rec = TaskExplanationRecord {
        explanation_id: "exp-del".into(),
        step_id: "step-del".into(),
        root_cause_zh: "test".into(),
        category: FailureCategory::Unknown.as_str().into(),
        suggested_fix: None,
        confidence: 0.1,
        llm_model: None,
        created_at: String::new(),
    };
    repo.create(&conn, &rec).unwrap();
    assert!(repo.get_by_id(&conn, "exp-del").unwrap().is_some());
    repo.delete(&conn, "exp-del").unwrap();
    assert!(repo.get_by_id(&conn, "exp-del").unwrap().is_none());
}

#[test]
fn failure_category_round_trip() {
    for cat in [
        FailureCategory::McpUnavailable,
        FailureCategory::PathNotAllowed,
        FailureCategory::ApprovalDenied,
        FailureCategory::NetworkError,
        FailureCategory::Unknown,
    ] {
        let s = cat.as_str();
        let back = FailureCategory::from_str(s).unwrap();
        assert_eq!(cat, back);
    }
}
```

- [ ] **Step 2: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_dag_repo_smoke`
Expected: PASS(7 个测试全绿)

若 FK 约束失败,确认 `task_repo.create` / `step_repo.create` 的方法签名是否与现有代码一致 — 参考 `voicepilot/crates/trust-kernel/src/repo/task_repo.rs` 和 `step_repo.rs` 的实际签名调整测试代码。

- [ ] **Step 3: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w8_dag_repo_smoke.rs
git commit -m "test(w8p1): add DagRepo + TaskExplanationRepo E2E smoke (7 tests)"
```

---

## Task 9: SlotTemplateEngine 集成测试

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w8_template_unit.rs`

- [ ] **Step 1: 创建集成测试文件**

```rust
//! W8 Plan 1 Task 9: SlotTemplateEngine 集成测试.
//!
//! 覆盖 spec §2.1 模板语法的全部 8 种表达式,
//! 以及 §6 安全约束中的双层校验(LLM 返回后立即校验 + 执行前再次校验)。

use std::collections::HashMap;

use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan, IterableSource, LoopSpec};
use trust_kernel::skills::template::{
    SlotKind, SlotTemplate, SlotTemplateEngine, TemplateError, TemplateExpr, VarRef, VarScope,
};
use trust_kernel::policy::types::ELevel;

fn tpl(kind: SlotKind, s: &str) -> SlotTemplate {
    SlotTemplate {
        kind,
        template: SlotTemplateEngine::parse(s).unwrap(),
    }
}

fn node(id: &str, kind: SlotKind, tpl_str: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "test.skill".into(),
        input_template: tpl(kind, tpl_str),
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with_nodes(nodes: Vec<DagNode>) -> DagPlan {
    DagPlan {
        plan_id: "p1".into(),
        user_goal: "test".into(),
        nodes,
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn integration_literal_only() {
    let expr = SlotTemplateEngine::parse("notepad").unwrap();
    let outs = HashMap::new();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
    assert_eq!(v, serde_json::Value::String("notepad".into()));
}

#[test]
fn integration_prev_output_path() {
    // 模拟 n1 输出 {output: {path: "C:/x.txt"}},n2 引用 ${prev.output.path}
    let mut outs = HashMap::new();
    outs.insert(
        "n1".into(),
        serde_json::json!({"output": {"path": "C:/Users/test/x.txt"}}),
    );
    let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
    assert_eq!(v, serde_json::Value::String("C:/Users/test/x.txt".into()));
}

#[test]
fn integration_concat_path() {
    let mut outs = HashMap::new();
    outs.insert(
        "n1".into(),
        serde_json::json!({"output": {"name": "report.md"}}),
    );
    let expr = SlotTemplateEngine::parse("C:/Docs/${n1.output.name}").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
    assert_eq!(v, serde_json::Value::String("C:/Docs/report.md".into()));
}

#[test]
fn integration_user_slot() {
    use trust_kernel::llm::types::{ExtractedSlot, SlotKind as LlmSlotKind};
    let slots = vec![ExtractedSlot {
        kind: LlmSlotKind::Text,
        raw: "alice".into(),
        high_risk: false,
    }];
    // 注意:user slot 匹配依赖 ExtractedSlot.kind 的 Debug 输出
    // 测试中用 kind=Text 匹配 ${user.text} 或 ${user.name}(后者需调整 matching 逻辑)
    let expr = SlotTemplateEngine::parse("${user.text}").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &HashMap::new(), &slots, None, None).unwrap();
    assert_eq!(v, serde_json::Value::String("alice".into()));
}

#[test]
fn integration_iter_in_loop() {
    let mut specs = HashMap::new();
    specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::Literal(vec!["a.txt".into(), "b.txt".into()]),
            max_iterations: 5,
            break_condition: None,
        },
    );
    let mut p = plan_with_nodes(vec![node("n1", SlotKind::Text, "${item}")]);
    p.loop_specs = specs;
    assert!(SlotTemplateEngine::validate_dag(&p).is_ok());
}

#[test]
fn integration_filter_size_gt() {
    let mut outs = HashMap::new();
    outs.insert(
        "n1".into(),
        serde_json::json!({"output": {"files": [
            {"name":"a","size":100},
            {"name":"b","size":2_000_000},
            {"name":"c","size":5_000_000}
        ]}}),
    );
    let expr = SlotTemplateEngine::parse("${prev.output.files}[?size > 1048576]").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
    match v {
        serde_json::Value::Array(a) => assert_eq!(a.len(), 2),
        _ => panic!("expected Array"),
    }
}

#[test]
fn integration_validate_dag_rejects_unknown_step() {
    let p = plan_with_nodes(vec![node("n1", SlotKind::Text, "${n99.output.path}")]);
    let err = SlotTemplateEngine::validate_dag(&p).unwrap_err();
    assert!(matches!(err, TemplateError::UnknownNodeId { .. }));
}

#[test]
fn integration_validate_dag_rejects_iter_in_non_loop() {
    let p = plan_with_nodes(vec![node("n1", SlotKind::Text, "${item}")]);
    let err = SlotTemplateEngine::validate_dag(&p).unwrap_err();
    assert!(matches!(err, TemplateError::VarNotFound { .. }));
}

#[test]
fn integration_double_layer_defense() {
    // 模拟 spec §2.1 双层防御:
    // Layer 1: LLM 返回后立即 validate_dag → 拒绝
    // Layer 2: 执行前 resolve → 失败
    let bad_plan = plan_with_nodes(vec![node("n1", SlotKind::Text, "${n99.unknown}")]);
    // Layer 1
    let err1 = SlotTemplateEngine::validate_dag(&bad_plan).unwrap_err();
    assert!(matches!(err1, TemplateError::UnknownNodeId { node_id } if node_id == "n99"));
    // Layer 2:即使 Layer 1 漏检,resolve 也会失败
    let expr = SlotTemplateEngine::parse("${n99.unknown}").unwrap();
    let err2 = SlotTemplateEngine::resolve(&expr, &HashMap::new(), &[], None, None).unwrap_err();
    assert!(matches!(err2, TemplateError::VarNotFound { .. }));
}
```

- [ ] **Step 2: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_template_unit`
Expected: PASS(9 个集成测试全绿)

注意:`ExtractedSlot` 的 `kind` 字段类型需确认 — 若 `llm::types::SlotKind` 与本 plan 中 `template::SlotKind` 不同,需在 `integration_user_slot` 测试中调整匹配。打开 `voicepilot/crates/trust-kernel/src/llm/types.rs` 确认 `ExtractedSlot` 结构后,可能需调整 `resolve_var` 中 User 分支的匹配逻辑(基于 kind 字段做匹配)。

- [ ] **Step 3: Commit**

```powershell
git add voicepilot/crates/trust-kernel/tests/w8_template_unit.rs
git commit -m "test(w8p1): add SlotTemplateEngine integration tests (9 tests, double-layer defense)"
```

---

## Task 10: clippy + 全 feature cargo check + PROGRESS.md 更新

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: 跑 clippy,确认 0 警告**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy --workspace --no-default-features -- -D warnings`
Expected: PASS(0 warnings)

若有警告,逐一修复:
- 未使用 import → 删除
- `explicit_auto_deref` → 改 `&kernel.conn()`(参考 project_memory.md "Engineering Conventions")
- 其他 lint → 按 clippy 提示修复

- [ ] **Step 2: 跑 6 套 feature 组合 cargo check**

```powershell
cd d:\voicepilot\voicepilot
cargo check --workspace --no-default-features
cargo check --workspace --features llm
cargo check --workspace --features tauri
cargo check --workspace --features voice,tauri
cargo check --workspace --features voice,tauri,llm
cargo check --workspace --features voice,tauri,llm,uia
```

Expected: 全 PASS(本 plan 不涉及 feature gate,所有组合应编译通过)

- [ ] **Step 3: 跑全量测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --no-default-features`
Expected: PASS(W1-W7 测试 + W8 Plan 1 新增测试全绿,无回归)

新增测试预期:
- `migrations::migration_003_creates_dag_tables`:1 个
- `skills::dag_types::tests::*`:8 个
- `skills::template::tests::*`:~28 个(parse + resolve + validate)
- `w8_dag_repo_smoke`:7 个
- `w8_template_unit`:9 个
- 合计 W8 Plan 1 新增 ≥ 50 个测试

- [ ] **Step 4: 更新 `docs/PROGRESS.md`**

在 W7 整体段落后追加 W8 Plan 1 段落:

```markdown
> **W8 Plan 1:** ✅ 已完成(2026-07-26)— DAG 基础设施(SlotTemplateEngine 模板解析/渲染/校验 + DB 迁移 003 dag_plans/dag_nodes/task_explanations + DagRepo/TaskExplanationRepo CRUD),~10 个 commit,新增 ≥ 50 个测试(default cargo test --workspace 全绿),clippy `-D warnings` 0 警告,6 套 feature 组合 cargo check 全 PASS,详见 §二 W8 Plan 1 段落
```

更新里程碑表(§一):
```markdown
| W8 Plan 1 | DAG 基础设施: SlotTemplateEngine + DB 003 + DagRepo + TaskExplanationRepo | ✅ 已完成 | +≥50 (default) | 2026-07-26 | (direct on master) |
```

更新累计测试数:在原 236 基础上加 W8 Plan 1 新增数(具体数字按 Step 3 实际跑出的结果填)。

- [ ] **Step 5: Commit**

```powershell
git add docs/PROGRESS.md
git commit -m "docs(w8p1): update PROGRESS.md with W8 Plan 1 completion + test counts"
```

- [ ] **Step 6: 跑最终验收**

Run:
```powershell
cd d:\voicepilot\voicepilot
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --no-default-features
cargo check --workspace --features voice,tauri,llm,uia
```

Expected: 全 PASS,无回归,W8 Plan 1 验收门禁闭合。

---

## Self-Review

### 1. Spec coverage

| Spec 章节 | 覆盖 Task |
|---|---|
| §2.1 SlotTemplateEngine(占位符解析 + 校验) | Task 3-5, 9 |
| §2.3 DagPlan / DagNode / DagStatus 数据结构 | Task 2 |
| §2.6 数据库迁移 003 | Task 1 |
| §2.6 DagRepo / TaskExplanationRepo | Task 6, 7 |
| §6 安全约束(双层防御) | Task 5, 9 |
| §7 验收门禁(编译 + clippy + 测试) | Task 10 |

### 2. 已知偏离

- **`dag_plans.root_task_id` ON DELETE 策略**:spec §2.6 写 `ON DELETE CASCADE`,实现选 `SET NULL`(保留 DAG 历史便于审计回溯)。已在 Task 1 Step 1 注释中说明。
- **`ExtractedSlot.kind` 字段类型**:本 plan 中 `template::SlotKind` 与 `llm::types::SlotKind` 可能不同 — Task 4 Step 1 已提示确认,Task 9 Step 2 也提示调整。
- **Filter predicate 支持**:W8 仅支持 `[?size > N]` / `[?size < N]` / `[?size >= N]` / `[?size <= N]`(spec §8 延后项),Task 4-5 实现 + 测试。

### 3. Type consistency

- `SlotTemplate` 在 Task 2(dag_types.rs DagNode.input_template)和 Task 3(template.rs)定义一致
- `TemplateExpr` / `VarRef` / `VarScope` 在 Task 3 定义,Task 4-5 引用一致
- `DagPlan` / `DagNode` / `DagStatus` / `DagNodeStatus` 在 Task 2 定义,Task 6(DagRepo)引用一致
- `TaskExplanationRecord` / `FailureCategory` 在 Task 7 定义,Task 8 测试引用一致
- `MAX_TOTAL_STEPS_HARD_LIMIT` / `MAX_LOOP_ITERATIONS_HARD_LIMIT` 常量在 Task 2 定义,Plan 2-3 引用

---

## Execution Handoff

本 Master Plan(Plan 1)完成后,后续 Plan 2-6 的执行流程:

1. **Plan 2:** LlmClient::decompose_to_dag + DagExecutor 简单节点 + dispatch_skill_executor 路由
2. **Plan 3:** DagExecutor 循环节点 + form.submit 新 Skill + task.explain LLM 增强
3. **Plan 4:** Router Bridge 集成 RouteDecision::Dag 分支
4. **Plan 5:** UI: DAG 骨架审批弹窗 + DAG 历史 + task.explain 面板
5. **Plan 6:** 集成测试 + 6 套 feature 组合 cargo check 矩阵 + clippy + npm build

每个 Plan 完成后,更新 `docs/PROGRESS.md` 并 commit。
