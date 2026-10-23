//! Explicit pairing removal and owner-lifted revocation.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::AppState;

pub async fn mesh_unpair(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.ident.identity.public_key {
        known_keys.insert(mesh.ident.identity.instance_id.clone(), pk.clone());
    }
    drop(mesh);

    let sender_id = codeterm_service_core::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()))?;

    {
        let mut mesh = state.mesh.lock();
        if let Err(e) = mesh.check_and_record_nonce(token) {
            return Err((StatusCode::UNAUTHORIZED, e));
        }
    }

    log::info!(
        "[mesh/unpair] sender={} removing {}",
        sender_id,
        instance_id
    );
    if sender_id != instance_id {
        return Err((
            StatusCode::FORBIDDEN,
            format!("Peer {} cannot remove pairing {}", sender_id, instance_id),
        ));
    }

    let mut mesh = state.mesh.lock();
    if mesh
        .unpair(&instance_id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?
        .is_some()
    {
        log::info!("[mesh] Unpaired from {}", instance_id);
    }
    drop(mesh);

    state
        .svc
        .ui
        .emit("mesh-state-changed", serde_json::Value::Null);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_unrevoke(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let local_token = state.config.lock().api.token.clone().unwrap_or_default();
    if !mesh_owner_authorized(&headers, &local_token) {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Owner authorization required".into(),
        ));
    }
    let instance_id = body["instance_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?;
    {
        let mut mesh = state.mesh.lock();
        mesh.unrevoke(instance_id)
            .map_err(|error| (StatusCode::CONFLICT, error))?;
        mesh.save()
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    }
    log::warn!(
        "[mesh] Owner explicitly lifted revocation for peer {}",
        instance_id
    );
    state
        .svc
        .ui
        .emit("mesh-state-changed", serde_json::Value::Null);
    Ok(Json(
        serde_json::json!({"ok": true, "instance_id": instance_id}),
    ))
}

fn mesh_owner_authorized(headers: &HeaderMap, local_token: &str) -> bool {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    !token.is_empty()
        && !local_token.is_empty()
        && codeterm_service_core::service::tokens::constant_time_eq(
            token.as_bytes(),
            local_token.as_bytes(),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn unrevoke_requires_the_local_owner_token() {
        let local = "Bearer local-token";
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("authorization", local.parse().unwrap());
        assert!(super::mesh_owner_authorized(&headers, "local-token"));
        assert!(!super::mesh_owner_authorized(&headers, "other-token"));
        assert!(!super::mesh_owner_authorized(&headers, ""));
    }
}
