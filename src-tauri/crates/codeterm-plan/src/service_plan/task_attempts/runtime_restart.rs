use super::super::{PlanError, PlanStore};
use crate::plan::TaskExecutionAttempt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeRestartMode {
    Resume,
    ModelSwitch,
    AuthorizedFresh,
    ProviderSwitch,
}

#[derive(Debug, Clone)]
pub struct RuntimeRestartBinding {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub executor_tab_id: String,
    pub provider: String,
    pub target_provider: Option<String>,
    pub old_thread: String,
    pub old_nonce: String,
    pub new_thread: String,
    pub new_nonce: String,
    pub owner_epoch: u64,
    pub mode: RuntimeRestartMode,
}

fn permits_rebind(request: &RuntimeRestartBinding, attempt: &TaskExecutionAttempt) -> bool {
    [
        &request.executor_tab_id,
        &request.provider,
        &request.old_thread,
        &request.old_nonce,
        &request.new_thread,
        &request.new_nonce,
    ]
    .iter()
    .all(|value| !value.is_empty())
        && request.owner_epoch > 0
        && request.old_nonce != request.new_nonce
        && attempt.id == request.attempt_id
        && attempt.executor_tab_id == request.executor_tab_id
        && attempt.provider.as_deref() == Some(request.provider.as_str())
        && attempt.session_id.as_deref() == Some(request.old_thread.as_str())
        && attempt.session_binding_nonce.as_deref() == Some(request.old_nonce.as_str())
        && attempt.retired_at.is_none()
        && (request.target_provider.is_none()
            || (request.mode == RuntimeRestartMode::ProviderSwitch
                && request
                    .target_provider
                    .as_deref()
                    .is_some_and(|target| !target.is_empty() && target != request.provider)))
        && (request.mode != RuntimeRestartMode::ProviderSwitch || request.target_provider.is_some())
        && (request.mode != RuntimeRestartMode::ProviderSwitch || attempt.closed_at.is_none())
        && (request.old_thread == request.new_thread
            || (matches!(
                request.mode,
                RuntimeRestartMode::AuthorizedFresh | RuntimeRestartMode::ProviderSwitch
            ) && attempt.closed_at.is_none()))
        && (request.mode != RuntimeRestartMode::AuthorizedFresh
            || request.old_thread != request.new_thread)
}

fn resolve_runtime_restart_attempt(
    request: &RuntimeRestartBinding,
    attempt: &TaskExecutionAttempt,
    authenticated_tab_id: &str,
    current_nonce: &str,
) -> Option<TaskExecutionAttempt> {
    if request.executor_tab_id != authenticated_tab_id
        || request.new_nonce != current_nonce
        || !permits_rebind(request, attempt)
    {
        return None;
    }
    let mut rebound = attempt.clone();
    if let Some(target) = &request.target_provider {
        rebound.provider = Some(target.clone());
        rebound.agent_type = Some(target.clone());
        rebound.model = None;
        rebound.requested_model = None;
        rebound.provider_resolved_model = None;
        rebound.session_model = None;
        rebound.reasoning_effort = None;
        rebound.actual_reasoning_effort = None;
        rebound.model_identity_mismatch = None;
    }
    rebound.session_id = Some(request.new_thread.clone());
    rebound.session_binding_nonce = Some(request.new_nonce.clone());
    Some(rebound)
}

impl PlanStore {
    pub fn rebind_runtime_restart(
        &self,
        request: RuntimeRestartBinding,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            &request.plan_id.clone(),
            &request.task_id.clone(),
            super::super::store_db::hot_write::TaskHistory::LiveAndTarget(
                request.attempt_id.clone(),
            ),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.id != request.task_id {
                    return Err(PlanError::TaskNotFound);
                }
                if task.assigned_tab_id.as_deref() != Some(request.executor_tab_id.as_str())
                    || task.interruption.owner_epoch != request.owner_epoch
                    || task.interruption.owner_tab_id.as_deref()
                        != Some(request.executor_tab_id.as_str())
                    || task.interruption.owner_session_id.as_deref()
                        != Some(request.old_thread.as_str())
                    || task
                        .execution_attempts
                        .iter()
                        .filter(|attempt| attempt.id == request.attempt_id)
                        .count()
                        != 1
                    || task.execution_attempts.iter().any(|attempt| {
                        attempt.id != request.attempt_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
                {
                    return Err(refused());
                }
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == request.attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                *attempt = resolve_runtime_restart_attempt(
                    &request,
                    attempt,
                    &request.executor_tab_id,
                    &request.new_nonce,
                )
                .ok_or_else(refused)?;
                task.interruption.owner_session_id = Some(request.new_thread);
                Ok(Some(()))
            },
        )?;
        Ok(())
    }
}

fn refused() -> PlanError {
    super::super::AttemptBindingRefusalKind::CurrentBindingRequired.refusal()
}

#[cfg(test)]
mod tests;
