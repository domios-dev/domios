use crate::routes::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::mesh::{crown, presence_notice::PresenceNotice, MeshManager};

pub async fn account_presence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(notice): Json<PresenceNotice>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    super::require_current_peer_version(&headers)?;
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    let sender = authorize_notice(&mut state.mesh.lock(), token)?;
    let nonce = crown::nonce_from_token(token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid peer notice".into()))?;
    let refreshed = match notice {
        PresenceNotice::Endpoint { endpoint_record } => {
            state
                .mesh
                .lock()
                .verify_endpoint_record(&endpoint_record)
                .map_err(|_| (StatusCode::FORBIDDEN, "Invalid endpoint record".to_owned()))?;
            let accepted = !codeterm_service_core::mesh::endpoint_gossip::receive_endpoint_records(
                &state.mesh,
                vec![endpoint_record.clone()],
            )
            .await
            .is_empty();
            if accepted {
                codeterm_service_core::mesh::presence_notice::broadcast_endpoint_record(
                    &state.mesh,
                    endpoint_record,
                )
                .await;
            }
            Ok(accepted)
        }
        PresenceNotice::Snapshot { signed_registry } => {
            state
                .svc
                .account_mesh_registry_snapshot(&signed_registry)
                .await
        }
        PresenceNotice::Presence { metadata_revision } => {
            state
                .svc
                .account_mesh_presence_notice(&sender, &nonce, metadata_revision)
                .await
        }
    }
    .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error))?;
    Ok(Json(serde_json::json!({"ok":true,"refreshed":refreshed})))
}

fn authorize_notice(mesh: &mut MeshManager, token: &str) -> Result<String, (StatusCode, String)> {
    if mesh.account_state().signed_in_account().is_none() {
        return Err((StatusCode::FORBIDDEN, "Mesh is signed out".into()));
    }
    let sender = crown::verify_signed_request(token, &mesh.paired_identity_keys())
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid peer signature".into()))?;
    if mesh.account_peer_revoked(&sender) {
        return Err((StatusCode::FORBIDDEN, "Peer is removed".into()));
    }
    mesh.check_and_record_nonce(token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Repeated peer notice".into()))?;
    Ok(sender)
}

#[cfg(test)]
mod tests;
