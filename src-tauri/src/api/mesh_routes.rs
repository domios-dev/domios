//! Mesh networking API routes — discovery, pairing, crown, relay, sync.
//! Split from routes.rs for maintainability.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use tauri::Emitter;

use super::routes::{AppState, check_auth};

// ── Debug endpoints ─────────────────────────────────────────────────

/// Debug endpoint — NO AUTH, dev builds only (route not registered in release).
/// ?filter=mesh&limit=100&level=INFO
pub async fn debug_endpoint(
    AxumState(state): AxumState<AppState>,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let filter = query.get("filter").map(|s| s.as_str()).unwrap_or("");
    let limit: usize = query.get("limit").and_then(|s| s.parse().ok()).unwrap_or(100);
    let level_filter = query.get("level").map(|s| s.to_uppercase()).unwrap_or_default();

    let log_path = crate::paths::data_dir().join("logs").join("codeterm.log");
    let lines: Vec<String> = std::fs::read_to_string(&log_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| filter.is_empty() || l.contains(filter))
        .filter(|l| level_filter.is_empty() || l.contains(&level_filter))
        .rev()
        .take(limit)
        .map(|s| s.to_string())
        .collect();

    let mesh = state.mesh.lock();
    let declarations: Vec<serde_json::Value> = mesh.trust_declarations.iter()
        .map(|d| serde_json::json!({
            "peer_id": d.peer_id,
            "trust_level": format!("{:?}", d.trust_level),
            "declared_by": d.declared_by,
            "version": d.version,
        }))
        .collect();
    let paired: Vec<serde_json::Value> = mesh.paired_peers.values()
        .map(|p| serde_json::json!({
            "id": p.instance_id,
            "name": p.instance_name,
            "trust": &p.trust_level,
            "online": p.online,
            "addresses": p.addresses.iter().map(|a| &a.ip).collect::<Vec<_>>(),
            "port": p.api_port,
        }))
        .collect();
    let crown_holder = mesh.crown_public.as_ref().map(|c| c.holder_id.clone());
    let our_id = mesh.identity.instance_id.clone();
    let our_name = mesh.instance_name.clone();
    drop(mesh);

    Json(serde_json::json!({
        "instance": {"id": our_id, "name": our_name},
        "crown_holder": crown_holder,
        "paired": paired,
        "trust_declarations": declarations,
        "logs": lines,
    }))
}

pub async fn mesh_ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({"mesh": true}))
}

/// Dev-only action endpoint — disconnect/connect without auth.
pub async fn debug_action(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let action = body["action"].as_str().unwrap_or("");
    let instance_id = body["instance_id"].as_str().unwrap_or("");
    match action {
        "disconnect" => {
            state.mesh.lock().unpair(instance_id);
            let _ = state.svc.app_handle.emit("mesh-state-changed", ());
            Ok(Json(serde_json::json!({"ok": true, "action": "disconnect", "instance_id": instance_id})))
        }
        "connect" => {
            // Trigger connect via Tauri command proxy
            Err((StatusCode::NOT_IMPLEMENTED, "Use UI for connect".to_string()))
        }
        _ => Err((StatusCode::BAD_REQUEST, format!("Unknown action: {}", action))),
    }
}

pub async fn mesh_debug_sign(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    match mesh.sign_request() {
        Ok(token) => Ok(Json(serde_json::json!({"token": token, "instance_id": mesh.identity.instance_id}))),
        Err(e) => Ok(Json(serde_json::json!({"error": e}))),
    }
}

// ── Status & discovery ──────────────────────────────────────────────

pub async fn mesh_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.status()).unwrap()))
}

pub async fn mesh_peers(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::to_value(mesh.peers_response()).unwrap()))
}

pub async fn mesh_discover(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let mesh = state.mesh.lock();
    let discovered: Vec<_> = mesh.discovered_peers.values().cloned().collect();
    Ok(Json(serde_json::to_value(discovered).unwrap()))
}

