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

pub async fn mesh_debug_logs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let log_path = crate::paths::data_dir().join("logs").join("codeterm.log");
    let lines: Vec<String> = std::fs::read_to_string(&log_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("mesh"))
        .rev()
        .take(50)
        .map(|s| s.to_string())
        .collect();
    Ok(Json(serde_json::json!({"lines": lines})))
}

pub async fn mesh_ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({"mesh": true}))
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

    log::info!("[mesh] Pairing complete (requester) with {} — pubkey exchanged", accept.from_instance_id);
    let _ = state.svc.app_handle.emit("mesh-state-changed", ());
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

    // Local user (own ID) or the peer disconnecting itself → always allowed.
    // Another paired peer requesting removal → allowed (mesh-wide disconnect broadcast).
    // Unknown sender → blocked by auth above.
    if sender_id != our_id && sender_id != instance_id {
        // Verify sender is at least a known paired peer
        let is_paired = state.mesh.lock().paired_peers.contains_key(&sender_id);
        if !is_paired {
            return Err((StatusCode::FORBIDDEN,
                format!("Unknown peer {} cannot remove {}", sender_id, instance_id)));
        }
        log::info!("[mesh] Peer {} requested removal of {} from our mesh", sender_id, instance_id);
    }

    let mut mesh = state.mesh.lock();
    if mesh.unpair(&instance_id).is_some() {
        log::info!("[mesh] Unpaired from {}", instance_id);
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

    let sender_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()))?;

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

// ── Sync & Relay ────────────────────────────────────────────────────

pub async fn mesh_sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(payload): Json<crate::mesh::types::MeshSyncPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;

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

    let crown_pubkey = mesh.crown_public.as_ref().map(|c| c.public_key.clone());
    if let Some(pubkey) = crown_pubkey {
        for decl in &payload.trust_declarations {
            if crate::mesh::crown::verify_trust_declaration(decl, &pubkey).is_ok() {
                let existing_ver = mesh.trust_declarations.iter()
                    .filter(|d| d.peer_id == decl.peer_id).map(|d| d.version).max().unwrap_or(0);
                if decl.version > existing_ver {
                    mesh.trust_declarations.retain(|d| d.peer_id != decl.peer_id);
                    mesh.trust_declarations.push(decl.clone());
                }
            }
        }
        let _ = crate::mesh::crown::save_trust_declarations(&mesh.trust_declarations);
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

pub fn check_mesh_trust(state: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    let mesh = state.mesh.lock();
    let mut known_keys = mesh.known_public_keys();
    if let Some(ref pk) = mesh.identity.public_key {
        known_keys.insert(mesh.identity.instance_id.clone(), pk.clone());
    }
    let our_id = mesh.identity.instance_id.clone();
    let peer_id = crate::mesh::crown::verify_signed_request(token, &known_keys)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid token".to_string()))?;
    if peer_id == our_id { return Ok("local".to_string()); }
    let trust = mesh.get_trust_level(&peer_id);
    match trust {
        crate::mesh::crown::TrustLevel::Trusted => Ok(peer_id),
        _ => {
            if mesh.crown_public.as_ref().map(|c| c.holder_id == peer_id).unwrap_or(false) {
                Ok(peer_id)
            } else {
                Err((StatusCode::FORBIDDEN, format!("Peer {} is {:?} — relay denied", peer_id, trust)))
            }
        }
    }
}

pub async fn mesh_relay(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(cmd): Json<crate::mesh::types::RelayCommand>,
) -> Result<Json<crate::mesh::types::RelayResponse>, (StatusCode, String)> {
    let sender_id = check_mesh_trust(&state, &headers)?;
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
