//! High-level peer management (CLI-friendly): connect / approve / reject / disconnect.

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use tauri::Emitter;

use crate::api::routes::{check_auth, AppState};

/// Initiate pairing with a discovered peer (same logic as Tauri mesh_connect).
pub async fn mesh_connect_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let instance_id = body["instance_id"].as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    let (request, target_addr, target_port) = {
        let m = state.mesh.lock();
        let peer = m.discovered_peers.get(&instance_id)
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Peer not found in discovered list".to_string()))?;
        let addr = peer.addresses.iter()
            .find(|a| a.is_tailscale)
            .or_else(|| peer.addresses.first())
            .ok_or_else(|| (StatusCode::BAD_REQUEST, "No address for peer".to_string()))?;

        let local_addr = m.tailscale_ip.clone()
            .or_else(|| m.local_ip.clone())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let public_key = m.ident.identity.public_key.clone()
            .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "No identity keypair".to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let request = crate::mesh::types::PairingRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            from_instance_id: m.ident.identity.instance_id.clone(),
            from_instance_name: m.ident.instance_name.clone(),
            from_address: local_addr,
            from_port: m.api_port,
            public_key,
            timestamp: now,
            status: crate::mesh::types::PairingStatus::Pending,
        };
        (request, addr.ip.clone(), peer.api_port)
    };

    {
        let mut m = state.mesh.lock();
        m.pairing_targets.insert(
            request.request_id.clone(),
            (target_addr.clone(), target_port, instance_id.clone()),
        );
        m.pending_requests.insert(request.request_id.clone(), request.clone());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
    let url = format!("http://{}:{}/api/mesh/pair/request", target_addr, target_port);
    let resp = client.post(&url).json(&request).send().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Network error: {}", e)))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err((StatusCode::BAD_GATEWAY, format!("Pair request rejected: {}", body)));
    }

    log::info!("[mesh/api] Pairing request sent to {}", instance_id);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true, "request_id": request.request_id})))
}

/// Approve an incoming pairing request (same logic as Tauri mesh_approve_pairing).
pub async fn mesh_approve_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let request_id = body["request_id"].as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing request_id".to_string()))?
        .to_string();

    let (request, our_pubkey, our_id, our_name) = {
        let m = state.mesh.lock();
        let req = m.pending_requests.get(&request_id).cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Request not found".to_string()))?;
        let pk = m.ident.identity.public_key.clone()
            .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "No identity keypair".to_string()))?;
        let id = m.ident.identity.instance_id.clone();
        let name = m.ident.instance_name.clone();
        (req, pk, id, name)
    };

    let accept = crate::mesh::types::PairingAccept {
        request_id: request_id.clone(),
        from_instance_id: our_id.clone(),
        from_instance_name: our_name,
        public_key: our_pubkey,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
    let url = format!("http://{}:{}/api/mesh/pair/accept", request.from_address, request.from_port);
    let resp = client.post(&url).json(&accept).send().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Network error: {}", e)))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err((StatusCode::BAD_GATEWAY, format!("Accept rejected: {}", body)));
    }

    {
        let mut m = state.mesh.lock();
        let peer = crate::mesh::types::PairedPeer {
            instance_id: request.from_instance_id.clone(),
            instance_name: request.from_instance_name.clone(),
            addresses: vec![crate::mesh::types::PeerAddress::from_ip(&request.from_address)],
            api_port: request.from_port,
            public_key: request.public_key.clone(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: Some(chrono::Utc::now().to_rfc3339()),
            online: true,
            is_primary: false,
            access_level: crate::mesh::types::AccessLevel::Full,
            version: None,
        };
        m.paired_peers.insert(request.from_instance_id.clone(), peer);
        m.pending_requests.remove(&request_id);
        let _ = m.save();
    }

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    log::info!("[mesh/api] Pairing approved for request {}", request_id);

    let mesh_state = state.mesh.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        crate::mesh::broadcast_sync(&mesh_state).await;
    });

    Ok(Json(serde_json::json!({"ok": true})))
}

/// Reject and block an incoming pairing request (same logic as Tauri mesh_reject_pairing).
pub async fn mesh_reject_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
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

/// Disconnect (unpair) from a paired peer (same logic as Tauri mesh_disconnect).
pub async fn mesh_disconnect_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let instance_id = body["instance_id"].as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    let (peer, signed_token, our_id) = {
        let m = state.mesh.lock();
        let peer = m.paired_peers.get(&instance_id).cloned();
        let token = m.sign_request().ok();
        let id = m.ident.identity.instance_id.clone();
        (peer, token, id)
    };

    if let (Some(peer), Some(token)) = (peer, signed_token.clone()) {
        let addr = peer.addresses.iter()
            .find(|a| a.is_tailscale).or_else(|| peer.addresses.first());
        if let Some(addr) = addr {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
            let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, peer.api_port, our_id);
            match client.delete(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send().await {
                Ok(resp) if resp.status().is_success() => {
                    log::info!("[mesh/api] Notified {} about disconnect", instance_id);
                }
                Ok(resp) => {
                    let body = resp.text().await.unwrap_or_default();
                    log::warn!("[mesh/api] Disconnect notify rejected by {}: {}", instance_id, body);
                }
                Err(e) => log::warn!("[mesh/api] Failed to notify {} about disconnect: {}", instance_id, e),
            }
        }
    }

    // Cascade to remaining peers
    let (remaining_peers, broadcast_token) = {
        let m = state.mesh.lock();
        let peers: Vec<_> = m.paired_peers.values()
            .filter(|p| p.instance_id != instance_id)
            .cloned().collect();
        let token = m.sign_request().ok();
        (peers, token)
    };

    state.mesh.lock().unpair(&instance_id);

    if let Some(token) = broadcast_token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
        for peer in &remaining_peers {
            if let Some(addr) = peer.addresses.first() {
                let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, peer.api_port, instance_id);
                let _ = http.delete(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await;
            }
        }
    }

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    log::info!("[mesh/api] Disconnected from {}", instance_id);
    Ok(Json(serde_json::json!({"ok": true})))
}
