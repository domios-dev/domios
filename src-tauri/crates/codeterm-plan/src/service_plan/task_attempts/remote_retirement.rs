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
        self.with_plan(plan, now, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task)
                .ok_or(PlanError::TaskNotFound)?;
            Ok(retire_remote_attempt(task, attempt, peer, tab, reason, now))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::TaskExecutionAttempt;

    #[test]
    fn remote_retirement_releases_assignment_and_fences_late_ack() {
        let mut task: Task = serde_json::from_value(serde_json::json!({
            "id": "task", "name": "Remote", "type": "implement", "status": TaskStatus::Pending
        }))
        .unwrap();
        task.status = TaskStatus::InProgress;
        task.assigned_tab_id = Some("worker".into());
        task.execution_attempts.push(TaskExecutionAttempt {
            id: "attempt".into(),
            mesh_peer: Some("peer".into()),
            executor_tab_id: "worker".into(),
            branch: Some("lane/worker".into()),
            remote_worktree_path: Some("remote/lane".into()),
            ..Default::default()
        });
        assert!(!retire_remote_attempt(
            &mut task,
            "attempt",
            "foreign",
            "worker",
            "remote_tab_closed",
            7
        ));
        assert!(retire_remote_attempt(
            &mut task,
            "attempt",
            "peer",
            "worker",
            "remote_tab_closed",
            7
        ));
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.assigned_tab_id.is_none());
        assert_eq!(task.execution_attempts[0].closed_at, Some(7));
        assert_eq!(task.execution_attempts[0].retired_at, Some(7));
        assert_eq!(
            task.execution_attempts[0].branch.as_deref(),
            Some("lane/worker")
        );
        assert_eq!(
            task.execution_attempts[0].remote_worktree_path.as_deref(),
            Some("remote/lane")
        );
        assert!(!retire_remote_attempt(
            &mut task,
            "attempt",
            "peer",
            "worker",
            "remote_tab_closed",
            8
        ));
    }
}
