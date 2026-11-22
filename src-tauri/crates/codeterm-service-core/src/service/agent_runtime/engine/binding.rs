use super::{RuntimeFailure, ServiceContext, Session};

use codeterm_plugin::agent_runtime::RuntimeFailureCode;
use serde_json::Value;

pub(super) fn matches_launch(
    tab: &crate::multiplexer::types::Tab,
    provider: &str,
    account: &str,
    nonce: &str,
) -> bool {
    crate::service::provider_accounts::selected_account(tab) == account
        && tab.agent_type.as_deref() == Some(provider)
        && tab.session_binding_nonce.as_deref() == Some(nonce)
}

pub fn session_load_refused(svc: &ServiceContext, session: &Session) -> bool {
    degraded_unloaded_with(svc, session, &[RuntimeFailureCode::SessionLoadRefused])
}

/// The bound thread can never load again (refused, or its provider record is gone), so only a fresh session continues the tab.
pub fn conversation_unavailable(svc: &ServiceContext, session: &Session) -> bool {
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
    let account = session.account.lock().clone();
    if !crate::service::provider_accounts::invocation_matches(
        session.invocation.lock().as_deref(),
        &session.provider,
        &account,
    ) {
        return Err(RuntimeFailure::Superseded);
    }
    if session.adapter.lock().is_none()
        || svc.agent_registry.provider_arc(&session.provider).is_none()
    {
        return Err(RuntimeFailure::Superseded);
    }
    if svc.mux.read().get_tab(&context.tab_id).is_some_and(|tab| {
        matches_launch(tab, &session.provider, &account, &context.binding_nonce)
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

pub(super) fn bind(
    svc: &ServiceContext,
    session: &Session,
    thread: &str,
    fresh: bool,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let mut mux = svc.mux.write();
    let tab = mux
        .get_tab_mut(&context.tab_id)
        .ok_or(RuntimeFailure::Superseded)?;
    if crate::service::messenger::switch_handoff::launch_authority(svc, &context.tab_id)
        .is_some_and(|authority| !authority.matches(tab))
    {
        return Err(RuntimeFailure::Superseded);
    }
    commit_runtime_binding(tab, &context.binding_nonce, thread, fresh, |tab| {
        svc.publish_boot_stage_transition(
            &context.tab_id,
            tab,
            codeterm_types::pty::BootStage::AgentReady,
            None,
        );
    })
}

pub(super) fn commit_runtime_binding(
    tab: &mut crate::multiplexer::types::Tab,
    nonce: &str,
    thread: &str,
    fresh: bool,
    publish: impl FnOnce(&crate::multiplexer::types::Tab),
) -> Result<(), RuntimeFailure> {
    if tab.session_binding_nonce.as_deref() != Some(nonce)
        || tab
            .agent_session_id
            .as_deref()
            .is_some_and(|bound| bound != thread)
    {
        return Err(RuntimeFailure::Superseded);
    }
    tab.agent_session_id = Some(thread.into());
    tab.reported_session_binding_nonce = Some(nonce.into());
    if fresh {
        let config = tab
            .agent_runtime_config
            .get_or_insert_with(|| serde_json::json!({}));
        config["unusedThreadId"] = Value::String(thread.into());
    }
    tab.agent_booting = false;
    crate::service::boot_stage::apply_boot_stage(
        tab,
        codeterm_types::pty::BootStage::AgentReady,
        None,
    );
    if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } = &mut tab.backing
    {
        *session_id = thread.into();
    }
    publish(tab);
    Ok(())
}
