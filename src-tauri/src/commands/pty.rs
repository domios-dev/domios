use std::collections::HashMap;
use std::io::Read;
use tauri::{AppHandle, Emitter};

use crate::multiplexer::commands::MuxState;
use crate::service::pty_io::build_spawn_dispatch;
use crate::service::ServiceState;
use crate::{AgentTrackerState, OutputBufferState, PtyState, ScreenStateHandle};

#[tauri::command]
pub async fn spawn_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    screen_state: tauri::State<'_, ScreenStateHandle>,
    config_state: tauri::State<'_, crate::config::ConfigState>,
    svc: tauri::State<'_, ServiceState>,
    _app: AppHandle,
    command: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    predicted_id: Option<String>,
    pane_id: Option<String>,
) -> Result<String, String> {
    // Protected panes reuse their backend PTY; wait briefly to avoid duplicate launch.
    if let Some(ref pid) = pane_id {
        let (is_protected, existing) = {
            let m = mux_state.read();
            let p = m.get_pane(pid);
            (
                p.map(|p| p.protected).unwrap_or(false),
                p.and_then(|p| p.pty_id.clone()),
            )
        };
        // Reuse a live PTY when possible; remounts should not re-trigger agent resume.
        if let Some(existing) = existing {
            let alive = state.lock().get_session(&existing).is_some();
            if alive {
                log::info!(
                    "[spawn_pty] pane {} already has live pty {} — reusing",
                    pid,
                    existing,
                );
                return Ok(existing);
            }
        }
        if is_protected {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Some(existing) = mux_state
                    .read()
                    .get_pane(pid)
                    .and_then(|p| p.pty_id.clone())
                {
                    let alive = state.lock().get_session(&existing).is_some();
                    if alive {
                        log::info!(
                            "[spawn_pty] protected pane {} already has live backend pty {} — reusing",
                            pid,
                            existing,
                        );
                        return Ok(existing);
                    }
                    log::warn!(
                        "[spawn_pty] protected pane {} has stale backend pty {} — waiting for relink",
                        pid,
                        existing,
                    );
                }
                if std::time::Instant::now() > deadline {
                    log::warn!(
                        "[spawn_pty] protected pane {} still has no backend pty after 5s — falling through",
                        pid,
                    );
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }

    let mut env_vars: HashMap<String, String> = HashMap::new();
    if let Some(ref pid) = pane_id {
        env_vars.insert("CODETERM_PANE_ID".to_string(), pid.clone());
        // Binding nonce so the agent's session_start hook can claim this pane (parity with pane_env).
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".to_string(),
            svc.ensure_session_binding_nonce(pid),
        );
        // Per-session shell-integration nonce — the injected shell script stamps it onto OSC 633;E.
        env_vars.insert(
            "CODETERM_SHELL_NONCE".to_string(),
            svc.ensure_shell_integration_nonce(pid),
        );
        // Pass role/parent info so agents know their place in hierarchy
        let mux = mux_state.read();
        if let Some(pane) = mux.get_pane(pid) {
            if let Some(ref role) = pane.role {
                env_vars.insert("CODETERM_ROLE".to_string(), format!("{:?}", role));
            }
            if let Some(ref parent) = pane.parent_pane_id {
                env_vars.insert("CODETERM_PARENT_PANE_ID".to_string(), parent.clone());
            }
            if let Some(ref wid) = pane.worker_id {
                env_vars.insert("CODETERM_WORKER_ID".to_string(), wid.clone());
            }
        }
        drop(mux);
    }
    env_vars.insert(
        "CODETERM_SOCKET_PATH".to_string(),
        crate::socket::server::socket_display_path(),
    );
    env_vars.insert(
        "CODETERM_VERSION".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    // Pass token/port to REST callbacks to avoid silent 401s.
    let api_cfg = config_state.lock().api.clone();
    if let Ok(port) = std::env::var("CODETERM_API_PORT") {
        env_vars.insert("CODETERM_API_PORT".into(), port);
    } else {
        env_vars.insert("CODETERM_API_PORT".into(), api_cfg.port.to_string());
    }
    if let Some(ref tok) = api_cfg.token {
        if !tok.is_empty() {
            env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
        }
    }

    let t0 = std::time::Instant::now();
    let built = tokio::task::spawn_blocking(move || {
        crate::pty::manager::build_session(command, cwd, cols, rows, env_vars)
    })
    .await
    .map_err(|e| format!("spawn_pty join error: {}", e))??;
    let build_ms = t0.elapsed().as_millis();

    let (pty_id, _session) = state.lock().register_session(predicted_id, built);
    log::info!("PTY {} spawned (build={}ms)", pty_id, build_ms);

    if let Some(ref pid) = pane_id {
        svc.link_pty(pid, pty_id.clone());
    }
    if let Err(e) = crate::service::pty_lifecycle::start_backend_reader(&svc, &pty_id, false) {
        log::warn!(
            "[spawn_pty] failed to start backend reader for {}: {}",
            &pty_id[..pty_id.len().min(8)],
            e
        );
    }

    // Auto-resume pane session after shell is ready.
    if let Some(ref pid) = pane_id {
        let (session_id, is_protected, pane_cwd, restored_from_snapshot, agent_type, pane_model) = {
            let m = mux_state.read();
            let pane = m.get_pane(pid);
            (
                pane.and_then(|p| p.agent_session_id.clone()),
                pane.map(|p| p.protected).unwrap_or(false),
                pane.and_then(|p| p.cwd.clone()),
                pane.map(|p| p.restored_from_snapshot).unwrap_or(false),
                pane.and_then(|p| p.agent_type.clone()),
                pane.and_then(|p| p.agent_session_model.clone()),
            )
        };
        let mut should_fresh_launch =
            restored_from_snapshot && !is_protected && agent_type.is_some();
        let agent_type_for_fresh = agent_type.clone();

        // Resume only restored snapshots; direct spawn uses backend start.
        if let Some(session_id) = session_id
            .filter(|_| !is_protected)
            .filter(|_| restored_from_snapshot)
        {
            let provider_id = agent_type.unwrap_or_else(crate::config::default_agent_provider);

            // Clear stale id if session is gone to avoid resume loop.
            let session_valid = pane_cwd
                .as_deref()
                .map(|cwd| {
                    svc.agent_registry
                        .with_provider(&provider_id, |p| p.session_exists(cwd, &session_id))
                        .unwrap_or(false)
                })
                .unwrap_or(false);

            if !session_valid {
                log::info!(
                    "[resume] session {} for pane {} has no backing file under {:?} — clearing, will try fresh launch",
                    session_id, pid, pane_cwd
                );
                svc.set_agent_session_id(pid, None);
                svc.emit_state_changed();
            } else {
                svc.set_and_lock_pane_agent_type(pid, provider_id.clone());
                svc.emit_state_changed();

                // Build resume command from provider and global fallback config.
                let pc = crate::config::provider_config(&provider_id);
                let _ = config_state; // global lock no longer needed for these fields
                let resume_enabled = pc.resume_sessions.unwrap_or(true);
                // The global "resume with --dangerously-skip-permissions" toggle is the user's explicit intent for restored agents; per-pane auto_approve is unreliable across spawn paths, so do NOT gate on it.
                let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
                let resume_cmd = svc
                    .agent_registry
                    .with_provider(&provider_id, |p| {
                        let base = p.build_resume_command(&session_id, skip_perms)?;
                        // Preserve the pane's model across restart — bare resume otherwise drops it to the provider default.
                        match pane_model.as_deref().filter(|m| !m.trim().is_empty()) {
                            Some(m) => {
                                let margs = p.model_launch_args(m);
                                Some(if margs.is_empty() {
                                    base
                                } else {
                                    format!("{} {}", base, margs.join(" "))
                                })
                            }
                            None => Some(base),
                        }
                    })
                    .flatten();

                if let Some(cmd) = resume_cmd {
                    let pty_clone = state.inner().clone();
                    let pty_id_for_poll = pty_id.clone();
                    let pty_id_for_write = pty_id.clone();
                    let screen_state_clone = screen_state.inner().clone();
                    tokio::spawn(async move {
                        if !resume_enabled {
                            return;
                        }

                        // Wait up to 60s for shell readiness before writing resume command.
                        let ready = tokio::task::spawn_blocking(move || {
                            crate::service::pty_io::poll_shell_prompt(
                                &screen_state_clone,
                                &pty_id_for_poll,
                                std::time::Duration::from_secs(60),
                            )
                        })
                        .await
                        .unwrap_or(false);

                        if !ready {
                            log::warn!(
                                "[resume] shell not ready for session {}, skipping",
                                session_id
                            );
                            return;
                        }

                        log::info!("[resume] sending: {}", cmd);
                        match build_spawn_dispatch(&cmd) {
                            Ok(bytes) => {
                                let _ = pty_clone.lock().write(&pty_id_for_write, &bytes);
                            }
                            Err(e) => {
                                log::error!(
                                    "[resume] spawn dispatch build failed: {} — skipping launch",
                                    e
                                );
                            }
                        }
                    });
                }
                should_fresh_launch = false;
            }
        }

        if should_fresh_launch {
            let provider_id = agent_type_for_fresh.expect("checked above");
            // Restored pane without session falls back to fresh launch.
            svc.clear_pane_restored_flag(pid);
            let pc = crate::config::provider_config(&provider_id);
            let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
            let launch_cmd = svc.agent_registry.with_provider(&provider_id, |p| {
                let args = pane_model
                    .as_deref()
                    .filter(|m| !m.trim().is_empty())
                    .map(|m| p.model_launch_args(m))
                    .filter(|a| !a.is_empty());
                p.build_launch_command(&crate::agent::provider::AgentLaunchParams {
                    skip_permissions: skip_perms,
                    args,
                    ..Default::default()
                })
            });
            if let Some(cmd) = launch_cmd {
                let pty_clone = state.inner().clone();
                let pty_id_for_poll = pty_id.clone();
                let pty_id_for_write = pty_id.clone();
                let screen_state_clone = screen_state.inner().clone();
                let provider_label = provider_id.clone();
                tokio::spawn(async move {
                    let ready = tokio::task::spawn_blocking(move || {
                        crate::service::pty_io::poll_shell_prompt(
                            &screen_state_clone,
                            &pty_id_for_poll,
                            std::time::Duration::from_secs(60),
                        )
                    })
                    .await
                    .unwrap_or(false);
                    if !ready {
                        log::warn!(
                            "[restore-fresh] shell not ready for {} pane, skipping launch",
                            provider_label
                        );
                        return;
                    }
                    log::info!(
                        "[restore-fresh] launching {} (no session): {}",
                        provider_label,
                        cmd
                    );
                    match build_spawn_dispatch(&cmd) {
                        Ok(bytes) => {
                            let _ = pty_clone.lock().write(&pty_id_for_write, &bytes);
                        }
                        Err(e) => {
                            log::error!(
                                "[restore-fresh] spawn dispatch build failed: {} — skipping launch",
                                e
                            );
                        }
                    }
                });
            }
        }
    }

    Ok(pty_id)
}

#[tauri::command]
pub async fn read_pty(
    state: tauri::State<'_, PtyState>,
    output_buf: tauri::State<'_, OutputBufferState>,
    screen_state: tauri::State<'_, ScreenStateHandle>,
    mux: tauri::State<'_, MuxState>,
    tracker: tauri::State<'_, AgentTrackerState>,
    svc: tauri::State<'_, ServiceState>,
    app: AppHandle,
    pty_id: String,
) -> Result<Vec<u8>, String> {
    let _ = (&state, &output_buf, &screen_state, &mux, &tracker, &app);
    read_pty_core(&svc, pty_id).await
}

/// Long-poll one chunk of PTY output. Shared by the Tauri command and the
/// web mirror's GET /api/pty/{id}/read — every state it needs lives on
/// ServiceContext.
pub async fn read_pty_core(svc: &ServiceState, pty_id: String) -> Result<Vec<u8>, String> {
    let state = &svc.pty;
    let output_buf = &svc.output_buf;
    let screen_state = &svc.screen_state;
    let mux = &svc.mux;
    let tracker = &svc.agent_tracker;
    let app = &svc.app_handle;
    let session = {
        let manager = state.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    // Prefer backend-reader output when available; it handles decoding/state transitions.
    {
        let rx_opt = session.backend_reader_rx.lock().unwrap().take();
        if let Some(mut rx) = rx_opt {
            let result = rx.recv().await;
            match result {
                Some(mut data) => {
                    if let Some(mark) = crate::pty::manager::take_spawn_time(&pty_id) {
                        log::info!(
                            "[pty-first-byte] pty={} elapsed={}ms bytes={} (via backend channel)",
                            &pty_id[..pty_id.len().min(8)],
                            mark.when.elapsed().as_millis(),
                            data.len()
                        );
                    }
                    coalesce_backend_chunks(&mut rx, &mut data).await;
                    *session.backend_reader_rx.lock().unwrap() = Some(rx);
                    return Ok(data);
                }
                None => {
                    *session.backend_reader_rx.lock().unwrap() = None;
                    return Err("PTY closed".to_string());
                }
            }
        }
    }

    let ob = (*output_buf).clone();
    let ss = (*screen_state).clone();
    let pid = pty_id.clone();

    // One read loop owns the reader; clone fresh via try_clone_reader if taken.
    let mut reader_to_use = {
        let mut reader_slot = session.reader.lock().unwrap();
        match reader_slot.take() {
            Some(r) => r,
            None => {
                // Reader taken by old readLoop — clone a new one from master
                let pair = session.pair.lock().unwrap();
                pair.master.try_clone_reader().map_err(|e| e.to_string())?
            }
        }
    };

    let pid_for_timing = pid.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; 8192];
        let n = reader_to_use.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("EOF".to_string());
        }
        buf.truncate(n);
        ob.lock().append(&pid, &buf);
        ss.lock().process(&pid, &buf);

        Ok((buf, reader_to_use))
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Ok((ref data, _)) = result {
        if let Some(mark) = crate::pty::manager::take_spawn_time(&pid_for_timing) {
            log::info!(
                "[pty-first-byte] pty={} elapsed={}ms bytes={} (via raw reader)",
                &pid_for_timing[..pid_for_timing.len().min(8)],
                mark.when.elapsed().as_millis(),
                data.len()
            );
        }
    }

    match result {
        Ok((data, reader_back)) => {
            // Put reader back for next call
            let mut reader_slot = session.reader.lock().unwrap();
            *reader_slot = Some(reader_back);

            // Status tracking + agent event detection (delegated to service layer)
            let (prev, now) = svc.process_pty_output(&pty_id, &data);

            // Emit status change event to frontend
            if now != prev {
                let pane_id = mux.read().pane_id_for_pty(&pty_id);
                if let Some(ref pid) = pane_id {
                    crate::api::events::emit_everywhere(
                        &app,
                        "pane-status-changed",
                        serde_json::json!({
                            "pane_id": pid,
                            "status": format!("{:?}", now),
                        }),
                    );
                }
            }

            Ok(data)
        }
        Err(e) => {
            // PTY died
            tracker.lock().set_dead(&pty_id);
            let pane_id = mux.write().pane_id_for_pty(&pty_id);
            if let Some(ref pid) = pane_id {
                crate::api::events::emit_everywhere(
                    &app,
                    "pane-status-changed",
                    serde_json::json!({
                        "pane_id": pid,
                        "status": "Dead",
                    }),
                );

                // Cron fallback marks failed if agent dies without cron completion.
                use tauri::Manager;
                if let Some(cron_state) = app.try_state::<crate::CronState>() {
                    let owner = cron_state.lock().get_pane_owner(pid).cloned();
                    if let Some((job_id, run_id)) = owner {
                        let mut store = cron_state.lock();
                        // Only save if no log exists yet (don't overwrite clean summary)
                        if store.read_log(&job_id, &run_id).is_none() {
                            let text = output_buf.lock().get_text(&pty_id).unwrap_or_default();
                            let clean = crate::pty::ansi::clean_terminal_output(&text);
                            let _ = store.save_log(&job_id, &run_id, &clean);
                            let _ = store.update_run_status(
                                &job_id,
                                &run_id,
                                crate::cron::store::CronRunStatus::Failed,
                            );
                        }
                        store.unregister_pane(pid);
                        log::info!("Cron pane exited: job={}, run={}", job_id, run_id);
                        let _ = app.emit("cron-state-changed", ());
                    }
                }
            }
            Err(e)
        }
    }
}

async fn coalesce_backend_chunks(
    rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    data: &mut Vec<u8>,
) {
    const COALESCE_WAIT: std::time::Duration = std::time::Duration::from_millis(8);
    const COALESCE_CAP: usize = 64 * 1024;

    tokio::time::sleep(COALESCE_WAIT).await;
    while data.len() < COALESCE_CAP {
        match rx.try_recv() {
            Ok(mut next) => data.append(&mut next),
            Err(_) => break,
        }
    }
}

#[tauri::command]
pub async fn write_pty_string(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
    data: String,
    raw: Option<bool>,
) -> Result<(), String> {
    let _ = (&state, &mux_state);
    if raw.unwrap_or(true) {
        write_pty_string_core(&svc, pty_id, data)
    } else {
        // Bracketed paste with sacrificial space on Windows — see build_spawn_dispatch.
        // Trailing \r is stripped and submitted as Enter; agent panes go through
        // the verified injector (drain-checked Enter retry), which blocks — keep
        // it off the async runtime.
        let svc = svc.inner().clone();
        tokio::task::spawn_blocking(move || {
            let text_stripped = data.trim_end_matches('\r');
            let enter = text_stripped.len() != data.len();
            svc.send_text_to_pty_smart(&pty_id, text_stripped, enter)
        })
        .await
        .map_err(|e| format!("send task failed: {e}"))?
    }
}

/// Deliver an agent's initial launch command via the no-verify launch writer
/// (no agent TUI exists pre-boot). poll_shell_prompt blocks → off the runtime.
#[tauri::command]
pub async fn write_pty_launch_command(
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
    command: String,
) -> Result<(), String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.send_launch_command_to_pty(&pty_id, &command))
        .await
        .map_err(|e| format!("launch task failed: {e}"))?
}

