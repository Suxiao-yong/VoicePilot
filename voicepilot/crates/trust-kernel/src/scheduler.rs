//! 进程内后台作业调度器 —— OpenClaw Automations 的复现（计划 Phase C）。
//!
//! 语义对齐（调研在案）：
//! - jobs 持久化在 SQLite（agent_jobs），到期唤醒 planner（复用
//!   `route_text_with_dag`，零新编排代码），结果写回 agent_job_runs。
//! - **进程退出期间错过的不追补**（OpenClaw 同款边界：gateway 不在就不触发）；
//!   重启后从 next_run_at_ms 续跑。要 OS 级可靠性再上 schtasks（v1 不做，R3 砍 2）。
//! - 投递：结果仅入任务历史 + 通知回调（UI 层接桌面通知通道，R4 核 5）。
//!   不做 webhook/聊天频道投递（R3 砍 4）。
//! - 审批姿态（R2 finding 3）：调度触发的执行与交互式同走 trust 通道 ——
//!   DAG 内每步照走审批；调度器用 `AutoDenier`（用户不在屏 = 审批拒绝，
//!   fail-closed 既有语义），被拒节点使作业记为 failed/cancelled。
//!
//! cron 故意只做子集（R3 砍 3）：`every N <unit>` + 每日 `HH:mm`，
//! 覆盖 90% 口述场景，全 cron 语法是伪需求。

use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;

/// 调度 tick 间隔（秒级，到期精度 ±1s）。
pub const TICK_SECS: u64 = 1;
/// 单个作业执行历史保留上限（防膨胀，超限删最旧）。
const MAX_RUNS_PER_JOB: i64 = 50;

// ===== 纯函数层：schedule 解析 + next_run 计算 =====

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    /// 每 N 秒一次（分钟/小时/天归一到秒）。
    EveryN { period_secs: i64 },
    /// 每日 HH:mm（本地时区）。
    Daily { hh: u32, mm: u32 },
}

impl Schedule {
    pub fn as_spec(&self) -> String {
        match self {
            Schedule::EveryN { period_secs } => format!("every {period_secs}s"),
            Schedule::Daily { hh, mm } => format!("daily {hh:02}:{mm:02}"),
        }
    }
}

/// 解析 schedule 子集。接受：
/// - `every <N> <unit>`：unit ∈ s/sec/秒 m/min/分钟 h/hour/小时 d/day/天
///   （"every 30 分钟"、"every 5m"）
/// - `每 <N> <分钟|小时|天>`（"每30分钟"）
/// - `每天 HH:mm` / `daily HH:mm` / 裸 `HH:mm`
///
/// 其余（含全 cron 语法）→ None，由调用方显式报错。
pub fn parse_schedule(text: &str) -> Option<Schedule> {
    let t = text.trim();
    let t = t
        .strip_prefix("每")
        .filter(|rest| !rest.starts_with("天"))
        .map(|rest| format!("every {rest}"))
        .unwrap_or_else(|| t.to_string());
    let lower = t.to_lowercase();

    // every N unit
    if let Some(rest) = lower
        .strip_prefix("every ")
        .or_else(|| lower.strip_prefix("every"))
    {
        let rest = rest.trim();
        let (num_str, unit) = split_num_unit(rest)?;
        let n: i64 = num_str.parse().ok()?;
        if n <= 0 {
            return None;
        }
        let mult = unit_multiplier(&unit)?;
        // 溢出防护：n*mult*1000 必须仍在 i64 范围内，否则回绕成负值会让
        // next_run 落在过去、作业每秒都被重复领取执行。超界 → None（显式报错）。
        let period_secs = n.checked_mul(mult)?;
        // 溢出防护：n*mult*1000 必须仍在 i64 范围内，否则回绕成负值会让
        // next_run 落在过去、作业每秒都被重复领取执行。超界 → None（显式报错）。
        period_secs.checked_mul(1000)?;
        return Some(Schedule::EveryN { period_secs });
    }

    // 每天 HH:mm / daily HH:mm / 裸 HH:mm
    let hhmm = lower
        .strip_prefix("每天")
        .or_else(|| lower.strip_prefix("daily"))
        .unwrap_or(&lower)
        .trim();
    if let Some((hh, mm)) = parse_hhmm(hhmm) {
        if hh < 24 && mm < 60 {
            return Some(Schedule::Daily { hh, mm });
        }
    }
    None
}

