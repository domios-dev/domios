use super::startup::{startup_subject_current, StartupSubject};
use crate::plan::{TaskExecutionAttempt, TaskStatus};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(in crate::service_plan) struct AckAttemptRead {
    pub id: String,
    pub executor: String,
    pub session: Option<String>,
    pub nonce: Option<String>,
    pub group: Option<String>,
    pub acknowledged: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(in crate::service_plan) struct TaskAckAuthorityRead {
    pub task_id: String,
    pub status: TaskStatus,
    pub assigned_tab: Option<String>,
    pub attempt: Option<AckAttemptRead>,
    pub latest_unclosed: Option<String>,
    pub latest_group: Option<String>,
    pub response_attempt: Option<TaskExecutionAttempt>,
}

impl TaskAckAuthorityRead {
    pub fn current(&self, tab: &str) -> Option<(&AckAttemptRead, bool)> {
        let first = self.attempt.as_ref()?;
        startup_subject_current(
            tab,
            self.assigned_tab.as_deref(),
            StartupSubject {
                id: &first.id,
                executor: &first.executor,
                session: first.session.as_deref(),
                group: first.group.as_deref(),
            },
            self.latest_unclosed.as_deref(),
            self.latest_group.as_deref(),
        )
        .then_some((first, first.group.is_some()))
    }
}

pub(in crate::service_plan) fn acknowledging_binding_matches(
    session: Option<&str>,
    nonce: Option<&str>,
    runtime_binding: Option<(&str, &str)>,
    enforce: bool,
) -> bool {
    if !enforce {
        return true;
    }
    match (session, nonce, runtime_binding) {
        (None, None, _) => true,
        (Some(session), Some(nonce), Some((current_session, current_nonce))) => {
            session == current_session && nonce == current_nonce
        }
        (Some(session), None, Some((current_session, _))) => session == current_session,
        (None, Some(nonce), Some((_, current_nonce))) => nonce == current_nonce,
        _ => false,
    }
}
