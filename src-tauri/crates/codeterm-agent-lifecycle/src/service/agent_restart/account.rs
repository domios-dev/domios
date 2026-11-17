use super::{AgentRestartError, TabRestartSnapshot};
use crate::prelude::*;
use crate::service::agent_lifecycle::AccountRestartTarget;
use crate::service::ServiceContext;

pub(super) fn validate_pty(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    target: &AccountRestartTarget,
) -> Result<(), AgentRestartError> {
    target
        .validate_target()
        .map_err(AgentRestartError::BadRequest)?;
    let turn_boundary = svc.tab_is_idle(&snap.tab_id)
        || target.terminal_turn.as_deref().is_some_and(|turn| {
            crate::service::provider_accounts::account_turn_boundary(
                svc,
                &snap.tab_id,
                &target.expected,
                turn,
            )
        });
    if target.expected.unbound_login
        || target.expected.refresh_login
        || target.expected.runtime_epoch.is_some()
        || !turn_boundary
    {
        return Err(AgentRestartError::BadRequest(
            "Account restart requires an idle turn boundary".into(),
        ));
    }
    let mux = svc.mux.read();
    let tab = mux
        .get_tab(&snap.tab_id)
        .ok_or_else(|| AgentRestartError::NotFound(snap.tab_id.clone()))?;
    target
        .expected
        .validate(tab, None)
        .map_err(AgentRestartError::BadRequest)?;
    if tab.pty_id.as_deref() != Some(&snap.pty_id)
        || snap.provider != target.expected.provider
        || snap.session_id.as_deref() != Some(&target.expected.session_id)
    {
        return Err(AgentRestartError::BadRequest(
            "Account restart PTY generation changed".into(),
        ));
    }
    Ok(())
}

pub(super) fn commit_pty(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    target: &AccountRestartTarget,
) -> Result<(), AgentRestartError> {
    if !svc.old_pty_was_torn_down(snap) {
        return Err(AgentRestartError::BadRequest(
            "Account restart requires confirmed PTY cleanup".into(),
        ));
    }
    let mut mux = svc.mux.write();
    let tab = mux
        .get_tab_mut(&snap.tab_id)
        .ok_or_else(|| AgentRestartError::NotFound(snap.tab_id.clone()))?;
    if tab.agent_type.is_some()
        || tab.agent_session_id.is_some()
        || tab.session_binding_nonce.is_some()
    {
        return Err(AgentRestartError::BadRequest(
            "Account restart identity was not retired".into(),
        ));
    }
    target
        .commit_retired_selection(tab)
        .map_err(AgentRestartError::BadRequest)
}

pub(super) fn validate_before_teardown(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    target: &AccountRestartTarget,
) -> Result<(), AgentRestartError> {
    if validate_pty(svc, snap, target).is_ok() {
        return Ok(());
    }
    if target.terminal_turn.is_some() {
        return Err(AgentRestartError::BadRequest(
            "Authenticated terminal turn boundary changed before teardown".into(),
        ));
    }
    let mux = svc.mux.read();
    let tab = mux
        .get_tab(&snap.tab_id)
        .ok_or_else(|| AgentRestartError::NotFound(snap.tab_id.clone()))?;
    if tab.pty_id.as_deref() != Some(&snap.pty_id) {
        return Err(AgentRestartError::BadRequest(
            "Account restart PTY generation changed".into(),
        ));
    }
    target
        .expected
        .validate_retired(tab)
        .map_err(AgentRestartError::BadRequest)
}

#[cfg(test)]
mod tests;