/// "30 分钟" / "30min" → ("30", "分钟"). unit 必须完整匹配已知单位
/// （s/sec/m/min/h/hour/d/day 及中文 秒/分/分钟/小时/天）；复合单位如
/// "1d12h" 返回 None（调用方显式报错），绝不只取首字符静默丢弃。
fn split_num_unit(rest: &str) -> Option<(String, String)> {
    let rest = rest.trim();
    let idx = rest.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit) = rest.split_at(idx);
    if num.is_empty() {
        return None;
    }
    let unit = unit.trim().to_string();
    // unit 必须完整匹配已知单位（见 unit_multiplier）；复合单位如 "1d12h"
    // → None（调用方显式报错），绝不只取首字符静默丢弃。
    unit_multiplier(&unit)?;
    Some((num.to_string(), unit))
}

/// 已知单位的秒数倍率（精确匹配，不取前缀；输入已 lower，中文原样）。
fn unit_multiplier(unit: &str) -> Option<i64> {
    match unit {
        "s" | "sec" | "秒" => Some(1),
        "m" | "min" | "分" | "分钟" => Some(60),
        "h" | "hour" | "小时" => Some(3600),
        "d" | "day" | "天" => Some(86_400),
        _ => None,
    }
}

/// "08:00" / "8:00" / "8点00" → Some((8, 0))
fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    let s = s.replace("点", ":").replace("：", ":");
    let (h, m) = s.split_once(':')?;
    let hh: u32 = h.trim().parse().ok()?;
    let mm: u32 = if m.trim().is_empty() { 0 } else { m.trim().parse().ok()? };
    Some((hh, mm))
}

/// 从 `now_ms` 起算下一次触发时刻（epoch ms）。纯函数（tick 测试用 mock 时钟）。
pub fn next_run_after(schedule: &Schedule, now_ms: i64) -> i64 {
    match schedule {
        Schedule::EveryN { period_secs } => now_ms
            .checked_add(period_secs.checked_mul(1000).unwrap_or(i64::MAX))
            .unwrap_or(i64::MAX),
        Schedule::Daily { hh, mm } => {
            let now = Utc.timestamp_millis_opt(now_ms).single().unwrap_or_else(Utc::now);
            // 本地时区的今日/明日 HH:mm。chrono Local 在 Windows 无 tzdata 依赖
            // （用系统 API），足够本用途；全 UTC 会让"每天早上八点"跑在错误钟点。
            let local_now: DateTime<chrono::Local> = now.into();
            let (hh, mm) = (*hh, *mm);
            let today_target = local_now
                .date_naive()
                .and_hms_opt(hh, mm, 0)
                .and_then(|naive| chrono::Local.from_local_datetime(&naive).single());
            let target = match today_target {
                Some(t) if t > local_now => t,
                _ => {
                    let tomorrow = local_now.date_naive().succ_opt().and_then(|d| {
                        d.and_hms_opt(hh, mm, 0)
                    });
                    match tomorrow.and_then(|naive| {
                        chrono::Local.from_local_datetime(&naive).single()
                    }) {
                        Some(t) => t,
                        // DST 缺口等极端情况：退化为 +24h（ponytail：边界兜底）。
                        None => local_now + chrono::Duration::hours(24),
                    }
                }
            };
            target.with_timezone(&Utc).timestamp_millis()
        }
    }
}

// ===== DB 层 =====

