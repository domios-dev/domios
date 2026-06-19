//! Backend PTY read loop — reads PTY output without requiring a frontend TerminalPane.
//!
//! The General Agent (and any headless pane) needs its PTY read even when no
//! frontend component is mounted. This module provides `start_backend_reader`
//! which spawns a tokio task that continuously reads from a cloned PTY reader,
//! feeds data into `OutputBuffer` + `ScreenState`, and runs `process_pty_output`
//! for agent/prompt detection — exactly the same pipeline as `read_pty` in commands.rs.
//!
//! The loop exits gracefully on EOF (PTY closed) and marks the pane as Dead.

use std::io::Read;

use tauri::Emitter;
use tokio::task::JoinHandle;

use super::ServiceState;

/// Start a backend read loop for the given PTY.
///
/// Owns the raw PTY reader and forwards chunks to the frontend channel.
/// Frontends must consume the forwarded channel or OutputBuffer tail, never
/// clone/read the raw PTY master in parallel.
///
/// Returns a `JoinHandle` that the caller can use to track or abort the task.
/// `headless` = no frontend xterm will attach (API/`ensure_pane_pty_for_send`
/// spawns). Such panes have nobody to answer PowerShell's CSI 6n cursor query,
/// so the reader must answer it itself or PowerShell blocks forever at init.
pub fn start_backend_reader(
    svc: &ServiceState,
    pty_id: &str,
    headless: bool,
) -> Result<JoinHandle<()>, String> {
    // Clone a reader from the PTY master — independent from the primary reader. Also set up the frontend channel so read_pty can receive forwarded
    // bytes.
    let (reader, frontend_tx) = {
        let manager = svc.pty.lock();
        if manager.has_backend_reader(pty_id) {
            let id = pty_id.to_string();
            return Ok(tokio::spawn(async move {
                log::debug!(
                    "[backend-reader] already active for pty {}",
                    &id[..id.len().min(8)]
                );
            }));
        }
        let session = manager
            .get_session(pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
        let pair = session.pair.lock().unwrap();
        let r = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;
        drop(pair);
        let tx = manager
            .setup_backend_channel(pty_id)
            .ok_or_else(|| format!("Failed to set up backend channel for PTY: {}", pty_id))?;
        (r, tx)
    };

    let svc = svc.clone();
    let pty_id = pty_id.to_string();

    let handle = tokio::spawn(async move {
        log::info!(
            "[backend-reader] started for pty {}",
            &pty_id[..pty_id.len().min(8)]
        );
        backend_read_loop(reader, svc, pty_id, frontend_tx, true, headless).await;
    });

    Ok(handle)
}

/// The actual read loop — runs until EOF or error.
async fn backend_read_loop(
    mut reader: Box<dyn Read + Send>,
    svc: ServiceState,
    pty_id: String,
    frontend_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    _measure_first_byte: bool,
    headless: bool,
) {
    let mut first_byte_logged = false;
    loop {
        let ob = svc.output_buf.clone();
        let ss = svc.screen_state.clone();
        let svc_for_watcher = svc.clone();
        let pid = pty_id.clone();

        // Blocking read in a dedicated thread — same pattern as read_pty.
        let result = tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 8192];
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("EOF".to_string());
            }
            buf.truncate(n);

            // Feed data into shared buffers (same as read_pty).
            ob.lock().append(&pid, &buf);
            ss.lock().process(&pid, &buf);
            // Auto-complete watcher: detects {"type":"result"} events, idle, or timeout.
            svc_for_watcher
                .run_watcher
                .feed(&*svc_for_watcher, &pid, &buf);

            Ok((buf, reader))
        })
        .await;

        match result {
            Ok(Ok((data, reader_back))) => {
                reader = reader_back;

                if !first_byte_logged {
                    first_byte_logged = true;
                    if let Some(mark) = crate::pty::manager::take_spawn_time(&pty_id) {
                        log::info!(
                            "[pty-first-byte] pty={} elapsed={}ms bytes={}",
                            &pty_id[..pty_id.len().min(8)],
                            mark.when.elapsed().as_millis(),
                            data.len()
                        );
                    }
                }

                // Answer CSI 6n so PowerShell doesn't hang. Visible frontend
                // panes let xterm.js answer (double-answering races and leaks
                // the second response into the terminal as input). Hidden panes
                // never get an xterm. Headless spawns (orchestrator/GA/API)
                // answer only until a frontend xterm claims resize ownership —
                // a background tab may stay unmounted forever.
                if data.windows(4).any(|w| w == b"\x1b[6n") {
                    let is_hidden = {
                        let mux = svc.mux.write();
                        mux.pane_id_for_pty(&pty_id)
                            .and_then(|pid| mux.get_pane(&pid).map(|p| p.hidden))
                            .unwrap_or(false)
                    };
                    let no_owner = || {
                        use tauri::Manager;
                        svc.app_handle
                            .try_state::<crate::ResizeOwnerState>()
                            .map(|ro| ro.lock().get(&pty_id).is_none())
                            .unwrap_or(true)
                    };
                    if is_hidden || (headless && no_owner()) {
                        let _ = svc.pty.lock().write(&pty_id, b"\x1b[1;1R");
                    }
                }

                let (prev, now) = svc.process_pty_output(&pty_id, &data);

                let _ = frontend_tx.try_send(data.clone());

                if now != prev {
                    let pane_id = svc.mux.write().pane_id_for_pty(&pty_id);
                    if let Some(ref pid) = pane_id {
                        crate::api::events::emit_everywhere(
                            &svc.app_handle,
                            "pane-status-changed",
                            serde_json::json!({
                                "pane_id": pid,
                                "status": format!("{:?}", now),
                            }),
                        );
                    }
                }
            }
            Ok(Err(_e)) => {
                // EOF — PTY closed.
                handle_pty_death(&svc, &pty_id);
                break;
            }
            Err(e) => {
                // spawn_blocking panicked or was cancelled.
                log::error!(
                    "[backend-reader] spawn_blocking failed for {}: {}",
                    &pty_id[..pty_id.len().min(8)],
                    e
                );
                handle_pty_death(&svc, &pty_id);
                break;
            }
        }
    }

    log::info!(
        "[backend-reader] exited for pty {}",
        &pty_id[..pty_id.len().min(8)]
    );
}

