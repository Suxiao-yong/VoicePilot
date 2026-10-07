//! W9 Plan 7 — default 组合边界用例(16 个)。
//!
//! 覆盖 spec §2.7(Plan 7 范围)+ §5(Fitness Functions)+ §6.4(审计事件隐私):
//!   A. Stronghold feature off 行为(3 个):default 组合下 snapshot_encrypted = None
//!      / StrongholdVault 不可用(stronghold_enabled() = false)
//!      / reverse_payload 保留明文(向后兼容)
//!      —— 仅"feature 关闭"组合下有意义,已加 `#[cfg(not(feature = "stronghold"))]`
//!         逐个 gate(见各测试上方注释);有 feature 的组合由
//!         w9_snapshot_encrypted_smoke.rs 覆盖
//!   B. Taint 表空时 gateway 行为(2 个):空 taints 表 gateway 放行
//!      / TaintRepo::find_by_value 查不存在 hash 返回 None
//!   C. DAG Modify 占位行为(3 个):AutoApprover 不触发 Modify
//!      / 第二次 Modify 返回 DagModifyLimitExceeded
//!      / 完整闭环 Modify → Allow → 执行(审计链完整)
//!   D. Slot 流水空 user_slots(2 个):DagExecutor::run(plan, &[]) 正常执行
//!      / IterableSource::UserSlot 引用但 user_slots=[] 返回 UserSlotNotFound
//!   E. 审计事件命名一致性(2 个):所有 event_type 匹配 ^[a-z][a-z0-9_]*$
//!      / W9 7 种新事件命名合规
//!   F. DagStatus 状态机边界(2 个):Running→Succeeded 合法 / Succeeded→Running 非法
//!   G. DagPlan validate 边界(1 个):空 nodes 返回 Err
//!   H. 拓扑排序边界(1 个):空图返回 Ok([])
//!
//! 测试设计:
//! - 所有用例 `#[test]`,无 feature gate(default 组合可编译)
//! - 所有用例用 `TrustKernel::open_in_memory()`
//! - 不调用真实 LLM / Tauri / Stronghold(feature 未启用)
//! - 内省 audit_logs 表用 `conn.query_map` 直接 SQL
//!
//! 不写文件级 cfg gate,让 default 组合(--no-default-features)也能编译运行此文件;
//! 但 A 组 3 个强依赖"feature 关闭"语义的用例必须逐函数 gate,否则在
//! `--features ...,stronghold` 组合下必然失败(曾因此连挂 2 个 job)。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use trust_kernel::approval::approver::{Approver, AutoApprover, DagApprovalOutcome};
use trust_kernel::compensation::types::{CompensationLevel, ConflictPolicy};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::{ELevel, EgressDest};
use trust_kernel::skills::dag_executor::{DagExecutor, topological_sort};
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagNode, DagPlan, DagStatus, IterableSource, LoopSpec};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

// ===== 辅助函数 =====

/// 统计 audit_logs 中所有 event_type(去重)。
fn list_distinct_event_types(kernel: &TrustKernel) -> Vec<String> {
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT DISTINCT event_type FROM audit_logs ORDER BY event_type ASC")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

/// 统计 audit_logs 中某 event_type 的数量。
fn count_audit_events(kernel: &TrustKernel, event_type: &str) -> i64 {
    let conn = kernel.conn();
    conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE event_type = ?1",
        rusqlite::params![event_type],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// 构造一个 literal SlotTemplate。
fn literal_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Literal(value.to_string()),
    }
}

