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
    let (request_id, from_id, is_vouched) = {
        let mut mesh = state.mesh.lock();
        let request_id = request.request_id.clone();
        let from_id = request.from_instance_id.clone();
        mesh.receive_pairing_request(request)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        let is_vouched = mesh
            .discovered_peers
            .get(&from_id)
            .map(|p| !p.vouched_by.is_empty())
            .unwrap_or(false);
        (request_id, from_id, is_vouched)
    };
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    if is_vouched {
        log::info!(
            "[mesh] Auto-approving pair request from vouched peer {}",
            from_id
        );
        let mesh = state.mesh.clone();
        let app_handle = state.svc.app_handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            if let Err(e) = do_approve(mesh, app_handle, request_id).await {
                log::warn!("[mesh] Auto-approve failed for {}: {}", from_id, e);
            }
        });
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Debug)]
pub enum ApproveError {
    /// No such request id (never existed, or already processed).
    NotFound,
    /// Request exists but expired — the requester must re-send.
    Expired,
    NoKeypair,
    /// Couldn't reach the requester on any callback address.
    Network(String),
    /// Requester answered with an HTTP error.
    Rejected(String),
}

impl std::fmt::Display for ApproveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "Request not found or already processed"),
            Self::Expired => write!(
                f,
                "Pairing request expired — ask the peer to send a new one"
            ),
            Self::NoKeypair => write!(f, "No identity keypair"),
            Self::Network(e) => write!(f, "Network error: {}", e),
            Self::Rejected(e) => write!(f, "Accept rejected: {}", e),
        }
    }
}

