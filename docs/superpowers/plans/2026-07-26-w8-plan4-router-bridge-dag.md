# W8 Plan 4: Router Bridge 集成 RouteDecision::Dag 分支 + route_text_with_dag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 W7 `route_text` 单 Skill 路由的基础上,新增 `route_text_with_dag` 函数:关键词优先 → LLM DAG 拆解(W8 新) → W7 `route_with_llm` 回退三级路由策略,并通过 `RouteDecision::Dag` + `RouteOutcome::DagPlan` 两个新变体把 DAG 骨架暴露给 CLI / Tauri 调用方,以便触发 DAG 骨架审批 UI(Plan 5 实现)。本 Plan 不实现 DagExecutor 本身(Plan 2/3 已完成),仅做 router_bridge 集成层。

**Architecture:** 三层路由策略 — (1) **关键词优先**:`SkillRouter::route(text)` 同步命中 `RouteDecision::Skill` / `SkillWithSlots` 时直接返回,不调 LLM(W7 §2.2 算法,keyword_score >= 0.5);(2) **LLM 拆解**:关键词未命中且 `kernel.llm_client().is_enabled()` + `!kernel.privacy_mode()` 时,调 `LlmClient::decompose_to_dag(text, skills, slots)` → `SlotTemplateEngine::validate_dag(&dag)` 双层防御 #1(spec §2.1 / §6)→ 返回 `RouteDecision::Dag(DagPlan)`;任何错误(HTTP / 解析 / 校验)catch 后回退到第 3 级;(3) **W7 回退**:调 `SkillRouter::route_with_llm(text).await` 走 W7 关键词 + LLM 单 Skill fallback。`TrustKernel` 新增 `llm_client: Mutex<Option<Arc<LlmClient>>>` 字段(`#[cfg(feature = "llm")]` 门控)+ `llm_client() -> Option<Arc<LlmClient>>` accessor + `privacy_mode() -> bool` 便捷 accessor(读 `app_config.privacy.mode`)。`RouteOutcome` 新增 `DagPlan(DagPlan)` 变体,供 CLI / Tauri 调用方判断是否需要 DAG 骨架审批 UI。

**Tech Stack:** Rust(stable),`tokio`(异步 LLM 调用),`wiremock 0.6`(HTTP mock,dev-dependency 已存在),`serde_json`(DagPlan 序列化),TDD,feature gate(`voice` + `llm`)。

**Spec:** `docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md` §2.8(Router Bridge 集成)+ §2.1(SlotTemplateEngine 双层防御)+ §2.2(decompose_to_dag 签名)+ §6(安全约束:privacy_mode / max_total_steps / LLM 不可绕过审批)

**Precondition:**
- W8 Plan 1 已完成:`crates/trust-kernel/src/skills/dag_types.rs` 定义 `DagPlan` / `DagNode` / `DagStatus` 等;`crates/trust-kernel/src/skills/template.rs` 定义 `SlotTemplateEngine::validate_dag`;DB 迁移 `003_dag_plans.sql` 已应用
- W8 Plan 2 已完成:`LlmClient::decompose_to_dag(text, skills, slots) -> Result<DagPlan>` 签名实现(spec §2.2);`SkillRouter` 暴露 `skills() -> &[SkillManifest]` accessor(若无,Plan 4 Task 1 补)
- W8 Plan 3 已完成:`form.submit` + `task.explain` LLM 增强(本 Plan 不直接依赖,但 Plan 5 UI 会用)
- W7 已完成(`commit 326e781` + 收尾 `957d40d`),`cargo check --workspace --features voice,tauri,llm,uia` PASS,clippy `-D warnings` 0 警告
- 现有代码状态:
  - `RouteDecision` 在 `crates/trust-kernel/src/skills/router.rs:26-32` 有 `Skill` / `SkillWithSlots(cfg llm)` / `Planner` 三个变体
  - `RouteOutcome` 在 `crates/trust-kernel/src/voice/router_bridge.rs:16-26` 有 `Routed { skill_id }` / `Unmatched { text }` / `Empty` 三个变体
  - `route_text(kernel, approver, text) -> Result<RouteOutcome>` 是同步函数(W5 PoC)
  - `TrustKernel` 在 `crates/trust-kernel/src/kernel.rs:19-32` 不持有 `LlmClient`;`config_repo()` 返回 `ConfigRepo` accessor;`conn()` 返回 `MutexGuard<'_, Connection>`
  - `LlmClient::is_enabled()` 检查 `api_key` + `base_url` 非空;`LlmClient::disabled()` 返回 no-op 客户端
  - `app_config` 表有 `privacy.mode` key(W6b-2 Settings),value 为 `"true"` / `"false"` 字符串

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Modify** `src/skills/router.rs` — 加 `RouteDecision::Dag(DagPlan)` 变体(`#[cfg(feature = "llm")]` 门控,与 `SkillWithSlots` 一致);加 `SkillRouter::skills() -> &[SkillManifest]` accessor
- **Modify** `src/kernel.rs` — 加 `llm_client: std::sync::Mutex<Option<Arc<LlmClient>>>` 字段(`#[cfg(feature = "llm")]` 门控);加 `llm_client() -> Option<Arc<LlmClient>>` accessor;加 `set_llm_client(Option<Arc<LlmClient>>)` setter;加 `privacy_mode() -> bool` 便捷 accessor(读 `app_config.privacy.mode`)
- **Modify** `src/voice/router_bridge.rs` — 加 `RouteOutcome::DagPlan(DagPlan)` 变体;加 `route_text_with_dag(text, kernel) -> Result<RouteOutcome>` async 函数(三级路由策略);保留 W7 `route_text` 向后兼容
- **Modify** `src/voice/mod.rs` — 无改动(`router_bridge` 模块已声明)

### CLI(`voicepilot/crates/cli/`)

- **Modify** `src/main.rs` — 加 `voice-dag <text>` 子命令:调用 `route_text_with_dag` → 根据 `RouteOutcome` 分支打印(skill_id / DAG 节点列表 / Unmatched / Empty);DAG 分支询问 Allow/Deny(简单的 y/N stdin,实际执行由 Plan 6 集成测试覆盖)

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan4_router_bridge_dag.rs` — `route_text_with_dag` 的 8 个 wiremock 集成测试场景:
  1. 关键词命中 → 直接返回 Skill(不调 LLM)
  2. 关键词未命中 + LLM disabled → 返回 Unmatched(Planner)
  3. 关键词未命中 + privacy_mode=true → 不调 LLM,返回 Unmatched
  4. 关键词未命中 + LLM enabled + 拆解成功 → 返回 DagPlan
  5. 关键词未命中 + LLM enabled + HTTP 失败 → 回退 route_with_llm(Unmatched)
  6. 关键词未命中 + LLM enabled + 校验失败(非法 skill_id) → 回退 route_with_llm(Unmatched)
  7. 关键词未命中 + LLM enabled + 拆解返回单节点 DAG → 返回 DagPlan(单节点也合法)
  8. 空字符串 → 返回 Empty

### Docs

- **Modify** `docs/PROGRESS.md` — W8 Plan 4 完成状态 + 测试统计

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行
- **TDD**:每个含逻辑的任务先写失败测试 → 跑 → 实现 → 跑通 → commit
- **Repo accessor pattern**:`kernel.config_repo()` / `kernel.skill_repo()` 等返回新实例,方法接收 `&Connection`
- **`&kernel.conn()` 不用 `&*kernel.conn()`**:clippy `explicit_auto_deref` lint,`MutexGuard` 自动 deref coercion 到 `&Connection`(见 project_memory.md "Engineering Conventions")
- **`Approver` import 完整路径**:`use crate::approval::approver::Approver;`(approval 模块未在 root re-export,见 project_memory.md)
- **TrustKernel 不是 Clone**:持有 `Mutex<Connection>`,`route_text_with_dag` 接收 `&TrustKernel`(不消耗)
- **LLM 代码 feature 门控**:`#[cfg(feature = "llm")]` 门控 `LlmClient` 相关代码;异步测试用 `#[tokio::test]`
- **`RouteDecision` 现有变体不能改签名**:W7 测试依赖 `Skill(Box<SkillManifest>)` / `SkillWithSlots(Box<SkillManifest>, Vec<ExtractedSlot>)` / `Planner` 签名,新增 `Dag` 变体不破坏 W7
- **`route_text_with_dag` 是 async**:LLM 调用是 async,函数签名 `async fn`,调用方需 tokio runtime(CLI 已有 `#[tokio::main]` 或在 main 中 `block_on`)
- **feature 组合**:`route_text_with_dag` 在 `voice` feature 下编译(因为 `voice/router_bridge.rs` 是 voice-gated);LLM 拆解分支在 `voice + llm` 双 feature 下编译;无 `llm` feature 时跳过 LLM 分支,直接走关键词 + Planner 回退
- **错误处理**:decompose_to_dag 失败 / validate_dag 失败 → catch Err,回退到 W7 单 Skill 路由(返回 `route_with_llm` 的结果);不向上传播 LLM 错误(用户感知:LLM 不可用时退化为 W7 行为)
- **`privacy_mode` 检查**:必须为 false 才调 LLM(spec §6 安全约束);`privacy_mode = true` 时强制走 W7 关键词路由,不发送任何网络请求
- **Commit message**:`feat(w8p4): ...` / `test(w8p4): ...` / `docs(w8p4): ...` / `refactor(w8p4): ...`
- **不引入新依赖**:本 plan 仅用 `tokio` + `wiremock`(dev)+ `serde_json`(均已在 workspace)
- **DagPlan 引用**:`use crate::skills::dag_types::DagPlan;`(Plan 1 已定义)
- **SlotTemplateEngine 引用**:`use crate::skills::template::SlotTemplateEngine;`(Plan 1 已定义)
- **LlmClient 引用**:`use crate::llm::client::LlmClient;`(`#[cfg(feature = "llm")]` 门控)
- **ExtractedSlot 引用**:`use crate::llm::types::ExtractedSlot;`(无 feature 门控,types 模块始终编译)

---

## Task 1: RouteDecision::Dag 变体 + SkillRouter::skills() accessor

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/router.rs:25-32`(RouteDecision enum)
- Modify: `voicepilot/crates/trust-kernel/src/skills/router.rs:42-142`(SkillRouter impl,加 skills() accessor)

**背景:** spec §2.8 要求 `RouteDecision` 加 `Dag(DagPlan)` 变体。`DagPlan` 在 Plan 1 的 `dag_types.rs` 已定义。本 Task 还需补 `SkillRouter::skills()` accessor,因为 `decompose_to_dag` 需要候选 Skill 列表(spec §2.2 签名:`decompose_to_dag(text, candidate_skills, user_slots)`)。

- [ ] **Step 1: 写失败测试 — RouteDecision::Dag 变体构造 + match**

打开 `voicepilot/crates/trust-kernel/src/skills/router.rs`,在文件末尾的 `#[cfg(test)] mod tests` 块中追加(在最后一个 `}` 前):

