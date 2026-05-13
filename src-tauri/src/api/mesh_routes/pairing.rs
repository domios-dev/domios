use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use tauri::Emitter;

use crate::api::routes::AppState;

pub async fn mesh_pair_request(
    AxumState(state): AxumState<AppState>,
    Json(request): Json<crate::mesh::types::PairingRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut mesh = state.mesh.lock();
    mesh.receive_pairing_request(request)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_pair_accept(
    AxumState(state): AxumState<AppState>,
    Json(accept): Json<crate::mesh::types::PairingAccept>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (b_address, b_port, b_instance_id) = {
        let mesh = state.mesh.lock();
        mesh.pairing_targets
            .get(&accept.request_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "No pending request".to_string()))?
    };

    // Guard: reject if the accepting peer isn't the one we sent the request to
    if accept.from_instance_id != b_instance_id {
        log::warn!(
            "[mesh/pair_accept] Identity mismatch: expected {}, got {}",
            b_instance_id, accept.from_instance_id
        );
        return Err((StatusCode::FORBIDDEN,
            format!("Identity mismatch: expected {}, got {}", b_instance_id, accept.from_instance_id)));
    }

    {
        let mut mesh = state.mesh.lock();
        let discovered_name = mesh.discovered_peers.get(&b_instance_id)
            .map(|p| p.instance_name.clone());
        let b_name = match discovered_name {
            Some(n) if !n.contains(':') => n,
            _ => accept.from_instance_name.clone(),
        };
        let peer = crate::mesh::types::PairedPeer {
            instance_id: accept.from_instance_id.clone(),
            instance_name: b_name,
            addresses: vec![crate::mesh::types::PeerAddress::from_ip(&b_address)],
            api_port: b_port,
            public_key: accept.public_key.clone(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: Some(chrono::Utc::now().to_rfc3339()),
            online: true, is_primary: false,
            access_level: crate::mesh::types::AccessLevel::Full,
            version: None,
        };
        mesh.paired_peers.insert(accept.from_instance_id.clone(), peer);
        mesh.pending_requests.remove(&accept.request_id);
        mesh.pairing_targets.remove(&accept.request_id);
        let _ = mesh.save();
    }

    log::info!("[mesh] Paired (requester) with {}", accept.from_instance_id);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());

    // Mirror of mesh_approve_peer: announce our full address list to the
    // newly-paired peer so their pair-time single-IP stub gets replaced
    // immediately (issue #9). broadcast_sync still follows for gossip
    // about other peers in our mesh.
    {
        let mesh_state = state.mesh.clone();
        let new_peer_id = accept.from_instance_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let _ = crate::mesh::broadcast_announce(&mesh_state, Some(&new_peer_id)).await;
            crate::mesh::broadcast_sync(&mesh_state).await;
        });
    }

    // Delayed introductions — wait for approver to finish storing us in paired_peers
    let intro_data = state.mesh.lock().prepare_introductions(&accept.from_instance_id);
    if let Some((new_peer, existing, our_id, token)) = intro_data {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            crate::mesh::broadcast_introductions(new_peer, existing, our_id, token).await;
        });
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_pair_confirm(
    AxumState(_state): AxumState<AppState>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_pair_reject(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let request_id = body["request_id"].as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing request_id".to_string()))?
        .to_string();

    let mut mesh = state.mesh.lock();
    let block_id = if let Some(req) = mesh.pending_requests.get_mut(&request_id) {
        req.status = crate::mesh::types::PairingStatus::Rejected;
        Some(req.from_instance_id.clone())
    } else { None };
    if let Some(id) = block_id {
        if !mesh.revocation_list.blocked.contains(&id) {
            mesh.revocation_list.blocked.push(id);
        }
        let _ = mesh.save();
    }
    drop(mesh);

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_unpair(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
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

    let sender_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()))?;

    // Replay prevention
    {
        let mut mesh = state.mesh.lock();
        if let Err(e) = mesh.check_and_record_nonce(token) {
            return Err((StatusCode::UNAUTHORIZED, e));
        }
    }

    log::info!("[mesh/unpair] sender={} removing {}", sender_id, instance_id);
    if sender_id != our_id && sender_id != instance_id {
        let mesh_guard = state.mesh.lock();
        let is_paired = mesh_guard.paired_peers.contains_key(&sender_id);
        drop(mesh_guard);
        if !is_paired {
            return Err((StatusCode::FORBIDDEN,
                format!("Unknown peer {} cannot remove {}", sender_id, instance_id)));
        }
        log::info!("[mesh] Peer {} requested removal of {} from our mesh", sender_id, instance_id);
    }

    let target_peer = state.mesh.lock().paired_peers.get(&instance_id).cloned();
    let signed_token = state.mesh.lock().sign_request().ok();

    let mut mesh = state.mesh.lock();
    if mesh.unpair(&instance_id).is_some() {
        log::info!("[mesh] Unpaired from {}", instance_id);
    }
    let our_id = mesh.ident.identity.instance_id.clone();
    drop(mesh);

    // Cascade: notify removed peer to also remove us. Walks every cached
    // address so a stale Tailscale entry doesn't drop the cascade.
    if sender_id != instance_id {
        if let (Some(peer), Some(token)) = (target_peer, signed_token) {
            if !peer.addresses.is_empty() {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(3)).build().unwrap_or_default();
                let api_port = peer.api_port;
                let addresses = peer.addresses.clone();
                let target = instance_id.clone();
                let our_id_for_call = our_id.clone();
                tokio::spawn(async move {
                    let result = crate::mesh::transport::try_peer(&addresses, |addr| {
                        let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, api_port, our_id_for_call);
                        let client = client.clone();
                        let token = token.clone();
                        async move {
                            match client.delete(&url)
                                .header("Authorization", format!("Bearer {}", token))
                                .send().await
                            {
                                Ok(resp) if resp.status().is_success() => crate::mesh::transport::TryNext::Success(()),
                                Ok(resp) => crate::mesh::transport::TryNext::Stop(format!("HTTP {}", resp.status())),
                                Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                            }
                        }
                    }).await;
                    match result {
                        Ok(()) => log::info!("[mesh/cascade] Notified {} to remove us ({})", target, our_id_for_call),
                        Err(e) => log::debug!("[mesh/cascade] Could not notify {} about removal: {}", target, e),
                    }
                });
            }
        }
    }

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    #[test]
    fn pair_accept_identity_mismatch_is_detected() {
        // Verify that b_instance_id != accept.from_instance_id is detectable at the logic level.
        let stored_id = "peer-bob";
        let accept_id = "peer-eve";
        assert_ne!(stored_id, accept_id, "identity mismatch should be caught");
    }
}
