//! Task 3 —— Axum HTTP/SSE 网关微服务（`voicepilot serve`，契约 §3）。
//!
//! 五个端点（契约 §3.2）：
//! - `POST /api/v1/route`          文本 → 规划结果（kind / slots / route_kind / latency）
//! - `POST /api/v1/execute`        Skill 执行（幂等键 + prepare_token TOCTOU 预检）
//! - `POST /api/v1/compensate`     task.compensate 补偿回滚
//! - `GET  /api/v1/events`         SSE 事件流（broadcast 扇出 + 15s 心跳）
//! - `GET  /api/v1/observability`  延迟 / 缓存 / DAG token / 审计链完整性
//!
//! 执行语义（契约 §3.3）：
//! - **幂等**：进程内 `HashMap<idempotency_key, ExecuteResponse>`，重复 key 返回首次
//!   结果并置 `idempotent_replay=true`，不二次执行（成功与执行期失败都入表）。
//! - **TOCTOU**：`prepare_token` 走 `TransactionManager::validate`（只读预检，不消费
//!   token），不存在 / 过期 / manifest 被篡改 → 409，不执行。
//! - **审计链**：可观测端点复用已实现的 `kernel.verify_audit_chain()`（勿重写）。
//!
//! 锁纪律：`kernel.conn()` 返回的 `MutexGuard` 非 `Send`，任何 handler 都不得跨
//! `.await` 持有 —— 本模块所有 DB 段一律用 block scope 包住。

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, anyhow};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::trace::TraceLayer;

// execute 走 AutoDenier（无人类审批者 → fail-closed）；compensate 是用户显式
// 发起的撤销请求，语义上已有人工确认，走 AutoApprover。
use trust_kernel::approval::approver::{AutoApprover, AutoDenier};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::types::ExtractedSlot;
use trust_kernel::planner::{PlanResult, PlannerInput, PlannerPipeline, PlannerSource};
use trust_kernel::policy::transaction::EffectManifest;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;
use trust_kernel::skills::task_compensate::{TaskCompensateInput, execute_compensate};

/// SSE 广播通道容量：慢订阅者超过此深度只丢帧（`Lagged`），不阻塞生产者。
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// SSE 心跳间隔（契约 §3.3：15s 注释帧，防中间代理掐断空闲连接）。
const KEEP_ALIVE_SECS: u64 = 15;

/// 可观测端点的默认统计窗口：最近 24 小时（与契约 §2.3 DTO 默认值一致）。
pub const DEFAULT_WINDOW_HOURS: u32 = 24;

// ===== 子命令参数 =====

/// `voicepilot serve` 子命令参数（契约 §3.1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeArgs {
    /// 监听地址（默认 127.0.0.1）。
    pub host: String,
    /// 监听端口（默认 8080；0 = 由内核分配，benchmark 从启动行解析）。
    pub port: u16,
    /// SQLite 库路径（`--db` 覆盖 `VOICEPILOT_DB`，再缺省 `voicepilot.db`）。
    pub db: String,
}

/// 解析 `voicepilot serve` 之后的参数（`[--host H] [--port P] [--db D]`）。
pub fn parse_args(args: &[String]) -> Result<ServeArgs> {
    let mut parsed = ServeArgs {
        host: "127.0.0.1".to_string(),
        port: 8080,
        db: std::env::var("VOICEPILOT_DB").unwrap_or_else(|_| "voicepilot.db".to_string()),
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--host" => {
                parsed.host = value_of(args, i, "--host")?.to_string();
                i += 2;
            }
            "--port" => {
                let raw = value_of(args, i, "--port")?;
                parsed.port = raw
                    .parse::<u16>()
                    .map_err(|e| anyhow!("invalid --port {:?}: {e} (expected 0..=65535)", raw))?;
                i += 2;
            }
            "--db" => {
                parsed.db = value_of(args, i, "--db")?.to_string();
                i += 2;
            }
            other => {
                return Err(anyhow!(
                    "unknown serve argument {:?}; usage: voicepilot serve [--host 127.0.0.1] \
                     [--port 8080] [--db voicepilot.db]",
                    other
                ));
            }
        }
    }
    Ok(parsed)
}

