# W10 — V1 发布门禁闭合 设计文档

> **创建日期:** 2026-08-01 (Asia/Shanghai)
> **作者:** TRAE Agent
> **状态:** Draft(待用户审核)
> **前置:** W9 Plan 7 已完成(commit `e467038`),spec §2.7-§2.11 + §5 Fitness Functions + §6.4 审计扩展全部闭合
> **范围:** spec §9.4 V1 验收门禁 10 项硬指标中的 5 项代码层指标(⑥⑦⑧⑨⑩)+ 1 项集成验收

---

## 一、背景与动机

### 1.1 W9 终态

W9 Plan 7(2026-08-01)闭合 spec §5 Fitness Functions,新增 19 个测试,7 套 feature 组合 cargo check 全 PASS,clippy `-D warnings` 0 警告(default + 全特性),npm build PASS,非门控测试 506 ≥ 286,default cargo test 505 passed 0 failed。Stronghold 加密快照 + Taint Tracking + DAG Modify + IterableSource::UserSlot + 审计事件扩展 + PostCommitCompensation 全链路闭合。

但 W9 仅闭合**代码层基础设施**,未触及 spec §9.4 V1 验收门禁的 10 项发布前硬指标。

### 1.2 V1 验收门禁现状(2026-08-01 调研)

| # | spec §9.4 验收项 | 现状 | 评级 |
|---|---|---|---|
| ① | 100 功能任务:单步 ≥ 95%、多步 ≥ 80% | 无 evals/ 目录,无 inspect eval 配置 | 未开始 |
| ② | 50 攻击样本:拦截 ≥ 95% | 无 promptfoo redteam 配置 | 未开始 |
| ③ | 20 TOCTOU 场景:0 成功 | 无 toctou 场景测试 | 未开始 |
| ④ | 15 恶意 Server 场景:0 绕过 | 无 malicious_server 场景测试 | 未开始 |
| ⑤ | 20 数据安全场景:0 未确认外发 | 无 data_security 场景测试 | 未开始 |
| ⑥ | Strong Verifier 覆盖率 ≥ 80% | 仅 1/8 Skill(files.organize)真实调用 verify 函数 | 部分就绪 |
| ⑦ | strong Compensation 成功率 ≥ 95% | 仅 1/6 可逆 Skill(files.organize)有 reverse 函数 | 部分就绪 |
| ⑧ | P95 首字延迟 ≤ 500ms | 无任何延迟测量代码,无 benchmark | 未开始 |
| ⑨ | Kill Switch 1s 内进入 CANCELLING | AtomicBool 即时,但无 CANCELLING 态、无 SLA 断言 | 部分就绪 |
| ⑩ | 审计日志覆盖率 100% | 哈希链测试通过,无覆盖率度量 | 部分就绪 |

### 1.3 W10 范围决策(用户 2026-08-01)

W10 聚焦**代码层硬指标优先**(⑥⑦⑧⑨⑩),不做外部评测(①-⑤)与 §10.4 工程规范(cargo-deny / CI / API docs / NSIS 打包)— 留 W11+。

| 决策项 | 选择 |
|---|---|
| 范围 | 代码层硬指标优先(⑥⑦⑧⑨⑩) |
| Plan 划分 | 按验收项横向划分(6 个 Plan) |
| Verifier 分母 | 8 个 Skill 全部(form.submit 验证提交成功、task.explain 验证记录写入) |
| Compensation 分母 | 仅 6 个可逆 Skill(form.submit / task.explain 显式 `compensation_level=none`) |
| P95 测量点 | ASR 首个 partial transcript(spec VP-FR-001 "≤ 500ms 显示首字") |
| Kill Switch 状态机 | Task + DAG 都加 `Cancelling` 中间态 |
| 审计覆盖率度量 | 事件类型枚举 + 集成测试触发 |

---

## 二、总体架构

```
W10 V1 发布门禁闭合
├── Plan 1: Strong Verifier 覆盖率(⑥ ≥ 80%)        — 8 个 Skill 真实 verify 函数 + manifest 升级
├── Plan 2: Strong Compensation 成功率(⑦ ≥ 95%)    — 6 个可逆 Skill reverse 函数 + reverse 路由扩展
├── Plan 3: P95 首字延迟基准(⑧ ≤ 500ms)            — 延迟埋点 + voice_latency_samples 表 + benchmark
├── Plan 4: Kill Switch CANCELLING 状态机(⑨ ≤ 1s)  — TaskState + DagStatus 加 Cancelling + 1s SLA 断言
├── Plan 5: 审计日志覆盖率(⑩ = 100%)                — AUDIT_EVENT_TYPE_REGISTRY + AuditCoverageChecker
└── Plan 6: 集成验收 + Fitness Functions 闭合       — w10_default_boundary_smoke + 7 套 feature 矩阵
```

