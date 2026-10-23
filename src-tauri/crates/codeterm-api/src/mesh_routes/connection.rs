//! Local peer management: account-authorized Add and explicit local unpair.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::{check_auth, AppState};
use codeterm_service_core::mesh::account_mesh::{self, AdmissionRefusal};

#[derive(serde::Deserialize)]
pub struct AccountLinkBody {
    pub installation_id: String,
    #[serde(default)]
    pub request_id: Option<String>,
}

pub(crate) fn refusal_status(refusal: &AdmissionRefusal) -> StatusCode {
    match refusal {
        AdmissionRefusal::MachineLimit { .. } => StatusCode::PAYMENT_REQUIRED,
        AdmissionRefusal::SignedOut | AdmissionRefusal::Unauthenticated => StatusCode::UNAUTHORIZED,
        AdmissionRefusal::Revoked => StatusCode::FORBIDDEN,
        AdmissionRefusal::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
        AdmissionRefusal::Offline | AdmissionRefusal::PeerUnreachable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        AdmissionRefusal::Cancelled => StatusCode::CONFLICT,
        AdmissionRefusal::HandshakeFailed { .. } => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

/// Add one same-account machine from the registry; resolves only after the bilateral handshake.
pub async fn mesh_account_link(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<AccountLinkBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let request_id = body
        .request_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if !crate::auth::is_local_caller(&state, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"ok": false, "request_id": request_id})),
        ));
    }
    match account_mesh::link::link_installation(&state.mesh, &body.installation_id).await {
        Ok(pair) => {
            state
                .svc
                .ui
                .emit("mesh-state-changed", serde_json::Value::Null);
            Ok(Json(serde_json::json!({
                "ok": true,
                "request_id": request_id,
                "pair": pair,
            })))
        }
        Err(refusal) => Err((
            refusal_status(&refusal),
            Json(serde_json::json!({
                "ok": false,
                "request_id": request_id,
                "failure": refusal,
            })),
        )),
    }
}

/// Explicit local unpair. Tombstones the edge so no provenance can recreate it; frees no slot.
pub async fn mesh_disconnect_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let arg = body["instance_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    let instance_id = {
        let m = state.mesh.lock();
        let peer = m.paired_peers.get(&arg).cloned().or_else(|| {
            m.paired_peers
                .values()
                .find(|p| p.instance_name == arg)
                .cloned()
        });
        peer.map(|p| p.instance_id).unwrap_or(arg)
    };

    if !state.mesh.lock().paired_peers.contains_key(&instance_id) {
        return Err((
            StatusCode::NOT_FOUND,
            format!("No paired peer matches '{}'", instance_id),
        ));
    }

    state
        .mesh
        .lock()
        .unpair(&instance_id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    state
        .svc
        .ui
        .emit("mesh-state-changed", serde_json::Value::Null);
    log::info!("[mesh/api] Disconnected from {}", instance_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_disconnect_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_disconnect_peer(
        state,
        headers,
        Json(serde_json::json!({"instance_id": instance_id})),
    )
    .await
}