/// 取 `flag` 后面的值（缺值直接报错，不做默认值兜底）。
fn value_of<'a>(args: &'a [String], i: usize, flag: &str) -> Result<&'a str> {
    args.get(i + 1)
        .map(|s| s.as_str())
        .ok_or_else(|| anyhow!("{flag} requires a value"))
}

/// 启动 HTTP/SSE 网关（阻塞直到监听关闭）。
///
/// 启动后打印 `voicepilot serve listening on http://{addr}`（benchmark 解析这一行
/// 拿真实端口，格式不要改）。
pub async fn run(args: ServeArgs) -> Result<()> {
    let kernel = TrustKernel::open_file(&args.db)
        .map_err(|e| anyhow!("failed to open kernel at {}: {}", args.db, e))?;
    configure_llm_from_env(&kernel);
    let app = router(AppState::new(kernel));
    let bind = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| anyhow!("failed to bind {bind}: {e}"))?;
    // 用内核分配的地址回显：`--port 0` 时 benchmark 也能拿到真实端口。
    let addr = listener
        .local_addr()
        .map_err(|e| anyhow!("failed to read local addr: {e}"))?;
    println!("voicepilot serve listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow!("http server error: {e}"))?;
    Ok(())
}

// ===== 共享状态 =====

/// 从环境变量注入 LLM 客户端（契约 §3.1）。
///
/// 三个变量必须同时存在才启用：`VOICEPILOT_LLM_BASE_URL` /
/// `VOICEPILOT_LLM_API_KEY` / `VOICEPILOT_LLM_MODEL`。缺任一则保持无 LLM
/// （路由退化为快路由 + 关键词，与桌面端未配置 key 时一致）。
#[cfg(feature = "llm")]
fn configure_llm_from_env(kernel: &TrustKernel) {
    let (Ok(base_url), Ok(api_key), Ok(model)) = (
        std::env::var("VOICEPILOT_LLM_BASE_URL"),
        std::env::var("VOICEPILOT_LLM_API_KEY"),
        std::env::var("VOICEPILOT_LLM_MODEL"),
    ) else {
        return;
    };
    if base_url.is_empty() || api_key.is_empty() || model.is_empty() {
        return;
    }
    kernel.set_llm_client(Some(std::sync::Arc::new(
        trust_kernel::llm::client::LlmClient::new(&base_url, &api_key, &model),
    )));
}

/// 无 llm feature：路由没有 LLM 能力，环境变量不生效。
#[cfg(not(feature = "llm"))]
fn configure_llm_from_env(_kernel: &TrustKernel) {}

/// 网关进程内共享状态（`Clone` 廉价：内核与幂等表都是 `Arc`）。
#[derive(Clone)]
pub struct AppState {
    /// 共享 TrustKernel（DB 连接 / 审计 / 扩展快照）。
    pub kernel: Arc<TrustKernel>,
    /// SSE 事件扇出（无订阅者时 `send` 返回 Err，忽略即可）。
    pub events: broadcast::Sender<ServerEvent>,
    /// 幂等键 → 首次执行结果（进程内，重启即失效）。
    pub idempotency: Arc<Mutex<HashMap<String, ExecuteResponse>>>,
    /// 正在执行中的幂等键（占坑表）。
    ///
    /// check-then-execute 不是原子的：两个并发请求同 key 时，若只查结果表
    /// 两边都会 miss 并各执行一次。先占坑再执行，后来者直接 409。
    pub in_flight: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl AppState {
    /// 用已打开的 kernel 构造状态（测试与 `run` 共用）。
    pub fn new(kernel: TrustKernel) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            kernel: Arc::new(kernel),
            events,
            idempotency: Arc::new(Mutex::new(HashMap::new())),
            in_flight: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }
}

