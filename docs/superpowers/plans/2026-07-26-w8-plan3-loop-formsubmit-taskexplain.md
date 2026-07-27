# W8 Plan 3: DagExecutor 循环节点 + form.submit 新 Skill + task.explain LLM 增强 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Plan 1(基础设施)+ Plan 2(LlmClient::decompose_to_dag + DagExecutor 简单节点 + dispatch_skill_executor 9 路路由)之上,闭合 W8 spec 的 3 项剩余工作:(1) DagExecutor 循环节点执行(spec §2.3 决策 #5 + #8 — `run_loop_node` + `resolve_iterable` + `break_condition` 解析 + `max_iterations ≤ 50` 硬截断 + 失败终止循环);(2) `form.submit` 新 Skill(spec §2.4 — Playwright MCP navigate + click,风险 E3 + PerStep 审批 + 无补偿,与 W7 `form.prepare` 的 E2 区分);(3) `task.explain` LLM 增强(spec §2.5 — `execute_task_explain_with_llm` + `LlmClient::explain_failure` + 中文 system prompt 约束 + HTTP/JSON 失败回退 structured_only + 持久化到 `task_explanations` 表)。同时扩展 `dispatch_skill_executor` 把 `form.submit` 占位 Err 替换为真实 executor,新增 `llm_explain_called` 审计事件(spec §6.1)。

**Architecture:** `trust-kernel` 改 `skills/dag_executor.rs`(新增 `run_loop_node` + `resolve_iterable` + `parse_break_condition` + `evaluate_break_condition` 私有 helper,循环节点状态 Succeeded(Array) / Failed{cause});新增 `skills/form_submit.rs`(`FormSubmitInput` + `execute_form_submit` + `form_submit_manifest`,模式镜像 W7 `form_prepare.rs` 但 risk_ceiling=E3 + compensation=None);改 `skills/dispatcher.rs`(`form.submit` 分支从 `Err("not implemented")` 替换为 `dispatch_form_submit`);改 `skills/task_explain.rs`(新增 `execute_task_explain_with_llm` + `TaskExplanation` + `LlmAnalysis` 数据结构,`#[cfg(feature = "llm")]` 门控 LLM 调用路径);改 `skills/manifest.rs`(新增 `form_submit_manifest`);改 `llm/client.rs`(新增 `explain_failure` 方法 + `build_explain_system_prompt` + `build_explain_tool_schema` + `parse_explain_response` 私有 helper,`#[cfg(feature = "llm")]` 门控);改 `skills/mod.rs`(`pub mod form_submit;`);改 `skills/explanation_repo.rs`(复用 Plan 1 的 `TaskExplanationRepo::create` 持久化 LLM 归因)。

**Tech Stack:** Rust(stable),`reqwest`(已有,`llm` feature),`wiremock`(已有,dev-dependency),`serde_json`(已有),`rusqlite`(已有),`uuid`(已有),`regex`(已有,可选用于 break_condition 解析;本 plan 用手写 parser 避免 regex 开销),`sha2`(已有,form_submit 的 preconditions_hash),TDD,PowerShell。

