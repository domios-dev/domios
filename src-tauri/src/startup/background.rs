//! Background tasks: auto-save, mesh health, update check, agent idle poll.

use tauri::{Emitter, Manager};

use crate::multiplexer::commands::MuxState;
use crate::session::SessionState;
use crate::{mesh, update};

use super::states::AppStates;

/// Start background threads: auto-save, update check, agent idle monitoring.
pub(crate) fn start_background_tasks(
    app_handle: &tauri::AppHandle,
    states: &AppStates,
) {
    // Auto-save session every 30 seconds
    {
        let app_handle = app_handle.clone();
        std::thread::spawn(move || {
            loop {
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
            }
        });
    }

    // Mesh: periodic health check + stale peer cleanup
    {
        let mesh = states.mesh.clone();
        let mesh_enabled = states.config.lock().mesh.enabled;
        if mesh_enabled {
            let app_h = app_handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create mesh health runtime");
                rt.block_on(async {
                    // Per-peer last successful sync timestamp (Unix secs).
                    // Used by should_proactive_sync to decide when to refresh
                    // gossip view from a still-online paired peer. Lives across
                    // loop iterations so periodic sync interval is honored.
                    let mut last_sync_secs: std::collections::HashMap<String, u64> =
                        std::collections::HashMap::new();
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                        // Clean up stale discovered peers
                        mesh::discovery::cleanup_stale_peers(&mesh);
                        // Clean up expired pairing requests
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
                            let addr = peer.addresses.iter()
                                .find(|a| a.is_tailscale)
                                .or_else(|| peer.addresses.first());
                            let Some(addr) = addr else { continue };
                            let client = if let Some(ref tok) = signed_token {
                                mesh::transport::PeerClient::new_signed(&addr.ip, peer.api_port, tok)
                            } else {
                                mesh::transport::PeerClient::new_unauthenticated(&addr.ip, peer.api_port)
                            };
                                let online = client.ping().await;
                                let was_online = {
                                    let mut m = mesh.lock();
                                    if let Some(p) = m.paired_peers.get_mut(&peer.instance_id) {
                                        let was = p.online;
                                        if p.online != online {
                                            p.online = online;
                                            changed = true;
                                        }
                                        was
                                    } else { false }
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
                                    let reason = if !was_online { "reconnected" } else { "periodic refresh" };
                                    log::info!("[mesh] Peer {} {} — syncing", peer.instance_name, reason);
                                    if let Ok(their_payload) = client.send_sync(&sync_payload).await {
                                        last_sync_secs.insert(peer.instance_id.clone(), now_secs);
                                        let voucher = peer.instance_id.clone();
                                        let mut m = mesh.lock();
                                        for p in their_payload.peers {
                                            if p.instance_id == m.ident.identity.instance_id { continue; }
                                            if m.paired_peers.contains_key(&p.instance_id) { continue; }
                                            // Sanitize addresses on this 4th ingest path — sync-poll
                                            // response from a paired peer can carry pre-fix poisoned
                                            // entries (the v1.1.9 sanitize-on-ingest covered the
                                            // mesh_sync inbound handler, mesh_introduce, and
                                            // apply_announce, but missed this background-poll path).
                                            // Same bug class as v1.1.9 issue #28 / commit f0ff692.
                                            let clean_addrs = mesh::types::sanitize_addresses(&p.addresses);
                                            // Merge transitive-trust voucher entry for the polled peer.
                                            let mut vouched_by = m.discovered_peers
                                                .get(&p.instance_id)
                                                .map(|dp| dp.vouched_by.clone())
                                                .unwrap_or_default();
                                            if !vouched_by.contains(&voucher) {
                                                vouched_by.push(voucher.clone());
                                            }
                                            vouched_by.sort();
                                            vouched_by.dedup();
                                            m.discovered_peers.insert(p.instance_id.clone(), mesh::types::DiscoveredPeer {
                                                instance_id: p.instance_id,
                                                instance_name: p.instance_name,
                                                addresses: clean_addrs,
                                                api_port: p.api_port,
                                                discovery_method: mesh::types::DiscoveryMethod::Gossip,
                                                last_seen: now_secs,
                                                dev: p.dev, mesh_id: None,
                                                vouched_by,
                                            });
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
                            let _ = app_h.emit("mesh-state-changed", ());
                        }
                    }
                });
            });
        }
    }

    // Periodic announce — push our current addresses to every paired peer
    // so cached records stay fresh as networks shuffle (Tailscale lease,
    // VPN toggle, LAN-to-WiFi swap, etc.). Loop fires every ~90s with
    // jitter so peers don't synchronize their announce clocks.
    {
        let mesh = states.mesh.clone();
        let mesh_enabled = states.config.lock().mesh.enabled;
        if mesh_enabled {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(20));
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create mesh announce runtime");
                rt.block_on(async {
                    use std::time::Duration;
                    loop {
                        // 90s base + 0..30s random jitter — keeps peers from
                        // all hitting the network simultaneously.
                        let jitter = (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .subsec_nanos() % 30) as u64;
                        tokio::time::sleep(Duration::from_secs(90 + jitter)).await;
                        // Refresh local addrs first so the announce reflects the
                        // current network state, not a snapshot from boot.
                        mesh::refresh_local_addresses(&mesh);
                        let report = mesh::broadcast_announce(&mesh, None).await;
                        if report.sent > 0 {
                            log::debug!(
                                "[mesh/announce.periodic] sent={} ok={} stale={} errors={}",
                                report.sent, report.ok,
                                report.stale_peers.len(), report.errors.len()
                            );
                        }
                    }
                });
            });
        }
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
                        log::info!("[idle-poll] {} {:?} (pane={})", &pty_id[..pty_id.len().min(8)], status, pid);
                        let _ = app_handle.emit("pane-status-changed", serde_json::json!({
                            "pane_id": pid,
                            "status": format!("{:?}", status),
                        }));

                        // Agent exit detection is handled solely in output.rs (on PTY output).
                        // Removed from idle-poll to avoid race conditions during agent startup.
                    }
                }

                // Agent exit detection removed from idle-poll — handled in output.rs only.
            }
        });
    }
}
