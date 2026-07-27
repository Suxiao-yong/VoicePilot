# W8: Skill 编排 + DAG 调度器 Design Spec

**日期:** 2026-07-26(Asia/Shanghai)
**对应规格:** V1.1.2 §5.1 Skill Router / §5.4 LLM Planner / §6.2 prepare→approve→commit / §7.2 强补偿 / §8.3 Skills Manager
**前置:** W7 已完成(commit `326e781`,W7 Plan 1-6 共 ~54 commits,全部验收门禁关闭)
**范围:** 引入 LLM 驱动的 DAG 拆解 + 调度器,允许用户语音触发多 Skill 编排(如"打开记事本写 TODO 然后保存到桌面" → DAG [note.capture, files.move]);补 `form.submit` 独立 Skill(Playwright submit 点击);增强 `task.explain` 接 LLM 失败归因

---

## 1. 背景

W7 LLM Planner 仅做单 Skill 路由,不做 Skill 编排。spec §8 列出 3 项延后到 W8 的工作:

1. **Skill 之间不组合** — 如"打开记事本写 TODO 然后保存到桌面"需拆为 `note.capture` + `files.move` 两步
2. **`form.prepare` 不点击 submit** — 留待 W8 用 `playwright.click` 链路补全
3. **`task.explain` 不接 LLM** — 失败原因解释延后

W8 闭合以上 3 项,引入 DagPlanner(LLM 拆解)+ DagExecutor(调度)+ SlotTemplateEngine(占位符解析)三个核心组件,复用 W7 的 LlmClient + SkillRouter + 8 个 Skill executor 基础设施。

**用户决策(2026-07-26)W8 范围确认:**

| # | 维度 | 决策 |
|---|---|---|
| 1 | DAG 来源 | LLM 拆解(语音 → 多步 DAG) |
| 2 | 审批粒度 | DAG 骨架一次审批 + 每步独立审批 |
| 3 | Slot 流水 | LLM 返回 input template(占位符 `${prev.output.xxx}`) |
| 4 | 事务边界 | 仅记录失败,用户手动补偿(不自动 saga) |
| 5 | DAG 复杂度 | 完整 DAG(含循环 + 条件分支;并行 fan-out/fan-in 延后 W9+) |
| 6 | form.submit | 新增独立 Skill |
| 7 | task.explain | 纳入 W8,接 LLM |
| 8 | 循环失败语义 | 终止循环 + DAG=PartiallySucceeded(若有成功节点) |

---

## 2. 范围

W8 共 8 项工作:

| # | 项 | 优先级 |
|---|---|---|
| 1 | SlotTemplateEngine(占位符解析 + 校验) | P0 |
| 2 | LlmClient::decompose_to_dag(LLM 拆解) | P0 |
| 3 | DagExecutor(拓扑排序 + 节点调度 + 循环) | P0 |
| 4 | `form.submit` 新 Skill | P0 |
| 5 | `task.explain` LLM 增强 | P1 |
| 6 | 数据库迁移 003 + DagRepo / TaskExplanationRepo | P0 |
| 7 | UI:DAG 骨架审批弹窗 + DAG 历史 + task.explain 面板 | P0 |
| 8 | 集成测试 + 验收门禁 | P0 |

### 2.1 SlotTemplateEngine(P0)

**新增文件:** `crates/trust-kernel/src/skills/template.rs`

**职责:** 解析 LLM 生成的模板字符串 → 编译为 `TemplateExpr` → 用上游节点 outputs + 用户 Slot + 循环变量渲染为最终值。

**数据结构:**

```rust
pub struct SlotTemplate {
    pub kind: SlotKind,                     // Path / App / Number / TimeRange / Url / Files(list)
    pub template: TemplateExpr,
}

pub enum TemplateExpr {
    Literal(String),                        // "notepad"
    Var(VarRef),                            // ${prev.output.path}
    Concat(Vec<TemplateExpr>),              // "C:/Users/" + ${user.profile} + "/Documents"
    Filter { source: Box<TemplateExpr>, predicate: String },  // ${prev.output.files}[?size > 1MB]
}

pub struct VarRef {
    pub scope: VarScope,
    pub path: String,                       // dotted path,如 "output.path"
}

pub enum VarScope {
    Prev,                                   // 紧邻上游节点
    Step(String),                           // 指定 node_id
    User,                                   // 用户审批阶段填的 Slot
    Iter,                                   // 循环变量 ${item}
}
```