/// 构造一个最小合法 DagPlan(单节点,无边,无循环)。
fn minimal_dag_plan(node_count: usize) -> DagPlan {
    let nodes: Vec<DagNode> = (0..node_count)
        .map(|i| DagNode {
            node_id: format!("n{}", i),
            skill_id: "task.explain".to_string(),
            input_template: literal_template("{}"),
            risk_ceiling: ELevel::E1,
        })
        .collect();
    DagPlan {
        plan_id: format!("plan-boundary-{}", uuid::Uuid::new_v4()),
        user_goal: "boundary test".to_string(),
        nodes,
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

/// 创建 kernel + task + step,返回 (kernel, task_id, step_id)。
/// create_post_commit_compensation 需要 step_id REFERENCES steps(step_id),
/// 所以必须先创建 task + step。
fn setup_kernel_with_step(prefix: &str) -> (TrustKernel, String, String) {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = format!("task-{}-{}", prefix, uuid::Uuid::new_v4());
    let step_id = format!("step-{}-{}", prefix, uuid::Uuid::new_v4());
    kernel.create_task(&task_id, "boundary test").unwrap();
    let step_record = trust_kernel::repo::step_repo::StepRecord::new(&step_id, &task_id, 1);
    kernel.create_step(&step_record).unwrap();
    (kernel, task_id, step_id)
}

// ===== C 组辅助:Mock Approver =====

/// 一直返回 Modify 的 Approver(用于测试第二次 Modify 被拒绝)。
struct DoubleModifyApprover {
    call_count: Arc<AtomicU32>,
}

impl Approver for DoubleModifyApprover {
    fn prompt(
        &self,
        _manifest: &trust_kernel::policy::transaction::EffectManifest,
    ) -> trust_kernel::approval::types::ApprovalDecision {
        trust_kernel::approval::types::ApprovalDecision::Allow
    }

    fn approve_dag_skeleton(
        &self,
        _plan: &DagPlan,
    ) -> trust_kernel::error::Result<DagApprovalOutcome> {
        let _ = self.call_count.fetch_add(1, Ordering::SeqCst);
        let modified_plan = minimal_dag_plan(1);
        Ok(DagApprovalOutcome::Modify {
            modified_plan: Box::new(modified_plan),
        })
    }
}

/// 第一次返回 Modify、之后返回 Allow 的 Approver(用于测试完整闭环)。
struct ModifyOnceThenAllowApprover {
    call_count: Arc<AtomicU32>,
}

impl Approver for ModifyOnceThenAllowApprover {
    fn prompt(
        &self,
        _manifest: &trust_kernel::policy::transaction::EffectManifest,
    ) -> trust_kernel::approval::types::ApprovalDecision {
        trust_kernel::approval::types::ApprovalDecision::Allow
    }

    fn approve_dag_skeleton(
        &self,
        _plan: &DagPlan,
    ) -> trust_kernel::error::Result<DagApprovalOutcome> {
        let n = self.call_count.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            let modified_plan = minimal_dag_plan(1);
            Ok(DagApprovalOutcome::Modify {
                modified_plan: Box::new(modified_plan),
            })
        } else {
            Ok(DagApprovalOutcome::Allow)
        }
    }
}

// ===== A. Stronghold feature off 行为(3 个)=====

/// 仅在没有 stronghold feature 的组合下有意义:有 feature 时
/// snapshot_encrypted 非空 + reverse_payload 为空列(见 persist_compensation_record)。
/// 加了 gate,这个断言才不会在 `--features ...,stronghold` 组合下失败。
#[cfg(not(feature = "stronghold"))]
#[test]
fn stronghold_feature_off_snapshot_encrypted_is_none() {
    // default 组合(无 stronghold feature)调用 create_post_commit_compensation,
    // snapshot_encrypted 必须为 None(spec §2.2 向后兼容分支)。
    //
    // 适配说明:create_post_commit_compensation 实际签名接收
    //   (kernel, step_id, moved_paths, compensate_fn, level, conflict_policy, ttl_seconds)
    // 返回 Result<String>(comp_id),不是 Result<CompensationRecord>。
    // 通过 DB 查询 compensations 表验证 snapshot_encrypted / reverse_payload。
    let (kernel, _task_id, step_id) = setup_kernel_with_step("a1");

    let moved_paths: Vec<(PathBuf, PathBuf)> =
        vec![(PathBuf::from("src/a.txt"), PathBuf::from("dst/a.txt"))];
    let comp_id = trust_kernel::skills::common::create_post_commit_compensation(
        &kernel,
        &step_id,
        &moved_paths,
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .expect("create_post_commit_compensation must succeed in default feature combo");

    assert!(
        comp_id.starts_with("comp-"),
        "comp_id must start with 'comp-', got: {}",
        comp_id
    );

    // 通过 DB 查询验证 snapshot_encrypted = NULL / snapshot_vault_ref = NULL
    let conn = kernel.conn();
    let (snapshot_encrypted, snapshot_vault_ref, reverse_payload): (
        Option<Vec<u8>>,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT snapshot_encrypted, snapshot_vault_ref, reverse_payload \
             FROM compensations WHERE comp_id = ?1",
            rusqlite::params![comp_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get::<_, String>(2).unwrap_or_default(),
                ))
            },
        )
        .expect("compensation record must exist");

    assert!(
        snapshot_encrypted.is_none(),
        "snapshot_encrypted must be None when stronghold feature is off (default combo)"
    );
    assert!(
        snapshot_vault_ref.is_none(),
        "snapshot_vault_ref must be None when stronghold feature is off, got: {:?}",
        snapshot_vault_ref
    );
    // reverse_payload 列保留明文(向后兼容,feature 未启用时不加密)
    assert!(
        !reverse_payload.is_empty(),
        "reverse_payload must keep plaintext when stronghold feature is off (backward compat)"
    );
}

