use axum::{body::Body, extract::State as AxumState, http::StatusCode, response::Response, Json};
use futures_util::StreamExt;

use super::connection::refusal_status;
use crate::routes::AppState;
use codeterm_service_core::mesh::account_mesh::protocol::{
    LinkCommit, LinkCommitReply, LinkConfirm, LinkConfirmReply, LinkHello, LinkHelloReply,
};
use codeterm_service_core::mesh::account_mesh::AdmissionRefusal;

type Refused = (StatusCode, Json<serde_json::Value>);

fn refused(refusal: AdmissionRefusal) -> Refused {
    (
        refusal_status(&refusal),
        Json(serde_json::to_value(&refusal).unwrap_or_default()),
    )
}

/// Ends a long-lived peer body the moment that edge is blocked (sign-out, revoke, expiry).
pub(crate) fn fence_body(
    body: Body,
    mesh: codeterm_service_core::MeshStateHandle,
    peer_id: String,
) -> Body {
    let blocked =
        Box::pin(codeterm_service_core::mesh::account_mesh::wait_until_blocked(mesh, peer_id));
    Body::from_stream(body.into_data_stream().take_until(blocked))
}

pub(crate) fn fence_response(
    response: Response,
    mesh: codeterm_service_core::MeshStateHandle,
    peer_id: String,
) -> Response {
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, fence_body(body, mesh, peer_id))
}

pub async fn mesh_link_hello(
    AxumState(state): AxumState<AppState>,
    Json(hello): Json<LinkHello>,
) -> Result<Json<LinkHelloReply>, Refused> {
    let reply = state
        .mesh
        .lock()
        .accept_link_hello(hello)
        .map_err(refused)?;
    Ok(Json(reply))
}

pub async fn mesh_link_confirm(
    AxumState(state): AxumState<AppState>,
    Json(confirm): Json<LinkConfirm>,
) -> Result<Json<LinkConfirmReply>, Refused> {
    let confirmation = state
        .mesh
        .lock()
        .begin_link_confirm(&confirm)
        .map_err(refused)?;
    let permit = confirmation
        .authority
        .authorize_link(confirmation.request.clone())
        .await
        .map_err(refused)?;
    state
        .mesh
        .lock()
        .finish_link_confirm(confirmation)
        .map_err(refused)?;
    Ok(Json(LinkConfirmReply { permit }))
}

pub async fn mesh_link_commit(
    AxumState(state): AxumState<AppState>,
    Json(commit): Json<LinkCommit>,
) -> Result<Json<LinkCommitReply>, Refused> {
    state.mesh.lock().commit_link(&commit).map_err(refused)?;
    state
        .svc
        .ui
        .emit("mesh-state-changed", serde_json::Value::Null);
    Ok(Json(LinkCommitReply { committed: true }))
}
