use codeterm_blueprint_types::blueprint_journal::JournalState;
use std::sync::Arc;

pub type RunObserver = Arc<dyn Fn(&'static str) + Send + Sync>;
impl crate::store::Store {
    pub fn bind_run_observer(&mut self, observer: RunObserver) {
        self.run_observer = Some(observer);
    }
}

pub fn terminal_outcome(state: JournalState) -> Option<&'static str> {
    match state {
        JournalState::Done => Some("success"),
        JournalState::Failed => Some("failure"),
        JournalState::Skipped => Some("cancelled"),
        JournalState::Running | JournalState::Held => None,
    }
}

pub(crate) fn publish(observer: Option<RunObserver>, state: JournalState) {
    if let (Some(observer), Some(outcome)) = (observer, terminal_outcome(state)) {
        observer(outcome);
    }
}
