use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;
use tauri::Manager;

use crate::multiplexer::types::TabCloseReason;
use crate::service::ServiceContext;

// Default idle wait is 30s (Claude can think between events while still streaming partials).
// `memory.worker_timeout_seconds` can lower/higher this at runtime.
const IDLE_COMPLETE_SECS: u64 = 30;
const SCANNER_BUFFER_CAP: usize = 64 * 1024;

const NUDGE_AFTER_IDLE_SECS: u64 = 18;
const MAX_NUDGES: u32 = 3;
const RESTART_GRACE_SECS: u64 = 25;
const RESTART_SETTLE_SECS: u64 = 20;
const MAX_RESTARTS: u32 = 2;

#[derive(Clone, Debug)]
pub enum AgentRunKind {
    MemWorker { worker_id: String },
    Cron { job_id: String, run_id: String },
}

#[derive(Debug, Default)]
struct CompletionSignal {
    is_error: bool,
    subtype: Option<String>,
    result: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ResultEvent {
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    result: Option<String>,
}

struct RunState {
    pty_id: String,
    tab_id: String,
    kind: AgentRunKind,
    scanner: Mutex<ResultEventScanner>,
    started_at: Instant,
    last_byte_at: Mutex<Instant>,
    completed: AtomicBool,
    armed: AtomicBool,
    saw_working: AtomicBool,
    saw_post_session_progress: AtomicBool,
    nudge_count: AtomicU32,
    restart_count: AtomicU32,
    last_restart_at: Mutex<Instant>,
    resume_system_prompt: Option<String>,
}

pub struct RunWatcherRegistry {
    runs: Mutex<HashMap<String, Arc<RunState>>>,
}

impl Default for RunWatcherRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RunWatcherRegistry {
    pub fn new() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        svc: ServiceContext,
        pty_id: String,
        tab_id: String,
        kind: AgentRunKind,
        timeout_secs: u64,
        resume_system_prompt: Option<String>,
    ) {
        let state = Arc::new(RunState {
            pty_id: pty_id.clone(),
            tab_id,
            kind,
            scanner: Mutex::new(ResultEventScanner::new()),
            started_at: Instant::now(),
            last_byte_at: Mutex::new(Instant::now()),
            completed: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            saw_working: AtomicBool::new(false),
            saw_post_session_progress: AtomicBool::new(false),
            nudge_count: AtomicU32::new(0),
            restart_count: AtomicU32::new(0),
            last_restart_at: Mutex::new(Instant::now()),
            resume_system_prompt,
        });
        self.runs.lock().insert(pty_id, state.clone());
        // Use tauri's runtime handle so this works even when register() is called from outside a tokio reactor (e.g. main thread spawn paths).
        tauri::async_runtime::spawn(watchdog_loop(svc, state, timeout_secs));
    }

    /// Tell the watcher this run is already finished via an explicit
    /// completion path (e.g. `codeterm cron complete`) so the idle / hard
    /// timeout watchdog does NOT fire afterwards and clobber the Success
    /// status with Failed.
    pub fn mark_completed_externally(&self, pty_id: &str) {
        let mut runs = self.runs.lock();
        if let Some(state) = runs.get(pty_id) {
            state.completed.store(true, Ordering::Release);
        }
        runs.remove(pty_id);
    }

    /// Finalize a run as Failed immediately, for a definite failure (e.g. a
    /// genuine PTY write failure during task delivery) rather than waiting on
    /// the idle / hard-timeout backstop. No-op if the run isn't registered yet
    /// (delivery raced ahead of registration) or already completed.
    pub fn fail_now(&self, svc: &ServiceContext, pty_id: &str, reason: String) {
        let state = self.runs.lock().get(pty_id).cloned();
        let Some(state) = state else {
            return;
        };
        if state.completed.swap(true, Ordering::AcqRel) {
            return;
        }
        log::warn!(
            "[run-watcher] failing run now: tab={} reason={}",
            state.tab_id,
            reason
        );
        finish_run(svc, &state, false, reason);
        self.runs.lock().remove(pty_id);
    }

    pub fn arm(&self, pty_id: &str) {
        if let Some(state) = self.runs.lock().get(pty_id) {
            state.armed.store(true, Ordering::Release);
        }
    }

    pub fn observe_output_status(&self, pty_id: &str, status: crate::agent::status::TabStatus) {
        if status != crate::agent::status::TabStatus::Working {
            return;
        }
        if let Some(state) = self.runs.lock().get(pty_id) {
            let armed = state.armed.load(Ordering::Acquire);
            if armed {
                state.saw_working.store(true, Ordering::Release);
            }
        }
    }

