//! PTY output processing — status tracking and agent event detection.
//!
//! Extracted from `commands::read_pty` to keep that function focused on I/O.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use std::time::Instant;

use super::ServiceContext;
use crate::agent::classify_input::{ClassifyParts, StatusBarRows};
use crate::agent::detector::AgentEvent;
use crate::agent::prompt::PromptDecision;
use crate::agent::status::TabStatus;

/// Per-tab notification cooldown: (tab_id, event_kind) → last fire time.
/// Prevents notification spam from TUI redraws (same event every chunk).
static EVENT_COOLDOWN: LazyLock<StdMutex<HashMap<String, Instant>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

/// Per-tab throttle for hookless, file-backed session-id detection.
/// Structured sessions require authoritative provider binding.
static SESSION_DETECT_COOLDOWN: LazyLock<StdMutex<HashMap<String, Instant>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

static AGENT_STOP_SUPPRESS_UNTIL: LazyLock<StdMutex<HashMap<String, Instant>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

const AGENT_STOP_SUPPRESS_MS: u64 = 1_500;

impl ServiceContext {
    pub(crate) fn mark_agent_interrupted(&self, pty_id: &str) {
        AGENT_STOP_SUPPRESS_UNTIL.lock().unwrap().insert(
            pty_id.to_string(),
            Instant::now() + Duration::from_millis(AGENT_STOP_SUPPRESS_MS),
        );
        self.force_agent_idle_for_pty(pty_id);
    }

    fn is_agent_stop_suppressed(pty_id: &str) -> bool {
        let now = Instant::now();
        let mut suppressed = AGENT_STOP_SUPPRESS_UNTIL.lock().unwrap();
        match suppressed.get(pty_id).copied() {
            Some(until) if until > now => true,
            Some(_) => {
                suppressed.remove(pty_id);
                false
            }
            None => false,
        }
    }

    fn force_agent_idle_for_pty(&self, pty_id: &str) {
        let tab_id = self.mux.read().tab_id_for_pty(pty_id);
        if let Some(tab_id) = tab_id {
            self.force_agent_idle_for_tab(&tab_id);
        }
    }

    fn force_agent_idle_for_tab(&self, tab_id: &str) {
        use crate::agent::offline_state_classifier::{AgentLiveState, TabVerdict, TimelineState};

        let idle_state: AgentLiveState = TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into();
        let changed = {
            let mut mux = self.mux.write();
            match mux.get_tab_mut(tab_id) {
                Some(tab) if tab.agent_type.is_some() => {
                    let changed = tab.agent_state.as_ref() != Some(&idle_state)
                        || tab.pending_prompt.is_some();
                    tab.agent_state = Some(idle_state);
                    tab.pending_prompt = None;
                    changed
                }
                _ => false,
            }
        };
        if changed {
            self.emit_state_changed();
            self.schedule_planner_replay(&tab_id);
            self.heal_recovery_state_if_ready();
        }
    }

