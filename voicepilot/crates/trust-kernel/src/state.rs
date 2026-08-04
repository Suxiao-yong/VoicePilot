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
    /// W10 Plan 4: Kill Switch 中间态(spec §6.2 v2 修订 #15)。
    ///
    /// 语义:Kill Switch 触发后,当前态可中断时(Idle/Listening/Planning/
    /// AwaitingApproval/voice loop chunk 边界)进入 Cancelling,等当前 chunk
    /// 完成后 → Cancelled。Cancelling 仅允许 → Cancelled(不可逆)。
    ///
    /// 向后兼容:Executing/Verifying/Compensating → Cancelled 直跳路径保留
    /// (allowed_next 同时包含 Cancelling 与 Cancelled),现有
    /// `kill_switch_can_cancel_from_any_non_terminal_state` 测试不破坏。
    Cancelling,
    Done,
    Failed,
    Cancelled,
}

impl TaskState {
    /// Returns allowed next states from the current state.
    /// Spec §3.1: IDLE→LISTENING→PLANNING→…; Kill Switch can cancel from most states.
    /// W10 Plan 4: 加 Cancelling 中间态(spec §6.2 v2 修订 #15)。
    pub fn allowed_next(self) -> &'static [TaskState] {
        use TaskState::*;
        match self {
            // Idle 不加 Cancelling:Kill Switch 触发时直接 → Cancelled(无 ongoing work)
            Idle => &[Listening, Cancelled],
            // W10 Plan 4: Listening/Planning/AwaitingApproval 可中断 → Cancelling
            Listening => &[Planning, Cancelling, Cancelled],
            Planning => &[AwaitingApproval, Failed, Cancelling, Cancelled],
            AwaitingApproval => &[Executing, Cancelling, Cancelled],
            // W10 Plan 4: Executing 同时允许 Cancelling(新)+ Cancelled(直跳,向后兼容)
            Executing => &[Verifying, Compensating, Failed, Cancelling, Cancelled],
            // W10 Plan 4: Verifying 同时允许 Cancelling + Cancelled
            Verifying => &[Done, Compensating, Failed, Cancelling, Cancelled],
            // W10 Plan 4: Compensating 同时允许 Cancelling + Cancelled
            Compensating => &[Done, Failed, Cancelling, Cancelled],
            // W10 Plan 4: Cancelling 仅允许 → Cancelled(终态前最后一步)
            Cancelling => &[Cancelled],
            Done => &[],
            Failed => &[],
            Cancelled => &[],
        }
    }

    pub fn can_transition_to(self, target: TaskState) -> bool {
        self.allowed_next().contains(&target)
    }
}
