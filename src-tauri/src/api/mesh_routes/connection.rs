//! High-level peer management (CLI-friendly): connect / approve / reject / disconnect.

use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};

/// Initiate pairing with a discovered peer (same logic as Tauri mesh_connect).
pub async fn mesh_connect_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let instance_id = body["instance_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    let (request, target_addr, target_port, instance_id) = {
        let m = state.mesh.lock();
        let peer = crate::mesh::discovered::resolve_discovered(&m.discovered_peers, &instance_id)
            .map_err(|e| {
            let status = match e {
                crate::mesh::discovered::ResolveError::NotFound => StatusCode::NOT_FOUND,
                crate::mesh::discovered::ResolveError::Ambiguous(_) => StatusCode::CONFLICT,
            };
            (status, e.to_string())
        })?;
        let instance_id = peer.instance_id.clone();
        let addr = peer
            .addresses
            .iter()
            .find(|a| a.is_tailscale)
            .or_else(|| peer.addresses.first())
            .ok_or_else(|| (StatusCode::BAD_REQUEST, "No address for peer".to_string()))?;

        let local_addr = m
            .tailscale_ip
            .clone()
            .or_else(|| m.local_ip.clone())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let public_key = m.ident.identity.public_key.clone().ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "No identity keypair".to_string(),
            )
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let request = crate::mesh::types::PairingRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            from_instance_id: m.ident.identity.instance_id.clone(),
            from_instance_name: m.ident.instance_name.clone(),
            from_address: local_addr,
            from_port: m.api_port,
            from_addresses: m.local_announce_addresses(),
            public_key,
            timestamp: now,
            status: crate::mesh::types::PairingStatus::Pending,
            introduced_by: None,
        };
        (request, addr.ip.clone(), peer.api_port, instance_id)
    };

    {
        let mut m = state.mesh.lock();
        m.pairing_targets.insert(
            request.request_id.clone(),
            (target_addr.clone(), target_port, instance_id.clone()),
        );
        m.pending_requests
            .insert(request.request_id.clone(), request.clone());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let url = format!(
        "http://{}:{}/api/mesh/pair/request",
        target_addr, target_port
    );
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Network error: {}", e)))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Pair request rejected: {}", body),
        ));
    }
    let receipt = resp.json::<serde_json::Value>().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Invalid pair response: {}", e),
        )
    })?;
    let remote_instance_id = receipt["instance_id"].as_str().ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            "Pair response omitted the peer identity".to_string(),
        )
    })?;
    state
        .mesh
        .lock()
        .confirm_pairing_target_identity(&request.request_id, remote_instance_id)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    log::info!("[mesh/api] Pairing request sent to {}", remote_instance_id);
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(Json(
        serde_json::json!({"ok": true, "request_id": request.request_id}),
    ))
}

/// Approve an incoming pairing request (same logic as Tauri mesh_approve_pairing).
pub async fn mesh_approve_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let request_id = body["request_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing request_id".to_string()))?
        .to_string();

    use crate::api::mesh_routes::pairing::{do_approve, ApproveError};
    do_approve(
        state.mesh.clone(),
        state.svc.app_handle.clone(),
        request_id.clone(),
    )
    .await
    .map_err(|e| {
        let status = match &e {
            ApproveError::NotFound => StatusCode::NOT_FOUND,
            ApproveError::Expired => StatusCode::GONE,
            ApproveError::NoKeypair => StatusCode::INTERNAL_SERVER_ERROR,
            ApproveError::Network(_) | ApproveError::Rejected(_) => StatusCode::BAD_GATEWAY,
        };
        (status, e.to_string())
    })?;
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

/// Disconnect (unpair) from a paired peer (same logic as Tauri mesh_disconnect).
pub async fn mesh_disconnect_peer(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let arg = body["instance_id"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing instance_id".to_string()))?
        .to_string();

    // keeps the API forgiving without bloating the CLI.
    let (instance_id, peer, signed_token, our_id) = {
        let m = state.mesh.lock();
        let peer = m.paired_peers.get(&arg).cloned().or_else(|| {
            m.paired_peers
                .values()
                .find(|p| p.instance_name == arg)
                .cloned()
        });
        let resolved_id = peer.as_ref().map(|p| p.instance_id.clone()).unwrap_or(arg);
        let token = m.sign_request().ok();
        let id = m.ident.identity.instance_id.clone();
        (resolved_id, peer, token, id)
    };

    if peer.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("No paired peer matches '{}'", instance_id),
        ));
    }

    if let (Some(peer), Some(token)) = (peer, signed_token.clone()) {
        if !peer.addresses.is_empty() {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default();
            let api_port = peer.api_port;
            let our_id_for_call = our_id.clone();
            let result = crate::mesh::transport::try_peer(&peer.addresses, |addr| {
                let url = format!(
                    "http://{}:{}/api/mesh/peers/{}",
                    addr.ip, api_port, our_id_for_call
                );
                let http = http.clone();
                let token = token.clone();
                async move {
                    match http
                        .delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            crate::mesh::transport::TryNext::Success(())
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            crate::mesh::transport::TryNext::Stop(format!(
                                "HTTP {}: {}",
                                status, body
                            ))
                        }
                        Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                    }
                }
            })
            .await;
            match result {
                Ok(()) => log::info!("[mesh/api] Notified {} about disconnect", instance_id),
                Err(e) => log::warn!(
                    "[mesh/api] Failed to notify {} about disconnect: {}",
                    instance_id,
                    e
                ),
            }
        }
    }

    // Cascade to remaining peers — tell each one we just unpaired the target so their mirror of paired_peers stays consistent.
    let (remaining_peers, broadcast_token) = {
        let m = state.mesh.lock();
        let peers: Vec<_> = m
            .paired_peers
            .values()
            .filter(|p| p.instance_id != instance_id)
            .cloned()
            .collect();
        let token = m.sign_request().ok();
        (peers, token)
    };

    state.mesh.lock().unpair(&instance_id);

    if let Some(token) = broadcast_token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        for peer in &remaining_peers {
            if peer.addresses.is_empty() {
                continue;
            }
            let api_port = peer.api_port;
            let target = instance_id.clone();
            let _ = crate::mesh::transport::try_peer(&peer.addresses, |addr| {
                let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, api_port, target);
                let http = http.clone();
                let token = token.clone();
                async move {
                    match http
                        .delete(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            crate::mesh::transport::TryNext::Success(())
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            crate::mesh::transport::TryNext::Stop(format!("HTTP {}", status))
                        }
                        Err(e) => crate::mesh::transport::TryNext::Retry(e.to_string()),
                    }
                }
            })
            .await;
        }
    }

    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    log::info!("[mesh/api] Disconnected from {}", instance_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mesh_approve_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_approve_peer(
        state,
        headers,
        Json(serde_json::json!({"request_id": request_id})),
    )
    .await
}

pub async fn mesh_reject_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_reject_peer(
        state,
        headers,
        Json(serde_json::json!({"request_id": request_id})),
    )
    .await
}

pub async fn mesh_disconnect_peer_by_id(
    state: AxumState<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    mesh_disconnect_peer(
        state,
        headers,
        Json(serde_json::json!({"instance_id": instance_id})),
    )
    .await
}