    /// Process new PTY output: track status, detect agent events, dispatch notifications.
    /// Returns the previous and current status for the caller to emit status-change events.
    pub fn process_pty_output(&self, pty_id: &str, data: &[u8]) -> (TabStatus, TabStatus) {
        // Dev capture tap — single relaxed atomic load when idle, no lock held here.
        crate::pty::capture::CaptureRegistry::global().tap_output(pty_id, data);

        static CALL_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n % 500 == 0 {
            log::info!(
                "[output] process_pty_output #{}, pty={}, len={}",
                n,
                &pty_id[..pty_id.len().min(8)],
                data.len()
            );
        }

        // Single read lock captures all tab data needed for this chunk.
        // Concurrent PTY handlers for different tabs can read in parallel.
        let (
            tab_id,
            agent_type,
            has_active_prompt,
            is_auto_approve,
            shell_nonce,
            tab_cwd,
            agent_session_id,
        ) = {
            let mux = self.mux.read();
            let pid = mux.tab_id_for_pty(pty_id);
            let tab = pid.as_ref().and_then(|id| mux.get_tab(id));
            let at = tab.and_then(|p| p.agent_type.clone());
            let active_prompt = tab.and_then(|p| p.pending_prompt.as_ref()).is_some();
            let auto_approve = tab.map(|p| p.auto_approve).unwrap_or(false);
            let nonce = tab.and_then(|p| p.shell_integration_nonce.clone());
            let cwd = tab.and_then(|p| p.cwd.clone());
            let sid = tab.and_then(|p| p.agent_session_id.clone());
            (pid, at, active_prompt, auto_approve, nonce, cwd, sid)
        };

        self.process_shell_markers(pty_id, tab_id.as_deref(), shell_nonce, data);

        // An unresolvable provider has UNKNOWN chrome, so refuse to hash
        // rather than measure the volatile band against a fictional zero.
        let status_bar_rows = match agent_type.as_deref() {
            None => Ok(StatusBarRows::NO_STATUS_BAR),
            Some(at) => StatusBarRows::resolve(&*self.agent_registry, at),
        };
        let (prev_status, screen_changed) = match &status_bar_rows {
            Err(err) => {
                log::warn!(
                    "[output] {}: {err}; screen-change tracking refused",
                    &pty_id[..pty_id.len().min(8)]
                );
                let prev = self.agent_tracker.lock().get_status(pty_id);
                (prev, false)
            }
            Ok(rows) => {
                let screen_hash = self
                    .screen_state
                    .lock()
                    .get_content_hash(pty_id, rows.get())
                    .unwrap_or([0u8; 32]);
                let mut t = self.agent_tracker.lock();
                let prev = t.get_status(pty_id);
                let changed = t.update_output(pty_id, screen_hash);
                (prev, changed)
            }
        };
        if screen_changed {
            if let Some(ref pid) = tab_id {
                self.usage_activity.mark(pid);
            }
        }
        let current_status = self.agent_tracker.lock().get_status(pty_id);
        if current_status != prev_status {
            log::info!(
                "[status] {} {:?} -> {:?}",
                &pty_id[..pty_id.len().min(8)],
                prev_status,
                current_status
            );
        }
        let previous_reconciled_status = tab_id
            .as_deref()
            .map(|pid| self.reconciled_tab_status_for_id(pid, prev_status))
            .unwrap_or(prev_status);

        if agent_type.is_some() && screen_changed {
            let screen = self
                .screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default();
            let has_agent_markers = self.agent_registry.detect_from_output(&screen).is_some();
            if matches!(
                decide_agent_output_lifecycle(&screen, has_agent_markers),
                AgentOutputLifecycleDecision::PreserveIdentityAndLogShellLikeFrame
            ) {
                log::debug!(
                    "[output] shell-looking marker-less frame in Tab {}; lifecycle unchanged",
                    tab_id.as_deref().unwrap_or("unknown"),
                );
            }
        }

        if let (Some(ref pid), Some(ref at)) = (&tab_id, &agent_type) {
            self.maybe_detect_agent_session(pid, at);
            if let Ok(raw) = std::str::from_utf8(data) {
                let chunk_text = crate::pty::ansi::clean_terminal_output(raw);
                let has_markers = self.agent_registry.has_event_markers(at, &chunk_text);

                // auto_approve tabs (GA), always read screen — chunk fragmentation can split markers across reads, so we check full screen every time.
                let check_screen = has_markers || has_active_prompt || is_auto_approve;
                let (native_prompt_input, mut clean_text) = {
                    let screen_state = self.screen_state.lock();
                    (
                        screen_state.native_prompt_input(pty_id),
                        check_screen
                            .then(|| screen_state.get_screen_text(pty_id))
                            .flatten(),
                    )
                };
                let structured = if check_screen {
                    self.structured_pending_prompt(
                        pid,
                        at,
                        tab_cwd.as_deref(),
                        agent_session_id.as_deref(),
                    )
                } else {
                    None
                };
                // Structured handles (SSE/API) are the only live prompt source;
                // PTY screen input cannot create a pending prompt here.
                let parsed = crate::agent::prompt::parse_prompt_safely(|| {
                    crate::agent::native_prompt::resolve_pending_prompt(
                        structured,
                        native_prompt_input.as_ref(),
                    )
                });
                let prompt_signal = parsed.is_some();
                if prompt_signal && clean_text.is_none() {
                    clean_text = self.screen_state.lock().get_screen_text(pty_id);
                }
                let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
                    pid,
                    check_screen || prompt_signal,
                    clean_text.is_some() || native_prompt_input.is_some(),
                    parsed,
                );

                self.apply_prompt_decision_with_screen(&decision, clean_text.as_deref());

                if has_markers {
                    if let Some(ref clean) = clean_text {
                        let events = self.agent_registry.detect_events(at, clean);
                        for event in &events {
                            if matches!(event, AgentEvent::PermissionRequest) {
                                self.agent_tracker.lock().set_waiting(pty_id);
                            } else {
                                self.handle_agent_event(pty_id, pid, at, event, prev_status);
                            }
                        }
                    }
                }
            }
        }