**API:**

```rust
impl SlotTemplateEngine {
    /// 解析模板字符串 → 编译后的 TemplateExpr
    pub fn parse(template_str: &str) -> Result<TemplateExpr>;

    /// 执行模板:用 node_outputs + user_slots + iter_var 渲染最终值
    pub fn resolve(
        expr: &TemplateExpr,
        node_outputs: &HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        iter_var: Option<&str>,
    ) -> Result<serde_json::Value>;

    /// 校验:模板中所有变量引用都能在 DAG 上下文中找到绑定
    pub fn validate(
        expr: &TemplateExpr,
        dag_nodes: &[DagNode],
        user_slot_kinds: &[&str],
    ) -> Result<()>;
}
```

**模板语法:**

| 表达式 | 含义 |
|---|---|
| `"notepad"` | 字面量 |
| `"${prev.output.path}"` | 紧邻上游节点的 output.path |
| `"${n1.output.path}"` | 指定节点 n1 的 output.path |
| `"${user.profile_name}"` | 用户审批阶段填的 Slot |
| `"${item}"` | 循环变量 |
| `"C:/Users/${user.name}/Documents/${item.name}"` | 字面量 + 变量拼接 |
| `"${prev.output.files}[?size > 1048576]"` | Filter 过滤(>1MB 的文件) |

**校验时机(双层防御):**
1. **LLM 返回后立即校验**:`decompose_to_dag` 解析后调 `SlotTemplateEngine::validate`,任何模板引用未声明的 node/slot → 拒绝整个 DAG,回退 W7 单 Skill 路由
2. **执行前再次校验**:`DagExecutor::run_node` 调用 `resolve` 时若变量解析失败 → 节点 Failed,不执行 Skill

### 2.2 LlmClient::decompose_to_dag(P0)

**改动文件:** `crates/trust-kernel/src/llm/client.rs`(W7 已存在)

**新签名:**

```rust
impl LlmClient {
    /// W8 新增 LLM 调用:语音 → 完整 DAG plan。
    /// 复用 W7 的 OpenAI 兼容 /chat/completions + function calling。
    pub async fn decompose_to_dag(
        &self,
        user_text: &str,
        candidate_skills: &[SkillManifest],
        user_slots: &[ExtractedSlot],
    ) -> Result<DagPlan>;
}
```

**Function calling schema:**

```json
{
  "name": "decompose_to_dag",
  "parameters": {
    "type": "object",
    "properties": {
      "nodes": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "node_id": { "type": "string" },
            "skill_id": { "type": "string" },
            "input_template": {
              "type": "object",
              "properties": {
                "kind": { "type": "string" },
                "template": { "type": "string" }
              }
            },
            "risk_ceiling": { "type": "string", "enum": ["E0","E1","E2","E3"] }
          }
        }
      },
      "edges": { "type": "array", "items": { "type": "object", "properties": { "from": {"type":"string"}, "to": {"type":"string"}, "port_binding": {"type":["string","null"]} } } },
      "loop_specs": { "type": "object" },
      "max_total_steps": { "type": "integer", "maximum": 20 }
    }
  }
}
```

**System prompt 关键约束(中文):**
- 你是 VoicePilot 的 DAG 拆解器,从用户语音转写文本中识别要执行的多步 Skill 编排
- 不得引用未在 `candidate_skills` 中的 skill_id
- `input_template.template` 中所有 `${...}` 必须指向合法 scope(Prev/Step/User/Iter)
- 循环必须填 `max_iterations`,默认 10,硬上限 50
- `max_total_steps` 硬上限 20(防 LLM 生成爆炸 DAG)
- 若用户意图只需单 Skill,返回单节点 DAG(不强制多步)

**错误处理(与 W7 一致):**
- HTTP 失败 / JSON 解析失败 → 回退到 W7 单 Skill 关键词路由
- LLM 返回非法 skill_id / 模板语法错误 → 校验失败,回退单 Skill 路由
- 超时(30s)→ 同上
- API key 无效(401)→ 同上,UI 在 Settings 中提示

**Privacy:**
- LLM 拆解是 opt-in(`llm_enabled = false` 默认)
- `privacy_mode = true` 时禁止 LLM 拆解(强制 W7 关键词路由)
- 转写文本 + 候选 Skill id/title/description 发给 LLM,不发用户敏感文件路径

### 2.3 DagExecutor(P0)

