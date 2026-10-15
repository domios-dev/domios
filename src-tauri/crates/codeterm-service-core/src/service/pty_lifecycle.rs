//! Backend PTY read loop — reads PTY output without requiring a frontend TerminalTab.
//!
//! The General Agent (and any headless tab) needs its PTY read even when no
//! frontend component is mounted. This module provides `start_backend_reader`
//! which spawns a tokio task that continuously reads from a cloned PTY reader,
//! feeds data into `OutputBuffer` + `ScreenState`, and runs `process_pty_output`
//! for agent/prompt detection — exactly the same pipeline as `read_pty` in commands.rs.
//!
//! The loop exits gracefully on EOF (PTY closed) and marks the tab as Dead.

use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::task::JoinHandle;

use super::pty_output_pipeline::{prepare_batches, OutputBatch};
use super::ServiceState;
use crate::multiplexer::types::TabCloseReason;

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
/// `headless` = no frontend xterm will attach (API/`ensure_tab_pty_for_send`
/// spawns). Such tabs have nobody to answer PowerShell's CSI 6n cursor query,
/// so the reader must answer it itself or PowerShell blocks forever at init.
pub struct PreparedBackendReader {
    reader: Box<dyn Read + Send>,
    frontend_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    lease: BackendReaderLease,
}

/// Synchronously claim the PTY stream: clone the reader AND register the
/// frontend channel in one step. Doing this BEFORE the tab becomes visible
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
    let generation = session.generation;
    drop(manager);
    if !svc
        .screen_state
        .lock()
        .bind_pty_generation(pty_id, generation)
    {
        return Err(format!("PTY {} was replaced before reader start", pty_id));
    }
    session.mark_backend_reader_started();
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
    let generation = lease.0.generation;
    tokio::spawn(async move {
        let _lease = lease;
        log::info!(
            "[backend-reader] started for pty {}",
            &pty_id[..pty_id.len().min(8)]
        );
        backend_read_loop(reader, svc, pty_id, generation, frontend_tx, true, headless).await;
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
    generation: u64,
    frontend_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    _measure_first_byte: bool,
    headless: bool,
) {
    let mut first_byte_logged = false;
    let rd_trace = std::env::var("CODETERM_RD_TRACE").is_ok();
    const FEED_QUEUE_CAPACITY: usize = 1024;
    let (feed_tx, feed_rx) = tokio::sync::mpsc::channel(FEED_QUEUE_CAPACITY);
    let feed_svc = svc.clone();
    let feed_pty_id = pty_id.clone();
    let feed_task = tokio::task::spawn_blocking(move || {
        let _role = codeterm_util::named_thread::Scope::enter(&format!("pty-feed:{feed_pty_id}"));
        process_pty_feed(feed_rx, feed_svc, feed_pty_id, generation, rd_trace)
    });
    loop {
        // Blocking read in a dedicated thread — same pattern as read_pty.
        let read_started = std::time::Instant::now();
        let read_role = format!("pty-read:{pty_id}");
        let trace_pty_id = pty_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _role = codeterm_util::named_thread::Scope::enter(&read_role);
            let mut buf = vec![0u8; 8192];
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("EOF".to_string());
            }
            let receipt = super::pty_frame_timing::Receipt::read_completed(&trace_pty_id, n);
            buf.truncate(n);

            Ok((buf, reader, receipt))
        })
        .await;
        let read_block_us = read_started.elapsed().as_micros();

        match result {
            Ok(Ok((data, reader_back, receipt))) => {
                if let Some(receipt) = &receipt {
                    receipt.phase("read_resumed", data.len());
                }
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

                let mut reply_count = 0;
                for reply in terminal_query_replies(&data, headless) {
                    reply_count += 1;
                    let reply_svc = svc.clone();
                    let reply_pty_id = pty_id.clone();
                    let reply = reply.to_vec();
                    tokio::task::spawn_blocking(move || {
                        let _ = crate::service::pty_io::submit_transaction::write_terminal_reply(
                            &reply_svc,
                            &reply_pty_id,
                            &reply,
                        );
                    });
                }
                let _ = frontend_tx.try_send(data.clone());
                let data_len = data.len();
                if let Some(receipt) = &receipt {
                    receipt.phase("enqueue_started", data_len);
                }
                let packet = OutputBatch {
                    data,
                    receipts: receipt.into_iter().collect(),
                };
                if feed_tx.send(packet).await.is_err() {
                    log::error!(
                        "[backend-reader] output processor closed for {}",
                        &pty_id[..pty_id.len().min(8)]
                    );
                    break;
                }

                if rd_trace && data_len > 16 {
                    log::info!(
                        "[rd-trace] pty={} n={} read_block={}us replies={}",
                        &pty_id[..pty_id.len().min(8)],
                        data_len,
                        read_block_us,
                        reply_count,
                    );
                }
            }
            Ok(Err(_e)) => {
                break;
            }
            Err(e) => {
                // spawn_blocking panicked or was cancelled.
                log::error!(
                    "[backend-reader] spawn_blocking failed for {}: {}",
                    &pty_id[..pty_id.len().min(8)],
                    e
                );
                break;
            }
        }
    }

    drop(feed_tx);
    match feed_task.await {
        Ok(status_task) => {
            if let Err(error) = status_task.await {
                log::error!("[backend-reader] status processor failed for {pty_id}: {error}");
            }
        }
        Err(error) => {
            log::error!("[backend-reader] screen processor failed for {pty_id}: {error}");
        }
    }
    handle_pty_death(&svc, &pty_id, generation);
    log::info!(
        "[backend-reader] exited for pty {}",
        &pty_id[..pty_id.len().min(8)]
    );
}

