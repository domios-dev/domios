use tauri::Emitter;

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
) -> Result<(), String> {
    let (request, target_addr, target_port) = {
        let m = mesh.lock();
        let peer = m.discovered_peers.get(&instance_id)
            .ok_or_else(|| "Peer not found in discovered list".to_string())?;
        let addr = peer.addresses.iter()
            .find(|a| a.is_tailscale)
            .or_else(|| peer.addresses.first())
            .ok_or_else(|| "No address for peer".to_string())?;

        let local_addr = m.tailscale_ip.clone()
            .or_else(|| m.local_ip.clone())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let public_key = m.ident.identity.public_key.clone()
            .ok_or("No identity keypair")?;

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

    // Store target info for the accept handler
    {
        let mut m = mesh.lock();
        m.pairing_targets.insert(
            request.request_id.clone(),
            (target_addr.clone(), target_port, instance_id.clone()),
        );
        m.pending_requests.insert(request.request_id.clone(), request.clone());
    }

    // Send request
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
    let url = format!("http://{}:{}/api/mesh/pair/request", target_addr, target_port);
    let resp = client.post(&url).json(&request).send().await
        .map_err(|e| format!("Network error: {}", e))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Pair request rejected: {}", body));
    }

    log::info!("[mesh] Pairing request sent to {}", instance_id);
    Ok(())
}

#[tauri::command]
pub async fn mesh_approve_pairing(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    request_id: String,
) -> Result<(), String> {
    {
        let m = mesh.lock();
        let has_crown = m.crown.public_info.is_some();
        if has_crown && !m.can_manage_mesh() {
            return Err("Only crown holder or trusted peers can approve pairing requests".to_string());
        }
    }
    let (request, our_pubkey, our_id, our_name) = {
        let m = mesh.lock();
        let req = m.pending_requests.get(&request_id).cloned()
            .ok_or_else(|| "Request not found".to_string())?;
        let pk = m.ident.identity.public_key.clone().ok_or("No identity keypair")?;
        let id = m.ident.identity.instance_id.clone();
        let name = m.ident.instance_name.clone();
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
        .map_err(|e| format!("Network error: {}", e))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Accept rejected: {}", body));
    }

    // Store requester as paired peer (we have their public_key from the request)
    {
        let mut m = mesh.lock();
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
            trust_level: String::new(),
            version: None,
        };
        m.paired_peers.insert(request.from_instance_id.clone(), peer);
        m.pending_requests.remove(&request_id);
        let _ = m.save();
    }

    let _ = svc.app_handle.emit("mesh-state-changed", ());
    log::info!("[mesh] Pairing complete (approver) with {} — pubkey exchanged", request.from_instance_id);

    // Auto-pair: introduce new peer to all existing peers and vice versa
    let intro_data = mesh.lock().prepare_introductions(&request.from_instance_id);
    if let Some((new_peer, existing, our_id, token)) = intro_data {
        crate::mesh::broadcast_introductions(new_peer, existing, our_id, token).await;
    }

    // Claim crown (acceptor claims) + save
    {
        let mut m = mesh.lock();
        m.claim_crown_if_unclaimed();
        let _ = m.save();
    }

    // Broadcast full sync (crown + trust + peers) to all
    let mesh_arc = mesh.inner().clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        crate::mesh::broadcast_sync(&mesh_arc).await;
    });

    Ok(())
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
    let _ = svc.app_handle.emit("mesh-state-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn mesh_disconnect(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    instance_id: String,
) -> Result<(), String> {
    {
        let m = mesh.lock();
        if m.crown.public_info.as_ref().map(|c| c.holder_id == instance_id).unwrap_or(false) {
            return Err("Cannot disconnect crown holder — transfer crown first".to_string());
        }
    }
    // Notify the other side to disconnect too (use signed JWT auth)
    let (peer, signed_token, our_id) = {
        let m = mesh.lock();
        let peer = m.paired_peers.get(&instance_id).cloned();
        let token = match m.sign_request() {
            Ok(t) => Some(t),
            Err(e) => { log::error!("[mesh] Failed to sign disconnect: {}", e); None }
        };
        let id = m.ident.identity.instance_id.clone();
        (peer, token, id)
    };
    if signed_token.is_none() {
        log::warn!("[mesh] No signed token — disconnect notification skipped");
    }
    if let (Some(peer), Some(token)) = (peer, signed_token) {
        let addr = peer.addresses.iter()
            .find(|a| a.is_tailscale).or_else(|| peer.addresses.first());
        if let Some(addr) = addr {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build().unwrap_or_default();
            let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, peer.api_port, our_id);
            match client.delete(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send().await {
                Ok(resp) if resp.status().is_success() => {
                    log::info!("[mesh] Notified {} about disconnect", instance_id);
                }
                Ok(resp) => {
                    let body = resp.text().await.unwrap_or_default();
                    log::warn!("[mesh] Disconnect notify rejected by {}: {}", instance_id, body);
                }
                Err(e) => log::warn!("[mesh] Failed to notify {} about disconnect: {}", instance_id, e),
            }
        }
    }
    // Extract remaining peers + token BEFORE unpair (need lock only briefly)
    let (remaining_peers, broadcast_token) = {
        let m = mesh.lock();
        let peers: Vec<_> = m.paired_peers.values()
            .filter(|p| p.instance_id != instance_id)
            .cloned().collect();
        let token = m.sign_request().ok();
        (peers, token)
    };

    mesh.lock().unpair(&instance_id);

    // Notify ALL remaining peers to also unpair the disconnected peer
    if let Some(token) = broadcast_token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
        for peer in &remaining_peers {
            if let Some(addr) = peer.addresses.first() {
                let url = format!("http://{}:{}/api/mesh/peers/{}", addr.ip, peer.api_port, instance_id);
                match http.delete(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await {
                    Ok(resp) if resp.status().is_success() => {
                        log::info!("[mesh] Notified {} to remove {}", peer.instance_name, instance_id);
                    }
                    _ => log::debug!("[mesh] Failed to notify {} about mesh removal", peer.instance_name),
                }
            }
        }
    }

    let _ = svc.app_handle.emit("mesh-state-changed", ());
    Ok(())
}

