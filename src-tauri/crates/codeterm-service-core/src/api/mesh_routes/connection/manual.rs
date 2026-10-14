use super::{connect_peer_at, ConnectPeerError, ConnectPeerResult};
use crate::api::routes::AppState;
use crate::mesh::manual::{IdentityChallenge, IdentityProof};
use axum::{extract::State, http::StatusCode, Json};

pub async fn mesh_pair_identity(
    State(state): State<AppState>,
    Json(challenge): Json<IdentityChallenge>,
) -> Result<Json<IdentityProof>, (StatusCode, String)> {
    IdentityProof::from_manager(&state.mesh.lock(), challenge)
        .map(Json)
        .map_err(|error| (StatusCode::CONFLICT, error))
}

pub(super) async fn connect_from_body(
    mesh: crate::MeshStateHandle,
    mut target: String,
    body: &serde_json::Value,
) -> Result<ConnectPeerResult, ConnectPeerError> {
    if target.is_empty() {
        return Err(ConnectPeerError::BadRequest(
            "Missing peer identity or name".into(),
        ));
    }
    let mut endpoint = None;
    if let Some(address) = body.get("addr") {
        let address = address
            .as_str()
            .ok_or_else(|| ConnectPeerError::BadRequest("addr must be an IP:port".into()))?
            .parse::<std::net::SocketAddr>()
            .map_err(|_| ConnectPeerError::BadRequest("addr must be an IP:port".into()))?;
        endpoint = Some(address);
        target = crate::mesh::manual::discover_at(&mesh, address, Some(&target))
            .await
            .map_err(ConnectPeerError::BadRequest)?;
    }
    connect_peer_at(mesh, target, endpoint).await
}

#[cfg(test)]
mod tests;