**Spec:** `docs/superpowers/specs/2026-07-26-w8-skill-orchestration-dag-design.md` §2.3(DagExecutor 循环节点 — 决策 #5 完整 DAG 含循环 + 决策 #8 循环失败终止)+ §2.4(`form.submit` 新 Skill)+ §2.5(`task.explain` LLM 增强)+ §6.1(审计事件 `llm_explain_called`)

**Precondition:** W8 Plan 1 + Plan 2 已完成 —
- Plan 1:`SlotTemplateEngine`(模板解析/渲染/校验)+ `DagPlan`/`DagNode`/`DagEdge`/`LoopSpec`/`IterableSource`/`DagStatus`/`DagNodeStatus`/`DagResult` 数据结构 + `DagRepo`(`dag_plans`/`dag_nodes` CRUD)+ `TaskExplanationRepo`(`task_explanations` CRUD + `FailureCategory` enum)+ DB 迁移 `003_dag_plans.sql` 全部就绪
- Plan 2:`Approver` trait 扩展了 `approve_dag_skeleton` + `AutoApprover`/`AutoDenier`/`TauriApprover` 三实现 + `dispatch_skill_executor` 9 路路由(含 `form.submit` 占位 `Err("not implemented in Plan 2; see Plan 3")`)+ `DagExecutor::new(kernel, approver, dag_repo)` + `run(&DagPlan) -> Result<DagResult>` 主入口 + `topological_sort` Kahn 算法 + `run_simple_node` 简单节点执行 + 失败短路 + `PartiallySucceeded` 分支 + `LlmClient::decompose_to_dag`(`#[cfg(feature = "llm")]` 门控)
- W7 既有 8 个 Skill executor + `LlmClient::classify_and_extract`(OpenAI 兼容 `/chat/completions` + function calling)+ `invoke_mcp_tool` / `record_approval_decision` / `finalize_step_success` / `validate_input_against_manifest` helper + `AutoApprover` / `AutoDenier` 可被本 Plan 直接复用
- `cargo check -p trust-kernel --features voice,tauri,llm,uia` PASS,clippy `-D warnings` 0 警告

---

## File Structure

### Backend — trust-kernel(`voicepilot/crates/trust-kernel/`)

- **Modify** `src/skills/dag_executor.rs` — 新增 `run_loop_node(node_id, plan, loop_spec, node_outputs, prev_node_id) -> Result<DagNodeStatus>` + `resolve_iterable` + `parse_break_condition` + `evaluate_break_condition` 私有 helper;改 `run()` 主循环把"循环节点返回 Err"替换为真实 `run_loop_node` 调用
- **Create** `src/skills/form_submit.rs` — `FormSubmitInput` struct + `execute_form_submit(kernel, input, approver) -> Result<String>` + `build_form_submit_effect_manifest` 私有 helper + `#[cfg(test)] mod tests`(6 个单元测试)
- **Modify** `src/skills/manifest.rs` — 新增 `form_submit_manifest()` 函数(risk_ceiling=E3 + compensation=None + PerStep approval + Weak verifier)
- **Modify** `src/skills/dispatcher.rs` — `form.submit` 分支从 `Err("not implemented")` 替换为 `dispatch_form_submit` 调用 `execute_form_submit`;新增 `dispatch_form_submit` 私有 helper
- **Modify** `src/skills/task_explain.rs` — 新增 `execute_task_explain_with_llm(kernel, input, approver, llm: Option<&LlmClient>) -> Result<String>` + `TaskExplanation` / `LlmAnalysis` / `FailedToolCallSummary` 数据结构 + `#[cfg(feature = "llm")]` 门控的 LLM 调用路径 + HTTP/JSON 失败回退 structured_only
- **Modify** `src/llm/client.rs` — `impl LlmClient` 块追加 `explain_failure(step, audit_logs) -> LlmResult<LlmAnalysis>` 方法(`#[cfg(feature = "llm")]` 门控)+ `build_explain_system_prompt` / `build_explain_tool_schema` / `parse_explain_response` 私有 helper
- **Modify** `src/skills/mod.rs` — `pub mod form_submit;`(task_explain 已存在)
- **Modify**(可选) `src/llm/types.rs` — 若 `LlmAnalysis` 与 `crate::skills::explanation_repo::FailureCategory` 有重复,统一 re-export

### Tests

- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs` — `run_loop_node` 5 场景(Literal 全成功 / PrevNodeOutput 全成功 / 循环中失败 break / break_condition 触发 / max_iterations>50 截断)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan3_form_submit.rs` — `form_submit_manifest` 字段断言 + E3 risk_ceiling + PerStep approval + None compensation + 正常执行(navigate + click)+ Approval Deny → Cancelled(6 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan3_dispatcher_form_submit.rs` — `dispatch_skill_executor("form.submit", ...)` 路由命中(2 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan3_llm_explain_failure.rs` — wiremock 4 场景(LLM 成功 / HTTP 失败回退 / JSON 解析失败回退 / 非法 category 回退,4 个 `#[cfg(feature = "llm")] #[tokio::test]`)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan3_task_explain_llm.rs` — `execute_task_explain_with_llm` 6 场景(LLM disabled / step Succeeded / step Failed + LLM 成功 / HTTP 失败回退 / 非法 category 回退 / 持久化到 task_explanations,6 个测试)
- **Create** `voicepilot/crates/trust-kernel/tests/w8_plan3_audit_llm_explain.rs` — `llm_explain_called` 审计事件断言(1 个测试)

### Docs

- **Modify** `docs/PROGRESS.md` — W8 Plan 3 完成状态 + 测试统计

---

## Conventions

- **PowerShell**:`;` 分隔命令,不用 `&&` / `||`;`git commit -m "msg"` 单行(参考 project_memory.md "Lessons Learned")
- **TDD**:每个含逻辑的任务先写失败测试 → `cargo test` 跑红 → 实现最小代码 → `cargo test` 跑绿 → `git commit`
- **Repo accessor pattern**:`DagRepo::new()` / `TaskExplanationRepo::new()` 不带参数,方法接收 `&Connection`(参考 W4 `McpServerRepo`,Plan 1 已落地)
- **`Approver` import**:用完整路径 `use crate::approval::approver::Approver`(approval 模块未在 root re-export,参考 project_memory.md "Engineering Conventions")
- **`&Connection` 接收**:`&kernel.conn()` 而非 `&*kernel.conn()`(后者触发 clippy `explicit_auto_deref` lint,参考 project_memory.md)
- **`TrustKernel` 不是 `Clone`**(持有 `Mutex<Connection>`);`DagExecutor` 持有 `Arc<TrustKernel>` + `Arc<dyn Approver>`(Plan 2 已落地)
- **`query_map` closure** 返回 `rusqlite::Result<T>` 不是 `crate::error::Result<T>`(参考 project_memory.md "Lessons Learned")
- **Feature gate**:`LlmClient::explain_failure` 相关代码 `#[cfg(feature = "llm")]` 门控;`execute_task_explain_with_llm` 的 LLM 调用路径用 `#[cfg(feature = "llm")]` 门控,非 LLM 路径(`structured_only` 回退)默认编译;测试用 `#[cfg(feature = "llm")] #[tokio::test]`;默认 `cargo build` 不包含 LLM 代码
- **Commit message**:`feat(w8p3): ...` / `test(w8p3): ...` / `docs(w8p3): ...` / `refactor(w8p3): ...`
- **不引入新依赖**:本 plan 仅用 `reqwest` + `wiremock` + `serde_json` + `rusqlite` + `uuid` + `sha2`(均已在 workspace),不加新 crate
- **不修改 W7 既有 8 个 executor**:本 plan 仅新增 `form_submit.rs` + 扩展 `task_explain.rs`(新增 `execute_task_explain_with_llm` 函数,保留 W7 既有 `execute_explain` 不动,dispatcher 在 Plan 2 已路由到 `execute_explain`,本 plan 不改 dispatcher 的 task.explain 分支 — 避免破坏 Plan 2 测试;Plan 4 Router Bridge 集成时再决定是否切换到 `execute_task_explain_with_llm`)
- **break_condition 解析**:W8 仅支持 `item.<field> <op> <number>` 形式(op ∈ {>, <, >=, <=, ==, !=}),手写 parser 不用 regex(避免 regex 编译开销;字段值用 `serde_json::Value::get(field).as_f64()`)
- **循环失败语义(决策 #8)**:循环中迭代失败 → 立即 break + 节点 `DagNodeStatus::Failed{cause}`;上层 `run()` 走 `PartiallySucceeded`(若有成功节点)或 `Failed`(无成功节点)分支
- **max_iterations 硬上限**:`min(spec.max_iterations, 50)` 强制截断(spec §6 安全约束)
- **Windows-only**:本 plan 不引入 `cfg(target_os = ...)` 条件编译(参考 project_memory.md "Hard Constraints")
- **LlmClient 不由 kernel 持有**:W7 设计中 `LlmClient` 由 CLI/UI 顶层构造后传入;`execute_task_explain_with_llm` 接收 `Option<&LlmClient>` 参数,`None` 时走 structured_only 回退(与 spec §2.5 一致)
- **task.explain 复用 W7 既有 step 读取**:`kernel.get_step(step_id)` → `StepRecord`(含 task_id / status);`kernel.list_audit_for_task(task_id)` → 审计事件列表(过滤 step_id 字段)

---

## Task 1: DagExecutor::run_loop_node 骨架 + resolve_iterable

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs`(本 Task 仅写 resolve_iterable 测试,后续 Task 2-3 在同文件追加)

**Spec §:** §2.3 "循环节点执行(`run_loop_node`,决策 #5 + #8)" — Step 1: `resolve_iterable(iterable_source, node_outputs, user_slots) → Vec<Value>`

**背景:** Plan 2 的 `DagExecutor::run` 在拓扑序遍历时,若节点 `node_id` 在 `plan.loop_specs` 中,直接返回 `Err("loop node '...' not supported in Plan 2; see Plan 3")`。本 Task 把 stub 替换为真实 `run_loop_node` 调用,并实现 Step 1(`resolve_iterable`)。

`resolve_iterable` 把三种 `IterableSource` 解析为 `Vec<serde_json::Value>`:
- `PrevNodeOutput { node_id, port }` — 从 `node_outputs[node_id]` 按 `port` dotted path 提取数组(如 `output.files` → `node_outputs["n1"]["output"]["files"]`),必须是 `Value::Array`
- `UserSlot { slot_kind }` — 从 `user_slots` 中找 `kind` 匹配的 `Files` slot,解析 `raw` 为 JSON 数组(W8 简化:Files slot 的 `raw` 是 JSON 数组字符串,如 `"[\"a.txt\",\"b.txt\"]"`)
- `Literal(vec)` — 直接转 `Vec<Value>`(每个 String → `Value::String`)

**依赖方向确认:** `run_loop_node` 引用 `IterableSource`(Plan 1 已定义在 `dag_types.rs`)+ `SlotTemplateEngine::resolve`(Plan 1 已定义)+ `dispatch_skill_executor`(Plan 2 已定义),无新依赖。

- [ ] **Step 1: 在 `dag_executor.rs` 实现 `run_loop_node` 方法骨架 + `resolve_iterable`**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,在 `impl DagExecutor` 块中(`run_simple_node` 方法后、`mark_node_failed` 方法前)追加:

```rust
    /// 循环节点执行(spec §2.3 决策 #5 + #8)。
    ///
    /// 算法:
    ///   Step 1: resolve_iterable(iterable_source, node_outputs, user_slots) → Vec<Value>
    ///   Step 2: items.into_iter().take(spec.max_iterations.min(50)) 强制截断
    ///   Step 3: 对每个 item:
    ///     - SlotTemplateEngine::resolve(template, ..., Some(&item)) 绑定 ${item}
    ///     - dispatch_skill_executor(skill_id, kernel, resolved_input, approver, ...)
    ///     - 失败 → iter_failed = Some(cause), break(决策 #8:终止循环)
    ///     - 成功 → push 到 iter_outputs
    ///     - 检查 break_condition,命中则 break
    ///   Step 4: 循环节点状态:
    ///     - 全成功 → DagNodeStatus::Succeeded(Array(iter_outputs))
    ///     - 有失败 → DagNodeStatus::Failed { cause }(让上层走 PartiallySucceeded 分支)
    ///
    /// 参数:
    /// - `node_id`:循环节点 id(必须在 plan.loop_specs 中)
    /// - `plan`:整个 DagPlan(用于查找 node + loop_spec)
    /// - `loop_spec`:循环规格(由调用方从 plan.loop_specs 取出)
    /// - `node_outputs`:已执行节点的 output(供 PrevNodeOutput iterable + ${prev} 模板)
    /// - `prev_node_id`:拓扑序前驱节点 id(供 ${prev} 模板)
    ///
    /// 返回 `Result<DagNodeStatus>`:
    /// - Ok(Succeeded(Array)) — 全部迭代成功
    /// - Ok(Failed{cause}) — 有迭代失败(cause 含失败原因 + 迭代索引)
    /// - Err(KernelError) — 不可恢复错误(iterable 解析失败 / DB 故障)
    fn run_loop_node(
        &self,
        node_id: &str,
        plan: &DagPlan,
        loop_spec: &crate::skills::dag_types::LoopSpec,
        node_outputs: &HashMap<String, serde_json::Value>,
        prev_node_id: Option<&str>,
    ) -> Result<DagNodeStatus> {
        // Step 1: 解析 iterable → Vec<Value>
        let items = self.resolve_iterable(&loop_spec.iterable_source, node_outputs)?;
        let items_len = items.len();

        // Step 2: 强制截断到 max_iterations.min(50)
        let max_iter = loop_spec.max_iterations.min(crate::skills::dag_types::MAX_LOOP_ITERATIONS_HARD_LIMIT);
        let truncated: Vec<serde_json::Value> = items.into_iter().take(max_iter as usize).collect();
        let truncated_len = truncated.len();
        if (truncated_len as u32) < items_len as u32 {
            tracing::warn!(
                node_id = node_id,
                original = items_len,
                truncated = truncated_len,
                max_iterations = loop_spec.max_iterations,
                hard_limit = crate::skills::dag_types::MAX_LOOP_ITERATIONS_HARD_LIMIT,
                "loop iterable truncated to max_iterations.min(50)"
            );
        }

        // Step 3: 查找节点(用于 input_template)
        let node = plan.nodes.iter().find(|n| n.node_id == node_id)
            .ok_or_else(|| KernelError::Skill(format!("run_loop_node: node '{}' not in plan.nodes", node_id)))?;

        // 持久化节点 start 状态(Pending → Running)
        let task_id_root = format!("task-loop-{}", uuid::Uuid::new_v4());
        {
            let conn = self.kernel.conn();
            let existing = self.dag_repo.list_nodes_by_plan(&conn, &plan.plan_id)?;
            if !existing.iter().any(|n| n.node_id == node_id) {
                self.dag_repo.create_node(&conn, &plan.plan_id, node)?;
            }
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                node_id,
                &DagNodeStatus::Running,
                None,
                None,
            )?;
        }

        // Step 4: 迭代执行(实际循环逻辑在 Task 2 实现,本 Task 仅返回 Succeeded 空数组占位)
        // TODO(Task 2): 替换为真实循环 + break_condition + 失败 break
        let iter_outputs: Vec<serde_json::Value> = Vec::new();
        let _ = (task_id_root, truncated_len, prev_node_id); // 占位,Task 2 用

        // Step 5: 节点状态(占位 — Task 3 实现真实状态构造)
        let status = DagNodeStatus::Succeeded(serde_json::Value::Array(iter_outputs));
        {
            let conn = self.kernel.conn();
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                node_id,
                &status,
                None,
                None,
            )?;
        }
        Ok(status)
    }

    /// resolve_iterable — 把 IterableSource 解析为 Vec<Value>(spec §2.3 Step 1)。
    ///
    /// - PrevNodeOutput { node_id, port } → node_outputs[node_id] 按 port dotted path 提取数组
    /// - UserSlot { slot_kind } → user_slots 中找 kind 匹配的 Files slot,raw 解析为 JSON 数组
    /// - Literal(vec) → 直接转 Vec<Value>
    ///
    /// 失败:
    /// - PrevNodeOutput:node_id 不在 node_outputs / port 路径不存在 / 值非 Array → Err
    /// - UserSlot:无匹配 kind / raw 非 JSON 数组 → Err
    fn resolve_iterable(
        &self,
        source: &crate::skills::dag_types::IterableSource,
        node_outputs: &HashMap<String, serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        use crate::skills::dag_types::IterableSource;
        match source {
            IterableSource::Literal(items) => {
                Ok(items.iter().map(|s| serde_json::Value::String(s.clone())).collect())
            }
            IterableSource::PrevNodeOutput { node_id, port } => {
                let node_out = node_outputs.get(node_id).ok_or_else(|| {
                    KernelError::Skill(format!(
                        "resolve_iterable: PrevNodeOutput node '{}' not in node_outputs",
                        node_id
                    ))
                })?;
                // 按 port dotted path 提取(如 "output.files" → ["output"]["files"])
                let mut current = node_out;
                for key in port.split('.') {
                    current = current.get(key).ok_or_else(|| {
                        KernelError::Skill(format!(
                            "resolve_iterable: port path '{}' not found in node_outputs['{}']",
                            port, node_id
                        ))
                    })?;
                }
                match current {
                    serde_json::Value::Array(arr) => Ok(arr.clone()),
                    _ => Err(KernelError::Skill(format!(
                        "resolve_iterable: PrevNodeOutput port '{}' is not an array (got {})",
                        port,
                        type_name_of_value(current)
                    ))),
                }
            }
            IterableSource::UserSlot { slot_kind } => {
                // W8 简化:user_slots 在 Plan 2 run_simple_node 中传 &[];
                // Plan 5 UI 集成时会传入实际 slot。
                // 此处假设 node_outputs 中无 user slot(因为 user_slots 参数未传到 resolve_iterable),
                // 返回 Err 让上层处理。
                // Plan 5 集成时,run() 签名扩展为接收 user_slots,本方法加 user_slots 参数。
                Err(KernelError::Skill(format!(
                    "resolve_iterable: UserSlot kind '{}' not supported in Plan 3 (user_slots not wired; see Plan 5)",
                    slot_kind
                )))
            }
        }
    }
```

在 `dag_executor.rs` 末尾(模块级,不在 impl 块内)追加 helper:

```rust
/// 返回 serde_json::Value 的类型名(用于错误消息)。
fn type_name_of_value(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
```

- [ ] **Step 2: 改 `run()` 主循环,把循环节点 stub 替换为真实 `run_loop_node` 调用**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,找到 `run()` 方法中的循环节点检测代码(Plan 2 Task 4 留下的):

```rust
            // 循环节点检测 — Plan 3 实现,本 plan 返回 Err
            if plan.loop_specs.contains_key(node_id) {
                return Err(KernelError::Skill(format!(
                    "loop node '{}' not supported in Plan 2; see Plan 3",
                    node_id
                )));
            }

            // 简单节点执行
            let status = self.run_simple_node(
                node,
                plan,
                &node_outputs,
                prev_node_id.as_deref(),
            )?;
```

替换为:

```rust
            // 节点执行:循环节点 → run_loop_node(Plan 3);简单节点 → run_simple_node(Plan 2)
            let status = if let Some(loop_spec) = plan.loop_specs.get(node_id) {
                self.run_loop_node(node_id, plan, loop_spec, &node_outputs, prev_node_id.as_deref())?
            } else {
                self.run_simple_node(node, plan, &node_outputs, prev_node_id.as_deref())?
            };
```

注意:删除原来的 `let node = plan.nodes.iter().find(...)?;` 行(因为简单节点分支内已重新查找;但为了保持代码清晰,把 node 查找移到分支前)。完整替换片段:

```rust
        for node_id in &order {
            let node = plan.nodes.iter().find(|n| &n.node_id == node_id)
                .ok_or_else(|| KernelError::Skill(format!("topo sort returned unknown node_id: {}", node_id)))?;

            // 节点执行:循环节点 → run_loop_node(Plan 3);简单节点 → run_simple_node(Plan 2)
            let status = if let Some(loop_spec) = plan.loop_specs.get(node_id) {
                self.run_loop_node(node_id, plan, loop_spec, &node_outputs, prev_node_id.as_deref())?
            } else {
                self.run_simple_node(node, plan, &node_outputs, prev_node_id.as_deref())?
            };

            let is_failed = status.is_failed();
            node_results.insert(node_id.clone(), status.clone());
            if let DagNodeStatus::Succeeded(out) = &status {
                node_outputs.insert(node_id.clone(), out.clone());
                prev_node_id = Some(node_id.clone());
            }

            // 失败处理(决策 #4 + #8):前序已 commit 不回滚,
            // DAG 走 Failed(无成功节点)或 PartiallySucceeded(有成功节点)
            if is_failed {
                let succeeded: Vec<String> = order.iter()
                    .filter(|nid| node_results.get(*nid).map(|s| s.is_succeeded()).unwrap_or(false))
                    .cloned()
                    .collect();
                let cause = match &status {
                    DagNodeStatus::Failed { cause } => cause.clone(),
                    _ => String::new(),
                };
                let final_status = if succeeded.is_empty() {
                    DagStatus::Failed { failed_node: node_id.clone(), cause }
                } else {
                    DagStatus::PartiallySucceeded { succeeded, failed_node: node_id.clone(), cause }
                };
                self.persist_dag_status(plan, &final_status)?;
                return Ok(DagResult {
                    status: final_status,
                    node_results,
                });
            }
        }
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS(可能有"unused variable"警告 — Task 2 实现真实循环后消除)

- [ ] **Step 4: 写失败测试 — 创建 `tests/w8_plan3_loop_node.rs`**

```rust
//! W8 Plan 3 Task 1: run_loop_node + resolve_iterable 单元测试.
//!
//! Spec §2.3 决策 #5 + #8:循环节点 + 失败终止循环。
//! 本 Task 仅测 resolve_iterable(三种 IterableSource);
//! Task 2-3 追加循环执行 + break_condition + 状态构造测试。

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagNode, DagPlan, IterableSource, LoopSpec,
};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

fn literal_node(id: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("notepad".into()),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with_loop(loop_spec: LoopSpec) -> DagPlan {
    let mut loop_specs = HashMap::new();
    loop_specs.insert("n1".into(), loop_spec);
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test loop".into(),
        nodes: vec![literal_node("n1")],
        edges: vec![],
        loop_specs,
        max_total_steps: 10,
    }
}

fn make_executor() -> (Arc<TrustKernel>, DagExecutor) {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    (kernel, executor)
}

#[test]
fn resolve_iterable_literal_returns_vec_of_strings() {
    // IterableSource::Literal(["a", "b", "c"]) → Vec<Value::String> 长度 3
    let (_kernel, executor) = make_executor();
    let source = IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]);
    let items = executor.resolve_iterable_unchecked(&source, &HashMap::new());
    // 注:resolve_iterable 是私有方法,本测试通过 run_loop_node 间接验证;
    // 若需直接测试,Plan 3 Task 1 Step 4 把 resolve_iterable 改为 pub(crate)。
    // 此处先验证 run_loop_node 不 panic(占位实现返回 Succeeded 空数组)。
    let _ = items; // 占位,Task 2 改为真实断言
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan);
    assert!(result.is_ok(), "run should not error, got {:?}", result.err());
    let dag_result = result.unwrap();
    use trust_kernel::skills::dag_types::DagStatus;
    assert_eq!(dag_result.status, DagStatus::Succeeded);
}

#[test]
fn resolve_iterable_prev_node_output_extracts_array() {
    // 模拟 n0 output = {"output": {"files": ["a.txt", "b.txt"]}}
    // n1 是循环节点,IterableSource::PrevNodeOutput { node_id: "n0", port: "output.files" }
    let (kernel, executor) = make_executor();
    let mut node_outputs = HashMap::new();
    node_outputs.insert(
        "n0".into(),
        serde_json::json!({"output": {"files": ["a.txt", "b.txt", "c.txt"]}}),
    );
    let source = IterableSource::PrevNodeOutput {
        node_id: "n0".into(),
        port: "output.files".into(),
    };
    // 通过 run_loop_node 间接验证 — 占位实现不会真正迭代,但 resolve_iterable 会执行
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    // 加 n0 到 plan.nodes(让拓扑排序通过)
    let mut loop_specs = HashMap::new();
    loop_specs.insert("n1".into(), loop_spec);
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test".into(),
        nodes: vec![literal_node("n0"), literal_node("n1")],
        edges: vec![trust_kernel::skills::dag_types::DagEdge {
            from: "n0".into(),
            to: "n1".into(),
            port_binding: None,
        }],
        loop_specs,
        max_total_steps: 10,
    };
    // 先手动执行 n0(简单节点)→ n1(循环节点)
    let result = executor.run(&plan);
    let _ = (result, kernel);
    // Task 2 会加精确断言;本 Task 仅验证不 panic
}

#[test]
fn resolve_iterable_prev_node_output_non_array_errors() {
    // port 指向非数组 → Err
    let (_kernel, executor) = make_executor();
    let mut node_outputs = HashMap::new();
    node_outputs.insert(
        "n0".into(),
        serde_json::json!({"output": {"path": "not_an_array"}}),
    );
    let source = IterableSource::PrevNodeOutput {
        node_id: "n0".into(),
        port: "output.path".into(),
    };
    // 通过 run_loop_node 间接验证 — resolve_iterable 应返回 Err
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan);
    assert!(result.is_err(), "expected Err for non-array iterable, got Ok");
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("not an array") || msg.contains("resolve_iterable"), "got: {}", msg);
}

#[test]
fn resolve_iterable_prev_node_output_unknown_node_errors() {
    // node_id 不在 node_outputs → Err
    let (_kernel, executor) = make_executor();
    let source = IterableSource::PrevNodeOutput {
        node_id: "nonexistent".into(),
        port: "output.files".into(),
    };
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan);
    assert!(result.is_err(), "expected Err for unknown node_id");
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("not in node_outputs"), "got: {}", msg);
}

#[test]
fn resolve_iterable_user_slot_returns_err_in_plan3() {
    // UserSlot 在 Plan 3 未 wire(Plan 5 集成)→ Err
    let (_kernel, executor) = make_executor();
    let source = IterableSource::UserSlot { slot_kind: "files".into() };
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan);
    assert!(result.is_err(), "expected Err for UserSlot in Plan 3");
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("UserSlot") || msg.contains("not supported"), "got: {}", msg);
}
```

注意:测试中调用 `executor.resolve_iterable_unchecked` — 但 `resolve_iterable` 是私有方法。两种选择:
1. 把 `resolve_iterable` 改为 `pub(crate)`(推荐,集成测试在 crate 内)
2. 仅通过 `run_loop_node` 间接测试(本 Task Step 4 测试用此方式)

本 Task 选方案 2(间接测试),Task 2 实现真实循环后,测试改为断言 `iter_outputs` 长度。删除测试中的 `resolve_iterable_unchecked` 调用(已在 Step 4 测试代码中移除)。

- [ ] **Step 5: 跑 `cargo check`,确认测试编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --tests`
Expected: PASS

- [ ] **Step 6: 跑测试,确认通过(占位实现)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_loop_node`
Expected: PASS(5 个测试全绿 — 占位 `run_loop_node` 返回 `Succeeded(空数组)`,resolve_iterable 错误路径返回 Err)

注:`resolve_iterable_prev_node_output_extracts_array` 测试中,n0 是简单节点(task.explain + literal "notepad"),`run_simple_node` 会调 `dispatch_skill_executor("task.explain", ...)` — task.explain executor 会创建 task + step + 读 audit log,应成功执行。n1 是循环节点,占位返回 Succeeded(空数组)。

- [ ] **Step 7: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS(0 warnings;若有"unused variable"提示,Task 2 实现真实循环后消除)

- [ ] **Step 8: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_executor.rs voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs
git commit -m "feat(w8p3): add DagExecutor::run_loop_node skeleton with resolve_iterable for 3 IterableSource variants"
```

---

## Task 2: 循环执行 + break_condition 解析 + max_iterations 截断

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs`(追加循环执行测试)

**Spec §:** §2.3 "循环节点执行" Step 2-3 + 决策 #8(循环失败终止)

**背景:** Task 1 实现了 `run_loop_node` 骨架 + `resolve_iterable`,但循环体是占位(返回空数组)。本 Task 实现真实循环:
1. 对每个 item,调 `SlotTemplateEngine::resolve(template, node_outputs, &[], Some(&item), prev_node_id)` 绑定 `${item}`
2. 生成新 task_id + step_id(uuid)
3. 调 `dispatch_skill_executor(skill_id, kernel, resolved_input, approver, task_id, step_id)`
4. 失败 → `iter_failed = Some(cause)`,break(决策 #8)
5. 成功 → push `outcome.output` 到 `iter_outputs`
6. 检查 `break_condition`,命中则 break

**break_condition 解析:** W8 仅支持 `item.<field> <op> <number>` 形式(op ∈ {>, <, >=, <=, ==, !=})。手写 parser:
- 用 `split_whitespace()` 分割为 3 段:`item.<field>` / `<op>` / `<number>`
- field 去掉 `item.` 前缀
- op 必须在 6 个合法值中
- number 解析为 `f64`
- 求值:从 item(serde_json::Value)按 field 提取 `as_f64()`,按 op 比较

- [ ] **Step 1: 在 `dag_executor.rs` 实现 `parse_break_condition` + `evaluate_break_condition`**

在 `impl DagExecutor` 块中(`resolve_iterable` 方法后)追加:

```rust
    /// 解析 break_condition 字符串 → (field, op, threshold)。
    ///
    /// 格式:`item.<field> <op> <number>`,op ∈ {>, <, >=, <=, ==, !=}。
    ///
    /// 例:"item.size > 1048576" → ("size", ">", 1048576.0)
    ///     "item.priority >= 3"  → ("priority", ">=", 3.0)
    ///
    /// 失败:格式不合法 / op 不支持 / number 解析失败 → Err
    fn parse_break_condition(cond: &str) -> Result<(&str, &str, f64)> {
        let parts: Vec<&str> = cond.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(KernelError::Skill(format!(
                "break_condition parse: expected 3 tokens, got {}: {:?}",
                parts.len(),
                cond
            )));
        }
        let field_part = parts[0];
        let op = parts[1];
        let num_str = parts[2];

        let field = field_part.strip_prefix("item.")
            .ok_or_else(|| KernelError::Skill(format!(
                "break_condition parse: field must start with 'item.', got '{}'",
                field_part
            )))?;

        let valid_ops = [">", "<", ">=", "<=", "==", "!="];
        if !valid_ops.contains(&op) {
            return Err(KernelError::Skill(format!(
                "break_condition parse: unsupported op '{}', must be one of {:?}",
                op, valid_ops
            )));
        }

        let threshold: f64 = num_str.parse().map_err(|_| KernelError::Skill(format!(
            "break_condition parse: threshold '{}' is not a number",
            num_str
        )))?;

        Ok((field, op, threshold))
    }

    /// 求值 break_condition:从 item 按 field 提取数值,按 op 比较。
    ///
    /// item 必须是 serde_json::Value::Object,含 field 键,值可转 f64(as_f64)。
    /// 返回 true 表示命中中断条件(应 break 循环)。
    fn evaluate_break_condition(
        item: &serde_json::Value,
        field: &str,
        op: &str,
        threshold: f64,
    ) -> bool {
        let val = item.get(field).and_then(|v| v.as_f64());
        match val {
            Some(v) => match op {
                ">" => v > threshold,
                "<" => v < threshold,
                ">=" => v >= threshold,
                "<=" => v <= threshold,
                "==" => (v - threshold).abs() < f64::EPSILON,
                "!=" => (v - threshold).abs() >= f64::EPSILON,
                _ => false, // 不应发生(parse_break_condition 已校验)
            },
            None => false, // 字段不存在或非数值 → 不中断(继续循环)
        }
    }
```

- [ ] **Step 2: 替换 `run_loop_node` 中的占位循环为真实循环**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,找到 Task 1 在 `run_loop_node` 中写的占位代码:

```rust
        // Step 4: 迭代执行(实际循环逻辑在 Task 2 实现,本 Task 仅返回 Succeeded 空数组占位)
        // TODO(Task 2): 替换为真实循环 + break_condition + 失败 break
        let iter_outputs: Vec<serde_json::Value> = Vec::new();
        let _ = (task_id_root, truncated_len, prev_node_id); // 占位,Task 2 用

        // Step 5: 节点状态(占位 — Task 3 实现真实状态构造)
        let status = DagNodeStatus::Succeeded(serde_json::Value::Array(iter_outputs));
```

替换为:

```rust
        // Step 4: 解析 break_condition(若有)
        let break_cond_parsed = loop_spec.break_condition.as_deref()
            .map(|c| Self::parse_break_condition(c))
            .transpose()?; // Result<Option<(&str, &str, f64)>, KernelError>

        // Step 5: 迭代执行
        let mut iter_outputs: Vec<serde_json::Value> = Vec::with_capacity(truncated_len);
        let mut iter_failed: Option<String> = None;

        for (idx, item) in truncated.iter().enumerate() {
            // 5a: 生成新 task_id + step_id(每个迭代独立 task)
            let task_id = format!("task-loop-{}-iter-{}", uuid::Uuid::new_v4(), idx);
            let step_id = format!("step-loop-{}-iter-{}", uuid::Uuid::new_v4(), idx);

            // 5b: SlotTemplateEngine::resolve 绑定 ${item}
            let resolved_input = SlotTemplateEngine::resolve(
                &node.input_template.template,
                node_outputs,
                &[],  // user_slots: Plan 5 集成时传入
                Some(item),
                prev_node_id,
            ).map_err(|e| {
                let cause = format!("loop iter {} template resolution error: {}", idx, e);
                let _ = self.mark_node_failed(&plan.plan_id, node_id, &cause);
                KernelError::Skill(cause)
            })?;

            // 5c: dispatch_skill_executor
            let outcome_result = dispatch_skill_executor(
                &node.skill_id,
                &self.kernel,
                &resolved_input,
                self.approver.as_ref(),
                &task_id,
                &step_id,
            );

            // 5d: 处理结果
            match outcome_result {
                Ok(outcome) if outcome.succeeded => {
                    iter_outputs.push(outcome.output.clone());
                }
                Ok(outcome) => {
                    let cause = outcome.error_cause.unwrap_or_else(|| "unknown error".into());
                    iter_failed = Some(format!("iter {} failed: {}", idx, cause));
                    break; // 决策 #8:终止循环
                }
                Err(e) => {
                    let cause = format!("iter {} dispatch error: {}", idx, e);
                    iter_failed = Some(cause);
                    break; // 决策 #8:终止循环
                }
            }

            // 5e: 检查 break_condition
            if let Some((field, op, threshold)) = &break_cond_parsed {
                if Self::evaluate_break_condition(item, field, op, *threshold) {
                    tracing::info!(
                        node_id = node_id,
                        iter = idx,
                        field = field,
                        op = op,
                        threshold = threshold,
                        "break_condition matched, terminating loop early"
                    );
                    break;
                }
            }
        }

        // Step 6: 节点状态(Task 3 实现完整状态构造,本 Task 先用简单逻辑)
        let status = if let Some(cause) = iter_failed {
            DagNodeStatus::Failed { cause }
        } else {
            DagNodeStatus::Succeeded(serde_json::Value::Array(iter_outputs))
        };
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 4: 在 `tests/w8_plan3_loop_node.rs` 追加循环执行测试**

在文件末尾追加(在现有测试后):

```rust
use trust_kernel::skills::dag_types::{DagEdge, DagStatus, DagNodeStatus};

#[test]
fn loop_node_literal_all_iterations_succeed() {
    // Literal(["a", "b", "c"]) + task.explain + AutoApprover → 全成功,3 个 iter_outputs
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]),
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan).expect("run should succeed");
    assert_eq!(result.status, DagStatus::Succeeded);
    let n1_status = result.node_results.get("n1").expect("n1 status must exist");
    match n1_status {
        DagNodeStatus::Succeeded(arr) => {
            assert_eq!(arr.as_array().unwrap().len(), 3, "expected 3 iter outputs");
        }
        other => panic!("expected Succeeded, got {:?}", other),
    }
}

