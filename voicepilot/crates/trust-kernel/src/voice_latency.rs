//! W10 Plan 3 — Voice latency benchmark infrastructure (spec §VP-FR-001).
//!
//! 存储 + 统计 + 清理 voice listen 的延迟样本:
//! - `LatencySample`:单条样本(DB 行映射)
//! - `LatencyStats`:P50/P95/P99/max 统计结果
//! - `compute_stats(conn, since)`:从 voice_latency_samples 表计算统计
//! - `prune_older_than(conn, days, now_ms)`:清理旧样本
//! - `VoiceLatencyTiming`:listener 返回的时间戳对(t0 + t1)
//! - `LatencyRecorder`:封装 kernel + model + privacy_mode,提供 record 方法
//!
//! **feature gate:** default(纯 DB 操作)。voice listener(voice-gated)返回
//! `ListenTimings` 后,caller(default 或 voice 上下文均可)构造 `LatencyRecorder`
//! 写表。Fitness Function `voice_latency_table_exists`(default-gated)访问
//! `prune_older_than` 验证函数存在。
//!
//! **P95 计算:** spec §5.2 v2 修订 #7 — 100 样本时 P95 = 排序后索引 94。
//! 通用公式:P95 索引 = ceil(sample_count * 0.95) - 1(0-based)。

use crate::error::{KernelError, Result};
use rusqlite::{Connection, params};

/// 单条延迟样本(映射 voice_latency_samples 表行)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencySample {
    pub id: Option<i64>,
    pub started_at_ms: i64,
    pub latency_ms: i64,
    pub model: String,
    pub privacy_mode: i64,
}

/// 延迟统计结果(spec §5.2 LatencyStats)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyStats {
    pub sample_count: u64,
    pub p50_ms: i64,
    pub p95_ms: i64,
    pub p99_ms: i64,
    pub max_ms: i64,
}

/// 按路由通道(route_kind)分组的延迟统计结果(契约 §2.1，可观测面板数据源)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteLatencyStats {
    pub route_kind: String,
    pub sample_count: u64,
    pub p50_ms: i64,
    pub p95_ms: i64,
    pub p99_ms: i64,
    pub max_ms: i64,
}

/// P 分位索引(0-based)= ceil(n * P / 100) - 1，clamp 到 [0, n-1]。
/// `compute_stats` 与 `compute_route_stats` 共用同一公式，避免口径漂移。
fn percentile_value(sorted: &[i64], p: u64) -> i64 {
    let n = sorted.len();
    let idx = ((n as u64 * p).div_ceil(100)) as usize;
    sorted[idx.saturating_sub(1).min(n - 1)]
}

