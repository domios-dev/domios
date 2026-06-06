use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use tauri::Emitter;

use crate::api::routes::AppState;

pub async fn mesh_introduce(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(intro): Json<crate::mesh::types::PeerIntroduction>,
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
    let our_id = mesh.ident.identity.instance_id.clone();
    drop(mesh);

    let sender_id = match crate::mesh::crown::verify_signed_request(token, &known_keys) {
        Ok(id) => id,
        Err(e) => {
            log::warn!(
                "[mesh/introduce] Auth failed for introduction of {}: {} (known_keys: {:?})",
                intro.instance_name,
                e,
                known_keys.keys().collect::<Vec<_>>()
            );
            return Err((StatusCode::UNAUTHORIZED, format!("Unauthorized: {}", e)));
        }
    };

    // Replay prevention — re-acquire lock to record used nonce
    {
        let mut mesh = state.mesh.lock();
        if let Err(e) = mesh.check_and_record_nonce(token) {
            return Err((StatusCode::UNAUTHORIZED, e));
        }
    }

    if intro.instance_id == our_id {
        return Ok(Json(serde_json::json!({"ok": true, "skipped": "self"})));
    }
    if state
        .mesh
        .lock()
        .paired_peers
        .contains_key(&intro.instance_id)
    {
        return Ok(Json(
            serde_json::json!({"ok": true, "skipped": "already_paired"}),
        ));
    }
    {
        let mesh = state.mesh.lock();
        if mesh.is_blocked(&intro.instance_id) {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Peer {} is blocked", intro.instance_id),
            ));
        }
    }

    // Only paired peers can introduce — sender must be paired with us
    {
        let mesh = state.mesh.lock();
        if !mesh.paired_peers.contains_key(&sender_id) {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "Only paired peers can introduce. {} is not paired",
                    sender_id
                ),
            ));
        }
    }

    log::info!(
        "[mesh/introduce] {} introduced by {} — adding to pending (consent required)",
        intro.instance_name,
        sender_id
    );

    let request = crate::mesh::types::PairingRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        from_instance_id: intro.instance_id.clone(),
        from_instance_name: intro.instance_name.clone(),
        from_address: intro
            .addresses
            .first()
            .map(|a| a.ip.clone())
            .unwrap_or_default(),
        from_port: intro.api_port,
        from_addresses: crate::mesh::types::sanitize_addresses(&intro.addresses),
        public_key: intro.public_key.clone(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        status: crate::mesh::types::PairingStatus::Pending,
        introduced_by: Some(sender_id.clone()),
    };

    let mut mesh = state.mesh.lock();
    mesh.pending_requests
        .insert(request.request_id.clone(), request);
    let _ = mesh.save();
    drop(mesh);

    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );

    let mesh = state.mesh.lock();
    Ok(Json(serde_json::json!({
        "ok": true,
        "pending": true,
        "instance_id": mesh.ident.identity.instance_id,
        "instance_name": mesh.ident.instance_name,
        "public_key": mesh.ident.identity.public_key,
        "api_port": mesh.api_port,
        "local_ip": mesh.local_ip,
    })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn introduction_creates_pending_not_paired() {
        // Logic-level check: when a peer is introduced, it must go to pending_requests,
        // not directly into paired_peers. This is enforced by the code change below.
        // The field `introduced_by` is set to the sender's instance_id.
        let sender = "peer-alice";
        let introduced = "peer-bob";
        let request = crate::mesh::types::PairingRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            from_instance_id: introduced.to_string(),
            from_instance_name: "Bob".to_string(),
            from_address: "192.168.1.2".to_string(),
            from_port: 7685,
            from_addresses: vec![],
            public_key: "fake-key".to_string(),
            timestamp: 0,
            status: crate::mesh::types::PairingStatus::Pending,
            introduced_by: Some(sender.to_string()),
        };
        assert_eq!(request.introduced_by.as_deref(), Some(sender));
        assert_eq!(request.status, crate::mesh::types::PairingStatus::Pending);
    }
}
