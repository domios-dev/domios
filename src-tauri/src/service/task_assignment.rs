use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab};
use crate::service::plan::interruption::TaskMutationAuthority;
use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
use crate::service::plan::{PlanError, PlanStore};

pub(crate) fn authenticated_tab_binding(tab: &Tab) -> Option<AssignmentRuntimeBinding> {
    let session_id = tab.agent_session_id.as_ref()?;
    let nonce = tab.session_binding_nonce.as_ref()?;
    (!session_id.trim().is_empty()
        && !nonce.trim().is_empty()
        && tab.reported_session_binding_nonce.as_ref() == Some(nonce))
    .then(|| AssignmentRuntimeBinding {
        session_id: session_id.clone(),
        session_binding_nonce: nonce.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_existing_task(
    store: &PlanStore,
    mux: &MultiplexerState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    peer: Option<&str>,
    authority: Option<&TaskMutationAuthority>,
    now: u64,
) -> Result<(), PlanError> {
    if peer.is_some_and(|peer| peer.trim().is_empty()) {
        return Err(PlanError::Io("assignment peer must not be empty".into()));
    }
    if let Some(authority) = authority {
        let caller = mux
            .get_tab(&authority.tab_id)
            .and_then(authenticated_tab_binding);
        if !caller.is_some_and(|binding| {
            binding.session_id == authority.session_id
                && binding.session_binding_nonce == authority.session_binding_nonce
        }) {
            return Err(PlanError::Io("task mutation caller rebound".into()));
        }
    }
    let (binding, role, session_model) = if peer.is_some() {
        (None, None, None)
    } else {
        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| PlanError::Io("assignment target tab is not live".into()))?;
        let binding = authenticated_tab_binding(tab)
            .ok_or_else(|| PlanError::Io("assignment target binding is not verified".into()))?;
        let role = (tab.role == Some(AgentRole::Manager)
            && tab.role_profile.as_deref() == Some("planner"))
        .then_some("planner");
        (
            Some(binding),
            role,
            tab.agent_session_model.clone(),
        )
    };
    store.assign_task_with_runtime_binding(
        plan_id,
        task_id,
        tab_id,
        peer,
        role,
        binding.as_ref(),
        authority,
        now,
    )?;
    if let (Some(binding), Some(model)) = (binding, session_model) {
        if let Err(error) = store.update_attempt_session_model_if_unknown(
            plan_id,
            task_id,
            tab_id,
            &binding.session_id,
            &binding.session_binding_nonce,
            &model,
            now,
        ) {
            log::warn!("[task-assignment] failed to persist bound session model: {error}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
