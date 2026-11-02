//! Task 2 —— 延迟 / 缓存可观测面板 Tauri commands（契约 §2.3）。
//!
//! 单命令 `get_observability_command`，一次返回面板需要的全部指标：
//! - `routes`：按路由通道分组的 P50/P95/P99（`voice_latency::compute_route_stats`）
//! - `cache`：route cache 命中率 = 窗口内 `route_kind='cache'` / (`cache` + `llm`)
//! - `dag_tokens`：`audit_logs` 中 `llm_decompose_called` 的 `details.token_count` 聚合
//! - `audit_chain`：`kernel.verify_audit_chain()`（哈希链完整性，勿重写）
//!
//! 只读：本模块不写任何表、不追加审计事件。

use serde::{Deserialize, Serialize};

use crate::error::UiResult;
use crate::state::AppState;

/// 默认统计窗口：最近 24 小时。
pub const DEFAULT_WINDOW_HOURS: u32 = 24;

/// 单路由通道的延迟分位（映射 `voice_latency::RouteLatencyStats`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteLatencyBucket {
    /// fastroute | keyword | llm | cache | voice
    pub route_kind: String,
    pub sample_count: u64,
    pub p50_ms: i64,
    pub p95_ms: i64,
    pub p99_ms: i64,
    pub max_ms: i64,
}

/// route cache 命中率统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    /// 窗口内 `route_kind='cache'` 的样本数。
    pub hits: u64,
    /// 窗口内 `route_kind='llm'` 的样本数（走了 LLM = 未命中缓存）。
    pub misses: u64,
    /// hits / (hits + misses)；无样本时为 0.0。
    pub hit_rate: f64,
}

/// DAG 拆解的 token 消耗统计（数据源：`llm_decompose_called` 审计）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagTokenStats {
    pub plans: u64,
    pub total_tokens: u64,
    pub avg_tokens: f64,
}

/// 审计哈希链完整性。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditChainStatus {
    pub valid: bool,
    pub checked: usize,
    pub broken_at: Option<String>,
}

/// 可观测面板快照（契约 §2.3，与 HTTP `/api/v1/observability` 同构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilitySnapshot {
    pub window_hours: u32,
    pub routes: Vec<RouteLatencyBucket>,
    pub cache: CacheStats,
    pub dag_tokens: DagTokenStats,
    pub audit_chain: AuditChainStatus,
}

/// 统计窗口起点（epoch ms）：`now - window_hours * 3600_000`。
fn window_start_ms(window_hours: u32, now_ms: i64) -> i64 {
    now_ms - (window_hours as i64) * 3_600_000
}

/// 采集可观测快照（`window_hours=None` → 默认 24 小时）。
///
/// `conn` 的锁在本函数内短持有：`verify_audit_chain()` 内部会再次取同一把
/// Mutex，必须在调用它之前释放，否则死锁。
pub fn get_observability(
    state: &AppState,
    window_hours: Option<u32>,
) -> UiResult<ObservabilitySnapshot> {
    let hours = window_hours.unwrap_or(DEFAULT_WINDOW_HOURS);
    let now_ms = chrono::Utc::now().timestamp_millis();
    let since_ms = window_start_ms(hours, now_ms);
    let since_rfc3339 = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(since_ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();

    let (routes, plans, total_tokens) = {
        let conn = state.kernel.conn();
        let routes: Vec<RouteLatencyBucket> =
            trust_kernel::voice_latency::compute_route_stats(&conn, Some(since_ms))?
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
        // dag_tokens：直接聚合 llm_decompose_called 审计的 details.token_count
        // （timestamp 列为 RFC3339 文本，同格式下字典序比较即时间序）。
        let (plans, total_tokens): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CAST(json_extract(details, '$.token_count') AS INTEGER)), 0)
                 FROM audit_logs
                 WHERE event_type = 'llm_decompose_called' AND timestamp >= ?1",
                [since_rfc3339.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(trust_kernel::error::KernelError::from)?;
        (routes, plans.max(0) as u64, total_tokens.max(0) as u64)
    };

    // cache 命中率复用 routes 的样本计数（同一窗口、同一分组口径）。
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

    // 注意：此处不能持有 kernel.conn() 的锁（verify_audit_chain 内部会再取）。
    let chain = state.kernel.verify_audit_chain()?;
    let audit_chain = AuditChainStatus {
        valid: chain.valid,
        checked: chain.checked,
        broken_at: chain.broken_at,
    };

    Ok(ObservabilitySnapshot {
        window_hours: hours,
        routes,
        cache,
        dag_tokens,
        audit_chain,
    })
}

