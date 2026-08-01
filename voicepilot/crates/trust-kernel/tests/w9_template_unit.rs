//! W9 Plan 6 Task 4 — Slot 流水闭合单元测试。
//!
//! 验证 Task 1-2 的闭合工作:
//! 1. IterableSource::UserSlot 能从 user_slots 解析为 Vec<Value>(JSON 数组)
//! 2. IterableSource::UserSlot 支持 CSV fallback("a,b,c" → 3 元素)
//! 3. IterableSource::UserSlot 在 user_slots=[] 时返回 Err(向后兼容)
//! 4. SlotTemplateEngine::resolve 能透传 user_slots 解析 ${user.xxx}
//! 5. DagExecutor::run(plan, &[]) 行为等价 W8 run(plan)(空 user_slots)
//!
//! 类型说明(核实自 src/llm/types.rs:23-30 + src/skills/dag_types.rs:50-70):
//! - `ExtractedSlot.kind` 是 `String`(不是 `SlotKind` enum),值为 "files" / "text" 等
//! - `LoopSpec` 字段:`loop_var` / `iterable_source` / `max_iterations` / `break_condition`(无 `body_template`)
//! - `IterableSource::UserSlot` 是 struct variant:`UserSlot { slot_kind: String }`

#![cfg(feature = "llm")]

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::types::ExtractedSlot;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagNode, DagPlan, DagStatus, IterableSource, LoopSpec,
};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

/// 构造 literal SlotTemplate(用于直接构造 DagPlan 的 input_template)。
/// 复用 w8_e2e_dag_smoke.rs 同名 helper 模式。
fn literal_text_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Literal(value.to_string()),
    }
}

/// 构造 task.explain 节点(input_template 是 JSON 字符串字面量,含 limit 字段)。
/// task.explain 在 W9 Plan 4 测试中已验证可在无 LLM 真实调用下 Succeeded(返回空 list)。
fn task_explain_node(node_id: &str, limit: u32) -> DagNode {
    DagNode {
        node_id: node_id.into(),
        skill_id: "task.explain".into(),
        input_template: literal_text_template(&format!(r#"{{"limit": {}}}"#, limit)),
        risk_ceiling: ELevel::E0,
    }
}

/// 构造含 UserSlot 循环的 DagPlan(slot_kind 用 "files",与 user_slots.kind 匹配)。
fn plan_with_user_slot_loop(plan_id: &str, slot_kind: &str) -> DagPlan {
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".to_string(),
        LoopSpec {
            loop_var: "item".to_string(),
            iterable_source: IterableSource::UserSlot {
                slot_kind: slot_kind.to_string(),
            },
            max_iterations: 10,
            break_condition: None,
        },
    );
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "W9 Plan 6 测试 UserSlot iterable".into(),
        nodes: vec![task_explain_node("n1", 1)],
        edges: vec![],
        loop_specs,
        max_total_steps: 5,
    }
}

/// 测试 1:IterableSource::UserSlot 从 user_slots 解析 JSON 数组。
/// 这是 Task 2 的核心行为 — 闭合 W8 "user_slots not wired" 欠债。
#[test]
fn user_slot_iterable_resolves_json_array() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    // user_slots 含一个 kind="files" 的 slot,raw 是 JSON 数组
    // ExtractedSlot.kind 是 String(见 src/llm/types.rs:23-30)
    let user_slots = vec![ExtractedSlot {
        kind: "files".to_string(),
        raw: r#"["a.txt","b.txt","c.txt"]"#.to_string(),
        high_risk: false,
    }];

    let plan = plan_with_user_slot_loop("w9p6-test-userslot-json", "files");

    let result = executor
        .run(&plan, &user_slots)
        .expect("run must not infra-error");
    // 循环节点应 Succeeded(3 次 body 执行)
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "expected Succeeded, got {:?}",
        result.status
    );
}

/// 测试 2:IterableSource::UserSlot CSV fallback。
/// slot.raw 不是合法 JSON 数组时,按 CSV 分隔解析(Task 2 Step 2 的 fallback 分支)。
#[test]
fn user_slot_iterable_csv_fallback() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    let user_slots = vec![ExtractedSlot {
        kind: "files".to_string(),
        raw: "a.txt,b.txt,c.txt".to_string(), // 非 JSON,CSV 格式
        high_risk: false,
    }];

    let plan = plan_with_user_slot_loop("w9p6-test-userslot-csv", "files");

    let result = executor
        .run(&plan, &user_slots)
        .expect("run must not infra-error");
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "CSV fallback should produce 3 items and Succeeded, got {:?}",
        result.status
    );
}

/// 测试 3:IterableSource::UserSlot 空数组返回 Err(向后兼容)。
/// user_slots=[] 时 IterableSource::UserSlot 找不到匹配 slot →
/// `resolve_iterable` 返回 Err → `run_loop_node` 传播 Err → `run` 返回 Err。
/// 这保证 Task 3 的 run(plan, &[]) 不会破坏既有语义(空 user_slots 不静默 Succeeded)。
#[test]
fn user_slot_iterable_empty_user_slots_returns_err() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    let plan = plan_with_user_slot_loop("w9p6-test-userslot-empty", "files");

    // user_slots = &[](Task 3 既有调用点的等价行为)
    let result = executor.run(&plan, &[]);
    // 循环节点应返回 Err(resolve_iterable UserSlot not found)
    assert!(
        result.is_err(),
        "empty user_slots should produce Err from resolve_iterable, got Ok({:?})",
        result.as_ref().ok()
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("UserSlot") && msg.contains("files"),
        "error should mention UserSlot kind 'files', got: {}",
        msg
    );
}

/// 测试 4:${user.xxx} 模板变量能从 user_slots 解析。
/// 验证 Task 1 透传 user_slots 到 SlotTemplateEngine::resolve 后,
/// 简单节点的 input_template 能引用 ${user.files}。
#[test]
fn resolve_template_with_user_slot_var() {
    use trust_kernel::skills::template::{VarRef, VarScope};

    let template = SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Var(VarRef {
            scope: VarScope::User,
            path: "files".into(),
        }),
    };

    let user_slots = vec![ExtractedSlot {
        kind: "files".to_string(),
        raw: "hello from user slot".to_string(),
        high_risk: false,
    }];

    let node_outputs = HashMap::new();
    let resolved = trust_kernel::skills::template::SlotTemplateEngine::resolve(
        &template.template,
        &node_outputs,
        &user_slots,
        None,
        None,
    )
    .expect("resolve must succeed");

    // resolved 应为 "hello from user slot"(字符串)
    assert_eq!(resolved, serde_json::json!("hello from user slot"));
}

/// 测试 5:DagExecutor::run(plan, &[]) 行为等价 W8。
/// 用 task.explain 节点(无 ${user.xxx} 依赖)验证空 user_slots 不破坏既有路径。
#[test]
fn run_with_empty_user_slots_equivalent_to_w8() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let executor = DagExecutor::new(
        Arc::new(kernel),
        Arc::new(AutoApprover),
        Arc::new(DagRepo::new()),
    );

    let plan = DagPlan {
        plan_id: "w9p6-test-empty-equivalent".into(),
        user_goal: "test empty user_slots equivalence".into(),
        nodes: vec![task_explain_node("n1", 1)],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };

    let result = executor
        .run(&plan, &[])
        .expect("run must not infra-error");
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "empty user_slots with no ${{user.xxx}} dependency should Succeed, got {:?}",
        result.status
    );
}