```rust
    // ===== W8 Plan 4 Task 1: RouteDecision::Dag 变体 =====

    #[cfg(feature = "llm")]
    #[test]
    fn route_decision_dag_variant_constructs_and_matches() {
        use crate::skills::dag_types::{DagNode, DagPlan, DagEdge};
        use crate::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
        use crate::policy::types::ELevel;
        use std::collections::HashMap;

        let node = DagNode {
            node_id: "n1".into(),
            skill_id: "note.capture".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("notepad".into()),
            },
            risk_ceiling: ELevel::E1,
        };
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![node],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        };
        let decision = RouteDecision::Dag(plan);
        match decision {
            RouteDecision::Dag(p) => {
                assert_eq!(p.plan_id, "p1");
                assert_eq!(p.nodes.len(), 1);
                assert_eq!(p.nodes[0].skill_id, "note.capture");
            }
            _ => panic!("expected Dag variant"),
        }
    }

    #[cfg(feature = "llm")]
    #[test]
    fn skill_router_skills_accessor_returns_registered() {
        let mut router = SkillRouter::new();
        router.register(files_organize_manifest());
        let skills = router.skills();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "files.organize");
    }
```

- [ ] **Step 2: 跑测试,确认失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features llm skills::router::tests::route_decision_dag_variant_constructs_and_matches`
Expected: FAIL(编译错误:`RouteDecision::Dag` 未定义 / `SkillRouter::skills` 方法不存在)

- [ ] **Step 3: 加 `RouteDecision::Dag` 变体 + `use` 导入**

修改 `voicepilot/crates/trust-kernel/src/skills/router.rs:25-32`,把 `RouteDecision` enum 改为:

```rust
#[derive(Debug, Clone)]
pub enum RouteDecision {
    Skill(Box<SkillManifest>),
    /// W7 新增:LLM 提取了 Slot,经 UI 反馈给用户修改/Apply 后再执行
    #[cfg(feature = "llm")]
    SkillWithSlots(Box<SkillManifest>, Vec<ExtractedSlot>),
    /// W8 Plan 4 新增:LLM 拆解为多步 DAG(spec §2.8)。
    /// 调用方(CLI / Tauri)收到此变体时应弹 DAG 骨架审批 UI(Plan 5 实现)。
    #[cfg(feature = "llm")]
    Dag(crate::skills::dag_types::DagPlan),
    Planner,
}
```

在文件顶部 `use` 区(第 16-23 行附近)追加:

```rust
#[cfg(feature = "llm")]
use crate::skills::dag_types::DagPlan;
```

注意:`Dag` 变体用 `crate::skills::dag_types::DagPlan` 全路径引用,避免 `DagPlan` 未使用时(无 `llm` feature)触发 unused import warning。或者用 `use` 导入 + `#[cfg(feature = "llm")]` 门控,二选一。本 plan 选 `use` 导入方式(更清晰)。

- [ ] **Step 4: 加 `SkillRouter::skills()` accessor**

在 `voicepilot/crates/trust-kernel/src/skills/router.rs` 的 `impl SkillRouter` 块中(第 42-142 行之间,`register` 方法后)追加:

```rust
    /// W8 Plan 4: 返回已注册的 Skill manifest 列表(供 LLM decompose_to_dag
    /// 作为候选 Skill 传入)。spec §2.2 `decompose_to_dag(text, candidate_skills, user_slots)`。
    pub fn skills(&self) -> &[SkillManifest] {
        &self.skills
    }
```

- [ ] **Step 5: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features llm skills::router::tests::`
Expected: PASS(所有 router 测试全绿,包括新增 2 个 W8 Plan 4 测试 + W7 既有测试无回归)

- [ ] **Step 6: 跑 default feature 测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib skills::router::tests::`
Expected: PASS(default feature = `["llm"]`,W7 既有测试全绿)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/router.rs
git commit -m "feat(w8p4): add RouteDecision::Dag variant + SkillRouter::skills() accessor"
```

---

## Task 2: TrustKernel::llm_client() + privacy_mode() accessors

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs:19-32`(struct 字段)
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs:47-126`(with_conn 初始化)
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs:34-175`(impl 块,加 accessors)

**背景:** spec §2.8 `route_text_with_dag` 需通过 `kernel.llm_client()` 拿到 `LlmClient`,通过 `kernel.privacy_mode()` 检查隐私模式。当前 `TrustKernel` 不持有 `LlmClient`(W7 把 LLM 状态管理放在 UI 层 `state.rs`)。本 Task 给 `TrustKernel` 加 `llm_client` 字段 + accessor + setter,以及 `privacy_mode()` 便捷 accessor(读 `app_config.privacy.mode`)。

- [ ] **Step 1: 写失败测试 — llm_client() 返回 None(默认)+ privacy_mode() 返回 false(默认)**

打开 `voicepilot/crates/trust-kernel/src/kernel.rs`,在文件末尾的 `#[cfg(test)] mod tests` 块中追加:

```rust
    // ===== W8 Plan 4 Task 2: llm_client + privacy_mode accessors =====

    #[cfg(feature = "llm")]
    #[test]
    fn llm_client_default_is_none() {
        use crate::llm::client::LlmClient;
        use std::sync::Arc;
        let kernel = TrustKernel::open_in_memory().unwrap();
        assert!(kernel.llm_client().is_none(), "default llm_client must be None");
    }

    #[cfg(feature = "llm")]
    #[test]
    fn llm_client_setter_round_trip() {
        use crate::llm::client::LlmClient;
        use std::sync::Arc;
        let kernel = TrustKernel::open_in_memory().unwrap();
        let llm = Arc::new(LlmClient::new(
            "https://api.deepseek.com/v1",
            "sk-test",
            "deepseek-chat",
        ));
        kernel.set_llm_client(Some(llm.clone()));
        let got = kernel.llm_client();
        assert!(got.is_some());
        assert!(got.as_ref().unwrap().is_enabled());
        assert_eq!(got.as_ref().unwrap().base_url(), "https://api.deepseek.com/v1");
    }

    #[cfg(feature = "llm")]
    #[test]
    fn llm_client_setter_clears() {
        use crate::llm::client::LlmClient;
        use std::sync::Arc;
        let kernel = TrustKernel::open_in_memory().unwrap();
        let llm = Arc::new(LlmClient::new("https://x", "sk", "m"));
        kernel.set_llm_client(Some(llm));
        assert!(kernel.llm_client().is_some());
        kernel.set_llm_client(None);
        assert!(kernel.llm_client().is_none());
    }

    #[test]
    fn privacy_mode_default_is_false() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        assert!(!kernel.privacy_mode(), "default privacy_mode must be false");
    }

    #[test]
    fn privacy_mode_reads_true_from_kv() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");
        {
            let kernel = TrustKernel::open_file(path).unwrap();
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(&conn, "privacy.mode", "true")
                .unwrap();
        }
        let kernel = TrustKernel::open_file(path).unwrap();
        assert!(kernel.privacy_mode(), "privacy_mode=true must be read from KV");
    }

    #[test]
    fn privacy_mode_reads_false_from_kv() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");
        {
            let kernel = TrustKernel::open_file(path).unwrap();
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(&conn, "privacy.mode", "false")
                .unwrap();
        }
        let kernel = TrustKernel::open_file(path).unwrap();
        assert!(!kernel.privacy_mode());
    }

    #[test]
    fn privacy_mode_treats_invalid_as_false() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");
        {
            let kernel = TrustKernel::open_file(path).unwrap();
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(&conn, "privacy.mode", "not-a-bool")
                .unwrap();
        }
        let kernel = TrustKernel::open_file(path).unwrap();
        assert!(!kernel.privacy_mode(), "invalid privacy.mode value must default to false");
    }
```

- [ ] **Step 2: 跑测试,确认失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features llm kernel::tests::llm_client_default_is_none`
Expected: FAIL(编译错误:`method llm_client` not found in `TrustKernel`)

- [ ] **Step 3: 加 `llm_client` 字段到 `TrustKernel` struct**

修改 `voicepilot/crates/trust-kernel/src/kernel.rs:19-32`,在 `allowed_apps` 字段后追加 `llm_client` 字段:

```rust
pub struct TrustKernel {
    conn: Arc<Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
    gateway: Arc<crate::gateway::ActionGateway>,
    fs: Arc<std::sync::Mutex<crate::tools::fs::FilesystemTool>>,
    comp_repo: Arc<crate::compensation::repo::CompensationRepo>,
    approval_repo: Arc<ApprovalRepo>,
    txn_mgr: Arc<crate::policy::transaction::TransactionManager>,
    // W7 Plan 4: UIA 应用白名单
    allowed_apps: Arc<std::sync::Mutex<Vec<String>>>,
    // W8 Plan 4: LLM 客户端(可选,None = 不调 LLM)。
    // `#[cfg(feature = "llm")]` 门控:无 llm feature 时不持有 LlmClient,
    // route_text_with_dag 直接走关键词 + Planner 回退(spec §6)。
    // 用 `Mutex<Option<Arc<LlmClient>>>` 而非 `Arc<Mutex<...>>`:kernel 是唯一 owner,
    // 不需要 Arc 共享;但 LlmClient 内部可能有 reqwest::Client(不可 Clone),
    // 用 Arc<LlmClient> 让 setter / getter 不需要 ownership transfer。
    #[cfg(feature = "llm")]
    llm_client: std::sync::Mutex<Option<Arc<crate::llm::client::LlmClient>>>,
}
```

在文件顶部 `use` 区追加(若 `Arc` 尚未 import):

```rust
#[cfg(feature = "llm")]
use std::sync::Arc;
```

注意:`Arc` 在 kernel.rs 已经 `use std::sync::{Arc, Mutex};` 导入(第 16 行),所以无需重复导入。检查实际 `use` 语句后调整。

- [ ] **Step 4: 在 `with_conn` 初始化 `llm_client` 字段**

修改 `voicepilot/crates/trust-kernel/src/kernel.rs:54-70`(kernel struct 字面量),在 `allowed_apps: Arc::new(...)` 后追加:

```rust
        let kernel = Self {
            conn: shared.clone(),
            task_repo: TaskRepo::new(),
            audit: Arc::new(SqliteAuditLogger::new(shared)),
            gateway,
            fs: Arc::new(std::sync::Mutex::new(crate::tools::fs::FilesystemTool::new())),
            comp_repo: Arc::new(crate::compensation::repo::CompensationRepo::new()),
            approval_repo: Arc::new(ApprovalRepo::new()),
            txn_mgr: Arc::new(crate::policy::transaction::TransactionManager::new()),
            allowed_apps: Arc::new(std::sync::Mutex::new(vec![
                "notepad".to_string(),
                "explorer".to_string(),
                "calc".to_string(),
            ])),
            // W8 Plan 4: 默认无 LLM 客户端(None)。Settings 面板在 LLM 启用时
            // 调 `set_llm_client(Some(Arc::new(LlmClient::new(...))))` 注入。
            #[cfg(feature = "llm")]
            llm_client: std::sync::Mutex::new(None),
        };