// ── Pairing ─────────────────────────────────────────────────────────

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

    {
        let mut mesh = state.mesh.lock();
        let b_name = mesh.discovered_peers.get(&b_instance_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| accept.from_instance_name.clone());
        let peer = crate::mesh::types::PairedPeer {
            instance_id: accept.from_instance_id.clone(),
            instance_name: b_name,
            addresses: vec![crate::mesh::types::PeerAddress { ip: b_address, is_tailscale: false }],
            api_port: b_port,
            public_key: accept.public_key.clone(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: Some(chrono::Utc::now().to_rfc3339()),
            online: true, is_primary: false,
            access_level: crate::mesh::types::AccessLevel::Full,
            trust_level: String::new(), version: None,
        };
        mesh.paired_peers.insert(accept.from_instance_id.clone(), peer);
        mesh.pending_requests.remove(&accept.request_id);
        mesh.pairing_targets.remove(&accept.request_id);
        let _ = mesh.save();
    }

    log::info!("[mesh] Paired (requester) with {}", accept.from_instance_id);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());

    let intro_data = state.mesh.lock().prepare_introductions(&accept.from_instance_id);
    if let Some((new_peer, existing, our_id, token)) = intro_data {
        crate::mesh::broadcast_introductions(new_peer, existing, our_id, token).await;
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
    if let Some(ref pk) = mesh.identity.public_key {
        known_keys.insert(mesh.identity.instance_id.clone(), pk.clone());
    }
    let our_id = mesh.identity.instance_id.clone();
    drop(mesh);

    let sender_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()))?;

    log::info!("[mesh/unpair] sender={} removing {}", sender_id, instance_id);
    if sender_id != our_id && sender_id != instance_id {
        let mesh_guard = state.mesh.lock();
        let is_paired = mesh_guard.paired_peers.contains_key(&sender_id);
        let target_is_crown = mesh_guard.crown_public.as_ref()
            .map(|c| c.holder_id == instance_id).unwrap_or(false);
        drop(mesh_guard);
        if !is_paired {
            return Err((StatusCode::FORBIDDEN,
                format!("Unknown peer {} cannot remove {}", sender_id, instance_id)));
        }
        if target_is_crown {
            return Err((StatusCode::FORBIDDEN,
                "Cannot disconnect the crown holder".to_string()));
        }
        log::info!("[mesh] Peer {} requested removal of {} from our mesh", sender_id, instance_id);
    }

    let target_peer = state.mesh.lock().paired_peers.get(&instance_id).cloned();
    let signed_token = state.mesh.lock().sign_request().ok();

    let mut mesh = state.mesh.lock();
    if mesh.unpair(&instance_id).is_some() {
        log::info!("[mesh] Unpaired from {}", instance_id);
    }
    let our_id = mesh.identity.instance_id.clone();
    drop(mesh);

    // Cascade: notify removed peer to also remove us
    if sender_id != instance_id {
        if let (Some(peer), Some(token)) = (target_peer, signed_token) {
            let addr = peer.addresses.iter()
                .find(|a| a.is_tailscale).or_else(|| peer.addresses.first());
            if let Some(addr) = addr {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(3)).build().unwrap_or_default();
                let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, peer.api_port, our_id);
                tokio::spawn(async move {
                    match client.delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send().await {
                        Ok(resp) if resp.status().is_success() => {
                            log::info!("[mesh/cascade] Notified {} to remove us ({})", instance_id, our_id);
                        }
                        _ => log::debug!("[mesh/cascade] Could not notify {} about removal", instance_id),
                    }
                });
            }
        }
    }

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Crown ───────────────────────────────────────────────────────────

pub async fn mesh_receive_crown(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

    if let (Some(version), Some(from_id)) = (
        body.get("version").and_then(|v| v.as_u64()),
        body.get("from_instance_id").and_then(|v| v.as_str()),
    ) {
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
        let token = auth.strip_prefix("Bearer ").unwrap_or("");
        let mesh = state.mesh.lock();
        let mut kk = mesh.known_public_keys();
        if let Some(ref pk) = mesh.identity.public_key {
            kk.insert(mesh.identity.instance_id.clone(), pk.clone());
        }
        let actual_sender = crate::mesh::crown::verify_signed_request(token, &kk)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Bad signature".to_string()))?;
        if actual_sender != from_id {
            return Err((StatusCode::FORBIDDEN, "Sender doesn't match from_instance_id".to_string()));
        }
        if let Some(ref cp) = mesh.crown_public {
            if cp.holder_id != from_id {
                return Err((StatusCode::FORBIDDEN,
                    format!("Only crown holder can transfer. Current: {}, sender: {}", cp.holder_id, from_id)));
            }
        }
        drop(mesh);

        let public_info = state.mesh.lock().accept_crown(version, from_id)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        let _ = state.svc.app_handle.emit("mesh-state-changed", ());
        return Ok(Json(serde_json::json!({"ok": true, "crown_public": public_info})));
    }

    if let Ok(public_info) = serde_json::from_value::<crate::mesh::crown::CrownPublicInfo>(body) {
        state.mesh.lock().update_crown_public(public_info)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        let _ = state.svc.app_handle.emit("mesh-state-changed", ());
        return Ok(Json(serde_json::json!({"ok": true})));
    }

    Err((StatusCode::BAD_REQUEST, "Invalid crown payload".to_string()))
}