/// 构造 HTTP 路由（测试用 `oneshot` 直接驱动，无需真实端口）。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/route", post(route_handler))
        .route("/api/v1/execute", post(execute_handler))
        .route("/api/v1/compensate", post(compensate_handler))
        .route("/api/v1/events", get(events_handler))
        .route("/api/v1/observability", get(observability_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ===== SSE 事件 =====

/// 网关推送给 SSE 订阅者的一条事件（契约 §3.3）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEvent {
    /// 事件唯一 id（uuid v4，便于前端去重）。
    pub id: String,
    /// 事件类型：`task_state | approval_required | audit | route | execute | compensate`。
    pub event_type: String,
    /// 关联任务 id（`route` 事件无任务时为 null）。
    pub task_id: Option<String>,
    /// 事件负载（各 event_type 自定形状）。
    pub payload: serde_json::Value,
    /// 事件时间（RFC3339）。
    pub timestamp: String,
}

impl ServerEvent {
    /// 构造一条事件（`task_id=None` → JSON null）。
    pub fn new(event_type: &str, task_id: Option<&str>, payload: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.to_string(),
            task_id: task_id.map(|s| s.to_string()),
            payload,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// 广播一条事件（无订阅者 / 已关闭时静默忽略，绝不因推送失败影响主流程）。
fn publish(state: &AppState, event: ServerEvent) {
    if state.events.send(event).is_err() {
        tracing::debug!("no active SSE subscriber; event dropped");
    }
}

/// broadcast 接收端 → SSE 帧流。
///
/// `Err(Lagged(n))` 只发一帧 `event: lagged` 后**继续**订阅（契约要求滞后不得
/// 终止服务）；仅通道关闭（`Err(Closed)`，由 `BroadcastStream` 直接结束流）才收尾。
pub fn sse_event_stream(
    rx: broadcast::Receiver<ServerEvent>,
) -> impl tokio_stream::Stream<Item = Result<Event, Infallible>> {
    BroadcastStream::new(rx).map(|item| {
        let frame = match item {
            Ok(event) => Event::default().data(serde_json::to_string(&event).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "sse event serialization failed");
                "{}".to_string()
            })),
            Err(err) => {
                tracing::warn!(error = %err, "sse subscriber lagged; skipping dropped events");
                Event::default()
                    .event("lagged")
                    .data(serde_json::json!({ "skipped": lagged_count(&err) }).to_string())
            }
        };
        Ok(frame)
    })
}

/// `BroadcastStreamRecvError::Lagged(n)` 的 n（跳过的事件数）。
fn lagged_count(err: &tokio_stream::wrappers::errors::BroadcastStreamRecvError) -> u64 {
    match err {
        tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n) => *n,
    }
}

// ===== 错误响应 =====

/// HTTP 错误：统一 `{"error": "..."}` 形状（契约 §3.3 的 409 即此形状）。
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    /// 409：prepare_token 校验失败（不存在 / 过期 / manifest 篡改）。
    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    /// 500：服务端错误（规划失败 / 序列化失败等）。
    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

// ===== POST /api/v1/route =====

/// `/api/v1/route` 请求体。
#[derive(Debug, Deserialize)]
pub struct RouteRequest {
    /// 待路由的原始文本（空 / 纯空白 → `kind=empty`）。
    pub text: String,
}

/// `/api/v1/route` 响应体（契约 §3.2，字段名冻结）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResponse {
    /// `skill | dag | chat | unmatched | empty`（由 `PlanResult` 映射）。
    pub kind: String,
    /// 命中的 Skill id（仅 `kind=skill`）。
    pub skill_id: Option<String>,
    /// 快路由 / LLM 提取的槽位。
    pub slots: Vec<ExtractedSlot>,
    /// `kind=dag` 时的 DAG 计划。
    pub dag: Option<serde_json::Value>,
    /// `kind=chat|unmatched` 时的文本。
    pub text: Option<String>,
    /// `fastroute | keyword | llm | cache`（`PlannerTrace.route`）。
    pub route_kind: String,
    /// 本次规划的 wall-clock 耗时（毫秒，含 LLM 等待）。
    pub latency_ms: i64,
    /// 本次规划是否调用过 LLM。
    pub used_llm: bool,
}