fn process_pty_feed(
    rx: tokio::sync::mpsc::Receiver<OutputBatch>,
    svc: ServiceState,
    pty_id: String,
    generation: u64,
    rd_trace: bool,
) -> JoinHandle<()> {
    const STATUS_QUEUE_CAPACITY: usize = 32;
    let (status_tx, status_rx) = tokio::sync::mpsc::channel(STATUS_QUEUE_CAPACITY);
    let status_svc = svc.clone();
    let status_pty_id = pty_id.clone();
    let status_task = tokio::spawn(process_pty_status(
        status_rx,
        status_svc,
        status_pty_id,
        generation,
        rd_trace,
    ));
    prepare_batches(rx, status_tx, 256 * 1024, |data| {
        svc.prepare_pty_output(&pty_id, Some(generation), data)
    });
    status_task
}

async fn process_pty_status(
    mut rx: tokio::sync::mpsc::Receiver<OutputBatch>,
    svc: ServiceState,
    pty_id: String,
    generation: u64,
    rd_trace: bool,
) {
    while let Some(data) = rx.recv().await {
        let svc = svc.clone();
        let pty_id = pty_id.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            let _role = codeterm_util::named_thread::Scope::enter(&format!("pty-status:{pty_id}"));
            let started = std::time::Instant::now();
            for receipt in &data.receipts {
                receipt.phase("observer_started", data.data.len());
            }
            let (prev, now) = svc.finish_pty_output(&pty_id, Some(generation), &data.data);
            for receipt in &data.receipts {
                receipt.phase("feed_completed", data.data.len());
            }
            if rd_trace && data.data.len() > 16 {
                log::info!(
                    "[feed-batch] pty={pty_id} n={} elapsed={}us",
                    data.data.len(),
                    started.elapsed().as_micros()
                );
            }
            svc.emit_tab_status_if_changed(&pty_id, prev, now);
        })
        .await
        {
            log::error!("[backend-reader] status batch failed: {error}");
            break;
        }
    }
}

/// Failure deadline for an exit the watcher has not recorded when the stream ends.
const SHELL_EXIT_CODE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) fn publish_shell_exit(svc: &super::ServiceContext, pty_id: &str) {
    let child_exit = svc
        .pty
        .lock()
        .get_session(pty_id)
        .map(|session| session.child_exit.clone());
    publish_shell_exit_from(pty_id, child_exit, SHELL_EXIT_CODE_DEADLINE);
}

fn publish_shell_exit_code(pty_id: &str, exit_code: Option<i32>) {
    crate::service::shell_exec::publish(
        pty_id,
        crate::service::shell_exec::ExecCompletion::ShellExited { exit_code },
    );
}