        let current_status = self.agent_tracker.lock().get_status(pty_id);
        if agent_type.is_some() {
            self.run_watcher
                .observe_output_status(pty_id, current_status);
            self.recompute_agent_state(pty_id);
            self.run_watcher.complete_via_semantic_idle(self, pty_id);
        }

        let current_reconciled_status = tab_id
            .as_deref()
            .map(|pid| self.reconciled_tab_status_for_id(pid, current_status))
            .unwrap_or(current_status);
        (previous_reconciled_status, current_reconciled_status)
    }

    /// Build a pending prompt from the provider's STRUCTURED handle, if it has
    /// one in this run mode. Two handle shapes, both capability-gated (never a
    /// provider-id branch):
    ///
    /// - PUSH (opencode): a background SSE subscriber has already parsed the
    ///   event into the structured-prompt store — an in-memory read, cheap, no
    ///   throttle.
    /// - PULL (claude): read the session JSONL tail for the live AskUserQuestion.
    ///   Bounded + throttled per tab so the file read stays off the per-chunk
    ///   hot path.
    ///
    /// Returns `None` when the provider has no structured handle or no prompt is
    /// active; the caller then falls back to the native PTY detector.
    fn structured_pending_prompt(
        &self,
        tab_id: &str,
        agent_type: &str,
        _cwd: Option<&str>,
        _session_id: Option<&str>,
    ) -> Option<crate::multiplexer::types::PendingPrompt> {
        if !self
            .agent_registry
            .with_provider(agent_type, |p| p.supports_structured_prompts())
            .unwrap_or(false)
        {
            return None;
        }
        crate::service::structured_prompt::pending_for_tab(tab_id)
    }

    /// Recompute and store the tab's live 3-state classification, then emit if
    /// it changed. Output-driven (here) and timer-driven (the live-state poller)
    /// both call this. Reuses the per-tab `ScreenState` parser (no second vt100)
    /// and the already-surfaced `pending_prompt` for the picker question, so the
    /// live path never re-parses the picker. Locks in CLAUDE.md order, each
    /// scoped and dropped before the next; nothing held across the emit.
    pub(crate) fn recompute_agent_state(&self, pty_id: &str) {
        use crate::agent::offline_state_classifier::{
            classify_state, extract_completion_summary, extract_question_message,
            extract_status_line, AgentLiveState, TabVerdict, TimelineState,
        };
        use crate::agent::signal_extractor::Motion;

        // mux (1): the tab's identity + agent gate + picker question + previous.
        let (tab_id, agent_type, prompt_question, previous) = {
            let mux = self.mux.read();
            let Some(tab_id) = mux.tab_id_for_pty(pty_id) else {
                return;
            };
            let Some(tab) = mux.get_tab(&tab_id) else {
                return;
            };
            let Some(agent_type) = tab.agent_type.clone() else {
                return;
            };
            let prompt_question = tab.pending_prompt.as_ref().map(|p| p.question.clone());
            let previous = tab
                .agent_state
                .clone()
                .map(TabVerdict::from)
                .unwrap_or(TabVerdict {
                    state: TimelineState::Idle,
                    subtype: None,
                });
            (tab_id, agent_type, prompt_question, previous)
        };
        if Self::is_agent_stop_suppressed(pty_id) {
            self.force_agent_idle_for_tab(&tab_id);
            return;
        }

        // screen_state (3): trimmed lines + tail from the existing parser.
        const LIVE_TAIL_ROWS: usize = 60;
        let Some((lines, tail, cursor_hidden, composer_ready, cursor_row)) = ({
            let screen_state = self.screen_state.lock();
            let Some((lines, tail)) = screen_state.lines_and_tail(pty_id, LIVE_TAIL_ROWS) else {
                return;
            };
            let cursor_hidden = screen_state
                .native_prompt_input(pty_id)
                .map(|input| input.cursor_hidden)
                .unwrap_or(false);
            let cursor_state = screen_state.cursor_input_state(pty_id);
            let composer_ready = cursor_state
                .map(|(_alt, vis, row, rows)| {
                    crate::agent::offline_state_classifier::composer_cursor_ready(vis, row, rows)
                })
                .unwrap_or(false);
            let cursor_row = cursor_state.map(|(_alt, _vis, row, _rows)| row as usize);
            Some((lines, tail, cursor_hidden, composer_ready, cursor_row))
        }) else {
            return;
        };

        // Ephemeral stream preview: telemetry only, never feeds classification.
        if let Some(event) =
            crate::service::stream_preview::step(pty_id, &tab_id, &lines, cursor_row)
        {
            crate::api::events::emit_everywhere(&self.app_handle, "chat-stream-preview", &event);
        }

        let status = extract_status_line(&lines);

        // agent_tracker (4): two-clock quiescence + the status widget's elapsed
        // baseline, so an advancing counter survives a quiet repaint gap.
        let Some((byte_idle_ms, semantic_idle_ms, status_elapsed)) = ({
            let mut tracker = self.agent_tracker.lock();
            match tracker.quiescence(pty_id, Instant::now()) {
                Some((byte_idle_ms, semantic_idle_ms)) => {
                    let progress = tracker.observe_status_elapsed(
                        pty_id,
                        status.as_ref().and_then(|status| status.elapsed_ms),
                    );
                    Some((byte_idle_ms, semantic_idle_ms, progress))
                }
                None => None,
            }
        }) else {
            return;
        };

        // Resolve first, classify second: an unresolvable provider refuses here
        // and no verdict is produced at all.
        let verdict = match crate::agent::classify_input::with_resolved_rows(
            &*self.agent_registry,
            &agent_type,
            |builder| {
                classify_state(
                    &builder.build(ClassifyParts {
                        picker_question: prompt_question.as_deref(),
                        screen_lines: &lines,
                        tail: &tail,
                        byte_idle_ms,
                        semantic_idle_ms,
                        motion: Motion::Still,
                        cursor_hidden,
                        composer_ready,
                        status_elapsed,
                    }),
                    previous,
                )
            },
        ) {
            Ok(verdict) => verdict,
            Err(err) => {
                log::warn!("[state] {tab_id}: {err}; classification refused");
                return;
            }
        };
        let became_idle =
            previous.state != TimelineState::Idle && verdict.state == TimelineState::Idle;
        let mut new_state: AgentLiveState = verdict.into();
        // Surface the live status/progress line while working (UI shows it verbatim);
        // when idle, surface the completed-turn summary as a muted secondary line.
        match new_state.state {
            TimelineState::Working => {
                if let Some(status) = status {
                    new_state.status_line = Some(status.line);
                    new_state.phase = status.phase;
                    new_state.elapsed_ms = status.elapsed_ms;
                }
            }
            TimelineState::Idle => {
                if let Some(summary) = extract_completion_summary(&lines) {
                    new_state.last_turn_summary = Some(summary.line);
                }
            }
            TimelineState::Question => {
                new_state.question = prompt_question
                    .clone()
                    .or_else(|| extract_question_message(&lines, new_state.subtype));
            }
        }

        // mux (1): store if changed.
        let changed = {
            let mut mux = self.mux.write();
            match mux.get_tab_mut(&tab_id) {
                Some(tab) if tab.agent_state.as_ref() != Some(&new_state) => {
                    tab.agent_state = Some(new_state);
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.emit_state_changed();
            self.schedule_planner_replay(&tab_id);
            if became_idle {
                // The turn boundary, not a status value: the pass itself runs on the
                // ledger thread, never here.
                self.on_agent_turn_settled(&tab_id);
                self.complete_worker_turn_without_report(
                    &tab_id,
                    crate::service::hierarchy::WorkerTurnSignal::IdleTransition,
                );
            }
        }
    }

    /// Feed a raw chunk into the tab's shell-integration accumulators and emit
    /// command-lifecycle changes on the unified frontend event path (same
    /// `emit_everywhere` channel `tab-status-changed` uses). OSC4 renders these.
    fn process_shell_markers(
        &self,
        pty_id: &str,
        tab_id: Option<&str>,
        nonce: Option<String>,
        data: &[u8],
    ) {
        let events = self.shell_caps.lock().feed(pty_id, nonce, data);
        if events.is_empty() {
            return;
        }
        // nothing to surface to a chat view yet.
        let Some(tab_id) = tab_id else {
            return;
        };
        for event in &events {
            if let crate::agent::command_detection::CommandLifecycle::Finished {
                exit_code, ..
            } = event
            {
                self.observe_agent_startup_launch_command_finished(tab_id, pty_id, *exit_code);
                // Claimed before the teardown clears the launch it names, published
                // after so the teardown still sees the boot stage it decides on.
                let exited = self.claim_launch_exit(tab_id, Some(pty_id), *exit_code);
                // The shell finished its foreground command: if that command was
                // a proven-up agent, the agent process is gone.
                self.note_provider_exit_in_surviving_shell(tab_id);
                if let Some(exited) = exited {
                    self.publish_launch_exit(&exited);
                }
            }
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "shell-command-changed",
                serde_json::json!({ "tab_id": tab_id, "event": event }),
            );
        }
    }

    /// Heuristic detection is limited to hookless, file-backed providers. A
    /// hooked provider that missed its launch deadline reads its own launch record.
    fn maybe_detect_agent_session(&self, tab_id: &str, agent_type: &str) {
        let (cwd, set_at_elapsed, current_session, deadline_elapsed) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            let cwd = tab.cwd.clone().unwrap_or_default();
            let elapsed = tab.agent_set_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            let sid = tab.agent_session_id.clone();
            let deadline_elapsed = tab.boot_stage
                == Some(codeterm_types::pty::BootStage::LaunchDeadlineElapsed)
                || tab.agent_set_at.is_some_and(|set_at| {
                    set_at.elapsed() >= crate::service::worker_ack::STARTUP_BINDING_DEADLINE
                });
            (cwd, elapsed, sid, deadline_elapsed)
        };
        if !should_detect_unbound_session(current_session.as_deref()) {
            return;
        }
        let (heuristic, hooked) = self
            .agent_registry
            .with_provider(agent_type, |p| {
                let inj = matches!(
                    p.session_id_injection(),
                    crate::agent::provider::SessionIdInjection::None
                );
                (
                    should_heuristically_detect_session(
                        inj,
                        p.has_hooks(),
                        p.uses_structured_chat(),
                    ),
                    p.has_hooks(),
                )
            })
            .unwrap_or((false, false));
        let launch_detection = !heuristic && hooked && deadline_elapsed;
        if !heuristic && !launch_detection {
            return;
        }
        if !launch_detection && cwd.is_empty() {
            return;
        }
        // Throttle: at most once per second per tab.
        {
            let mut cd = SESSION_DETECT_COOLDOWN.lock().unwrap();
            let now = Instant::now();
            if let Some(last) = cd.get(tab_id) {
                if now.duration_since(*last) < std::time::Duration::from_secs(1) {
                    return;
                }
            }
            cd.insert(tab_id.to_string(), now);
        }
        if launch_detection {
            self.detect_unbound_launch_session(tab_id);
            return;
        }

        let exclude = self.mux.read().other_session_ids(tab_id);
        let max_age = std::time::Duration::from_secs(set_at_elapsed + 30);
        let found = self
            .agent_registry
            .with_provider(agent_type, |p| {
                p.detect_session_id(&cwd, &exclude, Some(max_age))
            })
            .flatten();
        if let Some(sid) = found {
            log::info!(
                "[output] post-launch detected {} session {} for Tab {}",
                agent_type,
                sid,
                tab_id
            );
            self.set_agent_session_id(tab_id, Some(sid));
            self.emit_state_changed();
        }
    }

    pub(crate) fn apply_prompt_decision(&self, decision: &PromptDecision) {
        self.apply_prompt_decision_with_screen(decision, None);
    }

    /// Set (or clear) a tab's pending prompt from a PUSH structured handle
    /// (opencode SSE). No-op when unchanged. Bypasses the auto-approve PTY
    /// send-back in `apply_prompt_decision` — structured prompts are answered
    /// over HTTP, never by typing into the PTY.
    pub(crate) fn push_structured_prompt(
        &self,
        tab_id: &str,
        prompt: Option<crate::multiplexer::types::PendingPrompt>,
    ) {
        {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            if tab.pending_prompt == prompt {
                return;
            }
            tab.pending_prompt = prompt.clone();
        }
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "prompt-state-changed",
            serde_json::json!({ "tab_id": tab_id, "prompt": prompt }),
        );
        self.emit_state_changed();
    }

    pub(crate) fn apply_prompt_decision_with_screen(
        &self,
        decision: &PromptDecision,
        screen_text: Option<&str>,
    ) {
        match decision {
            PromptDecision::None => {}
            PromptDecision::NewPrompt { tab_id, prompt }
            | PromptDecision::PromptUpdated { tab_id, prompt } => {
                let (auto_approve, agent_type, pty_id, is_general_tab) = {
                    let mux = self.mux.read();
                    let is_general_tab = mux
                        .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
                        .as_deref()
                        == Some(tab_id.as_str());
                    let tab = mux.get_tab(tab_id);
                    (
                        tab.map(|p| p.auto_approve).unwrap_or(false),
                        tab.and_then(|p| p.agent_type.clone()),
                        tab.and_then(|p| p.pty_id.clone()),
                        is_general_tab,
                    )
                };

                if auto_approve {
                    let directory_trust = agent_type.as_deref().and_then(|provider| {
                        self.agent_registry
                            .with_provider(provider, |provider| {
                                provider.directory_trust_prompt_response(
                                    prompt,
                                    screen_text.unwrap_or(""),
                                )
                            })
                            .flatten()
                    });
                    if let Some(response) = directory_trust {
                        let provenance = {
                            let mux = self.mux.read();
                            mux.get_tab(tab_id)
                                .map(crate::service::directory_trust::directory_trust_provenance)
                        };
                        match provenance {
                            Some(Ok(())) => {
                                self.prompt_detector.lock().mark_responded(tab_id);
                                if let Some(pty_id) = pty_id {
                                    write_prompt_steps(self.pty.clone(), pty_id, response.steps);
                                }
                            }
                            Some(Err(refusal)) => log::info!(
                                "[directory-trust] Left directory trust prompt open for Tab {}: {}",
                                tab_id,
                                refusal.reason()
                            ),
                            None => {}
                        }
                        return;
                    }
                    if is_general_tab {
                        if let Some(ack) = crate::service::general_agent::blocker::classify(
                            prompt,
                            screen_text.unwrap_or(""),
                        ) {
                            log::info!(
                                "[general-agent/auto-ack] Auto-acknowledging {} for Tab {}",
                                ack.kind.label(),
                                tab_id
                            );
                            self.create_notification(
                                "General Agent auto-acknowledged a blocker".to_string(),
                                Some(format!(
                                    "Main selected option {} for {}.",
                                    ack.option_index + 1,
                                    ack.kind.label()
                                )),
                                Some("warning"),
                            );
                            self.prompt_detector.lock().mark_responded(tab_id);
                            if let Some(pty_id) = pty_id {
                                let steps = agent_type
                                    .as_deref()
                                    .and_then(|at| {
                                        self.agent_registry.build_prompt_response(
                                            at,
                                            ack.option_index,
                                            screen_text.unwrap_or(""),
                                        )
                                    })
                                    .unwrap_or_else(|| default_prompt_response(ack.option_index));
                                write_prompt_steps(self.pty.clone(), pty_id, steps);
                            }
                            return;
                        }
                    }

                    if let Some(response) = legacy_auto_approve_response(prompt) {
                        log::info!(
                            "[auto-approve] Auto-approving {} prompt for Tab {}",
                            response.kind,
                            tab_id
                        );
                        self.prompt_detector.lock().mark_responded(tab_id);
                        if let Some(pty_id) = pty_id {
                            write_prompt_steps(self.pty.clone(), pty_id, vec![response.bytes]);
                        }
                        return;
                    }

                    log::debug!(
                        "[auto-approve] Skipping non-approvable prompt for Tab {}",
                        tab_id
                    );
                }
                {
                    let mut mux = self.mux.write();
                    if let Some(tab) = mux.get_tab_mut(tab_id) {
                        tab.pending_prompt = Some(prompt.clone());
                    }
                }
                crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "prompt-state-changed",
                    serde_json::json!({
                        "tab_id": tab_id,
                        "prompt": prompt,
                    }),
                );
                self.emit_state_changed();
            }
            PromptDecision::PromptCleared { tab_id } => {
                let mut mux = self.mux.write();
                if let Some(tab) = mux.get_tab_mut(tab_id) {
                    tab.pending_prompt = None;
                }
                drop(mux);
                crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "prompt-state-changed",
                    serde_json::json!({
                        "tab_id": tab_id,
                        "prompt": null,
                    }),
                );
                self.emit_state_changed();
            }
        }
    }

    fn handle_agent_event(
        &self,
        _pty_id: &str,
        tab_id: &str,
        _agent_type: &str,
        event: &AgentEvent,
        prev_status: TabStatus,
    ) {
        let cooldown_key = format!("{}:{:?}", tab_id, event);
        let mut map = EVENT_COOLDOWN.lock().unwrap();
        let now = Instant::now();
        if let Some(last) = map.get(&cooldown_key) {
            if now.duration_since(*last).as_secs() < 30 {
                return;
            }
        }
        map.insert(cooldown_key, now);
        drop(map);

        match event {
            AgentEvent::PermissionRequest => {}
            AgentEvent::TaskComplete => {
                if prev_status == TabStatus::Working {
                    let title = self.mux.read().get_tab(tab_id).map(|p| p.title.clone());
                    if let Some(title) = title {
                        self.notify_event(
                            "agent.task_complete",
                            serde_json::json!({
                                "tab_id": tab_id,
                                "title": format!("{} finished", title),
                                "body": "Task complete",
                            }),
                        );
                    }
                }
            }
        }
    }
}