每个 Plan 一个 commit,严格 TDD(红 → 绿 → 重构),clippy `-D warnings` 0 警告,6 套 feature 组合 cargo check 全 PASS。

---

## 三、Plan 1 — Strong Verifier 覆盖率(8/8 = 100% ≥ 80%)

### 3.1 现状

| Skill | manifest VerifierConfig | executor 实际 evidence_strength | 真实 verify 函数 |
|---|---|---|---|
| files.organize | strong(manifest.rs:213) | 真实 verify_result.evidence_strength(executor.rs:252) | ✅ `FilesystemTool::verify_move`(fs.rs:220-273) |
| note.capture | medium(manifest.rs:586) | "strong" 硬编码(note_capture.rs:236) | ❌ 无 |
| research.save_markdown | strong(manifest.rs:666) | "strong" 硬编码(research_save.rs:214) | ❌ 无 |
| form.prepare | strong(manifest.rs:742) | "weak" 硬编码(form_prepare.rs:210)— 与 manifest 不一致 | ❌ 无 |
| form.submit | weak(manifest.rs:820) | "weak" 硬编码(form_submit.rs:179) | ❌ 无 |
| task.explain | weak(manifest.rs:349) | "weak" 硬编码(task_explain.rs:279) | ❌ 无 |
| task.repeat_verified | medium(manifest.rs:287) | "weak" 硬编码(task_repeat.rs:121)— 与 manifest 不一致 | ❌ 无 |
| task.compensate | strong(manifest.rs:409) | "strong" 硬编码(task_compensate.rs:188) | ❌ 无 |

仅 1/8 = 12.5% 真实覆盖,远低于 80% 阈值。form.prepare 与 task.repeat_verified 的 executor 与 manifest 声明不一致(已知 bug)。

### 3.2 Plan 1 设计

为 7 个未实现的 Skill 添加真实 Verifier 函数,manifest 升级 4 个 Skill 到 `strong`:

| Skill | 新 Verifier 函数 | 验证策略 | manifest 升级 |
|---|---|---|---|
| note.capture | `verify_note_capture` | 重读 note 文件存在 + sha256 + size 匹配 effect_manifest | medium → strong |
| research.save_markdown | `verify_research_save` | 重读 markdown 文件存在 + sha256 + size | 已 strong,无需升 |
| form.prepare | `verify_form_prepare` | Playwright 重查表单字段值匹配 effect_manifest.fields | 已 strong,无需升 |
| form.submit | `verify_form_submit` | Playwright 查页面 URL 变更 或 success 元素存在 | weak → strong |
| task.explain | `verify_task_explain` | 查 task_explanations 表记录存在 + failure_category 非空 | weak → strong |
| task.repeat_verified | `verify_task_repeat` | 同 files.organize,重读目标文件 sha256+size | medium → strong |
| task.compensate | `verify_task_compensate` | 查 compensations 表 status=Reversed + reverse_payload 非空 | 已 strong,无需升 |

### 3.3 实现要点

**新文件:** `crates/trust-kernel/src/skills/verifiers.rs`

```rust
pub struct VerificationContext<'a> {
    pub kernel: &'a TrustKernel,
    pub step_id: i64,
    pub effect_manifest: &'a serde_json::Value,
    pub tool_name: &'a str,
}

pub enum VerificationOutcome {
    Strong { evidence: serde_json::Value },
    Medium { evidence: serde_json::Value },
    Weak { reason: String },
    Failed { reason: String },
}

pub fn verify_note_capture(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
pub fn verify_research_save(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
pub fn verify_form_prepare(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
pub fn verify_form_submit(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
pub fn verify_task_explain(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
pub fn verify_task_repeat(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
pub fn verify_task_compensate(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
```

**修改:** 各 Skill executor 调用真实 verify 函数,移除硬编码字符串。例如 `note_capture.rs:236` 改为:

```rust
let outcome = verifiers::verify_note_capture(&ctx)?;
finalize_step_success(kernel, step_id, outcome.evidence(), EvidenceStrength::Strong).await?;
```

**修改:** `manifest.rs` 升级 4 个 Skill 的 `verifier.strategy` 为 `"strong"`。

### 3.4 测试

- 单元测试:每个 verify 函数 happy path + 失败路径(文件不存在 / sha256 不匹配 / Playwright 元素不存在 / DB 记录不存在)
- 集成测试:`w10_verifier_coverage_smoke.rs` 枚举 8 个 Skill,各自跑一次 happy path,断言 evidence_strength="strong"
- Fitness Function:`w10_default_boundary_smoke.rs::verifier_coverage_all_strong` 强制 8/8 Strong

---

## 四、Plan 2 — Strong Compensation 成功率(6/6 可逆 = 100% ≥ 95%)

### 4.1 现状

| Skill | manifest CompensationLevel | create_post_commit_compensation 调用 | reverse 函数 |
|---|---|---|---|
| files.organize | Strong(manifest.rs:208) | ✅ executor.rs:184, 225 | ✅ `auto_reverse_move`(executor.rs:18-81) |
| note.capture | Strong(manifest.rs:581) | ❌ comp_ref=None | ❌ 无 |
| research.save_markdown | Strong(manifest.rs:661) | ❌ | ❌ 无 |
| form.prepare | Strong(manifest.rs:737) | ❌ | ❌ 无 |
| form.submit | None(manifest.rs:815)— 不可逆,正确 | N/A | N/A |
| task.explain | None(manifest.rs:344)— 只读,正确 | N/A | N/A |
| task.repeat_verified | Strong(manifest.rs:283) | ❌ | ❌ 无 |
| task.compensate | Strong(manifest.rs:405) | 创建新记录,复用 auto_reverse_move | ✅(复用) |

仅 2/6 = 33% 真实覆盖(files.organize + task.compensate 复用)。远低于 95% 阈值。

### 4.2 Plan 2 设计

为 4 个可逆 Skill 实现 reverse 函数;扩展 reverse 路由机制:

| Skill | 新 reverse 函数 | 反向操作 |
|---|---|---|
| note.capture | `reverse_note_capture` | 删除 note 文件 + audit `compensation_reversed` |
| research.save_markdown | `reverse_research_save` | 删除 markdown 文件 |
| form.prepare | `reverse_form_prepare` | Playwright clear 表单字段 / 关闭 tab |
| task.repeat_verified | (复用) | 复用 `auto_reverse_move`(底层即 files.organize) |

form.submit / task.explain 不计入分母,manifest 已显式 `compensation_level=none`,无需修改。

### 4.3 实现要点

**修改:** `compensation/executor.rs` 添加 reverse 函数注册表:

```rust
pub struct ReverseFnRegistry {
    fns: HashMap<String, ReverseFn>,
}

type ReverseFn = fn(&TrustKernel, &serde_json::Value) -> Result<ReverseOutcome>;

impl ReverseFnRegistry {
    pub fn new() -> Self {
        let mut fns = HashMap::new();
        fns.insert("filesystem.reverse_move".to_string(), auto_reverse_move);
        fns.insert("note.reverse_capture".to_string(), reverse_note_capture);
        fns.insert("research.reverse_save".to_string(), reverse_research_save);
        fns.insert("form.reverse_prepare".to_string(), reverse_form_prepare);
        Self { fns }
    }

    pub fn call(&self, name: &str, kernel: &TrustKernel, payload: &serde_json::Value) -> Result<ReverseOutcome> {
        let f = self.fns.get(name).ok_or_else(|| Error::ReverseFnNotFound(name.into()))?;
        f(kernel, payload)
    }
}
```

**新文件:** `crates/trust-kernel/src/skills/reverse_fns.rs` 实现 4 个新 reverse 函数。

**修改:** 各 Skill executor 调用 `create_post_commit_compensation` 注册对应的 `compensate_fn` 名:

```rust
// note_capture.rs
let comp_ref = create_post_commit_compensation(kernel, step_id, "note.reverse_capture", payload).await?;
```

### 4.4 测试

- 单元测试:每个 reverse 函数 happy path + partial failure(文件已删 / Playwright tab 已关 / etc.)
- 集成测试:`w10_compensation_coverage_smoke.rs` 枚举 6 个可逆 Skill,各跑一次 prepare → commit → reverse,断言 `compensations.status=Reversed`
- Fitness Function:`w10_default_boundary_smoke.rs::compensation_coverage_all_strong` 强制 6/6 Strong

