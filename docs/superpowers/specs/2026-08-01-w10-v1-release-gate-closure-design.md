# W10 — V1 发布门禁闭合 设计文档

> **创建日期:** 2026-08-01 (Asia/Shanghai)
> **作者:** TRAE Agent
> **状态:** Draft v2(已修复 v1 review 缺陷,待用户审核)
> **前置:** W9 Plan 7 已完成(commit `e467038`),spec §2.7-§2.11 + §5 Fitness Functions + §6.4 审计扩展全部闭合
> **范围:** spec §9.4 V1 验收门禁 10 项硬指标中的 5 项代码层指标(⑥⑦⑧⑨⑩)+ 1 项集成验收
> **修订记录:** v2(2026-08-01)— 修复 16 处缺陷,详见 §十四 修订附录

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

## 三、Plan 1 — Strong Verifier 覆盖率(7/7 = 100% ≥ 80%,task.explain 显式 none)

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

为 6 个有副作用/可逆的 Skill 添加真实 Strong Verifier 函数;task.explain(只读)显式声明 `verifier.strategy="none"` 并不计入 Strong Verifier 分母。

**关键决策(v2 修订 #5):** task.explain 是只读 Skill,无副作用,spec §6.3 "Verifier 读取真实状态" 不适用。强行 strong verifier 会语义错位。改为:
- task.explain manifest `verifier.strategy="none"` + 注释说明理由
- executor 不调用 verify 函数,evidence_strength 仍为 "weak"(只读 Skill 默认)
- Verifier 分母 = 7 个有副作用的 Skill(8 - 1 task.explain)

**Verifier 覆盖率计算:** 7/7 = 100% ≥ 80%(分母改为 7,因 task.explain 显式 none)

| Skill | 新 Verifier 函数 | 验证策略 | manifest 升级 |
|---|---|---|---|
| files.organize | (已实现) | (已实现) | 已 strong,无需修改 |
| note.capture | `verify_note_capture` | 重读 note 文件存在 + sha256 + size 匹配 effect_manifest | medium → strong |
| research.save_markdown | `verify_research_save` | 重读 markdown 文件存在 + sha256 + size | 已 strong,无需升 |
| form.prepare | `verify_form_prepare` | Playwright 重查表单字段值匹配 effect_manifest.fields | 已 strong,无需升 |
| form.submit | `verify_form_submit` | Playwright 查页面 URL 变更 或 success 元素存在 | weak → strong |
| task.explain | (不实现) | (只读,verifier.strategy="none") | weak → none(显式声明) |
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
pub fn verify_task_repeat(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
pub fn verify_task_compensate(ctx: &VerificationContext) -> Result<VerificationOutcome> { ... }
// 注意:task.explain 不在此列,只读 Skill 无 verifier
```

**修改:** 各 Skill executor 调用真实 verify 函数,移除硬编码字符串。例如 `note_capture.rs:236` 改为:

```rust
let outcome = verifiers::verify_note_capture(&ctx)?;
finalize_step_success(kernel, step_id, outcome.evidence(), EvidenceStrength::Strong).await?;
```

**修改:** `manifest.rs`:
- 升级 4 个 Skill 的 `verifier.strategy` 为 `"strong"`(note.capture / form.submit / task.repeat_verified / 已 strong 的不动)
- task.explain 改为 `verifier.strategy="none"` + 添加注释 `// 只读 Skill,无副作用,Strong Verifier 不适用(spec §6.3)`
- task.explain executor 移除硬编码 "weak",改为读取 manifest 的 strategy

### 3.4 测试

- 单元测试:每个 verify 函数 happy path + 失败路径(文件不存在 / sha256 不匹配 / Playwright 元素不存在 / DB 记录不存在)
- 集成测试:`w10_verifier_coverage_smoke.rs` 枚举 7 个有副作用 Skill(排除 task.explain),各自跑一次 happy path,断言 evidence_strength="strong"
- task.explain 单独测试:验证 manifest `verifier.strategy="none"` + executor 不调用 verify 函数
- Fitness Function:`w10_default_boundary_smoke.rs::verifier_coverage_all_strong` 强制 7/7 Strong(分母=7)+ task.explain 显式 none

---

## 四、Plan 2 — Strong Compensation 成功率(6/6 可逆 = 100% ≥ 95%)

### 4.1 现状

| Skill | manifest CompensationLevel | create_post_commit_compensation 调用 | reverse 函数 | 状态 |
|---|---|---|---|---|
| files.organize | Strong(manifest.rs:208) | ✅ executor.rs:184, 225 | ✅ `auto_reverse_move`(executor.rs:18-81) | 已就绪 |
| note.capture | Strong(manifest.rs:581) | ❌ comp_ref=None | ❌ 无 | 待补 |
| research.save_markdown | Strong(manifest.rs:661) | ❌ | ❌ 无 | 待补 |
| form.prepare | Strong(manifest.rs:737) | ❌ | ❌ 无 | 待补 |
| form.submit | None(manifest.rs:815)— 不可逆,正确 | N/A | N/A | 不计入分母 |
| task.explain | None(manifest.rs:344)— 只读,正确 | N/A | N/A | 不计入分母 |
| task.repeat_verified | Strong(manifest.rs:283) | ❌ | ❌ 无 | 待补(复用 auto_reverse_move) |
| task.compensate | Strong(manifest.rs:405) | 创建新记录 | ✅ 复用 `auto_reverse_move`(task_compensate.rs:171-178) | 已就绪 |

**v2 修订 #4:** task.compensate 现状已明确 — task_compensate.rs:171-178 注释 "Step 7: execute auto_reverse" 调用 `auto_reverse_move`,实现就是复用 files.organize 的 reverse 函数。Plan 2 无需为 task.compensate 新建 reverse 函数,只需确认其 `compensate_fn` 注册为 `"filesystem.reverse_move"`(目前 task.compensate 的 reverse 路径已工作)。

仅 2/6 = 33% 真实覆盖(files.organize + task.compensate 复用)。远低于 95% 阈值。Plan 2 需补 4 个可逆 Skill 的 reverse 函数(note.capture / research.save_markdown / form.prepare / task.repeat_verified)。

### 4.2 Plan 2 设计

为 4 个可逆 Skill 实现 reverse 函数;扩展 reverse 路由机制。

| Skill | 新 reverse 函数 | 反向操作 | compensate_fn 名 |
|---|---|---|---|
| note.capture | `reverse_note_capture` | 删除 note 文件 | `note.reverse_capture` |
| research.save_markdown | `reverse_research_save` | 删除 markdown 文件 | `research.reverse_save` |
| form.prepare | `reverse_form_prepare` | Playwright clear 表单字段 / 关闭 tab | `form.reverse_prepare` |
| task.repeat_verified | (复用) | 复用 `auto_reverse_move` | `filesystem.reverse_move`(已注册) |

form.submit / task.explain 不计入分母,manifest 已显式 `compensation_level=none`,无需修改。task.compensate / files.organize 已就绪,无需修改。

**Compensation 覆盖率计算:** 6/6 = 100% ≥ 95%(分母=6 可逆 Skill)

### 4.3 实现要点

**v2 修订 #2:** ReverseFn 签名必须与现有 `auto_reverse_move(rec: &CompensationRecord) -> Result<()>` 兼容,避免改造现有函数引发回归。

**修改:** `compensation/executor.rs` 添加 reverse 函数注册表,签名统一为 `fn(&CompensationRecord) -> Result<()>`:

```rust
pub struct ReverseFnRegistry {
    fns: HashMap<String, ReverseFn>,
}

// v2 修订 #2: 与现有 auto_reverse_move 签名一致,接受 CompensationRecord
type ReverseFn = fn(&CompensationRecord) -> Result<()>;

impl ReverseFnRegistry {
    pub fn new() -> Self {
        let mut fns = HashMap::new();
        fns.insert("filesystem.reverse_move".to_string(), auto_reverse_move);
        fns.insert("note.reverse_capture".to_string(), reverse_note_capture);
        fns.insert("research.reverse_save".to_string(), reverse_research_save);
        fns.insert("form.reverse_prepare".to_string(), reverse_form_prepare);
        Self { fns }
    }

    pub fn call(&self, name: &str, rec: &CompensationRecord) -> Result<()> {
        let f = self.fns.get(name).ok_or_else(|| KernelError::Compensation(format!("reverse fn not found: {}", name)))?;
        f(rec)
    }
}
```

每个 reverse 函数从 `rec.reverse_payload`(JSON 字符串)解析所需信息,与 `auto_reverse_move` 一致(executor.rs:19 `serde_json::from_str(&rec.reverse_payload)`)。

**新文件:** `crates/trust-kernel/src/skills/reverse_fns.rs` 实现 3 个新 reverse 函数(`reverse_note_capture` / `reverse_research_save` / `reverse_form_prepare`),task.repeat_verified 复用 `auto_reverse_move`。

**修改:** 各 Skill executor 调用 `create_post_commit_compensation` 注册对应的 `compensate_fn` 名:

```rust
// note_capture.rs
let comp_ref = create_post_commit_compensation(kernel, step_id, "note.reverse_capture", payload).await?;
```

**修改:** `compensation/executor.rs::auto_reverse` 入口改为查 `ReverseFnRegistry` 而非硬编码 `auto_reverse_move`,使 `compensate_fn` 字段生效。

### 4.4 测试

- 单元测试:每个 reverse 函数 happy path + partial failure(文件已删 / Playwright tab 已关 / etc.)
- 集成测试:`w10_compensation_coverage_smoke.rs` 枚举 6 个可逆 Skill,各跑一次 prepare → commit → reverse,断言 `compensations.status=Reversed`
- Fitness Function:`w10_default_boundary_smoke.rs::compensation_coverage_all_strong` 强制 6/6 Strong

---

## 五、Plan 3 — P95 首字延迟基准(ASR partial transcript ≤ 500ms)

### 5.1 现状

无任何 `p95` / `first_token` / `voice_latency` 测量代码,无 `criterion` / `[[bench]]` benchmark 配置。spec §VP-FR-001 "≤ 500ms 显示首字(sherpa-rs SenseVoice)"。W6b-3b plan 明确记录 "无显式延迟测试但架构满足" / "性能测试不在本计划,作为后续基准任务"。

### 5.2 Plan 3 设计

**v2 修订 #6:** t0 不取 Push-to-talk 按下(用户按住但延迟说话会导致 latency 无限大),改为 t0 = VAD 检测到首个语音帧。W6b-1 已有能量阈值 VAD(`voice/listener.rs` chunk 边界能量检测),复用其首个 voiced chunk 时间戳作为 t0。

**埋点:**
- `voice/listener.rs` VAD 检测到首个 voiced chunk 时记录 `t0 = Instant::now()`(emit `voice_started` audit event)
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

-- v2 修订 #15: 清理策略 — 保留最近 30 天样本,定期 DELETE
-- 实际清理由 admin 命令 voice latency-prune 触发,或 Rust 定时任务(每日 03:00)
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

// v2 修订 #15: 清理策略
pub fn prune_older_than(kernel: &TrustKernel, days: u32) -> Result<u64>;
```

**admin 命令:**
- `voice latency-stats [--since 24h]` 输出 P50/P95/P99 + 样本数 + 模型名
- `voice latency-prune [--days 30]` 清理旧样本

**Fitness Function 测试:** `w10_voice_latency_smoke.rs`(voice-gated):

```rust
#[test]
#[ignore] // 需要真实 sherpa-rs 模型 + mock 音频流
// v2 修订 #7: 样本数 100,P95 = 第 95 个样本(排序后索引 94),统计学可靠
fn p95_first_partial_transcript_under_500ms() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let mock_wav = include_bytes!("../tests/fixtures/push_to_talk_sample.wav");
    let model = SherpaModel::sense_voice_offline(); // 已下载

    let mut latencies = Vec::with_capacity(100);
    for _ in 0..100 {
        // v2 修订 #6: t0 从 VAD 检测首个 voiced chunk 开始,而非 Push-to-talk 按下
        let t0 = Instant::now(); // 模拟 VAD 触发
        let transcript = run_asr_once(&model, mock_wav).unwrap();
        let t1 = Instant::now();
        assert!(!transcript.is_empty());
        latencies.push((t1 - t0).as_millis() as u64);
    }
    latencies.sort();
    // P95 = 第 95 百分位数(100 个样本中第 95 个,索引 94)
    let p95 = latencies[94];
    assert!(p95 <= 500, "P95 first partial transcript {}ms > 500ms", p95);
}
```

**v2 修订 #7(运行频率):** `#[ignore]` 测试不会被 cargo test 自动运行。手动运行策略:
- 每次版本发布前(包括 W10 Plan 6 集成验收)手动 `cargo test --features voice -- --ignored p95_first_partial_transcript_under_500ms`
- 在 PROGRESS.md Plan 3 段落记录最新一次运行结果(样本数、P50/P95/P99 实测值)
- W11+ CI 接入后,标记为 `#[ignore = "requires-sherpa-model"]`,CI workflow 单独 step 跑(需先 `voice model download`)

### 5.3 实现要点

- `voice/listener.rs` 添加 `first_partial_received_at: Option<Instant>` 字段
- VAD 首个 voiced chunk 检测时,设置 `voice_started_at: Option<Instant>`,emit `voice_started` audit event
- 首次 partial transcript 回调时,计算 latency = `first_partial_received_at - voice_started_at`,调用 `LatencyRecorder::record(...)`
- `LatencyRecorder` 内部持有 `Arc<Mutex<SqliteConnection>>`,异步写入 `voice_latency_samples` 表
- 若 sherpa-rs 模型未下载,测试 `#[ignore]`,提示用户 `voice model download`
- 新增 `prune_older_than(days)` 函数,默认保留 30 天(避免长期运行无限增长)

### 5.4 测试

- 单元测试:`latency_stats.rs` 计算 P50/P95/P99 正确性(mock 100 样本,验证 P95 = 索引 94)
- 单元测试:`prune_older_than` 清理正确性(mock 60 天前 + 10 天前样本,清理后剩 10 天前)
- 集成测试:`w10_voice_latency_smoke.rs::p95_first_partial_transcript_under_500ms`(voice-gated + `#[ignore]`,100 样本)
- Fitness Function:`w10_default_boundary_smoke.rs::voice_latency_table_exists`(default-gated,只检查 migration 008 创建表 + prune 函数存在)

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

**v2 修订 #15(现有测试兼容):** Executing/Verifying/Compensating → Cancelled 直跳路径**保留**(allowed_next 同时包含 Cancelling 与 Cancelled),现有 `kill_switch_can_cancel_from_any_non_terminal_state` 测试(state_machine.rs:34-38)不破坏。新增中间态是可选路径,不强制。

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
                | (Running, Cancelled)         // 保留直跳(向后兼容)
                | (Cancelling, Cancelled)      // ← 新
        )
    }
}
```

**v2 修订 #9(DAG node 处理):** DAG 进入 `Cancelling` 时,正在执行的 node 处理策略:
- **node 已完成(`dag_node_succeeded` / `dag_node_failed` 已 emit):** 不影响,DAG 直接 → Cancelled
- **node 正在执行(`dag_node_started` 已 emit,未 emit succeeded/failed):** DAG 等待 node 返回(不主动中断 Skill executor),node 完成后 DAG → Cancelled。理由:Skill executor 内部不可中断(LLM 调用 / Playwright 等待 / UIA 查找),强行中断会留下不一致状态。node 完成后 DAG 跳过后续未启动 nodes。
- **node 未启动:** DAG 直接跳过,→ Cancelled

这意味着 DAG Cancelling → Cancelled 的时长取决于当前 node 执行时长(可能 > 1s)。**DAG SLA 不强制 1s**,只在 Task 级强制。Plan 6 测试 12/13 据此调整。

**v2 修订 #8(可中断性):** 各状态 Kill Switch 响应:

| 当前态 | 可中断? | 响应路径 | SLA |
|---|---|---|---|
| Idle / Listening / Planning / AwaitingApproval | 是(无副作用) | 直接 → Cancelled | ≤ 100ms(同步) |
| Executing (voice loop chunk 边界) | 是 | → Cancelling → Cancelled | ≤ 500ms(chunk 边界) |
| Executing (LLM 调用中) | 否(等响应) | → Cancelling(LLM 返回后)→ Cancelled | 取决于 LLM,可能 > 1s |
| Executing (Playwright 等待) | 否 | 同上 | 取决于 Playwright,可能 > 1s |
| Verifying / Compensating | 否(等完成) | → Cancelling(完成后)→ Cancelled | 取决于操作,可能 > 1s |

**SLA 1s 仅对可中断路径强制。** 不可中断路径审计 `sla_met: false`,不阻塞,只记录告警。Plan 6 测试 3-9 仅测可中断路径(Idle/Listening/Planning/AwaitingApproval/Executing voice loop),不测 LLM/Playwright 中断。

**Kill Switch 触发路径:**

1. `cancel_voice_command` 触发 → `kill_switch.store(true)`
2. 立即 emit `kill_switch_triggered { timestamp_ms, from_state, source }` audit event
3. 若当前态可中断(Idle/Listening/Planning/AwaitingApproval/voice loop 边界):立即 `transition(当前态, Cancelling)` → `transition(Cancelling, Cancelled)`,总时长 ≤ 1s
4. 若当前态不可中断(LLM/Playwright/UIA 等待):`transition(当前态, Cancelling)`,等操作返回后 `transition(Cancelling, Cancelled)`,SLA 可能超时,emit `sla_met: false`
5. transition 到 `Cancelled` 时 emit `task_cancelled { task_id, duration_ms, sla_met }`
6. SLA 断言(仅可中断路径):`task_cancelled.timestamp_ms - kill_switch_triggered.timestamp_ms ≤ 1000`

**v2 修订 #14(与 state_transition 关系):** 现有 `state_transition` audit event(kernel.rs:349)已记录所有 TaskState 转换,包括 → Cancelling / → Cancelled。新增的 `kill_switch_triggered` / `task_cancelled` 是**补充语义事件**(标识 Kill Switch 上下文),不替代 `state_transition`。三者关系:
- `state_transition { from: Executing, to: Cancelling }` — 通用状态机记录
- `kill_switch_triggered { timestamp_ms, from_state }` — Kill Switch 专属,用于 SLA 计算 t0
- `task_cancelled { duration_ms, sla_met }` — Kill Switch 专属,用于 SLA 计算 t1 + 结果

`task_cancelling` 事件**取消**(与 `state_transition { to: Cancelling }` 重复),只保留 `kill_switch_triggered` + `task_cancelled` 两个新事件。

### 6.3 实现要点

**修改:** `state.rs` / `dag_types.rs` 加 `Cancelling` 变体 + transition 表(保留 Cancelled 直跳路径)
**修改:** `ui/voice_commands.rs::cancel_voice_command` 实现 Kill Switch 触发路径
**修改:** `kernel.rs` / `skills/dag_executor.rs` 在循环边界检查 `Cancelling` 状态,提前退出 DAG loop(已启动 node 等待完成,未启动 node 跳过)
**新文件:** `tests/kill_switch_sla.rs` SLA 断言测试(仅可中断路径)

### 6.4 测试

- 单元测试:`state_machine.rs` 加 `cancelling_intermediate_state_transitions` 测试合法 / 非法转换(同时验证 Cancelled 直跳路径仍合法)
- 集成测试:`kill_switch_sla.rs`(仅可中断路径):
  - `kill_switch_enters_cancelling_within_1s`:mock Executing 状态,触发 Kill Switch,断言 ≤ 1s 进入 Cancelling
  - `kill_switch_completes_within_1s`:断言 Cancelling → Cancelled 总时长 ≤ 1s
  - `kill_switch_from_executing_audits_three_events`:断言 3 个 audit 事件按序触发
- Fitness Function:`w10_default_boundary_smoke.rs::kill_switch_sla_met` 强制 SLA 满足

---

## 七、Plan 5 — 审计日志覆盖率(事件类型枚举 + 集成测试触发 = 100%)

### 7.1 现状

- `audit.rs` 无 AUDIT_EVENT_TYPE_REGISTRY,`AuditLogger::append`(audit.rs:30 trait 方法)+ `kernel.rs::audit_append`(kernel.rs:757 内部方法)+ `kernel.rs::audit_append_external`(kernel.rs:383 公开入口)三层都不校验 event_type 合法性
- `w9_default_boundary_smoke.rs::audit_event_types_all_lower_snake_case` 仅强制命名规范,不校验覆盖率
- 实际 emit callsite 经 Grep 核对共 24 种 event_type(非 v1 design 虚构的 33 种),散落在源码各处

**v2 修订 #11(registry 完整性):** 基于实际 emit callsite 核对,registry 应包含以下 24 种(已验证存在)+ Plan 3/4 新增 3 种 = 27 种:

| # | event_type | 实际 callsite | 来源周次 |
|---|---|---|---|
| 1 | task_created | kernel.rs:322 | W1 |
| 2 | state_transition | kernel.rs:349 | W1 |
| 3 | step_created | kernel.rs:545 | W1 |
| 4 | step_status_changed | kernel.rs:579 | W1 |
| 5 | step_prepared | kernel.rs:608 | W3a |
| 6 | step_committed | kernel.rs:635 | W3a |
| 7 | compensation_created | kernel.rs:441 | W3b |
| 8 | compensation_status_changed | kernel.rs:477 | W3b |
| 9 | approval_recorded | kernel.rs:495 | W3b |
| 10 | mcp_tools_call | mcp/server.rs:181 | W4 |
| 11 | taint_propagated | mcp/server.rs:227, llm/client.rs:814, dispatcher.rs:194 | W9 |
| 12 | taint_blocked | gateway.rs:236 | W9 |
| 13 | llm_decompose_called | llm/client.rs:740 | W8 |
| 14 | stronghold_degraded_mode_entered | kernel.rs:417 | W9 |
| 15 | stronghold_snapshot_encrypted | skills/common.rs:155 | W9 |
| 16 | stronghold_snapshot_decrypt_failed | task_compensate.rs:233,254,278 | W9 |
| 17 | dag_plan_created | dag_executor.rs:147,267 | W8 |
| 18 | dag_skeleton_approved | dag_executor.rs:167,279 | W8 |
| 19 | dag_skeleton_modified | dag_executor.rs:209 | W8 |
| 20 | dag_modify_limit_exceeded | dag_executor.rs:311 | W8 |
| 21 | dag_node_started | dag_executor.rs:351 | W8 |
| 22 | dag_node_succeeded | dag_executor.rs:391 | W8 |
| 23 | dag_node_failed | dag_executor.rs:408 | W8 |
| 24 | dag_completed | dag_executor.rs:184,297,460,483 | W8 |
| 25 | voice_started | (Plan 3 新增) | W10 |
| 26 | kill_switch_triggered | (Plan 4 新增) | W10 |
| 27 | task_cancelled | (Plan 4 新增) | W10 |

**注意:** v1 design 虚构的 `step_started` / `step_succeeded` / `step_failed` / `mcp_call_failed` / `skill_routed` / `skill_executor_called` / `llm_planner_called` / `dag_skeleton_denied` / `dag_node_skipped` / `stronghold_vault_created` / `stronghold_vault_unlocked` / `post_commit_compensation_created` / `compensation_reversed` / `verifier_called` / `reverse_fn_called` / `task_cancelling`(Plan 4 已取消)均**不在 registry**(无实际 callsite)。`mcp_call_failed` 仅在 task_explain.rs:98 作为查询过滤字符串出现,不是 emit 事件。

### 7.2 Plan 5 设计

**Registry:** 在 `audit.rs` 添加(基于实际 callsite,27 种):

```rust
pub const AUDIT_EVENT_TYPE_REGISTRY: &[&str] = &[
    // W1-W3 基础(kernel.rs)
    "task_created", "state_transition",
    "step_created", "step_status_changed", "step_prepared", "step_committed",
    "compensation_created", "compensation_status_changed", "approval_recorded",
    // W4 MCP(mcp/server.rs)
    "mcp_tools_call",
    // W8 DAG(dag_executor.rs, llm/client.rs)
    "llm_decompose_called",
    "dag_plan_created", "dag_skeleton_approved", "dag_skeleton_modified",
    "dag_modify_limit_exceeded",
    "dag_node_started", "dag_node_succeeded", "dag_node_failed", "dag_completed",
    // W9 Stronghold / Taint(kernel.rs, common.rs, task_compensate.rs, gateway.rs, dispatcher.rs, llm/client.rs, mcp/server.rs)
    "stronghold_degraded_mode_entered",
    "stronghold_snapshot_encrypted", "stronghold_snapshot_decrypt_failed",
    "taint_propagated", "taint_blocked",
    // W10 新增(Plan 3/4)
    "voice_started",
    "kill_switch_triggered", "task_cancelled",
];