```

- [ ] **Step 5: 加 `llm_client()` accessor + `set_llm_client()` setter + `privacy_mode()` accessor**

在 `voicepilot/crates/trust-kernel/src/kernel.rs` 的 `impl TrustKernel` 块中,在 `allowed_apps()` / `set_allowed_apps()` 方法后(约第 165 行附近)追加:

```rust
    // ===== W8 Plan 4: LLM client + privacy_mode accessors =====

    /// W8 Plan 4: 返回当前 LLM 客户端的 Arc 克隆(若有)。
    /// `route_text_with_dag` 用此方法判断是否调 LLM 拆解 DAG。
    /// 返回 `Option<Arc<LlmClient>>` 而非 `Option<&LlmClient>`:内部用
    /// `Mutex<Option<Arc<LlmClient>>>` 存储,返回引用需要持有 guard,API 不便;
    /// 克隆 Arc(参考计数 +1)开销极低,调用方拿到 Arc 后可自由持有。
    #[cfg(feature = "llm")]
    pub fn llm_client(&self) -> Option<Arc<crate::llm::client::LlmClient>> {
        let guard = self.llm_client.lock().unwrap();
        guard.clone()
    }

    /// W8 Plan 4: 注入或清除 LLM 客户端。
    /// Settings 面板 `update_settings_command` 在 LLM 启用且 api_key 非空时
    /// 调 `set_llm_client(Some(Arc::new(LlmClient::new(...))))`;
    /// privacy_mode 切换为 true 或 LLM 禁用时调 `set_llm_client(None)`。
    #[cfg(feature = "llm")]
    pub fn set_llm_client(&self, client: Option<Arc<crate::llm::client::LlmClient>>) {
        let mut guard = self.llm_client.lock().unwrap();
        *guard = client;
    }

    /// W8 Plan 4: 读取 privacy_mode(spec §6 安全约束)。
    /// 从 `app_config.privacy.mode` 读取,value="true" → true,其他 → false。
    /// 读取失败 / key 缺失 / value 非法 → 默认 false(保守策略)。
    /// `route_text_with_dag` 在 privacy_mode=true 时禁止调 LLM 拆解。
    pub fn privacy_mode(&self) -> bool {
        let conn = self.conn();
        match self.config_repo().get(&conn, "privacy.mode") {
            Ok(Some(v)) => v.trim().eq_ignore_ascii_case("true"),
            _ => false,
        }
    }
```

- [ ] **Step 6: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features llm kernel::tests::llm_client kernel::tests::privacy_mode`
Expected: PASS(7 个测试全绿:llm_client_default_is_none / llm_client_setter_round_trip / llm_client_setter_clears / privacy_mode_default_is_false / privacy_mode_reads_true_from_kv / privacy_mode_reads_false_from_kv / privacy_mode_treats_invalid_as_false)

- [ ] **Step 7: 跑 default feature 测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib kernel::tests::`
Expected: PASS(W7 既有测试 + W8 Plan 4 新增测试全绿)

- [ ] **Step 8: 跑 no-default-features 测试,确认无 llm feature 时编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --no-default-features`
Expected: PASS(无 `llm` feature 时 `llm_client` 字段 / accessor / setter 全部 cfg-gated 不编译)

- [ ] **Step 9: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/kernel.rs
git commit -m "feat(w8p4): add TrustKernel llm_client field/accessor/setter + privacy_mode accessor"
```

---

## Task 3: route_text_with_dag 骨架 + 关键词优先分支

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs:1-83`(在 route_text 后追加 route_text_with_dag)

**背景:** spec §2.8 三级路由策略的第 1 级 — 关键词优先。`SkillRouter::route(text)` 同步匹配,命中 `Skill` / `SkillWithSlots` 时直接返回 `RouteOutcome::Routed`,不调 LLM。本 Task 实现骨架 + 第 1 级 + Empty 短路。

- [ ] **Step 1: 写失败测试 — 关键词命中 + 空字符串**

创建 `voicepilot/crates/trust-kernel/tests/w8_plan4_router_bridge_dag.rs`:

```rust
//! W8 Plan 4 Task 3-6: route_text_with_dag 集成测试.
//!
//! 覆盖 spec §2.8 三级路由策略的全部 8 个场景。
//! Task 3 仅写场景 1(keyword 命中)+ 场景 8(空字符串)作为失败测试。

#![cfg(all(feature = "voice", feature = "llm"))]

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::voice::router_bridge::{route_text_with_dag, RouteOutcome};

#[tokio::test]
async fn scenario_1_keyword_match_returns_skill_without_llm() {
    // "整理下载目录" 命中 files.organize 的 keyword "整理" → 直接返回 Routed,
    // 不调 LLM(用 disabled LlmClient 验证:即使 LLM 不可用,keyword 命中仍工作)。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let outcome = route_text_with_dag(&kernel, "整理下载目录").await.unwrap();
    match outcome {
        RouteOutcome::Routed { skill_id } => {
            assert_eq!(skill_id, "files.organize");
        }
        other => panic!("expected Routed, got {:?}", other),
    }
}

#[tokio::test]
async fn scenario_8_empty_string_returns_empty() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let outcome = route_text_with_dag(&kernel, "").await.unwrap();
    assert!(matches!(outcome, RouteOutcome::Empty), "expected Empty, got {:?}", outcome);

    let outcome2 = route_text_with_dag(&kernel, "   \t\n  ").await.unwrap();
    assert!(matches!(outcome2, RouteOutcome::Empty), "whitespace-only should be Empty");
}
```

- [ ] **Step 2: 跑测试,确认失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm`
Expected: FAIL(编译错误:`route_text_with_dag` not found in `voice::router_bridge`)

- [ ] **Step 3: 加 `RouteOutcome::DagPlan` 变体(暂时不用,Task 5 填充)**

修改 `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs:15-26`,把 `RouteOutcome` enum 改为:

```rust
#[derive(Debug)]
pub enum RouteOutcome {
    /// Skill matched. Caller (CLI) prompts user for args, then invokes
    /// `FilesOrganizeSkill::execute` to run the prepare→approve→commit flow.
    Routed {
        skill_id: String,
    },
    /// W8 Plan 4 新增:LLM 拆解为多步 DAG。Caller 应弹 DAG 骨架审批 UI
    /// (Plan 5 实现),Allow 后调 `DagExecutor::run` 执行。
    #[cfg(feature = "llm")]
    DagPlan(trust_kernel::skills::dag_types::DagPlan),
    /// No skill matched; caller should fall back to LLM Planner (W7).
    Unmatched { text: String },
    /// Input was empty/whitespace.
    Empty,
}
```

- [ ] **Step 4: 实现 `route_text_with_dag` 骨架 + 关键词优先分支 + Empty 短路**

在 `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs` 末尾(`#[cfg(test)] mod tests` 块前)追加:

```rust
/// W8 Plan 4: 三级路由策略(spec §2.8)。
///
/// 1. **关键词优先**:SkillRouter::route(text) 同步命中 → 直接返回 Routed,不调 LLM。
///    匹配 W7 §2.2 算法:keyword_score >= 0.5 直接返回 Skill。
/// 2. **LLM 拆解**(W8 新):关键词未命中且 LLM 启用 + !privacy_mode 时,
///    调 LlmClient::decompose_to_dag → SlotTemplateEngine::validate_dag
///    双层防御(spec §2.1 / §6)→ 返回 DagPlan。
///    任何错误(HTTP / 解析 / 校验)catch 后回退到第 3 级。
/// 3. **W7 回退**:调 route_with_llm 走 W7 关键词 + LLM 单 Skill fallback。
///
/// 与 W7 `route_text` 的区别:
/// - async(LLM 调用是 async)
/// - 不接收 `approver` 参数(本函数只路由不执行,DAG 执行时由 DagExecutor 自带 approver)
/// - 返回 `RouteOutcome` 而非 `RouteDecision`(便于调用方直接处理 DAG / Routed / Unmatched)
///
/// # Errors
/// - `KernelError::Db` 当 KV 读取 privacy_mode 失败时(保守策略:返回 Err 让上层处理)
/// - 其他错误均被 catch,回退到 W7 单 Skill 路由(返回 route_with_llm 结果)
#[cfg(feature = "voice")]
pub async fn route_text_with_dag(
    kernel: &TrustKernel,
    text: &str,
) -> crate::error::Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    // 构建 SkillRouter,注册 W7 全部 built-in Skills(与 route_text 一致)。
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    router.register(task_repeat_verified_manifest());
    // 注册顺序:task_compensate 必须在 task_explain 之前(与 route_text 一致)。
    router.register(task_compensate_manifest());
    router.register(task_explain_manifest());
    #[cfg(all(windows, feature = "uia"))]
    {
        use crate::skills::manifest::{app_control_manifest, note_capture_manifest};
        router.register(app_control_manifest());
        router.register(note_capture_manifest());
    }
    router.register(research_save_manifest());
    router.register(form_prepare_manifest());

    // ===== 第 1 级:关键词优先(spec §2.8)=====
    // SkillRouter::route 是同步方法,只做 keyword + intent_example 匹配,不调 LLM。
    // 命中 Skill / SkillWithSlots → 直接返回 Routed,跳过 LLM 拆解。
    let keyword_decision = router.route(trimmed);
    match keyword_decision {
        RouteDecision::Skill(manifest) => {
            return Ok(RouteOutcome::Routed {
                skill_id: manifest.id,
            });
        }
        #[cfg(feature = "llm")]
        RouteDecision::SkillWithSlots(manifest, _slots) => {
            return Ok(RouteOutcome::Routed {
                skill_id: manifest.id,
            });
        }
        #[cfg(feature = "llm")]
        RouteDecision::Dag(_) => {
            // route() 是同步方法,从不返回 Dag;此处 unreachable,保持防御
        }
        RouteDecision::Planner => { /* 落到第 2 级 */ }
    }

    // ===== 第 2 级 + 第 3 级:LLM 拆解 / W7 回退(Task 4-5 填充)=====
    // 暂时回退到 Planner(返回 Unmatched),Task 4 实现 LLM 拆解,Task 5 接 route_with_llm。
    #[cfg(feature = "llm")]
    {
        // Task 4-5 在此填 LLM 拆解 + 回退逻辑。
        // 暂时返回 Unmatched 让集成测试 scenario_1 / scenario_8 通过(它们不依赖 LLM 分支)。
    }
    Ok(RouteOutcome::Unmatched {
        text: trimmed.to_string(),
    })
}
```

注意:
- `route_text_with_dag` 是 `async fn`,即使本 Task 内部没 `await`,签名仍为 async(Task 4-5 会用 `.await`)。
- `#[cfg(feature = "voice")]` 门控:本函数在 `voice/router_bridge.rs`,文件本身 voice-gated,但显式标注更清晰。
- 关键词优先分支完整:命中 `Skill` / `SkillWithSlots` 直接返回,不调 LLM。
- `RouteDecision::Dag(_)` arm 是防御性:同步 `route()` 从不返回 `Dag`,但 enum 完整性要求 match 穷尽。

