//! Background tasks: auto-save, mesh health, update check, agent idle poll.

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::session::SessionState;
use crate::{keepawake, mesh, update};

use super::states::AppStates;

fn auto_pair_callback_address(
    addresses: &[crate::mesh::types::PeerAddress],
) -> Option<&crate::mesh::types::PeerAddress> {
    addresses
        .iter()
        .find(|address| address.is_tailscale)
        .or_else(|| addresses.first())
}

#[cfg(test)]
mod tests {
    use super::auto_pair_callback_address;
    use crate::mesh::types::PeerAddress;

    #[test]
    fn mesh_reachability_autopair_prefers_tailscale_callback_address() {
        let addresses = vec![
            PeerAddress::from_ip("192.168.1.2"),
            PeerAddress {
                ip: "100.64.0.1".to_string(),
                is_tailscale: true,
            },
        ];

        assert_eq!(
            auto_pair_callback_address(&addresses).map(|address| address.ip.as_str()),
            Some("100.64.0.1")
        );
    }

    #[test]
    fn mesh_reachability_autopair_falls_back_to_first_callback_address() {
        let addresses = vec![PeerAddress::from_ip("192.168.1.2")];

        assert_eq!(
            auto_pair_callback_address(&addresses).map(|address| address.ip.as_str()),
            Some("192.168.1.2")
        );
    }

    #[test]
    fn mesh_reachability_autopair_skips_request_when_no_callback_address_is_live() {
        let addresses: Vec<PeerAddress> = Vec::new();

        assert!(auto_pair_callback_address(&addresses).is_none());
    }
}