#[derive(Debug, Clone)]
pub struct AgentJob {
    pub job_id: String,
    pub prompt: String,
    pub schedule: String,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub next_run_at_ms: Option<i64>,
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentJob> {
    Ok(AgentJob {
        job_id: row.get(0)?,
        prompt: row.get(1)?,
        schedule: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        created_at_ms: row.get(4)?,
        last_run_at_ms: row.get(5)?,
        next_run_at_ms: row.get(6)?,
    })
}

const JOB_COLUMNS: &str =
    "job_id, prompt, schedule, enabled, created_at_ms, last_run_at_ms, next_run_at_ms";

/// 建作业（task.schedule executor 用）。schedule 必须可解析，否则显式报错。
pub fn create_job(
    conn: &Connection,
    job_id: &str,
    prompt: &str,
    schedule: &str,
    now_ms: i64,
) -> Result<AgentJob> {
    let parsed = parse_schedule(schedule).ok_or_else(|| {
        KernelError::Skill(format!(
            "unsupported schedule '{schedule}' (subset: 'every N 分钟/小时/天' or '每天 HH:mm')"
        ))
    })?;
    let next = next_run_after(&parsed, now_ms);
    conn.execute(
        "INSERT INTO agent_jobs (job_id, prompt, schedule, enabled, created_at_ms, next_run_at_ms)
         VALUES (?1, ?2, ?3, 1, ?4, ?5)",
        params![job_id, prompt, schedule, now_ms, next],
    )?;
    Ok(AgentJob {
        job_id: job_id.to_string(),
        prompt: prompt.to_string(),
        schedule: schedule.to_string(),
        enabled: true,
        created_at_ms: now_ms,
        last_run_at_ms: None,
        next_run_at_ms: Some(next),
    })
}

pub fn list_jobs(conn: &Connection) -> Result<Vec<AgentJob>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {JOB_COLUMNS} FROM agent_jobs ORDER BY created_at_ms"))?;
    let rows = stmt.query_map([], row_to_job)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// 停用/启用（task.unschedule executor 用）。返回是否命中行。
pub fn set_job_enabled(conn: &Connection, job_id: &str, enabled: bool) -> Result<bool> {
    let n = conn.execute(
        "UPDATE agent_jobs SET enabled = ?2 WHERE job_id = ?1",
        params![job_id, enabled as i64],
    )?;
    Ok(n > 0)
}

pub fn get_job(conn: &Connection, job_id: &str) -> Result<Option<AgentJob>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {JOB_COLUMNS} FROM agent_jobs WHERE job_id = ?1"))?;
    stmt.query_row(params![job_id], row_to_job)
        .optional()
        .map_err(Into::into)
}

/// 领取到期作业：enabled 且 next_run_at_ms <= now。领取即预推进 next_run
/// （防同一作业被连续 tick 重复领取——claim-then-run 语义）。
pub fn claim_due_jobs(conn: &Connection, now_ms: i64) -> Result<Vec<AgentJob>> {
    let due: Vec<AgentJob> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM agent_jobs
             WHERE enabled = 1 AND next_run_at_ms IS NOT NULL AND next_run_at_ms <= ?1
             ORDER BY next_run_at_ms LIMIT 10"
        ))?;
        let rows = stmt.query_map(params![now_ms], row_to_job)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for job in &due {
        // 预推进：从原 next_run 起算下一周期（不用 now，避免慢执行造成漂移累积）。
        let base = job.next_run_at_ms.unwrap_or(now_ms);
        let next = next_run_after(
            &parse_schedule(&job.schedule).ok_or_else(|| {
                KernelError::Skill(format!("job '{}' has corrupt schedule", job.job_id))
            })?,
            base,
        );
        // next 已过期（进程退出漏跑）→ 直接跳到 now 起算（不追补，只续跑）。
        let next = if next <= now_ms {
            next_run_after(
                &parse_schedule(&job.schedule).ok_or_else(|| {
                    KernelError::Skill(format!("job '{}' has corrupt schedule", job.job_id))
                })?,
                now_ms,
            )
        } else {
            next
        };
        conn.execute(
            "UPDATE agent_jobs SET next_run_at_ms = ?2 WHERE job_id = ?1",
            params![job.job_id, next],
        )?;
    }
    Ok(due)
}

/// 记录一次执行 + 更新 last_run_at。返回 run_id。
pub fn record_run(
    conn: &Connection,
    job_id: &str,
    started_at_ms: i64,
    finished_at_ms: i64,
    outcome: &str,
    result: &str,
) -> Result<String> {
    let run_id = format!("run-{}", uuid::Uuid::new_v4());
    conn.execute(
        "INSERT INTO agent_job_runs (run_id, job_id, started_at_ms, finished_at_ms, outcome, result)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![run_id, job_id, started_at_ms, finished_at_ms, outcome, result],
    )?;
    conn.execute(
        "UPDATE agent_jobs SET last_run_at_ms = ?2 WHERE job_id = ?1",
        params![job_id, finished_at_ms],
    )?;
    // 历史膨胀防护：超出上限删最旧。
    conn.execute(
        "DELETE FROM agent_job_runs WHERE job_id = ?1 AND run_id NOT IN (
            SELECT run_id FROM agent_job_runs WHERE job_id = ?1
            ORDER BY started_at_ms DESC LIMIT ?2)",
        params![job_id, MAX_RUNS_PER_JOB],
    )?;
    Ok(run_id)
}