- [ ] **Step 5: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm`
Expected: PASS(scenario_1 + scenario_8 全绿;其他场景尚未写,测试文件只有 2 个测试)

- [ ] **Step 6: 跑 W7 既有 router_bridge 测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features voice voice::router_bridge::tests::`
Expected: PASS(W7 的 `route_text_*` 测试全绿,`route_text` 函数未被破坏)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/voice/router_bridge.rs voicepilot/crates/trust-kernel/tests/w8_plan4_router_bridge_dag.rs
git commit -m "feat(w8p4): add route_text_with_dag skeleton with keyword-priority branch + Empty short-circuit"
```

---

## Task 4: LLM 拆解分支 + validate_dag 双层防御

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`(route_text_with_dag 的第 2 级)

**背景:** spec §2.8 第 2 级 — 关键词未命中且 LLM 启用 + !privacy_mode 时,调 `LlmClient::decompose_to_dag` 拆解为 DAG,然后 `SlotTemplateEngine::validate_dag` 双层防御 #1(spec §2.1 / §6)。校验失败 → catch Err,回退到第 3 级(Task 5 实现)。

- [ ] **Step 1: 写失败测试 — 场景 3 (privacy_mode=true 不调 LLM) + 场景 4 (拆解成功) + 场景 7 (单节点 DAG)**

在 `voicepilot/crates/trust-kernel/tests/w8_plan4_router_bridge_dag.rs` 末尾追加:

```rust
use std::sync::Arc;
use trust_kernel::llm::client::LlmClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 构造一个 mock LLM server,返回给定 JSON body(用于 decompose_to_dag 测试)。
async fn mock_llm_server_decompose(body: serde_json::Value) -> (MockServer, String) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let uri = server.uri();
    (server, uri)
}

/// decompose_to_dag 的成功 mock 响应:2 节点 DAG(note.capture → files.move)。
fn decompose_success_body_two_nodes() -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": "{\"nodes\":[{\"node_id\":\"n1\",\"skill_id\":\"note.capture\",\"input_template\":{\"kind\":\"text\",\"template\":\"notepad\"},\"risk_ceiling\":\"E1\"},{\"node_id\":\"n2\",\"skill_id\":\"files.move\",\"input_template\":{\"kind\":\"path\",\"template\":\"${prev.output.path}\"},\"risk_ceiling\":\"E2\"}],\"edges\":[{\"from\":\"n1\",\"to\":\"n2\",\"port_binding\":\"output.path -> input.source\"}],\"loop_specs\":{},\"max_total_steps\":5}"
                    }
                }]
            }
        }]
    })
}

/// decompose_to_dag 的成功 mock 响应:1 节点 DAG(单节点也合法,spec §2.2)。
fn decompose_success_body_single_node() -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": "{\"nodes\":[{\"node_id\":\"n1\",\"skill_id\":\"note.capture\",\"input_template\":{\"kind\":\"text\",\"template\":\"notepad\"},\"risk_ceiling\":\"E1\"}],\"edges\":[],\"loop_specs\":{},\"max_total_steps\":3}"
                    }
                }]
            }
        }]
    })
}

#[tokio::test]
async fn scenario_3_privacy_mode_true_skips_llm() {
    // privacy_mode=true 时即使 LLM 启用也不调用,回退到 W7 Planner。
    let kernel = TrustKernel::open_in_memory().unwrap();
    {
        let conn = kernel.conn();
        kernel.config_repo().set(&conn, "privacy.mode", "true").unwrap();
    }
    // LLM 启用(api_key 非空)— 但 privacy_mode 应阻止调用
    let llm = Arc::new(LlmClient::new(
        "https://invalid.example.com",
        "sk-test",
        "test",
    ));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理").await.unwrap();
    // privacy_mode=true → 不调 LLM → 回退 W7 route_with_llm
    // W7 route_with_llm 调 invalid URL 失败 → Planner → Unmatched
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!("privacy_mode=true should fall back to Unmatched, got {:?}", other),
    }
}

#[tokio::test]
async fn scenario_4_llm_decompose_success_returns_dag_plan() {
    let (server, uri) = mock_llm_server_decompose(decompose_success_body_two_nodes()).await;
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "打开记事本写 TODO 然后保存到桌面").await.unwrap();
    match outcome {
        RouteOutcome::DagPlan(plan) => {
            assert_eq!(plan.nodes.len(), 2);
            assert_eq!(plan.nodes[0].skill_id, "note.capture");
            assert_eq!(plan.nodes[1].skill_id, "files.move");
            assert_eq!(plan.edges.len(), 1);
            assert_eq!(plan.edges[0].from, "n1");
            assert_eq!(plan.edges[0].to, "n2");
            assert_eq!(plan.max_total_steps, 5);
        }
        other => panic!("expected DagPlan, got {:?}", other),
    }
}

#[tokio::test]
async fn scenario_7_llm_decompose_single_node_dag_returns_dag_plan() {
    let (server, uri) = mock_llm_server_decompose(decompose_success_body_single_node()).await;
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "打开记事本").await.unwrap();
    match outcome {
        RouteOutcome::DagPlan(plan) => {
            assert_eq!(plan.nodes.len(), 1, "single-node DAG is valid (spec §2.2)");
            assert_eq!(plan.nodes[0].skill_id, "note.capture");
        }
        other => panic!("expected DagPlan for single-node DAG, got {:?}", other),
    }
}
```

- [ ] **Step 2: 跑测试,确认失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm scenario_3 scenario_4 scenario_7`
Expected: FAIL(scenario_3:返回 Unmatched ✓ 可能通过;scenario_4 / scenario_7:返回 Unmatched 而非 DagPlan,因为 LLM 拆解分支未实现)

- [ ] **Step 3: 实现 LLM 拆解分支**

修改 `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs` 的 `route_text_with_dag` 函数,把 `// ===== 第 2 级 + 第 3 级 =====` 注释后的占位代码替换为:

```rust
    // ===== 第 2 级:LLM 拆解(spec §2.8)=====
    // 关键词未命中且 LLM 启用 + !privacy_mode 时,尝试拆解为多步 DAG。
    // 任何错误(HTTP / 解析 / 校验)catch 后回退到第 3 级 route_with_llm。
    #[cfg(feature = "llm")]
    {
        if let Some(llm) = kernel.llm_client() {
            if llm.is_enabled() && !kernel.privacy_mode() {
                // spec §6.1 审计事件:llm_decompose_called(LLM 拆解调用)
                // 在调用前预创建临时 task_id(audit_logs.task_id NOT NULL + FK),
                // goal 标明"LLM decompose for: ..."。LLM 成功时此 task 留在 tasks 表
                // 作为 LLM 调用历史(DagExecutor::run 会创建自己的 root_task_id,
                // 不复用此临时 task);LLM 失败时同样保留作为审计回溯依据。
                let llm_task_id = format!("task-llm-{}", uuid::Uuid::new_v4());
                kernel.create_task(&llm_task_id, &format!("LLM decompose for: {}", trimmed))?;
                let started = std::time::Instant::now();

                // spec §2.2:decompose_to_dag(text, candidate_skills, user_slots)
                // user_slots 暂传空 slice(W7 route_with_llm 也是 LLM 内部提取 slots,
                // 不预先 regex 解析);Plan 5+ 视需要补 slot_parser 模块。
                let user_slots: Vec<crate::llm::types::ExtractedSlot> = Vec::new();
                match llm
                    .decompose_to_dag(trimmed, router.skills(), &user_slots)
                    .await
                {
                    Ok(dag) => {
                        // 审计 — llm_decompose_called(成功,spec §6.1)
                        // 关键字段:plan_id(LLM 返回的真实 plan_id)/ llm_model /
                        // latency_ms(实际耗时)/ token_count(W9+ 由 LLM provider 返回真实计数,
                        // 当前占位 0)
                        let latency_ms = started.elapsed().as_millis() as i64;
                        let _ = kernel.audit_append_external(
                            &llm_task_id,
                            None,
                            "llm_decompose_called",
                            serde_json::json!({
                                "plan_id": dag.plan_id,
                                "llm_model": llm.model(),
                                "latency_ms": latency_ms,
                                "token_count": 0i64,
                            }),
                        );

                        // 双层防御 #1(spec §2.1 / §6):LLM 返回后立即校验。
                        // validate_dag 检查:node_id 引用 / slot kind / iter 仅在循环节点 /
                        // filter predicate 支持。
                        match crate::skills::template::SlotTemplateEngine::validate_dag(&dag) {
                            Ok(()) => {
                                return Ok(RouteOutcome::DagPlan(dag));
                            }
                            Err(e) => {
                                // 校验失败:回退到 W7 route_with_llm(spec §2.8 错误处理)。
                                tracing::warn!(
                                    error = %e,
                                    "validate_dag failed; falling back to W7 route_with_llm"
                                );
                                // 落到第 3 级(下方 route_with_llm 调用)
                            }
                        }
                    }
                    Err(e) => {
                        // decompose_to_dag 失败(HTTP / 解析 / 超时):回退到 W7 route_with_llm。
                        // 不发 llm_decompose_called 审计事件(LLM 调用未成功,
                        // tracing::warn! 已记录错误;审计链路在 DagExecutor::run 内
                        // 通过 dag_plan_created → dag_completed 闭合,LLM 失败路径
                        // 不进入 DagExecutor,无审计缺口)。
                        tracing::warn!(
                            error = ?e,
                            "decompose_to_dag failed; falling back to W7 route_with_llm"
                        );
                        // 落到第 3 级
                    }
                }
            }
        }
    }

    // ===== 第 3 级:W7 route_with_llm 回退(Task 5 填充)=====
    // 暂时返回 Unmatched;Task 5 替换为 router.route_with_llm(text).await 调用。
    Ok(RouteOutcome::Unmatched {
        text: trimmed.to_string(),
    })
```