---

## 五、Plan 3 — P95 首字延迟基准(ASR partial transcript ≤ 500ms)

### 5.1 现状

无任何 `p95` / `first_token` / `voice_latency` 测量代码,无 `criterion` / `[[bench]]` benchmark 配置。spec §VP-FR-001 "≤ 500ms 显示首字(sherpa-rs SenseVoice)"。W6b-3b plan 明确记录 "无显式延迟测试但架构满足" / "性能测试不在本计划,作为后续基准任务"。

### 5.2 Plan 3 设计

**埋点:**
- `voice_commands.rs` `voice_listen_command` 入口记录 `t0 = Instant::now()`(Push-to-talk 按下)
- `voice/listener.rs` 首个 partial transcript 回调记录 `t1`,计算 `latency_ms = (t1 - t0).as_millis()`
- 写入 SQLite `voice_latency_samples` 表

**存储:** 新增 migration 008:

```sql
-- migrations/008_voice_latency_samples.sql
CREATE TABLE IF NOT EXISTS voice_latency_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at_ms INTEGER NOT NULL,         -- t0 epoch ms
    latency_ms INTEGER NOT NULL,            -- t1 - t0
    model TEXT NOT NULL,                    -- sherpa-rs model name
    privacy_mode INTEGER NOT NULL DEFAULT 0 -- 0=cloud LLM, 1=local only
);
CREATE INDEX IF NOT EXISTS idx_voice_latency_started ON voice_latency_samples(started_at_ms);
```

**统计 API:** 新增 `voice/latency_stats.rs`:

```rust
pub struct LatencyStats {
    pub sample_count: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
    pub max_ms: u64,
}

pub fn compute_stats(kernel: &TrustKernel, since: Option<i64>) -> Result<LatencyStats>;
```

**admin 命令:** CLI `voice latency-stats [--since 24h]` 输出 P50/P95/P99 + 样本数 + 模型名。

**Fitness Function 测试:** `w10_voice_latency_smoke.rs`(voice-gated):

```rust
#[test]
#[ignore] // 需要真实 sherpa-rs 模型 + mock 音频流
fn p95_first_partial_transcript_under_500ms() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let mock_wav = include_bytes!("../tests/fixtures/push_to_talk_sample.wav");
    let model = SherpaModel::sense_voice_offline(); // 已下载

    let mut latencies = Vec::with_capacity(20);
    for _ in 0..20 {
        let t0 = Instant::now();
        let transcript = run_asr_once(&model, mock_wav).unwrap();
        let t1 = Instant::now();
        assert!(!transcript.is_empty());
        latencies.push((t1 - t0).as_millis() as u64);
    }
    latencies.sort();
    let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
    assert!(p95 <= 500, "P95 first partial transcript {}ms > 500ms", p95);
}
```

### 5.3 实现要点

- `voice/listener.rs` 添加 `first_partial_received_at: Option<Instant>` 字段
- 首次 partial transcript 回调时,计算 latency 并调用 `LatencyRecorder::record(...)`
- `LatencyRecorder` 内部持有 `Arc<Mutex<SqliteConnection>>`,异步写入 `voice_latency_samples` 表
- 若 sherpa-rs 模型未下载,测试 `#[ignore]`,提示用户 `voice model download`

### 5.4 测试

- 单元测试:`latency_stats.rs` 计算 P50/P95/P99 正确性(mock 100 样本)
- 集成测试:`w10_voice_latency_smoke.rs::p95_first_partial_transcript_under_500ms`(voice-gated + `#[ignore]`)
- Fitness Function:`w10_default_boundary_smoke.rs::voice_latency_table_exists`(default-gated,只检查 migration 008 创建表)

---

## 六、Plan 4 — Kill Switch CANCELLING 状态机(≤ 1s)

### 6.1 现状

- `state.rs:5-16` TaskState 枚举只有 `Cancelled` 终态,无 `Cancelling` 中间态
- `dag_types.rs:75-87` DagStatus 同样只有 `Cancelled` 终态
- `ui/state.rs:14, 51, 68` `kill_switch: Arc<AtomicBool>` 即时生效(纳秒级)
- `voice/listener.rs` 默认 `chunk_duration = 500ms`,voice loop 在 chunk 边界检查 kill_switch
- 无任何 1s SLA 断言,无 `kill_switch_triggered` 审计事件

