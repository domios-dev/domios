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
#[path = "output/agent_idle.rs"]
mod agent_idle;
#[path = "output/launch_exit.rs"]
mod launch_exit;
#[path = "output/prompt_steps.rs"]
mod prompt_steps;
#[path = "output/shell_markers.rs"]
mod shell_markers;
#[cfg(test)]
pub(super) use prompt_steps::prompt_step_dispatch_count_for_test;
pub(super) use prompt_steps::write_bound_prompt_steps;
use prompt_steps::write_prompt_steps;
mod prompt_notifications;
use prompt_notifications::should_notify_pending_prompt;

fn parse_manual_update_prompt(
    provider: &dyn codeterm_plugin::provider::AgentProvider,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
) -> Option<crate::multiplexer::types::PendingPrompt> {
    if !manual_update_prompt_candidate(&snapshot.screen_text) {
        return None;
    }
    let classification = provider.classify_tab_state(snapshot);
    crate::agent::state_reactor::is_manual_update_prompt(classification.as_ref())
        .then(|| provider.parse_prompt(&snapshot.screen_text))
        .flatten()
}

#[cfg(test)]
fn parse_directory_trust_prompt(
    provider: &dyn codeterm_plugin::provider::AgentProvider,
    screen: &str,
) -> Option<crate::multiplexer::types::PendingPrompt> {
    if !trust_candidate::may_be_directory_trust_prompt(screen)
        || !provider.supports_directory_trust_prompt_parsing()
    {
        return None;
    }
    provider.parse_directory_trust_prompt(screen)
}