注意:
- `tracing::warn!` 记录 LLM 失败原因,不向上传播(用户感知:LLM 不可用时退化为 W7 行为)。
- `user_slots` 暂传空 slice(spec §2.8 的 `slot_parser::parse(text)` 是占位,Plan 5+ 视需要补)。
- `decompose_to_dag` 签名来自 Plan 2(spec §2.2):`async fn decompose_to_dag(text, skills, slots) -> Result<DagPlan>`。
- `validate_dag` 签名来自 Plan 1:`pub fn validate_dag(plan: &DagPlan) -> Result<(), TemplateError>`。
- **新增 spec §6.1 审计事件 `llm_decompose_called`**:LLM 成功时记录 `plan_id` / `llm_model`(Plan 2 Task 8 加的 `LlmClient::model()` accessor)/ `latency_ms`(实际耗时)/ `token_count`(0 占位,W9+ 由 LLM provider 返回真实计数)。LLM 失败时不发(避免审计噪音;`dag_plan_created` → `dag_completed` 链路在 DagExecutor::run 内闭合,LLM 失败路径不进入 DagExecutor)。
- **临时 task_id `task-llm-{uuid}`**:`audit_logs.task_id` NOT NULL + FK 约束要求先创建 task。LLM 调用前预创建 `task-llm-{uuid}`(goal="LLM decompose for: ..."),LLM 成功/失败均保留作为审计回溯依据(DagExecutor::run 创建自己的 `root_task_id`,不复用此临时 task)。`uuid` crate 已在 trust-kernel 依赖中(Plan 2 Task 7 `format!("task-dag-{}", uuid::Uuid::new_v4())` 已使用)。
- `kernel.create_task(&llm_task_id, ...)` 失败时返回 Err,直接 `?` 传播(route_text_with_dag 整体返回 Err,调用方处理);此情况下 LLM 不会被调用,无审计缺口。

- [ ] **Step 4: 跑测试,确认 scenario_4 / scenario_7 通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm scenario_4 scenario_7`
Expected: PASS(LLM mock 返回合法 DAG → validate_dag 通过 → 返回 DagPlan)

- [ ] **Step 5: 跑 scenario_3,确认 privacy_mode=true 不调 LLM**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm scenario_3`
Expected: PASS(privacy_mode=true → 跳过 LLM 分支 → 落到第 3 级 → 当前占位返回 Unmatched)

注意:scenario_3 此 Task 通过是因为第 3 级占位返回 Unmatched,符合预期。Task 5 实现真实 route_with_llm 后,scenario_3 仍应通过(invalid URL → LLM 失败 → Planner → Unmatched)。

- [ ] **Step 6: 跑全部已有测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm`
Expected: PASS(5 个场景全绿:scenario_1, scenario_3, scenario_4, scenario_7, scenario_8)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/voice/router_bridge.rs voicepilot/crates/trust-kernel/tests/w8_plan4_router_bridge_dag.rs
git commit -m "feat(w8p4): implement LLM decompose branch with validate_dag double-layer defense"
```

---

## Task 5: 错误回退到 route_with_llm + 完整第 3 级

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`(route_text_with_dag 的第 3 级)

**背景:** spec §2.8 第 3 级 — LLM 拆解失败 / 校验失败 / LLM disabled / privacy_mode=true 时,回退到 W7 `route_with_llm`。本 Task 把第 3 级占位替换为真实 `router.route_with_llm(text).await` 调用,并把 `RouteDecision` 转 `RouteOutcome`。

- [ ] **Step 1: 写失败测试 — scenario_2 (LLM disabled) + scenario_5 (HTTP 失败) + scenario_6 (校验失败)**

在 `voicepilot/crates/trust-kernel/tests/w8_plan4_router_bridge_dag.rs` 末尾追加:

```rust
#[tokio::test]
async fn scenario_2_llm_disabled_returns_unmatched() {
    // LLM disabled → 不走 LLM 拆解分支 → 回退 W7 route_with_llm
    // W7 route_with_llm 在 LLM disabled 时只做 keyword 匹配 → 不命中 → Planner → Unmatched
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::disabled());
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理").await.unwrap();
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!("LLM disabled should fall back to Unmatched, got {:?}", other),
    }
}

#[tokio::test]
async fn scenario_5_llm_http_failure_falls_back_to_route_with_llm() {
    // LLM 启用但 HTTP 失败(401)→ decompose_to_dag 失败 → 回退 route_with_llm
    // route_with_llm 也调 LLM(同一 URL)→ 同样 401 失败 → Planner → Unmatched
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let uri = server.uri();

    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-invalid", "test"));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理").await.unwrap();
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!("HTTP 401 should fall back to Unmatched, got {:?}", other),
    }
}

#[tokio::test]
async fn scenario_6_llm_validation_failure_falls_back_to_route_with_llm() {
    // LLM 返回非法 skill_id(unknown.skill)→ validate_dag 应失败 → 回退 route_with_llm。
    // 注:decompose_to_dag 内部应已校验 skill_id(spec §2.2 "不得引用未在 candidate_skills
    // 中的 skill_id"),若 Plan 2 实现严格则 decompose_to_dag 直接 Err。
    // 此测试构造一个 decompose_to_dag 成功但返回非法 DAG 的 mock(假设 Plan 2 校验宽松),
    // 验证 validate_dag 双层防御 #1 兜底。
    let body = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "decompose_to_dag",
                        // 引用 unknown.skill(不在 candidate_skills)+ 引用未声明节点 n99
                        "arguments": "{\"nodes\":[{\"node_id\":\"n1\",\"skill_id\":\"unknown.skill\",\"input_template\":{\"kind\":\"text\",\"template\":\"${n99.output.path}\"},\"risk_ceiling\":\"E1\"}],\"edges\":[],\"loop_specs\":{},\"max_total_steps\":3}"
                    }
                }]
            }
        }]
    });
    let (server, uri) = mock_llm_server_decompose(body).await;

    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理").await.unwrap();
    // 不论是 decompose_to_dag 内部校验失败还是 validate_dag 失败,
    // 都应 catch Err 并回退到 route_with_llm → 同一 mock 返回非法 DAG →
    // route_with_llm 的 classify_and_extract 也可能失败 → Planner → Unmatched
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!("validation failure should fall back to Unmatched, got {:?}", other),
    }
}
```

- [ ] **Step 2: 跑测试,确认 scenario_2 通过 + scenario_5 / scenario_6 失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm scenario_2 scenario_5 scenario_6`
Expected: scenario_2 PASS(LLM disabled → 跳过 LLM 分支 → 占位返回 Unmatched);scenario_5 / scenario_6 可能 PASS(占位也返回 Unmatched)或 FAIL(取决于第 3 级是否已实现)。

注意:由于 Task 4 的占位代码已返回 Unmatched,scenario_2 / scenario_5 / scenario_6 此 Step 可能全部通过。但这不证明第 3 级 route_with_llm 真的被调用 — Task 5 Step 3 实现真实 route_with_llm 后,需重跑确认行为一致。

- [ ] **Step 3: 实现第 3 级 route_with_llm 回退**

修改 `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs` 的 `route_text_with_dag` 函数,把 `// ===== 第 3 级 =====` 后的占位代码替换为:

```rust
    // ===== 第 3 级:W7 route_with_llm 回退(spec §2.8)=====
    // LLM 拆解失败 / 校验失败 / LLM disabled / privacy_mode=true 时,走 W7 单 Skill 路由。
    // route_with_llm 内部:keyword 匹配(已在第 1 级做过,此处冗余但 W7 逻辑保持)→
    // LLM classify_and_extract → Planner。
    //
    // 需要 LLM 启用的 router:若 kernel.llm_client() 为 Some,用 with_llm 构造新 router;
    // 否则用第 1 级的 keyword-only router(此时 route_with_llm 退化为 route,返回 Planner)。
    #[cfg(feature = "llm")]
    {
        // 重建带 LLM 的 router(route_with_llm 是 SkillRouter 方法,需 router 持有 LLM)
        let llm_router = build_router_with_llm(kernel, &router);
        let decision = llm_router.route_with_llm(trimmed).await;
        return Ok(decision_to_outcome(decision, trimmed));
    }

    // 无 llm feature 时:第 1 级 keyword 未命中 → 直接 Unmatched
    Ok(RouteOutcome::Unmatched {
        text: trimmed.to_string(),
    })
```

在 `route_text_with_dag` 函数后追加两个辅助函数:

```rust
/// 用 kernel.llm_client() 构造带 LLM 的 SkillRouter(若 LLM 不可用则用原 router)。
/// route_with_llm 是 SkillRouter 方法,需 router 持有 LlmClient。
#[cfg(all(feature = "voice", feature = "llm"))]
fn build_router_with_llm(
    kernel: &TrustKernel,
    keyword_router: &SkillRouter,
) -> SkillRouter {
    use std::sync::Arc;
    if let Some(llm) = kernel.llm_client() {
        // 用 with_llm 构造新 router,重新注册全部 Skills(与 keyword_router 一致)。
        let mut new_router = SkillRouter::with_llm(llm);
        for skill in keyword_router.skills() {
            new_router.register(skill.clone());
        }
        new_router
    } else {
        // LLM 不可用:用原 keyword-only router 的 clone(SkillRouter 是 Clone)。
        // route_with_llm 在 LLM=None 时退化为 keyword 匹配,行为与 route() 一致。
        keyword_router.clone()
    }
}

/// RouteDecision → RouteOutcome 转换。
#[cfg(feature = "voice")]
fn decision_to_outcome(decision: RouteDecision, text: &str) -> RouteOutcome {
    match decision {
        RouteDecision::Skill(manifest) => RouteOutcome::Routed {
            skill_id: manifest.id,
        },
        #[cfg(feature = "llm")]
        RouteDecision::SkillWithSlots(manifest, _slots) => RouteOutcome::Routed {
            skill_id: manifest.id,
        },
        #[cfg(feature = "llm")]
        RouteDecision::Dag(plan) => {
            // route_with_llm 不返回 Dag(它调 classify_and_extract 不是 decompose_to_dag);
            // 此 arm 防御性:若上游契约被破坏,把 Dag 转为 DagPlan 暴露给调用方。
            RouteOutcome::DagPlan(plan)
        }
        RouteDecision::Planner => RouteOutcome::Unmatched {
            text: text.to_string(),
        },
    }
}
```

注意:
- `build_router_with_llm` 在 LLM 不可用时 clone 原 router(SkillRouter derive Clone,见 router.rs:34-40)。但 `keyword_router` 是 `&SkillRouter`,clone 返回 owned `SkillRouter`。
- `decision_to_outcome` 把 W7 `RouteDecision` 转 `RouteOutcome`,Dag 变体防御性转 DagPlan(实际 route_with_llm 不会返回 Dag)。
- 无 `llm` feature 时:第 3 级直接返回 Unmatched(无 LLM 可调)。

- [ ] **Step 4: 跑全部 8 个场景,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm`
Expected: PASS(8 个场景全绿)

若 scenario_5 / scenario_6 失败,检查:
- scenario_5:LLM mock 返回 401 → decompose_to_dag 失败 → 回退 route_with_llm → route_with_llm 也调 LLM mock → 401 → LLM 失败 → Planner → Unmatched ✓
- scenario_6:LLM mock 返回非法 DAG → 可能 decompose_to_dag 内部校验失败(Plan 2 实现严格)→ 回退 route_with_llm → route_with_llm 调同一 mock → classify_and_extract 尝试解析为 LlmRouteResponse(可能 parse 失败,因为 mock 返回的是 decompose_to_dag schema 不是 route_skill schema)→ Planner → Unmatched ✓

若 scenario_6 因 mock schema 不匹配导致 route_with_llm 内部 parse 失败但仍返回 Planner,测试通过;若 route_with_llm panic 或返回 Skill,需调整 mock body 使 classify_and_extract 也失败(用空 choices 或缺 tool_calls 的 body)。

- [ ] **Step 5: 跑 W7 既有 router_bridge 测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features voice voice::router_bridge::tests::`
Expected: PASS(W7 `route_text_*` 测试全绿,`route_text` 函数未被破坏)

