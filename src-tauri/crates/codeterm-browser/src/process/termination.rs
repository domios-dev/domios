use super::CloseOutcome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminationAction {
    Wait,
    ForceTerminate,
    KillOwnedPid(u32),
    ConfirmExit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Graceful,
    Force,
    Kill,
    AwaitExit,
    Exited,
}

pub struct TerminationModel {
    pid: u32,
    stage: Stage,
}

impl TerminationModel {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            stage: Stage::Graceful,
        }
    }

    pub fn graceful_deadline(&mut self) -> TerminationAction {
        if self.stage != Stage::Graceful {
            return TerminationAction::Wait;
        }
        self.stage = Stage::Force;
        TerminationAction::ForceTerminate
    }

    pub fn force_deadline(&mut self, verified_pid: Option<u32>) -> TerminationAction {
        if self.stage != Stage::Force {
            return TerminationAction::Wait;
        }
        if self.pid <= 1 || verified_pid != Some(self.pid) {
            self.stage = Stage::AwaitExit;
            return TerminationAction::Wait;
        }
        self.stage = Stage::Kill;
        TerminationAction::KillOwnedPid(self.pid)
    }

    pub fn observed_exit(&mut self) -> TerminationAction {
        if self.stage == Stage::Exited {
            return TerminationAction::Wait;
        }
        self.stage = Stage::Exited;
        TerminationAction::ConfirmExit
    }

    pub fn may_discard_profile(&self) -> bool {
        self.stage == Stage::Exited
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminationFailure {
    ExitUnconfirmed,
}

impl TerminationFailure {
    pub fn code(self) -> &'static str {
        match self {
            Self::ExitUnconfirmed => "browser_termination_unconfirmed",
        }
    }
}

impl std::fmt::Display for TerminationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: browser exit was not confirmed; close did not complete",
            self.code()
        )
    }
}

impl std::error::Error for TerminationFailure {}

pub fn close_result(outcome: Option<CloseOutcome>) -> Result<(), TerminationFailure> {
    if outcome.is_none_or(CloseOutcome::confirms_exit) {
        Ok(())
    } else {
        Err(TerminationFailure::ExitUnconfirmed)
    }
}

#[cfg(test)]
mod tests;
