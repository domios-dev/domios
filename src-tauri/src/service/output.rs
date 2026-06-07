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

        let (pane_id, agent_type) = {
            let mux = self.mux.lock();
            let pid = mux.pane_id_for_pty(pty_id);
            let at = pid
                .as_ref()
                .and_then(|id| mux.get_pane(id))
                .and_then(|p| p.agent_type.clone());
            (pid, at)
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
                let (in_grace, locked) = {
                    let mux = self.mux.lock();
                    let pane = mux.get_pane(pid);
                    let grace = pane
                        .and_then(|p| p.agent_set_at)
                        .map(|t| t.elapsed().as_secs() < 30)
                        .unwrap_or(false);
                    let locked = pane.map(|p| p.agent_type_locked).unwrap_or(false);
                    (grace, locked)
                };
                if in_grace || locked {
                    agent_type
                } else {
                    let screen = self
                        .screen_state
                        .lock()
                        .get_screen_text(pty_id)
                        .unwrap_or_default();
                    if !screen.is_empty()
                        && self.agent_registry.detect_from_output(&screen).is_none()
                    {
                        // Screen has content but no agent markers — check for shell prompt
                        if let Some(last_line) = screen.lines().rev().find(|l| !l.trim().is_empty())
                        {
                            let t = last_line.trim();
                            if t.ends_with('>')
                                || t.ends_with('$')
                                || t.ends_with('%')
                                || t.ends_with('#')
                            {
                                log::info!(
                                    "[output] agent exited in pane {} (shell prompt detected)",
                                    pid
                                );
                                self.set_pane_agent_type(pid, None);
                                self.prompt_detector.lock().clear(pid);
                                {
                                    let mut mux = self.mux.lock();
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
                            agent_type
                        }
                    } else {
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
            if let Ok(raw) = std::str::from_utf8(data) {
                let chunk_text = crate::pty::ansi::clean_terminal_output(raw);
                let has_markers = self.agent_registry.has_event_markers(at, &chunk_text);

                // Read screen text if markers found OR if there's an active prompt (need screen text to detect when user answers prompt in terminal). For
                // auto_approve panes (GA), always read screen — chunk fragmentation can split markers across reads, so we check full screen every time.
                let (has_active_prompt, is_auto_approve) = {
                    let mux = self.mux.lock();
                    let pane = mux.get_pane(pid);
                    (
                        pane.and_then(|p| p.pending_prompt.as_ref()).is_some(),
                        pane.map(|p| p.auto_approve).unwrap_or(false),
                    )
                };
                let check_screen = has_markers || has_active_prompt || is_auto_approve;
                let clean_text = if check_screen {
                    self.screen_state.lock().get_screen_text(pty_id)
                } else {
                    None
                };

                let decision = {
                    let mut detector = self.prompt_detector.lock();
                    let agent_type_clone = at.clone();
                    let registry = &self.agent_registry;
                    detector.evaluate_pty_output(pid, check_screen, clean_text.as_deref(), |text| {
                        registry.parse_prompt(&agent_type_clone, text)
                    })
                };

                self.apply_prompt_decision(&decision);

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

    pub(crate) fn apply_prompt_decision(&self, decision: &PromptDecision) {
        match decision {
            PromptDecision::None => {}
            PromptDecision::NewPrompt { pane_id, prompt }
            | PromptDecision::PromptUpdated { pane_id, prompt } => {
                // Auto-approve: if pane has auto_approve set, respond immediately
                let auto_approve = self
                    .mux
                    .lock()
                    .get_pane(pane_id)
                    .map(|p| p.auto_approve)
                    .unwrap_or(false);

                if auto_approve {
                    // Check for yes/no prompts (value is "y" or "yes")
                    let is_yes_no = prompt.options.iter().any(|o| {
                        let v = o.value.to_lowercase();
                        v == "y" || v == "yes"
                    });

                    // Check for numbered prompts where first option is affirmative (e.g. "1. Yes, continue" — trust directory prompts)
                    let is_numbered_yes = !is_yes_no
                        && !prompt.options.is_empty()
                        && prompt.options[0].label.to_lowercase().starts_with("yes");

                    if is_yes_no || is_numbered_yes {
                        let response: &[u8] = if is_yes_no { b"y\r" } else { b"\r" };
                        log::info!(
                            "[auto-approve] Auto-approving {} prompt for pane {}",
                            if is_yes_no { "yes/no" } else { "numbered-yes" },
                            pane_id
                        );
                        let pty_id = self
                            .mux
                            .lock()
                            .get_pane(pane_id)
                            .and_then(|p| p.pty_id.clone());
                        if let Some(pty_id) = pty_id {
                            let pty = self.pty.clone();
                            let response = response.to_vec();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(500));
                                let _ = pty.lock().write(&pty_id, &response);
                            });
                        }
                        return;
                    }
                    log::debug!(
                        "[auto-approve] Skipping non-approvable prompt for pane {}",
                        pane_id
                    );
                }
                {
                    let mut mux = self.mux.lock();
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
                let title = self.mux.lock().get_pane(pane_id).map(|p| p.title.clone());
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
                let mut mux = self.mux.lock();
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
                    let title = self.mux.lock().get_pane(pane_id).map(|p| p.title.clone());
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
