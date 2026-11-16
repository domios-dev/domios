use super::{RuntimeFailure, ServiceContext, Session};

use codeterm_plugin::agent_runtime::RuntimeFailureCode;

pub(crate) fn session_load_refused(svc: &ServiceContext, session: &Session) -> bool {
    degraded_unloaded_with(svc, session, &[RuntimeFailureCode::SessionLoadRefused])
}

/// The bound thread can never load again (refused, or its provider record is gone), so only a fresh session continues the tab.
pub(crate) fn conversation_unavailable(svc: &ServiceContext, session: &Session) -> bool {
    degraded_unloaded_with(
        svc,
        session,
        &[
            RuntimeFailureCode::SessionLoadRefused,
            RuntimeFailureCode::ThreadNotFound,
        ],
    )
}

fn degraded_unloaded_with(
    svc: &ServiceContext,
    session: &Session,
    codes: &[RuntimeFailureCode],
) -> bool {
    let matched = {
        let state = session.state.lock();
        matches!(state.phase, super::RuntimePhase::Degraded { .. })
            && state.failure_code.is_some_and(|code| codes.contains(&code))
    };
    matched
        && !session
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire)
        && validate_binding(svc, session).is_ok()
}

pub fn validate_binding(svc: &ServiceContext, session: &Session) -> Result<(), RuntimeFailure> {
    validate_binding_inner(svc, session, true)
}

pub(in crate::service::agent_runtime) fn validate_restart_binding(
    svc: &ServiceContext,
    session: &Session,
) -> Result<(), RuntimeFailure> {
    validate_binding_inner(svc, session, false)
}

fn validate_binding_inner(
    svc: &ServiceContext,
    session: &Session,
    require_thread: bool,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    if session.adapter.lock().is_none()
        || svc.agent_registry.provider_arc(&session.provider).is_none()
    {
        return Err(RuntimeFailure::Superseded);
    }
    if svc.mux.read().get_tab(&context.tab_id).is_some_and(|tab| {
        tab.agent_type.as_deref() == Some(session.provider.as_str())
            && tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
            && (!require_thread
                || context
                    .thread_id
                    .as_deref()
                    .is_none_or(|thread| tab.agent_session_id.as_deref() == Some(thread)))
    }) {
        Ok(())
    } else {
        Err(RuntimeFailure::Superseded)
    }
}
