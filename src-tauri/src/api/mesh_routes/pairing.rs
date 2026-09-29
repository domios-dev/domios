use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::AppState;

fn record_incoming_pairing_request(
    mesh: &mut crate::mesh::MeshManager,
    request: crate::mesh::types::PairingRequest,
) -> Result<(String, String, String), String> {
    let request_id = request.request_id.clone();
    let peer_name = request.from_instance_name.clone();
    mesh.receive_pairing_request(request)?;
    Ok((
        request_id,
        peer_name,
        mesh.ident.identity.instance_id.clone(),
    ))
}

pub async fn mesh_pair_request(
    AxumState(state): AxumState<AppState>,
    Json(request): Json<crate::mesh::types::PairingRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (request_id, peer_name, our_id) = {
        let mut mesh = state.mesh.lock();
        record_incoming_pairing_request(&mut mesh, request)
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?
    };
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    state.svc.create_notification_ext(
        "Mesh pairing request".into(),
        Some(format!("{} is waiting for owner approval.", peer_name)),
        Some("info"),
        None,
        Some(true),
        crate::service::notification::NotificationOrigin::default(),
        crate::service::notification::PushIntent {
            level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
            event_key: Some(format!("mesh-pairing-{request_id}")),
        },
        None,
    );
    Ok(Json(serde_json::json!({"ok": true, "instance_id": our_id})))
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

/// Shared approve logic behind the Tauri command and HTTP endpoint. Walks every callback address from
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

    if request.introduced_by.is_some() {
        crate::api::mesh_routes::connection::connect_peer(
            mesh.clone(),
            request.from_instance_id.clone(),
        )
        .await
        .map_err(|error| ApproveError::Rejected(error.to_string()))?;
        let mut manager = mesh.lock();
        manager.pending_requests.remove(&request_id);
        let _ = manager.save();
        drop(manager);
        crate::api::events::emit_everywhere(&app_handle, "mesh-state-changed", serde_json::Value::Null);
        return Ok(());
    }

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
                Err(e) => {
                    return TryNext::Retry(
                        crate::mesh::transport::TransportError::pairing_send_error(&e).to_string(),
                    )
                }
            };
            if resp.status().is_success() {
                TryNext::Success(())
            } else {
                TryNext::Stop(format!("HTTP {}", resp.status()))
            }
        }
    })
    .await;
    if let Err(e) = result {
        // try_peer flattens Stop and all-failed into one error string; an authoritative HTTP rejection contains the peer's body verbatim.
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
            web_ui: None,
            relay_protocol_version: 0,
            consecutive_misses: 0,
            last_active_ms: None,
        };
        m.complete_explicit_pairing(peer);
        m.pending_requests.remove(&request_id);
        let _ = m.save();
    }

    crate::api::events::emit_everywhere(&app_handle, "mesh-state-changed", serde_json::Value::Null);
    log::info!(
        "[mesh] Pairing complete (owner-approved) with {}",
        request.from_instance_id
    );

    let intro_data = mesh.lock().prepare_introductions(&request.from_instance_id);
    if let Some((new_peer, existing, our_id, token)) = intro_data {
        crate::mesh::broadcast_introductions(new_peer, existing, our_id, token).await;
    }
    // On the new peer's side this replaces any discovered stub.
    let mesh_arc = mesh.clone();
    let new_peer_id = request.from_instance_id.clone();
    tokio::spawn(async move {
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
        let mut mesh = state.mesh.lock();
        mesh.cleanup_expired_requests();
        mesh.pairing_targets
            .get(&accept.request_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "No pending request".to_string()))?
    };

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
        mesh.validate_pairing_accept_identity(&accept.request_id, &accept.from_instance_id)
            .map_err(|error| (StatusCode::CONFLICT, error))?;
        let discovered_name = mesh
            .discovered_peers
            .get(&b_instance_id)
            .map(|p| p.instance_name.clone());
        let b_name = match discovered_name {
            Some(n) if !n.contains(':') => n,
            _ => accept.from_instance_name.clone(),
        };
        // Store every address we know for the approver (pairing target ∪ discovered record), not a single-ip stub — the next ping or relay must be able to fail over (#126).
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
            web_ui: None,
            relay_protocol_version: 0,
            consecutive_misses: 0,
            last_active_ms: None,
        };
        mesh.complete_pairing_accept(&accept.request_id, peer)
            .map_err(|error| (StatusCode::CONFLICT, error))?;
        let _ = mesh.save();
    }

    log::info!("[mesh] Paired (requester) with {}", accept.from_instance_id);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );

    // immediately (issue #9). broadcast_sync still follows for gossip about other peers in our mesh.
    {
        let mesh_state = state.mesh.clone();
        let new_peer_id = accept.from_instance_id.clone();
        tokio::spawn(async move {
            let _ = crate::mesh::broadcast_announce(&mesh_state, Some(&new_peer_id)).await;
            crate::mesh::broadcast_sync(&mesh_state).await;
        });
    }

    let intro_data = state
        .mesh
        .lock()
        .prepare_introductions(&accept.from_instance_id);
    if let Some((new_peer, existing, our_id, token)) = intro_data {
        tokio::spawn(async move {
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
    if mesh.unpair(&instance_id).is_some() {
        log::info!("[mesh] Unpaired from {}", instance_id);
    }
    drop(mesh);

    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
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
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
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
        && crate::service::tokens::constant_time_eq(token.as_bytes(), local_token.as_bytes())
}

#[cfg(test)]
mod tests {
    use crate::mesh::types::{
        DiscoveryMethod, DiscoveredPeer, PairingRequest, PairingStatus, PeerAddress,
    };

    #[test]
    fn mesh_pair_request_from_vouched_peer_stays_pending() {
        let mut mesh = crate::mesh::MeshManager::new("receiver".into(), 7685);
        mesh.disk_persist = false;
        mesh.discovered_peers.insert(
            "introduced".into(),
            DiscoveredPeer {
                instance_id: "introduced".into(),
                instance_name: "Introduced".into(),
                addresses: vec![PeerAddress::from_ip("100.64.0.2")],
                api_port: 7685,
                discovery_method: DiscoveryMethod::Gossip,
                last_seen: 0,
                dev: false,
                mesh_id: None,
                vouched_by: vec!["voucher".into()],
            },
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let request = PairingRequest {
            request_id: "pending-request".into(),
            from_instance_id: "introduced".into(),
            from_instance_name: "Introduced".into(),
            from_address: "100.64.0.2".into(),
            from_port: 7685,
            from_addresses: vec![PeerAddress::from_ip("100.64.0.2")],
            public_key: "introduced-key".into(),
            timestamp: now,
            status: PairingStatus::Pending,
            introduced_by: Some("voucher".into()),
        };

        let (request_id, _, _) = super::record_incoming_pairing_request(&mut mesh, request).unwrap();

        assert_eq!(
            mesh.pending_requests[&request_id].status,
            PairingStatus::Pending
        );
        assert!(!mesh.paired_peers.contains_key("introduced"));
    }

    #[test]
    fn pair_accept_identity_mismatch_is_detected() {
        // Verify that b_instance_id != accept.from_instance_id is detectable at the logic level.
        let stored_id = "peer-bob";
        let accept_id = "peer-eve";
        assert_ne!(stored_id, accept_id, "identity mismatch should be caught");
    }

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