/// 某作业的执行历史（时间倒序；UI 列表用）。
pub fn list_runs(conn: &Connection, job_id: &str, limit: usize) -> Result<Vec<JobRun>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, job_id, started_at_ms, finished_at_ms, outcome, result
         FROM agent_job_runs WHERE job_id = ?1 ORDER BY started_at_ms DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![job_id, limit as i64], |row| {
        Ok(JobRun {
            run_id: row.get(0)?,
            job_id: row.get(1)?,
            started_at_ms: row.get(2)?,
            finished_at_ms: row.get(3)?,
            outcome: row.get(4)?,
            result: row.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[derive(Debug, Clone)]
pub struct JobRun {
    pub run_id: String,
    pub job_id: String,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub outcome: String,
    pub result: String,
}

// ===== 运行时层 =====

/// 作业完成通知回调（UI 层接桌面通知通道；测试用 no-op）。
pub type NotifyFn = Arc<dyn Fn(&AgentJob, &JobRun) + Send + Sync>;

/// 执行一个到期作业：route_text_with_dag → 按结果落历史。
///
/// - `Chat` → LLM 直接回答即结果（outcome=chat）。
/// - `DagPlan` → AutoDenier 执行（R2-3：用户不在屏审批默认 Deny，
///   fail-closed；被拒节点 → outcome=failed/cancelled）。
/// - `Routed` / `Unmatched` / `Empty` → 记录原样（需交互的作业转人工）。
async fn execute_job(kernel: &Arc<TrustKernel>, job: &AgentJob) -> JobRun {
    let started = chrono::Utc::now().timestamp_millis();
    let (outcome, result) = match crate::route_bridge::route_text_with_dag(kernel, &job.prompt).await
    {
        Ok(crate::route_bridge::RouteOutcome::Chat { text }) => ("chat".to_string(), text),
        #[cfg(feature = "llm")]
        Ok(crate::route_bridge::RouteOutcome::DagPlan(dag)) => {
            // 阻塞执行放 spawn_blocking，避免卡 tokio runtime。
            let kernel = kernel.clone();
            let res = tokio::task::spawn_blocking(move || {
                let approver: Arc<dyn crate::approval::approver::Approver> =
                    Arc::new(crate::approval::approver::AutoDenier);
                let executor = crate::skills::dag_executor::DagExecutor::new(
                    kernel,
                    approver,
                    Arc::new(crate::skills::dag_repo::DagRepo::new()),
                );
                executor.run(&dag, &[])
            })
            .await;
            match res {
                Ok(Ok(dag_result)) => {
                    let status = dag_result.status.as_str().to_string();
                    let summary = summarize_dag_result(&dag_result);
                    (status, summary)
                }
                Ok(Err(e)) => ("failed".to_string(), format!("dag execute error: {e}")),
                Err(e) => ("failed".to_string(), format!("spawn_blocking: {e}")),
            }
        }
        Ok(crate::route_bridge::RouteOutcome::Routed { skill_id }) => (
            "routed".to_string(),
            format!("命中技能 {skill_id}，需交互式执行（转人工确认卡）"),
        ),
        Ok(crate::route_bridge::RouteOutcome::Unmatched { text }) => {
            ("unmatched".to_string(), text)
        }
        Ok(crate::route_bridge::RouteOutcome::Empty) => {
            ("empty".to_string(), String::new())
        }
        Err(e) => ("failed".to_string(), e.to_string()),
    };
    let finished = chrono::Utc::now().timestamp_millis();
    JobRun {
        run_id: String::new(), // 由 record_run 填
        job_id: job.job_id.clone(),
        started_at_ms: started,
        finished_at_ms: finished,
        outcome,
        result: result.chars().take(2000).collect(),
    }
}

/// DAG 结果摘要（llm-gated 的 execute_job 使用；no-llm 下 dead-code 豁免）。
#[cfg_attr(not(feature = "llm"), allow(dead_code))]
fn summarize_dag_result(result: &crate::skills::dag_types::DagResult) -> String {
    use crate::skills::dag_types::DagNodeStatus;
    let mut ok = 0;
    let mut failed = 0;
    let mut first_err = String::new();
    for (node_id, status) in &result.node_results {
        match status {
            DagNodeStatus::Succeeded { .. } => ok += 1,
            DagNodeStatus::Failed { cause } => {
                failed += 1;
                if first_err.is_empty() {
                    first_err = format!("{node_id}: {cause}");
                }
            }
            _ => {}
        }
    }
    if failed > 0 {
        format!("{ok} 节点成功，{failed} 节点失败；首个失败：{first_err}")
    } else {
        format!("{ok} 节点全部成功")
    }
}

/// 单次 tick：领取并串行执行所有到期作业，写历史 + 通知。
/// 独立成函数供集成测试直接调用（mock 时钟 = 先改 next_run_at_ms 再调）。
pub async fn run_due_jobs_once(kernel: &Arc<TrustKernel>, notify: &NotifyFn) -> usize {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let due = {
        let conn = kernel.conn();
        claim_due_jobs(&conn, now_ms).unwrap_or_default()
    };
    for job in &due {
        let run = execute_job(kernel, job).await;
        let run = {
            let conn = kernel.conn();
            match record_run(
                &conn,
                &run.job_id,
                run.started_at_ms,
                run.finished_at_ms,
                &run.outcome,
                &run.result,
            ) {
                Ok(run_id) => JobRun { run_id, ..run },
                Err(e) => {
                    tracing::warn!(error = ?e, job_id = %job.job_id, "record_run failed");
                    continue;
                }
            }
        };
        notify(job, &run);
    }
    due.len()
}

/// 启动调度器后台任务（UI/CLI boot 时调用一次）。返回线程句柄。
///
/// 独立线程 + current-thread runtime（`crate::planner::block_on_planner` 同构）：
/// Tauri `setup` 等无 runtime 上下文处直接 `tokio::spawn` 会 panic
///（"there is no reactor running"），自带线程则处处可调。
pub fn spawn_scheduler(
    kernel: Arc<TrustKernel>,
    notify: NotifyFn,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("scheduler runtime");
        runtime.block_on(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(TICK_SECS));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let _ = run_due_jobs_once(&kernel, &notify).await;
            }
        })
    })
}

