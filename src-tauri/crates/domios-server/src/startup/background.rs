//! Background tasks: mesh health, update check, agent idle poll.

use codeterm_service_core::keepawake;
use codeterm_service_core::mesh;

use codeterm_service_core::startup::AppStates;

/// Start the persistence worker and background monitoring threads.
pub(crate) fn start_background_tasks(
    service: &codeterm_service_core::service::ServiceState,
    states: &AppStates,
    bundles: Option<std::sync::Arc<codeterm_frontend_bundle::FrontendBundles>>,
) {
    if let Err(error) = codeterm_service_core::service::session_persist::start_worker(service) {
        log::error!("[session-persist] worker startup failed: {error}");
    }
    keepawake::start_supervisor(states.keepawake.clone(), service.ui.clone());

    // Mesh loop runs continuously; `mesh.enabled` is re-read each tick.
    {
        let mesh = states.mesh.clone();
        let cfg = states.config.clone();
        let ui = service.ui.clone();
        codeterm_util::named_thread::spawn("mesh-health", move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create mesh health runtime");
            rt.block_on(async {
                let mut last_sync_secs: std::collections::HashMap<String, u64> =
                    std::collections::HashMap::new();
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    if !cfg.lock().mesh.enabled {
                        // Offline: skip peer polling/cleanup; peers timeout naturally.
                        continue;
                    }
                    let peers: Vec<_> = mesh
                        .lock()
                        .paired_peers
                        .values()
                        .map(|peer| (peer.instance_id.clone(), peer.online))
                        .collect();
                    let mut changed = false;
                    for (peer_id, was_online) in &peers {
                        let now = mesh.lock().now();
                        if let Some(online) = mesh::health::check_peer(
                            &mesh,
                            peer_id,
                            last_sync_secs.get(peer_id).copied(),
                        )
                        .await
                        {
                            last_sync_secs.insert(peer_id.clone(), now);
                            changed = true;
                            if online && !was_online && mesh.lock().has_unacked_announce(peer_id) {
                                let _ = mesh::broadcast_announce(&mesh, Some(peer_id)).await;
                            }
                        }
                    }
                    let paired_ids: std::collections::HashSet<_> =
                        peers.into_iter().map(|(id, _)| id).collect();
                    last_sync_secs.retain(|id, _| paired_ids.contains(id));
                    if changed {
                        ui.emit("mesh-state-changed", serde_json::Value::Null);
                    }
                }
            });
        });
    }

    // Keep address cache fresh: startup burst, then announce loop.
    {
        let mesh = states.mesh.clone();
        let cfg = states.config.clone();
        codeterm_util::named_thread::spawn("mesh-announce", move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create mesh announce runtime");
            rt.block_on(async {
                use std::time::Duration;
                use super::mesh_announce::Readiness;
                let mut gate_changes = mesh.lock().gate_watch();
                let mut readiness;
                // Startup burst runs only while mesh is enabled.
                let startup_enabled = cfg.lock().mesh.enabled;
                if startup_enabled {
                    let _ = mesh::refresh_local_addresses(&mesh).await;
                    readiness = {
                        let manager = mesh.lock();
                        gate_changes.borrow_and_update();
                        Readiness::snapshot(&manager, true)
                    };
                    let report = mesh::broadcast_announce(&mesh, None).await;
                    log::info!(
                        "[mesh/announce.startup] sent={} ok={} stale={} errors={}",
                        report.sent,
                        report.ok,
                        report.stale_peers.len(),
                        report.errors.len()
                    );
                } else {
                    readiness = Readiness::snapshot(&mesh.lock(), false);
                }

                let mut last_heartbeat = std::time::Instant::now();
                let refresh_tick = tokio::time::sleep(announce_refresh_delay());
                tokio::pin!(refresh_tick);
                loop {
                    let periodic = tokio::select! {
                        changed = gate_changes.changed() => {
                            if changed.is_err() {
                                return;
                            }
                            false
                        }
                        _ = &mut refresh_tick => {
                            refresh_tick.as_mut().reset(tokio::time::Instant::now() + announce_refresh_delay());
                            true
                        }
                    };
                    // Re-check mesh toggle each tick; offline skips refresh.
                    let enabled = cfg.lock().mesh.enabled;
                    if !enabled {
                        readiness.update(Readiness::snapshot(&mesh.lock(), false));
                        continue;
                    }
                    let changed = periodic && mesh::refresh_local_addresses(&mesh).await;
                    let became_ready = {
                        let manager = mesh.lock();
                        gate_changes.borrow_and_update();
                        readiness.update(Readiness::snapshot(&manager, true))
                    };
                    let heartbeat_due = periodic && last_heartbeat.elapsed() >= Duration::from_secs(90);
                    if !changed && !heartbeat_due && !became_ready {
                        continue;
                    }
                    let report = if changed && !became_ready && !heartbeat_due {
                        mesh::broadcast_address_change(&mesh).await
                    } else {
                        mesh::broadcast_announce(&mesh, None).await
                    };
                    last_heartbeat = std::time::Instant::now();
                    let kind = if became_ready {
                        "readiness"
                    } else if changed {
                        "addr-change"
                    } else {
                        "heartbeat"
                    };
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

    {
        let update_st = states.update.clone();
        let update_ui = service.ui.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            codeterm_commands::update::discovery::lifecycle::run(update_ui, update_st, bundles)
                .await;
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
        let service = service.clone();
        codeterm_util::named_thread::spawn("agent-idle-poll", move || {
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
                    codeterm_service_core::service::pty_io::notify_submit_cycle_status_transition(
                        &pty_id, raw_prev, raw_now,
                    );
                    let tab_id = mux.write().tab_id_for_pty(&pty_id);
                    if let Some(pid) = tab_id {
                        log::info!(
                            "[idle-poll] {} {:?} (tab={})",
                            &pty_id[..pty_id.len().min(8)],
                            raw_now,
                            pid
                        );
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
        codeterm_util::named_thread::spawn("runaway-sweeper", move || {
            // Initial delay: give the daemon a moment to settle before first sweep.
            std::thread::sleep(std::time::Duration::from_secs(60));
            loop {
                let runaway_cfg = cfg.lock().runaway.clone();
                if runaway_cfg.enabled {
                    codeterm_service_core::runaway_sweeper::sweep_once(&runaway_cfg);
                }
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        });
    }
}

fn announce_refresh_delay() -> std::time::Duration {
    let jitter = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        % 5) as u64;
    std::time::Duration::from_secs(15 + jitter)
}