### 6.2 Plan 4 设计

**TaskState 扩展:**

```rust
pub enum TaskState {
    Idle, Listening, Planning, AwaitingApproval,
    Executing, Verifying, Compensating,
    Cancelling,    // ← 新增中间态
    Done, Failed, Cancelled,
}

impl TaskState {
    fn allowed_next(&self) -> &[TaskState] {
        use TaskState::*;
        match self {
            Idle => &[Listening, Cancelled],
            Listening => &[Planning, Cancelled],
            Planning => &[AwaitingApproval, Failed, Cancelled],
            AwaitingApproval => &[Executing, Cancelled],
            Executing => &[Verifying, Compensating, Failed, Cancelling],
            Verifying => &[Done, Compensating, Failed, Cancelling],
            Compensating => &[Done, Failed, Cancelling],
            Cancelling => &[Cancelled],              // ← 新转换
            Done => &[],
            Failed => &[],
            Cancelled => &[],
        }
    }
}
```

**DagStatus 扩展:**

```rust
pub enum DagStatus {
    Pending, Running,
    Cancelling,    // ← 新增
    Succeeded, Failed, PartiallySucceeded, Cancelled,
}

impl DagStatus {
    pub fn transition(from, to) -> bool {
        matches!(
            (from, to),
            (Pending, Running)
                | (Running, Succeeded)
                | (Running, Failed)
                | (Running, Cancelling)        // ← 新
                | (Running, PartiallySucceeded { .. })
                | (Cancelling, Cancelled)      // ← 新
        )
    }
}
```

**Kill Switch 触发路径:**

1. `cancel_voice_command` 触发 → `kill_switch.store(true)`
2. 立即 `transition(当前态, Cancelling)` + audit `kill_switch_triggered { timestamp_ms, from_state }`
3. 立即 `transition(当前态, Cancelled)`(若当前态无副作用,如 Idle/Listening/Planning/AwaitingApproval)或等待 voice loop chunk 边界检测 flag(若 Executing/Verifying/Compensating)
4. transition 到 `Cancelled` 时 audit `task_cancelled { timestamp_ms, duration_ms }`
5. SLA 断言:`task_cancelled.timestamp_ms - kill_switch_triggered.timestamp_ms ≤ 1000`

**新审计事件:**
- `kill_switch_triggered` { timestamp_ms, from_state, source }
- `task_cancelling` { task_id, from_state }
- `task_cancelled` { task_id, duration_ms, sla_met: bool }

### 6.3 实现要点

**修改:** `state.rs` / `dag_types.rs` 加 `Cancelling` 变体 + transition 表
**修改:** `ui/voice_commands.rs::cancel_voice_command` 实现 Kill Switch 触发路径
**修改:** `kernel.rs` / `skills/dag_executor.rs` 在循环边界检查 `Cancelling` 状态,提前退出
**新文件:** `tests/kill_switch_sla.rs` SLA 断言测试

### 6.4 测试

- 单元测试:`state_machine.rs` 加 `cancelling_intermediate_state_transitions` 测试 8 种合法 / 非法转换
- 集成测试:`kill_switch_sla.rs`:
  - `kill_switch_enters_cancelling_within_1s`:mock Executing 状态,触发 Kill Switch,断言 ≤ 1s 进入 Cancelling
  - `kill_switch_completes_within_1s`:断言 Cancelling → Cancelled 总时长 ≤ 1s
  - `kill_switch_from_executing_audits_three_events`:断言 3 个 audit 事件按序触发
- Fitness Function:`w10_default_boundary_smoke.rs::kill_switch_sla_met` 强制 SLA 满足

---

## 七、Plan 5 — 审计日志覆盖率(事件类型枚举 + 集成测试触发 = 100%)

### 7.1 现状

- `audit.rs` 无 AUDIT_EVENT_TYPE_REGISTRY,emit_audit 不校验 event_type 合法性
- `w9_default_boundary_smoke.rs::audit_event_types_all_lower_snake_case` 仅强制命名规范,不校验覆盖率
- 现有 30+ 种 event_type 散落在源码各 emit callsite,无中心化枚举

### 7.2 Plan 5 设计

**Registry:** 在 `audit.rs` 添加:

```rust
pub const AUDIT_EVENT_TYPE_REGISTRY: &[&str] = &[
    // W1-W4 基础
    "task_created", "step_created", "step_status_changed",
    "step_prepared", "step_committed", "step_started",
    "step_succeeded", "step_failed", "state_transition",
    "compensation_created", "compensation_status_changed",
    "approval_recorded",
    // W4 MCP
    "mcp_tools_call", "mcp_call_failed",
    // W7 Skills
    "skill_routed", "skill_executor_called", "llm_planner_called",
    // W8 DAG
    "dag_plan_created", "dag_skeleton_approved", "dag_skeleton_denied",
    "dag_node_succeeded", "dag_node_failed", "dag_node_skipped",
    "llm_decompose_called", "dag_skeleton_modified", "dag_modify_limit_exceeded",
    // W9 Stronghold / Taint / Compensation
    "stronghold_vault_created", "stronghold_vault_unlocked",
    "taint_propagated", "taint_blocked",
    "post_commit_compensation_created", "compensation_reversed",
    // W10 Kill Switch(Plan 4 新增)
    "kill_switch_triggered", "task_cancelling", "task_cancelled",
    // W10 Verifier / Compensation(Plan 1/2 新增)
    "verifier_called", "reverse_fn_called",
];

pub fn is_valid_event_type(name: &str) -> bool {
    AUDIT_EVENT_TYPE_REGISTRY.contains(&name)
}
```

**运行时校验:** `emit_audit` 调用时检查 event_type ∈ registry,否则 `tracing::error!("unknown audit event_type: {}", name)` + 返回 `Err(UnknownEventType)`。这是防御性检查,避免新事件未注册。

**AuditCoverageChecker:** 新文件 `audit_coverage.rs`:

```rust
pub struct AuditCoverageChecker<'a> {
    kernel: &'a TrustKernel,
    expected: &'a [&'a str],
}

impl<'a> AuditCoverageChecker<'a> {
    pub fn new(kernel: &'a TrustKernel) -> Self {
        Self { kernel, expected: AUDIT_EVENT_TYPE_REGISTRY }
    }

    pub fn covered(&self) -> Result<Vec<String>> {
        // 查询 audit_logs SELECT DISTINCT event_type
    }

    pub fn uncovered(&self) -> Result<Vec<String>> {
        // expected - covered
    }

    pub fn coverage_ratio(&self) -> Result<f64> {
        // covered.len() / expected.len()
    }
}
```

**admin 命令:** CLI `audit coverage` 输出已覆盖 / 未覆盖 event_type 列表 + 覆盖率百分比。

### 7.3 实现要点

- `audit.rs::emit_audit` 加 `is_valid_event_type` 检查
- 现有所有 emit callsite 不需要修改(event_type 字符串字面量已在 registry 中)
- 新增 `audit_coverage.rs` 实现 AuditCoverageChecker
- 新增 `w10_audit_coverage_smoke.rs` 集成测试:运行所有 Skill 路径,断言 registry 100% 覆盖

### 7.4 测试

- 单元测试:`audit_coverage.rs` 计算 covered / uncovered / coverage_ratio 正确性
- 集成测试:`w10_audit_coverage_smoke.rs`:
  - `audit_registry_all_lower_snake_case`:registry 所有 event_type 匹配 `^[a-z][a-z0-9_]*$`
  - `audit_emit_rejects_unknown_event_type`:emit "unknown_event" 返回 Err
  - `audit_coverage_100_percent_after_full_skill_run`:跑 8 个 Skill happy path + Kill Switch + Stronghold unlock,断言覆盖率 100%
- Fitness Function:`w10_default_boundary_smoke.rs::audit_coverage_full` 强制 100%

---

## 八、Plan 6 — 集成验收 + Fitness Functions 闭合

### 8.1 测试矩阵

**新增测试文件:**

| 文件 | feature gate | 测试数 | 验收项 |
|---|---|---|---|
| `w10_default_boundary_smoke.rs` | default | 16 | ⑥⑦⑨⑩ Fitness Functions |
| `w10_verifier_coverage_smoke.rs` | default | 8 | ⑥ 各 Skill Verifier |
| `w10_compensation_coverage_smoke.rs` | default | 6 | ⑦ 各可逆 Skill reverse |
| `w10_audit_coverage_smoke.rs` | default | 3 | ⑩ registry + 校验 + 覆盖率 |
| `kill_switch_sla.rs` | default | 3 | ⑨ 1s SLA |
| `w10_voice_latency_smoke.rs` | voice | 1 (`#[ignore]`) | ⑧ P95 ≤ 500ms |

