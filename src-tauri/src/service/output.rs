//! PTY output processing — status tracking, agent event detection, notifications.
//!
//! Extracted from `commands::read_pty` to keep that function focused on I/O.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use tauri::Emitter;

use super::ServiceContext;
use crate::agent::detector::AgentEvent;
use crate::agent::prompt::PromptDecision;
use crate::agent::status::PaneStatus;

/// Per-pane notification cooldown: (pane_id, event_kind) → last fire time.
/// Prevents notification spam from TUI redraws (same event every chunk).
static EVENT_COOLDOWN: LazyLock<StdMutex<HashMap<String, Instant>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

/// Per-pane throttle for post-launch session-id detection (cursor and any other
/// hookless, non-injecting provider). Detection is on-disk fs work; once per
/// second per pane is plenty during the brief window before the id is found.
static SESSION_DETECT_COOLDOWN: LazyLock<StdMutex<HashMap<String, Instant>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

impl ServiceContext {
    /// Process new PTY output: track status, detect agent events, dispatch notifications.
    /// Returns the previous and current status for the caller to emit status-change events.
    pub fn process_pty_output(&self, pty_id: &str, data: &[u8]) -> (PaneStatus, PaneStatus) {
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

        // Single read lock captures all pane data needed for this chunk.
        // Concurrent PTY handlers for different panes can read in parallel.
        let (
            pane_id,
            agent_type,
            in_grace_cached,
            locked_cached,
            has_active_prompt,
            is_auto_approve,
        ) = {
            let mux = self.mux.read();
            let pid = mux.pane_id_for_pty(pty_id);
            let pane = pid.as_ref().and_then(|id| mux.get_pane(id));
            let at = pane.and_then(|p| p.agent_type.clone());
            let grace = pane
                .and_then(|p| p.agent_set_at)
                .map(|t| t.elapsed().as_secs() < 30)
                .unwrap_or(false);
            let locked = pane.map(|p| p.agent_type_locked).unwrap_or(false);
            let active_prompt = pane.and_then(|p| p.pending_prompt.as_ref()).is_some();
            let auto_approve = pane.map(|p| p.auto_approve).unwrap_or(false);
            (pid, at, grace, locked, active_prompt, auto_approve)
        };

        let exclude_rows = agent_type
            .as_deref()
            .map(|at| self.agent_registry.status_bar_rows(at))
            .unwrap_or(0);
        let (prev_status, screen_changed) = {
            let screen_hash = self
                .screen_state
                .lock()
                .get_content_hash(pty_id, exclude_rows)
                .unwrap_or([0u8; 32]);
            let mut t = self.agent_tracker.lock();
            let prev = t.get_status(pty_id);
            let changed = t.update_output(pty_id, screen_hash);
            (prev, changed)
        };
        if screen_changed {
            if let Some(ref pid) = pane_id {
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

        // agent_type is intentional: explicit spawn paths set it directly.
        // Output parsing no longer infers identity except the narrow shell-prompt exit path.
        let agent_type = if agent_type.is_none() {
            None
        } else if let Some(ref pid) = pane_id {
            // Agent already detected — check if it's still running. This is the SOLE agent exit detection point (removed from idle-poll to avoid races).
            // Only check periodically to avoid overhead on every chunk.
            static UNDETECT_COUNT: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let count = UNDETECT_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if count % 50 == 0 {
                // Keep a short grace after session-tagging so early prompt snapshots don't trigger false exits.
                // Locked panes skip output-based clearing because slow startup can leave stale prompt text visible.
                if in_grace_cached || locked_cached {
                    agent_type
                } else {
                    // Debounce window: a still-running TUI can flash a marker-less,
                    // shell-prompt-looking frame for an instant (a WebView reload /
                    // resize redraw). Require the prompt to persist this long before
                    // declaring the agent gone, so one transient frame can't clear it.
                    const EXIT_DEBOUNCE_SECS: u64 = 2;
                    let screen = self
                        .screen_state
                        .lock()
                        .get_screen_text(pty_id)
                        .unwrap_or_default();
                    let suspected_exit = !screen.is_empty()
                        && self.agent_registry.detect_from_output(&screen).is_none()
                        && screen_ends_in_shell_prompt(&screen);
                    if suspected_exit {
                        let confirmed = {
                            let mut mux = self.mux.write();
                            match mux.get_pane_mut(pid) {
                                Some(pane) => {
                                    let (clear, next) = exit_debounce(
                                        pane.exit_suspected_at,
                                        std::time::Instant::now(),
                                        std::time::Duration::from_secs(EXIT_DEBOUNCE_SECS),
                                    );
                                    pane.exit_suspected_at = next;
                                    clear
                                }
                                None => false,
                            }
                        };
                        if confirmed {
                            log::info!(
                                "[output] agent exited in pane {} (shell prompt persisted)",
                                pid
                            );
                            self.set_pane_agent_type(pid, None);
                            self.prompt_detector.lock().clear(pid);
                            {
                                let mut mux = self.mux.write();
                                if let Some(pane) = mux.get_pane_mut(pid) {
                                    pane.pending_prompt = None;
                                }
                            }
                            self.emit_state_changed();
                            None
                        } else {
                            agent_type
                        }
                    } else {
                        // Agent markers present (or no shell prompt) — still alive; clear suspicion.
                        if let Some(pane) = self.mux.write().get_pane_mut(pid) {
                            pane.exit_suspected_at = None;
                        }
                        agent_type
                    }
                }
            } else {
                agent_type
            }
        } else {
            agent_type
        };

        if let (Some(ref pid), Some(ref at)) = (&pane_id, &agent_type) {
            // Hookless, non-injecting providers (cursor) can't report their
            // session id — detect it on disk post-launch and pin it, or the chat
            // view never binds (chat_start stays pending). No-op once found.
            self.maybe_detect_agent_session(pid, at);
            if let Ok(raw) = std::str::from_utf8(data) {
                let chunk_text = crate::pty::ansi::clean_terminal_output(raw);
                let has_markers = self.agent_registry.has_event_markers(at, &chunk_text);

                // Read screen text if markers found OR if there's an active prompt (need screen text to detect when user answers prompt in terminal). For
                // auto_approve panes (GA), always read screen — chunk fragmentation can split markers across reads, so we check full screen every time.
                let check_screen = has_markers || has_active_prompt || is_auto_approve;
                let clean_text = if check_screen {
                    self.screen_state.lock().get_screen_text(pty_id)
                } else {
                    None
                };

                let parsed = clean_text.as_deref().and_then(|text| {
                    let agent_type_clone = at.clone();
                    crate::agent::prompt::parse_prompt_safely(|| {
                        self.agent_registry.parse_prompt(&agent_type_clone, text)
                    })
                });
                let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
                    pid,
                    check_screen,
                    clean_text.is_some(),
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

        (prev_status, current_status)
    }

    /// Post-launch session-id detection for providers that report their session
    /// neither by hook nor by launch injection (cursor). claude/codex hook their
    /// id in; injecting providers get a pre-generated id at spawn. A pure-detect
    /// provider's id only exists once the agent writes its on-disk session, so we
    /// poll the provider's `detect_session_id` here (where the agent is already
    /// detected from output) until it resolves, then pin it — otherwise
    /// `chat_start` stays "pending" forever and the chat view renders empty even
    /// though the TUI works. Bounded: skips once the id is set, throttled to 1/s.
    fn maybe_detect_agent_session(&self, pane_id: &str, agent_type: &str) {
        let (cwd, set_at_elapsed) = {
            let mux = self.mux.read();
            let Some(pane) = mux.get_pane(pane_id) else {
                return;
            };
            if pane.agent_session_id.is_some() {
                return;
            }
            let cwd = pane.cwd.clone().unwrap_or_default();
            let elapsed = pane
                .agent_set_at
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            (cwd, elapsed)
        };
        if cwd.is_empty() {
            return;
        }
        // Only hookless + non-injecting providers need on-disk detection.
        let needs = self
            .agent_registry
            .with_provider(agent_type, |p| {
                !p.has_hooks()
                    && matches!(
                        p.session_id_injection(),
                        crate::agent::provider::SessionIdInjection::None
                    )
            })
            .unwrap_or(false);
        if !needs {
            return;
        }
        // Throttle: at most once per second per pane.
        {
            let mut cd = SESSION_DETECT_COOLDOWN.lock().unwrap();
            let now = Instant::now();
            if let Some(last) = cd.get(pane_id) {
                if now.duration_since(*last) < std::time::Duration::from_secs(1) {
                    return;
                }
            }
            cd.insert(pane_id.to_string(), now);
        }
        // Window the search to sessions touched since the agent was detected (+30s
        // slack) so a reused cwd's older sessions can't shadow the fresh one.
        let exclude = self.mux.read().other_session_ids(pane_id);
        let max_age = std::time::Duration::from_secs(set_at_elapsed + 30);
        let found = self
            .agent_registry
            .with_provider(agent_type, |p| {
                p.detect_session_id(&cwd, &exclude, Some(max_age))
            })
            .flatten();
        if let Some(sid) = found {
            log::info!(
                "[output] post-launch detected {} session {} for pane {}",
                agent_type,
                sid,
                pane_id
            );
            self.set_agent_session_id(pane_id, Some(sid));
            self.emit_state_changed();
        }
    }

    pub(crate) fn apply_prompt_decision(&self, decision: &PromptDecision) {
        self.apply_prompt_decision_with_screen(decision, None);
    }

    pub(crate) fn apply_prompt_decision_with_screen(
        &self,
        decision: &PromptDecision,
        screen_text: Option<&str>,
    ) {
        match decision {
            PromptDecision::None => {}
            PromptDecision::NewPrompt { pane_id, prompt }
            | PromptDecision::PromptUpdated { pane_id, prompt } => {
                // Auto-approve: if pane has auto_approve set, respond immediately
                let (auto_approve, agent_type, pty_id, is_general_pane) = {
                    let mux = self.mux.read();
                    let is_general_pane = mux
                        .resolve_pane_id(crate::service::general_agent::GENERAL_ALIAS)
                        .as_deref()
                        == Some(pane_id.as_str());
                    let pane = mux.get_pane(pane_id);
                    (
                        pane.map(|p| p.auto_approve).unwrap_or(false),
                        pane.and_then(|p| p.agent_type.clone()),
                        pane.and_then(|p| p.pty_id.clone()),
                        is_general_pane,
                    )
                };

                if auto_approve {
                    if is_general_pane {
                        if let Some(ack) = crate::service::general_agent::blocker::classify(
                            prompt,
                            screen_text.unwrap_or(""),
                        ) {
                            log::info!(
                                "[general-agent/auto-ack] Auto-acknowledging {} for pane {}",
                                ack.kind.label(),
                                pane_id
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
                            self.prompt_detector.lock().mark_responded(pane_id);
                            if let Some(pty_id) = pty_id {
                                let steps = agent_type
                                    .as_deref()
                                    .and_then(|at| {
                                        self.agent_registry
                                            .build_prompt_response(at, ack.option_index)
                                    })
                                    .unwrap_or_else(|| default_prompt_response(ack.option_index));
                                write_prompt_steps(self.pty.clone(), pty_id, steps);
                            }
                            return;
                        }
                    }

                    if let Some(response) = legacy_auto_approve_response(prompt) {
                        log::info!(
                            "[auto-approve] Auto-approving {} prompt for pane {}",
                            response.kind,
                            pane_id
                        );
                        self.prompt_detector.lock().mark_responded(pane_id);
                        if let Some(pty_id) = pty_id {
                            write_prompt_steps(self.pty.clone(), pty_id, vec![response.bytes]);
                        }
                        return;
                    }

                    log::debug!(
                        "[auto-approve] Skipping non-approvable prompt for pane {}",
                        pane_id
                    );
                }
                {
                    let mut mux = self.mux.write();
                    if let Some(pane) = mux.get_pane_mut(pane_id) {
                        pane.pending_prompt = Some(prompt.clone());
                    }
                }
                // Emit focused event — frontend subscribes directly, no polling needed
                let _ = self.app_handle.emit(
                    "prompt-state-changed",
                    serde_json::json!({
                        "pane_id": pane_id,
                        "prompt": prompt,
                    }),
                );
                self.emit_state_changed();
                let title = self.mux.read().get_pane(pane_id).map(|p| p.title.clone());
                if let Some(title) = title {
                    self.notify_event(
                        "agent.permission_request",
                        serde_json::json!({
                            "pane_id": pane_id,
                            "title": format!("{} needs approval", title),
                            "body": "Agent is waiting for your permission",
                        }),
                    );
                }
            }
            PromptDecision::PromptCleared { pane_id } => {
                let mut mux = self.mux.write();
                if let Some(pane) = mux.get_pane_mut(pane_id) {
                    pane.pending_prompt = None;
                }
                drop(mux);
                let _ = self.app_handle.emit(
                    "prompt-state-changed",
                    serde_json::json!({
                        "pane_id": pane_id,
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
        pane_id: &str,
        _agent_type: &str,
        event: &AgentEvent,
        prev_status: PaneStatus,
    ) {
        let cooldown_key = format!("{}:{:?}", pane_id, event);
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
                if prev_status == PaneStatus::Working {
                    let title = self.mux.read().get_pane(pane_id).map(|p| p.title.clone());
                    if let Some(title) = title {
                        self.notify_event(
                            "agent.task_complete",
                            serde_json::json!({
                                "pane_id": pane_id,
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

/// Debounce decision for output-based agent-exit detection. A still-running TUI
/// can flash ONE marker-less, shell-prompt-looking frame (a WebView reload /
/// resize redraw); requiring the suspicion to persist past `debounce` stops that
/// single frame from clearing a live agent. Returns `(should_clear, next_state)`:
/// the caller writes `next_state` back to the pane's `exit_suspected_at`.
fn exit_debounce(
    suspected_at: Option<std::time::Instant>,
    now: std::time::Instant,
    debounce: std::time::Duration,
) -> (bool, Option<std::time::Instant>) {
    match suspected_at {
        Some(since) if now.duration_since(since) >= debounce => (true, None),
        Some(_) => (false, suspected_at),
        None => (false, Some(now)),
    }
}

#[cfg(test)]
mod exit_detection_tests {
    use super::{exit_debounce, screen_ends_in_shell_prompt};
    use std::time::{Duration, Instant};

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
    fn first_suspicion_arms_timer_but_does_not_clear() {
        let now = Instant::now();
        let (clear, next) = exit_debounce(None, now, Duration::from_secs(2));
        assert!(!clear, "one transient frame must never clear the agent");
        assert_eq!(next, Some(now), "timer armed for the debounce window");
    }

    #[test]
    fn suspicion_within_window_holds_without_clearing() {
        let t0 = Instant::now();
        let later = t0 + Duration::from_millis(1500);
        let (clear, next) = exit_debounce(Some(t0), later, Duration::from_secs(2));
        assert!(!clear, "still inside the 2s window — keep the agent");
        assert_eq!(next, Some(t0), "timer unchanged");
    }

    #[test]
    fn persisted_suspicion_past_window_clears() {
        let t0 = Instant::now();
        let later = t0 + Duration::from_millis(2100);
        let (clear, next) = exit_debounce(Some(t0), later, Duration::from_secs(2));
        assert!(
            clear,
            "shell prompt persisted past debounce — real exit, clear"
        );
        assert_eq!(next, None, "suspicion reset after clearing");
    }
}