// ── Introduction ────────────────────────────────────────────────────

pub async fn mesh_introduce(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(intro): Json<crate::mesh::types::PeerIntroduction>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.identity.public_key {
        known_keys.insert(mesh.identity.instance_id.clone(), pk.clone());
    }
    let our_id = mesh.identity.instance_id.clone();
    drop(mesh);

    let sender_id = match crate::mesh::crown::verify_signed_request(token, &known_keys) {
        Ok(id) => id,
        Err(e) => {
            log::warn!("[mesh/introduce] Auth failed for introduction of {}: {} (known_keys: {:?})",
                intro.instance_name, e, known_keys.keys().collect::<Vec<_>>());
            return Err((StatusCode::UNAUTHORIZED, format!("Unauthorized: {}", e)));
        }
    };

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

    log::info!("[mesh/introduce] {} introduced by {} — auto-pairing", intro.instance_name, sender_id);

    let mut mesh = state.mesh.lock();
    let peer = crate::mesh::types::PairedPeer {
        instance_id: intro.instance_id.clone(),
        instance_name: intro.instance_name.clone(),
        addresses: intro.addresses.clone(),
        api_port: intro.api_port,
        public_key: intro.public_key.clone(),
        paired_at: chrono::Utc::now().to_rfc3339(),
        last_seen: Some(chrono::Utc::now().to_rfc3339()),
        online: true, is_primary: false,
        access_level: crate::mesh::types::AccessLevel::Full,
        trust_level: String::new(), version: None,
    };
    mesh.paired_peers.insert(intro.instance_id.clone(), peer);
    mesh.discovered_peers.remove(&intro.instance_id);
    let _ = mesh.save();
    drop(mesh);

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    let mesh = state.mesh.lock();
    Ok(Json(serde_json::json!({
        "ok": true,
        "instance_id": mesh.identity.instance_id,
        "instance_name": mesh.instance_name,
        "public_key": mesh.identity.public_key,
        "api_port": mesh.api_port,
        "local_ip": mesh.local_ip,
    })))
}