/// On Unix the output stream can end before the exit watcher records the code,
/// so a pending exit is awaited on its own thread, never on the async runtime.
fn publish_shell_exit_from(
    pty_id: &str,
    child_exit: Option<std::sync::Arc<codeterm_pty::pty::manager::ChildExit>>,
    deadline: std::time::Duration,
) {
    let Some(child_exit) = child_exit else {
        publish_shell_exit_code(pty_id, None);
        return;
    };
    if let Some(exit_code) = child_exit.resolved() {
        publish_shell_exit_code(pty_id, exit_code);
        return;
    }
    let waiter_pty_id = pty_id.to_string();
    let waiter_exit = child_exit.clone();
    let spawned = std::thread::Builder::new()
        .name("pty-exit-publish".into())
        .spawn(move || {
            let exit_code = waiter_exit.wait_resolved(deadline).unwrap_or_else(|| {
                log::warn!(
                    "[pty-exit] pty {} exit code not recorded within {:?}; publishing unknown",
                    &waiter_pty_id[..waiter_pty_id.len().min(8)],
                    deadline
                );
                None
            });
            publish_shell_exit_code(&waiter_pty_id, exit_code);
        });
    if let Err(error) = spawned {
        log::warn!(
            "[pty-exit] pty {} exit waiter spawn failed: {error}; publishing unknown",
            &pty_id[..pty_id.len().min(8)]
        );
        publish_shell_exit_code(pty_id, child_exit.code());
    }
}

/// Mark PTY as dead and emit events — mirrors the Err branch in read_pty.
fn handle_pty_death(svc: &ServiceState, pty_id: &str, generation: u64) {
    handle_pty_death_with_cause(
        svc,
        pty_id,
        generation,
        crate::crash::shutdown::active_shutdown(),
    );
}

/// Typed result of a PTY-death sweep so suppression, branch refusal, and a
/// completed prune are distinguishable instead of collapsing into silence.
#[derive(Debug, PartialEq, Eq)]
enum PtyDeathOutcome {
    StaleEof,
    PreservedForShutdown,
    PreservedForRecovery,
    NoOwningTab,
    NotRoleBearing,
    Pruned,
    PruneRefused(String),
}

