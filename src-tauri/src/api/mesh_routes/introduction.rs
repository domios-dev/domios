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
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
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
            log::warn!("[mesh/introduce] Auth failed for introduction of {}: {} (known_keys: {:?})",
                intro.instance_name, e, known_keys.keys().collect::<Vec<_>>());
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
    if state.mesh.lock().paired_peers.contains_key(&intro.instance_id) {
        return Ok(Json(serde_json::json!({"ok": true, "skipped": "already_paired"})));
    }
    {
        let mesh = state.mesh.lock();
        if mesh.is_blocked(&intro.instance_id) {
            return Err((StatusCode::FORBIDDEN, format!("Peer {} is blocked", intro.instance_id)));
        }
    }

    // Only paired peers can introduce — sender must be paired with us
    {
        let mesh = state.mesh.lock();
        if !mesh.paired_peers.contains_key(&sender_id) {
            return Err((StatusCode::FORBIDDEN,
                format!("Only paired peers can introduce. {} is not paired", sender_id)));
        }
    }

    log::info!("[mesh/introduce] {} introduced by {} — auto-pairing", intro.instance_name, sender_id);

    let mut mesh = state.mesh.lock();
    let peer = crate::mesh::types::PairedPeer {
        instance_id: intro.instance_id.clone(),
        instance_name: intro.instance_name.clone(),
        addresses: crate::mesh::types::sanitize_addresses(&intro.addresses),
        api_port: intro.api_port,
        public_key: intro.public_key.clone(),
        paired_at: chrono::Utc::now().to_rfc3339(),
        last_seen: Some(chrono::Utc::now().to_rfc3339()),
        online: true, is_primary: false,
        access_level: crate::mesh::types::AccessLevel::Full,
        version: None,
    };
    mesh.paired_peers.insert(intro.instance_id.clone(), peer);
    mesh.discovered_peers.remove(&intro.instance_id);
    let _ = mesh.save();
    drop(mesh);

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());

    {
        let mesh_state = state.mesh.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            crate::mesh::broadcast_sync(&mesh_state).await;
        });
    }

    let mesh = state.mesh.lock();
    Ok(Json(serde_json::json!({
        "ok": true,
        "instance_id": mesh.ident.identity.instance_id,
        "instance_name": mesh.ident.instance_name,
        "public_key": mesh.ident.identity.public_key,
        "api_port": mesh.api_port,
        "local_ip": mesh.local_ip,
    })))
}
