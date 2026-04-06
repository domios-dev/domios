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
/// Clones a second reader handle via `try_clone_reader` so this coexists with
/// the frontend `read_pty` command. When both are active, whichever reads first
/// gets the data — the frontend reader will naturally starve (return nothing)
/// while this loop is running, which is fine for headless panes.
///
/// Returns a `JoinHandle` that the caller can use to track or abort the task.
pub fn start_backend_reader(
    svc: &ServiceState,
    pty_id: &str,
) -> Result<JoinHandle<()>, String> {
    // Clone a reader from the PTY master — independent from the primary reader.
    // Also set up the frontend channel so read_pty can receive forwarded bytes.
    let (reader, frontend_tx) = {
        let manager = svc.pty.lock();
        let session = manager
            .get_session(pty_id)
            .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
        let pair = session.pair.lock().unwrap();
        let r = pair.master
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
        log::info!("[backend-reader] started for pty {}", &pty_id[..pty_id.len().min(8)]);
        backend_read_loop(reader, svc, pty_id, frontend_tx).await;
    });

    Ok(handle)
}

/// The actual read loop — runs until EOF or error.
async fn backend_read_loop(
    mut reader: Box<dyn Read + Send>,
    svc: ServiceState,
    pty_id: String,
    frontend_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
) {
    loop {
        let ob = svc.output_buf.clone();
        let ss = svc.screen_state.clone();
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

            Ok((buf, reader))
        })
        .await;

        match result {
            Ok(Ok((data, reader_back))) => {
                reader = reader_back;

                // Status tracking + agent detection (same as read_pty).
                let (prev, now) = svc.process_pty_output(&pty_id, &data);

                // Forward raw bytes to frontend read_pty channel (ignore send errors — frontend may not be connected).
                let _ = frontend_tx.send(data.clone());

                if now != prev {
                    let pane_id = svc.mux.lock().pane_id_for_pty(&pty_id);
                    if let Some(ref pid) = pane_id {
                        let _ = svc.app_handle.emit(
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

    log::info!("[backend-reader] exited for pty {}", &pty_id[..pty_id.len().min(8)]);
}

/// Mark PTY as dead and emit events — mirrors the Err branch in read_pty.
fn handle_pty_death(svc: &ServiceState, pty_id: &str) {
    svc.agent_tracker.lock().set_dead(pty_id);
    let pane_id = svc.mux.lock().pane_id_for_pty(pty_id);

    if let Some(ref pid) = pane_id {
        let _ = svc.app_handle.emit(
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
                    let clean = crate::api::routes::clean_terminal_output(&text);
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

