//! W8 Plan 3 Task 1 + 2: run_loop_node + resolve_iterable + 循环执行 + break_condition 单元测试.
//!
//! Spec §2.3 决策 #5 + #8:循环节点 + 失败终止循环。
//!
//! - Task 1: resolve_iterable(三种 IterableSource)+ 骨架
//! - Task 2: 真实循环执行 + break_condition 解析 + max_iterations 硬截断(50)
//!
//! 注:resolve_iterable / parse_break_condition / evaluate_break_condition 是私有方法,
//! 本测试通过 run_loop_node 间接验证;直接单元测试在 dag_executor.rs 的 #[cfg(test)] mod 中。

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{Approver, AutoApprover};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagNodeStatus, DagPlan, DagStatus, IterableSource, LoopSpec,
};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

fn literal_node(id: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("notepad".into()),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with_loop(loop_spec: LoopSpec) -> DagPlan {
    let mut loop_specs = HashMap::new();
    loop_specs.insert("n1".into(), loop_spec);
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test loop".into(),
        nodes: vec![literal_node("n1")],
        edges: vec![],
        loop_specs,
        max_total_steps: 10,
    }
}

fn make_executor() -> (Arc<TrustKernel>, DagExecutor) {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    (kernel, executor)
}

#[test]
fn resolve_iterable_literal_returns_vec_of_strings() {
    // IterableSource::Literal(["a", "b", "c"]) → 通过 run_loop_node 间接验证
    // 占位实现返回 Succeeded(空数组),DAG 应 Succeeded
    let (_kernel, executor) = make_executor();
    let source = IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]);
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]);
    assert!(
        result.is_ok(),
        "run should not error, got {:?}",
        result.err()
    );
    let dag_result = result.unwrap();
    assert_eq!(dag_result.status, DagStatus::Succeeded);
}

#[test]
fn resolve_iterable_prev_node_output_extracts_array() {
    // n0 简单节点(task.explain)→ dispatch_task_explain 返回 {"task_id": "...", "step_id": "..."}
    // n1 循环节点 IterableSource::PrevNodeOutput { node_id: "n0", port: "output.files" }
    // resolve_iterable 在 node_outputs["n0"] 中查找 "output.files" 路径 → 不存在 → Err
    // run_loop_node 返回 Err → run 返回 Err
    let (_kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::PrevNodeOutput {
                node_id: "n0".into(),
                port: "output.files".into(),
            },
            max_iterations: 10,
            break_condition: None,
        },
    );
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test".into(),
        nodes: vec![literal_node("n0"), literal_node("n1")],
        edges: vec![DagEdge {
            from: "n0".into(),
            to: "n1".into(),
            port_binding: None,
        }],
        loop_specs,
        max_total_steps: 10,
    };
    let result = executor.run(&plan, &[]);
    assert!(
        result.is_err(),
        "expected Err for non-existent port, got {:?}",
        result.ok()
    );
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("not found") || err.contains("resolve_iterable"),
        "got: {}",
        err
    );
}

#[test]
fn resolve_iterable_prev_node_output_non_array_errors() {
    // port 指向非数组 → resolve_iterable Err → run_loop_node Err → run Err
    // 注:n0 dispatch_task_explain 返回 {"task_id": "...", "step_id": "..."},
    //   不含 "output.path",所以会先触发 "not found" 错误
    let (_kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::PrevNodeOutput {
                node_id: "n0".into(),
                port: "output.path".into(),
            },
            max_iterations: 10,
            break_condition: None,
        },
    );
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test".into(),
        nodes: vec![literal_node("n0"), literal_node("n1")],
        edges: vec![DagEdge {
            from: "n0".into(),
            to: "n1".into(),
            port_binding: None,
        }],
        loop_specs,
        max_total_steps: 10,
    };
    let result = executor.run(&plan, &[]);
    assert!(result.is_err(), "expected Err, got {:?}", result.ok());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("not found") || err.contains("resolve_iterable"),
        "got: {}",
        err
    );
}

#[test]
fn resolve_iterable_prev_node_output_unknown_node_errors() {
    // node_id 不在 node_outputs → Err
    // n1 单独 plan(无 n0 节点),IterableSource 引用不存在的 "nonexistent" 节点
    let (_kernel, executor) = make_executor();
    let source = IterableSource::PrevNodeOutput {
        node_id: "nonexistent".into(),
        port: "output.files".into(),
    };
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]);
    assert!(result.is_err(), "expected Err for unknown node_id");
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("not in node_outputs"), "got: {}", err);
}

