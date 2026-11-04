use axum::http::{HeaderMap, StatusCode};
use codeterm_service_core::mesh::crown::{verify_request_claims, AuthenticatedAgent};

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
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    if let Some(caller) = state.svc.identify_tab_credential(token) {
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
        return Ok(Some(AuthenticatedAgent {
            instance_id: state.mesh.lock().ident.identity.instance_id.clone(),
            tab_id: caller.tab_id,
            can_spawn,
        }));
    }
    if authenticated_origin(state, headers)? != AuthenticatedOrigin::Agent {
        return Ok(None);
    }
    let (keys, _) = super::mesh_keys(state);
    verify_request_claims(token, &keys)
        .map(|claims| claims.agent)
        .map_err(|error| (StatusCode::FORBIDDEN, error))
}