async fn route_handler(
    State(state): State<AppState>,
    Json(req): Json<RouteRequest>,
) -> Result<Json<RouteResponse>, ApiError> {
    let kernel = state.kernel.clone();
    let pipeline = PlannerPipeline::new(kernel.clone(), kernel.extension_snapshot());
    // started_at_ms 取 plan 开始时刻（时间窗口过滤口径），耗时用单调时钟。
    let started_at_ms = chrono::Utc::now().timestamp_millis();
    let started = std::time::Instant::now();
    let (plan, trace) = pipeline
        .plan(PlannerInput {
            text: req.text,
            source: PlannerSource::Text,
            snapshot: None,
        })
        .await
        .map_err(|e| ApiError::internal(format!("plan failed: {e}")))?;
    let latency_ms = started.elapsed().as_millis() as i64;

    // 契约 §2.2 + 勘误 #11：路由采样直接 INSERT（不建占位 task、不写审计），
    // 与 route_bridge 同口径 —— 让 /observability 与 Task 4 的 SQLite 直读一致。
    {
        let conn = kernel.conn();
        if let Err(e) = trust_kernel::voice_latency::record_route_sample(
            &conn,
            started_at_ms,
            latency_ms,
            trace.route.as_str(),
            trace.cache_hit,
        ) {
            tracing::warn!(error = %e, "route latency sample insert failed; continuing");
        }
    }

    let response = match plan {
        PlanResult::Empty => RouteResponse {
            kind: "empty".to_string(),
            skill_id: None,
            slots: Vec::new(),
            dag: None,
            text: None,
            route_kind: trace.route.as_str().to_string(),
            latency_ms,
            used_llm: trace.used_llm,
        },
        PlanResult::Skill {
            extension_id,
            slots,
        } => RouteResponse {
            kind: "skill".to_string(),
            skill_id: Some(extension_id),
            slots,
            dag: None,
            text: None,
            route_kind: trace.route.as_str().to_string(),
            latency_ms,
            used_llm: trace.used_llm,
        },
        PlanResult::Dag(dag) => {
            let dag = serde_json::to_value(&dag)
                .map_err(|e| ApiError::internal(format!("dag serialize failed: {e}")))?;
            RouteResponse {
                kind: "dag".to_string(),
                skill_id: None,
                slots: Vec::new(),
                dag: Some(dag),
                text: None,
                route_kind: trace.route.as_str().to_string(),
                latency_ms,
                used_llm: trace.used_llm,
            }
        }
        PlanResult::Chat { text } => RouteResponse {
            kind: "chat".to_string(),
            skill_id: None,
            slots: Vec::new(),
            dag: None,
            text: Some(text),
            route_kind: trace.route.as_str().to_string(),
            latency_ms,
            used_llm: trace.used_llm,
        },
        PlanResult::Unmatched { text } => RouteResponse {
            kind: "unmatched".to_string(),
            skill_id: None,
            slots: Vec::new(),
            dag: None,
            text: Some(text),
            route_kind: trace.route.as_str().to_string(),
            latency_ms,
            used_llm: trace.used_llm,
        },
    };

    publish(
        &state,
        ServerEvent::new(
            "route",
            None,
            serde_json::to_value(&response).unwrap_or(serde_json::Value::Null),
        ),
    );
    Ok(Json(response))
}

// ===== POST /api/v1/execute =====

