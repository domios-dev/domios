use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::multiplexer::types::TabCloseReason;
use crate::service::ServiceContext;

mod cron_end;

// Default idle wait is 30s (Claude can think between events while still streaming partials).
// `memory.worker_timeout_seconds` can lower/higher this at runtime.
const IDLE_COMPLETE_SECS: u64 = 30;
const SCANNER_BUFFER_CAP: usize = 64 * 1024;

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

    pub fn fail_cron_on_process_exit(
        &self,
        svc: &ServiceContext,
        pty_id: &str,
        reason: String,
    ) -> bool {
        let state = self.runs.lock().get(pty_id).cloned();
        let Some(state) = state.filter(|state| matches!(&state.kind, AgentRunKind::Cron { .. }))
        else {
            return false;
        };
        let finish = !state.completed.swap(true, Ordering::AcqRel);
        self.runs.lock().remove(pty_id);
        if finish {
            log::warn!(
                "[run-watcher] cron agent exited: tab={} reason={}",
                state.tab_id,
                reason
            );
            finish_run(svc, &state, false, reason);
        }
        true
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
        let observation = matches!(&state.kind, AgentRunKind::Cron { .. })
            .then(|| svc.observe_provider_process_tree(&state.tab_id, &state.pty_id))
            .flatten();
        let provider_exit = cron_provider_exit_is_proven(&state.kind, observation.as_ref());
        if provider_exit {
            svc.run_watcher.fail_cron_on_process_exit(
                &svc,
                &state.pty_id,
                "provider process exited from its tab process tree".into(),
            );
            return;
        }
        let elapsed_total = state.started_at.elapsed().as_secs();
        let idle_secs = state.last_byte_at.lock().elapsed().as_secs();
        let provider_prompt_pending = idle_secs >= IDLE_COMPLETE_SECS
            && surface_idle_provider_prompt(&svc, &state.tab_id, &state.pty_id);

        // Liveness: agent dropped to a bare shell (crashed / exited) before completing → resume it, bounded. Skip during launch grace + just after a restart.
        if elapsed_total >= RESTART_GRACE_SECS
            && state.last_restart_at.lock().elapsed().as_secs() >= RESTART_SETTLE_SECS
            && !provider_prompt_pending
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

        if idle_secs >= IDLE_COMPLETE_SECS {
            let session_bound = tab_completion_state(&svc, &state.tab_id).1;
            let ready_for_success = can_complete_on_semantic_idle(
                state.armed.load(Ordering::Acquire),
                state.saw_working.load(Ordering::Acquire),
                state.saw_post_session_progress.load(Ordering::Acquire),
                idle_status_is_terminal_success(&svc, &state.pty_id),
                session_bound,
            );
            let busy = provider_prompt_pending
                || !ready_for_success
                || agent_screen_busy(&svc, &state.pty_id);
            if !busy {
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
            let (svc_end, state_end) = (svc.clone(), state.clone());
            let _ = tauri::async_runtime::spawn_blocking(move || {
                end_on_hard_timeout(&svc_end, &state_end, elapsed_total)
            })
            .await;
            return;
        }
    }
}

fn cron_provider_exit_is_proven(
    kind: &AgentRunKind,
    observation: Option<&crate::service::process_tree::ProviderTreeObservation>,
) -> bool {
    matches!(kind, AgentRunKind::Cron { .. })
        && observation.is_some_and(|observation| observation.proves_exit_from_lifecycle_clear())
}

fn surface_idle_provider_prompt(svc: &ServiceContext, tab_id: &str, pty_id: &str) -> bool {
    let (agent_type, pending) = {
        let mux = svc.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return false;
        };
        (tab.agent_type.clone(), tab.pending_prompt.is_some())
    };
    if pending {
        return true;
    }
    let Some(agent_type) = agent_type else {
        return false;
    };
    let Some(screen) = svc.screen_state.lock().get_screen_text(pty_id) else {
        return false;
    };
    let parsed = crate::agent::prompt::parse_prompt_safely(|| {
        svc.agent_registry
            .parse_interactive_prompt(&agent_type, &screen)
    });
    let Some((prompt, kind)) = parsed.filter(|(prompt, _)| {
        !crate::agent::state_reactor::prompt_is_composer_text(&screen, prompt)
    }) else {
        return false;
    };
    if !crate::agent::state_reactor::has_structural_prompt_evidence(&screen, &prompt) {
        return false;
    }
    let decision = svc.prompt_detector.lock().evaluate_pty_output_parsed(
        tab_id,
        true,
        true,
        Some(crate::agent::native_prompt::ResolvedPrompt {
            prompt,
            explicit: false,
        }),
    );
    svc.apply_prompt_decision_with_kind(&decision, Some(&screen), kind, true);
    true
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
    crate::service::task_files::release_tab(svc, tab_id);
    let cmd = crate::service::task_files::capture(svc, tab_id, || {
        hidden_recovery_resume_command(
            &svc.agent_registry,
            &provider,
            &sid,
            full_access,
            system_prompt,
        )
    });
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
                agent_name: None,
                system_prompt: system_prompt.map(str::to_string),
                instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
                args: Vec::new(),
            })
        })
        .flatten()
}

/// The hard deadline ended this run: record it as timed out, then let the
/// host finalize a dreaming run's diary, since the agent never will.
fn end_on_hard_timeout(svc: &ServiceContext, state: &RunState, elapsed_secs: u64) {
    log::warn!(
        "[run-watcher] hard timeout: tab={} elapsed={}s",
        state.tab_id,
        elapsed_secs
    );
    let summary = format!("hard timeout after {}s", elapsed_secs);
    finish_run_as(svc, state, RunEnd::TimedOut, summary.clone());
    if let AgentRunKind::MemWorker { worker_id } = &state.kind {
        crate::mem::dream_runs::finalize_dream_at_deadline(svc, worker_id, &summary);
    }
    svc.run_watcher.runs.lock().remove(&state.pty_id);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunEnd {
    Success,
    Failure,
    TimedOut,
}

fn finish_run(svc: &ServiceContext, state: &RunState, success: bool, summary: String) {
    let end = if success {
        RunEnd::Success
    } else {
        RunEnd::Failure
    };
    finish_run_as(svc, state, end, summary);
}

fn finish_run_as(svc: &ServiceContext, state: &RunState, run_end: RunEnd, summary: String) {
    let success = run_end == RunEnd::Success;
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
            // Execution ending is not the same fact as the run finishing its
            // work: a process that died or timed out never reports `done`.
            let end = match run_end {
                RunEnd::Success => codeterm_mem::dream_status::ExecutionEnd::Reported,
                RunEnd::TimedOut => codeterm_mem::dream_status::ExecutionEnd::TimedOut {
                    detail: summary.clone(),
                },
                RunEnd::Failure => codeterm_mem::dream_status::ExecutionEnd::ProcessEnded {
                    detail: summary.clone(),
                },
            };
            let _ = svc
                .workers
                .finish_execution(worker_id.clone().as_str(), Some(summary), end);
            let _ = svc.close_tab_with_reason(&state.tab_id, TabCloseReason::AgentRunEnded);
        }
        AgentRunKind::Cron { job_id, run_id } => {
            cron_end::finish(
                svc,
                cron_end::EndedCronRun {
                    tab_id: &state.tab_id,
                    job_id,
                    run_id,
                    success,
                    summary,
                    raw_output: &clean,
                    session_id: session_id.as_deref(),
                    cwd: run_cwd.as_deref(),
                },
            );
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
#[path = "run_watcher/tests.rs"]
mod tests;
