use crate::plan::{Task, TaskStatus};
use crate::service_plan::{PlanError, PlanStore};

fn retire_remote_attempt(
    task: &mut Task,
    attempt_id: &str,
    peer: &str,
    tab: &str,
    reason: &str,
    now: u64,
) -> bool {
    let Some(attempt) = task.execution_attempts.iter_mut().find(|attempt| {
        attempt.id == attempt_id
            && attempt.mesh_peer.as_deref() == Some(peer)
            && attempt.executor_tab_id == tab
            && attempt.closed_at.is_none()
            && attempt.retired_at.is_none()
            && attempt.report.is_none()
    }) else {
        return false;
    };
    attempt.closed_at = Some(now);
    attempt.retired_at = Some(now);
    attempt.closure_reason = Some(reason.into());
    if task.assigned_tab_id.as_deref() == Some(tab) {
        task.assigned_tab_id = None;
        if !task.status.is_terminal()
            && !matches!(task.status, TaskStatus::InReview | TaskStatus::Paused)
        {
            task.status = TaskStatus::Pending;
            task.status_changed_at = now;
            task.status_label = Some(reason.into());
        }
    }
    task.updated_at = now;
    true
}

impl PlanStore {
    pub fn retire_remote_task_attempt(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
        peer: &str,
        tab: &str,
        reason: &str,
        now: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(
            plan,
            task,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt.into()),
            now,
            |edit| {
                let task = &mut edit.task;
                Ok(retire_remote_attempt(task, attempt, peer, tab, reason, now))
            },
        )
    }
}

#[cfg(test)]
mod tests;
