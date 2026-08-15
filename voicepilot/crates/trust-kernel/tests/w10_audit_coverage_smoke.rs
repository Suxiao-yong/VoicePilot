//! W10 Plan 5 — 审计事件覆盖率集成测试(spec §7.4 + §9.4 ⑩)。
//!
//! 3 个 default-gated 测试:
//! 1. `audit_registry_all_lower_snake_case` — registry 命名规范(编译时常量)
//! 2. `audit_emit_warns_on_unknown_event_type` — emit "unknown_event" 触发 warn
//! 3. `audit_coverage_default_full` — default feature 26/26 = 100% 覆盖
//!
//! **不写文件级 cfg gate** —— 让 default 组合(--no-default-features)也能编译运行此文件。
//! tracing_test dev-dep 在 default 下可用(不依赖任何 feature)。

use trust_kernel::audit::AUDIT_EVENT_TYPE_REGISTRY;
use trust_kernel::audit_coverage::AuditCoverageChecker;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};

/// W10 Plan 5 测试 1:registry 所有 event_type 匹配 `^[a-z][a-z0-9_]*$`(lower_snake_case)。
///
/// spec §10 Conventions + §7.4:registry 是编译期常量,本测试验证常量本身
/// 命名合规(与 W9 `audit_event_types_all_lower_snake_case` 测试互补 ——
/// W9 测运行时 audit_logs 表数据,本测试测编译期 registry 常量)。
#[test]
fn audit_registry_all_lower_snake_case() {
    let re = regex::Regex::new(r"^[a-z][a-z0-9_]*$").unwrap();
    let mut non_compliant: Vec<String> = Vec::new();
    for et in AUDIT_EVENT_TYPE_REGISTRY {
        if !re.is_match(et) {
            non_compliant.push(et.to_string());
        }
    }
    assert!(
        non_compliant.is_empty(),
        "AUDIT_EVENT_TYPE_REGISTRY contains event_type(s) not matching lower_snake_case: {:?}",
        non_compliant
    );
}

/// W10 Plan 5 测试 2:emit 未知 event_type 触发 tracing::warn!(不阻塞写入)。
///
/// spec §7.2 v2 修订 #3:运行时校验降级为 warn,不返回 Err。
/// 用 `#[traced_test]` 捕获 tracing 输出,断言 warn 含 "unknown audit event_type"。
/// 同时验证事件仍被写入 audit_logs(warn 不阻塞)。
///
/// **注意:** tracing-test 0.2.6 默认 env-filter 只捕获测试 crate 本身的日志,
/// 会过滤掉 `trust_kernel` 库 crate 的 `tracing::warn!`。集成测试必须启用
/// `no-env-filter` feature(Cargo.toml dev-dep 已配置),否则 logs_contain 永远
/// 返回 false(tracing-test 官方文档 "Per-crate Filtering" 章节明确说明此限制)。
#[tracing_test::traced_test]
#[test]
fn audit_emit_warns_on_unknown_event_type() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 创建占位 task 满足 FK
    let task_id = "t-warn-test";
    {
        let conn = kernel.conn();
        let task = TaskRecord::new(task_id, "warn test placeholder");
        TaskRepo::new().create(&conn, &task).unwrap();
    }
    // emit 未知 event_type
    kernel
        .audit_append_external(task_id, None, "unknown_event_type", serde_json::json!({}))
        .unwrap();
    // 验证 warn 被触发(tracing_test 注入 logs_contain 方法)
    assert!(
        logs_contain("unknown audit event_type"),
        "tracing::warn must be emitted for unknown event_type"
    );
    assert!(
        logs_contain("unknown_event_type"),
        "warn message must contain the unknown event_type name"
    );
    // 验证事件仍被写入 audit_logs(warn 不阻塞)
    let events = kernel.list_audit_for_task(task_id).unwrap();
    let found = events.iter().any(|e| e.event_type == "unknown_event_type");
    assert!(
        found,
        "unknown event_type must still be written to audit_logs (warn is non-blocking)"
    );
}

