//! PTY output processing — status tracking, agent event detection, notifications.
//!
//! Extracted from `commands::read_pty` to keep that function focused on I/O.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use tauri::Emitter;

use crate::agent::detector::AgentEvent;
use crate::agent::status::PaneStatus;
use crate::multiplexer::types::PendingPrompt;
use crate::multiplexer::types::PromptOption;
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
            log::info!("[output] process_pty_output called {} times, pty={}, data_len={}", n, &pty_id[..pty_id.len().min(8)], data.len());
        }
        // 1. Track status
        let prev_status = {
            let mut t = self.agent_tracker.lock();
            let prev = t.get_status(pty_id);
            t.update_output(pty_id, data);
            prev
        };

        // 2. Clear pending prompt when agent resumes from Waiting
        let current_status = self.agent_tracker.lock().get_status(pty_id);
        if current_status != prev_status && prev_status == PaneStatus::Waiting {
            let mut mux = self.mux.lock();
            if let Some(pid) = mux.pane_id_for_pty(pty_id) {
                if let Some(pane) = mux.get_pane_mut(&pid) {
                    pane.pending_prompt = None;
                }
            }
        }

        // 3. Get agent type for this pane
        let (pane_id, agent_type) = {
            let mux = self.mux.lock();
            let pid = mux.pane_id_for_pty(pty_id);
            let at = pid.as_ref().and_then(|id| mux.get_pane(id)).and_then(|p| p.agent_type.clone());
            (pid, at)
        };

        // Agent event detection: agents with hooks (Claude Code) get detected via API
        // endpoints (see /api/panes/{id}/hook/permission). For agents WITHOUT hooks
        // (Codex, Aider, generic), fall back to PTY output parsing.
        if let (Some(ref pid), Some(ref at)) = (&pane_id, &agent_type) {
            let provider = self.agent_registry.get(at);
            let has_hooks = provider.map(|p| p.has_hooks()).unwrap_or(false);

            if !has_hooks {
                if let Ok(raw) = std::str::from_utf8(data) {
                    let chunk_text = crate::api::routes::clean_terminal_output(raw);
                    if self.agent_registry.has_event_markers(at, &chunk_text) {
                        let ring_text = self.output_buf.lock().get_text(pty_id).unwrap_or_default();
                        let clean = crate::api::routes::clean_terminal_output(&ring_text);
                        let events = self.agent_registry.detect_events(at, &clean);
                        for event in &events {
                            self.handle_agent_event(pty_id, pid, at, event, prev_status);
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

    /// Handle a single agent event — update state and dispatch notifications.
    fn handle_agent_event(
        &self,
        pty_id: &str,
        pane_id: &str,
        agent_type: &str,
        event: &AgentEvent,
        prev_status: PaneStatus,
    ) {
        // Cooldown: skip if same event for same pane fired within last 30s
        let cooldown_key = format!("{}:{:?}", pane_id, event);
        {
            let mut map = EVENT_COOLDOWN.lock().unwrap();
            let now = Instant::now();
            if let Some(last) = map.get(&cooldown_key) {
                if now.duration_since(*last).as_secs() < 30 {
                    return;
                }
            }
            map.insert(cooldown_key, now);
        }

        match event {
            AgentEvent::PermissionRequest => {
                self.agent_tracker.lock().set_waiting(pty_id);

                // Check if prompt already set — if so, skip (no re-notification)
                let already_has_prompt = self.mux.lock()
                    .get_pane(pane_id)
                    .and_then(|p| p.pending_prompt.as_ref())
                    .is_some();

                if already_has_prompt {
                    return; // Already notified, don't spam
                }

                // Parse prompt options from ring buffer (last 64KB, safe)
                {
                    let buf = self.output_buf.lock();
                    let ring_text = buf.get_text(pty_id).unwrap_or_default();
                    let clean_tail = crate::api::routes::clean_terminal_output(&ring_text);
                    if let Some((question, opts)) = self.agent_registry.parse_prompt(agent_type, &clean_tail) {
                        log::debug!("[prompt] Parsed: {:?} with {} options", question, opts.len());
                        let prompt = PendingPrompt {
                            question,
                            options: opts.into_iter().map(|(label, value)| {
                                PromptOption { label, value }
                            }).collect(),
                        };
                        if let Some(pane) = self.mux.lock().get_pane_mut(pane_id) {
                            pane.pending_prompt = Some(prompt);
                        }
                    }
                }

                // Single event — frontend shows toast, webhooks get notified
                let title = self.mux.lock().get_pane(pane_id).map(|p| p.title.clone());
                if let Some(title) = title {
                    self.notify_event("agent.permission_request", serde_json::json!({
                        "pane_id": pane_id,
                        "title": format!("{} needs approval", title),
                        "body": "Agent is waiting for your permission",
                    }));
                }
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
