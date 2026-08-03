//! W10 Plan 3/6 — default-gated Fitness Functions(spec §8.1)。
//!
//! Plan 3 贡献测试 14:`voice_latency_table_exists`。
//! Plan 6 会追加测试 1-13(verifier_coverage / compensation_coverage /
//! kill_switch_sla / audit_registry / audit_coverage)。
//!
//! 这些测试在 default feature(--no-default-features)下运行,验证
//! V1 发布门禁的代码层硬指标(spec §9.4 ⑥⑦⑨⑩)。
//!
//! **不写文件级 cfg gate** —— 让 default 组合(--no-default-features)也能编译运行此文件。
//! voice_latency 模块本身就是 default-gated(纯 DB 操作,不依赖 voice feature)。

use trust_kernel::kernel::TrustKernel;

/// 测试 14(W10 Plan 3):voice_latency_samples 表存在 + prune 函数可调用。
///
/// spec §5.4 Fitness Function:default-gated,只检查 migration 008 创建表 +
/// prune 函数存在(不验证 P95,因 P95 需 voice feature + sherpa-rs 模型,
/// 由 `w10_voice_latency_smoke.rs::p95_first_partial_transcript_under_500ms` 覆盖)。
///
/// 验证项:
/// 1. `voice_latency_samples` 表存在(migration 008 已运行)
/// 2. `idx_voice_latency_started` 索引存在(加速 compute_stats 查询)
/// 3. `prune_voice_latency_older_than(30, now_ms)` 可调用(空表返回 0)
/// 4. `compute_voice_latency_stats(None)` 可调用(空表返回 0 统计)
#[test]
fn voice_latency_table_exists() {
    let kernel = TrustKernel::open_in_memory().unwrap();

    // 1. 验证 voice_latency_samples 表存在(通过 kernel.conn 查 sqlite_master)
    let conn = kernel.conn();
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='voice_latency_samples'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        table_exists,
        "voice_latency_samples table must exist (migration 008)"
    );

    // 2. 验证 idx_voice_latency_started 索引存在
    let index_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_voice_latency_started'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        index_exists,
        "idx_voice_latency_started index must exist (migration 008)"
    );
    drop(conn);

    // 3. 验证 prune_voice_latency_older_than 函数存在且可调用(空表返回 0)
    let now_ms = chrono::Utc::now().timestamp_millis();
    let deleted = kernel
        .prune_voice_latency_older_than(30, now_ms)
        .expect("prune_voice_latency_older_than must be callable");
    assert_eq!(
        deleted, 0,
        "prune on empty table must return 0 (no rows to delete)"
    );

    // 4. 验证 compute_voice_latency_stats 函数存在且可调用(空表返回 0 统计)
    let stats = kernel
        .compute_voice_latency_stats(None)
        .expect("compute_voice_latency_stats must be callable");
    assert_eq!(stats.sample_count, 0, "empty table must have 0 samples");
    assert_eq!(stats.p50_ms, 0);
    assert_eq!(stats.p95_ms, 0);
    assert_eq!(stats.p99_ms, 0);
    assert_eq!(stats.max_ms, 0);
}