/// Shared by the Tauri command and the web mirror's POST /api/pty/{id}/write.
pub fn write_pty_string_core(
    svc: &ServiceState,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    let session = {
        let manager = svc.pty.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };
    let bytes = data.into_bytes();
    parse_pty_input(svc, &svc.mux, &pty_id, &bytes);
    mark_user_activity(svc, &svc.mux, &pty_id);
    session.write_queue.send(bytes)
}

#[tauri::command]
pub async fn write_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };
    parse_pty_input(&svc, &mux_state, &pty_id, &data);
    mark_user_activity(&svc, &mux_state, &pty_id);
    session.write_queue.send(data)
}

fn mark_user_activity(
    svc: &ServiceState,
    mux: &crate::multiplexer::commands::MuxState,
    pty_id: &str,
) {
    let pane_id = mux.read().pane_id_for_pty(pty_id);
    if let Some(pid) = pane_id {
        svc.usage_activity.mark(&pid);
    }
}

/// Feed bytes to the input parser when the destination pane is not locked.
/// On match, lock `agent_type` to the detected provider — user typed an explicit
/// launch command, which is the highest-authority intent next to a real spawn.
fn parse_pty_input(
    svc: &ServiceState,
    mux: &crate::multiplexer::commands::MuxState,
    pty_id: &str,
    bytes: &[u8],
) {
    let (pane_id, locked) = {
        let m = mux.read();
        let pid = m.pane_id_for_pty(pty_id);
        let locked = pid
            .as_ref()
            .and_then(|id| m.get_pane(id))
            .map(|p| p.agent_type_locked)
            .unwrap_or(false);
        (pid, locked)
    };
    let Some(pane_id) = pane_id else { return };
    if locked {
        svc.input_parser.clear(&pane_id);
        return;
    }
    if let Some(provider_id) = svc.input_parser.process(&pane_id, bytes) {
        log::info!(
            "[input-parser] detected '{}' launch in pane {} — locking agent_type",
            provider_id,
            pane_id,
        );
        svc.set_and_lock_pane_agent_type(&pane_id, provider_id);
        svc.emit_state_changed();
    }
}