// ── High-level management (CLI-friendly) ───────────────────────────

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
        let public_key = m.identity.public_key.clone()
            .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "No identity keypair".to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let request = crate::mesh::types::PairingRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            from_instance_id: m.identity.instance_id.clone(),
            from_instance_name: m.instance_name.clone(),
            from_address: local_addr,
            from_port: m.api_port,
            public_key,
            timestamp: now,
            status: crate::mesh::types::PairingStatus::Pending,
        };
        (request, addr.ip.clone(), peer.api_port)
    };

    // Store target info for the accept handler
    {
        let mut m = state.mesh.lock();
        m.pairing_targets.insert(
            request.request_id.clone(),
            (target_addr.clone(), target_port, instance_id.clone()),
        );
        m.pending_requests.insert(request.request_id.clone(), request.clone());
    }

    // Send request to remote peer
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
        let pk = m.identity.public_key.clone()
            .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "No identity keypair".to_string()))?;
        let id = m.identity.instance_id.clone();
        let name = m.instance_name.clone();
        (req, pk, id, name)
    };

    // Send our public key to the requester
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

    // Store requester as paired peer
    {
        let mut m = state.mesh.lock();
        let peer = crate::mesh::types::PairedPeer {
            instance_id: request.from_instance_id.clone(),
            instance_name: request.from_instance_name.clone(),
            addresses: vec![crate::mesh::types::PeerAddress {
                ip: request.from_address.clone(),
                is_tailscale: false,
            }],
            api_port: request.from_port,
            public_key: request.public_key.clone(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: Some(chrono::Utc::now().to_rfc3339()),
            online: true,
            is_primary: false,
            access_level: crate::mesh::types::AccessLevel::Full,
            trust_level: String::new(),
            version: None,
        };
        m.paired_peers.insert(request.from_instance_id.clone(), peer);
        m.pending_requests.remove(&request_id);
        m.claim_crown_if_unclaimed();
        let _ = m.save();
    }

    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    log::info!("[mesh/api] Pairing approved for request {}", request_id);
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

    // Block disconnecting the crown holder
    {
        let m = state.mesh.lock();
        let is_crown = m.crown_public.as_ref()
            .map(|c| c.holder_id == instance_id).unwrap_or(false);
        if is_crown {
            return Err((StatusCode::FORBIDDEN, "Cannot disconnect crown holder — transfer crown first".to_string()));
        }
    }


    let (peer, signed_token, our_id) = {
        let m = state.mesh.lock();
        let peer = m.paired_peers.get(&instance_id).cloned();
        let token = m.sign_request().ok();
        let id = m.identity.instance_id.clone();
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

// ── Sync & Relay ────────────────────────────────────────────────────

pub async fn mesh_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(payload): Json<crate::mesh::types::MeshSyncPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    log::info!("[mesh/sync] Incoming sync: {} peers, crown={}, {} trust declarations",
        payload.peers.len(),
        payload.crown_public.as_ref().map(|c| c.holder_id.as_str()).unwrap_or("none"),
        payload.trust_declarations.len());
    check_auth(&state, &headers).map_err(|(s, e)| {
        log::warn!("[mesh/sync] Auth FAILED: {}", e);
        (s, e.to_string())
    })?;

    let mut mesh = state.mesh.lock();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    for peer_info in &payload.peers {
        if peer_info.instance_id == mesh.identity.instance_id { continue; }
        if mesh.paired_peers.contains_key(&peer_info.instance_id) { continue; }
        mesh.discovered_peers.insert(peer_info.instance_id.clone(), crate::mesh::types::DiscoveredPeer {
            instance_id: peer_info.instance_id.clone(),
            instance_name: peer_info.instance_name.clone(),
            addresses: peer_info.addresses.clone(),
            api_port: peer_info.api_port,
            discovery_method: crate::mesh::types::DiscoveryMethod::Mdns,
            last_seen: now, dev: peer_info.dev, mesh_id: None,
        });
        log::info!("[mesh/gossip] Learned about peer {} ({})", peer_info.instance_name, peer_info.instance_id);
    }

    if let Some(crown_info) = payload.crown_public {
        log::info!("[mesh/sync] Received crown info: holder={}, v{}", crown_info.holder_id, crown_info.version);
        match mesh.update_crown_public(crown_info) {
            Ok(()) => log::info!("[mesh/sync] Crown updated successfully"),
            Err(e) => log::warn!("[mesh/sync] Crown update rejected: {}", e),
        }
    }

    log::info!("[mesh/sync] Received {} trust declarations", payload.trust_declarations.len());
    let crown_pubkey = mesh.crown_public.as_ref().map(|c| c.public_key.clone());
    if let Some(pubkey) = crown_pubkey {
        for decl in &payload.trust_declarations {
            match crate::mesh::crown::verify_trust_declaration(decl, &pubkey) {
                Ok(_) => {
                    let existing_ver = mesh.trust_declarations.iter()
                        .filter(|d| d.peer_id == decl.peer_id).map(|d| d.version).max().unwrap_or(0);
                    if decl.version > existing_ver {
                        mesh.trust_declarations.retain(|d| d.peer_id != decl.peer_id);
                        mesh.trust_declarations.push(decl.clone());
                        log::info!("[mesh/sync] Accepted trust declaration: {} → {:?} v{}", decl.peer_id, decl.trust_level, decl.version);
                    }
                }
                Err(e) => log::warn!("[mesh/sync] Rejected trust declaration for {}: {}", decl.peer_id, e),
            }
        }
        let _ = crate::mesh::crown::save_trust_declarations(&mesh.trust_declarations);
        log::info!("[mesh/sync] Total trust declarations after sync: {}", mesh.trust_declarations.len());
    } else {
        log::warn!("[mesh/sync] No crown_public — cannot verify {} trust declarations, skipping", payload.trust_declarations.len());
    }
    drop(mesh);

    let mesh = state.mesh.lock();
    let our_payload = build_sync_payload(&mesh);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
    Ok(Json(serde_json::to_value(our_payload).unwrap()))
}

fn build_sync_payload(mesh: &crate::mesh::MeshManager) -> crate::mesh::types::MeshSyncPayload {
    crate::mesh::types::MeshSyncPayload {
        peers: mesh.paired_peers.values()
            .map(|p| crate::mesh::types::GossipPeerInfo {
                instance_id: p.instance_id.clone(), instance_name: p.instance_name.clone(),
                addresses: p.addresses.clone(), api_port: p.api_port, dev: false,
            }).collect(),
        crown_public: mesh.crown_public.clone(),
        trust_declarations: mesh.trust_declarations.clone(),
    }
}

/// Verify relay sender: must be paired + trusted (or crown). Untrusted blocked.
pub fn check_mesh_relay_auth(state: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.identity.public_key {
        known_keys.insert(mesh.identity.instance_id.clone(), pk.clone());
    }
    let our_id = mesh.identity.instance_id.clone();
    let peer_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or unknown signature".to_string()))?;
    if peer_id == our_id { return Ok("local".to_string()); }

    // Must be paired
    let is_paired = mesh.paired_peers.contains_key(&peer_id);
    let is_crown = mesh.crown_public.as_ref().map(|c| c.holder_id == peer_id).unwrap_or(false);
    if !is_paired && !is_crown {
        return Err((StatusCode::UNAUTHORIZED, format!("Peer {} not paired", peer_id)));
    }
    if is_crown { return Ok(peer_id); }
    let trust = mesh.get_trust_level(&peer_id);
    match trust {
        crate::mesh::crown::TrustLevel::Trusted => Ok(peer_id),
        _ => Err((StatusCode::FORBIDDEN, format!("Peer {} is {:?}", peer_id, trust))),
    }
}

pub async fn mesh_relay(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(cmd): Json<crate::mesh::types::RelayCommand>,
) -> Result<Json<crate::mesh::types::RelayResponse>, (StatusCode, String)> {
    let sender_id = check_mesh_relay_auth(&state, &headers)?;
    log::info!("[mesh/relay] {} {} from {}", cmd.method, cmd.path, sender_id);

    // Get sender's name for message prefix
    let sender_name = {
        let mesh = state.mesh.lock();
        mesh.paired_peers.get(&sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| sender_id.clone())
    };

    // Prefix text with [Mesh: PEER pane=ID] for send endpoints
    let mut body = cmd.body.clone();
    if cmd.path.contains("/send") && cmd.method.eq_ignore_ascii_case("POST") {
        if let Some(text) = body.get("text").and_then(|v| v.as_str()) {
            // Extract source pane from body or use "unknown"
            let source_pane = body.get("source_pane").and_then(|v| v.as_str()).unwrap_or("unknown");
            body["text"] = serde_json::json!(format!("[Mesh: {} pane={}] {}", sender_name, source_pane, text));
            // Remove source_pane from body before forwarding
            if let Some(obj) = body.as_object_mut() { obj.remove("source_pane"); }
        }
    }

    let local_url = format!("http://127.0.0.1:{}{}", state.api_port, cmd.path);
    let signed_token = state.mesh.lock().sign_request()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Sign failed: {}", e)))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30)).build().unwrap_or_default();
    let req = match cmd.method.to_uppercase().as_str() {
        "GET" => client.get(&local_url),
        "POST" => client.post(&local_url).json(&body),
        "PUT" => client.put(&local_url).json(&body),
        "PATCH" => client.patch(&local_url).json(&body),
        "DELETE" => client.delete(&local_url),
        _ => return Err((StatusCode::BAD_REQUEST, format!("Unsupported method: {}", cmd.method))),
    };

    let resp = req.header("Authorization", format!("Bearer {}", signed_token))
        .send().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Local relay failed: {}", e)))?;

    let status = resp.status().as_u16();
    let data: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!(null));
    Ok(Json(crate::mesh::types::RelayResponse {
        ok: status < 400, status, data: Some(data),
        error: if status >= 400 { Some(format!("HTTP {}", status)) } else { None },
    }))
}
