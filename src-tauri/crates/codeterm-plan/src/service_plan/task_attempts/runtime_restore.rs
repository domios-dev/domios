use super::super::{PlanError, PlanStore};

#[derive(Debug, Clone)]
pub struct RestoredRuntimeSessionBinding {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub executor_tab_id: String,
    pub thread: String,
    pub nonce: String,
    pub expected_thread: Option<String>,
    pub expected_nonce: Option<String>,
    pub owner_epoch: u64,
}

impl PlanStore {
    pub fn bind_restored_runtime_task_session(
        &self,
        request: RestoredRuntimeSessionBinding,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan_if_changed(&request.plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == request.task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if request.thread.trim().is_empty()
                || request.nonce.trim().is_empty()
                || task.assigned_tab_id.as_deref() != Some(request.executor_tab_id.as_str())
                || task.interruption.owner_epoch != request.owner_epoch
                || task
                    .execution_attempts
                    .iter()
                    .filter(|attempt| attempt.id == request.attempt_id)
                    .count()
                    != 1
            {
                return Err(refused());
            }
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == request.attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if attempt.executor_tab_id != request.executor_tab_id
                || attempt.closed_at.is_some()
                || attempt.retired_at.is_some()
                || attempt.session_id != request.expected_thread
                || attempt.session_binding_nonce != request.expected_nonce
                || attempt
                    .session_id
                    .as_deref()
                    .is_some_and(|thread| thread != request.thread)
            {
                return Err(refused());
            }
            attempt.session_id = Some(request.thread.clone());
            attempt.session_binding_nonce = Some(request.nonce.clone());
            task.interruption.owner_session_id = Some(request.thread.clone());
            Ok(Some(()))
        })?;
        Ok(())
    }
}

fn refused() -> PlanError {
    super::super::AttemptBindingRefusalKind::CurrentBindingRequired.refusal()
}
