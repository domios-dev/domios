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
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::task::JoinHandle;

use super::ServiceState;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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
pub struct PreparedBackendReader {
    reader: Box<dyn Read + Send>,
    frontend_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    lease: BackendReaderLease,
}

/// Synchronously claim the PTY stream: clone the reader AND register the
/// frontend channel in one step. Doing this BEFORE the pane becomes visible
/// closes the race where read_pty attaches first, clones a second raw reader,
/// and the two readers split the byte stream (eaten prompts, blank terminals).
/// Returns Ok(None) when a backend reader is already active.
pub fn prepare_backend_reader(
    svc: &ServiceState,
    pty_id: &str,
) -> Result<Option<PreparedBackendReader>, String> {
    let manager = svc.pty.lock();
    if manager.has_backend_reader(pty_id) {
        return Ok(None);
    }
    let session = manager
        .get_session(pty_id)
        .ok_or_else(|| format!("PTY not found: {}", pty_id))?;
    let pair_guard = session.pair.lock().unwrap();
    let Some(pair) = pair_guard.as_ref() else {
        return Err(format!("PTY {} is closed", pty_id));
    };
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;
    drop(pair_guard);
    let frontend_tx = manager
        .setup_backend_channel(pty_id)
        .ok_or_else(|| format!("Failed to set up backend channel for PTY: {}", pty_id))?;
    session
        .backend_reader_active
        .store(true, std::sync::atomic::Ordering::Release);
    Ok(Some(PreparedBackendReader {
        reader,
        frontend_tx,
        lease: BackendReaderLease(session),
    }))
}

/// Run the read loop for a stream claimed via `prepare_backend_reader`.
pub fn spawn_prepared_backend_reader(
    svc: &ServiceState,
    pty_id: &str,
    prepared: PreparedBackendReader,
    headless: bool,
) -> JoinHandle<()> {
    let svc = svc.clone();
    let pty_id = pty_id.to_string();
    let PreparedBackendReader {
        reader,
        frontend_tx,
        lease,
    } = prepared;
    tokio::spawn(async move {
        let _lease = lease;
        log::info!(
            "[backend-reader] started for pty {}",
            &pty_id[..pty_id.len().min(8)]
        );
        backend_read_loop(reader, svc, pty_id, frontend_tx, true, headless).await;
    })
}

pub fn start_backend_reader(
    svc: &ServiceState,
    pty_id: &str,
    headless: bool,
) -> Result<JoinHandle<()>, String> {
    match prepare_backend_reader(svc, pty_id)? {
        Some(prepared) => Ok(spawn_prepared_backend_reader(
            svc, pty_id, prepared, headless,
        )),
        None => {
            let id = pty_id.to_string();
            Ok(tokio::spawn(async move {
                log::debug!(
                    "[backend-reader] already active for pty {}",
                    &id[..id.len().min(8)]
                );
            }))
        }
    }
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
        // Blocking read in a dedicated thread — same pattern as read_pty.
        let result = tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 8192];
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("EOF".to_string());
            }
            buf.truncate(n);

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
                    if should_answer_cursor_query(is_hidden, headless, no_owner()) {
                        let _ = svc.pty.lock().write(&pty_id, b"\x1b[1;1R");
                    }
                }
                let (prev, now) = svc.feed_pty_output(&pty_id, &data);

                let _ = frontend_tx.try_send(data.clone());

                svc.emit_pane_status_if_changed(&pty_id, prev, now);
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
    let pane_id = svc.mux.read().pane_id_for_pty(pty_id);
    svc.agent_tracker.lock().set_dead(pty_id);
    svc.shell_caps.lock().remove(pty_id);

    if let Some(ref pid) = pane_id {
        svc.flag_bound_tasks_for_gone_pane(pid, epoch_ms());
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
            // Hold the cron lock ACROSS owner-lookup and the read/write below.
            let mut store = cron_state.lock();
            let owner = store.get_pane_owner(pid).cloned();
            if let Some((job_id, run_id)) = owner {
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
                crate::api::events::emit_everywhere(&svc.app_handle, "cron-run-finished", ());
            }
        }

        let watchdog = {
            let mux = svc.mux.read();
            let store = svc.report_store.lock();
            crate::service::hierarchy::worker_death_report(&mux, &store, pid)
        };
        if let Some((target_id, msg)) = watchdog {
            let role_tag = if msg.starts_with("MANAGER ") {
                "manager-death"
            } else {
                "worker-death"
            };
            match svc.submit_report(pid, &msg, Default::default(), None, None, None) {
                Ok(_) => log::info!(
                    "[{}] synthesized dead-report for {} -> {}",
                    role_tag,
                    pid,
                    target_id
                ),
                Err(e) => log::warn!("[{}] synthetic report failed for {}: {}", role_tag, pid, e),
            }
        }

        let leaf_role = svc
            .mux
            .read()
            .get_pane(pid)
            .and_then(|p| match p.role {
                Some(crate::multiplexer::types::AgentRole::Manager) => Some("manager"),
                Some(crate::multiplexer::types::AgentRole::Worker) => Some("worker"),
                _ => None,
            })
            .map(str::to_string);
        if let Some(role_label) = leaf_role {
            if let Err(e) = svc.close_pane(pid) {
                log::warn!(
                    "[{}-death] prune of dead {} {} failed: {}",
                    role_label,
                    role_label,
                    pid,
                    e
                );
            }
        }
    }
}

struct BackendReaderLease(std::sync::Arc<crate::pty::manager::PtySession>);

impl Drop for BackendReaderLease {
    fn drop(&mut self) {
        self.0
            .backend_reader_active
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

fn should_answer_cursor_query(is_hidden: bool, _headless: bool, no_owner: bool) -> bool {
    is_hidden || no_owner
}

#[cfg(test)]
mod tests {
    #[test]
    fn cursor_query_is_answered_for_visible_pane_before_renderer_owns_pty() {
        assert!(super::should_answer_cursor_query(false, false, true));
    }

    #[test]
    fn cursor_query_is_left_to_renderer_once_visible_pane_has_owner() {
        assert!(!super::should_answer_cursor_query(false, false, false));
    }

    #[test]
    fn cursor_query_is_answered_for_hidden_pane_even_with_owner_state_unknown() {
        assert!(super::should_answer_cursor_query(true, false, false));
    }
}
