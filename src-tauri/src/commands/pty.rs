use std::collections::HashMap;
use std::io::Read;
use tauri::{AppHandle, Emitter};

use crate::multiplexer::commands::MuxState;
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
    let mut env_vars: HashMap<String, String> = HashMap::new();
    if let Some(ref pid) = pane_id {
        env_vars.insert("CODETERM_PANE_ID".to_string(), pid.clone());
        // Pass role/parent info so agents know their place in hierarchy
        let mux = mux_state.lock();
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
    // Hook callbacks (Notification/Stop/PreCompact/SessionStart) POST to our
    // REST API with an Authorization header read from this env var. Without
    // it, the hook curl hits auth_middleware and 401s silently.
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

    // Auto-resume: if this pane has a saved session_id, resume it after shell is ready.
    // This runs in backend so it works even for non-visible tabs.
    // Skip protected panes (General Agent) — backend manages their lifecycle.
    if let Some(ref pid) = pane_id {
        let (session_id, is_protected, pane_cwd) = {
            let m = mux_state.lock();
            let pane = m.get_pane(pid);
            (
                pane.and_then(|p| p.agent_session_id.clone()),
                pane.map(|p| p.protected).unwrap_or(false),
                pane.and_then(|p| p.cwd.clone()),
            )
        };
        let agent_type = mux_state.lock().get_pane(pid)
            .and_then(|p| p.agent_type.clone());
        if let Some(session_id) = session_id.filter(|_| !is_protected) {
            let provider_id = agent_type.unwrap_or_else(crate::config::default_agent_provider);

            // Validate the session file still exists. If it's gone (cleared cache,
            // different machine, /new inside the TUI last run), resuming produces
            // spam like "No conversation found with session ID: ..." which the
            // watchdog then retries forever. Clear and bail quietly instead.
            let session_valid = pane_cwd.as_deref().map(|cwd| {
                svc.agent_registry.with_provider(&provider_id, |p| {
                    p.session_exists(cwd, &session_id)
                }).unwrap_or(false)
            }).unwrap_or(false);

            if !session_valid {
                log::info!(
                    "[resume] session {} for pane {} has no backing file under {:?} — clearing, skipping resume",
                    session_id, pid, pane_cwd
                );
                svc.set_agent_session_id(pid, None);
                svc.emit_state_changed();
                return Ok(pty_id);
            }

            svc.set_pane_agent_type(pid, Some(provider_id.clone()));
            svc.emit_state_changed();

            // Build resume command via provider registry (provider-agnostic).
            // Per-provider config: each plugin has its own resume_sessions /
            // resume_skip_permissions in `[agents.<id>]` (back-compat fallback
            // to global `[agent]` is handled by `provider_config()`).
            let pc = crate::config::provider_config(&provider_id);
            let _ = config_state; // global lock no longer needed for these fields
            let resume_enabled = pc.resume_sessions.unwrap_or(true);
            let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
            let resume_cmd = svc.agent_registry.with_provider(&provider_id, |p| {
                p.build_resume_command(&session_id, skip_perms)
            }).flatten();

            if let Some(cmd) = resume_cmd {
                let pty_clone = state.inner().clone();
                let pty_id_for_poll = pty_id.clone();
                let pty_id_for_write = pty_id.clone();
                let screen_state_clone = screen_state.inner().clone();
                tokio::spawn(async move {
                    if !resume_enabled { return; }

                    // 60s is generous but matters under contention: at app
                    // restart dozens of panes spawn in parallel and shell
                    // initialization can stall well past the old 15s budget
                    // (observed 14-20s for Nth-pane zsh under load on macOS).
                    let ready = tokio::task::spawn_blocking(move || {
                        crate::service::pty_io::poll_shell_prompt(
                            &screen_state_clone, &pty_id_for_poll,
                            std::time::Duration::from_secs(60),
                        )
                    }).await.unwrap_or(false);

                    if !ready {
                        log::warn!("[resume] shell not ready for session {}, skipping", session_id);
                        return;
                    }

                    log::info!("[resume] sending: {}", cmd);
                    let paste = format!("\x1b[200~{}\x1b[201~\r", cmd);
                    let _ = pty_clone.lock().write(&pty_id_for_write, paste.as_bytes());
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
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    // If a backend reader is active, it forwards output via channel to avoid competing reads.
    // Read from channel instead of raw PTY. Processing (OutputBuffer, ScreenState, process_pty_output)
    // is already done by the backend reader — don't duplicate it here.
    {
        let rx_opt = session.backend_reader_rx.lock().unwrap().take();
        if let Some(mut rx) = rx_opt {
            let result = rx.recv().await;
            match result {
                Some(data) => {
                    if let Some(mark) = crate::pty::manager::take_spawn_time(&pty_id) {
                        log::info!(
                            "[pty-first-byte] pty={} elapsed={}ms bytes={} (via backend channel)",
                            &pty_id[..pty_id.len().min(8)],
                            mark.when.elapsed().as_millis(),
                            data.len()
                        );
                    }
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

    // Take reader from session — only one readLoop can own it at a time.
    // If reader was already taken (by a previous readLoop that's blocking in read()),
    // create a new one via try_clone_reader.
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
                let pane_id = mux.lock().pane_id_for_pty(&pty_id);
                if let Some(ref pid) = pane_id {
                    let _ = app.emit("pane-status-changed", serde_json::json!({
                        "pane_id": pid,
                        "status": format!("{:?}", now),
                    }));
                }
            }

            Ok(data)
        }
        Err(e) => {
            // PTY died
            tracker.lock().set_dead(&pty_id);
            let pane_id = mux.lock().pane_id_for_pty(&pty_id);
            if let Some(ref pid) = pane_id {
                let _ = app.emit("pane-status-changed", serde_json::json!({
                    "pane_id": pid,
                    "status": "Dead",
                }));

                // Cron fallback: if agent died without calling cron complete, mark as failed
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
                            let _ = store.update_run_status(&job_id, &run_id, crate::cron::store::CronRunStatus::Failed);
                        }
                        store.unregister_pane(pid);
                        log::info!("Cron pane exited: job={}, run={}", job_id, run_id);
                        let _ = app.emit("cron-state-changed", ());
                    }
                }
            }
            Err(e)
        },
    }
}

#[tauri::command]
pub async fn write_pty_string(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    session.write_queue.send(data.into_bytes())
}

#[tauri::command]
pub async fn write_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };
    session.write_queue.send(data)
}

#[tauri::command]
pub async fn resize_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = {
        let manager = state.lock();
        manager.get_session(&pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?
    };

    let pair = session.pair.lock().unwrap();
    pair.master.resize(portable_pty::PtySize {
        rows, cols, pixel_width: 0, pixel_height: 0,
    }).map_err(|e| format!("Resize failed: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn kill_pty(
    state: tauri::State<'_, PtyState>,
    pty_id: String,
) -> Result<(), String> {
    let mut manager = state.lock();
    manager.remove(&pty_id);
    Ok(())
}