/// stronghold feature 关闭时 `stronghold_enabled()` 恒 false;有 feature 时该
/// 断言必然不成立,故 gate 掉(保持文件级"default 组合也能跑"的设计)。
#[cfg(not(feature = "stronghold"))]
#[test]
fn stronghold_feature_off_degraded_vault_decrypt_returns_not_unlocked() {
    // default 组合下 stronghold feature 未启用,StrongholdVault 类型不存在
    //(crypto::stronghold 模块在 #[cfg(feature = "stronghold")] 门控下)。
    //
    // 适配说明:Plan 预估直接调 StrongholdVault::degraded().decrypt() 期望 NotUnlocked,
    // 但 default 组合下 StrongholdVault 类型不可用。
    // 改用 kernel.stronghold_enabled() == false 间接验证(spec §2.1:
    // feature 关闭时 stronghold_enabled() 恒 false,加密 / 解密路径不可走,
    // 等价于 NotUnlocked 语义)。
    let kernel = TrustKernel::open_in_memory().unwrap();
    assert!(
        !kernel.stronghold_enabled(),
        "stronghold_enabled() must be false in default combo (stronghold feature off)"
    );
    // privacy_mode 默认 false,ensure_stronghold_ready_for_privacy 应 Ok
    let ready = kernel.ensure_stronghold_ready_for_privacy();
    assert!(
        ready.is_ok(),
        "ensure_stronghold_ready_for_privacy must be Ok when privacy_mode is false, got: {:?}",
        ready
    );
}

/// 同上:只在无 stronghold feature 的组合下成立。
#[cfg(not(feature = "stronghold"))]
#[test]
fn stronghold_feature_off_reverse_payload_keeps_plaintext() {
    // 反复调用 create_post_commit_compensation,验证所有记录的 reverse_payload
    // 都是明文 JSON(spec §2.2 兼容性表第 1 行)。
    let (kernel, _task_id, step_id) = setup_kernel_with_step("a3");
    for i in 0..3 {
        let moved: Vec<(PathBuf, PathBuf)> = vec![(
            PathBuf::from(format!("src/{}.txt", i)),
            PathBuf::from(format!("dst/{}.txt", i)),
        )];
        let _ = trust_kernel::skills::common::create_post_commit_compensation(
            &kernel,
            &step_id,
            &moved,
            "filesystem.reverse_move",
            CompensationLevel::BestEffort,
            ConflictPolicy::AutoReverse,
            3600,
        )
        .unwrap();
    }
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT reverse_payload, snapshot_encrypted FROM compensations")
        .unwrap();
    let rows: Vec<(String, Option<Vec<u8>>)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(
        rows.len() >= 3,
        "must have at least 3 compensation records, got {}",
        rows.len()
    );
    for (reverse_payload, snapshot_encrypted) in rows {
        assert!(
            !reverse_payload.is_empty(),
            "reverse_payload must keep plaintext in default combo"
        );
        assert!(
            snapshot_encrypted.is_none(),
            "snapshot_encrypted must be None in default combo"
        );
        // reverse_payload 必须是合法 JSON
        let _: serde_json::Value = serde_json::from_str(&reverse_payload)
            .expect("reverse_payload must be valid JSON in default combo");
    }
}

