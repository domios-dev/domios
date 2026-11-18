use super::*;
use codeterm_service_core::mesh::{account_mesh::ledger::LinkAdmission, types::AccessLevel};

fn refused() -> Response {
    (
        StatusCode::FORBIDDEN,
        "Receiver grant reads require an admitted Full peer on the current account",
    )
        .into_response()
}

fn admit(state: &AppState, headers: &AgentHeaders, uid: &str) -> Result<(), Response> {
    if !matches!(headers.actor, RequestActor::Peer) || !state.config.lock().mesh.enabled {
        return Err(refused());
    }
    let CallerIdentity::Peer { instance_id, .. } =
        crate::auth::caller_identity(state, headers).map_err(IntoResponse::into_response)?
    else {
        return Err(refused());
    };
    let mesh = state.mesh.lock();
    let allowed = mesh.account_state().signed_in_account() == Some(uid)
        && mesh.traffic_open(&instance_id)
        && mesh
            .paired_peers
            .get(&instance_id)
            .is_some_and(|peer| peer.access_level == AccessLevel::Full)
        && mesh.link_record(&instance_id).is_some_and(|link| {
            matches!(&link.admission, LinkAdmission::Account { account_id, .. } if account_id == uid)
        });
    if allowed {
        Ok(())
    } else {
        Err(refused())
    }
}

pub(super) async fn read(
    state: &AppState,
    headers: &AgentHeaders,
    command: NeighborRuntimeCommand,
) -> Result<Json<serde_json::Value>, Response> {
    if !matches!(command, NeighborRuntimeCommand::Permission { .. }) {
        return Err(refused());
    }
    let account = state.svc.account().await.map_err(|_| refused())?;
    let session = account.session().await.ok_or_else(refused)?;
    admit(state, headers, &session.identity.uid)?;
    let result = super::super::neighbor_runtime(
        State(state.clone()),
        Some(Extension(CallerIdentity::Local)),
        Default::default(),
        Json(command),
    )
    .await;
    let current = account.session().await.ok_or_else(refused)?;
    if current.identity.uid != session.identity.uid
        || current.login_generation != session.login_generation
    {
        return Err(refused());
    }
    admit(state, headers, &session.identity.uid)?;
    result
}

#[cfg(test)]
mod tests;