fn should_detect_unbound_session(current_session: Option<&str>) -> bool {
    current_session.is_none()
}

fn should_heuristically_detect_session(
    has_no_injection: bool,
    has_hooks: bool,
    uses_structured_chat: bool,
) -> bool {
    has_no_injection && !has_hooks && !uses_structured_chat
}

struct LegacyAutoApproveResponse {
    kind: &'static str,
    bytes: Vec<u8>,
}

fn legacy_auto_approve_response(
    prompt: &crate::multiplexer::types::PendingPrompt,
) -> Option<LegacyAutoApproveResponse> {
    let is_yes_no = prompt.options.iter().any(|o| {
        let v = o.value.to_lowercase();
        v == "y" || v == "yes"
    });
    if is_yes_no {
        return Some(LegacyAutoApproveResponse {
            kind: "yes/no",
            bytes: b"y\r".to_vec(),
        });
    }

    let is_numbered_yes = prompt
        .options
        .first()
        .map(|o| o.label.to_lowercase().starts_with("yes"))
        .unwrap_or(false);
    if is_numbered_yes {
        Some(LegacyAutoApproveResponse {
            kind: "numbered-yes",
            bytes: b"\r".to_vec(),
        })
    } else {
        None
    }
}

fn default_prompt_response(option_index: usize) -> Vec<Vec<u8>> {
    let mut steps = Vec::new();
    for _ in 0..option_index {
        steps.push(b"\x1b[B".to_vec());
    }
    steps.push(b"\r".to_vec());
    steps
}