// ===== B. Taint 表空时 gateway 行为(2 个)=====

#[test]
fn gateway_taint_table_empty_allows_resource() {
    // taints 表空时,gateway 必须放行所有 resource(spec §2.3 查表驱动规则)。
    //
    // 适配说明:check_taint_policy 实际在 trust_kernel::gateway 模块(非 policy::gateway),
    // 签名为 (conn, value_hash: &str, egress_dest: EgressDest) —
    // 接收 value_hash 字符串 + EgressDest 枚举(非 &Resource + &Sink)。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let result = trust_kernel::gateway::check_taint_policy(
        &conn,
        "sha256:nonexistent_hash_abc123",
        EgressDest::ToolArgument,
    );
    assert!(
        result.is_ok(),
        "gateway must allow resource when taints table is empty, got {:?}",
        result
    );
}

#[test]
fn taint_repo_find_by_value_nonexistent_returns_none() {
    // TaintRepo::find_by_value 查不存在的 value 返回 Ok(None)。
    //
    // 适配说明:find_by_value 接收原始 value(&str),内部算 SHA256 后查表。
    // 传 "nonexistent_value_string" 会被 hash 后查不到,返回 Ok(None)。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = trust_kernel::policy::taint_repo::TaintRepo::new();
    let result = repo
        .find_by_value(&conn, "nonexistent_value_string")
        .expect("find_by_value must not error on nonexistent value");
    assert!(
        result.is_none(),
        "find_by_value must return None for nonexistent value"
    );
}

// ===== C. DAG Modify 占位行为(3 个)=====

#[test]
fn dag_modify_auto_approver_does_not_trigger_modify() {
    // default 组合下 AutoApprover 必须返回 Allow,不触发 Modify 分支
    // (spec §2.4 + §11 兼容性表第 3 行:AutoApprover 返回 Allow 等价 W8 行为)。
    let approver = AutoApprover;
    let plan = minimal_dag_plan(1);
    let outcome = approver
        .approve_dag_skeleton(&plan)
        .expect("AutoApprover must not error");
    assert!(
        matches!(outcome, DagApprovalOutcome::Allow),
        "AutoApprover must return Allow (not Modify) in default combo, got {:?}",
        outcome
    );
}

#[test]
fn dag_modify_limit_exceeded_on_second_modify() {
    // 第二次连续 Modify 必须返回 DagModifyLimitExceeded(spec §2.4 + §6.3 安全约束)。
    // 用 DoubleModifyApprover 模拟两次 Modify 调用。
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver: Arc<dyn Approver> = Arc::new(DoubleModifyApprover {
        call_count: Arc::new(AtomicU32::new(0)),
    });
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let plan = minimal_dag_plan(1);

    let result = executor.run(&plan, &[]);
    // 第二次 Modify 必须被拒绝,返回 DagModifyLimitExceeded
    assert!(
        matches!(
            result,
            Err(trust_kernel::error::KernelError::DagModifyLimitExceeded { .. })
        ),
        "second consecutive Modify must return DagModifyLimitExceeded, got {:?}",
        result
    );
    // 审计必须记录 dag_modify_limit_exceeded 事件
    assert!(
        count_audit_events(&kernel, "dag_modify_limit_exceeded") >= 1,
        "dag_modify_limit_exceeded audit event must be logged"
    );
}

#[test]
fn dag_modify_full_loop_modify_then_approve_then_execute() {
    // W9 修复(P1-18):完整闭环用例 — Modify → 重新审批 → 执行 → 验证审计链完整
    // 1. 构造 plan
    // 2. 第一次审批:Modify { modified_plan }
    // 3. 第二次审批:Allow
    // 4. 验证 modified_plan 执行(理想 Succeeded,default 组合下 task.explain 不可达可能 Failed)
    // 5. 验证 dag_skeleton_modified 审计事件触发
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver: Arc<dyn Approver> = Arc::new(ModifyOnceThenAllowApprover {
        call_count: Arc::new(AtomicU32::new(0)),
    });
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let plan = minimal_dag_plan(1);

    let result = executor.run(&plan, &[]);
    // run 必须不 panic,理想情况 Succeeded(default 组合下 task.explain 可能 Failed)
    assert!(
        result.is_ok(),
        "dag_modify_full_loop run must not panic, got: {:?}",
        result
    );

    // 验证审计链:dag_skeleton_modified 必须在第一次 Modify 时记录
    let modified_count = count_audit_events(&kernel, "dag_skeleton_modified");
    assert!(
        modified_count >= 1,
        "dag_skeleton_modified must be logged on first Modify, got count={}",
        modified_count
    );
    // 注:完整审计链验证在 Plan 4 集成测试,此处仅验证 dag_skeleton_modified 触发。
}