**w10_default_boundary_smoke.rs 16 个测试:**

1. `verifier_coverage_all_strong`:8/8 Skill evidence_strength=strong
2. `compensation_coverage_all_strong`:6/6 可逆 Skill status=Reversed
3. `kill_switch_sla_met_from_executing`:Executing → Cancelling → Cancelled ≤ 1s
4. `kill_switch_sla_met_from_verifying`:Verifying → Cancelling → Cancelled ≤ 1s
5. `kill_switch_sla_met_from_compensating`:Compensating → Cancelling → Cancelled ≤ 1s
6. `kill_switch_immediate_from_idle`:Idle → Cancelled ≤ 1s(无 Cancelling 中间态)
7. `kill_switch_immediate_from_listening`:Listening → Cancelled ≤ 1s
8. `kill_switch_immediate_from_planning`:Planning → Cancelled ≤ 1s
9. `kill_switch_immediate_from_awaiting_approval`:AwaitingApproval → Cancelled ≤ 1s
10. `task_state_cancelling_transitions_legal`:Cancelling → Cancelled 合法
11. `task_state_cancelling_transitions_illegal`:Cancelling → 其他态非法
12. `dag_status_cancelling_transitions_legal`:DAG Running → Cancelling → Cancelled 合法
13. `dag_status_cancelling_transitions_illegal`:DAG Cancelling → Running 非法
14. `audit_registry_all_lower_snake_case`:registry 命名规范
15. `audit_coverage_full`:registry 100% 覆盖
16. `voice_latency_table_exists`:migration 008 创建表

### 8.2 7 套 feature 组合 cargo check 矩阵

```powershell
cargo check --workspace --no-default-features                             # PASS
cargo check --workspace --features voice                                  # PASS
cargo check --workspace --features llm                                    # PASS
cargo check --workspace --features voice,llm                              # PASS
cargo check --workspace --features voice,tauri,llm                        # PASS
cargo check --workspace --features voice,tauri,llm,uia                    # PASS (Windows)
cargo check --workspace --features voice,tauri,llm,uia,stronghold         # PASS (Windows)
```

### 8.3 clippy + npm build

```powershell
cargo clippy --workspace --no-default-features -- -D warnings                          # 0 warnings
cargo clippy --workspace --features voice,tauri,llm,uia,stronghold -- -D warnings     # 0 warnings
cd voicepilot/crates/ui/web ; npm.cmd run build                                       # PASS
```

### 8.4 PROGRESS.md 更新

- 添加 W10 段落到 §二(已完成工作详细记录)
- 添加 W10 行到 §一 里程碑表
- 更新 §三 当前 master 状态(测试数 506 → ~540)
- 更新 §四 未完成工作(W10 完成,W11 评测基础设施 + §10.4 工程规范待启动)

### 8.5 提交策略

每个 Plan 一个 commit,共 6 个 commit:

```
Plan 1: feat(w10p1): Strong Verifier coverage 8/8 Skills
Plan 2: feat(w10p2): Strong Compensation reverse fn 6/6 reversible Skills
Plan 3: feat(w10p3): P95 first partial transcript latency benchmark
Plan 4: feat(w10p4): Kill Switch CANCELLING state + 1s SLA
Plan 5: feat(w10p5): audit event_type registry + coverage checker
Plan 6: test(w10p6): integration acceptance + fitness functions closure
```

---

## 九、风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| Plan 1 form.prepare / form.submit Verifier 依赖 Playwright MCP | 测试需 mock,真实 Playwright `#[ignore]` | mock Playwright 返回 canned 响应(W7 Plan 5 已有先例) |
| Plan 2 form.prepare reverse 依赖 Playwright clear 表单 | 同上 | 同上 |
| Plan 3 P95 测试需真实 sherpa-rs 模型(~80MB) | CI 无法下载 | 测试 `#[ignore]`,本地手动运行,记录在 PROGRESS.md |
| Plan 3 mock 音频流可能不代表真实延迟 | P95 测量偏差 | 用预录 WAV fixture + 真实 sherpa-rs 模型,保留真实 ASR 延迟 |
| Plan 4 Cancelling 状态机改动影响现有 Cancelled 测试 | 现有测试可能失败 | 渐进式:先加 Cancelling 不改 Cancelled 直跳路径,再迁移副作用态 |
| Plan 5 registry 漏注册新 event_type | emit_audit 返回 Err,功能阻断 | 集成测试 `audit_coverage_100_percent_after_full_skill_run` 强制全覆盖,CI 拦截 |
| 全 feature 组合 `--features voice,tauri,llm,uia,stronghold` link.exe OOM(Windows 页面文件不足) | cargo check 失败 | 改用 `cargo check -p trust-kernel --features ...` 单 crate 验证(已有先例) |