// ===== 内置技能：task.schedule / task.jobs / task.unschedule =====

pub fn task_schedule_manifest() -> crate::skills::manifest::SkillManifest {
    use crate::skills::manifest::*;
    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "prompt".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: Some(500),
            default: None,
        },
    );
    inputs.insert(
        "schedule".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: Some(60),
            default: None,
        },
    );
    SkillManifest {
        id: "task.schedule".to_string(),
        version: "1.0.0".to_string(),
        title: "创建定时任务".to_string(),
        description: "创建后台定时作业（task.schedule）：到点自动用当前输入执行 prompt。schedule 格式必须是 'every N 分钟/小时/天' 或 '每天 HH:mm'（如 '每天 08:00'、'every 30 分钟'）；其他时间说法先换算成这两种。prompt 是到点要执行的完整指令。".to_string(),
        description_body: None,
        execution: None,
        intent_examples: vec![
            "每天早上八点帮我总结新闻".to_string(),
            "每30分钟检查一次".to_string(),
        ],
        keywords: vec!["定时".to_string(), "每天".to_string(), "后台任务".to_string()],
        inputs,
        risk_ceiling: crate::policy::types::ELevel::E1,
        data_class_ceiling: crate::policy::types::DLevel::D2,
        egress: crate::skills::manifest::EgressKind::LocalOnly,
        max_steps: 1,
        tools: vec!["task.schedule".to_string()],
        approval: ApprovalConfig {
            mode: ApprovalMode::None,
            required_for: "commit".to_string(),
            show_effect_manifest: false,
            max_approval_scope: 0,
        },
        compensation: CompensationConfig {
            level: crate::compensation::types::CompensationLevel::None,
            ttl_seconds: 0,
            conflict_policy: crate::compensation::types::ConflictPolicy::AutoReverse,
        },
        verifier: VerifierConfig {
            strategy: "none".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}

pub fn task_jobs_manifest() -> crate::skills::manifest::SkillManifest {
    use crate::skills::manifest::*;
    SkillManifest {
        id: "task.jobs".to_string(),
        version: "1.0.0".to_string(),
        title: "列出定时任务".to_string(),
        description: "列出当前所有后台定时作业及下次运行时间（task.jobs）".to_string(),
        description_body: None,
        execution: None,
        intent_examples: vec!["我有哪些定时任务".to_string()],
        keywords: vec!["定时任务".to_string(), "有哪些任务".to_string()],
        inputs: std::collections::HashMap::new(),
        risk_ceiling: crate::policy::types::ELevel::E0,
        data_class_ceiling: crate::policy::types::DLevel::D1,
        egress: crate::skills::manifest::EgressKind::LocalOnly,
        max_steps: 1,
        tools: vec!["task.jobs".to_string()],
        approval: ApprovalConfig {
            mode: ApprovalMode::None,
            required_for: "commit".to_string(),
            show_effect_manifest: false,
            max_approval_scope: 0,
        },
        compensation: CompensationConfig {
            level: crate::compensation::types::CompensationLevel::None,
            ttl_seconds: 0,
            conflict_policy: crate::compensation::types::ConflictPolicy::AutoReverse,
        },
        verifier: VerifierConfig {
            strategy: "none".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}

pub fn task_unschedule_manifest() -> crate::skills::manifest::SkillManifest {
    use crate::skills::manifest::*;
    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "job_id".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: Some(64),
            default: None,
        },
    );
    SkillManifest {
        id: "task.unschedule".to_string(),
        version: "1.0.0".to_string(),
        title: "停用定时任务".to_string(),
        description: "按 job_id 停用一个后台定时作业（task.unschedule）".to_string(),
        description_body: None,
        execution: None,
        intent_examples: vec!["停掉那个定时任务".to_string()],
        keywords: vec!["停用".to_string(), "取消定时".to_string()],
        inputs,
        risk_ceiling: crate::policy::types::ELevel::E1,
        data_class_ceiling: crate::policy::types::DLevel::D2,
        egress: crate::skills::manifest::EgressKind::LocalOnly,
        max_steps: 1,
        tools: vec!["task.unschedule".to_string()],
        approval: ApprovalConfig {
            mode: ApprovalMode::None,
            required_for: "commit".to_string(),
            show_effect_manifest: false,
            max_approval_scope: 0,
        },
        compensation: CompensationConfig {
            level: crate::compensation::types::CompensationLevel::None,
            ttl_seconds: 0,
            conflict_policy: crate::compensation::types::ConflictPolicy::AutoReverse,
        },
        verifier: VerifierConfig {
            strategy: "none".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}

// ===== 内置技能 executor（dispatcher 调用）=====
// 签名：text_in 形态 = (kernel, approver, task_id, step_id, input)；
// text_out 形态 = (kernel, task_id, step_id)，与 dispatcher.rs 分发表一致。

pub fn execute_task_schedule(
    kernel: &TrustKernel,
    _approver: &dyn crate::approval::approver::Approver,
    _task_id: &str,
    _step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let prompt = inputs
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            inputs
                .get("prompt")
                .map(|v| v.to_string().trim_matches('"').to_string())
        })
        .ok_or_else(|| KernelError::Skill("task.schedule: missing field 'prompt'".to_string()))?;
    let schedule = inputs
        .get("schedule")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            inputs
                .get("schedule")
                .map(|v| v.to_string().trim_matches('"').to_string())
        })
        .ok_or_else(|| KernelError::Skill("task.schedule: missing field 'schedule'".to_string()))?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let job_id = format!("job-{}", uuid::Uuid::new_v4());
    let conn = kernel.conn();
    let job = create_job(&conn, &job_id, &prompt, &schedule, now_ms)?;
    Ok(format!(
        "已创建定时任务 {}（{}）：到点自动执行「{}」；下次运行 {}。",
        job.job_id,
        job.schedule,
        job.prompt,
        job.next_run_at_ms
            .map(|ms| chrono::DateTime::<Utc>::from_timestamp_millis(ms)
                .map(|d| d.with_timezone(&chrono::Local).to_rfc3339())
                .unwrap_or_else(|| "?".to_string()))
            .unwrap_or_else(|| "?".to_string())
    ))
}