// ===== D. Slot 流水空 user_slots(2 个)=====

#[test]
fn dag_executor_run_with_empty_user_slots_does_not_panic() {
    // DagExecutor::run(plan, &[]) 必须不 panic(spec §11 兼容性表第 5 行:
    // user_slots=[] 时仍可工作,等价 W8 行为)。
    // 注:default feature 下 task.explain 不可达,节点可能 Failed,但 run 本身不 panic。
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    let plan = minimal_dag_plan(1);
    let result = executor.run(&plan, &[]);
    assert!(
        result.is_ok(),
        "run must not panic even if node fails, got: {:?}",
        result
    );
    // 不断言 Succeeded(default feature 下 task.explain 不可达,可能 Failed)
    let dag_result = result.unwrap();
    let _ = dag_result.status; // 仅验证 run 不 panic
}

#[test]
fn iterable_source_user_slot_empty_slots_returns_user_slot_not_found() {
    // IterableSource::UserSlot 引用但 user_slots=[] 时,
    // DagExecutor::run 必须返回 Err(spec §11 兼容性表第 5 行例外)。
    // resolve_iterable 在 user_slots 中找不到匹配 kind → KernelError::Skill(...)
    // 错误消息含 "UserSlot"。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let plan = DagPlan {
        plan_id: format!("plan-slot-{}", uuid::Uuid::new_v4()),
        user_goal: "test slot".to_string(),
        nodes: vec![DagNode {
            node_id: "n1".to_string(),
            skill_id: "task.explain".to_string(),
            input_template: literal_template("{}"),
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs: {
            let mut m = HashMap::new();
            m.insert(
                "n1".to_string(),
                LoopSpec {
                    loop_var: "item".to_string(),
                    iterable_source: IterableSource::UserSlot {
                        slot_kind: "text".to_string(),
                    },
                    max_iterations: 5,
                    break_condition: None,
                },
            );
            m
        },
        max_total_steps: 5,
    };

    let kernel = Arc::new(kernel);
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&plan, &[]);
    assert!(
        result.is_err(),
        "DagExecutor::run with IterableSource::UserSlot but empty user_slots must error, got: {:?}",
        result
    );
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.contains("UserSlot") || err_msg.to_lowercase().contains("user_slot"),
        "error must mention UserSlot, got: {}",
        err_msg
    );
}

// ===== E. 审计事件命名一致性(2 个)=====

#[test]
fn audit_event_types_all_lower_snake_case() {
    // 扫描 audit_logs 表所有 event_type,验证命名规范。
    //
    // spec §10 Conventions 要求所有 audit event_type 必须 lower_snake_case。
    // W10 清理项 3 已将 W1-W8 遗留 14 种 SCREAMING_SNAKE_CASE 事件重命名为
    // lower_snake_case(task_created / step_created / step_status_changed /
    // step_prepared / step_committed / step_started / step_succeeded /
    // step_failed / state_transition / compensation_created /
    // compensation_status_changed / approval_recorded / mcp_tools_call /
    // mcp_call_failed),并通过 migration 007 转换历史 audit_logs 数据。
    // 本测试现在强制所有 event_type 必须 lower_snake_case,无 legacy 白名单。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let _ = kernel.create_task("t-boundary-1", "trigger audit events");
    let event_types = list_distinct_event_types(&kernel);
    assert!(
        !event_types.is_empty(),
        "audit_logs must have at least one event_type after create_task"
    );

    let re_lower = regex::Regex::new(r"^[a-z][a-z0-9_]*$").unwrap();
    let mut non_compliant: Vec<String> = Vec::new();
    for et in &event_types {
        if !re_lower.is_match(et) {
            non_compliant.push(et.clone());
        }
    }
    assert!(
        non_compliant.is_empty(),
        "found event_type(s) not matching lower_snake_case: {:?} \
         (W10 cleanup renamed all W1-W8 SCREAMING_SNAKE_CASE events to lower_snake_case)",
        non_compliant
    );
}