/// Exit the entire mesh — disconnect from all peers.
#[tauri::command]
pub async fn mesh_exit(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
) -> Result<(), String> {
    // Get signed token + peer list before exiting
    let (peers_info, token) = {
        let m = mesh.lock();
        let token = m.sign_request().ok();
        let our_id = m.ident.identity.instance_id.clone();
        let peers: Vec<_> = m.paired_peers.values()
            .map(|p| (p.addresses.first().map(|a| a.ip.clone()), p.api_port, our_id.clone()))
            .collect();
        (peers, token)
    };

    // Notify all peers we're leaving
    if let Some(token) = &token {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();
        for (ip, port, our_id) in &peers_info {
            if let Some(ip) = ip {
                let url = format!("http://{}:{}/api/mesh/peers/{}", ip, port, our_id);
                let _ = http.delete(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await;
            }
        }
    }

    // Clear all local mesh state
    mesh.lock().exit_mesh();
    let _ = svc.app_handle.emit("mesh-state-changed", ());
    log::info!("[mesh] Exited mesh");
    Ok(())
}

#[tauri::command]
pub async fn mesh_transfer_crown(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    instance_id: String,
) -> Result<(), String> {
    // Relinquish crown — get next version number
    let (next_version, our_id, target_client) = {
        let mut m = mesh.lock();
        let v = m.transfer_crown(&instance_id)?;
        let id = m.ident.identity.instance_id.clone();
        let client = m.client_for(&instance_id);
        (v, id, client)
    };

    // Tell target to accept crown — they generate keypair locally
    if let Some(client) = target_client {
        let body = serde_json::json!({"version": next_version, "from_instance_id": our_id});
        let resp = client.auth_post_json("/api/mesh/crown", &body).await
            .map_err(|e| format!("Crown transfer failed: {}", e))?;
        // Response contains new CrownPublicInfo — broadcast to all peers
        if let Some(public_info_json) = resp.get("crown_public") {
            let info_str = serde_json::to_string(public_info_json).unwrap_or_default();
            // Update our own crown_public
            if let Ok(info) = serde_json::from_str::<crate::mesh::crown::CrownPublicInfo>(&info_str) {
                let _ = mesh.lock().update_crown_public(info);
            }
            // Broadcast to other peers
            let other_clients: Vec<_> = {
                let m = mesh.lock();
                m.paired_peers.keys()
                    .filter(|pid| pid.as_str() != instance_id)
                    .filter_map(|pid| m.client_for(pid))
                    .collect()
            };
            for c in other_clients {
                let _ = c.send_crown_update(&info_str).await;
            }
        }
    }

    let _ = svc.app_handle.emit("mesh-state-changed", ());
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
    };
    mesh.lock()
        .discovered_peers
        .insert(peer.instance_id.clone(), peer);
    let _ = svc.app_handle.emit("mesh-state-changed", ());
    Ok(())
}

/// Set trust level for a peer (only crown holder can do this).
#[tauri::command]
pub async fn mesh_set_trust(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    instance_id: String,
    trust_level: String,
) -> Result<(), String> {
    let level = match trust_level.as_str() {
        "trusted" => crate::mesh::crown::TrustLevel::Trusted,
        "untrusted" => crate::mesh::crown::TrustLevel::Untrusted,
        "observer" => crate::mesh::crown::TrustLevel::Observer,
        _ => return Err(format!("Invalid trust level: {}", trust_level)),
    };

    mesh.lock().set_trust_level(&instance_id, level)?;

    // Broadcast full sync payload (crown_public + all trust declarations) to all peers
    let mesh_arc = mesh.inner().clone();
    tokio::spawn(async move {
        crate::mesh::broadcast_sync(&mesh_arc).await;
    });

    let _ = svc.app_handle.emit("mesh-state-changed", ());
    log::info!("[mesh] Set trust for {} to {}", instance_id, trust_level);
    Ok(())
}

/// Start mesh discovery dynamically (called when user enables mesh in settings).
#[tauri::command]
pub fn mesh_start_discovery(
    mesh: tauri::State<'_, crate::MeshStateHandle>,
    config: tauri::State<'_, crate::config::ConfigState>,
) -> Result<(), String> {
    let mesh_config = config.lock().mesh.clone();
    if !mesh_config.enabled {
        return Err("Mesh is not enabled".to_string());
    }

    // Don't claim crown here — crown is only created during pairing (acceptor claims).
    // Standalone machines must not have a crown.

    // Detect network addresses
    crate::mesh::discovery::detect_tailscale(&mesh);
    crate::mesh::discovery::detect_local_ip(&mesh);

    // Start mDNS if auto-discover is on
    if mesh_config.auto_discover {
        if mesh.lock().discovery_active {
            log::info!("[mesh] Discovery already active");
            return Ok(());
        }
        match crate::mesh::discovery::start_mdns(mesh.inner().clone(), mesh_config.stealth) {
            Ok(handle) => {
                log::info!("[mesh] mDNS discovery started dynamically");
                std::mem::forget(handle);
            }
            Err(e) => log::error!("[mesh] mDNS start failed: {}", e),
        }
        // UDP broadcast fallback
        if let Err(e) = crate::mesh::discovery::start_udp_broadcast(mesh.inner().clone()) {
            log::error!("[mesh] UDP broadcast start failed: {}", e);
        }
    }

    Ok(())
}
