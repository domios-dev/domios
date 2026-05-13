//! High-level peer management (CLI-friendly): connect / approve / reject / disconnect.

use axum::{
    extract::{Path, State as AxumState},
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
            introduced_by: None,
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

    // Pair-time peer records on both sides only contain the single observed
    // address from the request packet — that asymmetry persisted in
    // peers.json on both ends until a third broadcast eventually realigned
    // it (issue #9 from 1.1.9 SMOKE). Push our authoritative full address
    // list to the new peer right away so apply_announce on their side
    // replaces the single-address stub with the complete record.
    let mesh_state = state.mesh.clone();
    let new_peer_id = request.from_instance_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = crate::mesh::broadcast_announce(&mesh_state, Some(&new_peer_id)).await;
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
    let arg = body["instance_id"].as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    // Accept either a UUID (paired_peers map key) or an instance_name.
    // CLI users typically copy names from `mesh peers` output; resolving
    // here keeps the API forgiving without bloating the CLI.
    let (instance_id, peer, signed_token, our_id) = {
        let m = state.mesh.lock();
        let peer = m.paired_peers.get(&arg).cloned().or_else(|| {
            m.paired_peers.values()
                .find(|p| p.instance_name == arg)
                .cloned()
        });
        let resolved_id = peer.as_ref().map(|p| p.instance_id.clone()).unwrap_or(arg);
        let token = m.sign_request().ok();
        let id = m.ident.identity.instance_id.clone();
        (resolved_id, peer, token, id)
    };

    if peer.is_none() {
        return Err((StatusCode::NOT_FOUND, format!("No paired peer matches '{}'", instance_id)));
    }

    if let (Some(peer), Some(token)) = (peer, signed_token.clone()) {
        if !peer.addresses.is_empty() {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
            let api_port = peer.api_port;
            let our_id_for_call = our_id.clone();
            let result = crate::mesh::transport::try_peer(&peer.addresses, |addr| {
                let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, api_port, our_id_for_call);
                let http = http.clone();
                let token = token.clone();
                async move {
                    match http.delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send().await
                    {
                        Ok(resp) if resp.status().is_success() => crate::mesh::transport::TryNext::Success(()),
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            crate::mesh::transport::TryNext::Stop(format!("HTTP {}: {}", status, body))
                        }
                        Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                    }
                }
            }).await;
            match result {
                Ok(()) => log::info!("[mesh/api] Notified {} about disconnect", instance_id),
                Err(e) => log::warn!("[mesh/api] Failed to notify {} about disconnect: {}", instance_id, e),
            }
        }
    }

    // Cascade to remaining peers — tell each one we just unpaired the
    // target so their mirror of paired_peers stays consistent.
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
            if peer.addresses.is_empty() { continue; }
            let api_port = peer.api_port;
            let target = instance_id.clone();
            let _ = crate::mesh::transport::try_peer(&peer.addresses, |addr| {
                let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, api_port, target);
                let http = http.clone();
                let token = token.clone();
                async move {
                    match http.delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send().await
                    {
                        Ok(resp) if resp.status().is_success() => crate::mesh::transport::TryNext::Success(()),
                        Ok(resp) => {
                            let status = resp.status();
                            crate::mesh::transport::TryNext::Stop(format!("HTTP {}", status))
                        }
                        Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                    }
                }
            }).await;
        }
    }

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    log::info!("[mesh/api] Disconnected from {}", instance_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_approve_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_approve_peer(state, headers, Json(serde_json::json!({"request_id": request_id}))).await
}

pub async fn mesh_reject_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_reject_peer(state, headers, Json(serde_json::json!({"request_id": request_id}))).await
}

pub async fn mesh_disconnect_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_disconnect_peer(state, headers, Json(serde_json::json!({"instance_id": instance_id}))).await
}
