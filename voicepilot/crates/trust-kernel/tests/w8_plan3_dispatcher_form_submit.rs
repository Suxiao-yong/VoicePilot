//! W8 Plan 3 Task 6: dispatch_skill_executor form.submit 路由命中测试.
//!
//! 验证 form.submit 分支不再返回 "not implemented" Err,
//! 而是路由到 execute_form_submit(可能因 manifest 校验失败,
//! 但不应是 "not implemented" 错误)。

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;

#[test]
fn dispatch_form_submit_routes_to_executor_not_placeholder() {
    // form.submit + 缺 url → 应返回 manifest 校验错误,不是 "not implemented"
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let input = serde_json::json!({"submit_selector": "button[type=submit]"});
    let result = dispatch_skill_executor(
        "form.submit",
        &kernel,
        &input,
        &approver,
        "task-submit",
        "step-submit",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(
        !msg.contains("not implemented"),
        "form.submit should not return 'not implemented' in Plan 3, got: {}",
        msg
    );
    // 应是 manifest 校验错误(url 缺失)
    assert!(
        msg.contains("url") || msg.contains("validation"),
        "got: {}",
        msg
    );
}

#[test]
fn dispatch_form_submit_missing_url_returns_validation_err() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let input = serde_json::json!({});
    let result = dispatch_skill_executor(
        "form.submit",
        &kernel,
        &input,
        &approver,
        "task-submit-2",
        "step-submit-2",
    );
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    // extract_string 失败或 manifest 校验失败
    assert!(msg.contains("url"), "got: {}", msg);
}
