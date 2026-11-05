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

mod status_parser;
mod status_responder;

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
    if !svc.screen_state.bind_pty_generation(pty_id, generation) {
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
    spawn_reader_task(async move {
        let _lease = lease;
        log::info!(
            "[backend-reader] started for pty {}",
            &pty_id[..pty_id.len().min(8)]
        );
        let replies = status_responder::ReplyOwner::start(&pty_id, _lease.0.clone());
        backend_read_loop(
            reader,
            svc,
            pty_id,
            generation,
            frontend_tx,
            replies,
            headless,
        )
        .await;
    })
}

fn spawn_reader_task(
    task: impl std::future::Future<Output = ()> + Send + 'static,
) -> JoinHandle<()> {
    // Reader feeds call synchronous provider hooks, so they must inherit the service runtime,
    // even when a launch thread uses a private CurrentThread runtime to await the reader.
    crate::async_runtime::spawn(task)
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
    replies: status_responder::ReplyOwner,
    _headless: bool,
) {
    let mut status_parser = status_parser::StatusParser::default();
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
                    crate::startup::boot_timing::mark("-", Some(&pty_id), "first_output");
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
                terminal_query_replies(&data, &mut status_parser, |reply| {
                    reply_count += usize::from(replies.enqueue(reply));
                });
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

    drop(replies);
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

        if let Some(cron_state) = svc.try_ext::<crate::CronState>() {
            crate::cron::pty_death_fallback::fail_run_on_pty_death(svc, cron_state, pid, pty_id);
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
    screen_state.remove_if_generation(pty_id, generation)
}

fn terminal_query_replies(
    data: &[u8],
    parser: &mut status_parser::StatusParser,
    mut reply: impl FnMut(&'static [u8]),
) {
    if data.windows(3).any(|window| window == b"\x1b[c") {
        reply(b"\x1b[?1;2c");
    }
    parser.feed(data, &mut reply);
    if data.windows(4).any(|window| window == b"\x1b[6n") {
        reply(b"\x1b[1;1R");
    }
}

#[cfg(test)]
mod tests;
