//! PTY output processing — status tracking, agent event detection, notifications.
//!
//! Extracted from `commands::read_pty` to keep that function focused on I/O.

use tauri::Emitter;

use crate::agent::detector::AgentEvent;
use crate::agent::status::PaneStatus;
use crate::multiplexer::types::PendingPrompt;
use crate::multiplexer::types::PromptOption;
use super::ServiceContext;

impl ServiceContext {
    /// Process new PTY output: track status, detect agent events, dispatch notifications.
    /// Returns the previous and current status for the caller to emit status-change events.
    pub fn process_pty_output(&self, pty_id: &str, data: &[u8]) -> (PaneStatus, PaneStatus) {
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

        // 4. Detect agent events from buffer tail (not from chunk — chunks can split prompts)
        if let (Some(ref pid), Some(ref at)) = (&pane_id, &agent_type) {
            let tail_text = {
                let buf = self.output_buf.lock();
                let full = buf.get_full_text(pty_id).unwrap_or_default();
                let tail = if full.len() > 2000 { &full[full.len() - 2000..] } else { &full };
                crate::api::routes::strip_ansi_codes(tail)
            };
            let events = self.agent_registry.detect_events(at, &tail_text);

            if !events.is_empty() {
                log::debug!("[agent] Events detected: {:?} for pty {}", events, &pty_id[..pty_id.len().min(8)]);
            }

            for event in &events {
                self.handle_agent_event(pty_id, pid, at, event, prev_status);
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

                // Parse prompt options from output buffer via provider
                {
                    let buf = self.output_buf.lock();
                    let output_text = buf.get_full_text(pty_id).unwrap_or_default();
                    let tail = if output_text.len() > 2000 {
                        &output_text[output_text.len() - 2000..]
                    } else {
                        &output_text
                    };
                    let clean_tail = crate::api::routes::strip_ansi_codes(tail);
                    if let Some((question, opts)) = self.agent_registry.parse_prompt(agent_type, &clean_tail) {
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

                // Notify once — OS notification + webhook + frontend
                let title = self.mux.lock().get_pane(pane_id).map(|p| p.title.clone());
                if let Some(title) = title {
                    let notif_title = format!("{} needs approval", title);
                    self.notify_event("notification-created", serde_json::json!({
                        "id": uuid::Uuid::new_v4().to_string(),
                        "title": &notif_title,
                        "body": "Agent is waiting for your permission",
                        "level": "Warning",
                        "pane_id": pane_id,
                        "workspace_id": serde_json::Value::Null,
                    }));
                    self.notify_event("agent.permission_request", serde_json::json!({
                        "pane_id": pane_id, "title": &title,
                    }));
                }
            }
            AgentEvent::TaskComplete => {
                if prev_status == PaneStatus::Working {
                    let title = {
                        self.mux.lock().get_pane(pane_id).map(|p| p.title.clone())
                    };
                    if let Some(title) = title {
                        let notif_title = format!("{} finished", title);
                        self.notify_event("notification-created", serde_json::json!({
                            "id": uuid::Uuid::new_v4().to_string(),
                            "title": &notif_title,
                            "body": "Task complete",
                            "level": "Info",
                            "pane_id": pane_id,
                            "workspace_id": serde_json::Value::Null,
                        }));
                        self.notify_event("agent.task_complete", serde_json::json!({
                            "pane_id": pane_id, "title": &title,
                        }));
                    }
                }
            }
        }
    }
}