/// W10 Plan 5 测试 3:default feature 25/25 = 100% 覆盖(分母=25)。
///
/// spec §7.4:default feature 可触发 25 种事件(排除 voice_started /
/// stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed 三个
/// 需其他 feature 的)。本测试通过 `audit_append_external` 直接 emit 25 种,
/// 验证 AuditCoverageChecker 报告 100% 覆盖。
///
/// **注意:** 本测试用直接 emit 方式,不通过复杂 E2E 路径触发(那些路径已在
/// w8_e2e_dag_smoke / w9_default_boundary_smoke 等测试覆盖)。本 Plan 聚焦
/// "registry 完整性 + coverage 机制",E2E 触发已有测试。
#[test]
fn audit_coverage_default_full() {
    // default feature 可达的 25 种 event_type(28 - 3 不可达)
    const DEFAULT_REACHABLE: &[&str] = &[
        // W1-W3 基础(kernel.rs)
        "task_created",
        "state_transition",
        "step_created",
        "step_status_changed",
        "step_prepared",
        "step_committed",
        "compensation_created",
        "compensation_status_changed",
        "approval_recorded",
        // W4 MCP
        "mcp_tools_call",
        // W11 Plan 4
        "malicious_server_detected",
        // W8 DAG + LLM
        "llm_decompose_called",
        "llm_explain_called",
        "dag_plan_created",
        "dag_skeleton_approved",
        "dag_skeleton_modified",
        "dag_modify_limit_exceeded",
        "dag_node_started",
        "dag_node_succeeded",
        "dag_node_failed",
        "dag_completed",
        // W9 Stronghold/Taint(stronghold_degraded_mode_entered 方法无 feature 门控)
        "stronghold_degraded_mode_entered",
        "taint_propagated",
        "taint_blocked",
        // W10 Plan 4
        "kill_switch_triggered",
        "task_cancelled",
    ];
    assert_eq!(
        DEFAULT_REACHABLE.len(),
        26,
        "default-reachable event types must be exactly 26 (29 - 3 unreachable)"
    );

    let kernel = TrustKernel::open_in_memory().unwrap();
    // 创建占位 task 满足 FK
    let task_id = "t-coverage-full";
    {
        let conn = kernel.conn();
        let task = TaskRecord::new(task_id, "coverage full test placeholder");
        TaskRepo::new().create(&conn, &task).unwrap();
    }
    // 直接 emit 25 种事件
    for et in DEFAULT_REACHABLE {
        kernel
            .audit_append_external(task_id, None, et, serde_json::json!({"test": et}))
            .unwrap();
    }
    // 用 with_expected 传入 25 种子集断言 100% 覆盖
    let checker = AuditCoverageChecker::with_expected(&kernel, DEFAULT_REACHABLE);
    let uncovered = checker.uncovered().unwrap();
    assert!(
        uncovered.is_empty(),
        "default feature must cover all 25 reachable event types, uncovered: {:?}",
        uncovered
    );
    let ratio = checker.coverage_ratio().unwrap();
    assert!(
        (ratio - 1.0).abs() < 1e-6,
        "default coverage ratio must be 1.0 (100%), got {}",
        ratio
    );

    // 额外验证:全量 registry(28 种)checker 报告 25/28
    let full_checker = AuditCoverageChecker::new(&kernel);
    let full_uncovered = full_checker.uncovered().unwrap();
    assert_eq!(
        full_uncovered.len(),
        3,
        "full registry (28) must have 3 uncovered (voice_started / stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed), got: {:?}",
        full_uncovered
    );
    assert!(
        full_uncovered.contains(&"voice_started".to_string()),
        "voice_started must be uncovered in default feature"
    );
    assert!(
        full_uncovered.contains(&"stronghold_snapshot_encrypted".to_string()),
        "stronghold_snapshot_encrypted must be uncovered in default feature"
    );
    assert!(
        full_uncovered.contains(&"stronghold_snapshot_decrypt_failed".to_string()),
        "stronghold_snapshot_decrypt_failed must be uncovered in default feature"
    );
}
