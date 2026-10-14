use super::super::{PlanError, PlanStore};
use crate::plan::TaskExecutionAttempt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeRestartMode {
    Resume,
    AuthorizedFresh,
}

#[derive(Debug, Clone)]
pub struct RuntimeRestartBinding {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub executor_tab_id: String,
    pub provider: String,
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
        && (request.old_thread == request.new_thread
            || (request.mode == RuntimeRestartMode::AuthorizedFresh && attempt.closed_at.is_none()))
        && (request.mode != RuntimeRestartMode::AuthorizedFresh
            || request.old_thread != request.new_thread)
}

impl PlanStore {
    pub fn rebind_runtime_restart(
        &self,
        request: RuntimeRestartBinding,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let plan_id = request.plan_id.clone();
        self.with_plan_if_changed(&plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == request.task_id)
                .ok_or(PlanError::TaskNotFound)?;
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
            if !permits_rebind(&request, attempt) {
                return Err(refused());
            }
            attempt.session_id = Some(request.new_thread.clone());
            attempt.session_binding_nonce = Some(request.new_nonce);
            task.interruption.owner_session_id = Some(request.new_thread);
            Ok(Some(()))
        })?;
        Ok(())
    }
}

fn refused() -> PlanError {
    super::super::AttemptBindingRefusalKind::CurrentBindingRequired.refusal()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_old_binding_and_explicit_fresh_authority_gate_thread_replacement() {
        let mut attempt = TaskExecutionAttempt::default();
        attempt.id = "attempt".into();
        attempt.executor_tab_id = "tab".into();
        attempt.provider = Some("opaque-provider".into());
        attempt.session_id = Some("thread".into());
        attempt.session_binding_nonce = Some("old".into());
        let request = RuntimeRestartBinding {
            plan_id: "plan".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            executor_tab_id: "tab".into(),
            provider: "opaque-provider".into(),
            old_thread: "thread".into(),
            old_nonce: "old".into(),
            new_thread: "thread".into(),
            new_nonce: "new".into(),
            owner_epoch: 1,
            mode: RuntimeRestartMode::Resume,
        };
        assert!(permits_rebind(&request, &attempt));
        let mut fresh = request.clone();
        fresh.new_thread = "new-thread".into();
        assert!(!permits_rebind(&fresh, &attempt));
        fresh.mode = RuntimeRestartMode::AuthorizedFresh;
        assert!(permits_rebind(&fresh, &attempt));
        for invalid in [
            RuntimeRestartBinding {
                old_thread: "foreign".into(),
                ..request.clone()
            },
            RuntimeRestartBinding {
                old_nonce: "foreign".into(),
                ..request.clone()
            },
            RuntimeRestartBinding {
                executor_tab_id: "foreign".into(),
                ..request.clone()
            },
            RuntimeRestartBinding {
                attempt_id: "foreign".into(),
                ..request.clone()
            },
        ] {
            assert!(!permits_rebind(&invalid, &attempt));
        }
        attempt.closed_at = Some(1);
        assert!(permits_rebind(&request, &attempt));
        assert!(!permits_rebind(&fresh, &attempt));
        attempt.retired_at = Some(2);
        assert!(!permits_rebind(&request, &attempt));
    }
}
