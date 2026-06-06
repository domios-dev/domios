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
    // Protected panes (General Agent) own their lifecycle from the backend —
    // `service::general_agent::lifecycle` spawns its OWN PTY, links it, and
    // reads from it. If the frontend also fires spawn_pty during rehydrate,
    // we get TWO PTYs racing for the same pane: link_pty then orphan-kills
    // the backend one, the GA reader is left holding a dead PTY, and GA
    // hangs 60s on `poll_shell_prompt` before the watchdog gives up.
    //
    // Resolution: when a protected pane already has a backend-spawned PTY,
    // return that pty_id instead of spawning a second one. When the backend
    // hasn't gotten there yet, briefly wait — GA's lifecycle starts within
    // a couple seconds of daemon boot.
    if let Some(ref pid) = pane_id {
        let (is_protected, existing) = {
            let m = mux_state.lock();
            let p = m.get_pane(pid);
            (
                p.map(|p| p.protected).unwrap_or(false),
                p.and_then(|p| p.pty_id.clone()),
            )
        };
        // If the pane already has a live PTY, reuse it instead of spawning a
        // second one. Without this, frontend re-mounts of the terminal
        // component (tab clicks, navigation) tear down the running agent
        // and trigger an unwanted resume from the auto-detected session id.
        // Protected panes additionally get a brief wait so the backend GA
        // lifecycle has a chance to spawn the PTY first.
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
                    .lock()
                    .get_pane(pid)
                    .and_then(|p| p.pty_id.clone())
                {
                    log::info!(
                        "[spawn_pty] protected pane {} already has backend pty {} — reusing",
                        pid,
                        existing,
                    );
                    return Ok(existing);
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
        let (session_id, is_protected, pane_cwd, restored_from_snapshot, agent_type) = {
            let m = mux_state.lock();
            let pane = m.get_pane(pid);
            (
                pane.and_then(|p| p.agent_session_id.clone()),
                pane.map(|p| p.protected).unwrap_or(false),
                pane.and_then(|p| p.cwd.clone()),
                pane.map(|p| p.restored_from_snapshot).unwrap_or(false),
                pane.and_then(|p| p.agent_type.clone()),
            )
        };
        let mut should_fresh_launch =
            restored_from_snapshot && !is_protected && agent_type.is_some();
        let agent_type_for_fresh = agent_type.clone();

        // Auto-resume in spawn_pty must ONLY fire when the pane was restored
        // from a daemon snapshot (kill → restart → frontend mounts panes).
        // For a fresh `sessions resume` / `agent spawn --resume`, the backend
        // already drives the launch via `launch_agent_in_background`. Without
        // this gate, both paths race: each polls shell-prompt + sends the
        // resume command, the second send lands in the TUI's input box →
        // garbled launch + stuck "Resume session" picker (see the prod bug
        // screenshot where the launch command appeared on screen twice).
        if let Some(session_id) = session_id
            .filter(|_| !is_protected)
            .filter(|_| restored_from_snapshot)
        {
            let provider_id = agent_type.unwrap_or_else(crate::config::default_agent_provider);

            // Validate the session file still exists. If it's gone (cleared cache,
            // different machine, /new inside the TUI last run), resuming produces
            // spam like "No conversation found with session ID: ..." which the
            // watchdog then retries forever. Clear and fall through so the
            // restored-from-snapshot branch can spawn a fresh agent if needed.
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

                // Build resume command via provider registry (provider-agnostic).
                // Per-provider config: each plugin has its own resume_sessions /
                // resume_skip_permissions in `[agents.<id>]` (back-compat fallback
                // to global `[agent]` is handled by `provider_config()`).
                let pc = crate::config::provider_config(&provider_id);
                let _ = config_state; // global lock no longer needed for these fields
                let resume_enabled = pc.resume_sessions.unwrap_or(true);
                let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
                let resume_cmd = svc
                    .agent_registry
                    .with_provider(&provider_id, |p| {
                        p.build_resume_command(&session_id, skip_perms)
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

                        // 60s is generous but matters under contention: at app
                        // restart dozens of panes spawn in parallel and shell
                        // initialization can stall well past the old 15s budget
                        // (observed 14-20s for Nth-pane zsh under load on macOS).
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
            // Pane was tagged with an agent but had no session to resume —
            // typically a "silent" claude pane the user spawned and never
            // talked to before a kill. Without this branch the pane comes
            // back as a bare powershell, which is exactly the previously
            // reported bug. Launch the agent fresh so the pane matches what
            // the snapshot promised. Gated on restored_from_snapshot so
            // it does NOT race the orchestrator's own launch on a fresh
            // `agent spawn` (would otherwise paste the launch twice).
            // Flag cleared after first spawn so a re-mount stays quiet.
            svc.clear_pane_restored_flag(pid);
            let pc = crate::config::provider_config(&provider_id);
            let skip_perms = pc.resume_skip_permissions.unwrap_or(true);
            let launch_cmd = svc.agent_registry.with_provider(&provider_id, |p| {
                p.build_launch_command(&crate::agent::provider::AgentLaunchParams {
                    skip_permissions: skip_perms,
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
            let pane_id = mux.lock().pane_id_for_pty(&pty_id);
            if let Some(ref pid) = pane_id {
                crate::api::events::emit_everywhere(
                    &app,
                    "pane-status-changed",
                    serde_json::json!({
                        "pane_id": pid,
                        "status": "Dead",
                    }),
                );

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

#[tauri::command]
pub async fn write_pty_string(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    svc: tauri::State<'_, ServiceState>,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    let _ = (&state, &mux_state);
    write_pty_string_core(&svc, pty_id, data)
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
    let pane_id = mux.lock().pane_id_for_pty(pty_id);
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
        let m = mux.lock();
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
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    resize_pty_core(&state, pty_id, cols, rows)
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

    let pair = session.pair.lock().unwrap();
    pair.master
        .resize(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Resize failed: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn kill_pty(
    state: tauri::State<'_, PtyState>,
    mux_state: tauri::State<'_, crate::multiplexer::commands::MuxState>,
    pty_id: String,
) -> Result<(), String> {
    // Protected panes (General Agent) own their PTY from the backend. The
    // frontend pty-connection effect calls kill_pty if the React component
    // dispose-races a `spawn_pty` that returned the backend's existing PTY,
    // which would tear down GA mid-launch. Refuse to kill those.
    let owned_by_protected = {
        let mux = mux_state.lock();
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
