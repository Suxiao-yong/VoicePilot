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