    pub fn observe_durable_transcript(&self, svc: &ServiceContext, tab_id: &str, observed: bool) {
        if !observed {
            return;
        }
        let pty_id = self
            .runs
            .lock()
            .values()
            .find(|state| state.tab_id == tab_id)
            .and_then(|state| {
                if state.armed.load(Ordering::Acquire) {
                    state
                        .saw_post_session_progress
                        .store(true, Ordering::Release);
                    Some(state.pty_id.clone())
                } else {
                    None
                }
            });
        if let Some(pty_id) = pty_id {
            self.complete_via_semantic_idle(svc, &pty_id);
        }
    }

    pub fn complete_via_semantic_idle(&self, svc: &ServiceContext, pty_id: &str) {
        let state = self.runs.lock().get(pty_id).cloned();
        let Some(state) = state else {
            return;
        };
        let (idle, session_bound) = tab_completion_state(svc, &state.tab_id);
        if !can_complete_on_semantic_idle(
            state.armed.load(Ordering::Acquire),
            state.saw_working.load(Ordering::Acquire),
            state.saw_post_session_progress.load(Ordering::Acquire),
            idle,
            session_bound,
        ) || state.completed.swap(true, Ordering::AcqRel)
        {
            return;
        }
        log::info!("[run-watcher] semantic-idle complete: tab={}", state.tab_id);
        finish_run(svc, &state, true, "(agent returned to idle)".to_string());
        self.runs.lock().remove(pty_id);
    }

    /// Completion-via-report: a provider Stop POST that carried the tab's
    /// binding nonce. Claims and completes the nonce-matching tab's run only;
    /// any other nonce claims nothing, leaving every classifier-fallback
    /// completion path (semantic idle, idle backstop, scanner) in charge.
    pub fn complete_via_stop_report(&self, svc: &ServiceContext, tab_id: &str, report_nonce: &str) {
        let Some(state) = self.claim_stop_report_run(&svc.mux, tab_id, report_nonce) else {
            return;
        };
        log::info!("[run-watcher] stop-report complete: tab={}", tab_id);
        finish_run(svc, &state, true, "(provider reported stop)".to_string());
        self.runs.lock().remove(&state.pty_id);
    }

    /// The nonce gate: a run is claimable only when the report nonce equals
    /// the tab's own launch nonce at claim time. Rejection touches nothing.
    fn claim_stop_report_run(
        &self,
        mux: &crate::multiplexer::commands::MuxState,
        tab_id: &str,
        report_nonce: &str,
    ) -> Option<Arc<RunState>> {
        let launch_nonce = mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone())?;
        if launch_nonce != report_nonce {
            return None;
        }
        let state = self
            .runs
            .lock()
            .values()
            .find(|s| s.tab_id == tab_id)
            .cloned()?;
        (!state.completed.swap(true, Ordering::AcqRel)).then_some(state)
    }

    /// Stop hook fired = the agent finished its turn. For a hidden worker/cron
    /// run that didn't explicitly report, treat this as success completion —
    /// faster and cleaner than the 30s idle backstop. No-op for visible tabs
    /// (no registered run) and for runs already completed via report.
    pub fn complete_via_stop_hook(&self, svc: &ServiceContext, tab_id: &str) {
        let state = self
            .runs
            .lock()
            .values()
            .find(|s| s.tab_id == tab_id)
            .cloned();
        let Some(state) = state else {
            return;
        };
        if state.completed.swap(true, Ordering::AcqRel) {
            return;
        }
        log::info!("[run-watcher] stop-hook complete: tab={}", tab_id);
        finish_run(svc, &state, true, "(agent stopped)".to_string());
        self.runs.lock().remove(&state.pty_id);
    }

    /// Test seam: register a run without spawning the watchdog loop.
    #[cfg(test)]
    fn insert_run_for_test(&self, pty_id: &str, tab_id: &str) -> Arc<RunState> {
        let state = Arc::new(RunState {
            pty_id: pty_id.to_string(),
            tab_id: tab_id.to_string(),
            kind: AgentRunKind::MemWorker {
                worker_id: format!("worker-{pty_id}"),
            },
            scanner: Mutex::new(ResultEventScanner::new()),
            started_at: Instant::now(),
            last_byte_at: Mutex::new(Instant::now()),
            completed: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            saw_working: AtomicBool::new(false),
            saw_post_session_progress: AtomicBool::new(false),
            nudge_count: AtomicU32::new(0),
            restart_count: AtomicU32::new(0),
            last_restart_at: Mutex::new(Instant::now()),
            resume_system_prompt: None,
        });
        self.runs.lock().insert(pty_id.to_string(), state.clone());
        state
    }

    pub fn feed(&self, svc: &ServiceContext, pty_id: &str, bytes: &[u8]) {
        let state = match self.runs.lock().get(pty_id).cloned() {
            Some(s) => s,
            None => return,
        };
        if state.completed.load(Ordering::Acquire) {
            return;
        }
        *state.last_byte_at.lock() = Instant::now();
        let chunk = String::from_utf8_lossy(bytes);
        let signal = state.scanner.lock().feed(&chunk);
        if let Some(signal) = signal {
            if !state.completed.swap(true, Ordering::AcqRel) {
                let summary = signal
                    .result
                    .clone()
                    .unwrap_or_else(|| "(claude completed)".to_string());
                let success = !signal.is_error;
                log::info!(
                    "[run-watcher] completion signal: tab={} success={} subtype={:?} summary_len={}",
                    state.tab_id, success, signal.subtype, summary.len()
                );
                finish_run(svc, &state, success, summary);
                self.runs.lock().remove(&state.pty_id);
            }
        }
    }
}