#[tauri::command]
pub async fn resize_pty(
    state: tauri::State<'_, PtyState>,
    resize_owner: tauri::State<'_, crate::ResizeOwnerState>,
    pty_id: String,
    cols: u16,
    rows: u16,
    client_id: Option<String>,
) -> Result<(), String> {
    if let Some(client_id) = client_id.as_deref() {
        if !resize_owner.lock().is_owner(&pty_id, client_id) {
            return Ok(());
        }
    }
    resize_pty_core(&state, pty_id, cols, rows)
}

#[tauri::command]
pub async fn claim_resize_owner(
    resize_owner: tauri::State<'_, crate::ResizeOwnerState>,
    events: tauri::State<'_, crate::api::events::EventBroadcasterState>,
    pane_id: String,
    pty_id: String,
    client_id: String,
    cols: u16,
    rows: u16,
    force: Option<bool>,
) -> Result<crate::resize_owner::ResizeOwner, String> {
    let owner = resize_owner.lock().claim(
        pane_id,
        pty_id,
        client_id,
        cols,
        rows,
        force.unwrap_or(false),
    );
    events.publish(crate::api::events::SystemEvent::Generic {
        event_type: "pty-resize-owner-changed".to_string(),
        data: serde_json::to_value(&owner).unwrap_or_default(),
    });
    Ok(owner)
}

/// Shared by the Tauri command and the web mirror's POST /api/pty/{id}/resize.
pub fn resize_pty_core(
    state: &PtyState,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager
            .get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let _ = session.resize(cols, rows)?;
    crate::pty::capture::CaptureRegistry::global().tap_resize(&pty_id, cols, rows);
    Ok(())
}

#[tauri::command]
pub async fn kill_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    pty_id: String,
) -> Result<(), String> {
    // Protected panes (General Agent) own backend PTYs. Ignore kill_pty
    // if it targets an already-owned PTY to avoid tearing GA down mid-launch.
    let owned_by_protected = {
        let mux = mux_state.read();
        mux.pane_id_for_pty(&pty_id)
            .and_then(|pid| mux.get_pane(&pid).map(|p| p.protected))
            .unwrap_or(false)
    };
    if owned_by_protected {
        log::info!(
            "[kill_pty] {} is owned by a protected pane — ignoring frontend kill",
            pty_id,
        );
        return Ok(());
    }
    let mut manager = state.lock();
    manager.remove(&pty_id);
    Ok(())
}
