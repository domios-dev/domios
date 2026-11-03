use crate::plan::{TaskExecutionAttempt, TaskExecutorTermination};
use crate::service_plan::{PlanError, PlanStore};

fn same_executor(left: &TaskExecutionAttempt, right: &TaskExecutionAttempt) -> bool {
    left.id == right.id
        && left.executor_tab_id == right.executor_tab_id
        && left.session_id == right.session_id
        && left.session_binding_nonce == right.session_binding_nonce
        && left.mesh_peer.is_none()
        && right.mesh_peer.is_none()
}

impl PlanStore {
    pub fn record_task_executor_termination(
        &self,
        plan: &str,
        task: &str,
        source: &TaskExecutionAttempt,
        now: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan,
            task,
            super::super::store_db::hot_write::TaskHistory::Attempts,
            now,
            |edit| {
                let attempt = edit
                    .task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| same_executor(attempt, source))
                    .ok_or_else(|| PlanError::Io("executor termination identity changed".into()))?;
                attempt.executor_termination = Some(TaskExecutorTermination {
                    attempt_id: source.id.clone(),
                    executor_tab_id: source.executor_tab_id.clone(),
                    session_id: source.session_id.clone(),
                    session_binding_nonce: source.session_binding_nonce.clone(),
                    confirmed_at: now,
                });
                Ok(())
            },
        )
    }

    pub fn task_executor_termination_confirmed(
        &self,
        plan: &str,
        task: &str,
        source: &TaskExecutionAttempt,
    ) -> Result<bool, PlanError> {
        let plan = self.get(plan).ok_or(PlanError::NotFound)?;
        let task =
            crate::plan::task_key::find_task(&plan.tasks, task).ok_or(PlanError::TaskNotFound)?;
        Ok(task.execution_attempts.iter().any(|attempt| {
            same_executor(attempt, source)
                && attempt
                    .executor_termination
                    .as_ref()
                    .is_some_and(|receipt| {
                        receipt.attempt_id == source.id
                            && receipt.executor_tab_id == source.executor_tab_id
                            && receipt.session_id == source.session_id
                            && receipt.session_binding_nonce == source.session_binding_nonce
                    })
        }))
    }
}
