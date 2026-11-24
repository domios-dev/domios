use axum::http::{HeaderMap, StatusCode};
use codeterm_service_core::mesh::crown::{verify_request_claims, AuthenticatedAgent};
use codeterm_service_core::service::general_agent::session_contract::owns_main;
use codeterm_service_core::service::tab_credentials::TabCaller;

use super::{authenticated_origin, AuthenticatedOrigin, RequestActor};
use crate::routes::AppState;

pub(crate) fn remote_agent(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<AuthenticatedAgent>, (StatusCode, String)> {
    Ok(match RequestActor::authenticate(state, headers)? {
        RequestActor::RemoteAgent(agent) => Some(agent),
        _ => None,
    })
}

pub(crate) fn authenticated_agent(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<AuthenticatedAgent>, (StatusCode, String)> {
    let token = bearer_token(headers);
    if let Some(agent) = tab_credential_agent(state, token)? {
        return Ok(Some(agent));
    }
    let origin = authenticated_origin(state, headers)?;
    signed_agent_for_origin(state, token, origin)
}

pub(crate) fn authenticated_agent_for_origin(
    state: &AppState,
    headers: &HeaderMap,
    origin: AuthenticatedOrigin,
) -> Result<Option<AuthenticatedAgent>, (StatusCode, String)> {
    let token = bearer_token(headers);
    if let Some(agent) = tab_credential_agent(state, token)? {
        return Ok(Some(agent));
    }
    signed_agent_for_origin(state, token, origin)
}

fn bearer_token(headers: &HeaderMap) -> &str {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default()
}

pub(crate) fn is_bound_main(state: &AppState, caller: &TabCaller) -> bool {
    let mux = state.svc.mux.read();
    mux.get_tab(&caller.tab_id).is_some_and(|tab| {
        tab.protected
            && owns_main(&mux, tab)
            && tab.team_id.is_none()
            && tab.parent_tab_id.is_none()
            && tab.neighbor_authority.is_none()
            && tab.launch_session_authenticated()
            && tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
    })
}

fn tab_credential_agent(
    state: &AppState,
    token: &str,
) -> Result<Option<AuthenticatedAgent>, (StatusCode, String)> {
    let Some(caller) = state.svc.identify_tab_credential(token) else {
        return Ok(None);
    };
    let mux = state.svc.mux.read();
    let tab = mux
        .get_tab(&caller.tab_id)
        .ok_or((StatusCode::FORBIDDEN, "Originating agent has closed".into()))?;
    if tab.session_binding_nonce.as_deref() != Some(caller.binding_nonce.as_str()) {
        return Err((
            StatusCode::FORBIDDEN,
            "Originating agent launch has changed".into(),
        ));
    }
    let can_spawn = tab.can_spawn;
    drop(mux);
    let can_spawn = can_spawn
        && codeterm_service_core::service::execution_sandbox_refuse_confined_mesh_delegation(
            &state.svc,
            &[Some(caller.tab_id.as_str())],
        )
        .is_ok();
    Ok(Some(AuthenticatedAgent {
        instance_id: state.mesh.lock().ident.identity.instance_id.clone(),
        tab_id: caller.tab_id,
        can_spawn,
    }))
}

fn signed_agent_for_origin(
    state: &AppState,
    token: &str,
    origin: AuthenticatedOrigin,
) -> Result<Option<AuthenticatedAgent>, (StatusCode, String)> {
    if origin != AuthenticatedOrigin::Agent {
        return Ok(None);
    }
    let (keys, _) = super::mesh_keys(state);
    verify_request_claims(token, &keys)
        .map(|claims| claims.agent.filter(|_| claims.appearance_store.is_none()))
        .map_err(|error| (StatusCode::FORBIDDEN, error))
}