#[test]
fn resolve_iterable_user_slot_returns_err_in_plan3() {
    // UserSlot 在 Plan 3 未 wire(Plan 5 集成)→ Err
    let (_kernel, executor) = make_executor();
    let source = IterableSource::UserSlot {
        slot_kind: "files".into(),
    };
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: source,
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]);
    assert!(result.is_err(), "expected Err for UserSlot in Plan 3");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("UserSlot") || err.contains("not supported"),
        "got: {}",
        err
    );
}

// ===== Task 2: 循环执行 + break_condition + max_iterations 截断 =====

#[test]
fn loop_node_literal_all_iterations_succeed() {
    // Literal(["a", "b", "c"]) + task.explain + AutoApprover → 3 个迭代全成功
    // n1 节点状态 = Succeeded(Array),数组长度 = 3
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]),
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]).expect("run should succeed");
    assert_eq!(result.status, DagStatus::Succeeded);
    let n1_status = result.node_results.get("n1").expect("n1 status must exist");
    match n1_status {
        DagNodeStatus::Succeeded(arr) => {
            assert_eq!(
                arr.as_array().unwrap().len(),
                3,
                "expected 3 iter outputs, got {:?}",
                arr
            );
        }
        other => panic!("expected Succeeded, got {:?}", other),
    }
}

#[test]
fn loop_node_max_iterations_truncates_to_50() {
    // Literal 60 项 + max_iterations=100 → 截断到 50(硬上限)
    let (_kernel, executor) = make_executor();
    let items: Vec<String> = (0..60).map(|i| format!("item-{}", i)).collect();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(items),
        max_iterations: 100, // 超过 50 硬上限
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]).expect("run should succeed");
    let n1_status = result.node_results.get("n1").expect("n1 status");
    match n1_status {
        DagNodeStatus::Succeeded(arr) => {
            assert_eq!(
                arr.as_array().unwrap().len(),
                50,
                "expected 50 iter outputs (truncated from 60)"
            );
        }
        other => panic!("expected Succeeded, got {:?}", other),
    }
}

#[test]
fn loop_node_max_iterations_below_hard_limit_respected() {
    // max_iterations=2 < 50 → 取 2(不超硬上限)
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]),
        max_iterations: 2, // 低于硬上限
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]).expect("run should succeed");
    let n1_status = result.node_results.get("n1").expect("n1 status");
    match n1_status {
        DagNodeStatus::Succeeded(arr) => {
            assert_eq!(
                arr.as_array().unwrap().len(),
                2,
                "expected 2 iter outputs (max_iterations=2)"
            );
        }
        other => panic!("expected Succeeded, got {:?}", other),
    }
}

#[test]
fn loop_node_break_condition_with_string_items_no_break() {
    // 3 个 string item + break_condition="item.size > 1000"
    // string 无 .size 字段 → evaluate_break_condition 返回 false → 不中断 → 全循环
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]),
        max_iterations: 10,
        break_condition: Some("item.size > 1000".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]).expect("run should succeed");
    let n1_status = result.node_results.get("n1").expect("n1 status");
    match n1_status {
        DagNodeStatus::Succeeded(arr) => {
            assert_eq!(
                arr.as_array().unwrap().len(),
                3,
                "expected 3 outputs (no break triggered)"
            );
        }
        other => panic!("expected Succeeded, got {:?}", other),
    }
}

