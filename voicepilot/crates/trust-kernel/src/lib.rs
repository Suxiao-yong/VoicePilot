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
// W9 Plan 1: 加密原语模块(stronghold 子模块内部 #[cfg(feature = "stronghold")] 门控)
pub mod crypto;
// W10 Plan 3: voice_latency_samples 表的统计 + 清理 + 记录器。
// default-gated(纯 DB 操作,不依赖 voice feature)—— voice listener(voice-gated)
// 返回 ListenTimings 后,caller 用 LatencyRecorder 写表;CLI admin 命令直接调
// compute_stats / prune_older_than。Fitness Function voice_latency_table_exists
// (default-gated)也能访问 prune_older_than 验证函数存在。
pub mod voice_latency;

// W10 Plan 5: 审计事件覆盖率检查器(spec §7.2)。
// default-gated(纯 DB 操作,不依赖 voice/stronghold feature)。
// AuditCoverageChecker 查 audit_logs 表 DISTINCT event_type,与
// AUDIT_EVENT_TYPE_REGISTRY 对比计算覆盖率。Fitness Function
// audit_coverage_default_full(default-gated)用 with_expected 传入
// 25 种 default-reachable 子集断言 100% 覆盖。
pub mod audit_coverage;

#[cfg(feature = "voice")]
pub mod voice;

// W7 Plan 4 Task 2: Windows UIA adapter — opt-in via `uia` feature.
// Project is Windows-only (user decision 2026-07-26); the `all(windows, ...)`
// gate is retained as a compile-time guard so a stray `--features uia` on a
// non-Windows target fails fast instead of trying to compile the
// `uiautomation` crate.
#[cfg(all(windows, feature = "uia"))]
pub mod uiautomation;
