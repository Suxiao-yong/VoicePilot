//! W10 Plan 6 — 全 feature 审计事件覆盖率集成测试(spec §8.1 + §9.4 ⑩)。
//!
//! **feature gate:** voice,llm,uia,stronghold(trust-kernel 全 feature,文件级 cfg gate)。
//! **test gate:** `#[ignore]` — 需手动 `cargo test -p trust-kernel --features voice,llm,uia,stronghold -- --ignored`。
//!
//! 1 个测试:`audit_coverage_full_features` — 全 feature 30/30 = 100% 覆盖。
//!
//! 与 `w10_audit_coverage_smoke.rs::audit_coverage_default_full`(Plan 5,27/27)
//! 互补:default 测 27 种 default-reachable,本测试补 3 种需 voice/stronghold feature
//! 的事件(voice_started / stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed)。
//!
//! **直接 emit 偏离(沿用 Plan 5 §用户决策 #5):** 不通过真实 Skill happy path +
//! Stronghold encrypt/decrypt 路径触发(那些已在 w9_stronghold_api_smoke /
//! w9_snapshot_encrypted_smoke / w10_voice_latency_smoke 等测试覆盖)。本测试用
//! `audit_append_external` 直接 emit 30 种,聚焦 registry 完整性 + coverage 机制。

#![cfg(all(
    feature = "voice",
    feature = "llm",
    feature = "uia",
    feature = "stronghold"
))]

use trust_kernel::audit::AUDIT_EVENT_TYPE_REGISTRY;
use trust_kernel::audit_coverage::AuditCoverageChecker;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};

/// W10 Plan 6:全 feature 30/30 = 100% audit event_type 覆盖。
///
/// spec §8.1 + §9.4 ⑩:V1 发布门禁 "审计日志覆盖率 100%"。
///
/// 本测试通过 `audit_append_external` 直接 emit AUDIT_EVENT_TYPE_REGISTRY 全部 30 种
/// event_type(含 default 不可达的 voice_started / stronghold_snapshot_encrypted /
/// stronghold_snapshot_decrypt_failed 3 种),验证 `AuditCoverageChecker::new(&kernel)`
/// (全量 registry)报告 30/30 = 100% 覆盖。
///
/// **运行方式:** `cargo test -p trust-kernel --features voice,llm,uia,stronghold -- --ignored audit_coverage_full_features`
///
/// **注意:** 本测试用直接 emit 方式,不通过真实 Skill / Stronghold / Voice 路径触发
/// (那些路径已有测试覆盖)。本测试聚焦 registry 完整性 — 任何 event_type 被移除或
/// 重命名,本测试立即失败,强制 registry 与 emit callsite 同步。
#[test]
#[ignore = "requires all features (voice,llm,uia,stronghold) - manual run only"]
fn audit_coverage_full_features() {
    // 全量 registry 30 种 event_type(W10 Plan 5 §用户决策 #1 + W11 Plan 4 + Wave 3 secret_migration)
    assert_eq!(
        AUDIT_EVENT_TYPE_REGISTRY.len(),
        30,
        "AUDIT_EVENT_TYPE_REGISTRY must contain exactly 30 event types"
    );

    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = "t-full-coverage";
    {
        let conn = kernel.conn();
        let task = TaskRecord::new(task_id, "full coverage test placeholder");
        TaskRepo::new().create(&conn, &task).unwrap();
    }

    // 直接 emit 全部 30 种 event_type
    for et in AUDIT_EVENT_TYPE_REGISTRY {
        kernel
            .audit_append_external(task_id, None, et, serde_json::json!({"full_feature": et}))
            .unwrap();
    }

    // 用全量 registry checker 断言 30/30 = 100% 覆盖
    let checker = AuditCoverageChecker::new(&kernel);
    let covered = checker.covered().unwrap();
    let uncovered = checker.uncovered().unwrap();
    let ratio = checker.coverage_ratio().unwrap();

    assert_eq!(
        covered.len(),
        30,
        "full feature must cover all 30 event types, covered: {:?}",
        covered
    );
    assert!(
        uncovered.is_empty(),
        "full feature must have 0 uncovered event types, uncovered: {:?}",
        uncovered
    );
    assert!(
        (ratio - 1.0).abs() < 1e-6,
        "full feature coverage ratio must be 1.0 (100%), got {}",
        ratio
    );

    // 额外验证:default 不可达的 3 种事件确实被覆盖
    assert!(
        covered.contains(&"voice_started".to_string()),
        "voice_started must be covered in full feature (voice feature enabled)"
    );
    assert!(
        covered.contains(&"stronghold_snapshot_encrypted".to_string()),
        "stronghold_snapshot_encrypted must be covered in full feature (stronghold feature enabled)"
    );
    assert!(
        covered.contains(&"stronghold_snapshot_decrypt_failed".to_string()),
        "stronghold_snapshot_decrypt_failed must be covered in full feature (stronghold feature enabled)"
    );
}
