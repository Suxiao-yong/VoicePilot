use trust_kernel::kernel::TrustKernel;
use trust_kernel::state::TaskState;
use uuid::Uuid;

#[test]
fn text_command_flows_through_state_machine_with_audit() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = Uuid::new_v4().to_string();
    let goal = "open notepad and write hello";

    // 1. Create task in IDLE
    let task = kernel.create_task(&task_id, goal).unwrap();
    assert_eq!(task.status, TaskState::Idle);
    assert_audit_event_count(&kernel, &task_id, 1, "task_created should be audited");

    // 2. Listen → Plan → Await approval (text input simulates voice transcript)
    kernel.transition(&task_id, TaskState::Listening).unwrap();
    kernel.transition(&task_id, TaskState::Planning).unwrap();
    kernel
        .transition(&task_id, TaskState::AwaitingApproval)
        .unwrap();

    // 3. User approves (in W1: auto-approve for text commands; real approval UI in W2+)
    kernel.transition(&task_id, TaskState::Executing).unwrap();

    // 4. Execute → Verify → Done. W1 has no real tools; executor is a stub.
    kernel.transition(&task_id, TaskState::Verifying).unwrap();
    kernel.transition(&task_id, TaskState::Done).unwrap();

    // 5. Final state and audit chain
    let final_task = kernel.get_task(&task_id).unwrap().unwrap();
    assert_eq!(final_task.status, TaskState::Done);

    let audit_count = kernel.audit_count_for_task(&task_id).unwrap();
    assert_eq!(
        audit_count, 7,
        "expected 7 audit events (1 create + 6 transitions), got {}",
        audit_count
    );
}

#[test]
fn illegal_transition_returns_error() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = Uuid::new_v4().to_string();
    kernel.create_task(&task_id, "goal").unwrap();

    // IDLE → EXECUTING is illegal (must go through PLANNING → AWAITING_APPROVAL)
    let result = kernel.transition(&task_id, TaskState::Executing);
    assert!(
        result.is_err(),
        "transition IDLE → EXECUTING must be rejected"
    );
}

#[test]
fn kill_switch_cancels_from_executing() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = Uuid::new_v4().to_string();
    kernel.create_task(&task_id, "goal").unwrap();
    kernel.transition(&task_id, TaskState::Listening).unwrap();
    kernel.transition(&task_id, TaskState::Planning).unwrap();
    kernel
        .transition(&task_id, TaskState::AwaitingApproval)
        .unwrap();
    kernel.transition(&task_id, TaskState::Executing).unwrap();

    kernel.transition(&task_id, TaskState::Cancelled).unwrap();
    let final_task = kernel.get_task(&task_id).unwrap().unwrap();
    assert_eq!(final_task.status, TaskState::Cancelled);
}

fn assert_audit_event_count(kernel: &TrustKernel, task_id: &str, expected: usize, msg: &str) {
    let count = kernel.audit_count_for_task(task_id).unwrap();
    assert_eq!(count, expected, "{} (got {})", msg, count);
}