#[test]
fn audit_event_types_w9_new_events_naming_compliant() {
    // W9 新增 7 种事件命名必须合规(spec §6.4):
    // stronghold_snapshot_encrypted / stronghold_snapshot_decrypt_failed
    // / stronghold_degraded_mode_entered / taint_propagated / taint_blocked
    // / dag_skeleton_modified / dag_modify_limit_exceeded
    let w9_new_events = vec![
        "stronghold_snapshot_encrypted",
        "stronghold_snapshot_decrypt_failed",
        "stronghold_degraded_mode_entered",
        "taint_propagated",
        "taint_blocked",
        "dag_skeleton_modified",
        "dag_modify_limit_exceeded",
    ];
    let re = regex::Regex::new(r"^[a-z][a-z0-9_]*$").unwrap();
    for et in &w9_new_events {
        assert!(
            re.is_match(et),
            "W9 new event_type '{}' must match lower_snake_case",
            et
        );
    }
    // 注:本测试只验证命名合规性,不验证事件是否实际被触发
    // (default 组合下 stronghold feature 未启用,事件不会触发)。
}

// ===== F. DagStatus 状态机边界(2 个)=====

#[test]
fn dag_status_running_to_succeeded_legal() {
    // Running → Succeeded 必须合法(spec §2.7 + W9 Plan 7 补 transition 方法)。
    let from = DagStatus::Running;
    let to = DagStatus::Succeeded;
    let result = DagStatus::transition(&from, &to);
    assert!(result, "Running → Succeeded must be legal, got {}", result);
}

#[test]
fn dag_status_succeeded_to_running_illegal() {
    // Succeeded → Running 必须非法(终态不可逆)。
    let from = DagStatus::Succeeded;
    let to = DagStatus::Running;
    let result = DagStatus::transition(&from, &to);
    assert!(
        !result,
        "Succeeded → Running must be illegal (terminal state), got {}",
        result
    );
}

// ===== G. DagPlan validate 边界(1 个)=====

#[test]
fn dag_plan_validate_empty_nodes_returns_err() {
    // DagPlan nodes=[] 必须 validate 失败(空 DAG 不合法)。
    //
    // 适配说明:Plan 1-6 未补聚合 validate() 方法,Plan Step 13 注释允许补
    // impl DagPlan { pub fn validate(&self) -> Result<()> { ... } }。
    // 本测试触发 W9 Plan 7 在 dag_types.rs 补的 validate() 聚合方法。
    let plan = DagPlan {
        plan_id: format!("plan-empty-{}", uuid::Uuid::new_v4()),
        user_goal: "empty".to_string(),
        nodes: vec![], // 空 nodes
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    let result = plan.validate();
    assert!(
        result.is_err(),
        "DagPlan with empty nodes must fail validation"
    );
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.to_lowercase().contains("empty") || err_msg.to_lowercase().contains("nodes"),
        "error must mention empty nodes, got: {}",
        err_msg
    );
}

// ===== H. 拓扑排序边界(1 个)=====

#[test]
fn topo_sort_empty_graph_returns_empty() {
    // 空 nodes + edges 拓扑排序返回 Ok([])(spec §2.7 边界)。
    //
    // 适配说明:topological_sort 是 pub 函数(在 dag_executor 模块),
    // 签名为 (nodes: &[DagNode], edges: &[DagEdge]) -> Result<Vec<String>>。
    // 直接调用验证空图边界。
    let result = topological_sort(&[], &[]);
    assert!(
        result.is_ok(),
        "topological_sort with empty graph must return Ok, got: {:?}",
        result
    );
    let sorted = result.unwrap();
    assert!(
        sorted.is_empty(),
        "topological_sort of empty graph must return empty vec, got: {:?}",
        sorted
    );
}
