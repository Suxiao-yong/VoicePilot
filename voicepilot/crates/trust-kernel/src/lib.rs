//! VoicePilot Trust Kernel — the single trust boundary.
//!
//! W1: SQLite persistence, audit logging, task state machine.
//! W2: Policy engine — types, E×D risk matrix, Cedar, Constraint, Egress, Transaction, Action Gateway.
//! Voice / MCP / full ToolResult V2 land in W3+.

pub mod error;
pub mod state;
pub mod db;
pub mod audit;
pub mod repo;
pub mod kernel;
pub mod policy;
pub mod gateway;
pub mod tools;
pub mod compensation;
pub mod toolresult;
pub mod approval;
pub mod allowed_paths;
pub mod mcp;
pub mod skills;
pub mod llm;
pub mod extensions;
// Wave 1 Task 1.2: 统一 PlannerPipeline(纯规划层,无 DB 副作用)。
// 文本 / 语音 / CLI 共用;trace 持久化由任务运行层完成。
pub mod planner;
// W9 Plan 1: 加密原语模块(stronghold 子模块内部 #[cfg(feature = "stronghold")] 门控)
pub mod crypto;
// W10 Plan 3: voice_latency_samples 表的统计 + 清理 + 记录器。
// default-gated(纯 DB 操作,不依赖 voice feature)—— voice listener(voice-gated)
// 返回 ListenTimings 后,caller 用 LatencyRecorder 写表;CLI admin 命令直接调
// compute_stats / prune_older_than。Fitness Function voice_latency_table_exists
// (default-gated)也能访问 prune_older_than 验证函数存在。
pub mod voice_latency;

// Task 4: turns 情景记忆表读写（纯 DB 操作，default-gated，与 voice_latency 同级）。
pub mod turns;

// Task 6: llm_route_cache 表读写（纯 DB 操作，default-gated）。
pub mod llm_cache;

// W10 Plan 5: 审计事件覆盖率检查器(spec §7.2)。
// default-gated(纯 DB 操作,不依赖 voice/stronghold feature)。
// AuditCoverageChecker 查 audit_logs 表 DISTINCT event_type,与
// AUDIT_EVENT_TYPE_REGISTRY 对比计算覆盖率。Fitness Function
// audit_coverage_default_full(default-gated)用 with_expected 传入
// 25 种 default-reachable 子集断言 100% 覆盖。
pub mod audit_coverage;

// Wave 3 Task 3.1: SecretStore(Windows Credential Manager via keyring)。
// 核心类型无条件;keyring-backed 实现 Windows-only。
pub mod secrets;

#[cfg(feature = "voice")]
pub mod voice;

// W7 Plan 4 Task 2: Windows UIA adapter — opt-in via `uia` feature.
// Project is Windows-only (user decision 2026-07-26); the `all(windows, ...)`
// gate is retained as a compile-time guard so a stray `--features uia` on a
// non-Windows target fails fast instead of trying to compile the
// `uiautomation` crate.
#[cfg(all(windows, feature = "uia"))]
pub mod uiautomation;