fn handle_pty_death_with_cause(
    svc: &super::ServiceContext,
    pty_id: &str,
    generation: u64,
    shutdown: Option<crate::crash::lifecycle::ShutdownReason>,
) -> PtyDeathOutcome {
    let observed_tab = { svc.mux.read().tab_id_for_pty(pty_id) };
    let captured_exit = observed_tab
        .as_deref()
        .filter(|_| shutdown.is_none())
        .and_then(|tab| svc.capture_exit_tab_observation(tab));
    if !handle_pty_screen_death(&svc.pty, &svc.screen_state, pty_id, generation) {
        log::debug!(
            "[backend-reader] ignoring stale EOF for pty {} generation {}",
            &pty_id[..pty_id.len().min(8)],
            generation
        );
        return PtyDeathOutcome::StaleEof;
    }
    crate::service::orchestrator::record_reader_termination(svc, pty_id);
    let tab_id = svc.mux.read().tab_id_for_pty(pty_id);
    let raw_prev = {
        let mut tracker = svc.agent_tracker.lock();
        let prev = tracker.get_status(pty_id);
        tracker.set_dead(pty_id);
        prev
    };
    publish_shell_exit(svc, pty_id);
    svc.ext::<crate::ShellCapabilityState>()
        .lock()
        .remove(pty_id);

    if preserve_tab_authority(shutdown) {
        log::info!(
            "[backend-reader] preserving Tab authority during shutdown ({:?}): {}",
            shutdown,
            tab_id.as_deref().unwrap_or("unknown")
        );
        return PtyDeathOutcome::PreservedForShutdown;
    }

    let Some(ref pid) = tab_id else {
        return PtyDeathOutcome::NoOwningTab;
    };
    let ga_snapshot = svc.mux.read().get_tab(pid).cloned();
    if ga_snapshot
        .as_ref()
        .is_some_and(|tab| tab.worker_id.is_some())
    {
        svc.run_watcher.fail_now(
            svc,
            pty_id,
            "memory worker PTY process exited without reporting".into(),
        );
    }
    svc.run_watcher
        .fail_cron_on_process_exit(svc, pty_id, "cron PTY process exited".into());
    let has_agent_identity = ga_snapshot
        .as_ref()
        .is_some_and(|tab| tab.agent_type.is_some() || tab.agent_session_id.is_some());
    if has_agent_identity {
        log::info!(
            "[backend-reader] preserving Tab agent identity after PTY death without an exit marker: {}",
            pid
        );
        svc.settle_restore_report();
        let prev = svc.reconciled_tab_status_for_id(pid, raw_prev);
        svc.emit_tab_status_if_changed(pty_id, prev, crate::agent::status::TabStatus::Dead);
        return PtyDeathOutcome::PreservedForRecovery;
    }
    {
        svc.observe_captured_tab_exit(pid, captured_exit);
        if let Some(tab) = ga_snapshot.as_ref() {
            svc.general_agent_watchdog_event(tab, super::general_agent::WatchdogEvent::PtyDeath);
        }
        svc.settle_restore_report();
        svc.flag_bound_tasks_for_gone_tab(pid, epoch_ms());
        let prev = svc.reconciled_tab_status_for_id(pid, raw_prev);
        svc.emit_tab_status_if_changed(pty_id, prev, crate::agent::status::TabStatus::Dead);

        use tauri::Manager;
        if let Some(cron_state) = svc.app_handle.try_state::<crate::CronState>() {
            crate::cron::pty_death_fallback::fail_run_on_pty_death(svc, &cron_state, pid, pty_id);
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
            .get_tab(pid)
            .and_then(|p| match p.role {
                Some(crate::multiplexer::types::AgentRole::Manager) => Some("manager"),
                Some(crate::multiplexer::types::AgentRole::Worker) => Some("worker"),
                _ => None,
            })
            .map(str::to_string);
        let Some(role_label) = leaf_role else {
            return PtyDeathOutcome::NotRoleBearing;
        };
        if let Err(e) = svc.close_tab_with_reason(pid, TabCloseReason::PtyExit) {
            log::warn!(
                "[{}-death] prune of dead {} {} failed: {}",
                role_label,
                role_label,
                pid,
                e
            );
            return PtyDeathOutcome::PruneRefused(e);
        }
        PtyDeathOutcome::Pruned
    }
}

/// Any armed graceful shutdown suppresses role-based pruning: the PTY died because
/// the app is going down, not because the agent crashed mid-session.
fn preserve_tab_authority(shutdown: Option<crate::crash::lifecycle::ShutdownReason>) -> bool {
    shutdown.is_some()
}

struct BackendReaderLease(std::sync::Arc<crate::pty::manager::PtySession>);

impl Drop for BackendReaderLease {
    fn drop(&mut self) {
        self.0.mark_backend_reader_finished();
    }
}

pub(crate) fn handle_pty_screen_death(
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    generation: u64,
) -> bool {
    handle_pty_screen_death_after_validation(pty, screen_state, pty_id, generation, || {})
}

fn handle_pty_screen_death_after_validation(
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    generation: u64,
    after_validation: impl FnOnce(),
) -> bool {
    if !pty.lock().session_generation_matches(pty_id, generation) {
        return false;
    }
    after_validation();
    let removed = screen_state.lock().remove_if_generation(pty_id, generation);
    if removed {
        crate::service::stream_preview::remove(pty_id);
    }
    removed
}

fn terminal_query_replies(data: &[u8], _headless: bool) -> Vec<&'static [u8]> {
    let mut replies = Vec::with_capacity(3);
    if data.windows(3).any(|window| window == b"\x1b[c") {
        replies.push(b"\x1b[?1;2c".as_slice());
    }
    if data.windows(4).any(|window| window == b"\x1b[5n") {
        replies.push(b"\x1b[0n".as_slice());
    }
    if data.windows(4).any(|window| window == b"\x1b[6n") {
        replies.push(b"\x1b[1;1R".as_slice());
    }
    replies
}

#[cfg(test)]
mod tests {
    use crate::crash::lifecycle::ShutdownReason;
    use crate::multiplexer::types::AgentRole;
    use crate::pty::manager::PtyManager;
    use codeterm_pty::pty::screen_state::ScreenState;
    use codeterm_util::lock_hold::TimedMutex;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::service::shell_exec::{ExecCompletion, ExecWaiter};
    use codeterm_pty::pty::manager::ChildExit;

    /// The stream ended while the exit was still pending; the exit recorded
    /// afterwards is the one published, whichever thread runs first.
    #[tokio::test]
    async fn shell_exit_after_stream_end_publishes_the_later_recorded_code() {
        let pty_id = "pty-exit-after-stream-end";
        let waiter = ExecWaiter::register(pty_id);
        let child_exit = Arc::new(ChildExit::default());

        super::publish_shell_exit_from(pty_id, Some(child_exit.clone()), Duration::from_secs(30));
        assert!(child_exit.record(Some(3)));

        assert_eq!(
            waiter.wait(Duration::from_secs(30)).await,
            Some(ExecCompletion::ShellExited { exit_code: Some(3) })
        );
    }