async fn watchdog_loop(svc: ServiceContext, state: Arc<RunState>, timeout_secs: u64) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        if state.completed.load(Ordering::Acquire) {
            return;
        }
        let elapsed_total = state.started_at.elapsed().as_secs();
        let idle_secs = state.last_byte_at.lock().elapsed().as_secs();

        // Liveness: agent dropped to a bare shell (crashed / exited) before completing → resume it, bounded. Skip during launch grace + just after a restart.
        if elapsed_total >= RESTART_GRACE_SECS
            && state.last_restart_at.lock().elapsed().as_secs() >= RESTART_SETTLE_SECS
            && agent_dropped_to_shell(&svc, &state.pty_id)
        {
            let restarts = state.restart_count.load(Ordering::Acquire);
            if restarts >= MAX_RESTARTS {
                if state.completed.swap(true, Ordering::AcqRel) {
                    return;
                }
                let raw = svc
                    .output_buf
                    .lock()
                    .get_full_text(&state.pty_id)
                    .unwrap_or_default();
                crate::service::launch_diagnostics::log_shell_drop(
                    &state.tab_id,
                    &state.pty_id,
                    &raw,
                    "restarts_exhausted",
                    restarts,
                    MAX_RESTARTS,
                );
                log::warn!(
                    "[run-watcher] agent died (shell) and exhausted {} restarts: tab={}",
                    MAX_RESTARTS,
                    state.tab_id
                );
                finish_run(
                    &svc,
                    &state,
                    false,
                    format!("agent exited to shell; {} restarts exhausted", MAX_RESTARTS),
                );
                svc.run_watcher.runs.lock().remove(&state.pty_id);
                return;
            }
            state.restart_count.fetch_add(1, Ordering::AcqRel);
            *state.last_restart_at.lock() = Instant::now();
            *state.last_byte_at.lock() = Instant::now();
            state.nudge_count.store(0, Ordering::Release);
            let raw = svc
                .output_buf
                .lock()
                .get_full_text(&state.pty_id)
                .unwrap_or_default();
            crate::service::launch_diagnostics::log_shell_drop(
                &state.tab_id,
                &state.pty_id,
                &raw,
                "restart",
                restarts + 1,
                MAX_RESTARTS,
            );
            log::warn!(
                "[run-watcher] agent dropped to shell — resuming ({}/{}): tab={}",
                restarts + 1,
                MAX_RESTARTS,
                state.tab_id
            );
            resume_hidden_run(
                &svc,
                &state.tab_id,
                &state.pty_id,
                state.resume_system_prompt.as_deref(),
            );
            continue;
        }

        // Nudge: when idle past the threshold, send Enter to clear any prompt auto_approve didn't recognize. Bounded so a finished run still reaches idle-complete. Never nudge an agent that's visibly working.
        if idle_secs >= NUDGE_AFTER_IDLE_SECS
            && state.nudge_count.load(Ordering::Acquire) < MAX_NUDGES
            && !agent_screen_busy(&svc, &state.pty_id)
        {
            state.nudge_count.fetch_add(1, Ordering::AcqRel);
            log::debug!(
                "[run-watcher] nudge Enter (idle {}s) tab={}",
                idle_secs,
                state.tab_id
            );
            let _ = svc.pty.lock().write(&state.pty_id, b"\r");
        }

        if idle_secs >= IDLE_COMPLETE_SECS {
            let session_bound = tab_completion_state(&svc, &state.tab_id).1;
            let ready_for_success = can_complete_on_semantic_idle(
                state.armed.load(Ordering::Acquire),
                state.saw_working.load(Ordering::Acquire),
                state.saw_post_session_progress.load(Ordering::Acquire),
                idle_status_is_terminal_success(&svc, &state.pty_id),
                session_bound,
            );
            let busy = !ready_for_success || agent_screen_busy(&svc, &state.pty_id);
            if busy {
                log::debug!(
                    "[run-watcher] idle {}s but agent remains active — deferring reap: tab={}",
                    idle_secs,
                    state.tab_id
                );
            } else {
                if state.completed.swap(true, Ordering::AcqRel) {
                    return;
                }
                log::info!(
                    "[run-watcher] idle complete: tab={} idle={}s",
                    state.tab_id,
                    idle_secs
                );
                finish_run(
                    &svc,
                    &state,
                    ready_for_success,
                    format!("idle for {}s without an explicit completion", idle_secs),
                );
                svc.run_watcher.runs.lock().remove(&state.pty_id);
                return;
            }
        }

        if elapsed_total >= timeout_secs {
            if state.completed.swap(true, Ordering::AcqRel) {
                return;
            }
            log::warn!(
                "[run-watcher] hard timeout: tab={} elapsed={}s",
                state.tab_id,
                elapsed_total
            );
            finish_run(
                &svc,
                &state,
                false,
                format!("hard timeout after {}s", elapsed_total),
            );
            svc.run_watcher.runs.lock().remove(&state.pty_id);
            return;
        }
    }
}

