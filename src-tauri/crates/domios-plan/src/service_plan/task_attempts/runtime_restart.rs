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
            *attempt = resolve_runtime_restart_attempt(
                &request,
                attempt,
                &request.executor_tab_id,
                &request.new_nonce,
            )
            .ok_or_else(refused)?;
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

    fn attempt() -> TaskExecutionAttempt {
        let mut attempt = TaskExecutionAttempt::default();
        attempt.id = "attempt".into();
        attempt.executor_tab_id = "tab".into();
        attempt.provider = Some("opaque-provider".into());
        attempt.session_id = Some("thread".into());
        attempt.session_binding_nonce = Some("old".into());
        attempt
    }

    fn request() -> RuntimeRestartBinding {
        RuntimeRestartBinding {
            plan_id: "plan".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            executor_tab_id: "tab".into(),
            provider: "opaque-provider".into(),
            target_provider: None,
            old_thread: "thread".into(),
            old_nonce: "old".into(),
            new_thread: "thread".into(),
            new_nonce: "new".into(),
            owner_epoch: 1,
            mode: RuntimeRestartMode::Resume,
        }
    }

    #[test]
    fn resume_restart_carries_the_same_attempt_to_the_new_launch_nonce() {
        let request = request();
        let before = attempt();

        let after = resolve_runtime_restart_attempt(&request, &before, "tab", "new")
            .expect("the authenticated resumed launch carries its attempt");

        assert_eq!(after.id, before.id);
        assert_eq!(after.executor_tab_id, before.executor_tab_id);
        assert_eq!(after.session_id.as_deref(), Some("thread"));
        assert_eq!(after.session_binding_nonce.as_deref(), Some("new"));
    }

    #[test]
    fn model_switch_resume_carries_the_same_attempt_to_the_new_launch_nonce() {
        let request = RuntimeRestartBinding {
            mode: RuntimeRestartMode::ModelSwitch,
            ..request()
        };
        let before = attempt();

        let after = resolve_runtime_restart_attempt(&request, &before, "tab", "new")
            .expect("a model switch preserving the provider session keeps the attempt");

        assert_eq!(after.id, before.id);
        assert_eq!(after.provider, before.provider);
        assert_eq!(after.session_binding_nonce.as_deref(), Some("new"));
    }

    #[test]
    fn runtime_restart_resolution_refuses_a_foreign_tab() {
        assert!(
            resolve_runtime_restart_attempt(&request(), &attempt(), "foreign-tab", "new").is_none()
        );
    }

    #[test]
    fn provider_switch_carries_exact_attempt_and_refuses_stale_authority() {
        let mut request = RuntimeRestartBinding {
            mode: RuntimeRestartMode::ProviderSwitch,
            target_provider: Some("target-provider".into()),
            new_thread: "target-thread".into(),
            ..request()
        };
        let before = attempt();
        let after = resolve_runtime_restart_attempt(&request, &before, "tab", "new").unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.executor_tab_id, before.executor_tab_id);
        assert_eq!(after.provider.as_deref(), Some("target-provider"));
        assert_eq!(after.session_id.as_deref(), Some("target-thread"));
        assert!(resolve_runtime_restart_attempt(&request, &after, "tab", "new").is_none());
        request.mode = RuntimeRestartMode::Resume;
        assert!(!permits_rebind(&request, &before));
        request.mode = RuntimeRestartMode::ProviderSwitch;
        let mut closed = before;
        closed.closed_at = Some(1);
        assert!(!permits_rebind(&request, &closed));
        request.new_thread = request.old_thread.clone();
        assert!(!permits_rebind(&request, &closed));
    }

    #[test]
    fn runtime_restart_resolution_refuses_the_old_nonce_after_carry() {
        let request = request();
        let rebound = resolve_runtime_restart_attempt(&request, &attempt(), "tab", "new")
            .expect("initial carry is authorized");

        assert!(resolve_runtime_restart_attempt(&request, &rebound, "tab", "old").is_none());
    }

    #[test]
    fn exact_old_binding_and_explicit_fresh_authority_gate_thread_replacement() {
        let mut attempt = attempt();
        let request = request();
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