    #[tokio::test]
    async fn shell_exit_publishes_unknown_when_the_exit_cannot_resolve() {
        let unrecorded = "pty-exit-deadline";
        let deadline_waiter = ExecWaiter::register(unrecorded);
        super::publish_shell_exit_from(
            unrecorded,
            Some(Arc::new(ChildExit::default())),
            Duration::ZERO,
        );
        assert_eq!(
            deadline_waiter.wait(Duration::from_secs(30)).await,
            Some(ExecCompletion::ShellExited { exit_code: None })
        );

        let unobservable = "pty-exit-unobservable";
        let unobservable_waiter = ExecWaiter::register(unobservable);
        let child_exit = Arc::new(ChildExit::default());
        child_exit.mark_unobservable();
        super::publish_shell_exit_from(unobservable, Some(child_exit), Duration::from_secs(30));
        assert_eq!(
            unobservable_waiter.wait(Duration::ZERO).await,
            Some(ExecCompletion::ShellExited { exit_code: None }),
            "an unobservable exit publishes at once"
        );
    }

    #[test]
    fn child_exit_is_recorded_exactly_once() {
        let child_exit = ChildExit::default();
        assert_eq!(child_exit.resolved(), None);
        assert!(child_exit.record(Some(3)));
        assert!(!child_exit.record(Some(4)));
        child_exit.mark_unobservable();
        assert_eq!(child_exit.wait_resolved(Duration::ZERO), Some(Some(3)));
    }

    #[test]
    fn update_owned_eof_preserves_tab_authority() {
        for cause in [
            ShutdownReason::Update,
            ShutdownReason::Signal,
            ShutdownReason::UserExit,
            ShutdownReason::Restart,
        ] {
            assert!(super::preserve_tab_authority(Some(cause)));
        }
        assert!(!super::preserve_tab_authority(None));
    }

    fn pty_state() -> crate::PtyState {
        Arc::new(TimedMutex::new("pty", PtyManager::new()))
    }

    fn spawn_generation(pty: &crate::PtyState, pty_id: &str) -> u64 {
        let (_, session) = {
            let gate = pty.lock().spawn_gate();
            crate::pty::manager::build_session(gate, None, None, 80, 24, HashMap::new())
                .map(|built| pty.lock().register_session(Some(pty_id.to_string()), built))
        }
        .expect("spawn test PTY");
        session.generation
    }

    fn replace_generation(pty: &crate::PtyState, pty_id: &str) -> u64 {
        pty.lock().kill(pty_id).expect("remove replaced PTY");
        spawn_generation(pty, pty_id)
    }

    fn bind_generation(screen_state: &crate::ScreenStateHandle, pty_id: &str, generation: u64) {
        assert!(screen_state.lock().bind_pty_generation(pty_id, generation));
    }

    #[test]
    fn device_attributes_query_is_answered_without_frontend_input() {
        assert_eq!(
            super::terminal_query_replies(b"\x1b[c", false),
            vec![b"\x1b[?1;2c".as_slice()]
        );
    }

    #[test]
    fn status_and_cursor_queries_are_answered_after_frontend_input_attaches() {
        assert_eq!(
            super::terminal_query_replies(b"\x1b[5n\x1b[6n", false),
            vec![b"\x1b[0n".as_slice(), b"\x1b[1;1R".as_slice()]
        );
    }

    #[test]
    fn terminal_queries_are_answered_for_headless_tabs() {
        assert_eq!(
            super::terminal_query_replies(b"\x1b[c\x1b[6n", true),
            vec![b"\x1b[?1;2c".as_slice(), b"\x1b[1;1R".as_slice()]
        );
    }