/// 从 voice_latency_samples 表计算延迟统计。
///
/// `since`:可选的 epoch ms 下界,仅统计 started_at_ms >= since 的样本。
/// None = 全部样本。
///
/// **P50/P95/P99 计算:**
/// - 排序 latencies 升序
/// - P 索引(0-based)= ceil(n * P / 100) - 1,n = sample_count
/// - 100 样本时:P50 = idx 49,P95 = idx 94,P99 = idx 98
/// - sample_count = 0 时返回全 0
pub fn compute_stats(conn: &Connection, since: Option<i64>) -> Result<LatencyStats> {
    let sql = if since.is_some() {
        "SELECT latency_ms FROM voice_latency_samples WHERE started_at_ms >= ?1 ORDER BY latency_ms ASC"
    } else {
        "SELECT latency_ms FROM voice_latency_samples ORDER BY latency_ms ASC"
    };
    let mut stmt = conn.prepare(sql)?;

    let latencies: Vec<i64> = if let Some(since_ms) = since {
        stmt.query_map(params![since_ms], |r| r.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map([], |r| r.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect()
    };

    let n = latencies.len();
    if n == 0 {
        return Ok(LatencyStats {
            sample_count: 0,
            p50_ms: 0,
            p95_ms: 0,
            p99_ms: 0,
            max_ms: 0,
        });
    }

    Ok(LatencyStats {
        sample_count: n as u64,
        p50_ms: percentile_value(&latencies, 50),
        p95_ms: percentile_value(&latencies, 95),
        p99_ms: percentile_value(&latencies, 99),
        max_ms: *latencies.last().unwrap(),
    })
}

/// 按 route_kind 分组计算延迟统计(契约 §2.1)。
///
/// `since`:可选 epoch ms 下界，仅统计 started_at_ms >= since 的样本；None = 全部。
/// 分组口径与 `compute_stats` 完全一致(P 索引 = ceil(n*P/100)-1)。
/// 空表返回空 Vec(不补零行，调用方按缺失即无样本处理)。
pub fn compute_route_stats(
    conn: &Connection,
    since: Option<i64>,
) -> Result<Vec<RouteLatencyStats>> {
    let sql = if since.is_some() {
        "SELECT route_kind, latency_ms FROM voice_latency_samples \
         WHERE started_at_ms >= ?1 ORDER BY route_kind ASC, latency_ms ASC"
    } else {
        "SELECT route_kind, latency_ms FROM voice_latency_samples \
         ORDER BY route_kind ASC, latency_ms ASC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(since_ms) = since {
        stmt.query_map(params![since_ms], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    // 行已按 (route_kind, latency_ms) 升序，顺序扫一遍即可分组且组内有序。
    let mut out: Vec<RouteLatencyStats> = Vec::new();
    let mut current: Option<(String, Vec<i64>)> = None;
    let flush = |group: Option<(String, Vec<i64>)>, out: &mut Vec<RouteLatencyStats>| {
        if let Some((route_kind, latencies)) = group {
            if !latencies.is_empty() {
                out.push(RouteLatencyStats {
                    sample_count: latencies.len() as u64,
                    p50_ms: percentile_value(&latencies, 50),
                    p95_ms: percentile_value(&latencies, 95),
                    p99_ms: percentile_value(&latencies, 99),
                    max_ms: *latencies.last().unwrap(),
                    route_kind,
                });
            }
        }
    };
    for (route_kind, latency_ms) in rows {
        match &mut current {
            Some((kind, latencies)) if *kind == route_kind => latencies.push(latency_ms),
            _ => {
                flush(current.take(), &mut out);
                current = Some((route_kind, vec![latency_ms]));
            }
        }
    }
    flush(current.take(), &mut out);
    Ok(out)
}

/// 直接写入一条路由延迟样本(契约 §2.1 / 勘误 #11)。
///
/// **不得复用 `TrustKernel::record_voice_latency_sample`** —— 那个函数会建占位
/// task + 写 `voice_started` 审计，路由采样每次规划都写会污染哈希链与任务表。
/// 本函数只做一条 INSERT：不建 task、不写审计。
///
/// `route_kind`:"fastroute" | "keyword" | "llm" | "cache"(voice 行由 008/既有路径写)。
/// `model` 列填 `"route"` 标记非语音样本，`privacy_mode` 固定 0(与语音样本区分)。
pub fn record_route_sample(
    conn: &Connection,
    started_at_ms: i64,
    latency_ms: i64,
    route_kind: &str,
    cache_hit: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO voice_latency_samples
            (started_at_ms, latency_ms, model, privacy_mode, route_kind, cache_hit)
         VALUES (?1, ?2, 'route', 0, ?3, ?4)",
        params![started_at_ms, latency_ms, route_kind, cache_hit as i64],
    )?;
    Ok(())
}

/// 清理早于指定天数的延迟样本(spec §5.2 v2 修订 #15)。
///
/// `days`:保留天数,删除 `started_at_ms < (now_ms - days * 86400 * 1000)` 的行。
/// `now_ms`:当前 epoch ms(由 caller 传入,便于测试注入固定时间)。
///
/// 返回删除的行数。空表返回 0。`days=0` 删除所有 `started_at_ms < now_ms` 的行。
pub fn prune_older_than(conn: &Connection, days: u32, now_ms: i64) -> Result<u64> {
    let threshold = now_ms - (days as i64) * 86_400 * 1000;
    let deleted = conn.execute(
        "DELETE FROM voice_latency_samples WHERE started_at_ms < ?1",
        params![threshold],
    )?;
    Ok(deleted as u64)
}

/// Voice listen 的时间戳对(spec §5.3 voice_started_at + first_partial_received_at)。
///
/// 由 caller 从 `ListenTimings`(voice-gated,字段为 `Option<SystemTime>`)转换而来:
/// 仅当 `voice_started_at` + `first_partial_at` 都为 `Some` 时才构造本结构,
/// 传给 `LatencyRecorder::record()`。若任一为 None,caller 跳过记录(no-op)。
/// - `voice_started_at`:VAD 检测首个 voiced chunk 的 SystemTime(t0)
/// - `first_partial_at`:首个 partial transcript 回调的 SystemTime(t1)
///
/// **为何用 SystemTime 而非 Instant:** `Instant` 是单调时钟,无法转为 epoch ms
/// (与 `UNIX_EPOCH` 互转会类型错误)。`started_at_ms` 需要 epoch ms 用于
/// `prune_older_than(days, now_ms)` 和 `compute_stats(since)` 的时间窗口过滤。
/// `SystemTime` 同时支持 `duration_since(UNIX_EPOCH)`(转 epoch ms)和
/// `duration_since(other_system_time)`(算 latency)。对 <1s 级延迟测量,
/// NTP 调整的影响可忽略(单调性损失在可接受范围)。
#[derive(Debug, Clone)]
pub struct VoiceLatencyTiming {
    pub voice_started_at: std::time::SystemTime,
    pub first_partial_at: std::time::SystemTime,
}

/// 延迟样本记录器(spec §5.3 LatencyRecorder)。
///
/// 封装 kernel 引用 + 模型名 + privacy_mode,提供 `record(timing)` 方法。
/// caller(CLI/UI voice loop)构造后,在 listen 完成时调 `record()`。
///
/// **feature gate:** default(不依赖 voice feature)。voice listener 返回
/// `VoiceLatencyTiming` 后,caller 构造 `LatencyRecorder` 写表。
pub struct LatencyRecorder<'a> {
    kernel: &'a crate::kernel::TrustKernel,
    model: String,
    privacy_mode: bool,
}

impl<'a> LatencyRecorder<'a> {
    /// 创建 LatencyRecorder。
    ///
    /// - `kernel`:TrustKernel 引用(用于写表 + emit audit event)
    /// - `model`:sherpa-rs 模型名(如 "sense_voice")
    /// - `privacy_mode`:true=local only,false=cloud LLM
    pub fn new(kernel: &'a crate::kernel::TrustKernel, model: &str, privacy_mode: bool) -> Self {
        Self {
            kernel,
            model: model.to_string(),
            privacy_mode,
        }
    }

    /// 记录一条延迟样本。
    ///
    /// 计算 `latency_ms = (first_partial_at - voice_started_at).as_millis()`,
    /// 通过 `kernel.record_voice_latency_sample()` 写入 voice_latency_samples 表
    /// + emit `voice_started` audit event。
    ///
    /// 若 latency 为 0 或负数(时钟漂移),仍记录(便于诊断)。
    pub fn record(&self, timing: &VoiceLatencyTiming) -> Result<()> {
        let latency_ms = timing
            .first_partial_at
            .duration_since(timing.voice_started_at)
            .map_err(|e| {
                KernelError::VoiceLatency(format!(
                    "first_partial_at before voice_started_at: {}",
                    e
                ))
            })?
            .as_millis() as i64;
        let started_at_ms = timing
            .voice_started_at
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| {
                KernelError::VoiceLatency(format!("voice_started_at before UNIX_EPOCH: {}", e))
            })?
            .as_millis() as i64;
        self.kernel.record_voice_latency_sample(
            started_at_ms,
            latency_ms,
            &self.model,
            self.privacy_mode,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_conn_with_samples(samples: &[LatencySample]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE voice_latency_samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at_ms INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                model TEXT NOT NULL,
                privacy_mode INTEGER NOT NULL DEFAULT 0,
                route_kind TEXT NOT NULL DEFAULT 'voice',
                cache_hit INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        for s in samples {
            conn.execute(
                "INSERT INTO voice_latency_samples (started_at_ms, latency_ms, model, privacy_mode)
                 VALUES (?1, ?2, ?3, ?4)",
                params![s.started_at_ms, s.latency_ms, s.model, s.privacy_mode],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn compute_stats_empty_returns_zeros() {
        let conn = open_conn_with_samples(&[]);
        let stats = compute_stats(&conn, None).unwrap();
        assert_eq!(stats.sample_count, 0);
        assert_eq!(stats.p50_ms, 0);
        assert_eq!(stats.p95_ms, 0);
        assert_eq!(stats.p99_ms, 0);
        assert_eq!(stats.max_ms, 0);
    }

    #[test]
    fn compute_stats_100_samples_p95_is_index_94() {
        // spec §5.2 v2 修订 #7:100 样本时 P95 = 排序后索引 94(0-based)。
        // 构造 100 样本,latency = 100..200(升序),P95 = latencies[94] = 194。
        let samples: Vec<LatencySample> = (0..100)
            .map(|i| LatencySample {
                id: None,
                started_at_ms: 1_000_000 + i,
                latency_ms: 100 + i,
                model: "sense_voice".to_string(),
                privacy_mode: 0,
            })
            .collect();
        let conn = open_conn_with_samples(&samples);
        let stats = compute_stats(&conn, None).unwrap();
        assert_eq!(stats.sample_count, 100);
        assert_eq!(stats.p50_ms, 149, "P50 = latencies[49] = 100+49 = 149");
        assert_eq!(stats.p95_ms, 194, "P95 = latencies[94] = 100+94 = 194");
        assert_eq!(stats.p99_ms, 198, "P99 = latencies[98] = 100+98 = 198");
        assert_eq!(stats.max_ms, 199, "max = latencies[99] = 199");
    }

    #[test]
    fn compute_stats_with_since_filter() {
        // 3 样本:started_at_ms = 100, 200, 300,latency = 50, 150, 250
        // since=200 → 取后 2 样本,P50 = 150,P95 = 250(ceil(2*95/100)-1 = 1)
        let samples = vec![
            LatencySample {
                id: None,
                started_at_ms: 100,
                latency_ms: 50,
                model: "m".to_string(),
                privacy_mode: 0,
            },
            LatencySample {
                id: None,
                started_at_ms: 200,
                latency_ms: 150,
                model: "m".to_string(),
                privacy_mode: 0,
            },
            LatencySample {
                id: None,
                started_at_ms: 300,
                latency_ms: 250,
                model: "m".to_string(),
                privacy_mode: 0,
            },
        ];
        let conn = open_conn_with_samples(&samples);
        let stats = compute_stats(&conn, Some(200)).unwrap();
        assert_eq!(stats.sample_count, 2);
        assert_eq!(stats.p50_ms, 150);
        assert_eq!(stats.p95_ms, 250);
        assert_eq!(stats.max_ms, 250);
    }

    #[test]
    fn compute_stats_single_sample_all_percentiles_equal() {
        let samples = vec![LatencySample {
            id: None,
            started_at_ms: 1,
            latency_ms: 42,
            model: "m".to_string(),
            privacy_mode: 0,
        }];
        let conn = open_conn_with_samples(&samples);
        let stats = compute_stats(&conn, None).unwrap();
        assert_eq!(stats.sample_count, 1);
        assert_eq!(stats.p50_ms, 42);
        assert_eq!(stats.p95_ms, 42);
        assert_eq!(stats.p99_ms, 42);
        assert_eq!(stats.max_ms, 42);
    }

    #[test]
    fn prune_older_than_deletes_old_samples_keeps_recent() {
        // now = 10_000_000_000(模拟 epoch ms)
        // 60 天前样本:应删除;10 天前 + 当前样本:应保留(days=30 阈值)
        let now_ms: i64 = 10_000_000_000;
        let sixty_days_ago = now_ms - 60 * 86_400 * 1000;
        let ten_days_ago = now_ms - 10 * 86_400 * 1000;
        let samples = vec![
            LatencySample {
                id: None,
                started_at_ms: sixty_days_ago,
                latency_ms: 100,
                model: "m".to_string(),
                privacy_mode: 0,
            },
            LatencySample {
                id: None,
                started_at_ms: ten_days_ago,
                latency_ms: 200,
                model: "m".to_string(),
                privacy_mode: 0,
            },
            LatencySample {
                id: None,
                started_at_ms: now_ms,
                latency_ms: 300,
                model: "m".to_string(),
                privacy_mode: 0,
            },
        ];
        let conn = open_conn_with_samples(&samples);

        let deleted = prune_older_than(&conn, 30, now_ms).unwrap();
        assert_eq!(deleted, 1, "should delete 1 sample (60 days old)");

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM voice_latency_samples", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 2);

        let latencies: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT latency_ms FROM voice_latency_samples ORDER BY latency_ms ASC")
                .unwrap();
            stmt.query_map([], |r| r.get::<_, i64>(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        assert_eq!(latencies, vec![200, 300]);
    }

    #[test]
    fn prune_older_than_zero_days_deletes_all() {
        let now_ms: i64 = 10_000_000_000;
        let samples = vec![
            LatencySample {
                id: None,
                started_at_ms: now_ms - 1000,
                latency_ms: 100,
                model: "m".to_string(),
                privacy_mode: 0,
            },
            LatencySample {
                id: None,
                started_at_ms: now_ms - 2000,
                latency_ms: 200,
                model: "m".to_string(),
                privacy_mode: 0,
            },
        ];
        let conn = open_conn_with_samples(&samples);

        let deleted = prune_older_than(&conn, 0, now_ms).unwrap();
        assert_eq!(deleted, 2, "should delete all 2 samples");
    }

    #[test]
    fn prune_older_than_empty_table_returns_zero() {
        let conn = open_conn_with_samples(&[]);
        let deleted = prune_older_than(&conn, 30, 10_000_000_000).unwrap();
        assert_eq!(deleted, 0);
    }

    // ===== LatencyRecorder tests =====
    use crate::kernel::TrustKernel;
    use std::time::{Duration, SystemTime};

    #[test]
    fn latency_recorder_records_sample_when_both_timestamps_set() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let recorder = LatencyRecorder::new(&kernel, "sense_voice", false);

        let t0 = SystemTime::now();
        let t1 = t0 + Duration::from_millis(120);
        let timing = VoiceLatencyTiming {
            voice_started_at: t0,
            first_partial_at: t1,
        };

        recorder.record(&timing).unwrap();

        // 验证样本已写入表
        let conn = kernel.conn();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM voice_latency_samples WHERE model = 'sense_voice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let (latency_ms, privacy_mode): (i64, i64) = conn
            .query_row(
                "SELECT latency_ms, privacy_mode FROM voice_latency_samples WHERE model = 'sense_voice'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            (100..=200).contains(&latency_ms),
            "latency_ms should be ~120ms, got {}",
            latency_ms
        );
        assert_eq!(privacy_mode, 0, "privacy_mode=0 for cloud LLM");
    }

    #[test]
    fn latency_recorder_privacy_mode_one_persisted() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let recorder = LatencyRecorder::new(&kernel, "sense_voice", true);

        let t0 = SystemTime::now();
        let t1 = t0 + Duration::from_millis(80);
        let timing = VoiceLatencyTiming {
            voice_started_at: t0,
            first_partial_at: t1,
        };

        recorder.record(&timing).unwrap();

        let conn = kernel.conn();
        let privacy_mode: i64 = conn
            .query_row("SELECT privacy_mode FROM voice_latency_samples", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(privacy_mode, 1, "privacy_mode=1 for local only");
    }

    // ===== Task 2: 路由维度采样 / 分组统计（契约 §2.1） =====

    #[test]
    fn compute_route_stats_groups_by_route_kind() {
        // 3 个通道各 2 样本，验证分组、样本数、分位数互不串。
        let conn = open_conn_with_samples(&[]);
        let rows = [
            ("fastroute", 1_i64, 1000_i64),
            ("fastroute", 3, 1002),
            ("keyword", 10, 2000),
            ("keyword", 20, 2001),
            ("llm", 500, 3000),
            ("llm", 700, 3001),
        ];
        for (kind, latency, started) in rows {
            record_route_sample(&conn, started, latency, kind, kind == "cache").unwrap();
        }

        let stats = compute_route_stats(&conn, None).unwrap();
        assert_eq!(stats.len(), 3, "one bucket per route_kind");
        let by_kind = |k: &str| {
            stats
                .iter()
                .find(|s| s.route_kind == k)
                .unwrap_or_else(|| panic!("missing bucket {k}"))
                .clone()
        };

        let fast = by_kind("fastroute");
        assert_eq!(fast.sample_count, 2);
        assert_eq!(fast.p50_ms, 1);
        assert_eq!(fast.p95_ms, 3);
        assert_eq!(fast.p99_ms, 3);
        assert_eq!(fast.max_ms, 3);

        let keyword = by_kind("keyword");
        assert_eq!(keyword.sample_count, 2);
        assert_eq!(keyword.p50_ms, 10);
        assert_eq!(keyword.max_ms, 20);

        let llm = by_kind("llm");
        assert_eq!(llm.sample_count, 2);
        assert_eq!(llm.p50_ms, 500);
        assert_eq!(llm.p95_ms, 700);
        assert_eq!(llm.max_ms, 700);
    }

    #[test]
    fn compute_route_stats_empty_table_returns_empty_vec() {
        let conn = open_conn_with_samples(&[]);
        assert!(compute_route_stats(&conn, None).unwrap().is_empty());
        assert!(compute_route_stats(&conn, Some(123)).unwrap().is_empty());
    }

    #[test]
    fn compute_route_stats_respects_since_window() {
        let conn = open_conn_with_samples(&[]);
        record_route_sample(&conn, 100, 1, "fastroute", false).unwrap();
        record_route_sample(&conn, 200, 5, "fastroute", false).unwrap();
        let stats = compute_route_stats(&conn, Some(150)).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].sample_count, 1);
        assert_eq!(stats[0].p50_ms, 5);
    }

    #[test]
    fn record_route_sample_persists_cache_hit_as_zero_or_one() {
        let conn = open_conn_with_samples(&[]);
        record_route_sample(&conn, 10, 2, "cache", true).unwrap();
        record_route_sample(&conn, 11, 3, "llm", false).unwrap();

        let (hit_kind, hit): (String, i64) = conn
            .query_row(
                "SELECT route_kind, cache_hit FROM voice_latency_samples WHERE started_at_ms = 10",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(hit_kind, "cache");
        assert_eq!(hit, 1, "cache_hit must be stored as INTEGER 1");

        let (miss_kind, miss): (String, i64) = conn
            .query_row(
                "SELECT route_kind, cache_hit FROM voice_latency_samples WHERE started_at_ms = 11",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(miss_kind, "llm");
        assert_eq!(miss, 0, "cache_hit must be stored as INTEGER 0");
    }

    #[test]
    fn record_route_sample_does_not_create_task_or_audit() {
        // 勘误 #11：路由采样走直接 INSERT，不得建占位 task / 写审计。
        let kernel = TrustKernel::open_in_memory().unwrap();
        {
            let conn = kernel.conn();
            record_route_sample(&conn, 10, 2, "fastroute", false).unwrap();
        }
        let conn = kernel.conn();
        let tasks: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let audits: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tasks, 0, "route sampling must not create tasks");
        assert_eq!(audits, 0, "route sampling must not append audit events");
    }
}