/// Start background threads: auto-save, update check, agent idle monitoring.
pub(crate) fn start_background_tasks(app_handle: &tauri::AppHandle, states: &AppStates) {
    keepawake::start_supervisor(states.keepawake.clone(), app_handle.clone());

    {
        let app_handle = app_handle.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            if let (Some(mux), Some(session)) = (
                app_handle.try_state::<MuxState>(),
                app_handle.try_state::<SessionState>(),
            ) {
                let snapshot = mux.write().to_session_snapshot();
                let saved = session.lock().save(&snapshot);
                if let Err(e) = saved {
                    log::warn!("Auto-save failed: {}", e);
                } else {
                    crate::session::scrollback_sync::sync_scrollback_sidecars(&app_handle);
                }
            }
        });
    }

    // Mesh loop runs continuously; `mesh.enabled` is re-read each tick.
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
                // Track last auto-pair per peer; retry after AUTO_PAIR_RETRY_SECS.
                let mut auto_pair_attempted: std::collections::HashMap<String, u64> =
                    std::collections::HashMap::new();
                const AUTO_PAIR_RETRY_SECS: u64 = 60;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    if !cfg.lock().mesh.enabled {
                        // Offline: skip peer polling/cleanup; peers timeout naturally.
                        continue;
                    }
                    mesh::discovery::cleanup_stale_peers(&mesh);
                    mesh.lock().cleanup_expired_requests();
                    let (peers, sync_payload, signed_token) = {
                        let mut m = mesh.lock();
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
                        // Multi-address ping: online if any cached address responds.
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
                        // Re-sync on reconnect/periodic; keep TTL fresh.
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
                            log::debug!("[mesh] Peer {} {} — syncing", peer.instance_name, reason);
                            let Some(client) = client else { continue };
                            if let Ok(their_payload) = client.send_sync(&sync_payload).await {
                                last_sync_secs.insert(peer.instance_id.clone(), now_secs);
                                let mut m = mesh.lock();
                                m.apply_gossip_snapshot(
                                    &peer.instance_id,
                                    their_payload.generation,
                                    &their_payload.peers,
                                    now_secs,
                                );
                                changed = true;
                            }
                        }
                    }
                    // Drop last_sync entries for unpaired peers.
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

                    // Auto-pair vouched peers; retry every AUTO_PAIR_RETRY_SECS.
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
                                if m.pairing_targets.len() >= crate::mesh::MAX_PAIRING_DELIVERIES {
                                    return;
                                }
                                let addr = addresses
                                    .iter()
                                    .find(|a| a.is_tailscale)
                                    .or_else(|| addresses.first())
                                    .map(|a| a.ip.clone());
                                let Some(addr) = addr else { return };
                                let from_addresses = m.local_announce_addresses();
                                let Some(local_addr) =
                                    auto_pair_callback_address(&from_addresses)
                                else {
                                    return;
                                };
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
                                    from_address: local_addr.ip.clone(),
                                    from_port: m.api_port,
                                    from_addresses,
                                    public_key: pk,
                                    timestamp: now,
                                    status: crate::mesh::types::PairingStatus::Pending,
                                    introduced_by: None,
                                };
                                // Store request for callback target resolution.
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
                            log::info!("[mesh] Auto-pairing vouched peer {}", target_id);
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
                                        "[mesh] Auto-pair {} rejected with HTTP {}",
                                        target_id, r.status()
                                    );
                                    let mut m = mesh2.lock();
                                    m.pending_requests.remove(&request.request_id);
                                    m.pairing_targets.remove(&request.request_id);
                                }
                                Err(e) => {
                                    let evidence =
                                        crate::mesh::transport::TransportError::pairing_send_error(&e);
                                    if evidence.kind()
                                        == crate::mesh::transport::TransportErrorKind::NothingSent
                                    {
                                        log::warn!(
                                            "[mesh] Auto-pair {} failed before send: {}",
                                            target_id, evidence
                                        );
                                        let mut m = mesh2.lock();
                                        m.pending_requests.remove(&request.request_id);
                                        m.pairing_targets.remove(&request.request_id);
                                    } else {
                                        log::warn!(
                                            "[mesh] Auto-pair {} delivery uncertain: {}",
                                            target_id, evidence
                                        );
                                    }
                                }
                            }
                        });
                    }
                }
            });
        });
    }

    // Keep address cache fresh: startup burst, then announce loop.
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
                // Startup burst runs only while mesh is enabled.
                if cfg.lock().mesh.enabled {
                    let _ = mesh::refresh_local_addresses(&mesh).await;
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
                    // Re-check mesh toggle each tick; offline skips refresh.
                    if !cfg.lock().mesh.enabled {
                        continue;
                    }
                    let changed = mesh::refresh_local_addresses(&mesh).await;
                    let heartbeat_due = last_heartbeat.elapsed() >= Duration::from_secs(90);
                    if !changed && !heartbeat_due {
                        continue;
                    }
                    let report = mesh::broadcast_announce(&mesh, None).await;
                    last_heartbeat = std::time::Instant::now();
                    let kind = if changed { "addr-change" } else { "heartbeat" };
                    if report.sent > 0 {
                        // Periodic heartbeat is debug; addr-change or any send error is INFO.
                        if changed || !report.errors.is_empty() {
                            log::info!(
                                "[mesh/announce.{}] sent={} ok={} stale={} errors={}",
                                kind,
                                report.sent,
                                report.ok,
                                report.stale_peers.len(),
                                report.errors.len()
                            );
                        } else {
                            log::debug!(
                                "[mesh/announce.{}] sent={} ok={} stale={} errors={}",
                                kind,
                                report.sent,
                                report.ok,
                                report.stale_peers.len(),
                                report.errors.len()
                            );
                        }
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
                            crate::api::events::emit_everywhere(&app_h, "update-available", &info);
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
                // Hold tracker lock for collect, then unlock before mux updates.
                let changed = {
                    let mut t = tracker.lock();
                    let pty_ids = t.all_ids();
                    let mut changes = Vec::new();
                    for pty_id in pty_ids {
                        let prev = t.get_status(&pty_id);
                        t.check_idle(&pty_id, idle_timeout);
                        let now = t.get_status(&pty_id);
                        if now != prev {
                            changes.push((pty_id, prev, now));
                        }
                    }
                    changes
                }; // tracker lock released here

                for (pty_id, raw_prev, raw_now) in changed {
                    let tab_id = mux.write().tab_id_for_pty(&pty_id);
                    if let Some(pid) = tab_id {
                        log::info!(
                            "[idle-poll] {} {:?} (tab={})",
                            &pty_id[..pty_id.len().min(8)],
                            raw_now,
                            pid
                        );
                        if let Some(service) =
                            app_handle.try_state::<crate::service::ServiceState>()
                        {
                            let prev = service.reconciled_tab_status_for_id(&pid, raw_prev);
                            let now = service.reconciled_tab_status_for_id(&pid, raw_now);
                            service.emit_tab_status_if_changed(&pty_id, prev, now);
                        }

                        // Agent exits are handled in output.rs, not idle poll.
                    }
                }

                // Exit detection moved from idle-poll to output.rs.
            }
        });
    }

    // Runaway test-binary sweeper — kills orphan */target/*/deps/* processes.
    {
        let cfg = states.config.clone();
        std::thread::spawn(move || {
            // Initial delay: give the daemon a moment to settle before first sweep.
            std::thread::sleep(std::time::Duration::from_secs(60));
            loop {
                let runaway_cfg = cfg.lock().runaway.clone();
                if runaway_cfg.enabled {
                    crate::runaway_sweeper::sweep_once(&runaway_cfg);
                }
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        });
    }
}