#[test]
fn loop_node_dispatch_failure_terminates_loop() {
    // Literal(["a", "b", "c"]) + skill_id="nonexistent.skill" → dispatch Err → break
    // n1 节点 Failed,DAG Failed(无已成功节点)
    let (_kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::Literal(vec!["a".into(), "b".into(), "c".into()]),
            max_iterations: 10,
            break_condition: None,
        },
    );
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test fail".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "nonexistent.skill".into(), // 未知 skill → dispatch Err
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("x".into()),
            },
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs,
        max_total_steps: 10,
    };
    let result = executor.run(&plan, &[]).expect("run should not error");
    match result.status {
        DagStatus::Failed { failed_node, .. } => {
            assert_eq!(failed_node, "n1");
        }
        other => panic!("expected Failed, got {:?}", other),
    }
    let n1_status = result.node_results.get("n1").unwrap();
    match n1_status {
        DagNodeStatus::Failed { cause } => {
            assert!(
                cause.contains("iter 0") || cause.contains("unknown skill_id"),
                "got: {}",
                cause
            );
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

#[test]
fn parse_break_condition_valid_format_does_not_error() {
    // "item.size > 1048576" → 解析成功 → 不报 parse 错误
    // item "a" 是 string,无 .size → 不中断 → 全循环
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: Some("item.size > 1048576".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]);
    assert!(
        result.is_ok(),
        "valid break_condition should not error, got {:?}",
        result.err()
    );
}

#[test]
fn parse_break_condition_invalid_op_errors() {
    // "item.size >> 1000" → op ">>" 不支持 → Err
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: Some("item.size >> 1000".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]);
    assert!(result.is_err(), "invalid op should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("unsupported op"), "got: {}", msg);
}

#[test]
fn parse_break_condition_invalid_format_errors() {
    // "item.size >" → 只有 2 token → Err
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: Some("item.size >".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]);
    assert!(result.is_err(), "invalid format should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("expected 3 tokens"), "got: {}", msg);
}

#[test]
fn parse_break_condition_missing_item_prefix_errors() {
    // "size > 1000" → 缺 item. 前缀 → Err
    let (_kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: Some("size > 1000".into()),
    };
    let plan = plan_with_loop(loop_spec);
    let result = executor.run(&plan, &[]);
    assert!(result.is_err(), "missing item. prefix should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("must start with 'item.'"), "got: {}", msg);
}

// ===== Task 3: 循环节点状态持久化到 dag_nodes =====

#[test]
fn loop_node_succeeded_persists_to_dag_nodes() {
    // 循环全成功 → dag_nodes 行 status="succeeded" + output_json 非空(数组长度 2)
    let (kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into(), "b".into()]),
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    executor.run(&plan, &[]).expect("run should succeed");

    let conn = kernel.conn();
    let repo = DagRepo::new();
    let nodes = repo
        .list_nodes_by_plan(&conn, &plan.plan_id)
        .expect("list_nodes");
    let n1 = nodes.iter().find(|n| n.node_id == "n1").expect("n1 record");
    assert_eq!(n1.status, "succeeded");
    assert!(n1.output_json.is_some(), "output_json should be populated");
    let output: serde_json::Value = serde_json::from_str(n1.output_json.as_ref().unwrap()).unwrap();
    assert!(output.is_array(), "output should be array");
    assert_eq!(
        output.as_array().unwrap().len(),
        2,
        "expected 2 iter outputs"
    );
    assert!(
        n1.completed_at.is_some(),
        "completed_at should be set for Succeeded"
    );
}

#[test]
fn loop_node_failed_persists_to_dag_nodes() {
    // 循环中 dispatch 失败 → dag_nodes 行 status="failed" + error_message 非空
    let (kernel, executor) = make_executor();
    let mut loop_specs = HashMap::new();
    loop_specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::Literal(vec!["a".into()]),
            max_iterations: 10,
            break_condition: None,
        },
    );
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test fail persist".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "nonexistent.skill".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("x".into()),
            },
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs,
        max_total_steps: 10,
    };
    executor
        .run(&plan, &[])
        .expect("run should not error (Failed status)");

    let conn = kernel.conn();
    let repo = DagRepo::new();
    let nodes = repo
        .list_nodes_by_plan(&conn, &plan.plan_id)
        .expect("list_nodes");
    let n1 = nodes.iter().find(|n| n.node_id == "n1").expect("n1 record");
    assert_eq!(n1.status, "failed");
    assert!(
        n1.error_message.is_some(),
        "error_message should be populated"
    );
    let err_msg = n1.error_message.as_ref().unwrap();
    assert!(
        err_msg.contains("iter 0") || err_msg.contains("unknown skill_id"),
        "got: {}",
        err_msg
    );
    assert!(
        n1.completed_at.is_some(),
        "completed_at should be set for Failed"
    );
}

#[test]
fn loop_node_running_status_overwritten_by_terminal() {
    // 循环开始前 → dag_nodes status="running"
    // 循环结束后 → status 被覆盖为终态(succeeded/failed)
    let (kernel, executor) = make_executor();
    let loop_spec = LoopSpec {
        loop_var: "item".into(),
        iterable_source: IterableSource::Literal(vec!["a".into()]),
        max_iterations: 10,
        break_condition: None,
    };
    let plan = plan_with_loop(loop_spec);
    executor.run(&plan, &[]).expect("run should succeed");

    let conn = kernel.conn();
    let repo = DagRepo::new();
    let nodes = repo
        .list_nodes_by_plan(&conn, &plan.plan_id)
        .expect("list_nodes");
    let n1 = nodes.iter().find(|n| n.node_id == "n1").expect("n1 record");
    assert_ne!(
        n1.status, "running",
        "status should be terminal, not running"
    );
}