/// `/api/v1/execute` 请求体（契约 §3.2）。
#[derive(Debug, Deserialize)]
pub struct ExecuteRequest {
    /// 任务 id（执行器自行创建 task/step 行，调用方须给未使用过的 id）。
    pub task_id: String,
    /// 步骤 id（同上）。
    pub step_id: String,
    /// 要执行的 Skill id（须在扩展快照中可执行）。
    pub skill_id: String,
    /// 槽位输入（直接作为 `dispatch_skill_executor` 的 resolved_input）。
    #[serde(default)]
    pub slots: serde_json::Value,
    /// 幂等键：重复提交同一个 key 不二次执行。
    pub idempotency_key: String,
    /// prepare_token（TOCTOU 预检用；给了就必须同时给 `manifest`）。
    #[serde(default)]
    pub prepare_token: Option<String>,
    /// prepare 阶段冻结的 effect manifest。
    #[serde(default)]
    pub manifest: Option<EffectManifest>,
}

/// `/api/v1/execute` 响应体（契约 §3.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    /// 执行是否成功提交（`DispatchOutcome.succeeded`）。
    pub committed: bool,
    /// 人类可读摘要（executor 输出；失败时为空）。
    pub summary: String,
    /// 失败原因（成功时为 null）。
    pub error: Option<String>,
    /// 是否为幂等重放（true = 本次未真正执行）。
    pub idempotent_replay: bool,
}

async fn execute_handler(
    State(state): State<AppState>,
    Json(req): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, ApiError> {
    // 1. 幂等：命中即返回首次结果（不二次执行）。
    // 锁只在 block 内持有，不跨 await（MutexGuard 非 Send）。
    let cached = {
        let table = state.idempotency.lock().unwrap();
        table.get(&req.idempotency_key).cloned()
    };
    if let Some(first) = cached {
        let mut replay = first;
        replay.idempotent_replay = true;
        return Ok(Json(replay));
    }

    // 2. prepare_token 只读预检（TOCTOU）：任一失败 → 409，不执行、不入幂等表
    //    （重新 prepare 后应当可以重试同一个幂等键）。
    if let Some(token) = req.prepare_token.as_deref() {
        let manifest = req
            .manifest
            .as_ref()
            .ok_or_else(|| ApiError::conflict("prepare_token requires manifest (contract §3.3)"))?;
        state
            .kernel
            .transaction_manager()
            .validate(token, manifest)
            .map_err(|e| ApiError::conflict(e.to_string()))?;
    }

    // 3. 占坑：同一 key 的并发请求只有一个能执行，其余 409（契约 §3.3 幂等语义）。
    {
        let mut claimed = state.in_flight.lock().unwrap();
        if !claimed.insert(req.idempotency_key.clone()) {
            return Err(ApiError::conflict(format!(
                "idempotency_key '{}' is already executing",
                req.idempotency_key
            )));
        }
    }

    // 4. 执行（成功与执行期失败都入幂等表）。
    let response = run_execute(&state.kernel, &req);
    {
        let mut table = state.idempotency.lock().unwrap();
        table.insert(req.idempotency_key.clone(), response.clone());
        let mut claimed = state.in_flight.lock().unwrap();
        claimed.remove(&req.idempotency_key);
    }

    publish(
        &state,
        ServerEvent::new(
            "execute",
            Some(&req.task_id),
            serde_json::to_value(&response).unwrap_or(serde_json::Value::Null),
        ),
    );
    Ok(Json(response))
}

/// 真正调用 dispatcher（同步：executor 全部是同步阻塞实现）。
///
/// **审批策略（安全关键）**：HTTP 网关没有人类审批者，所以：
///   1. `approval_required` 白名单内的技能直接 fail-closed（不执行）；
///   2. 其余技能用 `AutoDenier` —— 任何 executor 内部调 `approver.prompt`
///      （如 files.organize 的 prepare→commit 门）都会拿到 Deny。
///
/// 需要人工确认的能力只能在桌面端（`TauriApprover`）走，这是有意的能力边界。
fn run_execute(kernel: &TrustKernel, req: &ExecuteRequest) -> ExecuteResponse {
    if trust_kernel::skills::simple::approval_required(&req.skill_id) {
        return ExecuteResponse {
            committed: false,
            summary: String::new(),
            error: Some(format!(
                "skill '{}' requires interactive approval; the HTTP gateway has no approver (fail-closed)",
                req.skill_id
            )),
            idempotent_replay: false,
        };
    }
    match dispatch_skill_executor(
        &req.skill_id,
        kernel,
        &req.slots,
        &AutoDenier,
        &req.task_id,
        &req.step_id,
    ) {
        Ok(outcome) => ExecuteResponse {
            committed: outcome.succeeded,
            summary: summarize_output(&outcome.output),
            error: outcome.error_cause.clone(),
            idempotent_replay: false,
        },
        Err(e) => ExecuteResponse {
            committed: false,
            summary: String::new(),
            error: Some(e.to_string()),
            idempotent_replay: false,
        },
    }
}

/// 把 executor 的 JSON output 压成一行摘要（优先 `result` 文本）。
fn summarize_output(output: &serde_json::Value) -> String {
    if let Some(text) = output.get("result").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = output.as_str() {
        return text.to_string();
    }
    output.to_string()
}

// ===== POST /api/v1/compensate =====

/// `/api/v1/compensate` 请求体（契约 §3.2）。
#[derive(Debug, Deserialize)]
pub struct CompensateRequest {
    /// 新任务 id（`execute_compensate` 内部会创建该 task 行）。
    pub task_id: String,
    /// 新步骤 id。
    pub step_id: String,
    /// 要回滚的目标步骤 id（须有 active 补偿记录）。
    pub target_step_id: String,
}

/// `/api/v1/compensate` 响应体（契约 §3.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensateResponse {
    /// 是否已回滚成功。
    pub compensated: bool,
    /// 补偿记录状态：成功恒为 `reversed`（勘误 #6）；失败为 `failed`（未落库）。
    pub compensation_status: String,
    /// 人类可读摘要。
    pub summary: String,
    /// 失败原因（成功时为 null）。
    pub error: Option<String>,
}