**新增文件:** `crates/trust-kernel/src/skills/dag_executor.rs`

**核心数据结构:**

```rust
pub struct DagPlan {
    pub plan_id: String,                    // uuid
    pub user_goal: String,                  // 原始语音转写
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
    pub loop_specs: HashMap<String, LoopSpec>,
    pub max_total_steps: u32,               // 全局上限
}

pub struct DagNode {
    pub node_id: String,                    // "n1" / "n2"
    pub skill_id: String,                   // "note.capture" / "files.move"
    pub input_template: SlotTemplate,
    pub risk_ceiling: ELevel,
}

pub struct DagEdge {
    pub from: String,
    pub to: String,
    pub port_binding: Option<String>,
}

pub struct LoopSpec {
    pub loop_var: String,                   // ${item}
    pub iterable_source: IterableSource,
    pub max_iterations: u32,                // 硬上限 50
    pub break_condition: Option<String>,
}

pub enum IterableSource {
    PrevNodeOutput { node_id: String, port: String },
    UserSlot { slot_kind: String },
    Literal(Vec<String>),
}

pub enum DagStatus {
    Pending,
    Running,
    Succeeded,
    Failed { failed_node: String, cause: String },
    PartiallySucceeded { succeeded: Vec<String>, failed_node: String, cause: String },
    Cancelled,
}

pub enum DagNodeStatus {
    Pending,
    Running,
    Succeeded(serde_json::Value),
    Failed { cause: String },
    Skipped,                                // 条件分支未命中
}
```

**调度算法:**

```rust
impl DagExecutor {
    pub fn run(&mut self, plan: &DagPlan) -> Result<DagResult> {
        // Step 1: 拓扑排序(Kahn 算法),检测 edges 中的隐式环(与 LoopSpec 显式循环节点不同)
        let order = topological_sort(&plan.nodes, &plan.edges)?;

        // Step 2: 全局审批 — DAG 骨架预览 + Allow/Deny(决策 #2)
        let dag_approval = self.approver.approve_dag_skeleton(plan)?;
        if dag_approval == ApprovalDecision::Deny {
            return Ok(DagResult::cancelled());
        }

        // Step 3: 按拓扑序执行节点
        for node_id in &order {
            if let Some(loop_spec) = plan.loop_specs.get(node_id) {
                self.run_loop_node(node_id, plan, loop_spec)?;
            } else {
                self.run_simple_node(node_id, plan)?;
            }

            // 失败处理(决策 #4 + #8)
            if matches!(self.node_status[node_id], DagNodeStatus::Failed { .. }) {
                let succeeded: Vec<_> = self.node_status.iter()
                    .filter(|(_, s)| matches!(s, DagNodeStatus::Succeeded(_)))
                    .map(|(k, _)| k.clone())
                    .collect();
                let status = if succeeded.is_empty() {
                    DagStatus::Failed { failed_node: node_id.clone(), cause: ... }
                } else {
                    DagStatus::PartiallySucceeded { succeeded, failed_node: node_id.clone(), cause: ... }
                };
                return Ok(DagResult { status, node_results: self.node_status.clone() });
            }
        }

        Ok(DagResult::succeeded(self.node_status.clone()))
    }
}
```

**简单节点执行(`run_simple_node`):**

1. `SlotTemplateEngine::resolve(node.input_template, node_outputs, user_slots, None)` → 解析 input
2. 查 Skill manifest,构造 SkillInput
3. 创建 task + step
4. `dispatch_skill_executor(skill_id, kernel, input, approver)` → 复用 W7 executor
5. 根据.ToolResult.status 更新 `node_status` + `node_outputs`

