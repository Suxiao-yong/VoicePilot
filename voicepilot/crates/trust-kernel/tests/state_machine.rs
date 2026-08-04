use trust_kernel::state::TaskState::*;

#[test]
fn idle_to_listening_is_allowed() {
    assert!(Idle.can_transition_to(Listening));
}

#[test]
fn listening_to_planning_is_allowed() {
    assert!(Listening.can_transition_to(Planning));
}

#[test]
fn planning_to_awaiting_approval_is_allowed() {
    assert!(Planning.can_transition_to(AwaitingApproval));
}

#[test]
fn awaiting_approval_to_executing_is_allowed() {
    assert!(AwaitingApproval.can_transition_to(Executing));
}

#[test]
fn executing_to_verifying_is_allowed() {
    assert!(Executing.can_transition_to(Verifying));
}

#[test]
fn verifying_to_done_is_allowed() {
    assert!(Verifying.can_transition_to(Done));
}

#[test]
fn kill_switch_can_cancel_from_any_non_terminal_state() {
    for s in [Idle, Listening, Planning, AwaitingApproval, Executing, Verifying, Compensating] {
        assert!(s.can_transition_to(Cancelled), "must allow Cancelled from {:?}", s);
    }
}

#[test]
fn terminal_states_have_no_outgoing_transitions() {
    for s in [Done, Failed, Cancelled] {
        assert!(s.allowed_next().is_empty(), "{:?} must be terminal", s);
    }
}

#[test]
fn backward_transitions_are_forbidden() {
    assert!(!Listening.can_transition_to(Idle));
    assert!(!Planning.can_transition_to(Listening));
    assert!(!Executing.can_transition_to(Planning));
    assert!(!Done.can_transition_to(Verifying));
}

#[test]
fn executing_to_failed_is_allowed_for_unrecoverable_errors() {
    assert!(Executing.can_transition_to(Failed));
}

#[test]
fn verifying_to_compensating_is_allowed_when_verification_fails() {
    assert!(Verifying.can_transition_to(Compensating));
}

#[test]
fn compensating_to_done_is_allowed_when_compensation_succeeds() {
    assert!(Compensating.can_transition_to(Done));
}

// ===== W10 Plan 4: Cancelling 中间态转换测试 =====

#[test]
fn cancelling_to_cancelled_is_allowed() {
    assert!(Cancelling.can_transition_to(Cancelled));
}

#[test]
fn cancelling_to_other_states_is_forbidden() {
    // Cancelling 仅允许 → Cancelled,其他态都非法
    assert!(!Cancelling.can_transition_to(Idle));
    assert!(!Cancelling.can_transition_to(Listening));
    assert!(!Cancelling.can_transition_to(Planning));
    assert!(!Cancelling.can_transition_to(AwaitingApproval));
    assert!(!Cancelling.can_transition_to(Executing));
    assert!(!Cancelling.can_transition_to(Verifying));
    assert!(!Cancelling.can_transition_to(Compensating));
    assert!(!Cancelling.can_transition_to(Done));
    assert!(!Cancelling.can_transition_to(Failed));
}

#[test]
fn executing_to_cancelling_is_allowed() {
    // W10 Plan 4: Executing → Cancelling 新增合法转换
    assert!(Executing.can_transition_to(Cancelling));
}

#[test]
fn verifying_to_cancelling_is_allowed() {
    // W10 Plan 4: Verifying → Cancelling 新增合法转换
    assert!(Verifying.can_transition_to(Cancelling));
}

#[test]
fn compensating_to_cancelling_is_allowed() {
    // W10 Plan 4: Compensating → Cancelling 新增合法转换
    assert!(Compensating.can_transition_to(Cancelling));
}

#[test]
fn listening_to_cancelling_is_allowed() {
    // W10 Plan 4: Listening → Cancelling 可中断路径
    assert!(Listening.can_transition_to(Cancelling));
}

#[test]
fn planning_to_cancelling_is_allowed() {
    // W10 Plan 4: Planning → Cancelling 可中断路径
    assert!(Planning.can_transition_to(Cancelling));
}

#[test]
fn awaiting_approval_to_cancelling_is_allowed() {
    // W10 Plan 4: AwaitingApproval → Cancelling 可中断路径
    assert!(AwaitingApproval.can_transition_to(Cancelling));
}

#[test]
fn executing_to_cancelled_direct_jump_still_allowed() {
    // W10 Plan 4 v2 修订 #15: Executing → Cancelled 直跳路径保留(向后兼容)
    assert!(Executing.can_transition_to(Cancelled));
}

#[test]
fn verifying_to_cancelled_direct_jump_still_allowed() {
    // W10 Plan 4 v2 修订 #15: Verifying → Cancelled 直跳路径保留
    assert!(Verifying.can_transition_to(Cancelled));
}

#[test]
fn compensating_to_cancelled_direct_jump_still_allowed() {
    // W10 Plan 4 v2 修订 #15: Compensating → Cancelled 直跳路径保留
    assert!(Compensating.can_transition_to(Cancelled));
}

#[test]
fn cancelling_is_not_terminal() {
    // Cancelling 不是终态,允许 → Cancelled
    assert!(!Cancelling.allowed_next().is_empty());
}