async fn compensate_handler(
    State(state): State<AppState>,
    Json(req): Json<CompensateRequest>,
) -> Json<CompensateResponse> {
    let input = TaskCompensateInput {
        task_id: req.task_id.clone(),
        step_id: req.step_id.clone(),
        target_step_id: req.target_step_id.clone(),
    };
    let response = match execute_compensate(&state.kernel, &input, &AutoApprover) {
        Ok(returned_task_id) => CompensateResponse {
            compensated: true,
            // 勘误 #6：成功标记是 "reversed"（不是 rolled_back）。
            compensation_status: "reversed".to_string(),
            summary: format!(
                "compensation for step {} committed as task {}",
                req.target_step_id, returned_task_id
            ),
            error: None,
        },
        Err(e) => CompensateResponse {
            compensated: false,
            compensation_status: "failed".to_string(),
            summary: String::new(),
            error: Some(e.to_string()),
        },
    };
    publish(
        &state,
        ServerEvent::new(
            "compensate",
            Some(&req.task_id),
            serde_json::to_value(&response).unwrap_or(serde_json::Value::Null),
        ),
    );
    Json(response)
}

// ===== GET /api/v1/events =====

async fn events_handler(State(state): State<AppState>) -> impl IntoResponse {
    let rx = state.events.subscribe();
    Sse::new(sse_event_stream(rx)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(KEEP_ALIVE_SECS))
            .text("keep-alive"),
    )
}

// ===== GET /api/v1/observability =====

/// `/api/v1/observability` 查询参数。
#[derive(Debug, Deserialize)]
pub struct ObservabilityQuery {
    /// 统计窗口（小时）；缺省 24。
    #[serde(default)]
    pub window_hours: Option<u32>,
}

/// 单路由通道的延迟分位（与契约 §2.3 `RouteLatencyBucket` 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteLatencyBucket {
    /// `fastroute | keyword | llm | cache | voice`
    pub route_kind: String,
    /// 窗口内样本数。
    pub sample_count: u64,
    /// P50（毫秒）。
    pub p50_ms: i64,
    /// P95（毫秒）。
    pub p95_ms: i64,
    /// P99（毫秒）。
    pub p99_ms: i64,
    /// 最大延迟（毫秒）。
    pub max_ms: i64,
}