    #[test]
    fn pty_death_screen_teardown_closes_armed_waiter() {
        let pty = pty_state();
        let pty_generation = spawn_generation(&pty, "pty-1");
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        bind_generation(&screen_state, "pty-1", pty_generation);
        let signal = screen_state.lock().subscribe_screen_changes("pty-1");
        let generation = signal.generation();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let waiter_signal = signal.clone();
        let waiter = std::thread::spawn(move || {
            done_tx
                .send(waiter_signal.wait_after(generation, Duration::from_secs(1)))
                .unwrap();
        });

        assert!(super::handle_pty_screen_death(
            &pty,
            &screen_state,
            "pty-1",
            pty_generation,
        ));

        assert!(!done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        waiter.join().unwrap();
        pty.lock().kill("pty-1").expect("cleanup PTY");
    }

    #[test]
    fn reused_current_pty_eof_tears_down_through_global_seam() {
        let pty = pty_state();
        let generation = spawn_generation(&pty, "pty-1");
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        bind_generation(&screen_state, "pty-1", generation);
        screen_state
            .lock()
            .process("pty-1", b"current session output");

        assert!(super::handle_pty_screen_death(
            &pty,
            &screen_state,
            "pty-1",
            generation,
        ));
        assert!(screen_state.lock().get_screen_text("pty-1").is_none());
        pty.lock().kill("pty-1").expect("cleanup PTY");
    }

    #[test]
    fn replaced_old_pty_eof_does_not_teardown_replacement_through_global_seam() {
        let pty = pty_state();
        let old_generation = spawn_generation(&pty, "pty-1");
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        bind_generation(&screen_state, "pty-1", old_generation);
        let replacement_generation = replace_generation(&pty, "pty-1");
        bind_generation(&screen_state, "pty-1", replacement_generation);
        screen_state
            .lock()
            .process("pty-1", b"replacement session output");

        assert!(!super::handle_pty_screen_death(
            &pty,
            &screen_state,
            "pty-1",
            old_generation,
        ));
        assert!(pty
            .lock()
            .session_generation_matches("pty-1", replacement_generation));
        assert!(screen_state.lock().get_screen_text("pty-1").is_some());
        pty.lock().kill("pty-1").expect("cleanup PTY");
    }

    #[test]
    fn reader_termination_after_replacement_preserves_new_screen_state() {
        let pty = pty_state();
        let old_generation = spawn_generation(&pty, "pty-1");
        let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
        bind_generation(&screen_state, "pty-1", old_generation);
        let teardown_pty = pty.clone();
        let teardown_screen = screen_state.clone();
        let (validated_tx, validated_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let old_reader = std::thread::spawn(move || {
            result_tx
                .send(super::handle_pty_screen_death_after_validation(
                    &teardown_pty,
                    &teardown_screen,
                    "pty-1",
                    old_generation,
                    || {
                        validated_tx.send(()).unwrap();
                        resume_rx.recv().unwrap();
                    },
                ))
                .unwrap();
        });

        validated_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let replacement_generation = replace_generation(&pty, "pty-1");
        bind_generation(&screen_state, "pty-1", replacement_generation);
        screen_state
            .lock()
            .process("pty-1", b"replacement session output");
        resume_tx.send(()).unwrap();

        assert!(!result_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        old_reader.join().unwrap();
        assert!(pty
            .lock()
            .session_generation_matches("pty-1", replacement_generation));
        assert!(screen_state.lock().get_screen_text("pty-1").is_some());
        pty.lock().kill("pty-1").expect("cleanup PTY");
    }

    const ROLE_PTY: &str = "pty-role";

    fn role_pty_fixture(
        role: AgentRole,
    ) -> (crate::service::test_fixture::ServiceFixture, String, u64) {
        let (mut mux, orch) = crate::multiplexer::state::MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
        let tab = mux.create_tab(&workspace_id).unwrap();
        match role {
            AgentRole::Manager => {
                mux.set_tab_role_kind(&tab, AgentRole::Manager, Some("planner"), Some(&orch))
                    .expect("attach manager");
            }
            _ => {
                mux.set_tab_role(&tab, role, Some(&orch))
                    .expect("attach role tab");
            }
        }
        // Bind through the real API so `pty_to_tab` resolves the death to this tab —
        // a raw `tab.pty_id` write leaves the index empty and the sweep finds no owner.
        assert_eq!(mux.link_pty(&tab, ROLE_PTY.to_string()), None);
        assert_eq!(mux.tab_id_for_pty(ROLE_PTY), Some(tab.clone()));
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let generation = {
            let (_, session) = {
                let gate = fixture.service.pty.lock().spawn_gate();
                crate::pty::manager::build_session(gate, None, None, 80, 24, HashMap::new()).map(
                    |built| {
                        fixture
                            .service
                            .pty
                            .lock()
                            .register_session(Some(ROLE_PTY.to_string()), built)
                    },
                )
            }
            .expect("spawn test PTY");
            session.generation
        };
        assert!(fixture
            .service
            .screen_state
            .lock()
            .bind_pty_generation(ROLE_PTY, generation));
        (fixture, tab, generation)
    }

    fn assert_shutdown_death_preserves_role_tabs(cause: ShutdownReason) {
        for role in [AgentRole::Worker, AgentRole::Manager] {
            let (fixture, tab, generation) = role_pty_fixture(role);
            let outcome = super::handle_pty_death_with_cause(
                &fixture.service,
                ROLE_PTY,
                generation,
                Some(cause),
            );
            assert_eq!(
                outcome,
                super::PtyDeathOutcome::PreservedForShutdown,
                "{role:?} death during {cause:?} shutdown must take the suppression branch"
            );
            assert!(
                fixture.service.mux.read().get_tab(&tab).is_some(),
                "{role:?} tab must survive a {cause:?} shutdown PTY death"
            );
            fixture
                .service
                .pty
                .lock()
                .kill(ROLE_PTY)
                .expect("cleanup PTY");
        }
    }

    #[test]
    fn updater_shutdown_pty_death_does_not_prune_role_tabs() {
        assert_shutdown_death_preserves_role_tabs(ShutdownReason::Update);
    }

    #[test]
    fn signal_shutdown_pty_death_does_not_prune_role_tabs() {
        assert_shutdown_death_preserves_role_tabs(ShutdownReason::Signal);
    }

    #[test]
    fn window_exit_shutdown_pty_death_does_not_prune_role_tabs() {
        assert_shutdown_death_preserves_role_tabs(ShutdownReason::UserExit);
    }

    #[test]
    fn restart_shutdown_pty_death_does_not_prune_role_tabs() {
        assert_shutdown_death_preserves_role_tabs(ShutdownReason::Restart);
    }

    #[test]
    fn abrupt_pty_death_preserves_live_provider_sessions_for_restore() {
        for provider in ["claude", "codex", "opencode", "grok"] {
            let (fixture, tab_id, generation) = role_pty_fixture(AgentRole::Worker);
            let session_id = format!("{provider}-session");
            let nonce = format!("{provider}-nonce");
            {
                let mut mux = fixture.service.mux.write();
                let tab = mux.get_tab_mut(&tab_id).expect("worker tab");
                tab.agent_type = Some(provider.into());
                tab.agent_session_id = Some(session_id.clone());
                tab.session_binding_nonce = Some(nonce.clone());
                tab.reported_session_binding_nonce = Some(nonce);
            }

            let outcome =
                super::handle_pty_death_with_cause(&fixture.service, ROLE_PTY, generation, None);

            assert_eq!(
                outcome,
                super::PtyDeathOutcome::PreservedForRecovery,
                "{provider}"
            );
            let snapshot = fixture.service.mux.read().to_session_snapshot();
            let restored =
                crate::multiplexer::state::MultiplexerState::from_session_snapshot(&snapshot);
            let restored_tab = restored.get_tab(&tab_id).expect("restored worker tab");
            assert_eq!(restored_tab.agent_type.as_deref(), Some(provider));
            assert_eq!(
                restored_tab.agent_session_id.as_deref(),
                Some(session_id.as_str())
            );
            assert_eq!(restored_tab.role, Some(AgentRole::Worker));
        }
    }

    #[test]
    fn in_session_agent_death_without_shutdown_still_prunes_role_tabs() {
        for role in [AgentRole::Worker, AgentRole::Manager] {
            let (fixture, tab, generation) = role_pty_fixture(role);
            let parent = fixture
                .service
                .mux
                .read()
                .get_tab(&tab)
                .and_then(|tab| tab.parent_tab_id.clone())
                .expect("role tab parent");
            let _parent =
                fixture.bind_recorded_agent_recipient(&parent, "parent-session", "parent-nonce");
            let outcome =
                super::handle_pty_death_with_cause(&fixture.service, ROLE_PTY, generation, None);
            assert_eq!(
                outcome,
                super::PtyDeathOutcome::Pruned,
                "{role:?} death with no shutdown armed must reach and complete the prune branch"
            );
            assert!(
                fixture.service.mux.read().get_tab(&tab).is_none(),
                "{role:?} tab must still be pruned when the agent dies in-session"
            );
        }
    }
}