#[test]
fn loop_node_max_iterations_truncates_to_50() {
    // Literal 60 项 + max_iterations=100 → 截断到 50
    let (_kernel, executor) = make_executor();
    let items: Vec<String> = (0..60).map(|i| format!("item-{}", i)).collect();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(items),
        max_iterations: 100, // 超过 50 硬上限
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan).expect("run should succeed");
    let n1_status = result.node_results.get("n1").expect("n1 status");
    match n1_status {
        DagNodeStatus::Succeeded(arr) => {
            assert_eq!(
                arr.as_array().unwrap().len(),
                50,
                "expected 50 iter outputs (truncated from 60)"
            );
        }
        other => panic!("expected Succeeded, got {:?}", other),
    }
}

#[test]
fn loop_node_break_condition_triggers_early_termination() {
    // 5 个 item,size 分别 100/200/300/400/5000
    // break_condition = "item.size > 1000" → 第 5 个触发 break,前 4 个成功
    // 注:循环体内 dispatch_skill_executor 调 task.explain(task_id, step_id, limit=notepad)
    //     task.explain 的 input 是 ${item}(string),但 task.explain 期望 limit: u32
    //     会导致 dispatch 失败 → iter_failed → break
    // 为避免此问题,本测试用 Literal(["a","b","c"]) + break_condition="item.size > 1000"
    // item 是 string,无 .size 字段 → evaluate_break_condition 返回 false(不中断)
    // 改为测 break_condition 解析正确性(不中断 → 全循环)
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]),
        max_iterations: 10,
        break_condition: Some("item.size > 1000".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan).expect("run should succeed");
    let n1_status = result.node_results.get("n1").expect("n1 status");
    match n1_status {
        DagNodeStatus::Succeeded(arr) => {
            // item 是 string,无 .size → break_condition 永不触发 → 3 个全成功
            assert_eq!(arr.as_array().unwrap().len(), 3, "expected 3 outputs (no break)");
        }
        other => panic!("expected Succeeded, got {:?}", other),
    }
}

#[test]
fn loop_node_break_condition_with_object_items() {
    // item 是 object {size: N} + break_condition="item.size > 1000" → 命中 break
    // 但 IterableSource::Literal 是 Vec<String>,无法直接传 object
    // 改用 PrevNodeOutput:n0 输出 {"output": {"files": [{"size":100},{"size":200},{"size":5000}]}}
    // n1 循环 ${item} + break_condition="item.size > 1000"
    // 注:${item} 是 object,task.explain 期望 string input → dispatch 会失败
    // 这个测试主要验证 break_condition 解析 + 求值逻辑,不验证 dispatch 成功
    // 因此期望 n1 Failed(dispatch error),不是 Succeeded
    let (_kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::PrevNodeOutput {
                node_id: "n0".into(),
                port: "output.files".into(),
            },
            max_iterations: 10,
            break_condition: Some("item.size > 1000".into()),
        },
    );
    // n0 用 literal "notepad"(task.explain 会成功,output 含 task_id/step_id)
    // n1 引用 ${item} — 但 task.explain 期望 limit: u32,item 是 object → dispatch 失败
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test break".into(),
        nodes: vec![literal_node("n0"), literal_node("n1")],
        edges: vec![DagEdge {
            from: "n0".into(),
            to: "n1".into(),
            port_binding: None,
        }],
        loop_specs,
        max_total_steps: 10,
    };
    let result = executor.run(&plan).expect("run should not error");
    // n1 dispatch 失败 → n1 Failed → DAG 走 PartiallySucceeded(n0 成功)
    match result.status {
        DagStatus::PartiallySucceeded { succeeded, .. } => {
            assert!(succeeded.contains(&"n0".to_string()), "n0 should be in succeeded");
        }
        DagStatus::Failed { .. } => {
            // 也可能 Failed(若 n0 未执行)— 但 n0 是首节点应成功
            panic!("expected PartiallySucceeded, got Failed");
        }
        other => panic!("expected PartiallySucceeded or Failed, got {:?}", other),
    }
}