/// True when the tab's screen shows a bare shell prompt and none of the agent's
/// own UI markers — i.e. the agent process exited and we're back at the shell.
fn agent_dropped_to_shell(svc: &ServiceContext, pty_id: &str) -> bool {
    let Some(text) = svc.screen_state.lock().get_screen_text(pty_id) else {
        return false;
    };
    screen_is_bare_shell(&text)
}

/// A semantic Idle verdict is terminal; otherwise a live TUI remains active
/// unless its screen is a bare shell.
fn agent_screen_busy(svc: &ServiceContext, pty_id: &str) -> bool {
    if idle_status_is_terminal_success(svc, pty_id) {
        return false;
    }
    let Some(text) = svc.screen_state.lock().get_screen_text(pty_id) else {
        return false;
    };
    screen_shows_working(&text) || !screen_is_bare_shell(&text)
}

fn idle_status_is_terminal_success(svc: &ServiceContext, pty_id: &str) -> bool {
    status_is_idle(svc.agent_tracker.lock().get_status(pty_id))
}

fn status_is_idle(status: crate::agent::status::TabStatus) -> bool {
    status == crate::agent::status::TabStatus::Idle
}

fn tab_completion_state(svc: &ServiceContext, tab_id: &str) -> (bool, bool) {
    let mux = svc.mux.read();
    let Some(tab) = mux.get_tab(tab_id) else {
        return (false, false);
    };
    let idle = tab
        .agent_state
        .as_ref()
        .is_some_and(|state| state.state == codeterm_types::pty::TimelineState::Idle);
    (idle, tab.agent_session_id.is_some())
}

fn can_complete_on_semantic_idle(
    armed: bool,
    saw_working: bool,
    saw_post_session_progress: bool,
    idle: bool,
    session_bound: bool,
) -> bool {
    armed && saw_working && saw_post_session_progress && idle && session_bound
}

pub(crate) fn has_nonempty_assistant_transcript(
    transcript: &crate::agent::chat_types::StructuredChat,
) -> bool {
    transcript.messages.iter().any(|message| {
        message.kind != "user"
            && (!message.content.trim().is_empty()
                || message.tool_name.is_some()
                || message.tool_detail.is_some()
                || message.tool_input.is_some())
    })
}

fn screen_shows_working(text: &str) -> bool {
    text.contains("esc to interrupt")
        || text.contains("tokens)")
        || text.contains("· ↑")
        || text.contains("· ↓")
}

fn screen_is_bare_shell(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let agent_visible = text.contains("Claude Code")
        || text.contains("? for shortcuts")
        || text.contains("esc to interrupt")
        || text.contains("Cogitated for")
        || text.contains("Brewed for");
    if agent_visible {
        return false;
    }
    crate::pty::screen_state::screen_has_shell_prompt(text)
}