pub fn execute_task_jobs(kernel: &TrustKernel, _task_id: &str, _step_id: &str) -> Result<String> {
    let conn = kernel.conn();
    let jobs = list_jobs(&conn)?;
    if jobs.is_empty() {
        return Ok("当前没有定时任务。".to_string());
    }
    let mut lines = vec![format!("共 {} 个定时任务：", jobs.len())];
    for j in jobs {
        lines.push(format!(
            "{} [{}] 「{}」 下次：{}",
            j.job_id,
            if j.enabled { "启用" } else { "停用" },
            j.prompt.chars().take(80).collect::<String>(),
            j.next_run_at_ms
                .map(|ms| chrono::DateTime::<Utc>::from_timestamp_millis(ms)
                    .map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "?".to_string()))
                .unwrap_or_else(|| "?".to_string()),
        ));
    }
    Ok(lines.join("\n"))
}

pub fn execute_task_unschedule(
    kernel: &TrustKernel,
    _approver: &dyn crate::approval::approver::Approver,
    _task_id: &str,
    _step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let job_id = inputs
        .get("job_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_matches('"').to_string())
        .ok_or_else(|| KernelError::Skill("task.unschedule: missing field 'job_id'".to_string()))?;
    let conn = kernel.conn();
    if set_job_enabled(&conn, &job_id, false)? {
        Ok(format!("已停用定时任务 {job_id}。"))
    } else {
        Err(KernelError::Skill(format!(
            "定时任务 {job_id} 不存在"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;
use chrono::Timelike;

    fn migrated() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn parse_schedule_subset() {
        assert_eq!(
            parse_schedule("every 30 分钟"),
            Some(Schedule::EveryN { period_secs: 1800 })
        );
        assert_eq!(
            parse_schedule("every 5m"),
            Some(Schedule::EveryN { period_secs: 300 })
        );
        assert_eq!(
            parse_schedule("每2小时"),
            Some(Schedule::EveryN { period_secs: 7200 })
        );
        assert_eq!(
            parse_schedule("每天 08:00"),
            Some(Schedule::Daily { hh: 8, mm: 0 })
        );
        assert_eq!(
            parse_schedule("daily 22:30"),
            Some(Schedule::Daily { hh: 22, mm: 30 })
        );
        assert_eq!(
            parse_schedule("08:00"),
            Some(Schedule::Daily { hh: 8, mm: 0 })
        );
        assert_eq!(
            parse_schedule("8点30"),
            Some(Schedule::Daily { hh: 8, mm: 30 })
        );
        // 全 cron 语法 / 乱写 → None（子集故意，R3 砍 3）。
        assert_eq!(parse_schedule("0 8 * * *"), None);
        assert_eq!(parse_schedule("每天早上八点"), None);
        assert_eq!(parse_schedule("every 0 min"), None);
        assert_eq!(parse_schedule(""), None);
        // 复合单位（1d12h）与超大周期（period_secs*1000 溢出回绕成负值）→ None
        //（显式报错，绝不只取首字符静默丢弃）。
        assert_eq!(parse_schedule("every 1d12h"), None);
        assert_eq!(parse_schedule("every 999999999999 d"), None);
        // 合法边界值不受影响：超大但 *1000 仍在 i64 范围内 → 正常解析。
        assert_eq!(
            parse_schedule("every 106000000000 d"),
            Some(Schedule::EveryN {
                period_secs: 9_158_400_000_000_000
            })
        );
    }

    #[test]
    fn next_run_every_n_is_now_plus_period() {
        let now = 1_700_000_000_000i64;
        assert_eq!(
            next_run_after(&Schedule::EveryN { period_secs: 300 }, now),
            now + 300_000
        );
    }

    #[test]
    fn next_run_every_n_never_wraps_negative_on_huge_period() {
        // period_secs*1000 超出 i64 → 饱和到 i64::MAX（永不回绕成负值，"
        // 防超大周期导致每秒重复领取执行"）。合法边界不受影响。
        let now = 1_000i64;
        let huge = next_run_after(
            &Schedule::EveryN {
                period_secs: i64::MAX / 1000 + 1,
            },
            now,
        );
        assert_eq!(huge, i64::MAX, "huge period must saturate, not wrap negative");
        // 合法边界：period*1000 仍在范围内 → 正常未来时刻。
        let ok = next_run_after(&Schedule::EveryN { period_secs: 60 }, now);
        assert_eq!(ok, now + 60_000);
    }

    #[test]
    fn next_run_daily_is_local_today_or_tomorrow() {
        // 真时钟：next 必须严格在未来、且落在目标钟点的 ±1 分钟内（DST 余量）。
        let now = chrono::Utc::now().timestamp_millis();
        let next = next_run_after(&Schedule::Daily { hh: 3, mm: 15 }, now);
        assert!(next > now, "next must be in the future");
        let next_local = chrono::DateTime::<Utc>::from_timestamp_millis(next)
            .unwrap()
            .with_timezone(&chrono::Local);
        let target_min = next_local.hour() * 60 + next_local.minute();
        let expected_min = 3 * 60 + 15;
        let diff = (target_min as i32 - expected_min as i32).abs();
        assert!(diff <= 1, "daily next should hit 03:15 local, got {next_local}");
        // 距离不超过 24h。
        assert!(next - now <= 25 * 3600_000);
    }

    #[test]
    fn claim_advances_next_run_and_marks_claimed() {
        let conn = migrated();
        let now = 1_000_000_000_000i64;
        let job = create_job(&conn, "j1", "总结", "every 1 小时", now).unwrap();
        assert_eq!(job.next_run_at_ms, Some(now + 3_600_000));
        // 到期领取 → next_run 预推进。
        let due = claim_due_jobs(&conn, now + 3_600_001).unwrap();
        assert_eq!(due.len(), 1);
        let j1 = get_job(&conn, "j1").unwrap().unwrap();
        assert!(j1.next_run_at_ms.unwrap() > now + 3_600_001, "claimed job advances");
        // 未到期不再领取（claim-then-run 防重复）。
        assert!(claim_due_jobs(&conn, now + 3_600_002).unwrap().is_empty());
        // 停用后不领取。
        set_job_enabled(&conn, "j1", false).unwrap();
        assert!(claim_due_jobs(&conn, now + 99 * 3_600_000).unwrap().is_empty());
    }

    #[test]
    fn missed_runs_catch_up_to_now_not_backfill() {
        let conn = migrated();
        let now = 1_000_000_000_000i64;
        create_job(&conn, "j1", "p", "every 1 分钟", now).unwrap();
        // 进程退了 10 分钟（漏 10 次不追补）→ 下一次从 now 起算。
        let due = claim_due_jobs(&conn, now + 600_000).unwrap();
        assert_eq!(due.len(), 1);
        let j1 = get_job(&conn, "j1").unwrap().unwrap();
        assert!(j1.next_run_at_ms.unwrap() > now + 600_000, "resume from now, no backfill");
    }

    #[test]
    fn run_history_persist_and_prune() {
        let conn = migrated();
        create_job(&conn, "j1", "p", "every 1 分钟", 1_000).unwrap();
        for i in 0..60 {
            record_run(&conn, "j1", i, i + 1, "chat", &format!("r{i}")).unwrap();
        }
        let runs = list_runs(&conn, "j1", 100).unwrap();
        assert_eq!(runs.len(), MAX_RUNS_PER_JOB as usize, "pruned to cap");
        assert_eq!(runs[0].result, "r59", "newest first");
        let j1 = get_job(&conn, "j1").unwrap().unwrap();
        assert_eq!(j1.last_run_at_ms, Some(60));
    }

    #[tokio::test]
    async fn run_due_jobs_once_executes_chat_job_end_to_end() {
        // 无 LLM 客户端 → route 收敛到 Unmatched → 作业历史记录 unmatched。
        // 本测试验证 tick 唤醒 + 历史落库链路，不依赖 LLM（DAG 执行链路由
        // 既有 w8 e2e 锁定）。
        let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
        {
            let conn = kernel.conn();
            create_job(&conn, "j1", "总结一下", "every 1 分钟", 1_000).unwrap();
        }
        let notified = Arc::new(std::sync::Mutex::new(0usize));
        let notified_c = notified.clone();
        let notify: NotifyFn = Arc::new(move |_job, _run| {
            *notified_c.lock().unwrap() += 1;
        });
        let ran = run_due_jobs_once(&kernel, &notify).await;
        assert_eq!(ran, 1);
        assert_eq!(*notified.lock().unwrap(), 1);
        let runs = {
            let conn = kernel.conn();
            list_runs(&conn, "j1", 10).unwrap()
        };
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].outcome, "unmatched");
    }

    #[test]
    fn spawn_scheduler_from_bare_thread_does_not_panic() {
        // 回归：Tauri setup（无 runtime 上下文）调 spawn_scheduler 曾 panic
        // "there is no reactor running"。裸线程复刻该上下文（无 tokio）。
        // 调度器线程与进程同寿，此处只验证 spawn 不 panic 即返回。
        let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
        let notify: NotifyFn = Arc::new(|_, _| {});
        let spawned = std::thread::spawn(move || spawn_scheduler(kernel, notify))
            .join()
            .expect("spawn_scheduler must not panic on a bare thread");
        let _ = spawned;
    }
}