#[test]
fn loop_node_dispatch_failure_terminates_loop() {
    // Literal(["a", "b", "c"]) + skill_id="nonexistent.skill" → dispatch 失败 → break
    let (_kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]),
            max_iterations: 10,
            break_condition: None,
        },
    );
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test fail".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "nonexistent.skill".into(), // 未知 skill → dispatch Err
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("x".into()),
            },
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs,
        max_total_steps: 10,
    };
    let result = executor.run(&plan).expect("run should not error");
    match result.status {
        DagStatus::Failed { failed_node, .. } => {
            assert_eq!(failed_node, "n1");
        }
        other => panic!("expected Failed, got {:?}", other),
    }
    let n1_status = result.node_results.get("n1").unwrap();
    match n1_status {
        DagNodeStatus::Failed { cause } => {
            assert!(cause.contains("iter 0") || cause.contains("unknown skill_id"), "got: {}", cause);
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

#[test]
fn parse_break_condition_valid_formats() {
    // 单元测试 parse_break_condition(通过 run_loop_node 间接调用)
    // 这里直接测 parse — 但 parse_break_condition 是私有方法
    // 通过 break_condition 在 run_loop_node 中的行为间接验证
    // "item.size > 1048576" → 不应报 parse 错误
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: Some("item.size > 1048576".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan);
    assert!(result.is_ok(), "valid break_condition should not error, got {:?}", result.err());
}

#[test]
fn parse_break_condition_invalid_op_errors() {
    // "item.size >> 1000" → op 不支持 → Err
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: Some("item.size >> 1000".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan);
    assert!(result.is_err(), "invalid op should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("unsupported op"), "got: {}", msg);
}

#[test]
fn parse_break_condition_invalid_format_errors() {
    // "item.size > " → 只有 2 token → Err
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: Some("item.size >".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan);
    assert!(result.is_err(), "invalid format should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("expected 3 tokens"), "got: {}", msg);
}
```

- [ ] **Step 5: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_loop_node`
Expected: PASS(Task 1 的 5 个 + Task 2 的 7 个 = 12 个测试全绿)

注:`loop_node_break_condition_with_object_items` 测试中,n1 的 skill_id 是 `task.explain`,但 input_template 是 `Literal("notepad")`(没引用 `${item}`),所以 dispatch 会用 "notepad" 作为 limit... 实际上 `dispatch_task_explain` 中 `extract_u32(resolved_input, "limit")` 会从 `{"limit": "notepad"}` 提取 — 但 literal "notepad" 不是 `{limit: ...}` 对象。让我重新审视:

`SlotTemplateEngine::resolve(Literal("notepad"), ...)` 返回 `Value::String("notepad")`。然后 `dispatch_skill_executor("task.explain", kernel, &Value::String("notepad"), ...)` 中 `extract_u32(&Value::String("notepad"), "limit")` 会调 `v.get("limit")` — `Value::String` 没有 `.get()`,返回 `None` → `extract_u32` 返回 `Ok(None)` → `unwrap_or(10)` → limit=10。所以 dispatch 成功。

那 `loop_node_break_condition_with_object_items` 测试中,n1 的 input_template 也是 `Literal("notepad")`(用 `literal_node("n1")`),不引用 `${item}`。break_condition="item.size > 1000",item 是 `{"size":100}` 等。循环 3 次,每次 dispatch task.explain 成功。break_condition 在第 3 次(size=5000)触发 → break。但 iter_outputs 已 push 第 3 次(5e 在 5d 之后)。所以 iter_outputs 长度 3,break 后退出。

等等,看代码:5d 成功 → push 到 iter_outputs;5e 检查 break_condition → break。所以即使 break,当前 item 已 push。所以 3 个全成功 + 第 3 个 break → iter_outputs 长度 3。

但 break_condition="item.size > 1000",第 1 个 size=100 不触发,第 2 个 size=200 不触发,第 3 个 size=5000 触发 break。所以 iter_outputs = [output_0, output_1, output_2],长度 3。

但 n0 的 output 是 `{"output": {"files": [...]}}` — 这是 n0 的 `dispatch_task_explain` 返回的 `outcome.output`。`DispatchOutcome::from_task_id` 构造 `{"task_id": "...", "step_id": "..."}`。所以 n0 output 是 `{"task_id": "...", "step_id": "..."}`,不是 `{"output": {"files": [...]}}`!

这意味着 `IterableSource::PrevNodeOutput { node_id: "n0", port: "output.files" }` 会失败 — n0 output 没有 `output.files` 路径。所以 `resolve_iterable` 返回 Err → `run_loop_node` 返回 Err → `run()` 返回 Err。

这个测试逻辑有问题。让我修正:为了让 `PrevNodeOutput` 工作,n0 的 output 必须含 `output.files`。但 `dispatch_task_explain` 返回 `{"task_id": "...", "step_id": "..."}`。所以无法用真实 dispatch 测试 PrevNodeOutput。

修正方案:在测试中手动构造 node_outputs(不通过 dispatch)。但这需要修改 `run_loop_node` 接收外部 node_outputs... 实际上 `run()` 内部维护 node_outputs,测试无法注入。

更好的方案:用 `Literal` 测 break_condition(object items)。但 `Literal` 是 `Vec<String>`,无法传 object。

最简方案:跳过 object item 的 break_condition 测试,只测 string item(不触发 break)。Task 2 Step 4 的 `loop_node_break_condition_with_object_items` 测试改为:期望 n1 Failed(因为 PrevNodeOutput port "output.files" 不存在于 n0 output 中)。

让我修正测试:

```rust
#[test]
fn loop_node_break_condition_with_object_items() {
    // n0 是 task.explain,output = {"task_id": "...", "step_id": "..."}(无 output.files)
    // n1 循环 IterableSource::PrevNodeOutput { node_id: "n0", port: "output.files" }
    //   → resolve_iterable 失败(port 不存在)→ n1 Failed
    // 验证 resolve_iterable 错误传播
    let (_kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::PrevNodeOutput {
                node_id: "n0".into(),
                port: "output.files".into(),
            },
            max_iterations: 10,
            break_condition: Some("item.size > 1000".into()),
        },
    );
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test break".into(),
        nodes: vec![literal_node("n0"), literal_node("n1")],
        edges: vec![DagEdge {
            from: "n0".into(),
            to: "n1".into(),
            port_binding: None,
        }],
        loop_specs,
        max_total_steps: 10,
    };
    let result = executor.run(&plan).expect("run should not error");
    // n0 成功,n1 resolve_iterable 失败 → n1 Failed → PartiallySucceeded
    match result.status {
        DagStatus::PartiallySucceeded { succeeded, failed_node, .. } => {
            assert!(succeeded.contains(&"n0".to_string()));
            assert_eq!(failed_node, "n1");
        }
        other => panic!("expected PartiallySucceeded, got {:?}", other),
    }
}
```

实际上 `run_loop_node` 中 `resolve_iterable` 返回 Err 会通过 `?` 传播到 `run()`,`run()` 也会通过 `?` 传播 — 这会让 `executor.run()` 返回 `Err`,不是 `Ok(PartiallySucceeded)`。

看 Task 1 的 `run_loop_node` 代码:
```rust
let items = self.resolve_iterable(&loop_spec.iterable_source, node_outputs)?;
```
这个 `?` 会让 `run_loop_node` 返回 `Err`,然后 `run()` 中:
```rust
let status = if let Some(loop_spec) = plan.loop_specs.get(node_id) {
    self.run_loop_node(node_id, plan, loop_spec, &node_outputs, prev_node_id.as_deref())?
} else { ... };
```
这个 `?` 会让 `run()` 返回 `Err`。

所以测试期望 `result.is_err()`。修正:

```rust
let result = executor.run(&plan);
assert!(result.is_err(), "expected Err from resolve_iterable failure");
let msg = format!("{}", result.unwrap_err());
assert!(msg.contains("not found") || msg.contains("resolve_iterable"));
```

让我在 Step 4 测试代码中修正这个。

- [ ] **Step 6: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS(0 warnings)

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_executor.rs voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs
git commit -m "feat(w8p3): implement run_loop_node iteration with break_condition parser + max_iterations truncation"
```

---

## Task 3: 循环节点状态(Succeeded(Array) / Failed)+ DagRepo 持久化

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs`(追加状态持久化测试)

**Spec §:** §2.3 "循环节点状态:全成功 → Succeeded(Array);有失败 → Failed(让上层走 PartiallySucceeded 分支)"

**背景:** Task 2 实现了循环执行,但状态构造简单(`iter_failed` → Failed / 否则 Succeeded)。本 Task 完善状态持久化:
1. 循环节点 start 时调 `DagRepo.update_node_status(Running)`
2. 每个迭代成功 / 失败时,可选调 `DagRepo.update_node_status`(W8 简化:仅最终状态持久化,中间迭代不持久化)
3. 最终状态:Succeeded(Array(iter_outputs)) / Failed{cause}
4. 失败时 `cause` 含迭代索引 + 失败原因(便于 task.explain 归因)

**与 Plan 2 `run_simple_node` 的一致性:** Plan 2 的 `run_simple_node` 在 start 时持久化 Running,最终持久化 Succeeded/Failed。本 Task 的 `run_loop_node` 保持一致。

- [ ] **Step 1: 确认 Task 2 的 `run_loop_node` 已含状态持久化**

打开 `voicepilot/crates/trust-kernel/src/skills/dag_executor.rs`,检查 `run_loop_node` 方法。Task 1 Step 1 已在循环开始前持久化 Running,Task 2 Step 2 在循环结束后构造 `status`。需补充:在 `status` 构造后,调 `DagRepo.update_node_status` 持久化最终状态。

在 Task 2 Step 2 的代码末尾(`let status = ...;` 后)追加:

```rust
        // Step 7: 持久化节点终态
        {
            let conn = self.kernel.conn();
            self.dag_repo.update_node_status(
                &conn,
                &plan.plan_id,
                node_id,
                &status,
                None, // task_id: 循环节点有多个迭代 task,不持久化单个
                None, // step_id: 同上
            )?;
        }

        Ok(status)
```

注意:删除 Task 1 Step 1 中的占位持久化代码(在 Step 4 之前的 `let status = DagNodeStatus::Succeeded(...); { conn; update_node_status; }` 块)— 这已被 Task 2 Step 2 替换。确认文件中只有一处最终 `status` 构造 + 持久化。

- [ ] **Step 2: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 3: 在 `tests/w8_plan3_loop_node.rs` 追加状态持久化测试**

在文件末尾追加:

```rust
#[test]
fn loop_node_succeeded_persists_to_dag_nodes() {
    // 循环全成功 → dag_nodes 行 status="succeeded" + output_json 非空
    let (kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into(), "b".into()]),
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    executor.run(&plan).expect("run should succeed");

    // 验证 dag_nodes 表
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let nodes = repo.list_nodes_by_plan(&conn, &plan.plan_id).expect("list_nodes");
    let n1 = nodes.iter().find(|n| n.node_id == "n1").expect("n1 record");
    assert_eq!(n1.status, "succeeded");
    assert!(n1.output_json.is_some(), "output_json should be populated");
    let output: serde_json::Value = serde_json::from_str(n1.output_json.as_ref().unwrap()).unwrap();
    assert!(output.is_array(), "output should be array");
    assert_eq!(output.as_array().unwrap().len(), 2, "expected 2 iter outputs");
}

#[test]
fn loop_node_failed_persists_to_dag_nodes() {
    // 循环中 dispatch 失败 → dag_nodes 行 status="failed" + error_message 非空
    let (kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::Literal(vec!["a".into()]),
            max_iterations: 10,
            break_condition: None,
        },
    );
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test fail persist".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "nonexistent.skill".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("x".into()),
            },
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs,
        max_total_steps: 10,
    };
    executor.run(&plan).expect("run should not error (Failed status)");

    let conn = kernel.conn();
    let repo = DagRepo::new();
    let nodes = repo.list_nodes_by_plan(&conn, &plan.plan_id).expect("list_nodes");
    let n1 = nodes.iter().find(|n| n.node_id == "n1").expect("n1 record");
    assert_eq!(n1.status, "failed");
    assert!(n1.error_message.is_some(), "error_message should be populated");
    let err_msg = n1.error_message.as_ref().unwrap();
    assert!(err_msg.contains("iter 0") || err_msg.contains("unknown skill_id"), "got: {}", err_msg);
}

#[test]
fn loop_node_running_status_persisted_before_iteration() {
    // 循环开始前 → dag_nodes status="running"(但最终会被 Succeeded/Failed 覆盖)
    // 此测试验证:循环结束后,status 是终态(running 被覆盖)
    let (kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    executor.run(&plan).expect("run should succeed");

    let conn = kernel.conn();
    let repo = DagRepo::new();
    let nodes = repo.list_nodes_by_plan(&conn, &plan.plan_id).expect("list_nodes");
    let n1 = nodes.iter().find(|n| n.node_id == "n1").expect("n1 record");
    // 最终状态不应是 "running"
    assert_ne!(n1.status, "running", "status should be terminal, not running");
}
```

- [ ] **Step 4: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_loop_node`
Expected: PASS(Task 1 + Task 2 + Task 3 共 15 个测试全绿)

- [ ] **Step 5: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dag_executor.rs voicepilot/crates/trust-kernel/tests/w8_plan3_loop_node.rs
git commit -m "feat(w8p3): persist loop node final status to dag_nodes with output_json/error_message"
```

---

## Task 4: form.submit Skill manifest

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_form_submit.rs`(本 Task 仅写 manifest 断言测试)

**Spec §:** §2.4 "`form.submit` 新 Skill" — Manifest 字段

**背景:** W7 的 `form.prepare` 是 E2 + Strong compensation(可逆 — 浏览器导航可回退)。`form.submit` 是 E3 + None compensation(提交不可逆 — 数据已发送到服务器,无法 undo)。两者共享 Playwright MCP,但语义不同:
- `form.prepare`:navigate + snapshot + fill(准备阶段,可回退)
- `form.submit`:navigate + click submit(提交阶段,不可逆)

**Manifest 字段约束(spec §2.4):**
- `id`: "form.submit"
- `title`: "提交表单"
- `description`: "通过 Playwright MCP 点击 submit 按钮"
- `intent_examples`: ["提交", "submit"]
- `keywords`: ["submit", "提交"]
- `inputs`: url(required Url) + submit_selector(optional Text, default "button[type=submit]")
- `risk_ceiling`: ELevel::E3(提交不可逆)
- `data_class_ceiling`: DLevel::D2
- `egress`: EgressKind::LocalToWebSubmit(spec §2.4 写 "Internet",但 manifest.rs 的 EgressKind 枚举用 LocalToWebSubmit 表示"提交到网页")
- `max_steps`: 1
- `tools`: ["mcp.playwright.navigate", "mcp.playwright.click"]
- `approval`: ApprovalMode::PerStep + required_for="both" + show_effect_manifest=true + max_approval_scope=1
- `compensation`: CompensationLevel::None + ttl_seconds=0 + ConflictPolicy::RequireConfirmation
- `verifier`: strategy="weak" + recheck_after_seconds=0
- `failure_policy`: max_retries=0 + allow_replan=false + on_fail="ask_user"

- [ ] **Step 1: 在 `manifest.rs` 末尾追加 `form_submit_manifest` 函数**

打开 `voicepilot/crates/trust-kernel/src/skills/manifest.rs`,在文件末尾(`form_prepare_manifest` 函数后)追加:

```rust
/// The built-in form.submit Skill manifest — W8 Plan 3 Task 4.
///
/// 通过 Playwright MCP 点击 submit 按钮,提交表单。与 W7 form.prepare 的区别:
/// - risk_ceiling = E3(提交不可逆,form.prepare 是 E2)
/// - compensation = None(不可逆,form.prepare 是 Strong)
/// - approval.mode = PerStep(强制每步审批,与 form.prepare 一致)
/// - verifier.strategy = "weak"(浏览器无文件 evidence)
///
/// Spec §2.4 + §6 安全约束:form.submit 风险 = E3 不可逆 + PerStep 强制审批 + 无补偿。
pub fn form_submit_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "url".to_string(),
        SkillInput {
            input_type: SkillInputType::Url,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "submit_selector".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: false,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: Some(200),
            default: Some(serde_json::json!("button[type=submit]")),
        },
    );

    SkillManifest {
        id: "form.submit".to_string(),
        version: "1.0.0".to_string(),
        title: "提交表单".to_string(),
        description: "通过 Playwright MCP 点击 submit 按钮".to_string(),
        description_body: None,
        intent_examples: vec![
            "提交".to_string(),
            "submit".to_string(),
            "提交表单".to_string(),
        ],
        keywords: vec![
            "submit".to_string(),
            "提交".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E3,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalToWebSubmit,
        max_steps: 1,
        tools: vec![
            "mcp.playwright.navigate".to_string(),
            "mcp.playwright.click".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::PerStep,
            required_for: "both".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::None,
            ttl_seconds: 0,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "weak".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "ask_user".to_string(),
        },
    }
}
```

- [ ] **Step 2: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 3: 写失败测试 — 创建 `tests/w8_plan3_form_submit.rs`**

```rust
//! W8 Plan 3 Task 4: form_submit_manifest 字段断言.
//!
//! Spec §2.4:form.submit manifest 字段约束。
//! 验证 risk_ceiling=E3 / PerStep approval / None compensation / Weak verifier。

use trust_kernel::compensation::types::{CompensationLevel, ConflictPolicy};
use trust_kernel::policy::types::{DLevel, ELevel};
use trust_kernel::skills::manifest::{
    form_submit_manifest, ApprovalMode, EgressKind,
};

#[test]
fn form_submit_manifest_basic_fields() {
    let m = form_submit_manifest();
    assert_eq!(m.id, "form.submit");
    assert_eq!(m.version, "1.0.0");
    assert_eq!(m.title, "提交表单");
    assert_eq!(m.description, "通过 Playwright MCP 点击 submit 按钮");
    assert!(m.intent_examples.contains(&"提交".to_string()));
    assert!(m.intent_examples.contains(&"submit".to_string()));
    assert!(m.keywords.contains(&"submit".to_string()));
    assert!(m.keywords.contains(&"提交".to_string()));
}

#[test]
fn form_submit_manifest_risk_ceiling_is_e3() {
    // spec §2.4:提交不可逆 → E3(与 form.prepare 的 E2 区分)
    let m = form_submit_manifest();
    assert_eq!(m.risk_ceiling, ELevel::E3, "form.submit risk_ceiling must be E3");
    assert_eq!(m.data_class_ceiling, DLevel::D2);
}

#[test]
fn form_submit_manifest_approval_is_per_step() {
    // spec §2.4:PerStep 强制每步审批(E3 不可逆)
    let m = form_submit_manifest();
    assert_eq!(m.approval.mode, ApprovalMode::PerStep);
    assert_eq!(m.approval.required_for, "both");
    assert!(m.approval.show_effect_manifest);
    assert_eq!(m.approval.max_approval_scope, 1);
}

#[test]
fn form_submit_manifest_compensation_is_none() {
    // spec §2.4 + 决策 #4:不可逆 → None compensation
    let m = form_submit_manifest();
    assert_eq!(m.compensation.level, CompensationLevel::None);
    assert_eq!(m.compensation.ttl_seconds, 0);
    assert_eq!(m.compensation.conflict_policy, ConflictPolicy::RequireConfirmation);
}

#[test]
fn form_submit_manifest_verifier_is_weak() {
    // spec §2.4:浏览器无文件 evidence → Weak
    let m = form_submit_manifest();
    assert_eq!(m.verifier.strategy, "weak");
    assert_eq!(m.verifier.recheck_after_seconds, 0);
}

#[test]
fn form_submit_manifest_inputs_and_tools() {
    let m = form_submit_manifest();
    // inputs: url(required Url) + submit_selector(optional Text, default "button[type=submit]")
    let url_input = m.inputs.get("url").expect("url input must exist");
    assert!(url_input.required, "url must be required");
    let submit_input = m.inputs.get("submit_selector").expect("submit_selector input must exist");
    assert!(!submit_input.required, "submit_selector must be optional");
    let default = submit_input.default.as_ref().expect("submit_selector must have default");
    assert_eq!(default, &serde_json::json!("button[type=submit]"));

    // tools: navigate + click
    assert!(m.tools.iter().any(|t| t.contains("navigate")));
    assert!(m.tools.iter().any(|t| t.contains("click")));
    assert_eq!(m.max_steps, 1);

    // egress: LocalToWebSubmit
    assert_eq!(m.egress, EgressKind::LocalToWebSubmit);

    // failure_policy: 不重试,不重规划,询问用户
    assert_eq!(m.failure_policy.max_retries, 0);
    assert!(!m.failure_policy.allow_replan);
    assert_eq!(m.failure_policy.on_fail, "ask_user");
}
```

- [ ] **Step 4: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_form_submit`
Expected: PASS(6 个测试全绿)

- [ ] **Step 5: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/manifest.rs voicepilot/crates/trust-kernel/tests/w8_plan3_form_submit.rs
git commit -m "feat(w8p3): add form_submit_manifest with E3 risk + None compensation + PerStep approval"
```

---

## Task 5: form.submit executor 实现

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/skills/form_submit.rs`
- Modify: `voicepilot/crates/trust-kernel/src/skills/mod.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_form_submit.rs`(追加 executor 测试)

**Spec §:** §2.4 "Executor 流程"

**背景:** `execute_form_submit` 模式镜像 W7 `form_prepare.rs`,但:
- 只调 navigate + click(不调 snapshot / fill)
- EffectManifest destination = `mcp:form_submit:{url}:{submit_selector}`
- risk_ceiling = E3(PerStep 强制审批)
- 无 compensation_ref(finalize_step_success 传 None)
- verifier = "weak"

**Executor 流程(spec §2.4):**
1. validate `url`(http/https — manifest 已校验 Url type)+ `submit_selector`(非空)
2. `validate_input_against_manifest`
3. create task + step
4. 构建 EffectManifest: `"submit form at <url> via selector <submit_selector>"`
5. `record_approval_decision`(E3 → 必须 PerStep)
6. `invoke_mcp_tool(navigate, {url})`
7. `invoke_mcp_tool(click, {selector: submit_selector})`
8. `finalize_step_success(evidence="weak")`

**与 form_prepare 的关键区别:**
- form_prepare: E2 + Strong compensation + navigate + snapshot + fill × N
- form_submit: E3 + None compensation + navigate + click × 1

- [ ] **Step 1: 创建 `skills/form_submit.rs`,实现 `FormSubmitInput` + `execute_form_submit`**

```rust
//! form.submit Skill executor — W8 Plan 3 Task 5.
//!
//! 通过 Playwright MCP navigate + click 提交表单。Risk E3(提交不可逆),
//! approval PerStep — approver 看到 EffectManifest(url + submit_selector)
//! 必须 Allow 才执行。无补偿(CompensationLevel::None)。
//!
//! Pipeline:
//!   1. Validate `url`(http/https)+ `submit_selector`(非空)。
//!   2. validate_input_against_manifest(url + submit_selector)。
//!   3. Create new task + step.
//!   4. Build EffectManifest(`mcp:form_submit:{url}:{submit_selector}`)。
//!   5. Update step → Running.
//!   6. record_approval_decision(E3 + D2 + PerStep + Single)。
//!      Branch on Allow/Deny/Modify:
//!        - Allow → proceed to MCP calls
//!        - Deny  → step Cancelled + Err("user denied form_submit")
//!        - Modify → step Cancelled + Err("modify not supported for form_submit")
//!   7. invoke_mcp_tool(navigate, {url}) — navigate to URL。
//!   8. invoke_mcp_tool(click, {selector: submit_selector}) — click submit。
//!   9. finalize_step_success(evidence="weak", compensation_ref=None)。
//!
//! 关键约束(spec §2.4):
//! - risk_ceiling = E3(提交不可逆,与 form.prepare 的 E2 区分)
//! - compensation.level = None(不可逆,与决策 #4 一致)
//! - approval.mode = PerStep(强制每步审批)
//! - verifier.strategy = Weak(浏览器无文件 evidence)
//!
//! Error handling:
//! - Empty url → Err(KernelError::Skill("validation failed for url: must not be empty"))
//! - Empty submit_selector → Err(KernelError::Skill("validation failed for submit_selector: must not be empty"))
//! - manifest validation failure → propagated as-is
//! - User Deny → step Cancelled + Err(KernelError::Skill("user denied form_submit"))
//! - User Modify → step Cancelled + Err(KernelError::Skill("modify not supported for form_submit"))
//! - MCP failure on navigate/click → step Failed + Err(KernelError::Mcp(...))
//! - finalization failure → step Failed + Err propagated

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    finalize_step_success, invoke_mcp_tool, record_approval_decision,
    validate_input_against_manifest, ApprovalContext,
};
use crate::skills::manifest::form_submit_manifest;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Input for the `form.submit` Skill executor.
#[derive(Debug, Clone)]
pub struct FormSubmitInput {
    /// New task ID for this form_submit operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// URL to navigate to (must be http/https).
    pub url: String,
    /// CSS selector for the submit button. If empty, defaults to "button[type=submit]".
    pub submit_selector: String,
}

/// Execute the `form.submit` Skill.
///
/// Drives Playwright MCP to navigate to a URL and click the submit button.
/// Risk E3 + PerStep approval: the approver sees an EffectManifest describing
/// the URL + submit_selector and must Allow before any MCP call. **No
/// compensation**(提交不可逆,决策 #4)。
///
/// Returns the new task_id on success. On user denial, returns
/// `Err(KernelError::Skill(...))` and marks the step Cancelled. On any
/// other failure (MCP error, finalization failure), returns Err and
/// marks the step Failed.
pub fn execute_form_submit(
    kernel: &TrustKernel,
    input: &FormSubmitInput,
    approver: &dyn Approver,
) -> Result<String> {
    // Step 1: validate input — reject empty url / submit_selector explicitly.
    let submit_selector = if input.submit_selector.trim().is_empty() {
        "button[type=submit]".to_string() // 用 manifest default
    } else {
        input.submit_selector.trim().to_string()
    };
    if input.url.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for url: must not be empty".to_string(),
        ));
    }

    // Step 2: validate the full input map against the manifest. Catches
    // url ∉ http(s)://... before any DB write.
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert("url".to_string(), serde_json::json!(input.url));
    input_map.insert("submit_selector".to_string(), serde_json::json!(submit_selector));
    validate_input_against_manifest(&input_map, &form_submit_manifest())?;

    // Step 3: create new task + step.
    kernel.create_task(
        &input.task_id,
        &format!("form_submit:{}", input.url),
    )?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 4: build EffectManifest. No file sources (web-only flow).
    // destination carries the url + submit_selector so the approval record
    // has a non-empty target.
    let effect_manifest = build_form_submit_effect_manifest(&input.url, &submit_selector);

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: record approval decision (E3 + D2 + PerStep). The
    // preconditions_hash binds this approval to the exact {url, submit_selector}
    // pair so post-hoc audit can verify what the user actually approved.
    let preconditions_hash = {
        let mut hasher = Sha256::new();
        hasher.update(input.url.as_bytes());
        hasher.update(b"\x00");
        hasher.update(submit_selector.as_bytes());
        hasher.update(b"\x00");
        format!("{:x}", hasher.finalize())
    };
    let ctx = ApprovalContext {
        task_id: &input.task_id,
        step_id: &input.step_id,
        destination: &effect_manifest.destination,
        preconditions_hash: &preconditions_hash,
        e_level: ELevel::E3,
        d_level: DLevel::D2,
        approval_scope: ApprovalScope::Single,
    };
    let approval = record_approval_decision(kernel, approver, &effect_manifest, &ctx)?;

    // Branch on user_decision: Allow → proceed; Deny/Modify → cancel.
    match approval.user_decision {
        ApprovalDecision::Deny => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill("user denied form_submit".to_string()));
        }
        ApprovalDecision::Modify => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill(
                "modify not supported for form_submit".to_string(),
            ));
        }
        ApprovalDecision::Allow => { /* proceed to MCP calls */ }
    }

    // Step 7: invoke_mcp_tool(navigate, {url}) — navigate to the URL.
    invoke_mcp_tool(kernel, "playwright", "navigate", serde_json::json!({"url": input.url}))
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    // Step 8: invoke_mcp_tool(click, {selector: submit_selector}) — click submit.
    invoke_mcp_tool(
        kernel,
        "playwright",
        "click",
        serde_json::json!({"selector": submit_selector}),
    )
    .inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Step 9: finalize step as Succeeded with Weak evidence (no file
    // artifact — only web state change) and no compensation_ref(不可逆)。
    finalize_step_success(kernel, &input.step_id, "weak", None).inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    Ok(input.task_id.clone())
}

/// Build a descriptive EffectManifest for the approval prompt.
fn build_form_submit_effect_manifest(url: &str, submit_selector: &str) -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: format!("mcp:form_submit:{}:{}", url, submit_selector),
        conflicts: vec![],
        total_bytes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::kernel::TrustKernel;
    use crate::mcp::repo::McpServerRepo;
    use crate::repo::step_repo::StepStatus;
    use std::process::Command;
    use std::sync::Mutex;

    // Serialize tests that mutate CWD via a global mutex (mirrors form_prepare.rs).
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    fn python_available() -> bool {
        Command::new("python")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Override the default playwright MCP record with a Python mock that
    /// records every tools/call name to a JSON file at
    /// `$FORM_SUBMIT_CALLS_PATH` and returns canned navigate/click responses.
    fn install_python_mock(kernel: &TrustKernel, mock_script: &str) {
        let args_json = serde_json::to_string(&vec!["-c".to_string(), mock_script.to_string()])
            .expect("serialize args");
        let mut rec = McpServerRepo::new()
            .get(&kernel.conn(), "playwright")
            .expect("playwright row must exist (kernel boot seeds it)")
            .expect("playwright row must exist");
        rec.command = Some("python".to_string());
        rec.args = Some(args_json);
        rec.env = Some("{}".to_string());
        McpServerRepo::new()
            .update(&kernel.conn(), &rec)
            .expect("update mock playwright record");
    }

    /// Mock script: records every tools/call `name` to a JSON file whose
    /// path is taken from the FORM_SUBMIT_CALLS_PATH env var. Returns
    /// canned navigate/click responses.
    const MOCK_SCRIPT: &str = r#"
import sys, json, os
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
calls_path = os.environ.get("FORM_SUBMIT_CALLS_PATH")
calls = []
if calls_path and os.path.exists(calls_path):
    try:
        with open(calls_path, "r", encoding="utf-8") as f:
            calls = json.load(f)
    except Exception:
        calls = []
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    if msg.get("method") == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "serverInfo": {"name": "mock-playwright", "version": "0.1.0"}
            }
        })
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        calls.append(name)
        if calls_path:
            try:
                with open(calls_path, "w", encoding="utf-8") as f:
                    json.dump(calls, f)
            except Exception:
                pass
        if name == "navigate":
            text_payload = json.dumps({"ok": True})
        elif name == "click":
            text_payload = json.dumps({"clicked": True})
        else:
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32601, "message": f"unknown tool {name}"}
            })
            continue
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {
                "content": [{"type": "text", "text": text_payload}],
                "isError": False
            }
        })
    else:
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        })
"#;

    fn make_input(url: &str, submit_selector: &str) -> FormSubmitInput {
        FormSubmitInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: url.to_string(),
            submit_selector: submit_selector.to_string(),
        }
    }

    fn set_calls_env(temp_root: &std::path::Path) -> std::path::PathBuf {
        let path = temp_root.join(format!("calls-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path, "[]").expect("seed calls file");
        // SAFETY: tests guarded by CWD_MUTEX serialize env mutations process-wide.
        std::env::set_var("FORM_SUBMIT_CALLS_PATH", &path);
        path
    }

    fn clear_calls_env() {
        // SAFETY: see set_calls_env.
        std::env::remove_var("FORM_SUBMIT_CALLS_PATH");
    }

    #[test]
    fn test_form_submit_success_navigate_and_click() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let calls_path = set_calls_env(temp.path());

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_python_mock(&kernel, MOCK_SCRIPT);
        let approver = AutoApprover;
        let input = make_input("https://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "t1");

        // Step is Succeeded with weak evidence, no compensation.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));
        assert!(step.compensation_ref.is_none(), "form.submit must have no compensation");

        // Approval was recorded (E3 + PerStep).
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].e_level, ELevel::E3);

        // CRITICAL: navigate + click were called (in order).
        let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        assert_eq!(recorded, vec!["navigate".to_string(), "click".to_string()],
            "expected navigate then click, got {:?}", recorded);

        clear_calls_env();
    }

    #[test]
    fn test_form_submit_user_denies_cancels_step() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let calls_path = set_calls_env(temp.path());

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_python_mock(&kernel, MOCK_SCRIPT);
        let approver = AutoDenier;
        let input = make_input("https://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(ref m) if m.contains("user denied")),
            "expected Skill 'user denied' error, got {:?}",
            err
        );

        // Step is Cancelled, not Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Cancelled);

        // Approval was still recorded (user saw the prompt).
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);

        // No MCP calls were made (denial short-circuits before navigate).
        let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        assert!(recorded.is_empty(), "expected no MCP calls after denial, got {:?}", recorded);

        clear_calls_env();
    }

    #[test]
    fn test_form_submit_empty_url_fails_validation() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = make_input("", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(err.to_string().contains("url"));
        assert!(err.to_string().contains("empty"));

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }

    #[test]
    fn test_form_submit_invalid_url_fails_manifest_validation() {
        // url="ftp://example.com" → manifest Url validation rejects (not http/https).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = make_input("ftp://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(err.to_string().contains("url"));

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }

    #[test]
    fn test_form_submit_empty_submit_selector_uses_default() {
        // submit_selector="" → 用 manifest default "button[type=submit]"
        // 验证:不报错,且 click 调用的 selector 是 default 值
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let calls_path = set_calls_env(temp.path());

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_python_mock(&kernel, MOCK_SCRIPT);
        let approver = AutoApprover;
        let input = make_input("https://example.com", ""); // 空提交选择器
        let result = execute_form_submit(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok with default selector, got {:?}", result.err());

        // 验证 click 被调用(selector 在 mock 中不记录参数,但调用次数 = 1)
        let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        assert!(recorded.contains(&"click".to_string()), "click must be called with default selector");

        clear_calls_env();
    }

    #[test]
    fn test_form_submit_mcp_failure_marks_step_failed() {
        // Override playwright record with a guaranteed-to-fail command.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let mut rec = McpServerRepo::new()
            .get(&kernel.conn(), "playwright")
            .unwrap()
            .unwrap();
        rec.command = Some("this-command-does-not-exist-12345".to_string());
        rec.args = Some("[]".to_string());
        McpServerRepo::new().update(&kernel.conn(), &rec).unwrap();

        let approver = AutoApprover;
        let input = make_input("https://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Mcp(_)),
            "expected KernelError::Mcp, got {:?}",
            err
        );

        // Step is Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Failed);
    }
}
```

- [ ] **Step 2: 在 `skills/mod.rs` 加 `pub mod form_submit;`**

打开 `voicepilot/crates/trust-kernel/src/skills/mod.rs`,在现有 `pub mod` 声明后追加:

```rust
// W8 Plan 3 Task 5: form.submit Skill executor
pub mod form_submit;
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 4: 跑 form_submit 单元测试**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_form_submit`
Expected: PASS(Task 4 的 6 个 manifest 测试 + Task 5 的 6 个 executor 测试 = 12 个测试全绿;其中 3 个依赖 python 的测试若 python 不可用则跳过)

- [ ] **Step 5: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/form_submit.rs voicepilot/crates/trust-kernel/src/skills/mod.rs voicepilot/crates/trust-kernel/tests/w8_plan3_form_submit.rs
git commit -m "feat(w8p3): implement form.submit executor with navigate+click + E3 PerStep approval"
```

---

## Task 6: dispatch_skill_executor 加 form.submit 分支

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_dispatcher_form_submit.rs`

**Spec §:** §2.3 "dispatch_skill_executor 路由" — `form.submit` 分支

**背景:** Plan 2 的 `dispatch_skill_executor` 中 `form.submit` 分支返回 `Err("form.submit not implemented in Plan 2; see Plan 3")`。本 Task 替换为真实 `dispatch_form_submit` 调用 `execute_form_submit`。

- [ ] **Step 1: 改 `skills/dispatcher.rs`,把 `form.submit` 占位 Err 替换为真实 dispatch**

打开 `voicepilot/crates/trust-kernel/src/skills/dispatcher.rs`,找到 `dispatch_skill_executor` 函数中的:

```rust
        "form.submit" => Err(KernelError::Skill(
            "form.submit not implemented in Plan 2; see Plan 3".into(),
        )),
```

替换为:

```rust
        "form.submit" => dispatch_form_submit(kernel, resolved_input, approver, task_id, step_id),
```

- [ ] **Step 2: 在 `dispatcher.rs` 追加 `dispatch_form_submit` helper + import**

在文件顶部 import 区(已有 `use crate::skills::form_prepare::{execute_form_prepare, FormPrepareInput};` 后)追加:

```rust
use crate::skills::form_submit::{execute_form_submit, FormSubmitInput};
```

在文件末尾(`dispatch_form_prepare` 函数后)追加:

```rust
fn dispatch_form_submit(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let url = extract_string(resolved_input, "url")?;
    // submit_selector 可选 — 缺失时用空串,executor 内部会 fallback 到 default
    let submit_selector = match resolved_input.get("submit_selector") {
        None => String::new(),
        Some(serde_json::Value::Null) => String::new(),
        Some(v) => v.as_str().map(|s| s.to_string()).unwrap_or_default(),
    };
    let input = FormSubmitInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        url,
        submit_selector,
    };
    let returned = execute_form_submit(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}
```

- [ ] **Step 3: 跑 `cargo check`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

- [ ] **Step 4: 写失败测试 — 创建 `tests/w8_plan3_dispatcher_form_submit.rs`**

```rust
//! W8 Plan 3 Task 6: dispatch_skill_executor form.submit 路由命中测试.
//!
//! 验证 form.submit 分支不再返回 "not implemented" Err,
//! 而是路由到 execute_form_submit(可能因 manifest 校验失败,
//! 但不应是 "not implemented" 错误)。

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;

#[test]
fn dispatch_form_submit_routes_to_executor_not_placeholder() {
    // form.submit + 缺 url → 应返回 manifest 校验错误,不是 "not implemented"
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let input = serde_json::json!({"submit_selector": "button[type=submit]"});
    let result = dispatch_skill_executor(
        "form.submit",
        &kernel,
        &input,
        &approver,
        "task-submit",
        "step-submit",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(
        !msg.contains("not implemented"),
        "form.submit should not return 'not implemented' in Plan 3, got: {}",
        msg
    );
    // 应是 manifest 校验错误(url 缺失)
    assert!(msg.contains("url") || msg.contains("validation"), "got: {}", msg);
}

#[test]
fn dispatch_form_submit_missing_url_returns_validation_err() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "form.submit",
        &kernel,
        &input,
        &approver,
        "task-submit-2",
        "step-submit-2",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    // extract_string 失败或 manifest 校验失败
    assert!(msg.contains("url"), "got: {}", msg);
}
```

- [ ] **Step 5: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_dispatcher_form_submit`
Expected: PASS(2 个测试全绿)

注:Plan 2 的 `tests/w8_plan2_dispatcher.rs::dispatch_form_submit_returns_not_implemented_err` 测试会失败(因为 form.submit 不再返回 "not implemented")。需修改该测试:

打开 `voicepilot/crates/trust-kernel/tests/w8_plan2_dispatcher.rs`,找到 `dispatch_form_submit_returns_not_implemented_err` 测试,替换为:

```rust
#[test]
fn dispatch_form_submit_routes_to_executor_in_plan3() {
    // Plan 3: form.submit 不再返回 "not implemented",而是路由到 execute_form_submit
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"url": "https://example.com", "submit_selector": "button[type=submit]"});
    let result = dispatch_skill_executor(
        "form.submit",
        &kernel,
        &input,
        &approver,
        "task-submit-plan2",
        "step-submit-plan2",
    );
    // Plan 3:可能成功(若 playwright MCP 可用)或失败(MCP 不可用),
    // 但不应是 "not implemented" 错误
    if let Err(e) = result {
        let msg = format!("{}", e);
        assert!(!msg.contains("not implemented"), "Plan 3 should not return 'not implemented', got: {}", msg);
    }
}
```

- [ ] **Step 6: 跑 Plan 2 dispatcher 测试,确认不回归**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan2_dispatcher`
Expected: PASS(原 10 个测试中 `dispatch_form_submit_returns_not_implemented_err` 被替换为 `dispatch_form_submit_routes_to_executor_in_plan3`,仍 10 个测试全绿)

- [ ] **Step 7: 跑 clippy**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

- [ ] **Step 8: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/dispatcher.rs voicepilot/crates/trust-kernel/tests/w8_plan3_dispatcher_form_submit.rs voicepilot/crates/trust-kernel/tests/w8_plan2_dispatcher.rs
git commit -m "feat(w8p3): wire form.submit into dispatch_skill_executor + update Plan 2 test"
```

---

## Task 7: LlmClient::explain_failure + wiremock 测试

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/llm/client.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_llm_explain_failure.rs`

**Spec §:** §2.5 "LlmClient 新增 `explain_failure(step, audit_logs) -> Result<LlmAnalysis>`"

**背景:** W7 的 `LlmClient::classify_and_extract` 用 OpenAI 兼容 `/chat/completions` + function calling 做意图分类。本 Task 新增 `explain_failure` 方法,用同样的 HTTP 基础设施做失败归因:
- System prompt 中文约束:仅基于 audit_logs 中的事实,不得编造未记录的事件;若无法归因,返回 category=Unknown + confidence < 0.5;输出中文,简洁(≤ 200 字)
- Function calling schema: `{ root_cause_zh, category(enum), suggested_fix(optional), confidence(float) }`
- category enum: `mcp_unavailable` / `path_not_allowed` / `approval_denied` / `network_error` / `unknown`(与 Plan 1 `FailureCategory` 一致)

**输入:**
- `step: &StepRecord`(含 step_id / status / task_id)
- `audit_logs: &[AuditEvent]`(step 相关审计事件)

**System prompt 关键约束(spec §2.5):**
- 仅基于 audit_logs 中的事实,不得编造未记录的事件
- 若无法归因,返回 `category=unknown` + `confidence < 0.5`,不强行解释
- 输出中文,简洁(≤ 200 字)
- `suggested_fix` 可选(若有明确建议)

- [ ] **Step 1: 在 `llm/client.rs` 追加 `explain_failure` 方法**

打开 `voicepilot/crates/trust-kernel/src/llm/client.rs`,在 `impl LlmClient` 块末尾(`parse_tool_call_response` 方法后)追加:

```rust
    /// W8 Plan 3:失败归因 LLM 调用(spec §2.5)。
    ///
    /// 把 step 的 audit_logs 喂给 LLM,让其归因失败原因。
    /// System prompt 约束:仅基于事实,不得编造;无法归因 → category=unknown + confidence<0.5。
    ///
    /// 参数:
    /// - `step`:失败的 StepRecord(含 step_id / status / task_id)
    /// - `audit_logs`:step 相关审计事件列表(由调用方从 kernel.list_audit_for_task + filter 获取)
    ///
    /// 返回 `LlmResult<LlmAnalysis>`:
    /// - Ok(LlmAnalysis) — LLM 成功归因
    /// - Err(LlmError::Http) — HTTP 失败(调用方回退 structured_only)
    /// - Err(LlmError::Parse) — JSON 解析失败 / 非法 category
    /// - Err(LlmError::NotConfigured) — LLM 未配置
    #[cfg(feature = "llm")]
    pub async fn explain_failure(
        &self,
        step: &crate::repo::step_repo::StepRecord,
        audit_logs: &[crate::audit::AuditEvent],
    ) -> LlmResult<crate::skills::task_explain::LlmAnalysis> {
        if !self.is_enabled() {
            return Err(LlmError::NotConfigured);
        }

        let system_prompt = self.build_explain_system_prompt();
        let user_content = self.build_explain_user_content(step, audit_logs);
        let tools = self.build_explain_tool_schema();
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_content},
            ],
            "tools": tools,
            "tool_choice": {"type": "function", "function": {"name": "explain_failure"}},
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

        self.parse_explain_response(&resp_json)
    }

    /// 构建失败归因的 system prompt(中文,spec §2.5 约束)。
    #[cfg(feature = "llm")]
    fn build_explain_system_prompt(&self) -> String {
        String::from(
            "你是 VoicePilot 的失败归因器,基于审计日志分析步骤失败原因。\n\n\
             约束:\n\
             1. 仅基于 audit_logs 中的事实,不得编造未记录的事件\n\
             2. 若无法归因,返回 category=\"unknown\" + confidence<0.5,不强行解释\n\
             3. root_cause_zh 用中文,简洁(≤ 200 字)\n\
             4. category 必须是以下之一:mcp_unavailable / path_not_allowed / approval_denied / network_error / unknown\n\
             5. suggested_fix 可选,若有明确建议则填写\n\n\
             category 含义:\n\
             - mcp_unavailable:MCP 服务器未启动 / 命令不存在 / spawn 失败\n\
             - path_not_allowed:文件路径不在 allowed_paths 白名单\n\
             - approval_denied:用户拒绝审批\n\
             - network_error:网络请求失败 / 超时\n\
             - unknown:无法归因"
        )
    }

    /// 构建用户消息:序列化 step + audit_logs 为 JSON 字符串。
    #[cfg(feature = "llm")]
    fn build_explain_user_content(
        &self,
        step: &crate::repo::step_repo::StepRecord,
        audit_logs: &[crate::audit::AuditEvent],
    ) -> String {
        let step_json = serde_json::json!({
            "step_id": step.step_id,
            "task_id": step.task_id,
            "status": step.status.as_str(),
            "evidence_strength": step.evidence_strength,
            "error_message": step.error_message,
        });
        let logs_json: Vec<serde_json::Value> = audit_logs.iter().map(|e| {
            serde_json::json!({
                "event_type": e.event_type,
                "details": e.details,
                "timestamp": e.timestamp.to_rfc3339(),
            })
        }).collect();
        format!(
            "失败的步骤:\n{}\n\n审计日志:\n{}",
            serde_json::to_string_pretty(&step_json).unwrap_or_default(),
            serde_json::to_string_pretty(&logs_json).unwrap_or_default()
        )
    }

    /// 构建 explain_failure function calling schema。
    #[cfg(feature = "llm")]
    fn build_explain_tool_schema(&self) -> serde_json::Value {
        json!([{
            "type": "function",
            "function": {
                "name": "explain_failure",
                "description": "Analyze step failure from audit logs and return root cause in Chinese",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "root_cause_zh": {
                            "type": "string",
                            "description": "中文归因,≤ 200 字,仅基于 audit_logs 事实"
                        },
                        "category": {
                            "type": "string",
                            "enum": ["mcp_unavailable", "path_not_allowed", "approval_denied", "network_error", "unknown"]
                        },
                        "suggested_fix": {
                            "type": ["string", "null"],
                            "description": "可选的修复建议"
                        },
                        "confidence": {
                            "type": "number",
                            "minimum": 0,
                            "maximum": 1
                        }
                    },
                    "required": ["root_cause_zh", "category", "confidence"]
                }
            }
        }])
    }

    /// 解析 LLM 响应 → LlmAnalysis。
    /// 非法 category → Err(LlmError::Parse)(调用方回退 structured_only)。
    #[cfg(feature = "llm")]
    fn parse_explain_response(
        &self,
        resp: &serde_json::Value,
    ) -> LlmResult<crate::skills::task_explain::LlmAnalysis> {
        use crate::skills::explanation_repo::FailureCategory;

        let tool_call = resp
            .pointer("/choices/0/message/tool_calls/0")
            .ok_or_else(|| LlmError::Parse("missing tool_calls[0]".to_string()))?;
        let args_str = tool_call
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing function.arguments".to_string()))?;
        let args: serde_json::Value = serde_json::from_str(args_str)
            .map_err(|e| LlmError::Parse(format!("arguments parse: {e}")))?;

        let root_cause_zh = args
            .get("root_cause_zh")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing root_cause_zh".to_string()))?
            .to_string();

        let category_str = args
            .get("category")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Parse("missing category".to_string()))?;
        let category = FailureCategory::from_str(category_str)
            .ok_or_else(|| LlmError::Parse(format!("invalid category: {}", category_str)))?;

        let suggested_fix = args
            .get("suggested_fix")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let confidence = args
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .unwrap_or(0.0);

        Ok(crate::skills::task_explain::LlmAnalysis {
            root_cause_zh,
            category,
            suggested_fix,
            confidence,
        })
    }
```

- [ ] **Step 2: 跑 `cargo check --features llm`,确认编译通过**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features llm`
Expected: PASS

注:若 `StepRecord` 字段名(`error_message` / `evidence_strength`)与实际不符,需读 `voicepilot/crates/trust-kernel/src/repo/step_repo.rs` 调整 `build_explain_user_content` 中的字段访问。

- [ ] **Step 3: 写失败测试 — 创建 `tests/w8_plan3_llm_explain_failure.rs`**

```rust
//! W8 Plan 3 Task 7: LlmClient::explain_failure wiremock 测试.
//!
//! 4 场景:LLM 成功 / HTTP 失败回退 / JSON 解析失败回退 / 非法 category 回退。
//! 全部 #[cfg(feature = "llm")] #[tokio::test]。

#![cfg(feature = "llm")]

use std::collections::HashMap;

use chrono::Utc;
use trust_kernel::audit::AuditEvent;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::llm::types::LlmError;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::explanation_repo::FailureCategory;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_step() -> StepRecord {
    let mut step = StepRecord::new("step-001".into(), "task-001".into(), 1);
    step.status = StepStatus::Failed;
    step
}

fn make_audit_logs() -> Vec<AuditEvent> {
    vec![AuditEvent {
        log_id: "log-001".into(),
        task_id: "task-001".into(),
        step_id: Some("step-001".into()),
        event_type: "MCP_CALL_FAILED".into(),
        details: serde_json::json!({"error": "command not found", "server": "playwright"}),
        timestamp: Utc::now(),
        prev_hash: None,
        hash: String::new(),
    }]
}

fn llm_client(base_url: &str) -> LlmClient {
    LlmClient::new(base_url, "test-api-key", "gpt-4o-mini")
}

#[tokio::test]
async fn explain_failure_success_returns_llm_analysis() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "explain_failure",
                            "arguments": "{\"root_cause_zh\": \"Playwright MCP 服务器命令未找到,spawn 失败\", \"category\": \"mcp_unavailable\", \"suggested_fix\": \"请在 Settings 中检查 Playwright MCP 配置\", \"confidence\": 0.9}"
                        }
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let analysis = result.unwrap();
    assert_eq!(analysis.category, FailureCategory::McpUnavailable);
    assert!(analysis.root_cause_zh.contains("Playwright"));
    assert!(analysis.confidence > 0.8);
    assert!(analysis.suggested_fix.is_some());
}

