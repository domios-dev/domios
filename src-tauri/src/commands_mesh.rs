// ── Mesh commands ───────────────────────────────────────────────────

#[tauri::command]
pub fn mesh_get_status(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
) -> Result<serde_json::Value, String> {
    let m = mesh.lock();
    serde_json::to_value(m.status()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mesh_list_peers(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
) -> Result<serde_json::Value, String> {
    let mut m = mesh.lock();
    m.cleanup_expired_requests();
    serde_json::to_value(m.peers_response()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mesh_refresh_discovery(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
) -> Result<serde_json::Value, String> {
    let m = mesh.lock();
    let discovered: Vec<_> = m.discovered_peers.values().cloned().collect();
    serde_json::to_value(discovered).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mesh_connect(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    instance_id: String,
) -> Result<serde_json::Value, String> {
    crate::api::mesh_routes::connection::connect_peer(mesh.inner().clone(), instance_id)
        .await
        .map(|result| result.as_json())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn mesh_approve_pairing(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    request_id: String,
) -> Result<(), String> {
    crate::api::mesh_routes::pairing::do_approve(
        mesh.inner().clone(),
        svc.app_handle.clone(),
        request_id,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mesh_reject_pairing(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    request_id: String,
) -> Result<(), String> {
    let mut m = mesh.lock();
    let block_id = if let Some(req) = m.pending_requests.get_mut(&request_id) {
        req.status = crate::mesh::types::PairingStatus::Rejected;
        Some(req.from_instance_id.clone())
    } else {
        None
    };
    if let Some(id) = block_id {
        if !m.revocation_list.blocked.contains(&id) {
            m.revocation_list.blocked.push(id);
        }
        let _ = m.save();
    }
    drop(m);
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(())
}

#[tauri::command]
pub async fn mesh_disconnect(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    instance_id: String,
) -> Result<(), String> {
    // Notify the other side to disconnect too (use signed JWT auth)
    let (peer, signed_token, our_id) = {
        let m = mesh.lock();
        let peer = m.paired_peers.get(&instance_id).cloned();
        let token = match m.sign_request() {
            Ok(t) => Some(t),
            Err(e) => {
                log::error!("[mesh] Failed to sign disconnect: {}", e);
                None
            }
        };
        let id = m.ident.identity.instance_id.clone();
        (peer, token, id)
    };
    if signed_token.is_none() {
        log::warn!("[mesh] No signed token — disconnect notification skipped");
    }
    if let (Some(peer), Some(token)) = (peer, signed_token) {
        let addr = peer
            .addresses
            .iter()
            .find(|a| a.is_tailscale)
            .or_else(|| peer.addresses.first());
        if let Some(addr) = addr {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default();
            let url = format!(
                "http://{}:{}/api/mesh/peers/{}",
                addr.ip, peer.api_port, our_id
            );
            match client
                .delete(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    log::info!("[mesh] Notified {} about disconnect", instance_id);
                }
                Ok(resp) => {
                    let body = resp.text().await.unwrap_or_default();
                    log::warn!(
                        "[mesh] Disconnect notify rejected by {}: {}",
                        instance_id,
                        body
                    );
                }
                Err(e) => log::warn!(
                    "[mesh] Failed to notify {} about disconnect: {}",
                    instance_id,
                    e
                ),
            }
        }
    }
    // Extract remaining peers + token BEFORE unpair (need lock only briefly)
    let (remaining_peers, broadcast_token) = {
        let m = mesh.lock();
        let peers: Vec<_> = m
            .paired_peers
            .values()
            .filter(|p| p.instance_id != instance_id)
            .cloned()
            .collect();
        let token = m.sign_request().ok();
        (peers, token)
    };

    mesh.lock().unpair(&instance_id);

    if let Some(token) = broadcast_token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        for peer in &remaining_peers {
            if let Some(addr) = peer.addresses.first() {
                let url = format!(
                    "http://{}:{}/api/mesh/peers/{}",
                    addr.ip, peer.api_port, instance_id
                );
                match http
                    .delete(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        log::info!(
                            "[mesh] Notified {} to remove {}",
                            peer.instance_name,
                            instance_id
                        );
                    }
                    _ => log::debug!(
                        "[mesh] Failed to notify {} about mesh removal",
                        peer.instance_name
                    ),
                }
            }
        }
    }

    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(())
}

/// Exit the entire mesh — disconnect from all peers.
#[tauri::command]
pub async fn mesh_exit(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<(), String> {
    let (peers_info, token) = {
        let m = mesh.lock();
        let token = m.sign_request().ok();
        let our_id = m.ident.identity.instance_id.clone();
        let peers: Vec<_> = m
            .paired_peers
            .values()
            .map(|p| {
                (
                    p.addresses.first().map(|a| a.ip.clone()),
                    p.api_port,
                    our_id.clone(),
                )
            })
            .collect();
        (peers, token)
    };

    if let Some(token) = &token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        for (ip, port, our_id) in &peers_info {
            if let Some(ip) = ip {
                let url = format!("http://{}:{}/api/mesh/peers/{}", ip, port, our_id);
                let _ = http
                    .delete(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send()
                    .await;
            }
        }
    }

    mesh.lock().exit_mesh();
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    log::info!("[mesh] Exited mesh");
    Ok(())
}

#[tauri::command]
pub fn mesh_add_manual(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    address: String,
    port: u16,
) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let peer = crate::mesh::types::DiscoveredPeer {
        instance_id: format!("manual-{}", uuid::Uuid::new_v4()),
        instance_name: format!("{}:{}", address, port),
        addresses: vec![crate::mesh::types::PeerAddress::from_ip(&address)],
        api_port: port,
        discovery_method: crate::mesh::types::DiscoveryMethod::Manual,
        last_seen: now,
        dev: false,
        mesh_id: None,
        vouched_by: Vec::new(),
    };
    mesh.lock()
        .discovered_peers
        .insert(peer.instance_id.clone(), peer);
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    Ok(())
}

/// Start mesh listeners (called when user enables mesh in settings).
#[tauri::command]
pub fn mesh_start_discovery(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    config: tauri::State<'_, crate::config::ConfigState>,
) -> Result<(), String> {
    let mesh_config = config.lock().mesh.clone();
    if !mesh_config.enabled {
        return Err("Mesh is not enabled".to_string());
    }

    crate::mesh::discovery::detect_tailscale(&mesh);
    crate::mesh::discovery::detect_local_ip(&mesh);
    crate::mesh::discovery::start_listeners(mesh.inner().clone())
}

/// Manual mesh on/off switch (sidebar toggle). Persists `config.mesh.enabled`
/// and drives the offline/resume side-effects. Returns the new enabled state.
#[tauri::command]
pub fn mesh_set_enabled(
    enabled: bool,
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    config: tauri::State<'_, crate::config::ConfigState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<bool, String> {
    Ok(crate::mesh::set_mesh_enabled(
        mesh.inner(),
        config.inner(),
        &svc.app_handle,
        enabled,
    ))
}

/// Start broadcasting for the configured duration.
#[tauri::command]
pub fn mesh_start_broadcast(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    config: tauri::State<'_, crate::config::ConfigState>,
) -> Result<(), String> {
    let mesh_config = config.lock().mesh.clone();
    if !mesh_config.enabled {
        return Err("Mesh is not enabled".to_string());
    }
    crate::mesh::discovery::start_broadcast(
        mesh.inner().clone(),
        mesh_config.broadcast_duration_secs,
    )
}

/// Stop broadcasting immediately.
#[tauri::command]
pub fn mesh_stop_broadcast(mesh: tauri::State<'_, crate::MeshStateHandle>) -> Result<(), String> {
    crate::mesh::discovery::stop_broadcast(&mesh);
    Ok(())
}

/// Active scan: clean up stale discovered peers, send one immediate UDP beacon,
/// and return the current discovered list. Called by the UI Refresh button.
#[tauri::command]
pub fn mesh_scan_peers(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
) -> Result<serde_json::Value, String> {
    crate::mesh::discovery::cleanup_stale_peers(&mesh);
    crate::mesh::discovery::send_immediate_beacon(&mesh);
    let discovered: Vec<_> = mesh.lock().discovered_peers.values().cloned().collect();
    serde_json::to_value(discovered).map_err(|e| e.to_string())
}

/// Set access level for a paired peer.
#[tauri::command]
pub fn mesh_set_access(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    instance_id: String,
    access_level: String,
) -> Result<(), String> {
    let level = match access_level.as_str() {
        "full" => crate::mesh::types::AccessLevel::Full,
        "read_only" => crate::mesh::types::AccessLevel::ReadOnly,
        "agent_isolated" => crate::mesh::types::AccessLevel::AgentIsolated,
        _ => return Err(format!("Invalid access level: {}", access_level)),
    };
    let mut m = mesh.lock();
    let peer = m
        .paired_peers
        .get_mut(&instance_id)
        .ok_or_else(|| "Peer not found".to_string())?;
    peer.access_level = level;
    let _ = m.save();
    drop(m);
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "mesh-state-changed",
        serde_json::Value::Null,
    );
    log::info!(
        "[mesh] Set access level for {} to {}",
        instance_id,
        access_level
    );
    Ok(())
}

/// Fetch a paired peer's services, walking all known addresses
/// (Tailscale → LAN → public) and falling back to the last-known cache when
/// the peer is unreachable. Always returns a `PeerServicesView` envelope —
/// stale data is flagged, errors are surfaced, nothing collapses to a
/// silent empty list (#119/#127).
#[tauri::command]
pub async fn mesh_peer_services_list(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    instance_id: String,
) -> Result<serde_json::Value, String> {
    fetch_peer_services_view(mesh.inner(), &instance_id).await
}

/// Shared implementation behind the Tauri command and the web-mirror HTTP
/// route (`GET /api/mesh/peers/{id}/services`).
pub async fn fetch_peer_services_view(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
) -> Result<serde_json::Value, String> {
    use crate::mesh::peer_services::{build_view, FetchOutcome};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (addresses, port, online, token, cache_fresh) = {
        let m = mesh.lock();
        let peer = m
            .paired_peers
            .get(instance_id)
            .ok_or_else(|| format!("Peer {} not paired", instance_id))?;
        let token = m
            .sign_request()
            .map_err(|e| format!("sign_request: {}", e))?;
        (
            crate::mesh::types::sanitize_addresses(&peer.addresses),
            peer.api_port,
            peer.online,
            token,
            m.peer_services.is_fresh(instance_id, now),
        )
    };

    let outcome = if cache_fresh || !online {
        FetchOutcome::Skipped
    } else {
        fetch_peer_services(&addresses, port, &token).await
    };

    let mut m = mesh.lock();
    if let FetchOutcome::Fetched(ref services) = outcome {
        m.peer_services.refresh(instance_id, services.clone(), now);
    }
    let view = build_view(m.peer_services.get(instance_id), online, outcome, now);
    serde_json::to_value(&view).map_err(|e| e.to_string())
}

/// Per-address timeout for the direct peer /api/services fetch. A peer with many
/// services + mesh-signed-token verification can take ~5s to answer (measured
/// against a 17-service peer); the old 3s cut every such peer off mid-request, so
/// the Mesh tab showed 0 services for healthy peers. Kept above real-world latency
/// but bounded so a dead address still fails fast enough for the next address.
const PEER_SERVICES_FETCH_TIMEOUT_SECS: u64 = 10;

async fn fetch_peer_services(
    addresses: &[crate::mesh::types::PeerAddress],
    port: u16,
    token: &str,
) -> crate::mesh::peer_services::FetchOutcome {
    use crate::mesh::peer_services::FetchOutcome;
    use crate::mesh::transport::TryNext;

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            PEER_SERVICES_FETCH_TIMEOUT_SECS,
        ))
        .build()
    {
        Ok(c) => c,
        Err(e) => return FetchOutcome::Failed(e.to_string()),
    };

    let result = crate::mesh::transport::try_peer(addresses, |addr| {
        let client = client.clone();
        let url = format!("http://{}:{}/api/services", addr.ip, port);
        let token = token.to_string();
        async move {
            let resp = match client
                .get(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return TryNext::Retry(format!("request: {}", e)),
            };
            if !resp.status().is_success() {
                return TryNext::Stop(format!("HTTP {}", resp.status()));
            }
            match resp.json::<Vec<serde_json::Value>>().await {
                Ok(services) => TryNext::Success(services),
                Err(e) => TryNext::Stop(format!("parse: {}", e)),
            }
        }
    })
    .await;

    match result {
        Ok(services) => FetchOutcome::Fetched(services),
        Err(e) => FetchOutcome::Failed(e),
    }
}

#[derive(Clone, Copy)]
enum PeerMethod {
    Get,
    Post,
    Delete,
}

/// Signed-token request to a paired peer's REST API, walking its addresses just
/// like the services fetch. This is the cross-mesh ACTION path: the Mesh tab can
/// delete a service or control a cron ON the peer, instead of every mutation
/// silently hitting the local daemon. Returns the peer's JSON response (or Null
/// for empty bodies like a 204 delete).
async fn peer_authed_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    method: PeerMethod,
    path: &str,
) -> Result<serde_json::Value, String> {
    use crate::mesh::transport::TryNext;

    let (addresses, port, token) = {
        let m = mesh.lock();
        let peer = m
            .paired_peers
            .get(instance_id)
            .ok_or_else(|| format!("Peer {} not paired", instance_id))?;
        let token = m
            .sign_request()
            .map_err(|e| format!("sign_request: {}", e))?;
        (
            crate::mesh::types::sanitize_addresses(&peer.addresses),
            peer.api_port,
            token,
        )
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            PEER_SERVICES_FETCH_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| e.to_string())?;

    crate::mesh::transport::try_peer(&addresses, |addr| {
        let client = client.clone();
        let url = format!("http://{}:{}{}", addr.ip, port, path);
        let token = token.clone();
        async move {
            let req = match method {
                PeerMethod::Get => client.get(&url),
                PeerMethod::Post => client.post(&url),
                PeerMethod::Delete => client.delete(&url),
            };
            let resp = match req
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return TryNext::Retry(format!("request: {}", e)),
            };
            if !resp.status().is_success() {
                return TryNext::Stop(format!("HTTP {}", resp.status()));
            }
            // Action endpoints may return JSON or an empty body (204); tolerate both.
            let text = resp.text().await.unwrap_or_default();
            let val = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            TryNext::Success(val)
        }
    })
    .await
}

