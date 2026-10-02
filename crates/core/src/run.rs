//! Run lifecycle states from section 8 of the specification.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    Preparing,
    Starting,
    Running,
    WaitingForApproval,
    WaitingForInput,
    Cancelling,
    Completed,
    Failed,
    Interrupted,
    OutcomeUnknown,
}

impl RunState {
    /// A chat can be idle again once its run reaches one of these.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted | Self::OutcomeUnknown)
    }

    /// States in which Stop is meaningful.
    pub fn can_interrupt(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::WaitingForApproval | Self::WaitingForInput)
    }
}

/// How the engine said a run finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Failed,
    Interrupted,
}