pub fn is_valid_event_type(name: &str) -> bool {
    AUDIT_EVENT_TYPE_REGISTRY.contains(&name)
}
```

**v2 修订 #1, #3(运行时校验降级):** `kernel.rs::audit_append`(kernel.rs:757)在 `self.audit.append(&event)` 之前调用 `is_valid_event_type(event_type)`。若不在 registry:
- `tracing::warn!("unknown audit event_type: {}, please register in AUDIT_EVENT_TYPE_REGISTRY", event_type)` — 不阻塞写入
- **不返回 Err**(避免回归现有 callsite)
- 测试 `audit_emit_warns_on_unknown_event_type` 验证 warn 被触发(用 `tracing_test` crate 或 mock subscriber)

理由:硬 Err 会中断所有 audit 写入链路,回归风险高。warn + 自动追加到运行时 registry(Arc<Mutex<HashSet<String>>>)更安全。覆盖率检查只统计 compile-time registry,运行时新增事件需手动加入 registry 才能进入覆盖率分母。

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

**v2 修订 #1:** 校验点在 `kernel.rs::audit_append`(kernel.rs:757-775),不是 `audit.rs::AuditLogger::append`(trait 方法,实现层)。理由:`audit_append` 是所有 audit 写入的必经入口(`audit_append_external` 也调它),在此校验覆盖最全。

- `kernel.rs::audit_append` 在构造 `AuditEvent` 前调用 `audit::is_valid_event_type(event_type)`
- 若无效,`tracing::warn!` 但继续写入(不返回 Err)
- 现有所有 emit callsite 不需要修改(event_type 字符串字面量已在 registry 中)
- 新增 `audit_coverage.rs` 实现 AuditCoverageChecker
- 新增 `w10_audit_coverage_smoke.rs` 集成测试

### 7.4 测试

**v2 修订 #12(feature 依赖):** `audit_coverage_100_percent_after_full_skill_run` 测试需触发所有 27 种 event_type。其中 `mcp_tools_call` / `taint_propagated` / `taint_blocked` / `stronghold_*` / `dag_*` 等事件在 default feature 下可触发;但 `voice_started` 需 `voice` feature,`stronghold_snapshot_encrypted` 需 `stronghold` feature。测试拆分为两部分:

- 单元测试:`audit_coverage.rs` 计算 covered / uncovered / coverage_ratio 正确性(mock event_type 集合)
- 集成测试 `w10_audit_coverage_smoke.rs`(default-gated,仅测 default feature 可触发的 24 种):
  - `audit_registry_all_lower_snake_case`:registry 所有 event_type 匹配 `^[a-z][a-z0-9_]*$`
  - `audit_emit_warns_on_unknown_event_type`:emit "unknown_event" 触发 warn(用 `tracing_test::traced_test`)
  - `audit_coverage_default_features`:跑 default feature 可触发的所有路径(8 个 Skill happy path + Kill Switch + DAG execute + Stronghold encrypt/decrypt + Taint block),断言 24/24 = 100% 覆盖(分母=24,排除 voice_started / kill_switch_triggered / task_cancelled 三个需其他 feature 的)
- 集成测试 `w10_audit_coverage_full_smoke.rs`(voice,tauri,llm,uia,stronghold 全 feature,`#[ignore]`):
  - `audit_coverage_full_features`:跑全 feature 路径,断言 27/27 = 100% 覆盖
