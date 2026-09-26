use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab};
use crate::service::plan::interruption::TaskMutationAuthority;
use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
use crate::service::plan::{PlanError, PlanStore};

pub(crate) struct ExistingTaskAssignment {
    binding: Option<AssignmentRuntimeBinding>,
    role: Option<String>,
}

pub(crate) fn authenticated_tab_binding(tab: &Tab) -> Option<AssignmentRuntimeBinding> {
    let session_id = tab.agent_session_id.as_ref()?;
    let nonce = tab.session_binding_nonce.as_ref()?;
    (!session_id.trim().is_empty()
        && !nonce.trim().is_empty()
        && tab.reported_session_binding_nonce.as_ref() == Some(nonce))
    .then(|| AssignmentRuntimeBinding {
        session_id: session_id.clone(),
        session_binding_nonce: nonce.clone(),
        provider: tab.agent_type.clone(),
        model: tab.agent_provider_resolved_model.clone(),
        reasoning_effort: tab.reasoning_effort_identity.resolved.clone(),
        session_model: tab
            .agent_session_model_identity
            .as_ref()
            .filter(|identity| {
                identity.session_id.as_str() == session_id.as_str()
                    && identity.session_binding_nonce.as_str() == nonce.as_str()
            })
            .and_then(|_| tab.agent_session_model.clone()),
    })
}

pub(crate) fn prepare_existing_task_assignment(
    mux: &MultiplexerState,
    tab_id: &str,
    peer: Option<&str>,
    authority: Option<&TaskMutationAuthority>,
) -> Result<ExistingTaskAssignment, PlanError> {
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
    let (binding, role) = if peer.is_some() {
        (None, None)
    } else {
        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| PlanError::Io("assignment target tab is not live".into()))?;
        let binding = authenticated_tab_binding(tab)
            .ok_or_else(|| PlanError::Io("assignment target binding is not verified".into()))?;
        let role = (tab.role == Some(AgentRole::Manager)
            && tab.role_profile.as_deref() == Some("planner"))
        .then_some("planner".to_string());
        (Some(binding), role)
    };
    Ok(ExistingTaskAssignment { binding, role })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn persist_existing_task_assignment(
    store: &PlanStore,
    assignment: ExistingTaskAssignment,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    peer: Option<&str>,
    authority: Option<&TaskMutationAuthority>,
    now: u64,
) -> Result<(), PlanError> {
    store.assign_task_with_runtime_binding(
        plan_id,
        task_id,
        tab_id,
        peer,
        assignment.role.as_deref(),
        assignment.binding.as_ref(),
        authority,
        now,
    )
}

#[cfg(test)]
mod tests;
