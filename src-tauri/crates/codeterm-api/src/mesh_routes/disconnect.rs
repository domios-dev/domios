use axum::{extract::State, http::{HeaderMap, StatusCode}, Json};
use codeterm_service_core::mesh::{crown, disconnect::DisconnectNotice};

use crate::routes::AppState;

pub async fn receive_disconnect(State(state): State<AppState>, headers: HeaderMap, Json(notice): Json<DisconnectNotice>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    super::require_current_peer_version(&headers)?;
    let token = headers.get("authorization").and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer ")).unwrap_or_default();
    let mut manager = state.mesh.lock();
    let sender = crown::verify_signed_request(token, &manager.paired_identity_keys())
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Disconnect sender is not paired".into()))?;
    if sender != notice.from { return Err((StatusCode::FORBIDDEN, "Disconnect sender differs".into())); }
    manager.check_and_record_nonce(token).map_err(|error| (StatusCode::UNAUTHORIZED, error))?;
    let removed = manager.apply_disconnect_notice(&notice).map_err(|error| (StatusCode::FORBIDDEN, error))?;
    drop(manager);
    state.svc.ui.emit("mesh-state-changed", serde_json::Value::Null);
    Ok(Json(serde_json::json!({ "ok": true, "removed": removed })))
}
