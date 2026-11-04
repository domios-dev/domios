use crate::multiplexer::state::MultiplexerState;
use crate::plan::TaskStatus;
use crate::service::plan::{PlanError, PlanStore};
use crate::service::{
    restore_attempt_binding_for_mux, AttemptBindingRestoreTarget, AuthenticatedPlanCaller,
};

pub fn restore_review_attempt_binding_for_mux(
    store: &PlanStore,
    mux: &MultiplexerState,
    plan_id: &str,
    task_id: &str,
    now_ms: u64,
) -> Result<(), PlanError> {
    let task = store.review_binding_restore_projection(plan_id, task_id)?;
    if task.status != TaskStatus::InReview {
        return Ok(());
    }
    let attempt = match task.attempts.as_slice() {
        [] => return Ok(()),
        [attempt] => attempt,
        _ => {
            return Err(PlanError::Io(
                "review bounce has ambiguous current attempts".into(),
            ))
        }
    };
    let Some(tab) = mux.get_tab(&attempt.executor_tab_id) else {
        return Ok(());
    };
    let Some(binding) = crate::service::task_assignment::authenticated_tab_binding(tab) else {
        return Ok(());
    };
    if attempt.session_binding_nonce.as_deref() == Some(&binding.session_binding_nonce) {
        return Ok(());
    }
    restore_attempt_binding_for_mux(
        store,
        mux,
        AttemptBindingRestoreTarget {
            plan_id,
            task_id: &task.task_id,
            attempt_id: &attempt.id,
            owner_epoch: task.owner_epoch,
        },
        &AuthenticatedPlanCaller {
            tab_id: Some(tab.id.clone()),
            session_id: Some(binding.session_id),
            binding_nonce: Some(binding.session_binding_nonce),
        },
        now_ms,
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests;
