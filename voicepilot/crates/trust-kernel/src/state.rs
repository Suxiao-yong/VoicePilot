//! Task state machine — see V1.1 spec §3.1 (Trust Kernel components).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    Idle,
    Listening,
    Planning,
    AwaitingApproval,
    Executing,
    Verifying,
    Compensating,
    Done,
    Failed,
    Cancelled,
}

impl TaskState {
    /// Returns allowed next states from the current state.
    /// Spec §3.1: IDLE→LISTENING→PLANNING→…; Kill Switch can cancel from most states.
    pub fn allowed_next(self) -> &'static [TaskState] {
        use TaskState::*;
        match self {
            Idle => &[Listening, Cancelled],
            Listening => &[Planning, Cancelled],
            Planning => &[AwaitingApproval, Failed, Cancelled],
            AwaitingApproval => &[Executing, Cancelled],
            Executing => &[Verifying, Compensating, Failed, Cancelled],
            Verifying => &[Done, Compensating, Failed, Cancelled],
            Compensating => &[Done, Failed, Cancelled],
            Done => &[],
            Failed => &[],
            Cancelled => &[],
        }
    }

    pub fn can_transition_to(self, target: TaskState) -> bool {
        self.allowed_next().contains(&target)
    }
}
