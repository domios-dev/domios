//! Background tasks: auto-save, mesh health, update check, agent idle poll.

use tauri::{Emitter, Manager};

use crate::multiplexer::commands::MuxState;
use crate::session::SessionState;
use crate::{mesh, update};

use super::states::AppStates;

/// Start background threads: auto-save, update check, agent idle monitoring.
pub(crate) fn start_background_tasks(app_handle: &tauri::AppHandle, states: &AppStates) {
    // Auto-save session every 30 seconds
    {
        let app_handle = app_handle.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            if let (Some(mux), Some(session)) = (
                app_handle.try_state::<MuxState>(),
                app_handle.try_state::<SessionState>(),
            ) {
                let snapshot = mux.lock().to_session_snapshot();
                if let Err(e) = session.lock().save(&snapshot) {
                    log::warn!("Auto-save failed: {}", e);
                }
            }
        });
    }

    // Mesh: periodic health check + stale peer cleanup. Loop is ALWAYS
    // spawned; the body re-reads `mesh.enabled` on every tick so a CLI
    // toggle to offline takes effect without restart (and dropping the
    // process back to online resumes work). See `codeterm mesh offline`.
    {
        let mesh = states.mesh.clone();
        let cfg = states.config.clone();
        let app_h = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create mesh health runtime");
            rt.block_on(async {
                let mut last_sync_secs: std::collections::HashMap<String, u64> =
                    std::collections::HashMap::new();
                // instance_id → last auto-pair attempt timestamp (secs). Retry
                // after AUTO_PAIR_RETRY_SECS so an offline vouched peer that
                // comes back online still gets paired without restart.
                let mut auto_pair_attempted: std::collections::HashMap<String, u64> =
                    std::collections::HashMap::new();
                const AUTO_PAIR_RETRY_SECS: u64 = 60;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    if !cfg.lock().mesh.enabled {
                        // Offline / invisible: don't poll peers, don't cleanup
                        // — peers will time us out on their side via their own
                        // ping scheduler. The Tailscale daemon is left alone.
                        continue;
                    }
                    mesh::discovery::cleanup_stale_peers(&mesh);
                    mesh.lock().cleanup_expired_requests();
                    // Ping all paired peers
                    let (peers, sync_payload, signed_token) = {
                        let m = mesh.lock();
                        let peers: Vec<_> = m.paired_peers.values().cloned().collect();
                        let token = m.sign_request().ok();
                        let payload = m.build_sync_payload();
                        (peers, payload, token)
                    };
                    let mut changed = false;
                    for peer in &peers {
                        if peer.addresses.is_empty() {
                            continue;
                        }
                        // Multi-address ping: peer is online if ANY of its
                        // cached addresses responds. A single Tailscale
                        // glitch shouldn't flip the peer to offline when
                        // LAN is still alive (and vice versa).
                        let ping_result = mesh::transport::try_peer(&peer.addresses, |addr| {
                            let signed_token = signed_token.clone();
                            let port = peer.api_port;
                            let ip = addr.ip.clone();
                            async move {
                                let client = if let Some(ref tok) = signed_token {
                                    mesh::transport::PeerClient::new_signed(&ip, port, tok)
                                } else {
                                    mesh::transport::PeerClient::new_unauthenticated(&ip, port)
                                };
                                if client.ping().await {
                                    mesh::transport::TryNext::Success(client)
                                } else {
                                    mesh::transport::TryNext::Retry("ping failed".to_string())
                                }
                            }
                        })
                        .await;
                        let (ping_ok, client) = match ping_result {
                            Ok(c) => (true, Some(c)),
                            Err(_) => (false, None),
                        };
                        let (online, was_online) = {
                            let mut m = mesh.lock();
                            if let Some(p) = m.paired_peers.get_mut(&peer.instance_id) {
                                let was = p.online;
                                let out = mesh::health::apply_ping_result(
                                    p.online,
                                    p.consecutive_misses,
                                    ping_ok,
                                );
                                p.consecutive_misses = out.misses;
                                if ping_ok {
                                    p.last_seen = Some(chrono::Utc::now().to_rfc3339());
                                }
                                if p.online != out.online {
                                    log::info!(
                                        "[mesh/health] peer {} online: {} -> {} (misses {})",
                                        peer.instance_id,
                                        p.online,
                                        out.online,
                                        out.misses
                                    );
                                    p.online = out.online;
                                    changed = true;
                                }
                                (out.online, was)
                            } else {
                                (false, false)
                            }
                        };
                        let now_secs = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        // Sync on reconnect (offline→online) AND
                        // periodically while online so gossip-discovered
                        // peers' last_seen stays fresh against the 300s
                        // PEER_TTL_SECS eviction in cleanup_stale_peers.
                        let should_sync = mesh::types::should_proactive_sync(
                            online,
                            was_online,
                            now_secs,
                            last_sync_secs.get(&peer.instance_id).copied(),
                            mesh::discovery::PROACTIVE_SYNC_INTERVAL_SECS,
                        );
                        if should_sync {
                            let reason = if !was_online {
                                "reconnected"
                            } else {
                                "periodic refresh"
                            };
                            log::info!("[mesh] Peer {} {} — syncing", peer.instance_name, reason);
                            let Some(client) = client else { continue };
                            if let Ok(their_payload) = client.send_sync(&sync_payload).await {
                                last_sync_secs.insert(peer.instance_id.clone(), now_secs);
                                let voucher = peer.instance_id.clone();
                                let mut m = mesh.lock();
                                for p in their_payload.peers {
                                    if m.paired_peers.contains_key(&p.instance_id) {
                                        continue;
                                    }
                                    m.upsert_discovered(
                                        mesh::discovered::DiscoverySighting {
                                            instance_id: p.instance_id,
                                            instance_name: p.instance_name,
                                            addresses: p.addresses,
                                            api_port: p.api_port,
                                            method: mesh::types::DiscoveryMethod::Gossip,
                                            dev: p.dev,
                                            mesh_id: None,
                                            voucher: Some(voucher.clone()),
                                        },
                                        now_secs,
                                    );
                                }
                                changed = true;
                            }
                        }
                    }
                    // Drop last_sync entries for peers no longer paired so the
                    // map doesn't accumulate stale instance_ids forever.
                    let paired_ids: std::collections::HashSet<String> =
                        peers.iter().map(|p| p.instance_id.clone()).collect();
                    last_sync_secs.retain(|id, _| paired_ids.contains(id));
                    if changed {
                        crate::api::events::emit_everywhere(
                            &app_h,
                            "mesh-state-changed",
                            serde_json::Value::Null,
                        );
                    }

                    // Auto-pair: for any vouched discovered peer not yet paired,
                    // send a pair request. The remote auto-approves if we're
                    // also vouched on their side. Failed attempts retry after
                    // AUTO_PAIR_RETRY_SECS so an offline peer eventually pairs
                    // when it comes back online.
                    let attempt_now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let auto_pair_candidates: Vec<_> = {
                        let m = mesh.lock();
                        m.discovered_peers
                            .values()
                            .filter(|p| {
                                !p.vouched_by.is_empty()
                                    && !m.paired_peers.contains_key(&p.instance_id)
                                    && auto_pair_attempted
                                        .get(&p.instance_id)
                                        .map(|&last| {
                                            attempt_now.saturating_sub(last) >= AUTO_PAIR_RETRY_SECS
                                        })
                                        .unwrap_or(true)
                                    && !m
                                        .pending_requests
                                        .values()
                                        .any(|r| r.from_instance_id == p.instance_id)
                            })
                            .map(|p| (p.instance_id.clone(), p.addresses.clone(), p.api_port))
                            .collect()
                    };
                    for (target_id, addresses, target_port) in auto_pair_candidates {
                        auto_pair_attempted.insert(target_id.clone(), attempt_now);
                        let mesh2 = mesh.clone();
                        let app_h2 = app_h.clone();
                        tokio::spawn(async move {
                            let (request, target_addr) = {
                                let mut m = mesh2.lock();
                                let addr = addresses
                                    .iter()
                                    .find(|a| a.is_tailscale)
                                    .or_else(|| addresses.first())
                                    .map(|a| a.ip.clone());
                                let Some(addr) = addr else { return };
                                let local_addr = m
                                    .tailscale_ip
                                    .clone()
                                    .or_else(|| m.local_ip.clone())
                                    .unwrap_or_else(|| "127.0.0.1".to_string());
                                let Some(pk) = m.ident.identity.public_key.clone() else {
                                    return;
                                };
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                let req = crate::mesh::types::PairingRequest {
                                    request_id: uuid::Uuid::new_v4().to_string(),
                                    from_instance_id: m.ident.identity.instance_id.clone(),
                                    from_instance_name: m.ident.instance_name.clone(),
                                    from_address: local_addr,
                                    from_port: m.api_port,
                                    from_addresses: m.local_announce_addresses(),
                                    public_key: pk,
                                    timestamp: now,
                                    status: crate::mesh::types::PairingStatus::Pending,
                                    introduced_by: None,
                                };
                                // Store so the accept handler can resolve target on callback
                                m.pairing_targets.insert(
                                    req.request_id.clone(),
                                    (addr.clone(), target_port, target_id.clone()),
                                );
                                m.pending_requests
                                    .insert(req.request_id.clone(), req.clone());
                                (req, addr)
                            };
                            let client = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(5))
                                .build()
                                .unwrap_or_default();
                            let url = format!(
                                "http://{}:{}/api/mesh/pair/request",
                                target_addr, target_port
                            );
                            log::info!("[mesh] Auto-pairing vouched peer {} at {}", target_id, url);
                            match client.post(&url).json(&request).send().await {
                                Ok(r) if r.status().is_success() => {
                                    log::info!("[mesh] Auto-pair request sent to {}", target_id);
                                    crate::api::events::emit_everywhere(
                                        &app_h2,
                                        "mesh-state-changed",
                                        serde_json::Value::Null,
                                    );
                                }
                                Ok(r) => {
                                    log::warn!(
                                        "[mesh] Auto-pair {} rejected: {}",
                                        target_id,
                                        r.status()
                                    );
                                    // Drop the stored request so we don't hold a dead entry
                                    let mut m = mesh2.lock();
                                    m.pending_requests.remove(&request.request_id);
                                    m.pairing_targets.remove(&request.request_id);
                                }
                                Err(e) => {
                                    log::warn!("[mesh] Auto-pair {} failed: {}", target_id, e);
                                    let mut m = mesh2.lock();
                                    m.pending_requests.remove(&request.request_id);
                                    m.pairing_targets.remove(&request.request_id);
                                }
                            }
                        });
                    }
                }
            });
        });
    }

    // Periodic + event-driven announce — push our current addresses to every
    // paired peer so cached records stay fresh as networks shuffle.
    //
    // Two layers:
    //   1. Startup burst (~10s after boot) so peers learn our fresh IPs
    //      before any of the periodic timers tick. Without this, a peer
    //      that came online while we were rebooting would have to wait
    //      out the first 90s heartbeat before discovering us.
    //   2. Detection loop on a short cadence (15s) — if Tailscale or LAN
    //      changed since the previous tick, announce immediately
    //      (Tailscale up/down, VPN toggle, LAN-to-WiFi swap). Otherwise a
    //      heartbeat announce fires every ~90s so a peer that joined
    //      while we were quiet catches up without needing a real network
    //      move to dislodge our cache on their side.
    {
        let mesh = states.mesh.clone();
        let cfg = states.config.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create mesh announce runtime");
            rt.block_on(async {
                use std::time::Duration;
                // Startup burst — only fires if mesh is currently enabled.
                // Going offline pre-startup-burst skips it cleanly.
                if cfg.lock().mesh.enabled {
                    let _ = mesh::refresh_local_addresses(&mesh);
                    let report = mesh::broadcast_announce(&mesh, None).await;
                    log::info!(
                        "[mesh/announce.startup] sent={} ok={} stale={} errors={}",
                        report.sent,
                        report.ok,
                        report.stale_peers.len(),
                        report.errors.len()
                    );
                }

                let mut last_heartbeat = std::time::Instant::now();
                loop {
                    let jitter = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .subsec_nanos()
                        % 5) as u64;
                    tokio::time::sleep(Duration::from_secs(15 + jitter)).await;
                    // Re-check on every tick so `codeterm mesh offline`
                    // takes effect without a daemon restart. While offline
                    // we explicitly do NOT call refresh_local_addresses —
                    // that's what pokes Tailscale CLI / interface scan and
                    // can keep the daemon's tun device alive.
                    if !cfg.lock().mesh.enabled {
                        continue;
                    }
                    let changed = mesh::refresh_local_addresses(&mesh);
                    let heartbeat_due = last_heartbeat.elapsed() >= Duration::from_secs(90);
                    if !changed && !heartbeat_due {
                        continue;
                    }
                    let report = mesh::broadcast_announce(&mesh, None).await;
                    last_heartbeat = std::time::Instant::now();
                    let kind = if changed { "addr-change" } else { "heartbeat" };
                    if report.sent > 0 {
                        log::info!(
                            "[mesh/announce.{}] sent={} ok={} stale={} errors={}",
                            kind,
                            report.sent,
                            report.ok,
                            report.stale_peers.len(),
                            report.errors.len()
                        );
                    }
                }
            });
        });
    }

    // Background update check (5s delay to not slow startup)
    {
        let update_st = states.update.clone();
        let app_h = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create update check runtime");
            rt.block_on(async {
                let method = update_st.lock().install_method.clone();
                match update::checker::check_for_update(&method).await {
                    Ok(info) => {
                        if info.update_available {
                            log::info!(
                                "Update available: {} → {}",
                                info.current_version,
                                info.latest_version
                            );
                            let _ = app_h.emit(
                                "update-available",
                                serde_json::to_value(&info).unwrap_or_default(),
                            );
                        }
                        let mut us = update_st.lock();
                        us.last_check = Some(info);
                        us.last_check_time = Some(std::time::Instant::now());
                    }
                    Err(e) => log::warn!("Update check failed: {}", e),
                }
            });
        });
    }

    // Agent idle check loop — transitions Working -> Idle after timeout
    {
        let tracker = states.agent_tracker.clone();
        let mux = states.mux.clone();
        let idle_timeout = {
            let cfg = states.config.lock();
            std::time::Duration::from_secs(cfg.agent.idle_timeout_s as u64)
        };
        let poll_interval = {
            let cfg = states.config.lock();
            std::time::Duration::from_millis(cfg.agent.poll_interval_ms as u64)
        };
        let app_handle = app_handle.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(poll_interval);
                // Collect status changes while holding tracker lock,
                // then release before touching mux (prevents deadlock with process_pty_output)
                let changed: Vec<(String, crate::agent::status::PaneStatus)> = {
                    let mut t = tracker.lock();
                    let pty_ids = t.all_ids();
                    let mut changes = Vec::new();
                    for pty_id in pty_ids {
                        let prev = t.get_status(&pty_id);
                        t.check_idle(&pty_id, idle_timeout);
                        let now = t.get_status(&pty_id);
                        if now != prev {
                            changes.push((pty_id, now));
                        }
                    }
                    changes
                }; // tracker lock released here

                for (pty_id, status) in changed {
                    let pane_id = mux.lock().pane_id_for_pty(&pty_id);
                    if let Some(pid) = pane_id {
                        log::info!(
                            "[idle-poll] {} {:?} (pane={})",
                            &pty_id[..pty_id.len().min(8)],
                            status,
                            pid
                        );
                        crate::api::events::emit_everywhere(
                            &app_handle,
                            "pane-status-changed",
                            serde_json::json!({
                                "pane_id": pid,
                                "status": format!("{:?}", status),
                            }),
                        );

                        // Agent exit detection is handled solely in output.rs (on PTY output).
                        // Removed from idle-poll to avoid race conditions during agent startup.
                    }
                }

                // Agent exit detection removed from idle-poll — handled in output.rs only.
            }
        });
    }
}