/// route cache 命中率（与契约 §2.3 `CacheStats` 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    /// 窗口内 `route_kind='cache'` 样本数。
    pub hits: u64,
    /// 窗口内 `route_kind='llm'` 样本数（走 LLM = 未命中）。
    pub misses: u64,
    /// `hits / (hits + misses)`；无样本为 0.0。
    pub hit_rate: f64,
}

/// DAG 拆解 token 消耗（与契约 §2.3 `DagTokenStats` 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagTokenStats {
    /// 窗口内 `llm_decompose_called` 次数。
    pub plans: u64,
    /// token 总数。
    pub total_tokens: u64,
    /// 平均 token 数。
    pub avg_tokens: f64,
}

/// 审计哈希链完整性（与契约 §2.3 `AuditChainStatus` 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditChainStatus {
    /// 链是否完整。
    pub valid: bool,
    /// 校验条目数。
    pub checked: usize,
    /// 首个断点（`None` = 完整）。
    pub broken_at: Option<String>,
}

/// `/api/v1/observability` 响应体（= 契约 §2.3 `ObservabilitySnapshot` 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityResponse {
    /// 生效的统计窗口（小时）。
    pub window_hours: u32,
    /// 分通道延迟分位。
    pub routes: Vec<RouteLatencyBucket>,
    /// 缓存命中率。
    pub cache: CacheStats,
    /// DAG token 统计。
    pub dag_tokens: DagTokenStats,
    /// 审计链完整性。
    pub audit_chain: AuditChainStatus,
}