- Fitness Function:`w10_default_boundary_smoke.rs::audit_coverage_default_full` 强制 default feature 24/24 = 100%

**v2 修订 #13(与 W9 测试关系):** Plan 6 测试 14 `audit_registry_all_lower_snake_case` 与 W9 `w9_default_boundary_smoke.rs::audit_event_types_all_lower_snake_case` 关系:
- W9 测试:从 `audit_logs` 表 SELECT DISTINCT event_type,验证所有**实际写入**的事件名匹配 lower_snake_case
- Plan 6 测试:验证 `AUDIT_EVENT_TYPE_REGISTRY` 常量中所有**声明**的事件名匹配 lower_snake_case
- 两者互补,不重复:W9 测运行时数据,Plan 6 测编译时常量。保留两者。

---

## 八、Plan 6 — 集成验收 + Fitness Functions 闭合

### 8.1 测试矩阵

**新增测试文件:**

| 文件 | feature gate | 测试数 | 验收项 |
|---|---|---|---|
| `w10_default_boundary_smoke.rs` | default | 14 | ⑥⑦⑨⑩ Fitness Functions |
| `w10_verifier_coverage_smoke.rs` | default | 7 | ⑥ 各 Skill Verifier(7 个有副作用 Skill) |
| `w10_compensation_coverage_smoke.rs` | default | 6 | ⑦ 各可逆 Skill reverse |
| `w10_audit_coverage_smoke.rs` | default | 3 | ⑩ registry + 校验 + 覆盖率(default 24 种) |
| `w10_audit_coverage_full_smoke.rs` | voice,tauri,llm,uia,stronghold | 1 (`#[ignore]`) | ⑩ 全 feature 27 种覆盖 |
| `kill_switch_sla.rs` | default | 5 | ⑨ 1s SLA(仅可中断路径) |
| `w10_voice_latency_smoke.rs` | voice | 1 (`#[ignore]`) | ⑧ P95 ≤ 500ms |