/// Mark PTY as dead and emit events — mirrors the Err branch in read_pty.
fn handle_pty_death(svc: &ServiceState, pty_id: &str) {
    let pane_id = svc.mux.write().pane_id_for_pty(pty_id);
    svc.agent_tracker.lock().set_dead(pty_id);

    if let Some(ref pid) = pane_id {
        crate::api::events::emit_everywhere(
            &svc.app_handle,
            "pane-status-changed",
            serde_json::json!({
                "pane_id": pid,
                "status": "Dead",
            }),
        );

        // Cron fallback: if agent died without calling cron complete, mark as failed.
        use tauri::Manager;
        if let Some(cron_state) = svc.app_handle.try_state::<crate::CronState>() {
            let owner = cron_state.lock().get_pane_owner(pid).cloned();
            if let Some((job_id, run_id)) = owner {
                let mut store = cron_state.lock();
                if store.read_log(&job_id, &run_id).is_none() {
                    let text = svc.output_buf.lock().get_text(pty_id).unwrap_or_default();
                    let clean = crate::pty::ansi::clean_terminal_output(&text);
                    let _ = store.save_log(&job_id, &run_id, &clean);
                    let _ = store.update_run_status(
                        &job_id,
                        &run_id,
                        crate::cron::store::CronRunStatus::Failed,
                    );
                }
                store.unregister_pane(pid);
                log::info!(
                    "[backend-reader] cron pane exited: job={}, run={}",
                    job_id,
                    run_id
                );
                let _ = svc.app_handle.emit("cron-state-changed", ());
            }
        }
    }
}