async fn observability_handler(
    State(state): State<AppState>,
    Query(query): Query<ObservabilityQuery>,
) -> Result<Json<ObservabilityResponse>, ApiError> {
    let hours = query.window_hours.unwrap_or(DEFAULT_WINDOW_HOURS);
    let now_ms = chrono::Utc::now().timestamp_millis();
    let since_ms = now_ms - (hours as i64) * 3_600_000;
    let since_rfc3339 = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(since_ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();

    // DB 段整体 block scope：verify_audit_chain 内部会再取同一把 conn Mutex。
    let (routes, plans, total_tokens) = {
        let conn = state.kernel.conn();
        let routes: Vec<RouteLatencyBucket> =
            trust_kernel::voice_latency::compute_route_stats(&conn, Some(since_ms))
                .map_err(|e| ApiError::internal(format!("route stats failed: {e}")))?
                .into_iter()
                .map(|s| RouteLatencyBucket {
                    route_kind: s.route_kind,
                    sample_count: s.sample_count,
                    p50_ms: s.p50_ms,
                    p95_ms: s.p95_ms,
                    p99_ms: s.p99_ms,
                    max_ms: s.max_ms,
                })
                .collect();
        // dag_tokens：聚合 llm_decompose_called 的 details.token_count
        // （timestamp 为 RFC3339 文本，同格式下字典序即时间序）。
        let (plans, total_tokens): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CAST(json_extract(details, '$.token_count') AS INTEGER)), 0)
                 FROM audit_logs
                 WHERE event_type = 'llm_decompose_called' AND timestamp >= ?1",
                [since_rfc3339.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| ApiError::internal(format!("dag token stats failed: {e}")))?;
        (routes, plans.max(0) as u64, total_tokens.max(0) as u64)
    };

    let count_of = |kind: &str| -> u64 {
        routes
            .iter()
            .find(|r| r.route_kind == kind)
            .map(|r| r.sample_count)
            .unwrap_or(0)
    };
    let hits = count_of("cache");
    let misses = count_of("llm");
    let cache_total = hits + misses;
    let cache = CacheStats {
        hits,
        misses,
        hit_rate: if cache_total == 0 {
            0.0
        } else {
            hits as f64 / cache_total as f64
        },
    };
    let dag_tokens = DagTokenStats {
        plans,
        total_tokens,
        avg_tokens: if plans == 0 {
            0.0
        } else {
            total_tokens as f64 / plans as f64
        },
    };

    let chain = state
        .kernel
        .verify_audit_chain()
        .map_err(|e| ApiError::internal(format!("audit chain verify failed: {e}")))?;

    Ok(Json(ObservabilityResponse {
        window_hours: hours,
        routes,
        cache,
        dag_tokens,
        audit_chain: AuditChainStatus {
            valid: chain.valid,
            checked: chain.checked,
            broken_at: chain.broken_at,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(n: usize) -> ServerEvent {
        ServerEvent::new("audit", Some("t-1"), serde_json::json!({ "n": n }))
    }

    /// 慢订阅者滞后（`Lagged`）只发一帧提示并继续订阅 —— 不得终止流。
    #[tokio::test]
    async fn sse_stream_survives_lagged_receiver() {
        let (tx, rx) = broadcast::channel(4);
        let mut stream = sse_event_stream(rx);

        // 远超容量：接收端必然 Lagged。
        for n in 0..10 {
            tx.send(sample_event(n)).unwrap();
        }

        let first = stream.next().await.expect("stream must not end").unwrap();
        let debug = format!("{first:?}");
        assert!(
            debug.contains("lagged"),
            "first frame should be the lag notice, got: {debug}"
        );
        assert!(
            debug.contains("skipped"),
            "lag notice must carry the skipped count, got: {debug}"
        );

        // 滞后之后仍能继续产出数据帧（证明 lag 不杀流）。
        let next = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("stream stalled after lag")
            .expect("stream ended after lag")
            .unwrap();
        assert!(
            format!("{next:?}").contains("data"),
            "post-lag frame should carry data"
        );

        // 通道关闭 → 排空剩余缓冲后流自然结束（不会无限产出）。
        drop(tx);
        let mut ended = false;
        for _ in 0..16 {
            match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                Ok(None) => {
                    ended = true;
                    break;
                }
                Ok(Some(_)) => continue,
                Err(_) => panic!("stream stalled after the channel closed"),
            }
        }
        assert!(ended, "stream should end once the channel closed");
    }

    /// 正常事件按 JSON 数据帧推送。
    #[tokio::test]
    async fn sse_stream_emits_json_data_frames() {
        let (tx, rx) = broadcast::channel(8);
        let mut stream = sse_event_stream(rx);
        tx.send(sample_event(1)).unwrap();
        let frame = stream.next().await.unwrap().unwrap();
        assert!(format!("{frame:?}").contains("data"));
    }

    #[test]
    fn parse_args_defaults_and_overrides() {
        let args = parse_args(&[]).unwrap();
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 8080);
        assert!(!args.db.is_empty());

        let custom = vec![
            "--host".to_string(),
            "0.0.0.0".to_string(),
            "--port".to_string(),
            "9090".to_string(),
            "--db".to_string(),
            "/tmp/vp.db".to_string(),
        ];
        let args = parse_args(&custom).unwrap();
        assert_eq!(args.host, "0.0.0.0");
        assert_eq!(args.port, 9090);
        assert_eq!(args.db, "/tmp/vp.db");
    }

    #[test]
    fn parse_args_rejects_bad_input() {
        assert!(
            parse_args(&["--port".to_string()]).is_err(),
            "missing value"
        );
        assert!(
            parse_args(&["--port".to_string(), "not-a-port".to_string()]).is_err(),
            "non-numeric port"
        );
        assert!(
            parse_args(&["--nope".to_string()]).is_err(),
            "unknown flag must fail"
        );
    }

    #[test]
    fn summarize_output_prefers_result_text() {
        assert_eq!(
            summarize_output(&serde_json::json!({ "result": "hello" })),
            "hello"
        );
        assert_eq!(summarize_output(&serde_json::json!("plain")), "plain");
        assert_eq!(
            summarize_output(&serde_json::json!({ "moved_count": 2 })),
            "{\"moved_count\":2}"
        );
    }
}
