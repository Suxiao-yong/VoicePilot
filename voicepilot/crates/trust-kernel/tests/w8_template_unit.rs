//! W8 Plan 1 Task 9: SlotTemplateEngine 集成测试.
//!
//! 覆盖 spec §2.1 模板语法的全部 8 种表达式,
//! 以及 §6 安全约束中的双层校验(LLM 返回后立即校验 + 执行前再次校验)。

use std::collections::HashMap;

use trust_kernel::llm::types::ExtractedSlot;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_types::{DagNode, DagPlan, IterableSource, LoopSpec};
use trust_kernel::skills::template::{
    SlotKind, SlotTemplate, SlotTemplateEngine, TemplateError, TemplateExpr,
};

fn tpl(kind: SlotKind, s: &str) -> SlotTemplate {
    SlotTemplate {
        kind,
        template: SlotTemplateEngine::parse(s).unwrap(),
    }
}

fn node(id: &str, kind: SlotKind, tpl_str: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "test.skill".into(),
        input_template: tpl(kind, tpl_str),
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with_nodes(nodes: Vec<DagNode>) -> DagPlan {
    DagPlan {
        plan_id: "p1".into(),
        user_goal: "test".into(),
        nodes,
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn integration_literal_only() {
    let expr = SlotTemplateEngine::parse("notepad").unwrap();
    let outs = HashMap::new();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
    assert_eq!(v, serde_json::Value::String("notepad".into()));
}

#[test]
fn integration_prev_output_path() {
    // 模拟 n1 输出 {output: {path: "C:/x.txt"}},n2 引用 ${prev.output.path}
    let mut outs = HashMap::new();
    outs.insert(
        "n1".into(),
        serde_json::json!({"output": {"path": "C:/Users/test/x.txt"}}),
    );
    let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
    assert_eq!(v, serde_json::Value::String("C:/Users/test/x.txt".into()));
}

#[test]
fn integration_concat_path() {
    let mut outs = HashMap::new();
    outs.insert(
        "n1".into(),
        serde_json::json!({"output": {"name": "report.md"}}),
    );
    let expr = SlotTemplateEngine::parse("C:/Docs/${n1.output.name}").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
    assert_eq!(v, serde_json::Value::String("C:/Docs/report.md".into()));
}

#[test]
fn integration_user_slot() {
    // ExtractedSlot.kind 是 String(不是 enum)— resolve_var User 分支
    // 用 kind.to_lowercase() == path.to_lowercase() 做匹配,故 ${user.text}
    // 匹配 kind="text"。
    let slots = vec![ExtractedSlot {
        kind: "text".into(),
        raw: "alice".into(),
        high_risk: false,
    }];
    let expr = SlotTemplateEngine::parse("${user.text}").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &HashMap::new(), &slots, None, None).unwrap();
    assert_eq!(v, serde_json::Value::String("alice".into()));
}

#[test]
fn integration_iter_in_loop() {
    let mut specs = HashMap::new();
    specs.insert(
        "n1".into(),
        LoopSpec {
            loop_var: "item".into(),
            iterable_source: IterableSource::Literal(vec!["a.txt".into(), "b.txt".into()]),
            max_iterations: 5,
            break_condition: None,
        },
    );
    let mut p = plan_with_nodes(vec![node("n1", SlotKind::Text, "${item}")]);
    p.loop_specs = specs;
    assert!(SlotTemplateEngine::validate_dag(&p).is_ok());
}

#[test]
fn integration_filter_size_gt() {
    let mut outs = HashMap::new();
    outs.insert(
        "n1".into(),
        serde_json::json!({"output": {"files": [
            {"name":"a","size":100},
            {"name":"b","size":2_000_000},
            {"name":"c","size":5_000_000}
        ]}}),
    );
    let expr = SlotTemplateEngine::parse("${prev.output.files}[?size > 1048576]").unwrap();
    let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
    match v {
        serde_json::Value::Array(a) => assert_eq!(a.len(), 2),
        _ => panic!("expected Array"),
    }
}

#[test]
fn integration_validate_dag_rejects_unknown_step() {
    let p = plan_with_nodes(vec![node("n1", SlotKind::Text, "${n99.output.path}")]);
    let err = SlotTemplateEngine::validate_dag(&p).unwrap_err();
    assert!(matches!(err, TemplateError::UnknownNodeId { .. }));
}

#[test]
fn integration_validate_dag_rejects_iter_in_non_loop() {
    let p = plan_with_nodes(vec![node("n1", SlotKind::Text, "${item}")]);
    let err = SlotTemplateEngine::validate_dag(&p).unwrap_err();
    assert!(matches!(err, TemplateError::VarNotFound { .. }));
}

#[test]
fn integration_double_layer_defense() {
    // 模拟 spec §2.1 双层防御:
    // Layer 1: LLM 返回后立即 validate_dag → 拒绝
    // Layer 2: 执行前 resolve → 失败
    let bad_plan = plan_with_nodes(vec![node("n1", SlotKind::Text, "${n99.unknown}")]);
    // Layer 1
    let err1 = SlotTemplateEngine::validate_dag(&bad_plan).unwrap_err();
    assert!(matches!(err1, TemplateError::UnknownNodeId { node_id } if node_id == "n99"));
    // Layer 2:即使 Layer 1 漏检,resolve 也会失败
    let expr = SlotTemplateEngine::parse("${n99.unknown}").unwrap();
    let err2 = SlotTemplateEngine::resolve(&expr, &HashMap::new(), &[], None, None).unwrap_err();
    assert!(matches!(err2, TemplateError::VarNotFound { .. }));
}

// 用 `TemplateExpr` 占位 — 防止 unused import warning(实际本测试文件不直接构造 enum)
#[test]
fn integration_template_expr_marker() {
    let _e: TemplateExpr = TemplateExpr::Literal("marker".into());
}