/// Re-launch the agent in its existing tab via `--resume <session_id>` so a
/// crashed unattended run continues where it left off. No-op if the session id
/// isn't known yet (let idle / timeout handle that case).
fn resume_hidden_run(
    svc: &ServiceContext,
    tab_id: &str,
    pty_id: &str,
    system_prompt: Option<&str>,
) {
    let (provider, session_id) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id);
        (
            tab.and_then(|p| p.agent_type.clone()),
            tab.and_then(|p| p.agent_session_id.clone()),
        )
    };
    let (Some(provider), Some(sid)) = (provider, session_id) else {
        log::warn!(
            "[run-watcher] cannot resume tab={}: no provider/session",
            tab_id
        );
        return;
    };
    let full_access = match crate::service::permission_policy::configured_permission_bypass(
        &svc.agent_registry,
        &provider,
    ) {
        Ok(authorization) => authorization.is_some(),
        Err(error) => {
            log::error!("[run-watcher] refusing policy downgrade for tab={tab_id}: {error}");
            return;
        }
    };
    if let Some(tab) = svc.mux.write().get_tab_mut(tab_id) {
        tab.auto_approve = full_access;
    }
    let cmd = hidden_recovery_resume_command(
        &svc.agent_registry,
        &provider,
        &sid,
        full_access,
        system_prompt,
    );
    let Some(cmd) = cmd else {
        log::warn!("[run-watcher] provider {} has no resume command", provider);
        return;
    };
    if let Err(e) = svc.send_shell_spawn(pty_id, &cmd) {
        log::error!("[run-watcher] resume send failed tab={}: {}", tab_id, e);
    }
}

fn hidden_recovery_resume_command(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    session_id: &str,
    full_access: bool,
    system_prompt: Option<&str>,
) -> Option<String> {
    registry
        .with_provider(provider, |p| {
            p.build_resume_command_with_context(&crate::agent::provider::AgentResumeParams {
                session_id: session_id.to_string(),
                skip_permissions: full_access,
                system_prompt: system_prompt.map(str::to_string),
                args: Vec::new(),
            })
        })
        .flatten()
}

fn finish_run(svc: &ServiceContext, state: &RunState, success: bool, summary: String) {
    let raw = svc
        .output_buf
        .lock()
        .get_full_text(&state.pty_id)
        .unwrap_or_default();
    let clean = crate::pty::ansi::clean_terminal_output(&raw);

    // Bound to the tab by the SessionStart hook; capture session_id + cwd before close_tab so the run stays renderable in the read-only chat-ui.
    let (session_id, run_cwd) = {
        let mux = svc.mux.write();
        let tab = mux.get_tab(&state.tab_id);
        (
            tab.and_then(|p| p.agent_session_id.clone()),
            tab.and_then(|p| p.cwd.clone()),
        )
    };

    match &state.kind {
        AgentRunKind::MemWorker { worker_id } => {
            if let Ok(path) = crate::mem::workers::save_worker_log(worker_id, &clean) {
                svc.workers
                    .set_log_path(worker_id, path.to_string_lossy().into_owned());
            }
            if let Some(ref sid) = session_id {
                svc.workers
                    .set_run_session(worker_id, sid.clone(), run_cwd.clone());
            }
            let _ = svc.workers.complete(worker_id.clone().as_str(), summary);
            let _ = svc.close_tab_with_reason(&state.tab_id, TabCloseReason::AgentRunEnded);
        }
        AgentRunKind::Cron { job_id, run_id } => {
            if let Some(cron_state) = svc.app_handle.try_state::<crate::CronState>() {
                let log_text = format!(
                    "--- SUMMARY ---\n{}\n\n--- RAW OUTPUT ---\n{}",
                    summary, clean
                );
                let (is_oneshot, fresh_transition) = {
                    let mut store = cron_state.lock();
                    let _ = store.save_log(job_id, run_id, &log_text);
                    if let Some(ref sid) = session_id {
                        store.set_run_session(job_id, run_id, sid, run_cwd.as_deref());
                    }
                    let status = if success {
                        crate::cron::store::CronRunStatus::Success
                    } else {
                        crate::cron::store::CronRunStatus::Failed
                    };
                    let fresh = store
                        .update_run_status(job_id, run_id, status)
                        .unwrap_or(false);
                    store.unregister_tab(&state.tab_id);
                    (
                        store.get(job_id).map(|job| job.oneshot).unwrap_or(false),
                        fresh,
                    )
                };
                if fresh_transition {
                    crate::cron::notify::notify_on_consecutive_failure(svc, &cron_state, job_id);
                }
                let _ = svc.close_tab_with_reason(&state.tab_id, TabCloseReason::AgentRunEnded);
                if is_oneshot {
                    if let Some(cron_wake) = svc.app_handle.try_state::<crate::CronWake>() {
                        log::info!("Oneshot cron auto-deleting: job={}", job_id);
                        if let Err(error) = svc.cron_delete(&cron_state, &cron_wake, job_id) {
                            log::error!("Failed to auto-delete oneshot cron: {}", error);
                        }
                    } else {
                        log::error!("Failed to auto-delete oneshot cron: CronWake unavailable");
                    }
                }
                crate::api::events::emit_everywhere(&svc.app_handle, "cron-run-finished", ());
            }
        }
    }
}

