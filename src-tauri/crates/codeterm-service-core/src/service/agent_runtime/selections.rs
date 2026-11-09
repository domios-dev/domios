use super::{engine, model::RuntimeFailure};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeSettings};

pub(super) fn persist_loaded(
    svc: &ServiceContext,
    session: &engine::Session,
    requested: &RuntimeSettings,
    confirmed: &RuntimeSettings,
) -> Result<(), RuntimeFailure> {
    let requested = RuntimeSettings {
        model: requested
            .model
            .clone()
            .filter(|model| confirmed.model.as_ref() == Some(model)),
        effort: requested
            .effort
            .clone()
            .filter(|effort| confirmed.effort.as_ref() == Some(effort)),
        ..Default::default()
    };
    if requested.model.is_none() && requested.effort.is_none() {
        return Ok(());
    }
    let bound = session.context.lock().clone();
    persist(svc, session, &bound, &requested, confirmed)
}

pub(super) fn persist(
    svc: &ServiceContext,
    session: &engine::Session,
    expected: &RuntimeContext,
    requested: &RuntimeSettings,
    confirmed: &RuntimeSettings,
) -> Result<(), RuntimeFailure> {
    engine::validate_binding(svc, session)?;
    let current = session.context.lock().clone();
    if current.epoch != expected.epoch
        || current.binding_nonce != expected.binding_nonce
        || current.thread_id != expected.thread_id
    {
        return Err(RuntimeFailure::Superseded);
    }
    for (requested, confirmed) in [
        (&requested.model, &confirmed.model),
        (&requested.effort, &confirmed.effort),
    ] {
        if requested.is_some() && requested != confirmed {
            return Err(RuntimeFailure::Refused(
                "Runtime did not confirm requested selection".into(),
            ));
        }
    }
    {
        let mut mux = svc.mux.write();
        let tab = mux
            .get_tab_mut(&expected.tab_id)
            .ok_or(RuntimeFailure::Superseded)?;
        if tab.agent_type.as_deref() != Some(session.provider.as_str())
            || tab.session_binding_nonce.as_deref() != Some(expected.binding_nonce.as_str())
            || tab.agent_session_id != expected.thread_id
        {
            return Err(RuntimeFailure::Superseded);
        }
        if let Some(model) = &requested.model {
            tab.agent_requested_model = Some(model.clone());
            tab.restorable_launch.requested_model = Some(model.clone());
            tab.restorable_launch.model = confirmed.model.clone();
            tab.agent_runtime_config
                .get_or_insert_with(|| serde_json::json!({}))["model"] =
                serde_json::json!(confirmed.model);
        }
        if let Some(effort) = &requested.effort {
            tab.restorable_launch.requested_reasoning_effort = Some(effort.clone());
            tab.restorable_launch.resolved_reasoning_effort = confirmed.effort.clone();
            crate::service::reasoning_effort::persist_completed_launch_identity(
                tab,
                confirmed.effort.as_deref(),
                Some(effort),
            );
            tab.agent_runtime_config
                .get_or_insert_with(|| serde_json::json!({}))["effort"] =
                serde_json::json!(confirmed.effort);
        }
    }
    crate::service::session_persist::persist_session_after_mutation(
        svc,
        crate::service::session_persist::SessionPersistUrgency::Prompt,
    )
    .map_err(RuntimeFailure::Refused)
}
