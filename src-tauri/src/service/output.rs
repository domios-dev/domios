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
        if n % 200 == 0 {
            log::info!("[output] process_pty_output called {} times, pty={}, data_len={}", n, &pty_id[..pty_id.len().min(8)], data.len());
        }
        // 1. Track status using screen content hash (not raw bytes).
        //    Cursor blinks and TUI noise don't change screen content,
        //    so they won't reset the idle timer.
        //    Screen hash stays stable when nothing meaningful changes →
        //    5s timeout kicks in → status transitions to Idle → spinner stops.
        let prev_status = {
            let screen_hash = {
                let buf = self.output_buf.lock();
                if let Some(screen) = buf.get_screen_text(pty_id) {
                    use sha2::{Digest, Sha256};
                    let mut h = Sha256::new();
                    h.update(screen.as_bytes());
                    h.finalize().into()
                } else {
                    [0u8; 32]
                }
            };
            let mut t = self.agent_tracker.lock();
            let prev = t.get_status(pty_id);
            t.update_output(pty_id, screen_hash);
            prev
        };

        // 3. Get agent type for this pane
        let (pane_id, agent_type) = {
            let mux = self.mux.lock();
            let pid = mux.pane_id_for_pty(pty_id);
            let at = pid.as_ref().and_then(|id| mux.get_pane(id)).and_then(|p| p.agent_type.clone());
            (pid, at)
        };

        let current_status = self.agent_tracker.lock().get_status(pty_id);

        // Agent event detection: always parse PTY output for all agents.
        // Hooks (Claude Code) provide a faster path via API endpoints
        // (see /api/panes/{id}/hook/permission), but PTY auto-detect is the
        // reliable fallback — hooks may not fire on Windows (env var syntax),
        // in dev mode, or when misconfigured.
        if let (Some(ref pid), Some(ref at)) = (&pane_id, &agent_type) {
            if let Ok(raw) = std::str::from_utf8(data) {
                let chunk_text = crate::api::routes::clean_terminal_output(raw);
                let has_markers = self.agent_registry.has_event_markers(at, &chunk_text);

                // For prompt detection, use the per-pane VT100 screen content.
                // This is what the user actually sees — handles cursor positioning,
                // alternate screen, and TUI overlays correctly.
                let clean_text = if has_markers {
                    self.output_buf.lock().get_screen_text(pty_id)
                } else {
                    None
                };

                // Delegate prompt detection to PromptDetector
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

                // Apply decision to mux state
                self.apply_prompt_decision(&decision);

                // Also check for non-prompt agent events (task completion, etc.)
                if has_markers {
                    if let Some(ref clean) = clean_text {
                        let events = self.agent_registry.detect_events(at, clean);
                        for event in &events {
                            if matches!(event, AgentEvent::PermissionRequest) {
                                // PermissionRequest is handled by PromptDetector above,
                                // but we still need to set the Waiting status
                                self.agent_tracker.lock().set_waiting(pty_id);
                            } else {
                                self.handle_agent_event(pty_id, pid, at, event, prev_status);
                            }
                        }
                    }
                }
            }
        }

        // Signal frontend that this pane has new output
        let _ = self.app_handle.emit("pane-output-changed", serde_json::json!({
            "pty_id": pty_id,
        }));

        (prev_status, current_status)
    }

    /// Apply a PromptDecision to mux state — set prompt on pane, emit events, notify.
    pub(crate) fn apply_prompt_decision(&self, decision: &PromptDecision) {
        match decision {
            PromptDecision::None => {},
            PromptDecision::NewPrompt { pane_id, prompt } |
            PromptDecision::PromptUpdated { pane_id, prompt } => {
                // Set on pane
                {
                    let mut mux = self.mux.lock();
                    if let Some(pane) = mux.get_pane_mut(pane_id) {
                        pane.pending_prompt = Some(prompt.clone());
                        log::info!("[prompt] Set pending_prompt on pane {} q={:?}", pane_id, prompt.question);
                    } else {
                        log::warn!("[prompt] Pane {} not found for setting pending_prompt", pane_id);
                    }
                }
                // Emit state change so frontend picks up the prompt
                self.emit_state_changed();
                // Notify frontend + webhooks
                let title = self.mux.lock().get_pane(pane_id).map(|p| p.title.clone());
                if let Some(title) = title {
                    self.notify_event("agent.permission_request", serde_json::json!({
                        "pane_id": pane_id,
                        "title": format!("{} needs approval", title),
                        "body": "Agent is waiting for your permission",
                    }));
                }
            }
        }
    }

    /// Handle a single agent event — update state and dispatch notifications.
    /// PermissionRequest is now handled by PromptDetector; only TaskComplete goes through here.
    fn handle_agent_event(
        &self,
        _pty_id: &str,
        pane_id: &str,
        _agent_type: &str,
        event: &AgentEvent,
        prev_status: PaneStatus,
    ) {
        // Cooldown: skip if same event for same pane fired recently.
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
            AgentEvent::PermissionRequest => {
                // Handled by PromptDetector — should not reach here
            }
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