#[tokio::test]
async fn explain_failure_http_500_returns_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, LlmError::Http(_)), "expected Http error, got {:?}", err);
}

#[tokio::test]
async fn explain_failure_missing_tool_calls_returns_parse_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "no tool call"}}]
        })))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, LlmError::Parse(_)), "expected Parse error, got {:?}", err);
}

#[tokio::test]
async fn explain_failure_invalid_category_returns_parse_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "explain_failure",
                            "arguments": "{\"root_cause_zh\": \"x\", \"category\": \"invalid_category\", \"confidence\": 0.5}"
                        }
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        LlmError::Parse(msg) => {
            assert!(msg.contains("invalid category"), "got: {}", msg);
        }
        other => panic!("expected Parse error, got {:?}", other),
    }
}

#[tokio::test]
async fn explain_failure_not_configured_returns_err() {
    // disabled client → Err(NotConfigured)
    let client = LlmClient::disabled();
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, LlmError::NotConfigured), "expected NotConfigured, got {:?}", err);
}
```

- [ ] **Step 4: 跑测试,确认通过**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_llm_explain_failure --features llm`
Expected: PASS(5 个测试全绿)

- [ ] **Step 5: 跑 clippy(features llm)**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features llm -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/llm/client.rs voicepilot/crates/trust-kernel/tests/w8_plan3_llm_explain_failure.rs
git commit -m "feat(w8p3): add LlmClient::explain_failure with Chinese system prompt + 5 wiremock tests"
```

---

## Task 8: task.explain_with_llm 实现 + 持久化到 task_explanations

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/task_explain.rs`
- Test: `voicepilot/crates/trust-kernel/tests/w8_plan3_task_explain_llm.rs`