// Track only top-level run completion events (`{"type":"result"}`), not nested `message_stop`.
struct ResultEventScanner {
    buffer: String,
}

const RESULT_NEEDLE: &str = "{\"type\":\"result\"";

impl ResultEventScanner {
    fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    fn feed(&mut self, chunk: &str) -> Option<CompletionSignal> {
        let cleaned = crate::pty::ansi::strip_ansi_codes(chunk);
        self.buffer.push_str(&cleaned);

        let mut search_from = 0;
        let mut found: Option<CompletionSignal> = None;
        let mut consumed = 0usize;

        loop {
            let Some(start_rel) = self.buffer[search_from..].find(RESULT_NEEDLE) else {
                break;
            };
            let start = search_from + start_rel;
            match find_balanced_end(&self.buffer, start) {
                Some(end) => {
                    let candidate = &self.buffer[start..=end];
                    if let Ok(parsed) = serde_json::from_str::<ResultEvent>(candidate) {
                        found = Some(CompletionSignal {
                            is_error: parsed.is_error,
                            subtype: parsed.subtype,
                            result: parsed.result,
                        });
                        consumed = end + 1;
                        break;
                    } else {
                        search_from = start + 1;
                    }
                }
                None => {
                    break;
                }
            }
        }

        if consumed > 0 {
            let mut safe_consume = consumed;
            while safe_consume > 0 && !self.buffer.is_char_boundary(safe_consume) {
                safe_consume -= 1;
            }
            self.buffer.drain(..safe_consume);
        }
        if self.buffer.len() > SCANNER_BUFFER_CAP {
            let mut cut = self.buffer.len() - SCANNER_BUFFER_CAP / 2;
            while cut < self.buffer.len() && !self.buffer.is_char_boundary(cut) {
                cut += 1;
            }
            self.buffer = self.buffer.split_off(cut);
        }
        found
    }
}