- [ ] **Step 6: 跑无 voice feature 测试,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --no-default-features`
Expected: PASS(无 voice feature 时 router_bridge.rs 不编译,route_text_with_dag 不存在)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/voice/router_bridge.rs voicepilot/crates/trust-kernel/tests/w8_plan4_router_bridge_dag.rs
git commit -m "feat(w8p4): implement route_with_llm fallback (level 3) with decision_to_outcome conversion"
```

---

## Task 6: RouteOutcome::DagPlan 变体完整化 + router_bridge 内联测试

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`(完善 RouteOutcome + 加内联单元测试)

**背景:** Task 3 已加 `RouteOutcome::DagPlan(DagPlan)` 变体骨架。本 Task 完善其 Debug / 部分 eq 行为,并在 router_bridge.rs 内联 `#[cfg(test)] mod tests` 块加单元测试,覆盖 `decision_to_outcome` 辅助函数 + RouteOutcome 变体构造。

- [ ] **Step 1: 写失败测试 — decision_to_outcome 单元测试**

在 `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs` 末尾的 `#[cfg(test)] mod tests` 块中追加(在最后一个 `}` 前):

```rust
    // ===== W8 Plan 4 Task 6: decision_to_outcome + RouteOutcome::DagPlan 单元测试 =====

    #[cfg(feature = "llm")]
    #[test]
    fn decision_to_outcome_skill_becomes_routed() {
        use crate::skills::manifest::files_organize_manifest;
        use crate::skills::router::RouteDecision;
        let manifest = Box::new(files_organize_manifest());
        let decision = RouteDecision::Skill(manifest);
        let outcome = decision_to_outcome(decision, "test");
        match outcome {
            RouteOutcome::Routed { skill_id } => {
                assert_eq!(skill_id, "files.organize");
            }
            other => panic!("expected Routed, got {:?}", other),
        }
    }

    #[cfg(feature = "llm")]
    #[test]
    fn decision_to_outcome_planner_becomes_unmatched() {
        use crate::skills::router::RouteDecision;
        let decision = RouteDecision::Planner;
        let outcome = decision_to_outcome(decision, "请讲解量子计算原理");
        match outcome {
            RouteOutcome::Unmatched { text } => {
                assert_eq!(text, "请讲解量子计算原理");
            }
            other => panic!("expected Unmatched, got {:?}", other),
        }
    }

    #[cfg(feature = "llm")]
    #[test]
    fn decision_to_outcome_dag_becomes_dag_plan() {
        use crate::skills::dag_types::{DagNode, DagPlan};
        use crate::skills::router::RouteDecision;
        use crate::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
        use crate::policy::types::ELevel;
        use std::collections::HashMap;
        let node = DagNode {
            node_id: "n1".into(),
            skill_id: "note.capture".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("notepad".into()),
            },
            risk_ceiling: ELevel::E1,
        };
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![node],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 3,
        };
        let decision = RouteDecision::Dag(plan);
        let outcome = decision_to_outcome(decision, "test");
        match outcome {
            RouteOutcome::DagPlan(p) => {
                assert_eq!(p.plan_id, "p1");
                assert_eq!(p.nodes.len(), 1);
            }
            other => panic!("expected DagPlan, got {:?}", other),
        }
    }

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn route_text_with_dag_preserves_w7_keyword_behavior() {
        // 集成测试:W7 route_text 已验证的 keyword 命中行为,route_text_with_dag 应保持。
        // "整理下载目录" 命中 files.organize keyword "整理"。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let outcome = route_text_with_dag(&kernel, "整理下载目录").await.unwrap();
        assert!(
            matches!(outcome, RouteOutcome::Routed { ref skill_id } if skill_id == "files.organize"),
            "expected Routed to files.organize, got {:?}",
            outcome
        );
    }

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn route_text_with_dag_no_llm_client_falls_back_to_planner() {
        // kernel 默认无 LLM client → 跳过 LLM 拆解 → 回退 route_with_llm
        // route_with_llm 在 LLM=None 时退化为 keyword 匹配 → 不命中 → Planner
        let kernel = TrustKernel::open_in_memory().unwrap();
        // 显式确保无 LLM
        kernel.set_llm_client(None);
        let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理").await.unwrap();
        assert!(
            matches!(outcome, RouteOutcome::Unmatched { .. }),
            "expected Unmatched when no LLM client, got {:?}",
            outcome
        );
    }
```

- [ ] **Step 2: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features voice,llm voice::router_bridge::tests::decision_to_outcome voice::router_bridge::tests::route_text_with_dag_preserves voice::router_bridge::tests::route_text_with_dag_no_llm_client`
Expected: PASS(5 个新测试全绿)

- [ ] **Step 3: 跑全部 router_bridge 测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --lib --features voice,llm voice::router_bridge::tests::`
Expected: PASS(W7 既有 4 个 `route_text_*` 测试 + W8 Plan 4 新增 5 个测试全绿)

- [ ] **Step 4: 跑集成测试,确认 8 个场景全绿**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan4_router_bridge_dag --features voice,llm`
Expected: PASS(8 个场景全绿)

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/voice/router_bridge.rs
git commit -m "test(w8p4): add decision_to_outcome unit tests + route_text_with_dag inline tests"
```

---

## Task 7: CLI voice-dag 子命令

**Files:**
- Modify: `voicepilot/crates/cli/src/main.rs`(加 voice-dag 子命令)

**背景:** W5 CLI 已存在 `voice route <text>` 子命令(同步,调 route_text)。本 Task 加 `voice-dag <text>` 子命令:调用 `route_text_with_dag`(async)→ 根据 RouteOutcome 分支打印。DAG 分支打印节点列表 + 询问 Allow/Deny(简单 y/N stdin;实际 DAG 执行由 Plan 6 集成测试覆盖,本 Plan 只到路由 + 打印)。

- [ ] **Step 1: 检查 CLI main.rs 是否有 tokio runtime**

打开 `voicepilot/crates/cli/src/main.rs`,检查 `fn main()` 是否是 `#[tokio::main]` 或在内部用 `block_on`。

W5 CLI 的 `fn main()` 是同步 `fn main() -> Result<()>`(第 44 行),不是 async。需要在调用 `route_text_with_dag` 时用 `tokio::runtime::Runtime::new().unwrap().block_on(async { ... })` 或把 main 改为 `#[tokio::main]`。

为最小改动,本 Task 用 `block_on` 在子命令内部创建 runtime(不全局改 main 签名,避免破坏其他同步子命令)。

- [ ] **Step 2: 写失败测试 — voice-dag 子命令 smoke 测试**

在 `voicepilot/crates/cli/src/main.rs` 末尾追加内联测试模块(若已有 `#[cfg(test)] mod tests`,在其内追加;否则新建):

```rust
#[cfg(test)]
mod w8_plan4_cli_tests {
    use super::*;
    use trust_kernel::kernel::TrustKernel;
    use trust_kernel::llm::client::LlmClient;
    use std::sync::Arc;

    /// smoke 测试:handle_voice_dag_command 在 keyword 命中时应打印 Routed。
    /// 不实际执行子进程,直接调内部函数(若 handle_voice_dag_command 是私有 fn,
    /// 测试模块在同一文件可访问)。
    #[test]
    fn voice_dag_keyword_match_prints_routed() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // 用 block_on 调 async 函数
        let rt = tokio::runtime::Runtime::new().unwrap();
        let output = rt.block_on(async {
            capture_handle_voice_dag_output(&kernel, "整理下载目录").await
        });
        assert!(output.contains("Routed"), "expected Routed in output, got: {}", output);
        assert!(output.contains("files.organize"), "expected skill_id in output, got: {}", output);
    }

    #[test]
    fn voice_dag_empty_input_prints_empty() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let output = rt.block_on(async {
            capture_handle_voice_dag_output(&kernel, "").await
        });
        assert!(output.contains("Empty"), "expected Empty in output, got: {}", output);
    }

    #[test]
    fn voice_dag_no_llm_unmatched_prints_unmatched() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel.set_llm_client(None);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let output = rt.block_on(async {
            capture_handle_voice_dag_output(&kernel, "请讲解量子计算原理").await
        });
        assert!(output.contains("Unmatched"), "expected Unmatched in output, got: {}", output);
    }

    /// 辅助:调用 handle_voice_dag_command 并捕获 stdout。
    /// 注:handle_voice_dag_command 直接 println!,无法 easy capture。
    /// 简化:测试只验证函数不 panic + 返回 Ok(()),具体输出由人工 / e2e 测试验证。
    async fn capture_handle_voice_dag_output(kernel: &TrustKernel, text: &str) -> String {
        // 由于 handle_voice_dag_command 直接 println!,此处仅调函数验证不 panic。
        // 真正的 stdout 捕获需重构为返回 String,本 plan 不做(避免过度工程)。
        let result = handle_voice_dag_command(kernel, text).await;
        assert!(result.is_ok(), "handle_voice_dag_command failed: {:?}", result);
        // 返回空串占位 — 测试用 contains 检查会失败,需调整测试策略。
        // 改为:测试只验证 result.is_ok()(函数不 panic)。
        String::new()
    }
}
```