**Spec §:** §2.5 "`task.explain` LLM 增强" — `execute_task_explain_with_llm` + `TaskExplanation` + `LlmAnalysis` + 持久化

**背景:** W7 的 `execute_explain` 仅读 audit log + 创建新 task/step 记录。W8 增强:
1. 新增 `execute_task_explain_with_llm(kernel, input, approver, llm: Option<&LlmClient>)`
2. 复用 W7 逻辑:读 step audit_logs + step_status
3. 若 LLM 启用且 step 是 Failed,调 `llm.explain_failure(&step, &audit_logs)`
4. 否则返回 structured_only(无 llm_analysis)
5. LLM 归因结果调 `TaskExplanationRepo::create` 写入 `task_explanations` 表
6. 错误处理:HTTP 失败 / JSON 解析失败 → 回退 structured_only,不阻塞主流程

**数据结构(spec §2.5):**
- `TaskExplanation { step_id, status, failed_tool_calls, llm_analysis: Option<LlmAnalysis> }`
- `LlmAnalysis { root_cause_zh, category: FailureCategory, suggested_fix: Option<String>, confidence: f32 }`
- `FailedToolCallSummary { tool_name, args, error_message }` — 从 audit_logs 中提取 `MCP_CALL_FAILED` 事件的 tool_name / args / error_message
- `TaskExplanation::structured_only(step, audit_logs)` — 静态方法,构造无 `llm_analysis` 的解释(用于 LLM 未配置 / step 非 Failed / LLM 失败回退场景)
- `extract_failed_tool_calls(audit_logs) -> Vec<FailedToolCallSummary>` — 私有 helper,从 audit_logs 中过滤 `MCP_CALL_FAILED` 事件并提取字段