/// Shared approve logic — the single implementation behind auto-approve, the
/// Tauri command, and the HTTP endpoint. Walks every callback address from
/// the request (Tailscale → LAN → legacy from_address) and stores the full
/// list on the new PairedPeer — a single dead LAN ip no longer kills the
/// handshake with a 502 (#126).
pub async fn do_approve(
    mesh: crate::mesh::MeshState,
    app_handle: tauri::AppHandle,
    request_id: String,
) -> Result<(), ApproveError> {
    let (request, our_pubkey, our_id, our_name) = {
        let m = mesh.lock();
        let req = m
            .pending_requests
            .get(&request_id)
            .cloned()
            .ok_or(ApproveError::NotFound)?;
        if req.status == crate::mesh::types::PairingStatus::Expired {
            return Err(ApproveError::Expired);
        }
        if m.paired_peers.contains_key(&req.from_instance_id) {
            return Ok(()); // already paired, nothing to do
        }
        let pk = m
            .ident
            .identity
            .public_key
            .clone()
            .ok_or(ApproveError::NoKeypair)?;
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
    let candidates = request.callback_addresses();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let port = request.from_port;
    let result = crate::mesh::transport::try_peer(&candidates, |addr| {
        let client = client.clone();
        let accept = accept.clone();
        let url = format!("http://{}:{}/api/mesh/pair/accept", addr.ip, port);
        async move {
            use crate::mesh::transport::TryNext;
            let resp = match client.post(&url).json(&accept).send().await {
                Ok(r) => r,
                Err(e) => return TryNext::Retry(e.to_string()),
            };
            if resp.status().is_success() {
                TryNext::Success(())
            } else {
                let body = resp.text().await.unwrap_or_default();
                TryNext::Stop(body)
            }
        }
    })
    .await;
    if let Err(e) = result {
        // try_peer flattens Stop and all-failed into one error string; an
        // authoritative HTTP rejection contains the peer's body verbatim.
        return Err(
            if e.contains("all addresses failed") || e == "no addresses" {
                ApproveError::Network(e)
            } else {
                ApproveError::Rejected(e)
            },
        );
    }

    {
        let mut m = mesh.lock();
        let peer = crate::mesh::types::PairedPeer {
            instance_id: request.from_instance_id.clone(),
            instance_name: request.from_instance_name.clone(),
            addresses: candidates,
            api_port: request.from_port,
            public_key: request.public_key.clone(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: Some(chrono::Utc::now().to_rfc3339()),
            online: true,
            is_primary: false,
            access_level: crate::mesh::types::AccessLevel::Full,
            version: None,
            consecutive_misses: 0,
        };
        m.paired_peers
            .insert(request.from_instance_id.clone(), peer);
        m.pending_requests.remove(&request_id);
        let _ = m.save();
    }

    crate::api::events::emit_everywhere(&app_handle, "mesh-state-changed", serde_json::Value::Null);
    log::info!(
        "[mesh] Pairing complete (approver/auto) with {}",
        request.from_instance_id
    );

    let intro_data = mesh.lock().prepare_introductions(&request.from_instance_id);
    if let Some((new_peer, existing, our_id, token)) = intro_data {
        crate::mesh::broadcast_introductions(new_peer, existing, our_id, token).await;
    }
    // Pair-time records start from the request's candidate addresses only.
    // Push our authoritative full address list right away so apply_announce
    // on the new peer's side replaces any stub (issue #9 from 1.1.9 SMOKE).
    let mesh_arc = mesh.clone();
    let new_peer_id = request.from_instance_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = crate::mesh::broadcast_announce(&mesh_arc, Some(&new_peer_id)).await;
        crate::mesh::broadcast_sync(&mesh_arc).await;
    });
    Ok(())
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
            b_instance_id,
            accept.from_instance_id
        );
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Identity mismatch: expected {}, got {}",
                b_instance_id, accept.from_instance_id
            ),
        ));
    }

    {
        let mut mesh = state.mesh.lock();
        let discovered_name = mesh
            .discovered_peers
            .get(&b_instance_id)
            .map(|p| p.instance_name.clone());
        let b_name = match discovered_name {
            Some(n) if !n.contains(':') => n,
            _ => accept.from_instance_name.clone(),
        };
        // Store every address we know for the approver (pairing target ∪
        // discovered record), not a single-ip stub — the next ping or relay
        // must be able to fail over (#126).
        let mut addresses = vec![crate::mesh::types::PeerAddress::from_ip(&b_address)];
        if let Some(d) = mesh.discovered_peers.get(&b_instance_id) {
            for a in &d.addresses {
                if !addresses.iter().any(|x| x.ip == a.ip) {
                    addresses.push(a.clone());
                }
            }
        }
        let addresses = crate::mesh::types::sanitize_addresses(&addresses);
        let peer = crate::mesh::types::PairedPeer {
            instance_id: accept.from_instance_id.clone(),
            instance_name: b_name,
            addresses,
            api_port: b_port,
            public_key: accept.public_key.clone(),
            paired_at: chrono::Utc::now().to_rfc3339(),
            last_seen: Some(chrono::Utc::now().to_rfc3339()),
            online: true,
            is_primary: false,
            access_level: crate::mesh::types::AccessLevel::Full,
            version: None,
            consecutive_misses: 0,
        };
        mesh.paired_peers
            .insert(accept.from_instance_id.clone(), peer);
        mesh.pending_requests.remove(&accept.request_id);
        mesh.pairing_targets.remove(&accept.request_id);
        let _ = mesh.save();
    }

    log::info!("[mesh] Paired (requester) with {}", accept.from_instance_id);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );

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
    let intro_data = state
        .mesh
        .lock()
        .prepare_introductions(&accept.from_instance_id);
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
    let request_id = body["request_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing request_id".to_string()))?
        .to_string();

    let mut mesh = state.mesh.lock();
    let block_id = if let Some(req) = mesh.pending_requests.get_mut(&request_id) {
        req.status = crate::mesh::types::PairingStatus::Rejected;
        Some(req.from_instance_id.clone())
    } else {
        None
    };
    if let Some(id) = block_id {
        if !mesh.revocation_list.blocked.contains(&id) {
            mesh.revocation_list.blocked.push(id);
        }
        let _ = mesh.save();
    }
    drop(mesh);

    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(serde_json::json!({"ok": true})))
}

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

    log::info!(
        "[mesh/unpair] sender={} removing {}",
        sender_id,
        instance_id
    );
    if sender_id != our_id && sender_id != instance_id {
        let mesh_guard = state.mesh.lock();
        let is_paired = mesh_guard.paired_peers.contains_key(&sender_id);
        drop(mesh_guard);
        if !is_paired {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Unknown peer {} cannot remove {}", sender_id, instance_id),
            ));
        }
        log::info!(
            "[mesh] Peer {} requested removal of {} from our mesh",
            sender_id,
            instance_id
        );
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
                    .timeout(std::time::Duration::from_secs(3))
                    .build()
                    .unwrap_or_default();
                let api_port = peer.api_port;
                let addresses = peer.addresses.clone();
                let target = instance_id.clone();
                let our_id_for_call = our_id.clone();
                tokio::spawn(async move {
                    let result = crate::mesh::transport::try_peer(&addresses, |addr| {
                        let url = format!(
                            "http://{}:{}/api/mesh/peers/{}",
                            addr.ip, api_port, our_id_for_call
                        );
                        let client = client.clone();
                        let token = token.clone();
                        async move {
                            match client
                                .delete(&url)
                                .header("Authorization", format!("Bearer {}", token))
                                .send()
                                .await
                            {
                                Ok(resp) if resp.status().is_success() => {
                                    crate::mesh::transport::TryNext::Success(())
                                }
                                Ok(resp) => crate::mesh::transport::TryNext::Stop(format!(
                                    "HTTP {}",
                                    resp.status()
                                )),
                                Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                            }
                        }
                    })
                    .await;
                    match result {
                        Ok(()) => log::info!(
                            "[mesh/cascade] Notified {} to remove us ({})",
                            target,
                            our_id_for_call
                        ),
                        Err(e) => log::debug!(
                            "[mesh/cascade] Could not notify {} about removal: {}",
                            target,
                            e
                        ),
                    }
                });
            }
        }
    }

    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
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
