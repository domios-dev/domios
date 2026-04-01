//! PTY output processing — status tracking, agent event detection, notifications.
//!
//! Extracted from `commands::read_pty` to keep that function focused on I/O.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use tauri::Emitter;

use crate::agent::detector::AgentEvent;
use crate::agent::prompt::PromptDecision;
use crate::agent::status::PaneStatus;
use super::ServiceContext;

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
            log::info!("[output] process_pty_output #{}, pty={}, len={}", n, &pty_id[..pty_id.len().min(8)], data.len());
        }

        let (pane_id, agent_type) = {
            let mux = self.mux.lock();
            let pid = mux.pane_id_for_pty(pty_id);
            let at = pid.as_ref().and_then(|id| mux.get_pane(id)).and_then(|p| p.agent_type.clone());
            (pid, at)
        };

        let exclude_rows = agent_type.as_deref()
            .map(|at| self.agent_registry.status_bar_rows(at))
            .unwrap_or(0);
        let prev_status = {
            let screen_hash = self.screen_state.lock()
                .get_content_hash(pty_id, exclude_rows)
                .unwrap_or([0u8; 32]);
            let mut t = self.agent_tracker.lock();
            let prev = t.get_status(pty_id);
            t.update_output(pty_id, screen_hash);
            prev
        };
        let current_status = self.agent_tracker.lock().get_status(pty_id);
        if current_status != prev_status {
            log::info!("[status] {} {:?} -> {:?}", &pty_id[..pty_id.len().min(8)], prev_status, current_status);
        }

        // Auto-detect agent type from PTY output if not already set.
        // Uses VT100-parsed screen content (not raw chunks — they may be fragmented/ANSI-wrapped).
        let agent_type = if agent_type.is_none() {
            if let Some(ref pid) = pane_id {
                let screen = self.screen_state.lock().get_screen_text(pty_id).unwrap_or_default();
                // Also try raw output buffer (screen_state may be empty for alternate-screen apps)
                let raw_text = if screen.is_empty() {
                    self.output_buf.lock().get_text(pty_id).unwrap_or_default()
                } else { String::new() };
                let check_text = if !screen.is_empty() { &screen } else { &raw_text };
                if !check_text.is_empty() {
                    let detected = self.agent_registry.detect_from_output(check_text);
                    // Log periodically to aid debugging
                    static DETECT_LOG_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    let dc = DETECT_LOG_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if dc < 5 || dc % 100 == 0 {
                        log::debug!("[agent-detect] pane={} screen_len={} raw_len={} detected={} text_sample={:?}",
                            pid, screen.len(), raw_text.len(), detected.is_some(),
                            &check_text[..check_text.len().min(100)]);
                    }
                    if let Some(provider) = detected {
                        let agent_id = provider.id().to_string();
                        log::info!("[output] auto-detected agent '{}' in pane {} from screen content", agent_id, pid);
                        self.set_pane_agent_type(pid, Some(agent_id.clone()));
                        // Also detect session immediately
                        let (cwd, exclude) = {
                            let m = self.mux.lock();
                            (m.get_pane(pid).and_then(|p| p.cwd.clone()), m.other_session_ids(pid))
                        };
                        if let Some(cwd) = cwd {
                            let max_age = Some(std::time::Duration::from_secs(86400));
                            if let Some(sid) = provider.detect_session_id(&cwd, &exclude, max_age) {
                                log::info!("[output] auto-detected session {} for pane {}", sid, pid);
                                self.set_agent_session_id(pid, Some(sid));
                            }
                        }
                        self.emit_state_changed();
                        Some(agent_id)
                    } else { None }
                } else { None }
            } else { None }
        } else if let Some(ref pid) = pane_id {
            // Agent already detected — check if it's still running.
            // If screen shows a shell prompt (no agent markers), clear agent_type.
            // Only check periodically to avoid overhead on every chunk.
            static UNDETECT_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let count = UNDETECT_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if count % 50 == 0 { // Check every ~50 chunks
                let screen = self.screen_state.lock().get_screen_text(pty_id).unwrap_or_default();
                if !screen.is_empty() && self.agent_registry.detect_from_output(&screen).is_none() {
                    // Screen has content but no agent markers → agent exited
                    if let Some(last_line) = screen.lines().rev().find(|l| !l.trim().is_empty()) {
                        let t = last_line.trim();
                        // Confirm it's a shell prompt, not just a blank screen
                        if t.ends_with('>') || t.ends_with('$') || t.ends_with('%') || t.ends_with('#') {
                            log::info!("[output] agent exited in pane {} (shell prompt detected)", pid);
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
                        } else { agent_type }
                    } else { agent_type }
                } else { agent_type }
            } else { agent_type }
        } else {
            agent_type
        };

        if let (Some(ref pid), Some(ref at)) = (&pane_id, &agent_type) {
            if let Ok(raw) = std::str::from_utf8(data) {
                let chunk_text = crate::api::routes::clean_terminal_output(raw);
                let has_markers = self.agent_registry.has_event_markers(at, &chunk_text);

                // Read screen text if markers found OR if there's an active prompt
                // (need screen text to detect when user answers prompt in terminal)
                let has_active_prompt = {
                    let mux = self.mux.lock();
                    mux.get_pane(pid).and_then(|p| p.pending_prompt.as_ref()).is_some()
                };
                let clean_text = if has_markers || has_active_prompt {
                    self.screen_state.lock().get_screen_text(pty_id)
                } else {
                    None
                };

                let decision = {
                    let mut detector = self.prompt_detector.lock();
                    let agent_type_clone = at.clone();
                    let registry = &self.agent_registry;
                    detector.evaluate_pty_output(
                        pid,
                        has_markers,
                        clean_text.as_deref(),
                        |text| registry.parse_prompt(&agent_type_clone, text),
                    )
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

        let _ = self.app_handle.emit("pane-output-changed", serde_json::json!({
            "pty_id": pty_id,
        }));

        // Detect Telegram bridge markers — only after activation delay (skip startup replay)
        let bridge_active = std::time::Instant::now() >= *self.bridge_activate_at.lock();
        if bridge_active {
        if let Ok(text) = std::str::from_utf8(data) {
            let clean = crate::api::routes::clean_terminal_output(text);
            if !clean.is_empty() {
                let mut accumulators = self.bridge_accumulators.lock();
                let acc = accumulators.entry(pty_id.to_string())
                    .or_insert_with(crate::bridge::manager::MarkerAccumulator::new);
                for line in clean.lines() {
                    if let Some(msg) = acc.feed_line(line) {
                        log::info!("[bridge-detect] marker found, sending {} chars", msg.text.len());
                        if let Some(tx) = self.bridge_outbound.lock().as_ref() {
                            let _ = tx.send(msg);
                        }
                    }
                }
            }
        }
        } // bridge_active

        (prev_status, current_status)
    }

    pub(crate) fn apply_prompt_decision(&self, decision: &PromptDecision) {
        match decision {
            PromptDecision::None => {},
            PromptDecision::NewPrompt { pane_id, prompt } |
            PromptDecision::PromptUpdated { pane_id, prompt } => {
                // Auto-approve: if pane has auto_approve set, respond immediately
                let auto_approve = self.mux.lock()
                    .get_pane(pane_id)
                    .map(|p| p.auto_approve)
                    .unwrap_or(false);

                if auto_approve {
                    log::info!("[auto-approve] Auto-approving prompt for pane {}", pane_id);
                    let pty_id = self.mux.lock()
                        .get_pane(pane_id)
                        .and_then(|p| p.pty_id.clone());
                    if let Some(pty_id) = pty_id {
                        let pty = self.pty.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            let _ = pty.lock().write(&pty_id, b"\r");
                        });
                    }
                    return;
                }
                {
                    let mut mux = self.mux.lock();
                    if let Some(pane) = mux.get_pane_mut(pane_id) {
                        pane.pending_prompt = Some(prompt.clone());
                    }
                }
                // Emit focused event — frontend subscribes directly, no polling needed
                let _ = self.app_handle.emit("prompt-state-changed", serde_json::json!({
                    "pane_id": pane_id,
                    "prompt": prompt,
                }));
                self.emit_state_changed();
                let title = self.mux.lock().get_pane(pane_id).map(|p| p.title.clone());
                if let Some(title) = title {
                    self.notify_event("agent.permission_request", serde_json::json!({
                        "pane_id": pane_id,
                        "title": format!("{} needs approval", title),
                        "body": "Agent is waiting for your permission",
                    }));
                }
            }
            PromptDecision::PromptCleared { pane_id } => {
                let mut mux = self.mux.lock();
                if let Some(pane) = mux.get_pane_mut(pane_id) {
                    pane.pending_prompt = None;
                }
                drop(mux);
                let _ = self.app_handle.emit("prompt-state-changed", serde_json::json!({
                    "pane_id": pane_id,
                    "prompt": null,
                }));
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
                        self.notify_event("agent.task_complete", serde_json::json!({
                            "pane_id": pane_id,
                            "title": format!("{} finished", title),
                            "body": "Task complete",
                        }));
                    }
                }
            }
        }
    }
}
