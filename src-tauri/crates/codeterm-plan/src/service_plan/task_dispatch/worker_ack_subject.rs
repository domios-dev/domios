use crate::plan::{TaskExecutionAttempt, TaskStatus};

pub const ACK_CAUSE_NO_LIVE_ATTEMPT: &str = "worker_ack_no_live_attempt";
pub const ACK_CAUSE_PLAN_NOT_ACTIVE: &str = "worker_ack_plan_not_active";
pub const ACK_CAUSE_TASK_NOT_OPEN: &str = "worker_ack_task_not_open";
pub const ACK_CAUSE_AMBIGUOUS_ATTEMPTS: &str = "worker_ack_ambiguous_live_attempts";
pub const ACK_CAUSE_ATTEMPT_SUPERSEDED: &str = "worker_ack_attempt_superseded";

/// A newer dispatch closed this tab's attempt while it was still starting. No
/// later ack from this tab can succeed, so it must stop rather than retry.
pub fn superseded_subject_cause(
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
    retired_attempt_id: &str,
    live_attempt_id: &str,
    live_tab_id: &str,
) -> String {
    format!(
        "{ACK_CAUSE_ATTEMPT_SUPERSEDED}: attempt {retired_attempt_id} on tab {tab_id} was superseded \
         by attempt {live_attempt_id} of {plan_id}/{task_id}, now held by tab {live_tab_id}. Stop and \
         exit; do not retry the ack."
    )
}

/// One live attempt reached through its executor tab. The tab binding is the
/// authority; a caller-supplied plan/task subject only disambiguates.
#[derive(Debug, Clone)]
pub struct LiveTabAttempt {
    pub plan_id: String,
    pub task_id: String,
    pub task_handle: Option<String>,
    pub plan_active: bool,
    pub task_status: TaskStatus,
    pub attempt: TaskExecutionAttempt,
}

impl LiveTabAttempt {
    pub fn matches_subject_hint(&self, plan_hint: Option<&str>, task_hint: Option<&str>) -> bool {
        plan_hint.is_none_or(|plan_id| self.plan_id == plan_id)
            && task_hint.is_none_or(|task_id| {
                self.task_id == task_id || self.task_handle.as_deref() == Some(task_id)
            })
    }

    fn describe(&self) -> String {
        format!(
            "{}/{} (plan_active={}, task_status={:?}, attempt={})",
            self.plan_id, self.task_id, self.plan_active, self.task_status, self.attempt.id
        )
    }
}

fn describe_all<'a>(candidates: impl IntoIterator<Item = &'a LiveTabAttempt>) -> String {
    candidates
        .into_iter()
        .map(LiveTabAttempt::describe)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Names why the tab resolved to no acknowledgeable attempt. The tab is only the
/// scan key, so the cause is the plan state, the task state, or nothing bound.
pub fn unresolved_subject_cause<'a>(
    tab_id: &str,
    candidates: impl IntoIterator<Item = &'a LiveTabAttempt>,
) -> String {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    if candidates.is_empty() {
        return format!("{ACK_CAUSE_NO_LIVE_ATTEMPT}: tab {tab_id} owns no live task attempt");
    }
    let cause = if candidates.iter().all(|candidate| !candidate.plan_active) {
        ACK_CAUSE_PLAN_NOT_ACTIVE
    } else {
        ACK_CAUSE_TASK_NOT_OPEN
    };
    format!(
        "{cause}: tab {tab_id} is bound to {}",
        describe_all(candidates)
    )
}

pub fn ambiguous_subject_cause<'a>(
    tab_id: &str,
    candidates: impl IntoIterator<Item = &'a LiveTabAttempt>,
) -> String {
    format!(
        "{ACK_CAUSE_AMBIGUOUS_ATTEMPTS}: tab {tab_id} owns {}",
        describe_all(candidates)
    )
}