fn find_balanced_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut i = start;
    while i < bytes.len() {
        let ch = bytes[i];
        if esc {
            esc = false;
            i += 1;
            continue;
        }
        if in_str {
            if ch == b'\\' {
                esc = true;
            } else if ch == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if ch == b'"' {
            in_str = true;
        } else if ch == b'{' {
            depth += 1;
        } else if ch == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_util::lock_hold::TimedRwLock;

    type TestMux = crate::multiplexer::commands::MuxState;

    fn mux_with_nonce(nonce: Option<&str>) -> (TestMux, String) {
        let (state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let mux: TestMux = Arc::new(TimedRwLock::new("mux-test", state));
        if let Some(nonce) = nonce {
            mux.write()
                .get_tab_mut(&tab_id)
                .unwrap()
                .session_binding_nonce = Some(nonce.into());
        }
        (mux, tab_id)
    }

    fn add_tab_with_nonce(mux: &TestMux, sibling_tab: &str, nonce: &str) -> String {
        let workspace_id = mux.read().find_workspace_for_tab(sibling_tab).unwrap();
        let tab_id = mux.write().create_tab(&workspace_id).unwrap();
        mux.write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = Some(nonce.into());
        tab_id
    }

    #[test]
    fn codex_stop_report_completes_only_the_nonce_matching_tab_run() {
        let (mux, tab_a) = mux_with_nonce(Some("nonce-a"));
        let tab_b = add_tab_with_nonce(&mux, &tab_a, "nonce-b");
        let registry = RunWatcherRegistry::new();
        let run_a = registry.insert_run_for_test("pty-a", &tab_a);
        let run_b = registry.insert_run_for_test("pty-b", &tab_b);

        let claimed = registry
            .claim_stop_report_run(&mux, &tab_a, "nonce-a")
            .expect("nonce-matching stop report claims the tab's run");
        assert_eq!(claimed.tab_id, tab_a);
        assert!(run_a.completed.load(Ordering::Acquire));
        assert!(
            !run_b.completed.load(Ordering::Acquire),
            "another tab's run must stay untouched"
        );
        assert!(
            registry
                .claim_stop_report_run(&mux, &tab_a, "nonce-a")
                .is_none(),
            "a claimed run is complete — a duplicate report claims nothing"
        );
    }

    #[test]
    fn codex_stop_foreign_nonce_is_rejected_and_completes_nothing() {
        let (mux, tab_a) = mux_with_nonce(Some("nonce-a"));
        let tab_b = add_tab_with_nonce(&mux, &tab_a, "nonce-b");
        let registry = RunWatcherRegistry::new();
        let run_a = registry.insert_run_for_test("pty-a", &tab_a);
        let run_b = registry.insert_run_for_test("pty-b", &tab_b);

        // Another tab's nonce, an unknown nonce: neither claims anything.
        assert!(registry
            .claim_stop_report_run(&mux, &tab_a, "nonce-b")
            .is_none());
        assert!(registry
            .claim_stop_report_run(&mux, &tab_a, "nonce-unknown")
            .is_none());
        assert!(!run_a.completed.load(Ordering::Acquire));
        assert!(!run_b.completed.load(Ordering::Acquire));

        // A tab that never armed a nonce can't be completed via report at all.
        let (nonceless_mux, nonceless_tab) = mux_with_nonce(None);
        let run = registry.insert_run_for_test("pty-c", &nonceless_tab);
        assert!(registry
            .claim_stop_report_run(&nonceless_mux, &nonceless_tab, "nonce-a")
            .is_none());
        assert!(!run.completed.load(Ordering::Acquire));
    }

    #[test]
    fn codex_stop_hook_channel_loss_leaves_classifier_fallback_completion_intact() {
        // DNC-12: the report path upgrades the signal, it never removes the
        // safety net. A dead hook channel (no report) or a rejected foreign
        // report leaves the run unclaimed, so the classifier-driven paths
        // (semantic idle, idle backstop, scanner) still own completion.
        let (mux, tab_id) = mux_with_nonce(Some("nonce-a"));
        let registry = RunWatcherRegistry::new();
        let run = registry.insert_run_for_test("pty-a", &tab_id);

        assert!(registry
            .claim_stop_report_run(&mux, &tab_id, "nonce-foreign")
            .is_none());
        assert!(
            !run.completed.load(Ordering::Acquire),
            "a rejected report must leave the run claimable by the fallback"
        );
        assert!(
            registry.runs.lock().contains_key("pty-a"),
            "the run stays registered for the classifier-driven watchdog"
        );
        // The classifier-fallback completion gate takes no hook input at all:
        // armed + saw_working + durable progress + classifier idle + bound.
        assert!(can_complete_on_semantic_idle(true, true, true, true, true));
        // Channel restored later: the same run is still completable by report.
        assert!(registry
            .claim_stop_report_run(&mux, &tab_id, "nonce-a")
            .is_some());
    }

    #[test]
    fn screen_working_defers_idle_reap() {
        // The real frozen-spinner status line that used to get reaped mid-work.
        assert!(screen_shows_working(
            "✢ Propagating… (3m 18s · ↑ 9.8k tokens)\n❯ "
        ));
        assert!(screen_shows_working("✻ Schlepping… (12s · ↓ 2.1k tokens)"));
        assert!(screen_shows_working("working… esc to interrupt"));
    }

    #[test]
    fn screen_working_false_on_idle_prompt() {
        assert!(!screen_shows_working(
            "╭─ Claude ─╮\n│ > │\n╰───╯  ? for shortcuts"
        ));
        assert!(!screen_shows_working("user@host:~$ "));
    }

    #[test]
    fn bare_shell_detected_when_agent_gone() {
        assert!(screen_is_bare_shell("user@host:~/proj$ "));
        assert!(screen_is_bare_shell("PS C:\\Users\\foo> "));
    }

    #[test]
    fn bare_shell_false_when_agent_visible() {
        assert!(!screen_is_bare_shell(
            "Welcome to Claude Code\nuser@host:~$ "
        ));
        assert!(!screen_is_bare_shell("thinking…  esc to interrupt"));
    }

    #[test]
    fn bare_shell_false_on_empty_or_non_prompt() {
        assert!(!screen_is_bare_shell(""));
        assert!(!screen_is_bare_shell("   \n  "));
        assert!(!screen_is_bare_shell("still working on it\nno prompt here"));
    }

    #[test]
    fn scanner_finds_result_event_inline() {
        let mut s = ResultEventScanner::new();
        let chunk = r#"some noise {"type":"system","x":1} more {"type":"result","subtype":"success","is_error":false,"result":"hello"}"#;
        let r = s.feed(chunk).expect("completion");
        assert_eq!(r.subtype.as_deref(), Some("success"));
        assert_eq!(r.is_error, false);
        assert_eq!(r.result.as_deref(), Some("hello"));
    }

    #[test]
    fn scanner_ignores_mid_stream_message_stop() {
        // kills the tab before claude can process tool results.
        let mut s = ResultEventScanner::new();
        let chunk = r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"x"}}}
{"type":"stream_event","event":{"type":"message_stop"},"session_id":"a","uuid":"b"}"#;
        assert!(
            s.feed(chunk).is_none(),
            "must not complete on nested message_stop"
        );
    }

    #[test]
    fn scanner_handles_split_across_chunks() {
        let mut s = ResultEventScanner::new();
        assert!(s.feed(r#"{"type":"result","is_error":false,"#).is_none());
        let r = s.feed(r#""result":"привет"}"#).expect("completion");
        assert_eq!(r.result.as_deref(), Some("привет"));
    }

    #[test]
    fn scanner_strips_ansi() {
        let mut s = ResultEventScanner::new();
        let chunk =
            "\x1b[32m\x1b]0;title\x07{\"type\":\"result\",\"is_error\":true,\"result\":\"err\"}";
        let r = s.feed(chunk).expect("completion");
        assert!(r.is_error);
        assert_eq!(r.result.as_deref(), Some("err"));
    }

    #[test]
    fn scanner_ignores_non_completion_events() {
        let mut s = ResultEventScanner::new();
        assert!(s
            .feed(r#"{"type":"assistant","message":{"id":"x"}}"#)
            .is_none());
        assert!(s
            .feed(r#"{"type":"stream_event","event":{"type":"content_block_delta"}}"#)
            .is_none());
    }

    #[test]
    fn scanner_handles_braces_inside_strings() {
        let mut s = ResultEventScanner::new();
        let chunk =
            r#"{"type":"result","is_error":false,"result":"text with {nested} {{braces}} ok"}"#;
        let r = s.feed(chunk).expect("completion");
        assert_eq!(
            r.result.as_deref(),
            Some("text with {nested} {{braces}} ok")
        );
    }

    #[test]
    fn scanner_caps_buffer_growth() {
        let mut s = ResultEventScanner::new();
        let big = "x".repeat(SCANNER_BUFFER_CAP * 2);
        let _ = s.feed(&big);
        assert!(s.buffer.len() <= SCANNER_BUFFER_CAP);
    }

    #[test]
    fn scanner_caps_unicode_buffer_without_panic() {
        let mut s = ResultEventScanner::new();
        let big = "✶✳✻… ".repeat(SCANNER_BUFFER_CAP); // far over cap, all multibyte
        let _ = s.feed(&big);
        assert!(s.buffer.len() <= SCANNER_BUFFER_CAP);
        // Feeding more must keep working (no panic on the retained multibyte tail).
        let _ = s.feed("✢✽ more ✶");
        assert!(s.buffer.is_char_boundary(0));
    }

    #[test]
    fn idle_status_is_terminal_for_any_provider() {
        assert!(status_is_idle(crate::agent::status::TabStatus::Idle));
        assert!(!status_is_idle(crate::agent::status::TabStatus::Working));
        assert!(!status_is_idle(crate::agent::status::TabStatus::Waiting));
    }

    #[test]
    fn semantic_idle_requires_post_session_progress() {
        assert!(!can_complete_on_semantic_idle(
            false, false, false, true, true
        ));
        assert!(!can_complete_on_semantic_idle(
            true, false, false, true, true
        ));
        assert!(!can_complete_on_semantic_idle(
            true, true, false, true, true
        ));
        assert!(!can_complete_on_semantic_idle(
            true, true, true, false, true
        ));
        assert!(!can_complete_on_semantic_idle(
            true, true, true, true, false
        ));
        assert!(can_complete_on_semantic_idle(true, true, true, true, true));
    }

    #[test]
    fn empty_assistant_metadata_is_not_durable_progress() {
        let empty = crate::agent::chat_types::StructuredChat {
            messages: vec![crate::agent::chat_types::ChatMsg {
                kind: "assistant".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!has_nonempty_assistant_transcript(&empty));

        let completed = crate::agent::chat_types::StructuredChat {
            messages: vec![crate::agent::chat_types::ChatMsg {
                kind: "assistant".into(),
                content: "done".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(has_nonempty_assistant_transcript(&completed));
    }

    #[test]
    fn hidden_agent_crash_recovery_uses_global_access_choice() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

        let full_access =
            hidden_recovery_resume_command(&registry, "claude", "session-1", true, None)
                .expect("resume command");
        let restricted =
            hidden_recovery_resume_command(&registry, "claude", "session-1", false, None)
                .expect("resume command");

        assert!(full_access.contains("--dangerously-skip-permissions"));
        assert!(!restricted.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn hidden_agent_crash_recovery_reapplies_the_registered_launch_context() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        let context = "opaque-relaunch-context-3d9c";

        let command =
            hidden_recovery_resume_command(&registry, "claude", "session-1", false, Some(context))
                .expect("resume command with context");
        let path = command
            .split("--append-system-prompt-file ")
            .nth(1)
            .expect("system prompt file flag")
            .trim()
            .trim_matches('\'');
        let written = std::fs::read_to_string(path).expect("written relaunch context");

        assert_eq!(written.trim_start_matches('\u{feff}'), context);
    }
}