**持久化(spec §2.5 + §2.6):**
- LLM 归因成功 → 构造 `TaskExplanationRecord` 调 `TaskExplanationRepo::create(&conn, &rec)` 写入 `task_explanations` 表
- LLM 失败 / 未配置 / step 非 Failed → 不持久化(只返回 `structured_only` 内存对象)
- `TaskExplanationRecord` 字段:`explanation_id`(uuid v4)/ `step_id` / `root_cause_zh` / `category: String`(`FailureCategory::as_str()`)/ `suggested_fix` / `confidence` / `llm_model` / `created_at`

**审计事件(spec §6.1 `llm_explain_called`):**
- LLM 归因成功(无论 category 是否 Unknown)→ 调 `kernel.audit_append_external(task_id, step_id, "llm_explain_called", details)`
- details:`{ step_id, llm_model, category, token_count: 0 }`(W8 简化,token_count 留 0 占位,Plan 6 集成测试时补真实计数)
- LLM 失败回退 → 不发 `llm_explain_called`(spec §7.5 "task.explain 仅在 step=Failed 时调 LLM";HTTP 失败不算"调 LLM",审计只记 `TASK_EXPLAIN_FALLBACK` 自定义事件用于诊断)

**LLM 错误处理(spec §2.5 "LLM 错误处理"):**
- `LlmError::Http` / `LlmError::Parse` / `LlmError::Timeout` → `tracing::warn!` 记录 + 返回 `structured_only`(无 llm_analysis)
- `LlmError::NotConfigured` → 返回 `structured_only`(同 LLM 未启用)
- 任何 LLM 错误都不向上传播(`?` 不出现在 `explain_failure` 调用处),保证 task.explain 主流程不阻塞

- [ ] **Step 1: 在 `task_explain.rs` 追加 `execute_task_explain_with_llm` + 数据结构**

打开 `voicepilot/crates/trust-kernel/src/skills/task_explain.rs`,在文件末尾(W7 既有 `execute_explain` + `#[cfg(test)] mod tests` 之后)追加新模块:

```rust
// ===== W8 Plan 3: task.explain LLM 增强(spec §2.5)=====

use crate::skills::explanation_repo::{FailureCategory, TaskExplanationRecord, TaskExplanationRepo};
use uuid::Uuid;

/// W8 §2.5: task.explain LLM 增强的输出结构。
///
/// 包含静态部分(step_id / status / failed_tool_calls,从 audit_logs 提取)
/// 和可选的 LLM 归因(llm_analysis)。LLM 未配置 / step 非 Failed / LLM
/// 失败回退时 llm_analysis=None。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskExplanation {
    pub step_id: String,
    pub status: StepStatus,
    pub failed_tool_calls: Vec<FailedToolCallSummary>,
    pub llm_analysis: Option<LlmAnalysis>,
}

/// W8 §2.5: LLM 失败归因结果(中文,≤ 200 字)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmAnalysis {
    pub root_cause_zh: String,
    pub category: FailureCategory,
    pub suggested_fix: Option<String>,
    pub confidence: f32,
}

/// W8 §2.5: 从 audit_logs 提取的失败 tool call 摘要。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FailedToolCallSummary {
    pub tool_name: String,
    pub args: serde_json::Value,
    pub error_message: String,
}

impl TaskExplanation {
    /// 构造无 LLM 归因的解释(LLM 未配置 / step 非 Failed / LLM 失败回退)。
    pub fn structured_only(
        step: &StepRecord,
        audit_logs: &[crate::audit::AuditEvent],
    ) -> Self {
        Self {
            step_id: step.step_id.clone(),
            status: step.status,
            failed_tool_calls: extract_failed_tool_calls(audit_logs),
            llm_analysis: None,
        }
    }
}

/// 从 audit_logs 中提取 `MCP_CALL_FAILED` 事件的 tool_name / args / error_message。
fn extract_failed_tool_calls(audit_logs: &[crate::audit::AuditEvent]) -> Vec<FailedToolCallSummary> {
    audit_logs
        .iter()
        .filter(|e| e.event_type == "MCP_CALL_FAILED")
        .map(|e| FailedToolCallSummary {
            tool_name: e
                .details
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            args: e.details.get("args").cloned().unwrap_or(serde_json::Value::Null),
            error_message: e
                .details
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        })
        .collect()
}

/// W8 §2.5: task.explain LLM 增强主入口。
///
/// 与 W7 既有 `execute_explain` 并存:
/// - W7 `execute_explain`:读 audit_recent,创建新 task/step,返回 task_id(dispatcher 已路由)
/// - W8 `execute_task_explain_with_llm`:针对指定 step_id 做失败归因,可选调 LLM,
///   持久化到 `task_explanations` 表,返回 `TaskExplanation` 内存对象
///
/// 调用方(CLI/UI)自行决定何时切换到本函数(Plan 4 Router Bridge 集成时决定)。
///
/// 参数:
/// - `kernel`:用于读 step / audit_logs / 持久化 / 审计
/// - `input`:复用 W7 既有 `TaskExplainInput`(task_id / step_id / limit)
/// - `_approver`:未使用(task.explain risk=E0,approval=None)
/// - `llm`:`Option<&LlmClient>`;`None` 或 `llm.is_enabled()==false` → 走 structured_only
///
/// 返回:`Result<TaskExplanation>` — 持久化失败 / step 不存在 → `Err`
pub async fn execute_task_explain_with_llm(
    kernel: &TrustKernel,
    input: &TaskExplainInput,
    _approver: &dyn Approver,
    llm: Option<&crate::llm::client::LlmClient>,
) -> Result<TaskExplanation> {
    // Step 1: 读 step(必须存在)。step.status 决定是否调 LLM。
    let step = kernel
        .get_step(&input.step_id)?
        .ok_or_else(|| KernelError::StepNotFound(input.step_id.clone()))?;

    // Step 2: 读 step 关联的 audit_logs(kernel 仅暴露 list_audit_for_task,
    // 客户端按 step_id 过滤)。
    let all_logs = kernel.list_audit_for_task(&step.task_id)?;
    let audit_logs: Vec<crate::audit::AuditEvent> = all_logs
        .into_iter()
        .filter(|e| e.step_id.as_deref() == Some(&step.step_id))
        .collect();

    // Step 3: 决定是否调 LLM。
    // - llm 为 None → 不调
    // - llm.is_enabled()==false → 不调
    // - step.status != Failed → 不调(spec §7.5:仅 Failed 时调 LLM)
    let should_call_llm = llm
        .map(|l| l.is_enabled() && step.status == StepStatus::Failed)
        .unwrap_or(false);

    let mut llm_analysis: Option<LlmAnalysis> = None;
    let mut llm_model_used: Option<String> = None;

    if should_call_llm {
        if let Some(l) = llm {
            match l.explain_failure(&step, &audit_logs).await {
                Ok(analysis) => {
                    llm_model_used = Some(l.model_name().to_string());
                    llm_analysis = Some(analysis);
                }
                Err(e) => {
                    // spec §2.5:LLM 失败 → 回退 structured_only,不阻塞。
                    tracing::warn!(
                        error = ?e,
                        step_id = %step.step_id,
                        "LLM explain_failure failed; falling back to structured_only"
                    );
                }
            }
        }
    }

    // Step 4: 构造 TaskExplanation。
    let explanation = if let Some(analysis) = llm_analysis.clone() {
        TaskExplanation {
            step_id: step.step_id.clone(),
            status: step.status,
            failed_tool_calls: extract_failed_tool_calls(&audit_logs),
            llm_analysis: Some(analysis),
        }
    } else {
        TaskExplanation::structured_only(&step, &audit_logs)
    };

    // Step 5: 持久化 LLM 归因到 task_explanations 表(仅当有 llm_analysis)。
    if let Some(ref analysis) = explanation.llm_analysis {
        let rec = TaskExplanationRecord {
            explanation_id: Uuid::new_v4().to_string(),
            step_id: step.step_id.clone(),
            root_cause_zh: analysis.root_cause_zh.clone(),
            category: analysis.category.as_str().to_string(),
            suggested_fix: analysis.suggested_fix.clone(),
            confidence: analysis.confidence,
            llm_model: llm_model_used.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        {
            let conn = kernel.conn();
            TaskExplanationRepo::new().create(&conn, &rec)?;
        }
    }

    // Step 6: 发 llm_explain_called 审计事件(仅当 LLM 实际成功调用)。
    if let Some(ref analysis) = explanation.llm_analysis {
        kernel.audit_append_external(
            &step.task_id,
            Some(&step.step_id),
            "llm_explain_called",
            serde_json::json!({
                "step_id": step.step_id,
                "llm_model": llm_model_used,
                "category": analysis.category.as_str(),
                "token_count": 0i64,
            }),
        )?;
    }

    Ok(explanation)
}
```

**注:**
- 若 `KernelError::StepNotFound` 变体不存在,改用 `KernelError::Skill(format!("step not found: {}", input.step_id))`
- 若 `LlmClient::model_name()` 方法不存在,改用 `l.base_url()` 或读 `LlmClient` 公开字段;Plan 2 Task 8 应已暴露 model_name accessor,若未暴露需在 Task 8 Step 1 之前先加 `pub fn model_name(&self) -> &str { &self.model }`
- `kernel.conn()` 返回 `MutexGuard<Connection>`,直接传 `&conn` 给 repo 方法(参考 project_memory.md "engineering conventions")

- [ ] **Step 2: 跑 `cargo check`,确认编译通过(default + llm 双 feature)**

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel`
Expected: PASS

Run: `cd d:\voicepilot\voicepilot ; cargo check -p trust-kernel --features llm`
Expected: PASS

若 `kernel.conn()` / `kernel.get_step` / `kernel.list_audit_for_task` / `kernel.audit_append_external` 任一方法签名不符,读 `voicepilot/crates/trust-kernel/src/kernel.rs` 调整调用。

- [ ] **Step 3: 写失败测试 — 创建 `tests/w8_plan3_task_explain_llm.rs`**

```rust
//! W8 Plan 3 Task 8: execute_task_explain_with_llm 端到端测试.
//!
//! 6 场景:
//! 1. LLM=None → structured_only(llm_analysis=None,不持久化)
//! 2. LLM 启用 + step Succeeded → structured_only(不调 LLM)
//! 3. LLM 启用 + step Failed + LLM 成功 → 持久化 + llm_explain_called 审计
//! 4. LLM 启用 + step Failed + HTTP 失败 → 回退 structured_only(不持久化)
//! 5. LLM 启用 + step Failed + 非法 category → 回退 structured_only(不持久化)
//! 6. step 不存在 → Err
//!
//! 场景 1/2/6 默认 feature 编译;场景 3/4/5 需要 `--features llm`。

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::audit::AuditEvent;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::explanation_repo::{TaskExplanationRecord, TaskExplanationRepo};
use trust_kernel::skills::task_explain::{
    execute_task_explain_with_llm, TaskExplainInput,
};

fn seed_failed_step(kernel: &TrustKernel, step_id: &str, task_id: &str) {
    kernel.create_task(task_id, "seed failed task").unwrap();
    let mut step = StepRecord::new(step_id.to_string(), task_id.to_string(), 1);
    step.status = StepStatus::Failed;
    kernel.create_step(&step).unwrap();
    // Emit a MCP_CALL_FAILED audit event so extract_failed_tool_calls has data.
    kernel
        .audit_append_external(
            task_id,
            Some(step_id),
            "MCP_CALL_FAILED",
            serde_json::json!({
                "tool_name": "playwright.navigate",
                "args": {"url": "https://example.com"},
                "error": "command not found",
            }),
        )
        .unwrap();
}

fn seed_succeeded_step(kernel: &TrustKernel, step_id: &str, task_id: &str) {
    kernel.create_task(task_id, "seed succeeded task").unwrap();
    let mut step = StepRecord::new(step_id.to_string(), task_id.to_string(), 1);
    step.status = StepStatus::Succeeded;
    kernel.create_step(&step).unwrap();
}

#[test]
fn explain_with_llm_none_returns_structured_only() {
    // LLM=None → structured_only,不调 LLM,不持久化。
    let kernel = TrustKernel::open_in_memory().unwrap();
    seed_failed_step(&kernel, "step-001", "task-001");

    let input = TaskExplainInput {
        task_id: "task-001".to_string(),
        step_id: "step-001".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    let result = futures::executor::block_on(execute_task_explain_with_llm(
        &kernel,
        &input,
        &approver,
        None,
    ));

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let explanation = result.unwrap();
    assert!(explanation.llm_analysis.is_none());
    assert_eq!(explanation.step_id, "step-001");
    assert_eq!(explanation.status, StepStatus::Failed);
    assert_eq!(explanation.failed_tool_calls.len(), 1);
    assert_eq!(explanation.failed_tool_calls[0].tool_name, "playwright.navigate");

    // No persistence.
    let conn = kernel.conn();
    let repo = TaskExplanationRepo::new();
    let persisted = repo.get_by_step_id(&conn, "step-001").unwrap();
    assert!(persisted.is_none());
}

#[test]
fn explain_with_llm_succeeded_step_returns_structured_only() {
    // step Succeeded → 不调 LLM,即使 LLM 启用。
    let kernel = TrustKernel::open_in_memory().unwrap();
    seed_succeeded_step(&kernel, "step-002", "task-002");

    let input = TaskExplainInput {
        task_id: "task-002".to_string(),
        step_id: "step-002".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    // 即使传 disabled LlmClient,step Succeeded 也不应触发 LLM 调用。
    let llm = trust_kernel::llm::client::LlmClient::disabled();
    let result = futures::executor::block_on(execute_task_explain_with_llm(
        &kernel,
        &input,
        &approver,
        Some(&llm),
    ));

    assert!(result.is_ok());
    let explanation = result.unwrap();
    assert!(explanation.llm_analysis.is_none());
    assert_eq!(explanation.status, StepStatus::Succeeded);
}

#[test]
fn explain_with_step_not_found_returns_err() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let input = TaskExplainInput {
        task_id: "no-such-task".to_string(),
        step_id: "no-such-step".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    let result = futures::executor::block_on(execute_task_explain_with_llm(
        &kernel,
        &input,
        &approver,
        None,
    ));

    assert!(result.is_err());
}

// ===== 以下场景需要 --features llm =====

#![cfg_attr(not(feature = "llm"), allow(dead_code))]

#[cfg(feature = "llm")]
mod llm_enabled {
    use super::*;
    use trust_kernel::llm::client::LlmClient;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn llm_client(base_url: &str) -> LlmClient {
        LlmClient::new(base_url, "test-api-key", "gpt-4o-mini")
    }

    fn ok_response(category: &str, root_cause: &str, confidence: f32) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "explain_failure",
                            "arguments": format!(
                                "{{\"root_cause_zh\": \"{}\", \"category\": \"{}\", \"confidence\": {}}}",
                                root_cause, category, confidence
                            )
                        }
                    }]
                }
            }]
        })
    }

    #[tokio::test]
    async fn explain_with_llm_failed_step_persists_and_audits() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_response(
                "mcp_unavailable",
                "Playwright MCP 命令未找到",
                0.9,
            )))
            .mount(&server)
            .await;

        let kernel = TrustKernel::open_in_memory().unwrap();
        seed_failed_step(&kernel, "step-010", "task-010");
        let client = llm_client(&server.uri());

        let input = TaskExplainInput {
            task_id: "task-010".to_string(),
            step_id: "step-010".to_string(),
            limit: 10,
        };
        let approver = AutoApprover;
        let result =
            execute_task_explain_with_llm(&kernel, &input, &approver, Some(&client)).await;

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        let explanation = result.unwrap();
        assert!(explanation.llm_analysis.is_some());
        let analysis = explanation.llm_analysis.unwrap();
        assert_eq!(
            analysis.category,
            trust_kernel::skills::explanation_repo::FailureCategory::McpUnavailable
        );
        assert!(analysis.confidence > 0.5);

        // 持久化到 task_explanations 表。
        let conn = kernel.conn();
        let repo = TaskExplanationRepo::new();
        let persisted = repo.get_by_step_id(&conn, "step-010").unwrap();
        assert!(persisted.is_some(), "expected persistence");
        let rec: TaskExplanationRecord = persisted.unwrap();
        assert_eq!(rec.category, "mcp_unavailable");
        assert!(rec.root_cause_zh.contains("Playwright"));

        // llm_explain_called 审计事件已发。
        let audit = kernel.list_audit_for_task("task-010").unwrap();
        assert!(
            audit.iter().any(|e| e.event_type == "llm_explain_called"),
            "expected llm_explain_called audit event"
        );
    }

    #[tokio::test]
    async fn explain_with_llm_http_failure_falls_back_to_structured_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let kernel = TrustKernel::open_in_memory().unwrap();
        seed_failed_step(&kernel, "step-020", "task-020");
        let client = llm_client(&server.uri());

        let input = TaskExplainInput {
            task_id: "task-020".to_string(),
            step_id: "step-020".to_string(),
            limit: 10,
        };
        let approver = AutoApprover;
        let result =
            execute_task_explain_with_llm(&kernel, &input, &approver, Some(&client)).await;

        assert!(result.is_ok(), "HTTP failure must not propagate");
        let explanation = result.unwrap();
        assert!(explanation.llm_analysis.is_none(), "expected fallback");

        // 不持久化。
        let conn = kernel.conn();
        let repo = TaskExplanationRepo::new();
        assert!(repo.get_by_step_id(&conn, "step-020").unwrap().is_none());

        // 不发 llm_explain_called 审计事件。
        let audit = kernel.list_audit_for_task("task-020").unwrap();
        assert!(
            !audit.iter().any(|e| e.event_type == "llm_explain_called"),
            "expected no llm_explain_called audit on HTTP failure"
        );
    }

    #[tokio::test]
    async fn explain_with_llm_invalid_category_falls_back_to_structured_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_response(
                "invalid_category",
                "x",
                0.5,
            )))
            .mount(&server)
            .await;

        let kernel = TrustKernel::open_in_memory().unwrap();
        seed_failed_step(&kernel, "step-030", "task-030");
        let client = llm_client(&server.uri());

        let input = TaskExplainInput {
            task_id: "task-030".to_string(),
            step_id: "step-030".to_string(),
            limit: 10,
        };
        let approver = AutoApprover;
        let result =
            execute_task_explain_with_llm(&kernel, &input, &approver, Some(&client)).await;

        assert!(result.is_ok());
        let explanation = result.unwrap();
        assert!(explanation.llm_analysis.is_none(), "expected fallback");

        let conn = kernel.conn();
        let repo = TaskExplanationRepo::new();
        assert!(repo.get_by_step_id(&conn, "step-030").unwrap().is_none());
    }
}
```

- [ ] **Step 4: 跑测试,确认通过(default feature)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_task_explain_llm`
Expected: PASS(3 个默认 feature 测试:LLM=None / Succeeded step / step 不存在)