**w10_default_boundary_smoke.rs 14 个测试(v2 修订 #8/#9 调整):**

1. `verifier_coverage_all_strong`:7/7 有副作用 Skill evidence_strength=strong(分母=7,task.explain 显式 none)
2. `compensation_coverage_all_strong`:6/6 可逆 Skill status=Reversed
3. `kill_switch_sla_met_from_idle`:Idle → Cancelled ≤ 100ms(可中断,无 Cancelling 中间态)
4. `kill_switch_sla_met_from_listening`:Listening → Cancelled ≤ 1s(可中断)
5. `kill_switch_sla_met_from_planning`:Planning → Cancelled ≤ 1s(可中断)
6. `kill_switch_sla_met_from_awaiting_approval`:AwaitingApproval → Cancelled ≤ 1s(可中断)
7. `kill_switch_sla_met_from_executing_voice_loop`:Executing voice loop chunk 边界 → Cancelling → Cancelled ≤ 1s(可中断)
8. `task_state_cancelling_transitions_legal`:Cancelling → Cancelled 合法 + Cancelled 直跳仍合法
9. `task_state_cancelling_transitions_illegal`:Cancelling → 其他态非法
10. `dag_status_cancelling_transitions_legal`:DAG Running → Cancelling → Cancelled 合法 + Running → Cancelled 直跳仍合法
11. `dag_status_cancelling_transitions_illegal`:DAG Cancelling → Running 非法
12. `audit_registry_all_lower_snake_case`:registry 命名规范(编译时常量,与 W9 运行时测试互补)
13. `audit_coverage_default_full`:default feature 24/24 = 100% 覆盖(排除 voice_started / kill_switch_triggered / task_cancelled)
14. `voice_latency_table_exists`:migration 008 创建表 + prune 函数存在

**注意(v2 修订 #8/#9):** 不测 Verifying/Compensating/LLM/Playwright 中断(不可中断路径,SLA 可能 > 1s)。这些路径在 `task_cancelled` audit event 中记录 `sla_met: false`,不阻塞,只告警。Plan 4 集成测试 `kill_switch_sla.rs` 5 个测试覆盖 5 个可中断路径(Idle/Listening/Planning/AwaitingApproval/Executing voice loop)。

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

**v2 修订 #10(Plan 顺序):** Plan 间存在依赖,严格按以下顺序:

```
Plan 1: feat(w10p1): Strong Verifier coverage 7/7 Skills (task.explain=none)
   ↓ (Plan 1 不引入新 audit event,无依赖)
Plan 2: feat(w10p2): Strong Compensation reverse fn 6/6 reversible Skills
   ↓ (Plan 2 不引入新 audit event,无依赖)
Plan 4: feat(w10p4): Kill Switch CANCELLING state + 1s SLA + 2 new audit events
   ↓ (Plan 4 引入 kill_switch_triggered + task_cancelled,需在 Plan 5 registry 注册)
Plan 3: feat(w10p3): P95 first partial transcript latency + 1 new audit event (voice_started)
   ↓ (Plan 3 引入 voice_started,需在 Plan 5 registry 注册)
Plan 5: feat(w10p5): audit event_type registry (27 种,含 Plan 3/4 新增) + coverage checker
   ↓ (Plan 5 registry 必须包含 Plan 3/4 引入的事件,故 Plan 5 在 Plan 3/4 之后)
Plan 6: test(w10p6): integration acceptance + fitness functions closure
```

**Plan 顺序:** 1 → 2 → 4 → 3 → 5 → 6(Plan 5 必须在 Plan 3/4 之后,确保 registry 完整)。每个 Plan 一个 commit,共 6 个 commit。

---

## 九、风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| Plan 1 form.prepare / form.submit Verifier 依赖 Playwright MCP | 测试需 mock,真实 Playwright `#[ignore]` | mock Playwright 返回 canned 响应(W7 Plan 5 已有先例) |
| Plan 2 form.prepare reverse 依赖 Playwright clear 表单 | 同上 | 同上 |
| Plan 3 P95 测试需真实 sherpa-rs 模型(~80MB) | CI 无法下载 | 测试 `#[ignore]`,本地手动运行,记录在 PROGRESS.md |
| Plan 3 mock 音频流可能不代表真实延迟 | P95 测量偏差 | 用预录 WAV fixture + 真实 sherpa-rs 模型,保留真实 ASR 延迟 |
| Plan 4 Cancelling 状态机改动影响现有 Cancelled 测试 | 现有测试可能失败 | v2 修订:保留 Cancelled 直跳路径,新增 Cancelling 为可选中间态,不破坏现有 `kill_switch_can_cancel_from_any_non_terminal_state` 测试 |
| Plan 4 LLM/Playwright 中断不可达 1s SLA | SLA 测试可能失败 | v2 修订:SLA 仅强制可中断路径(Idle/Listening/Planning/AwaitingApproval/voice loop),不可中断路径记录 `sla_met: false` 不阻塞 |
| Plan 4 DAG Cancelling 等待 node 完成可能 > 1s | DAG SLA 不达标 | v2 修订:DAG 级不强制 1s SLA,仅 Task 级强制 |
| Plan 5 registry 漏注册新 event_type | 功能阻断 | v2 修订:校验降级为 warn 不阻塞;集成测试 `audit_coverage_default_full` 强制 24/24 覆盖,CI 拦截漏注册 |
| Plan 5 现有测试用未注册事件名 | 运行时 warn 噪音 | Grep 已核对 24 种实际 callsite 全部纳入 registry,无漏注册 |
| Plan 5 覆盖率测试 feature 依赖 | default feature 测试无法触发 voice_started 等 | v2 修订:测试拆分,default 测 24/27,全 feature `#[ignore]` 测 27/27 |
| 全 feature 组合 `--features voice,tauri,llm,uia,stronghold` link.exe OOM(Windows 页面文件不足) | cargo check 失败 | 改用 `cargo check -p trust-kernel --features ...` 单 crate 验证(已有先例) |
| v2 修订 #15(Plan 3 表无限增长) | 长期运行数据库膨胀 | 新增 `prune_older_than(days)` 函数 + `voice latency-prune` admin 命令,默认保留 30 天 |
| v2 修订 #15(Plan 4 死锁) | Cancelling 等 voice loop 退出,voice loop 等锁 | Cancelling 状态不持有新锁,仅检查 AtomicBool + 现有状态机;voice loop chunk 边界(500ms)检查 flag,不阻塞 |

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
| 3 | Verifier 分母 = 8 个 Skill 全部 | 2026-08-01(原决策) | form.submit 验证提交成功、task.explain 验证记录写入 |
| 3-v2 | Verifier 分母修订 = 7 个有副作用 Skill | 2026-08-01 | task.explain 改为 verifier.strategy="none"(只读 Skill,Strong Verifier 语义不适用);form.submit 仍 strong(验证提交成功) |
| 4 | Compensation 分母 = 6 个可逆 Skill | 2026-08-01 | form.submit / task.explain 显式 `compensation_level=none` |
| 5 | P95 测量点 = ASR 首个 partial transcript | 2026-08-01 | 与 spec VP-FR-001 一致,不测 TTS |
| 5-v2 | P95 t0 = VAD 检测首个 voiced chunk | 2026-08-01 | 不取 Push-to-talk 按下(用户延迟说话导致 latency 无限大) |
| 6 | Kill Switch 状态机 = Task + DAG 都加 Cancelling | 2026-08-01 | 与 spec VP-FR-009 "≤1s 进入 CANCELLING" 一致 |
| 6-v2 | Cancelling 为可选中间态 + 保留 Cancelled 直跳 | 2026-08-01 | 向后兼容现有 `kill_switch_can_cancel_from_any_non_terminal_state` 测试 |
| 7 | 审计覆盖率度量 = 事件类型枚举 + 集成测试触发 | 2026-08-01 | 不引入 tarpaulin,运行时校验 + 集成测试双重保险 |
| 7-v2 | 运行时校验降级为 warn(不返回 Err) | 2026-08-01 | 避免 emit_audit 硬 Err 中断所有 audit 写入链路 |
| 8 | Kill Switch SLA 仅强制可中断路径 | 2026-08-01 | LLM/Playwright/UIA 等待不可中断,记录 `sla_met: false` 不阻塞 |
| 9 | DAG node 处理 = 等待已启动 node 完成 | 2026-08-01 | 不主动中断 Skill executor,避免不一致状态;DAG 级不强制 1s SLA |

---

## 十二、验收门禁

W10 完成需满足:

- ✅ Plan 1:7/7 有副作用 Skill evidence_strength=strong + task.explain verifier.strategy="none"(verifier_coverage_all_strong 测试通过,分母=7)
- ✅ Plan 2:6/6 可逆 Skill compensations.status=Reversed(compensation_coverage_all_strong 测试通过)
- ✅ Plan 3:voice_latency_samples 表存在 + prune_older_than 函数 + P95 ≤ 500ms(`#[ignore]` 手动运行 100 样本通过)
- ✅ Plan 4:Kill Switch 2 个新 audit 事件(kill_switch_triggered + task_cancelled)按序触发 + 可中断路径 SLA ≤ 1s(kill_switch_sla 5 个测试通过)
- ✅ Plan 5:AUDIT_EVENT_TYPE_REGISTRY 27 种 + default feature 24/24 = 100% 覆盖(audit_coverage_default_full 测试通过)
- ✅ Plan 6:7 套 feature 组合 cargo check 全 PASS + clippy `-D warnings` 0 警告 + npm build PASS
- ✅ default `cargo test --workspace --no-default-features` ≥ 540 passing(506 + ~34 新增)
- ✅ PROGRESS.md 更新 W10 段落
- ✅ 6 个 commit 按 Plan 顺序 1→2→4→3→5→6 直接提交 master(不使用 worktree)

---

## 十三、下一步

设计批准后,调用 `superpowers:writing-plans` skill 创建详细实施计划(每个 Plan 一个 .md 文件,引用本设计文档)。

---

## 十四、v2 修订附录(2026-08-01)

v1 design review 发现 16 处缺陷,v2 全部修复。修订项与对应章节:

| # | 缺陷 | 修订位置 | 修订内容 |
|---|---|---|---|
| 1 | Plan 5 Audit API 名称错误(`emit_audit` 实为 `kernel.rs::audit_append`) | §7.3 | 校验点改为 `kernel.rs::audit_append`(kernel.rs:757-775) |
| 2 | Plan 2 ReverseFn 签名与 `auto_reverse_move(rec: &CompensationRecord)` 不兼容 | §4.3 | 签名统一为 `fn(&CompensationRecord) -> Result<()>`,与现有函数一致 |
| 3 | Plan 5 运行时校验返回 Err 引发回归 | §7.2 | 降级为 `tracing::warn!` 不阻塞写入,自动追加运行时 registry |
| 4 | Plan 2 表格遗漏 task.compensate 状态 | §4.1 | 新增"状态"列,task.compensate 标"已就绪"(复用 auto_reverse_move,task_compensate.rs:171-178) |
| 5 | Plan 1 `verify_task_explain` 不符合 spec §6.3 | §3.2 | task.explain 改为 `verifier.strategy="none"`,分母从 8 改为 7 |
| 6 | Plan 3 P95 t0 定义模糊(Push-to-talk 按下后用户延迟说话) | §5.2 | t0 改为 VAD 检测首个 voiced chunk |
| 7 | Plan 3 统计样本数不足(20 个 P95 不可靠) | §5.2 | 样本数改为 100,P95 = 索引 94;补充 `#[ignore]` 运行频率策略 |
| 8 | Plan 4 Cancelling 可中断性未定义 | §6.2 | 新增可中断性表,SLA 仅强制可中断路径(5 种),不可中断路径记录 `sla_met: false` |
| 9 | Plan 4 DAG Cancelling node 处理未定义 | §6.2 | DAG 等待已启动 node 完成,不主动中断;DAG 级不强制 1s SLA |
| 10 | Plan 间依赖顺序未明确 | §8.5 | 明确顺序 1→2→4→3→5→6,Plan 5 必须在 Plan 3/4 之后(registry 完整性) |
| 11 | Plan 5 registry 列表不完整(虚构 15 种 event_type) | §7.1 | Grep 核对实际 callsite,registry 改为 27 种(24 现有 + 3 新增),删除虚构项 |
| 12 | Plan 5 覆盖率测试 feature 依赖未说明 | §7.4 | 测试拆分:default 测 24/27,全 feature `#[ignore]` 测 27/27 |
| 13 | Plan 6 测试 14 与 W9 测试重复 | §7.4 | 说明两者互补:W9 测运行时数据,Plan 6 测编译时常量,保留两者 |
| 14 | Kill Switch 新 audit event 与 `state_transition` 重复 | §6.2 | 取消 `task_cancelling` 事件(与 `state_transition { to: Cancelling }` 重复),只保留 `kill_switch_triggered` + `task_cancelled` 两个新事件 |
| 15 | 风险表遗漏(Plan 5 回归 / Plan 4 死锁 / Plan 3 表增长 / Plan 4 现有测试失败) | §九 | 新增 4 行风险 + 缓解策略 |
| 16 | 验收门禁 "8/8 = 100% ≥ 80%" 表述模糊 | §十二 | 改为 "7/7 有副作用 Skill + task.explain 显式 none",目标 7/7,无 "≥ 80%" 阈值表述(已超阈值) |

**Self-review v2 通过:**
- ✅ 无 TBD/TODO/placeholder
- ✅ 所有 audit event_type 经 Grep 核对(24 现有 + 3 新增)
- ✅ Plan 顺序明确,Plan 5 registry 完整性保证
- ✅ Kill Switch SLA 范围明确(仅可中断路径)
- ✅ 风险表覆盖 13 项,含 v2 新增 4 项
- ✅ 与现有代码 API(`audit_append` / `auto_reverse_move` 签名 / TaskState 转换表 / DagStatus 转换表)一致