/// 采集可观测快照（延迟分位 / 缓存命中率 / DAG token / 审计链完整性）。
#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn get_observability_command(
    state: tauri::State<'_, AppState>,
    window_hours: Option<u32>,
) -> Result<ObservabilitySnapshot, String> {
    get_observability(&state, window_hours).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_start_ms_subtracts_hours() {
        assert_eq!(window_start_ms(24, 100_000_000), 100_000_000 - 86_400_000);
        assert_eq!(window_start_ms(0, 12_345), 12_345);
    }

    #[test]
    fn empty_db_reports_zeroed_cache_and_dag_stats() {
        let state = AppState::new_in_memory().unwrap();
        let snap = get_observability(&state, None).unwrap();
        assert_eq!(snap.window_hours, DEFAULT_WINDOW_HOURS);
        assert!(snap.routes.is_empty());
        assert_eq!(snap.cache.hits, 0);
        assert_eq!(snap.cache.misses, 0);
        assert_eq!(snap.cache.hit_rate, 0.0);
        assert_eq!(snap.dag_tokens.plans, 0);
        assert_eq!(snap.dag_tokens.total_tokens, 0);
        assert_eq!(snap.dag_tokens.avg_tokens, 0.0);
        assert!(snap.audit_chain.valid);
        assert_eq!(snap.audit_chain.checked, 0);
        assert!(snap.audit_chain.broken_at.is_none());
    }

    #[test]
    fn cache_hit_rate_and_routes_come_from_route_samples() {
        let state = AppState::new_in_memory().unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        {
            let conn = state.kernel.conn();
            // 窗口内：2 cache 命中 + 1 llm miss + 1 fastroute
            trust_kernel::voice_latency::record_route_sample(&conn, now_ms - 10, 2, "cache", true)
                .unwrap();
            trust_kernel::voice_latency::record_route_sample(&conn, now_ms - 9, 3, "cache", true)
                .unwrap();
            trust_kernel::voice_latency::record_route_sample(&conn, now_ms - 8, 900, "llm", false)
                .unwrap();
            trust_kernel::voice_latency::record_route_sample(
                &conn,
                now_ms - 7,
                1,
                "fastroute",
                false,
            )
            .unwrap();
            // 窗口外（25 小时前）：不计入
            trust_kernel::voice_latency::record_route_sample(
                &conn,
                now_ms - 25 * 3_600_000,
                5000,
                "llm",
                false,
            )
            .unwrap();
        }

        let snap = get_observability(&state, Some(24)).unwrap();
        assert_eq!(snap.cache.hits, 2);
        assert_eq!(snap.cache.misses, 1);
        assert!((snap.cache.hit_rate - 2.0 / 3.0).abs() < 1e-9);

        let kinds: Vec<&str> = snap.routes.iter().map(|r| r.route_kind.as_str()).collect();
        assert_eq!(kinds, vec!["cache", "fastroute", "llm"]);
        let llm = snap.routes.iter().find(|r| r.route_kind == "llm").unwrap();
        assert_eq!(llm.sample_count, 1, "out-of-window sample excluded");
        assert_eq!(llm.p95_ms, 900);
    }

    #[cfg(feature = "llm")]
    #[test]
    fn dag_token_stats_aggregate_llm_decompose_audit() {
        use trust_kernel::llm::types::DecomposeStats;

        let state = AppState::new_in_memory().unwrap();
        state.kernel.create_task("t-dag", "decompose").unwrap();
        let stats = DecomposeStats {
            llm_model: "stub".to_string(),
            latency_ms: 12,
            token_count: 100,
        };
        trust_kernel::llm::client::record_llm_decompose_called(
            &state.kernel,
            "t-dag",
            "plan-1",
            &stats,
        )
        .unwrap();

        let snap = get_observability(&state, Some(24)).unwrap();
        assert_eq!(snap.dag_tokens.plans, 1);
        assert_eq!(snap.dag_tokens.total_tokens, 100);
        assert!((snap.dag_tokens.avg_tokens - 100.0).abs() < 1e-9);
    }
}