fn write_prompt_steps(pty: crate::PtyState, pty_id: String, steps: Vec<Vec<u8>>) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        for step in steps {
            let _ = pty.lock().write(&pty_id, &step);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

/// Does the screen's last non-empty line look like a bare shell prompt? Combined
/// with "no agent markers", this is what makes a frame suspect the agent exited.
fn screen_ends_in_shell_prompt(screen: &str) -> bool {
    screen
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| {
            let t = l.trim();
            t.ends_with('>') || t.ends_with('$') || t.ends_with('%') || t.ends_with('#')
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentOutputLifecycleDecision {
    PreserveIdentity,
    PreserveIdentityAndLogShellLikeFrame,
}

fn decide_agent_output_lifecycle(
    screen: &str,
    has_agent_markers: bool,
) -> AgentOutputLifecycleDecision {
    if !screen.is_empty() && !has_agent_markers && screen_ends_in_shell_prompt(screen) {
        AgentOutputLifecycleDecision::PreserveIdentityAndLogShellLikeFrame
    } else {
        AgentOutputLifecycleDecision::PreserveIdentity
    }
}

#[cfg(test)]
mod exit_detection_tests {
    use super::{
        decide_agent_output_lifecycle, screen_ends_in_shell_prompt, should_detect_unbound_session,
        should_heuristically_detect_session, AgentOutputLifecycleDecision,
    };

    #[test]
    fn shell_prompt_recognized_only_on_bare_prompt_lines() {
        assert!(screen_ends_in_shell_prompt("user@host ~ %"));
        assert!(screen_ends_in_shell_prompt("C:\\Users\\me>"));
        assert!(screen_ends_in_shell_prompt("$"));
        assert!(screen_ends_in_shell_prompt("output\n# "));
        // A running agent TUI footer is not a shell prompt.
        assert!(!screen_ends_in_shell_prompt(
            "auto mode on (shift+tab to cycle)"
        ));
        assert!(!screen_ends_in_shell_prompt(""));
    }

    #[test]
    fn shell_prompt_without_markers_is_diagnostic_only() {
        assert_eq!(
            decide_agent_output_lifecycle("PS D:\\Developer\\codeterm>", false),
            AgentOutputLifecycleDecision::PreserveIdentityAndLogShellLikeFrame
        );
        assert_eq!(
            decide_agent_output_lifecycle("PS D:\\Developer\\codeterm>", true),
            AgentOutputLifecycleDecision::PreserveIdentity
        );
    }

    #[test]
    fn session_detection_never_replaces_a_bound_session() {
        assert!(should_detect_unbound_session(None));
        assert!(!should_detect_unbound_session(Some("session-owned-by-tab")));
    }

    #[test]
    fn structured_sessions_require_authoritative_provider_binding() {
        assert!(!should_heuristically_detect_session(true, true, true));
        assert!(!should_heuristically_detect_session(true, false, true));
        assert!(should_heuristically_detect_session(true, false, false));
    }
}

#[cfg(test)]
#[path = "output/agent_lifecycle_tests.rs"]
mod agent_lifecycle_tests;