#[tauri::command]
pub async fn mesh_peer_service_delete(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    instance_id: String,
    id: String,
) -> Result<serde_json::Value, String> {
    peer_service_delete_request(mesh.inner(), &instance_id, &id).await
}

pub async fn peer_service_delete_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
) -> Result<serde_json::Value, String> {
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Delete,
        &format!("/api/services/{}", id),
    )
    .await
}

#[tauri::command]
pub async fn mesh_peer_cron_action(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    instance_id: String,
    id: String,
    action: String,
) -> Result<serde_json::Value, String> {
    peer_cron_action_request(mesh.inner(), &instance_id, &id, &action).await
}

pub async fn peer_cron_action_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
    action: &str,
) -> Result<serde_json::Value, String> {
    // POST against the peer's API surface.
    if !matches!(action, "enable" | "disable" | "trigger" | "cancel") {
        return Err(format!("unsupported cron action: {}", action));
    }
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Post,
        &format!("/api/crons/{}/{}", id, action),
    )
    .await
}

#[tauri::command]
pub async fn mesh_peer_cron_history(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    instance_id: String,
    id: String,
) -> Result<serde_json::Value, String> {
    peer_cron_history_request(mesh.inner(), &instance_id, &id).await
}

pub async fn peer_cron_history_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
) -> Result<serde_json::Value, String> {
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Get,
        &format!("/api/crons/{}/history", id),
    )
    .await
}