---

## 十、不在 W10 范围(留 W11+)

### 10.1 评测基础设施(spec ①-⑤)

- `evals/functional/100_tasks.yaml` + inspect eval 配置
- `evals/redteam/50_attacks.yaml` + promptfoo redteam
- `evals/toctou/20_scenarios.yaml`
- `evals/malicious_server/15_scenarios.yaml`
- `evals/data_security/20_scenarios.yaml`
- `evals/run_all.sh` 串联

### 10.2 §10.4 工程规范

- `cargo-deny` + `deny.toml` 许可证与漏洞检查
- `cargo audit` + `npm audit` 周审计
- GitHub Actions CI:lint + test + build + 100 任务评测 + 50 攻击回归
- API docs:rustdoc + typedoc 生成
- ADR 独立文档(目前散落在 spec 设计文档)
- NSIS `.exe` 安装包实际生成(目前 sandbox 阻塞,需非 sandbox 环境)

### 10.3 其他延后项

- Argon2id 性能优化(W9 已记录,低端 Windows 设备 OOM)
- vault 文件权限 600(Windows ACL)
- 真实 Silero VAD(W6b-1 用能量阈值)
- LLM 调用计费 / 速率限制

---

## 十一、用户决策记录

| # | 决策 | 时间 | 上下文 |
|---|---|---|---|
| 1 | W10 范围 = 代码层硬指标优先(⑥⑦⑧⑨⑩) | 2026-08-01 | 评测基础设施 / CI / 打包 / 文档留 W11+ |
| 2 | Plan 划分 = 按验收项横向划分(6 个 Plan) | 2026-08-01 | 每个 Plan 一个 commit,便于 review |
| 3 | Verifier 分母 = 8 个 Skill 全部 | 2026-08-01 | form.submit 验证提交成功、task.explain 验证记录写入 |
| 4 | Compensation 分母 = 6 个可逆 Skill | 2026-08-01 | form.submit / task.explain 显式 `compensation_level=none` |
| 5 | P95 测量点 = ASR 首个 partial transcript | 2026-08-01 | 与 spec VP-FR-001 一致,不测 TTS |
| 6 | Kill Switch 状态机 = Task + DAG 都加 Cancelling | 2026-08-01 | 与 spec VP-FR-009 "≤1s 进入 CANCELLING" 一致 |
| 7 | 审计覆盖率度量 = 事件类型枚举 + 集成测试触发 | 2026-08-01 | 不引入 tarpaulin,运行时校验 + 集成测试双重保险 |

---

## 十二、验收门禁

W10 完成需满足:

- ✅ Plan 1:8/8 Skill evidence_strength=strong(verifier_coverage_all_strong 测试通过)
- ✅ Plan 2:6/6 可逆 Skill compensations.status=Reversed(compensation_coverage_all_strong 测试通过)
- ✅ Plan 3:voice_latency_samples 表存在 + P95 ≤ 500ms(`#[ignore]` 手动运行通过)
- ✅ Plan 4:Kill Switch 3 个 audit 事件按序触发 + SLA ≤ 1s(kill_switch_sla 测试通过)
- ✅ Plan 5:AUDIT_EVENT_TYPE_REGISTRY 全覆盖(audit_coverage_full 测试通过)
- ✅ Plan 6:7 套 feature 组合 cargo check 全 PASS + clippy `-D warnings` 0 警告 + npm build PASS
- ✅ default `cargo test --workspace --no-default-features` ≥ 540 passing(506 + ~34 新增)
- ✅ PROGRESS.md 更新 W10 段落
- ✅ 6 个 commit 直接提交 master(不使用 worktree)

---

## 十三、下一步

设计批准后,调用 `superpowers:writing-plans` skill 创建详细实施计划(每个 Plan 一个 .md 文件,引用本设计文档)。