注意:由于 `handle_voice_dag_command` 直接 `println!`,stdout 捕获困难。本 plan 简化测试策略 — 测试只验证函数不 panic(返回 Ok(())）。真正的输出验证由 e2e smoke 测试(Plan 6)覆盖。

更新测试为简化版:

```rust
#[cfg(test)]
mod w8_plan4_cli_tests {
    use super::*;
    use trust_kernel::kernel::TrustKernel;

    #[test]
    fn voice_dag_keyword_match_does_not_panic() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { handle_voice_dag_command(&kernel, "整理下载目录").await });
        assert!(result.is_ok(), "handle_voice_dag_command failed: {:?}", result);
    }

    #[test]
    fn voice_dag_empty_input_does_not_panic() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { handle_voice_dag_command(&kernel, "").await });
        assert!(result.is_ok());
    }

    #[test]
    fn voice_dag_unmatched_does_not_panic() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel.set_llm_client(None);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { handle_voice_dag_command(&kernel, "请讲解量子计算原理").await });
        assert!(result.is_ok());
    }
}
```

- [ ] **Step 3: 跑测试,确认失败**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p cli --features voice,llm w8_plan4_cli_tests`
Expected: FAIL(编译错误:`handle_voice_dag_command` not found)

- [ ] **Step 4: 实现 `handle_voice_dag_command` 函数**

在 `voicepilot/crates/cli/src/main.rs` 中,在 `#[cfg(feature = "voice")] fn handle_voice_route_command` 函数附近(或文件末尾的 `#[cfg(feature = "voice")]` 区块)追加:

```rust
/// W8 Plan 4: voice-dag <text> 子命令。
/// 调用 route_text_with_dag,根据 RouteOutcome 分支打印:
/// - Routed { skill_id }:打印匹配的 Skill id
/// - DagPlan(plan):打印 DAG 节点列表 + 询问 Allow/Deny(本 Plan 不执行 DAG,
///   实际执行由 Plan 6 集成测试覆盖)
/// - Unmatched { text }:打印未匹配
/// - Empty:打印空输入
#[cfg(feature = "voice")]
async fn handle_voice_dag_command(kernel: &TrustKernel, text: &str) -> anyhow::Result<()> {
    use trust_kernel::voice::router_bridge::{route_text_with_dag, RouteOutcome};

    let outcome = route_text_with_dag(kernel, text)
        .await
        .map_err(|e| anyhow!("route_text_with_dag failed: {}", e))?;

    match outcome {
        RouteOutcome::Routed { skill_id } => {
            println!("Routed → skill_id: {}", skill_id);
            println!("(W8 Plan 4: 执行由 Plan 5 UI / Plan 6 e2e 覆盖)");
        }
        #[cfg(feature = "llm")]
        RouteOutcome::DagPlan(plan) => {
            println!("DagPlan → plan_id: {}", plan.plan_id);
            println!("  user_goal: {}", plan.user_goal);
            println!("  max_total_steps: {}", plan.max_total_steps);
            println!("  nodes ({}):", plan.nodes.len());
            for node in &plan.nodes {
                println!(
                    "    - {} | skill_id: {} | risk: {:?}",
                    node.node_id, node.skill_id, node.risk_ceiling
                );
            }
            println!("  edges ({}):", plan.edges.len());
            for edge in &plan.edges {
                println!(
                    "    - {} → {} (port: {:?})",
                    edge.from, edge.to, edge.port_binding
                );
            }
            // 简单 Allow/Deny 询问(本 Plan 不实际执行 DAG)
            print!("\nAllow DAG skeleton execution? [y/N] ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let mut buf = String::new();
            let n = std::io::stdin().read_line(&mut buf).unwrap_or(0);
            if n == 0 {
                println!("(EOF → Deny)");
            } else if buf.trim().eq_ignore_ascii_case("y") {
                println!("(W8 Plan 4: DAG 执行由 DagExecutor 实现,本 Plan 仅路由)");
                // Plan 6 集成测试会调 DagExecutor::run
            } else {
                println!("Denied — DAG not executed.");
            }
        }
        RouteOutcome::Unmatched { text } => {
            println!("Unmatched → text: {}", text);
            println!("(W7 Planner fallback)");
        }
        RouteOutcome::Empty => {
            println!("Empty — no input text.");
        }
    }
    Ok(())
}
```

- [ ] **Step 5: 在 main 函数的命令分发逻辑中加 `voice-dag` 分支**

修改 `voicepilot/crates/cli/src/main.rs` 的 `fn main()` 中 `#[cfg(feature = "voice")]` 区块(约第 109-129 行),在 `voice route` 分支后追加:

```rust
        #[cfg(feature = "voice")]
        {
            if line == "voice list-models" {
                handle_voice_list_models_command();
                return Ok(());
            }
            if let Some(rest) = line.strip_prefix("voice transcribe ") {
                let path = rest.trim();
                handle_voice_transcribe_command(path)?;
                return Ok(());
            }
            if let Some(rest) = line.strip_prefix("voice route ") {
                let text = rest.trim();
                handle_voice_route_command(text)?;
                return Ok(());
            }
            // W8 Plan 4: voice-dag <text> 子命令
            if let Some(rest) = line.strip_prefix("voice-dag ") {
                let text = rest.trim();
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow!("failed to create tokio runtime: {}", e))?;
                rt.block_on(handle_voice_dag_command(&kernel, text))?;
                return Ok(());
            }
            if line == "voice listen" {
                handle_voice_listen_command()?;
                return Ok(());
            }
        }
```

并在命令帮助打印区(约第 67-75 行)追加:

```rust
    #[cfg(feature = "voice")]
    println!("  voice-dag <text>         Route text through W8 DAG-aware router (decompose to multi-step DAG if LLM enabled)");
```

- [ ] **Step 6: 跑 CLI 测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p cli --features voice,llm w8_plan4_cli_tests`
Expected: PASS(3 个测试全绿:不 panic)

- [ ] **Step 7: 跑 CLI 编译,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p cli --features voice,llm`
Expected: PASS

- [ ] **Step 8: 手动 smoke 测试 — 启动 CLI 输入 voice-dag**

Run: `cd d:\voicepilot\voicepilot ; cargo build -p cli --features voice,llm`
然后:`.\target\debug\cli.exe`(在交互式终端输入 `voice-dag 整理下载目录`)

Expected output:
```
> voice-dag 整理下载目录
Routed → skill_id: files.organize
(W8 Plan 4: 执行由 Plan 5 UI / Plan 6 e2e 覆盖)
```

- [ ] **Step 9: Commit**

```powershell
git add voicepilot/crates/cli/src/main.rs
git commit -m "feat(w8p4): add CLI voice-dag subcommand with DAG node listing + Allow/Deny prompt"
```

---

## Task 8: clippy + 6 套 feature cargo check + PROGRESS.md 更新

**Files:**
- Modify: `docs/PROGRESS.md`

**背景:** W8 验收门禁(spec §7.1)要求 6 套 feature 组合 cargo check 全 PASS + clippy `-D warnings` 0 警告。本 Task 跑全量验收 + 更新 PROGRESS.md。

- [ ] **Step 1: 跑 clippy,确认 0 警告**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy --workspace --no-default-features -- -D warnings`
Expected: PASS(0 warnings)

若有警告,逐一修复:
- 未使用 import → 删除或加 `#[cfg(feature = "...")]` 门控
- `explicit_auto_deref` → 改 `&kernel.conn()` 不用 `&*kernel.conn()`(project_memory.md "Engineering Conventions")
- `await_holding_lock` → 检查 `route_text_with_dag` 是否在持有 `kernel.conn()` guard 时 `.await`;若是,先 drop guard 再 await(本 plan 的 `privacy_mode()` 是同步读取,不持有 guard 跨 await,应无此 lint)
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

Expected: 全 PASS

若某组合失败,检查:
- 无 `voice` feature 时:`voice/router_bridge.rs` 不编译,`route_text_with_dag` 不存在;CLI `voice-dag` 子命令 `#[cfg(feature = "voice")]` 门控不编译 → 应 PASS
- 无 `llm` feature 时:`RouteDecision::Dag` / `RouteOutcome::DagPlan` / `RouteDecision::SkillWithSlots` 不编译;`kernel.llm_client()` / `set_llm_client()` 不存在;`route_text_with_dag` 的 LLM 分支 `#[cfg(feature = "llm")]` 门控跳过 → 应 PASS
- `voice` + `llm` 双 feature:全部代码编译,`route_text_with_dag` 完整可用 → 应 PASS

- [ ] **Step 3: 跑全量测试,确认无回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test --workspace --features voice,tauri,llm`
Expected: PASS(W1-W7 测试 + W8 Plan 1-4 新增测试全绿)

新增测试预期(W8 Plan 4):
- `skills::router::tests::route_decision_dag_variant_*`:1 个
- `skills::router::tests::skill_router_skills_accessor_*`:1 个
- `kernel::tests::llm_client_*`:3 个
- `kernel::tests::privacy_mode_*`:4 个
- `voice::router_bridge::tests::decision_to_outcome_*`:3 个
- `voice::router_bridge::tests::route_text_with_dag_*`:2 个
- `w8_plan4_router_bridge_dag`(集成测试):8 个场景
- `cli::w8_plan4_cli_tests`:3 个
- 合计 W8 Plan 4 新增 ≥ 25 个测试

- [ ] **Step 4: 更新 `docs/PROGRESS.md`**

在 W8 Plan 3 段落后追加 W8 Plan 4 段落(若 Plan 2/3 段落不存在,在 W8 Plan 1 段落后直接追加):

```markdown
> **W8 Plan 4:** ✅ 已完成(2026-07-26)— Router Bridge 集成 RouteDecision::Dag 分支 + route_text_with_dag 三级路由策略(关键词优先 → LLM 拆解 → W7 route_with_llm 回退)+ TrustKernel llm_client/privacy_mode accessors + CLI voice-dag 子命令,~8 个 commit,新增 ≥ 25 个测试(default cargo test --workspace 全绿),clippy `-D warnings` 0 警告,6 套 feature 组合 cargo check 全 PASS,详见 §二 W8 Plan 4 段落
```

更新里程碑表(§一):
```markdown
| W8 Plan 4 | Router Bridge 集成 RouteDecision::Dag + route_text_with_dag + llm_client/privacy_mode accessors + CLI voice-dag | ✅ 已完成 | +≥25 (voice,llm) | 2026-07-26 | (direct on master) |
```

更新累计测试数:在 W8 Plan 1 基础上加 W8 Plan 4 新增数(具体数字按 Step 3 实际跑出的结果填)。

- [ ] **Step 5: Commit**

```powershell
git add docs/PROGRESS.md
git commit -m "docs(w8p4): update PROGRESS.md with W8 Plan 4 completion + test counts"
```

- [ ] **Step 6: 跑最终验收门禁**

Run:
```powershell
cd d:\voicepilot\voicepilot
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --features voice,tauri,llm
cargo check --workspace --features voice,tauri,llm,uia
```

Expected: 全 PASS,无回归,W8 Plan 4 验收门禁闭合。

---

## Self-Review

### 1. Spec coverage

| Spec 章节 | 覆盖 Task |
|---|---|
| §2.8 Router Bridge 集成 RouteDecision::Dag 分支 | Task 1(RouteDecision::Dag)+ Task 3-5(route_text_with_dag) |
| §2.8 route_text_with_dag 三级路由策略 | Task 3(关键词优先 + Empty)+ Task 4(LLM 拆解 + validate_dag)+ Task 5(route_with_llm 回退) |
| §2.8 RouteOutcome::DagPlan 变体 | Task 3(变体定义)+ Task 6(内联测试) |
| §2.1 双层防御 #1(LLM 返回后立即 validate_dag) | Task 4(validate_dag 调用 + 失败回退) |
| §2.2 decompose_to_dag 签名引用 | Task 4(调用 `llm.decompose_to_dag(text, router.skills(), &user_slots)`) |
| §6 privacy_mode 安全约束 | Task 2(privacy_mode accessor)+ Task 4(privacy_mode=true 跳过 LLM) |
| §6 LLM 不可绕过审批 | Task 3-5(route_text_with_dag 只路由不执行,DAG 执行由 DagExecutor 走 W7 prepare→approve→commit) |
| §6 max_total_steps 硬上限 | Task 4(validate_dag 间接覆盖,Plan 1 已实现 validate_total_steps) |
| §7.1 编译门禁(6 套 feature cargo check + clippy) | Task 8 |
| §7.2 测试门禁(W8 新增 ≥ 30 个测试,本 Plan 贡献 ≥ 25) | Task 1-7 测试 + Task 8 验收 |
| §7.4 安全门禁(privacy_mode=true 不调 LLM) | Task 4 scenario_3 |

### 2. Placeholder scan

检查本 plan 是否有 placeholder:

- ❌ "TBD" / "TODO" / "implement later":无
- ❌ "Add appropriate error handling":所有错误处理均明确(catch Err + tracing::warn! + 回退)
- ❌ "Write tests for the above":每个 Task 有完整测试代码
- ❌ "Similar to Task N":无
- ❌ Steps 无代码块:每个 Step 有完整 Rust 代码
- ✅ 所有类型 / 函数 / 方法引用均在某 Task 定义或来自既有代码

### 3. Type consistency

- `RouteDecision::Dag(DagPlan)`:Task 1 定义,Task 4 / Task 5 / Task 6 引用一致
- `RouteOutcome::DagPlan(DagPlan)`:Task 3 定义,Task 4 / Task 5 / Task 6 / Task 7 引用一致
- `DagPlan`:来自 Plan 1 `crate::skills::dag_types::DagPlan`,全 plan 一致引用
- `SlotTemplateEngine::validate_dag(&dag) -> Result<(), TemplateError>`:来自 Plan 1,Task 4 引用一致
- `LlmClient::decompose_to_dag(text, skills, slots) -> Result<DagPlan>`:来自 Plan 2(spec §2.2),Task 4 引用一致
- `LlmClient::is_enabled() -> bool`:W7 既有,Task 4 引用一致
- `kernel.llm_client() -> Option<Arc<LlmClient>>`:Task 2 定义,Task 4 / Task 5 引用一致
- `kernel.privacy_mode() -> bool`:Task 2 定义,Task 4 引用一致
- `kernel.set_llm_client(Option<Arc<LlmClient>>)`:Task 2 定义,Task 7 测试引用一致
- `SkillRouter::skills() -> &[SkillManifest]`:Task 1 定义,Task 4 / Task 5 引用一致
- `SkillRouter::with_llm(Arc<LlmClient>) -> Self`:W7 既有,Task 5 引用一致
- `SkillRouter::route_with_llm(text) -> RouteDecision`:W7 既有(`#[cfg(feature = "llm")]` 门控),Task 5 引用一致
- `route_text_with_dag(kernel, text) -> Result<RouteOutcome>`:Task 3 定义 async,Task 4-5 填充,Task 6-7 引用一致
- `decision_to_outcome(RouteDecision, &str) -> RouteOutcome`:Task 5 定义,Task 6 测试引用一致
- `build_router_with_llm(&TrustKernel, &SkillRouter) -> SkillRouter`:Task 5 定义,Task 5 引用一致

### 4. 已知偏离

- **`route_text_with_dag` 返回 `RouteOutcome` 而非 `RouteDecision`**:spec §2.8 写的是 `Result<RouteDecision>`,但 task description 要求返回 `RouteOutcome`(便于调用方直接处理 DAG / Routed / Unmatched)。本 plan 选 `RouteOutcome`,与 W7 `route_text` 模式一致。这是合理偏离 — `RouteDecision` 是 router 内部类型,`RouteOutcome` 是 bridge 对外类型。
- **`user_slots` 暂传空 slice**:spec §2.8 写 `let user_slots = slot_parser::parse(text);`,但 trust-kernel 无 `slot_parser` 模块(W6b 在 UI crate 有)。本 plan 暂传 `Vec::new()`,LLM 内部 `decompose_to_dag` 会自行提取 slots(spec §2.2)。Plan 5+ 视需要补 trust-kernel slot_parser 模块。
- **CLI `voice-dag` 不实际执行 DAG**:本 Plan 只到路由 + 打印 DAG 骨架 + 询问 Allow/Deny;实际 DagExecutor::run 由 Plan 6 集成测试覆盖。CLI 输出 "DAG 执行由 DagExecutor 实现,本 Plan 仅路由" 提示用户。
- **CLI 测试不捕获 stdout**:`handle_voice_dag_command` 直接 `println!`,stdout 捕获困难。本 plan 测试只验证函数不 panic(返回 Ok(())）。真正输出验证由 Plan 6 e2e smoke 测试覆盖。
- **`build_router_with_llm` clone keyword_router**:当 LLM 不可用时 clone 原 router(SkillRouter derive Clone)。这是性能中性操作(SkillManifest 是 Vec,clone O(n)),且只在 LLM 拆解失败回退路径触发,不影响热路径。
- **`route_with_llm` 在 LLM=None 时退化为 keyword 匹配**:W7 `route_with_llm` 的 `if let Some(llm) = &self.llm` 分支跳过,落到 Planner。本 plan 依赖此行为 — `build_router_with_llm` 在 LLM=None 时返回 keyword-only router,`route_with_llm` 内部 keyword 已在第 1 级做过(不命中),直接落到 Planner → Unmatched。这与 spec §2.8 第 3 级 "W7 单 Skill 路由(关键词 + LLM fallback)" 一致。
- **`kernel.privacy_mode()` 每次读 KV**:`config_repo().get(&conn, "privacy.mode")` 每次查 DB,无缓存。性能可接受(Settings 变更不频繁,且 route_text_with_dag 不在热路径)。若 W9+ 需优化,可加 `Mutex<bool>` 字段 + setter 同步。

### 5. 风险点

- **Plan 2 `decompose_to_dag` 签名风险**:本 plan 假设 `decompose_to_dag(text, skills, slots) -> Result<DagPlan, LlmError>`(spec §2.2)。若 Plan 2 实际签名不同(如返回 `Result<DagPlan, KernelError>`),Task 4 的 `match llm.decompose_to_dag(...).await` 错误分支需调整。Plan 2 完成后,执行者需核对签名并调整 Task 4 代码。
- **Plan 2 `decompose_to_dag` 内部校验严格度**:若 Plan 2 在 LLM 返回前已校验 skill_id / 模板语法,scenario_6(非法 skill_id)可能在 `decompose_to_dag` 阶段就 Err,而非 `validate_dag` 阶段。两种情况都走 catch Err → 回退 route_with_llm,测试行为一致,但审计日志的 error 字段不同。Plan 2 完成后需确认 scenario_6 mock body 是否能触发期望的失败路径。
- **wiremock mock body schema**:scenario_4 / scenario_7 的 mock body 假设 `decompose_to_dag` 用 OpenAI function calling 格式(`choices[0].message.tool_calls[0].function.arguments`)。若 Plan 2 用不同 schema(如 response_format json_schema),需调整 mock body。Plan 2 完成后核对。
- **`RouteDecision::Dag` 在 `route()` 同步方法中从不返回**:Task 3 的 match arm `RouteDecision::Dag(_)` 是防御性 unreachable。若未来 route() 同步方法扩展为返回 Dag(不可能,因为 Dag 需要 LLM 调用),需重新评估。

---

## Execution Handoff

本 Plan(Plan 4)完成后,后续 Plan 5-6 的执行流程:

1. **Plan 5:** UI: DAG 骨架审批弹窗 + DAG 历史 + task.explain 面板
   - 依赖本 Plan 的 `RouteOutcome::DagPlan(DagPlan)` 变体(Tauri command `route_text_with_dag_command` 返回此类型)
   - 依赖本 Plan 的 `kernel.llm_client()` / `set_llm_client()` accessors(Settings 面板 LLM 配置变更时注入)
2. **Plan 6:** 集成测试 + 6 套 feature 组合 cargo check 矩阵 + clippy + npm build
   - 依赖本 Plan 的 `route_text_with_dag` 函数(e2e 测试 "打开记事本写 TODO 然后保存到桌面" → DAG [note.capture, files.move])
   - 依赖本 Plan 的 CLI `voice-dag` 子命令(e2e smoke 测试)

每个 Plan 完成后,更新 `docs/PROGRESS.md` 并 commit。

---

## 附录 A: route_text_with_dag 完整代码(供 Plan 5/6 参考)

执行完 Task 1-5 后,`voicepilot/crates/trust-kernel/src/voice/router_bridge.rs` 的 `route_text_with_dag` 函数完整代码:

```rust
#[cfg(feature = "voice")]
pub async fn route_text_with_dag(
    kernel: &TrustKernel,
    text: &str,
) -> crate::error::Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    router.register(task_repeat_verified_manifest());
    router.register(task_compensate_manifest());
    router.register(task_explain_manifest());
    #[cfg(all(windows, feature = "uia"))]
    {
        use crate::skills::manifest::{app_control_manifest, note_capture_manifest};
        router.register(app_control_manifest());
        router.register(note_capture_manifest());
    }
    router.register(research_save_manifest());
    router.register(form_prepare_manifest());

    // 第 1 级:关键词优先
    let keyword_decision = router.route(trimmed);
    match keyword_decision {
        RouteDecision::Skill(manifest) => {
            return Ok(RouteOutcome::Routed {
                skill_id: manifest.id,
            });
        }
        #[cfg(feature = "llm")]
        RouteDecision::SkillWithSlots(manifest, _slots) => {
            return Ok(RouteOutcome::Routed {
                skill_id: manifest.id,
            });
        }
        #[cfg(feature = "llm")]
        RouteDecision::Dag(_) => { /* unreachable: route() 同步不返回 Dag */ }
        RouteDecision::Planner => { /* 落到第 2 级 */ }
    }

    // 第 2 级:LLM 拆解
    #[cfg(feature = "llm")]
    {
        if let Some(llm) = kernel.llm_client() {
            if llm.is_enabled() && !kernel.privacy_mode() {
                let user_slots: Vec<crate::llm::types::ExtractedSlot> = Vec::new();
                match llm
                    .decompose_to_dag(trimmed, router.skills(), &user_slots)
                    .await
                {
                    Ok(dag) => {
                        match crate::skills::template::SlotTemplateEngine::validate_dag(&dag) {
                            Ok(()) => return Ok(RouteOutcome::DagPlan(dag)),
                            Err(e) => {
                                tracing::warn!(error = %e, "validate_dag failed; falling back to W7 route_with_llm");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "decompose_to_dag failed; falling back to W7 route_with_llm");
                    }
                }
            }
        }
    }

    // 第 3 级:W7 route_with_llm 回退
    #[cfg(feature = "llm")]
    {
        let llm_router = build_router_with_llm(kernel, &router);
        let decision = llm_router.route_with_llm(trimmed).await;
        return Ok(decision_to_outcome(decision, trimmed));
    }

    Ok(RouteOutcome::Unmatched {
        text: trimmed.to_string(),
    })
}
```

执行者注意:本附录代码仅供 Plan 5/6 参考,实际代码以 Task 3-5 逐步实现的版本为准(可能有微调)。