**循环节点执行(`run_loop_node`,决策 #5 + #8):**

1. `resolve_iterable(iterable_source, node_outputs, user_slots)` → Vec<Value>
2. `items.into_iter().take(spec.max_iterations.min(50))` 强制截断
3. 对每个 item:
   - `SlotTemplateEngine::resolve(template, ..., Some(&item))` 绑定 `${item}`
   - 调 `dispatch_skill_executor`
   - 失败 → `iter_failed = Some(...)`,break(决策 #8:终止循环)
   - 成功 → push 到 `iter_outputs`
   - 检查 `break_condition`,命中则 break
4. 循环节点状态:全成功 → Succeeded(Array);有失败 → Failed(让上层走 PartiallySucceeded 分支)

**dispatch_skill_executor 路由:**

```rust
fn dispatch_skill_executor(skill_id: &str, kernel: &TrustKernel, input: &SkillInput, approver: &dyn Approver) -> Result<SkillExecution> {
    match skill_id {
        "files.organize" => FilesOrganizeSkill::new().execute(kernel, input, approver),
        "task.repeat_verified" => execute_task_repeat(kernel, input, approver),
        "task.explain" => execute_task_explain_with_llm(kernel, input, approver, /* llm */),
        "task.compensate" => execute_task_compensate(kernel, input, approver),
        "quick.app_control" => execute_app_control(kernel, input, approver),
        "note.capture" => execute_note_capture(kernel, input, approver),
        "research.save_markdown" => execute_research_save(kernel, input, approver),
        "form.prepare" => execute_form_prepare(kernel, input, approver),
        "form.submit" => execute_form_submit(kernel, input, approver),  // W8 新增
        _ => Err(KernelError::Skill(format!("unknown skill_id: {}", skill_id))),
    }
}
```

**关键设计点:**
- **每步独立 approval**:每个 Skill 仍按 W7 manifest 的 `approval.mode` 执行,与 DAG 骨架审批是两层
- **节点 output schema**:每个 Skill 的 `ToolResult.outputs` 用 `serde_json::Value`(自由 schema),LLM 拆解时已根据 Skill manifest 的 `outputs` 字段决定如何引用
- **循环上限硬约束**:`max_iterations.min(50)` + 全局 `max_total_steps ≤ 20`

### 2.4 `form.submit` 新 Skill(P0)

**新增文件:** `crates/trust-kernel/src/skills/form_submit.rs`

**Manifest:**

```rust
pub fn form_submit_manifest() -> SkillManifest {
    SkillManifest {
        id: "form.submit".into(),
        title: "提交表单".into(),
        description: "通过 Playwright MCP 点击 submit 按钮".into(),
        intent_examples: vec!["提交".into(), "submit".into()],
        keywords: vec!["submit".into(), "提交".into()],
        inputs: HashMap::from([
            ("url".into(), SkillInput { input_type: SkillInputType::Text, required: true, .. }),
            ("submit_selector".into(), SkillInput {
                input_type: SkillInputType::Text,
                required: false,
                default: Some("button[type=submit]".into()),
                .. }),
        ]),
        risk_ceiling: ELevel::E3,            // 提交动作不可逆 → E3
        data_class_ceiling: DLevel::D2,
        egress: Egress::Internet,
        max_steps: 1,
        tools: vec!["mcp.playwright.navigate", "mcp.playwright.click"],
        approval: ApprovalSpec {
            mode: ApprovalMode::PerStep,
            required_for: ApprovalFor::Both,
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationSpec {
            level: CompensationLevel::None,  // 不可逆,无补偿(决策 #4 一致)
            ttl_seconds: 0,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierSpec { strategy: VerifierStrategy::Weak, .. },
        failure_policy: FailurePolicy { max_retries: 0, allow_replan: false, on_fail: OnFail::AskUser },
    }
}
```

**Executor 流程:**

1. validate `url`(http/https)+ `submit_selector`(非空)
2. `validate_input_against_manifest`
3. create task + step
4. 构建 EffectManifest:`"submit form at <url> via selector <submit_selector>"`
5. `record_approval_decision`(E3 → 必须 PerStep)
6. `invoke_mcp_tool(navigate, {url})`
7. `invoke_mcp_tool(click, {selector: submit_selector})`
8. `finalize_step_success(evidence="weak")`

**关键约束:**
- `risk_ceiling = E3`(提交不可逆,与 W7 `form.prepare` 的 E2 区分)
- `compensation.level = None`(不可逆,与决策 #4 "仅记录失败"一致)
- `approval.mode = PerStep`(强制每步审批)
- `verifier.strategy = Weak`(浏览器无文件 evidence)

### 2.5 `task.explain` LLM 增强(P1)

**改动文件:** `crates/trust-kernel/src/skills/task_explain.rs`

W7 的 `task.explain` 仅展示 audit log + 步骤状态。W8 增加 LLM 失败归因:

```rust
pub async fn execute_task_explain_with_llm(
    kernel: &TrustKernel,
    input: &TaskExplainInput,
    approver: &dyn Approver,
    llm: Option<&LlmClient>,
) -> Result<SkillExecution> {
    // 1. 复用 W7 逻辑:读 step audit_logs + step_status
    let step = kernel.get_step(&input.step_id)?;
    let audit_logs = kernel.list_audit_logs_for_step(&input.step_id)?;

    // 2. 若 LLM 启用且 step 是 Failed,调 LLM 归因
    let explanation: TaskExplanation = if let Some(llm) = llm {
        if llm.is_enabled() && step.status == StepStatus::Failed {
            llm.explain_failure(&step, &audit_logs).await?
        } else {
            TaskExplanation::structured_only(&step, &audit_logs)
        }
    } else {
        TaskExplanation::structured_only(&step, &audit_logs)
    };

    Ok(SkillExecution {
        tool_result: ToolResult::success(serde_json::to_value(&explanation)?),
        ..
    })
}

pub struct TaskExplanation {
    pub step_id: String,
    pub status: StepStatus,
    pub failed_tool_calls: Vec<FailedToolCallSummary>,
    pub llm_analysis: Option<LlmAnalysis>,  // W8 新增
}

pub struct LlmAnalysis {
    pub root_cause_zh: String,             // 中文归因,≤ 200 字
    pub category: FailureCategory,         // McpUnavailable / PathNotAllowed / ApprovalDenied / NetworkError / Unknown
    pub suggested_fix: Option<String>,
    pub confidence: f32,
}
```

**System prompt 约束:**
- 仅基于 audit_logs 中的事实,不得编造未记录的事件
- 若无法归因,返回 `category=Unknown` + `confidence < 0.5`,不强行解释
- 输出中文,简洁(≤ 200 字)

**LLM 错误处理:**
- HTTP 失败 → 回退到 `structured_only`(无 llm_analysis)
- JSON 解析失败 → 同上
- 与 W7 LLM fallback 一致,不阻塞主流程

### 2.6 数据库迁移 + Repo(P0)

**新增迁移文件:** `crates/trust-kernel/src/migrations/003_dag_plans.sql`

```sql
CREATE TABLE IF NOT EXISTS dag_plans (
    plan_id TEXT PRIMARY KEY,
    user_goal TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    completed_at TEXT,
    root_task_id TEXT,
    FOREIGN KEY (root_task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
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
```

**新增 Repo:**
- `crates/trust-kernel/src/skills/dag_repo.rs` — `DagRepo::new()` + `&Connection` 方法(CRUD + `list_by_status` + 级联删除)
- `crates/trust-kernel/src/skills/explanation_repo.rs` — `TaskExplanationRepo::new()` + `&Connection`(CRUD + `get_by_step_id`)

遵循 W4 `McpServerRepo` 的 `new()` / `&Connection` 模式(参考 project_memory.md "Repo accessor pattern")。

### 2.7 UI 扩展(P0)

**改动文件:**
- `voicepilot/crates/ui/src/commands.rs`:新增 Tauri 命令 `approve_dag_skeleton_command` / `list_dag_history_command` / `get_dag_plan_command`
- `voicepilot/crates/ui/web/src/components/DagApprovalDialog.tsx`(新增):DAG 骨架审批弹窗,显示节点卡片 + 边连线图 + Allow/Deny/Modify
- `voicepilot/crates/ui/web/src/components/DagHistoryView.tsx`(新增):Trust Center → DAG 历史 tab
- `voicepilot/crates/ui/web/src/components/TaskExplainPanel.tsx`(新增):task.explain 输出含 LLM 归因段落(可折叠)

**UI 安全规则(与 W6 一致):**
- WebView 不能直接访问 filesystem
- UI 不能直接调用 MCP
- `approval_request_id` 单次使用

### 2.8 Router Bridge 集成(P0)

**改动文件:** `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`

W7 的 `route_text` 仅返回单 Skill。W8 增强:在 W7 `route_with_llm` 之前插入"LLM 拆解"分支。

```rust
pub async fn route_text_with_dag(text: &str, kernel: &TrustKernel) -> Result<RouteDecision> {
    let router = build_router(kernel)?;
    let user_slots = slot_parser::parse(text);

    // 1. 关键词优先(W7 §2.2 算法:keyword_score >= 0.5 直接返回 Skill,不调 LLM)
    if let RouteDecision::Skill(_) | RouteDecision::SkillWithSlots(_, _) = router.route(text) {
        return Ok(router.route(text));
    }

    // 2. LLM 拆解(W8 新增):关键词未命中且 LLM 启用时,尝试拆解为多步 DAG
    if let Some(llm) = kernel.llm_client() {
        if llm.is_enabled() && !kernel.privacy_mode() {
            let dag = llm.decompose_to_dag(text, &router.skills(), &user_slots).await?;
            SlotTemplateEngine::validate_dag(&dag)?;  // 双层防御 #1
            // 校验失败 → decompose_to_dag 内部返回 Err,下面 ? 会传播;
            // 调用方(router_bridge)需 catch 错误并回退到 W7 route_with_llm
            return Ok(RouteDecision::Dag(dag));
        }
    }

    // 3. 回退:W7 单 Skill 路由(关键词 + LLM fallback)
    router.route_with_llm(text).await
}

pub enum RouteDecision {
    Skill(Box<SkillManifest>),                                       // W7
    SkillWithSlots(Box<SkillManifest>, Vec<ExtractedSlot>),         // W7
    Dag(DagPlan),                                                    // W8 新增
    Planner,                                                         // W7
}
```

---

## 3. 数据流

### 3.1 DAG 拆解 + 执行流程(W8)

```
[语音转写文本]
      ↓
[SlotParser::parse]  ← regex 优先,提取全局 Slot
      ↓
[SkillRouter::route_with_high_confidence]
      ↓ 命中?
      ↓ 是               ↓ 否
[返回 Skill]        [LlmClient::decompose_to_dag]
                      ↓
                      ↓ LLM HTTP 失败 / 校验失败?
                      ↓ 是               ↓ 否
                      ↓             [SlotTemplateEngine::validate_dag]
                      ↓                      ↓ 校验通过?
                      ↓                      ↓ 是           ↓ 否
                      ↓                  [返回 Dag]      [回退 W7 单 Skill]
                      ↓                       ↓
                      ↓             [UI: DAG 骨架审批弹窗]
                      ↓                       ↓ Allow?
                      ↓                       ↓ 是           ↓ Deny
                      ↓                  [DagExecutor::run]  [Cancelled]
                      ↓                       ↓
                      ↓             [for each node in topo order:
                      ↓                resolve template → dispatch_skill_executor
                      ↓                → record ToolResult + DagNodeStatus
                      ↓                → if Failed: break, DagStatus=Failed/PartiallySucceeded]
                      ↓                       ↓
                      ↓             [DagResult { status, node_results }]
                      ↓
[UI: 结果展示 + DAG 历史 + task.explain 入口]
```

### 3.2 LLM 调用边界

- LLM 仅在拆解阶段调用(`decompose_to_dag`)+ task.explain 阶段调用(`explain_failure`),**不**在 Skill 执行阶段调用
- LLM 拆解结果(DagPlan)+ Slot 模板 → 经 UI 反馈给用户 → 用户 Allow 后才执行 DAG
- LLM 不接触用户文件内容(只接触转写文本 + 候选 Skill manifest 的 id/title/description)
- task.explain LLM 仅接触 step audit_logs + step_status,不接触用户敏感文件路径

---

## 4. 数据库迁移

**新增迁移文件:** `crates/trust-kernel/src/migrations/003_dag_plans.sql`(见 §2.6)

**回滚:** `DROP TABLE dag_plans; DROP TABLE dag_nodes; DROP TABLE task_explanations;`

---

## 5. 错误处理

| 场景 | 处理 |
|---|---|
| LLM 拆解 HTTP 失败 | 回退到 W7 单 Skill 关键词路由 |
| LLM 拆解超时(30s) | 同上 |
| LLM 拆解返回非法 skill_id | 拒绝 DAG,回退单 Skill 路由 |
| LLM 拆解返回非法模板语法 | 同上 |
| LLM 拆解返回 max_total_steps > 20 | 强制截断到 20 或拒绝(实现选拒绝,防 LLM 误判) |
| DAG 骨架审批 Deny | DagStatus=Cancelled,不执行任何节点,审计记录 `dag_skeleton_approved` Deny |
| 节点 Skill::execute 失败 | 节点 Failed,前面已 commit 不回滚(决策 #4) |
| 循环中迭代失败 | 终止循环,循环节点 Failed;DAG 走 PartiallySucceeded(若有成功节点)或 Failed(决策 #8) |
| 节点 input_template 解析失败 | 节点 Failed,cause="template resolution error: ..." |
| `form.submit` 用户 Deny | 节点 Cancelled,与 W7 一致 |
| task.explain LLM HTTP 失败 | 回退到 structured_only,不阻塞 |
| task.explain LLM 返回非 JSON | 同上 |
| 拓扑排序检测到循环依赖 | 拒绝 DAG,回退单 Skill 路由 |
| DagRepo CRUD 失败 | 返回 KernelError::Db,事务回滚 |

---

## 6. 安全约束

| 维度 | 约束 |
|---|---|
| **DAG 节点数硬上限** | `max_total_steps ≤ 20`(防 LLM 生成爆炸 DAG) |
| **循环迭代硬上限** | `max_iterations ≤ 50`,且 `min(spec.max_iterations, 50)` |
| **LLM 拆解校验** | 节点 `skill_id` 必须在 candidate_skills 中;`input_template` 中所有 `${...}` 引用必须能在 DAG 上下文中找到绑定 |
| **风险叠加** | DAG 整体 risk = max(各节点 risk_ceiling);W8 仅记录到 audit_log,不强制拦截(用户审批时自行判断)。Settings 的 `max_risk_ceiling` 字段延后 W9+ |
| **LLM 不可绕过审批** | LLM 仅在拆解阶段调用;执行阶段每个 Skill 仍走 W7 prepare→approve→commit |
| **privacy_mode** | `privacy_mode = true` 时禁止 LLM 拆解,回退到 W7 单 Skill 关键词路由 |
| **form.submit 风险** | E3 不可逆 + PerStep 强制审批 + 无补偿 |
| **DAG 用户审批 UI** | 显示节点列表 + 每节点 risk_ceiling + 输入预览(模板渲染后),用户可 Modify 调整 input_template |
| **审计完整性** | 所有 DAG 事件入 audit_logs,哈希链不断;`dag_plans.plan_json` 不可篡改(immutable) |
| **task.explain LLM 输入** | 仅传 step audit_logs + step_status,不传用户敏感文件路径 |
| **task.explain LLM 幻觉防护** | System prompt 强制"仅基于事实";`category=Unknown` 时不输出 root_cause_zh |

### 6.1 审计事件类型(W8 新增 `audit_logs.event_type`)

| event_type | 触发时机 | 关键字段 |
|---|---|---|
| `dag_plan_created` | LLM 拆解完成 | `plan_id`, `node_count`, `max_total_steps` |
| `dag_skeleton_approved` | 用户审批 DAG 骨架 Allow/Deny | `plan_id`, `approver`, `decision` |
| `dag_node_started` | 节点开始执行 | `plan_id`, `node_id`, `skill_id` |
| `dag_node_succeeded` | 节点成功 | `plan_id`, `node_id`, `evidence_strength` |
| `dag_node_failed` | 节点失败 | `plan_id`, `node_id`, `cause` |
| `dag_completed` | DAG 终止 | `plan_id`, `final_status`, `succeeded_count`, `failed_node?` |
| `llm_decompose_called` | LLM 拆解调用 | `plan_id`, `llm_model`, `latency_ms`, `token_count` |
| `llm_explain_called` | task.explain 调 LLM | `step_id`, `llm_model`, `category`, `token_count` |

所有事件均参与 W1 的**哈希链**(`audit_append` 自动链接 `prev_hash`)。

---

## 7. 验收门禁

### 7.1 编译门禁
- 6 套 feature 组合 `cargo check` 全 PASS(default / +llm / +tauri / +voice,tauri / +voice,tauri,llm / +voice,tauri,llm,uia)
- `cargo clippy --workspace --no-default-features -- -D warnings` 0 warnings
- `npm.cmd run build` PASS

### 7.2 测试门禁
- W7 现有测试全 PASS(无回归)
- W8 新增 ≥ 30 个测试(8 单元 + 7 E2E + 3 UI + 内部)
- 总测试数 ≥ 286(default) / +58(tauri) / +78(voice)

### 7.3 功能门禁
- LLM 启用 + 语音"打开记事本写 TODO 然后保存到桌面" → DAG [note.capture, files.move] 成功执行
- LLM 启用 + 语音"打开网页填表单然后提交" → DAG [form.prepare, form.submit] 成功执行
- DAG 骨架审批 Deny → 0 节点执行 + 审计完整
- DAG 中某步 Failed → DAG=PartiallySucceeded + 前序已 commit 步骤无回滚
- task.explain 调用 LLM → 输出含 root_cause_zh + category
- 循环节点 break_condition 触发 → 循环提前终止

### 7.4 安全门禁
- `privacy_mode=true` → LLM 拆解不被调用(回退 W7 关键词路由)
- LLM 拆解返回非法 skill_id / 模板 → DAG 被拒绝,回退单 Skill
- DAG 节点数 > 20 → 拒绝执行
- 循环 max_iterations > 50 → 强制截断到 50
- `form.submit` 必须 PerStep 审批(E3)
- LLM 调用审计日志完整(`dag_plan_created` + `llm_decompose_called` + `dag_node_*` + `dag_completed` 全链路)
- 哈希链不断

### 7.5 LLM 成本门禁(W8 新增)
- 单次 DAG 拆解 ≤ 1 次 LLM 调用(不重复调用)
- task.explain 仅在 step=Failed 时调 LLM(Succeeded 不调)
- LLM 调用 token 计数记录到 audit_log(供后续计费分析)

---

## 8. 已知偏离 / 延后项

> **用户决策(2026-07-26)项目永久约束:** Windows-only + 云端 LLM only。下列"延后 W9+"措辞中,涉及"本地 LLM"和"macOS/Linux"的项均改为"永久放弃";其余项保留为延后。

- **本地 LLM 路径永久放弃:** W8 仍选云端 OpenAI 兼容 API,本地 LLM(ollama / llama.cpp)永久不实现(用户决策 2026-07-26:只用云端 LLM)。
- **DAG 自动 Saga 补偿延后 W9+:** 决策 #4 选"仅记录失败,用户手动补偿",自动反序补偿(CompensationRecord.auto_reverse 链)延后 W9+。W8 的 `task.compensate` Skill 仍可手动调用。
- **DAG 模板 filter 表达式延后 W9+:** `${prev.output.files}[?size > 1MB]` 的 filter 语法 W8 仅支持 `[?size > N]` 简单形式,完整 JSONPath filter(如 `[?@.type == 'image']`)延后 W9+。
- **DAG 模板Modify UI 延后 W9+:** W8 DAG 骨架审批弹窗仅支持 Allow/Deny,Modify(用户调整 input_template)延后 W9+(需可视化模板编辑器)。
- **DAG 节点并行执行延后 W9+:** W8 仅支持串行拓扑序执行(循环节点内串行迭代),并行 fan-out/fan-in 延后 W9+(需并发审批队列)。
- **task.explain 多轮对话延后 W9+:** W8 仅单次 LLM 归因,不支持用户追问"为什么"的多轮对话。
- **LLM 调用计费 / 速率限制延后 W9+:** W8 仅记录 token 计数,不实现本地速率限制(用户在 LLM provider 侧管理)。
- **macOS / Linux UIA 永久放弃:** 项目永久 Windows-only(用户决策 2026-07-26)。
- **D3/E3 红色高亮:** 仍延后(自 W6b-3a 起未实现),W8 不在范围。

---

## 9. 实现顺序建议(供 writing-plans 参考)

1. **Task 1-2:** SlotTemplateEngine + 单元测试(基础设施,其他 Task 依赖)
2. **Task 3:** 数据库迁移 003 + DagRepo + TaskExplanationRepo
3. **Task 4:** LlmClient::decompose_to_dag + 校验
4. **Task 5:** DagExecutor(简单节点)+ dispatch_skill_executor 路由
5. **Task 6:** DagExecutor(循环节点)
6. **Task 7:** `form.submit` 新 Skill
7. **Task 8:** `task.explain` LLM 增强
8. **Task 9:** Router Bridge 集成(RouteDecision::Dag 分支)
9. **Task 10:** UI:DAG 骨架审批弹窗
10. **Task 11:** UI:DAG 历史 + task.explain 面板
11. **Task 12:** 集成测试 + 验收门禁复跑

---

## 10. 参考

- V1.1.2 规格相关章节:§5.1 / §5.4 / §6.2 / §7.2 / §8.3
- W7 spec:`docs/superpowers/specs/2026-07-25-w7-llm-planner-skills-design.md`(§8 延后项 → W8)
- W7 LlmClient:`crates/trust-kernel/src/llm/client.rs`
- W7 SkillRouter:`crates/trust-kernel/src/skills/router.rs`
- W7 8 个 Skill executors:`crates/trust-kernel/src/skills/*.rs`
- W3b `FilesOrganizeSkill` 编排器:`crates/trust-kernel/src/skills/executor.rs`
- OpenAI function calling 参考:https://platform.openai.com/docs/guides/function-calling
