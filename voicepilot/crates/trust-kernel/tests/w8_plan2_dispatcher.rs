//! W8 Plan 2 Task 2: dispatch_skill_executor 路由测试.
//!
//! 覆盖 9 路 skill_id 命中 + 1 路未知 skill_id 报错。
//! W8 Plan 3:`form.submit` 占位已替换为真实 dispatch,本 plan 测试
//! 改为断言"不再是 not implemented"。
//! uia 相关测试受 `windows + uia` feature 门控,默认 feature 下跳过。

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;

fn kernel() -> TrustKernel {
    TrustKernel::open_in_memory().expect("open_in_memory")
}

#[test]
fn dispatch_unknown_skill_id_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "nonexistent.skill",
        &kernel,
        &input,
        &approver,
        "task-x",
        "step-x",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("unknown skill_id"), "got: {}", msg);
}

#[test]
fn dispatch_form_submit_routes_to_executor_in_plan3() {
    // Plan 3: form.submit 不再返回 "not implemented",而是路由到 execute_form_submit
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"url": "https://example.com", "submit_selector": "button[type=submit]"});
    let result = dispatch_skill_executor(
        "form.submit",
        &kernel,
        &input,
        &approver,
        "task-submit-plan2",
        "step-submit-plan2",
    );
    // Plan 3:可能成功(若 playwright MCP 可用)或失败(MCP 不可用),
    // 但不应是 "not implemented" 错误
    if let Err(e) = result {
        let msg = format!("{}", e);
        assert!(
            !msg.contains("not implemented"),
            "Plan 3 should not return 'not implemented', got: {}",
            msg
        );
    }
}

#[test]
fn dispatch_files_organize_routes_to_files_organize_skill() {
    // 验证 files.organize 分支命中:用非法 destination 触发 executor 错误,
    // 错误消息应来自 FilesOrganizeSkill(executor 层),不是 "unknown skill_id"。
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({
        "source": "Z:/nonexistent_source_dir",
        "filter": "*.pdf",
        "destination": "Z:/nonexistent_dest_dir"
    });
    let result = dispatch_skill_executor(
        "files.organize",
        &kernel,
        &input,
        &approver,
        "task-files",
        "step-files",
    );
    // executor 大概率返回 Err(无文件匹配 / 路径不允许)。
    // 关键断言:不是 "unknown skill_id" 错误,证明路由命中。
    if let Err(e) = result {
        let msg = format!("{}", e);
        assert!(
            !msg.contains("unknown skill_id"),
            "should not be unknown skill_id, got: {}",
            msg
        );
    }
}

#[test]
fn dispatch_task_explain_routes_to_execute_explain() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"limit": 5});
    let result = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &input,
        &approver,
        "task-explain",
        "step-explain",
    );
    let outcome = result.expect("task.explain with valid limit should succeed");
    assert!(outcome.succeeded);
    assert_eq!(outcome.step_id, "step-explain");
    assert!(outcome.output.get("task_id").is_some());
}

#[test]
fn dispatch_task_explain_uses_default_limit_when_missing() {
    let kernel = kernel();
    let approver = AutoApprover;
    // limit 缺失 → extract_u32 返回 None → unwrap_or(10)
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &input,
        &approver,
        "task-explain-default",
        "step-explain-default",
    );
    let outcome = result.expect("task.explain with default limit should succeed");
    assert!(outcome.succeeded);
}

#[test]
fn dispatch_task_explain_rejects_invalid_limit_type() {
    let kernel = kernel();
    let approver = AutoApprover;
    // limit 是字符串而非数字 → extract_u32 返回 Err
    let input = serde_json::json!({"limit": "not-a-number"});
    let result = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &input,
        &approver,
        "task-explain-bad",
        "step-explain-bad",
    );
    assert!(result.is_err());
}

#[test]
fn dispatch_task_repeat_missing_target_task_id_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"source_filter": "*.pdf"});
    let result = dispatch_skill_executor(
        "task.repeat_verified",
        &kernel,
        &input,
        &approver,
        "task-repeat",
        "step-repeat",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("target_task_id"), "got: {}", msg);
}

#[test]
fn dispatch_task_compensate_missing_target_step_id_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "task.compensate",
        &kernel,
        &input,
        &approver,
        "task-comp",
        "step-comp",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("target_step_id"), "got: {}", msg);
}

#[test]
fn dispatch_research_save_missing_url_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"save_path": "C:/test.md"});
    let result = dispatch_skill_executor(
        "research.save_markdown",
        &kernel,
        &input,
        &approver,
        "task-research",
        "step-research",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("url"), "got: {}", msg);
}

#[test]
fn dispatch_form_prepare_missing_fields_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"url": "https://example.com"});
    let result = dispatch_skill_executor(
        "form.prepare",
        &kernel,
        &input,
        &approver,
        "task-form",
        "step-form",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("fields"), "got: {}", msg);
}

#[test]
fn dispatch_form_prepare_accepts_fields_as_object() {
    // fields 是 HashMap<String, String>。JSON object 形式应被接受。
    // (不会真正成功,因为 url=https://example.com 触发 MCP 失败,但应能
    // 通过 dispatcher 的字段提取阶段,错误来自 executor 而非 dispatcher)
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({
        "url": "https://example.com",
        "fields": {"input[name=q]": "test"}
    });
    let result = dispatch_skill_executor(
        "form.prepare",
        &kernel,
        &input,
        &approver,
        "task-form-obj",
        "step-form-obj",
    );
    // 错误不应是 "fields" 相关的 dispatcher 错误
    if let Err(e) = result {
        let msg = format!("{}", e);
        assert!(
            !msg.contains("dispatch: missing field 'fields'"),
            "should not be missing fields, got: {}",
            msg
        );
    }
}

#[cfg(all(windows, feature = "uia"))]
#[test]
fn dispatch_app_control_missing_app_name_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"action": "launch"});
    let result = dispatch_skill_executor(
        "quick.app_control",
        &kernel,
        &input,
        &approver,
        "task-app",
        "step-app",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("app_name"), "got: {}", msg);
}

#[cfg(all(windows, feature = "uia"))]
#[test]
fn dispatch_note_capture_missing_content_returns_err() {
    let kernel = kernel();
    let approver = AutoApprover;
    let input = serde_json::json!({"save_path": "C:/test.txt"});
    let result = dispatch_skill_executor(
        "note.capture",
        &kernel,
        &input,
        &approver,
        "task-note",
        "step-note",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("content"), "got: {}", msg);
}