fn manual_update_prompt_candidate(screen: &str) -> bool {
    let mut options = Vec::new();
    let mut footer = false;
    for row in screen.lines() {
        let lower = row.to_ascii_lowercase();
        if lower.contains("enter")
            && ["confirm", "continue", "select"]
                .iter()
                .any(|word| lower.contains(word))
        {
            footer = true;
        }
        let row = row.trim_start();
        let row = row
            .strip_prefix('>')
            .or_else(|| row.strip_prefix('❯'))
            .or_else(|| row.strip_prefix('›'))
            .unwrap_or(row)
            .trim_start();
        let digits = row.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let Some(separator) = row[digits..].trim_start().chars().next() else {
            continue;
        };
        if !matches!(separator, '.' | ')') {
            continue;
        }
        let label = row[digits..]
            .trim_start()
            .get(separator.len_utf8()..)
            .unwrap_or_default()
            .trim_start()
            .to_ascii_lowercase();
        options.push(label);
    }
    footer
        && options.iter().any(|label| label.contains("update"))
        && options.iter().any(|label| label.contains("skip"))
}

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
            directory_trust_eligible,
            directory_trust_skip,
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
            let trust = tab.is_some_and(super::directory_trust::directory_trust_check_applies);
            let skip = tab.and_then(super::directory_trust::directory_trust_skip_reason);
            (
                pid,
                at,
                active_prompt,
                auto_approve,
                nonce,
                cwd,
                sid,
                trust,
                skip,
            )
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

        let changed_screen = screen_changed.then(|| {
            self.screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default()
        });
        if let (Some(ref pid), Some(ref at)) = (&tab_id, &agent_type) {
            let trust_candidate = trust_candidate::candidate_for_output(
                self,
                pid,
                at,
                changed_screen.as_deref(),
                directory_trust_eligible,
                is_auto_approve,
                directory_trust_skip,
            );
            if let Ok(raw) = std::str::from_utf8(data) {
                let chunk_text = crate::pty::ansi::clean_terminal_output(raw);
                let has_markers = self.agent_registry.has_event_markers(at, &chunk_text);
                let update_candidate = changed_screen
                    .as_deref()
                    .is_some_and(manual_update_prompt_candidate);
                let trust_prompt_candidate = trust_candidate::pending_prompt_candidate(
                    self,
                    pty_id,
                    at,
                    has_active_prompt,
                    changed_screen.as_deref(),
                );

                // auto_approve tabs (GA), always read screen — chunk fragmentation can split markers across reads, so we check full screen every time.
                let trust_candidate_screen = trust_candidate.then(|| {
                    self.screen_state
                        .lock()
                        .get_screen_text(pty_id)
                        .unwrap_or_default()
                });
                let check_screen = has_markers
                    || has_active_prompt
                    || is_auto_approve
                    || trust_candidate
                    || trust_prompt_candidate
                    || update_candidate;
                let (native_prompt_input, mut clean_text) = {
                    let screen_state = self.screen_state.lock();
                    let clean_text = if check_screen {
                        trust_candidate_screen
                            .or_else(|| changed_screen.clone())
                            .or_else(|| screen_state.get_screen_text(pty_id))
                    } else {
                        None
                    };
                    (screen_state.native_prompt_input(pty_id), clean_text)
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
                let manual_update_candidate = structured.is_none() && update_candidate;
                if manual_update_candidate && screen_changed {
                    let snapshot = codeterm_plugin::tab_state::TabStateSnapshot {
                        tab_id: pid.to_string(),
                        agent_type: at.to_string(),
                        session_id: agent_session_id.clone(),
                        cwd: tab_cwd.clone(),
                        screen_text: changed_screen.clone().unwrap_or_default(),
                    };
                    self.classify_manual_update_prompt_off_runtime(pty_id.to_string(), snapshot);
                }
                let default_prompt_kind = if structured.is_some() {
                    codeterm_types::pty::PromptKind::Structured
                } else if manual_update_candidate {
                    codeterm_types::pty::PromptKind::ManualUpdate
                } else {
                    codeterm_types::pty::PromptKind::Provider
                };
                let (provider_prompt, directory_trust_prompt, prompt_kind) =
                    if structured.is_none() && !manual_update_candidate {
                        clean_text
                            .as_deref()
                            .and_then(|screen| {
                                crate::agent::prompt::parse_prompt_safely(|| {
                                    self.agent_registry.parse_interactive_prompt(at, screen)
                                })
                            })
                            .map(|(prompt, kind)| {
                                let is_directory_trust =
                                    kind == codeterm_types::pty::PromptKind::DirectoryTrust;
                                (Some(prompt), is_directory_trust, kind)
                            })
                            .unwrap_or((None, false, default_prompt_kind))
                    } else {
                        (None, false, default_prompt_kind)
                    };
                let parsed = crate::agent::prompt::parse_prompt_safely(|| {
                    if let Some(prompt) = structured {
                        crate::agent::native_prompt::resolve_pending_prompt(
                            Some(prompt),
                            native_prompt_input.as_ref(),
                        )
                    } else {
                        provider_prompt.map(|prompt| crate::agent::native_prompt::ResolvedPrompt {
                            prompt,
                            explicit: false,
                        })
                    }
                });
                let prompt_signal = parsed.is_some();
                if prompt_signal && clean_text.is_none() {
                    clean_text = self.screen_state.lock().get_screen_text(pty_id);
                }
                let decision = if manual_update_candidate {
                    PromptDecision::None
                } else {
                    self.prompt_detector.lock().evaluate_pty_output_parsed(
                        pid,
                        check_screen || prompt_signal,
                        clean_text.is_some() || native_prompt_input.is_some(),
                        parsed,
                    )
                };

                self.apply_prompt_decision_with_kind(
                    &decision,
                    clean_text.as_deref(),
                    if trust_candidate || directory_trust_prompt {
                        codeterm_types::pty::PromptKind::DirectoryTrust
                    } else {
                        prompt_kind
                    },
                    manual_update_candidate,
                );

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

    /// Build a pending prompt from the provider's structured source, if it has
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
    /// active; the caller may then use its provider parser for the visible screen.
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
        self.apply_prompt_decision_with_screen(decision, None, false, false);
    }

    fn classify_manual_update_prompt_off_runtime(
        &self,
        pty_id: String,
        snapshot: codeterm_plugin::tab_state::TabStateSnapshot,
    ) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let prompt = svc
                .agent_registry
                .with_provider(&snapshot.agent_type, |provider| {
                    parse_manual_update_prompt(provider, &snapshot)
                })
                .flatten();
            let Some(prompt) = prompt else {
                return;
            };
            let screen = svc.screen_state.lock().get_screen_text(&pty_id);
            if screen.as_deref() != Some(snapshot.screen_text.as_str()) {
                return;
            }
            let decision = svc.prompt_detector.lock().evaluate_pty_output_parsed(
                &snapshot.tab_id,
                true,
                true,
                Some(crate::agent::prompt::ResolvedPrompt {
                    prompt,
                    explicit: false,
                }),
            );
            svc.apply_prompt_decision_with_screen(
                &decision,
                Some(&snapshot.screen_text),
                false,
                true,
            );
        });
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
            tab.pending_prompt_kind = prompt
                .as_ref()
                .map(|_| codeterm_types::pty::PromptKind::Structured);
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
        directory_trust_prompt: bool,
        require_user_confirmation: bool,
    ) {
        let prompt_kind = if directory_trust_prompt {
            codeterm_types::pty::PromptKind::DirectoryTrust
        } else {
            codeterm_types::pty::PromptKind::Provider
        };
        self.apply_prompt_decision_with_kind(
            decision,
            screen_text,
            prompt_kind,
            require_user_confirmation,
        );
    }

    pub(crate) fn apply_prompt_decision_with_kind(
        &self,
        decision: &PromptDecision,
        screen_text: Option<&str>,
        prompt_kind: codeterm_types::pty::PromptKind,
        require_user_confirmation: bool,
    ) {
        let directory_trust_prompt = prompt_kind == codeterm_types::pty::PromptKind::DirectoryTrust;
        match decision {
            PromptDecision::None => {}
            PromptDecision::NewPrompt { tab_id, prompt }
            | PromptDecision::PromptUpdated { tab_id, prompt } => {
                let (auto_approve, agent_type, pty_id, is_general_tab, session_authenticated) = {
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
                        tab.is_some_and(|p| p.launch_session_authenticated()),
                    )
                };

                let trust = if require_user_confirmation {
                    super::directory_trust::TrustDecision::NotApplicable
                } else {
                    self.respond_to_directory_trust(tab_id, screen_text)
                };
                if trust.owns_prompt() {
                    return;
                }
                if auto_approve && !directory_trust_prompt && !require_user_confirmation {
                    if is_general_tab {
                        let provider_classified_safety_dialog = agent_type
                            .as_deref()
                            .zip(screen_text)
                            .is_some_and(|(agent_type, screen)| {
                                trust_candidate::provider_classifies_non_auto_ack_dialog(
                                    self,
                                    trust_candidate::ProviderDialogIdentity { agent_type, tab_id },
                                    screen,
                                )
                            });
                        if let Some(ack) = crate::service::general_agent::blocker::classify(
                            prompt,
                            screen_text.unwrap_or(""),
                            directory_trust_prompt || provider_classified_safety_dialog,
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

                    let structured_prompt =
                        crate::service::structured_prompt::pending_for_tab(tab_id)
                            .is_some_and(|structured| &structured == prompt);
                    let legacy_permitted = super::directory_trust::legacy_auto_approve_permitted(
                        &trust,
                        session_authenticated,
                        structured_prompt,
                        directory_trust_prompt,
                    );
                    if let Some(response) = legacy_permitted
                        .then(|| legacy_auto_approve_response(prompt, directory_trust_prompt))
                        .flatten()
                    {
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
                if !directory_trust_prompt
                    && !require_user_confirmation
                    && self
                        .try_permission_auto_confirm(tab_id, screen_text)
                        .owns_prompt()
                {
                    return;
                }
                let notify_prompt = {
                    let mut mux = self.mux.write();
                    if let Some(tab) = mux.get_tab_mut(tab_id) {
                        let previous_identity = tab.prompt_identity();
                        let notify = should_notify_pending_prompt(
                            decision,
                            tab.pending_prompt.as_ref(),
                            tab.pending_prompt_kind,
                            prompt_kind,
                        );
                        tab.pending_prompt = Some(prompt.clone());
                        tab.pending_prompt_kind = Some(prompt_kind);
                        Some((notify, previous_identity))
                    } else {
                        None
                    }
                };
                let Some((notify_prompt, previous_identity)) = notify_prompt else {
                    return;
                };
                let identity = codeterm_types::pty::PromptIdentity::new(prompt_kind, prompt);
                self.resolve_prompt_transition(tab_id, previous_identity, Some(&identity));
                crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "prompt-state-changed",
                    serde_json::json!({
                        "tab_id": tab_id,
                        "prompt": prompt,
                    }),
                );
                self.emit_state_changed();
                if notify_prompt {
                    self.notify_pending_prompt(tab_id, prompt, prompt_kind);
                }
            }
            PromptDecision::PromptCleared { tab_id } => {
                if let Some(identity) = self.clear_pending_prompt(tab_id) {
                    self.resolve_prompt_notifications(tab_id, &identity);
                }
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

pub(super) struct LegacyAutoApproveResponse {
    kind: &'static str,
    bytes: Vec<u8>,
}

pub(super) fn legacy_auto_approve_response(
    prompt: &crate::multiplexer::types::PendingPrompt,
    directory_trust_prompt: bool,
) -> Option<LegacyAutoApproveResponse> {
    if directory_trust_prompt {
        return None;
    }
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

#[cfg(test)]
#[path = "output/manual_update_prompt_tests.rs"]
mod manual_update_prompt_tests;
#[path = "output/trust_candidate.rs"]
mod trust_candidate;