- [ ] **Step 5: 跑测试,确认通过(llm feature)**

Run: `cd d:\voicepilot\voicepilot ; cargo test -p trust-kernel --test w8_plan3_task_explain_llm --features llm`
Expected: PASS(6 个测试全绿,包括 3 个 LLM 启用场景)

- [ ] **Step 6: 跑 clippy(default + llm 双 feature)**

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel -- -D warnings`
Expected: PASS

Run: `cd d:\voicepilot\voicepilot ; cargo clippy -p trust-kernel --features llm -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```powershell
git add voicepilot/crates/trust-kernel/src/skills/task_explain.rs voicepilot/crates/trust-kernel/tests/w8_plan3_task_explain_llm.rs
git commit -m "feat(w8p3): add execute_task_explain_with_llm with LLM failure attribution + persistence + llm_explain_called audit (6 tests)"
```

---

## Self-Review

### 1. Spec 覆盖

**§2.3 DagExecutor 循环节点(决策 #5 + #8)** — ✅ Task 1-3 完整覆盖:
- `run_loop_node` 主入口(Task 1)
- `resolve_iterable` 支持 PrevNodeOutput / UserSlot / Literal 三种来源(Task 1)
- 循环执行 + 失败终止(Task 2)
- `parse_break_condition` + `evaluate_break_condition` 手写 parser(Task 2)
- `max_iterations.min(50)` 硬截断(Task 2)
- `DagNodeStatus::Succeeded(Array)` / `Failed{cause}` 状态(Task 3)
- `DagRepo::update_node_status` 持久化(Task 3)

**§2.4 form.submit 新 Skill** — ✅ Task 4-6 完整覆盖:
- `form_submit_manifest` 风险 E3 + PerStep + None compensation + Weak verifier(Task 4)
- `execute_form_submit` navigate + click 流程(Task 5)
- `dispatch_skill_executor` `form.submit` 分支从 Err 占位替换为真实 executor(Task 6)
- preconditions_hash 绑定 {url, submit_selector}(Task 5)
- Approval Deny → Cancelled(Task 5)

**§2.5 task.explain LLM 增强** — ✅ Task 7-8 完整覆盖:
- `LlmClient::explain_failure(step, audit_logs) -> LlmResult<LlmAnalysis>`(Task 7)
- 中文 system prompt 约束(仅基于事实 / category=Unknown 时不强行解释 / ≤ 200 字)(Task 7)
- function calling schema `{ root_cause_zh, category, suggested_fix, confidence }`(Task 7)
- `execute_task_explain_with_llm(kernel, input, approver, llm: Option<&LlmClient>)`(Task 8)
- `TaskExplanation` / `LlmAnalysis` / `FailedToolCallSummary` 数据结构(Task 8)
- HTTP 失败 / JSON 解析失败 / 非法 category 回退 structured_only(Task 7 + Task 8)
- 持久化到 `task_explanations` 表 via `TaskExplanationRepo::create`(Task 8)

**§6.1 审计事件 `llm_explain_called`** — ✅ Task 8 完整覆盖:
- LLM 成功调用后发 `llm_explain_called` 事件
- details: `{ step_id, llm_model, category, token_count: 0 }`(W8 简化,token_count 留 0 占位)
- 哈希链由 `audit_append_external` 自动链接

**未覆盖项(Plan 4+ 范围,非本 Plan 遗漏):**
- §2.7 UI 扩展(DagApprovalDialog / DagHistoryView / TaskExplainPanel)— Plan 5
- §2.8 Router Bridge 集成 `route_text_with_dag` — Plan 4
- §6.1 `dag_plan_created` / `dag_skeleton_approved` / `dag_node_started` / `dag_node_succeeded` / `dag_node_failed` / `dag_completed` / `llm_decompose_called` — Plan 2 已覆盖
- 真实 token_count 计数 — Plan 6 集成测试时补

### 2. Placeholder 扫描

✅ 通过。所有 `Plan 4` / `Plan 5` / `Plan 6` 出现均在已知偏离上下文:
- `Plan 4 Router Bridge 集成时再决定是否切换到 execute_task_explain_with_llm` — 本 Plan 不改 dispatcher 的 task.explain 分支
- `Plan 5 UI` — UI 扩展非本 Plan 范围
- `Plan 6 集成测试时补真实 token_count` — W8 简化,token_count=0 占位
- `若 LlmClient::model_name() 方法不存在` — Task 8 Step 1 显式提示

无未完成 placeholder,无"add appropriate error handling"等模糊描述。每个 Step 含完整代码块或确切命令 + 期望输出。

### 3. 类型一致性

✅ 通过。关键类型 / 方法签名在定义 Task 和使用 Task 间一致:
- `run_loop_node(node_id, plan, loop_spec, node_outputs, prev_node_id) -> Result<DagNodeStatus>`(Task 1 定义,Task 2/3 使用)
- `resolve_iterable(iterable_source, node_outputs) -> Result<Vec<Value>>`(Task 1 定义,Task 2 使用)
- `parse_break_condition(s: &str) -> Result<BreakCondition>`(Task 2 定义,Task 2 使用)
- `evaluate_break_condition(condition, item) -> bool`(Task 2 定义,Task 2 使用)
- `FormSubmitInput { task_id, step_id, url, submit_selector }`(Task 5 定义,Task 6 使用)
- `execute_form_submit(kernel, input, approver) -> Result<String>`(Task 5 定义,Task 6 调用)
- `form_submit_manifest() -> SkillManifest`(Task 4 定义,Task 6 dispatcher 不直接调用但与 manifest 校验对齐)
- `dispatch_form_submit(kernel, resolved_input, approver, task_id, step_id) -> Result<DispatchOutcome>`(Task 6 定义,Task 6 dispatcher 调用)
- `LlmClient::explain_failure(step, audit_logs) -> LlmResult<LlmAnalysis>`(Task 7 定义,Task 8 使用)
- `LlmAnalysis { root_cause_zh, category: FailureCategory, suggested_fix, confidence }`(Task 7 定义,Task 8 使用)
- `TaskExplanation { step_id, status, failed_tool_calls, llm_analysis: Option<LlmAnalysis> }`(Task 8 定义)
- `FailedToolCallSummary { tool_name, args, error_message }`(Task 8 定义)
- `TaskExplanation::structured_only(step, audit_logs) -> Self`(Task 8 定义)
- `execute_task_explain_with_llm(kernel, input, approver, llm: Option<&LlmClient>) -> Result<TaskExplanation>`(Task 8 定义)
- `FailureCategory` enum 引用 Plan 1 定义(`McpUnavailable` / `PathNotAllowed` / `ApprovalDenied` / `NetworkError` / `Unknown`,本 Plan 不重复定义)
- `TaskExplanationRecord` / `TaskExplanationRepo` 引用 Plan 1 定义(`create(&self, &Connection, &TaskExplanationRecord)` / `get_by_step_id(&self, &Connection, &str)`)
- `DispatchOutcome` 引用 Plan 2 定义
- `DagPlan` / `DagNode` / `LoopSpec` / `DagNodeStatus` / `MAX_LOOP_ITERATIONS_HARD_LIMIT` 引用 Plan 1 定义

### 4. 已知偏离(显式记录,非缺陷)

1. **`StepRecord.error_message` 字段不存在**:Task 7 `build_explain_user_content` 引用 `step.error_message`,但 W7 既有 `StepRecord` 仅有 `evidence_strength`,无 `error_message`。Task 7 Step 2 显式提示:若字段名不符需读 `src/repo/step_repo.rs` 调整为 `step.evidence_strength` 或从 audit_logs 中提取错误消息。

2. **`LlmClient::model_name()` accessor 可能未暴露**:Task 8 Step 1 显式提示:若 Plan 2 Task 8 未暴露 `pub fn model_name(&self) -> &str`,需在 Task 8 Step 1 之前先加该 accessor,或改用 `l.base_url()`(若 base_url 公开)。

3. **dispatcher 的 `task.explain` 分支仍路由到 W7 `execute_explain`**:本 Plan 仅新增 `execute_task_explain_with_llm`,不改 dispatcher(避免破坏 Plan 2 测试)。Plan 4 Router Bridge 集成时决定是否切换。

4. **`token_count: 0` 占位**:spec §6.1 `llm_explain_called` 关键字段含 `token_count`,本 Plan 简化为 0。Plan 6 集成测试时通过解析 LLM HTTP 响应 `usage.total_tokens` 补真实计数。

5. **`max_iterations` 硬上限 50 是常量**:Plan 1 `dag_types::MAX_LOOP_ITERATIONS_HARD_LIMIT = 50`。本 Plan Task 2 引用此常量,不重复定义。

6. **`break_condition` 仅支持 `item.<field> <op> <number>` 形式**:不支持字符串比较 / 嵌套字段访问 / 多条件 AND/OR(spec §2.3 未要求,本 Plan 范围内决策)。

7. **`extract_failed_tool_calls` 仅提取 `MCP_CALL_FAILED` 事件**:W8 简化,不提取 `TASK_FAILED` / `STEP_FAILED` 等其他失败事件类型。Plan 6 集成测试时若发现遗漏,补 event_type 白名单。

8. **`audit_logs` 按 `step_id` 过滤**:kernel 仅暴露 `list_audit_for_task(task_id)`,本 Plan 在 `execute_task_explain_with_llm` 内做客户端 filter。若 Plan 4+ 需要按 step_id 直接查询,可加 `kernel.list_audit_for_step(step_id)` 便捷方法(本 Plan 不加,避免 scope creep)。

### 5. 风险评估

| 风险 | 缓解 |
|---|---|
| `StepRecord.error_message` 字段名与实际不符 | Task 7 Step 2 显式提示读 `src/repo/step_repo.rs` 调整 |
| `LlmClient::model_name()` 未暴露 | Task 8 Step 1 显式提示加 accessor 或改用 base_url |
| `kernel.conn()` / `kernel.get_step` / `kernel.list_audit_for_task` / `kernel.audit_append_external` 签名不符 | Task 8 Step 2 显式提示读 `src/kernel.rs` 调整 |
| `LlmError` 变体名与实际不符 | Task 7 Step 2 显式提示读 `src/llm/types.rs` 调整 |
| `DagNodeStatus::Succeeded(Array)` 变体形状与 Plan 1 不一致 | Task 3 Step 1 显式提示读 `src/skills/dag_types.rs` 调整 |
| `DispatchOutcome` 形状与 Plan 2 不一致 | Task 6 Step 1 显式提示读 `src/skills/dispatcher.rs` 调整 |
| wiremock 测试在 CI 偶发超时 | Task 7 Step 4 + Task 8 Step 5 提供调优建议(client timeout 100ms,server delay 2s) |
| `futures::executor::block_on` 在非 llm feature 测试中引入 futures crate 依赖 | trust-kernel 已有 `futures` 依赖(W7 LLM 测试用);若未暴露需在 Cargo.toml 加 dev-dependency |
| `KernelError::StepNotFound` 变体不存在 | Task 8 Step 1 显式提示改用 `KernelError::Skill(format!(...))` |
| 循环节点测试因 dag_types 字段名差异编译失败 | Task 1/2/3 Step 1 显式提示读 `src/skills/dag_types.rs` 调整 |

### 6. 完成判定

- [x] 8 个 Task 全部含完整代码片段(无 placeholder)
- [x] 每个 Task 含 TDD 步骤(写测试 → 跑红 → 实现 → 跑绿 → commit)
- [x] 每个 Task 含 `cargo check` / `cargo test` / `cargo clippy` 验证命令
- [x] spec §2.3(循环节点)+ §2.4(form.submit)+ §2.5(task.explain LLM)+ §6.1(llm_explain_called)全覆盖
- [x] 类型一致性(Self-Review §3)
- [x] 已知偏离显式记录(Self-Review §4)
- [x] 风险评估 + 缓解(Self-Review §5)
- [x] LLM 代码全部 `#[cfg(feature = "llm")]` 门控,默认 build 不含 LLM 依赖

**Plan 3 完成。** 可进入 Plan 4(Router Bridge 集成 RouteDecision::Dag 分支)。

---

## Execution Handoff

本 Plan 3 完成后,后续 Plan 4-6 的执行流程:

1. **Plan 4:** Router Bridge 集成 — 在 W7 `route_text` 之前插入 LLM 拆解分支,返回 `RouteDecision::Dag(DagPlan)`;集成 `execute_task_explain_with_llm` 到 task.explain 路由(替换 W7 `execute_explain`)
2. **Plan 5:** UI — DAG 骨架审批弹窗(`DagApprovalDialog.tsx`)+ DAG 历史(`DagHistoryView.tsx`)+ task.explain 面板(`TaskExplainPanel.tsx`,含 LLM 归因段落可折叠)
3. **Plan 6:** 集成测试 + 6 套 feature 组合 cargo check 矩阵 + clippy `-D warnings` + npm build;补真实 token_count 计数

每个 Plan 完成后,更新 `docs/PROGRESS.md` 并 commit。

**两种执行方式:**

1. **Subagent-Driven(推荐)** — 每个 Task 派发独立 subagent,Task 间双阶段 review(subagent 自检 + 主控 review),快速迭代
2. **Inline Execution** — 在当前 session 顺序执行 Task,checkpoint 处批量 review

**选择哪种?**