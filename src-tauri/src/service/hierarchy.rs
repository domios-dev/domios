//! Agent hierarchy: roles, reports, status queries.

use serde::Serialize;
use tauri::Emitter;

use crate::agent::status::PaneStatusTracker;
use crate::multiplexer::types::AgentRole;
use super::{ReportResult, ServiceContext};

impl ServiceContext {
    pub fn set_pane_role(
        &self,
        pane_id: &str,
        role_str: &str,
        parent_pane_id: Option<&str>,
        can_spawn: Option<bool>,
    ) -> Result<(), String> {
        let role = Self::parse_role(role_str)?;
        let mut mux = self.mux.lock();
        mux.set_pane_role(pane_id, role, parent_pane_id)?;
        if let Some(cs) = can_spawn {
            if let Err(e) = mux.set_can_spawn(pane_id, cs) {
                log::warn!("[hierarchy] set_can_spawn({}) failed: {}", pane_id, e);
            }
        }
        drop(mux);
        self.emit_state_changed();
        Ok(())
    }

    pub fn clear_pane_role(&self, pane_id: &str) -> Result<(), String> {
        self.mux.lock().clear_pane_role(pane_id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn set_can_spawn(&self, pane_id: &str, can_spawn: bool) -> Result<(), String> {
        self.mux.lock().set_can_spawn(pane_id, can_spawn)?;
        self.emit_state_changed();
        Ok(())
    }

    /// Send report to direct parent (not orchestrator). This enables cascade reporting:
    /// Worker → Lead → Orchestrator.
    pub fn submit_report(&self, pane_id: &str, message: &str) -> Result<ReportResult, String> {
        let (from_title, target_id) = {
            let mux = self.mux.lock();
            let pane = mux.get_pane(pane_id).ok_or("Pane not found")?;
            let title = pane.title.clone();
            // Report goes to direct parent, then report_target (cron creator), then orchestrator
            let target = pane.parent_pane_id.clone()
                .or_else(|| pane.report_target_pane_id.clone())
                .unwrap_or_else(|| {
                    find_orchestrator(&mux, pane_id).unwrap_or_default()
                });
            if target.is_empty() {
                return Err("No parent or orchestrator found".to_string());
            }
            (title, target)
        };

        let report = self
            .report_store
            .lock()
            .add(&target_id, pane_id, &from_title, message);

        // Broadcast webhook + Tauri event
        self.events.broadcast(
            "agent.report",
            serde_json::json!({
                "target_pane_id": target_id,
                "report": report,
            }),
        );
        let _ = self.app_handle.emit(
            "agent-report",
            serde_json::json!({
                "target_pane_id": target_id,
                "from_pane_id": pane_id,
                "from_title": from_title,
                "message": message,
            }),
        );

        // Auto-push report into parent's PTY so Claude sees it immediately.
        {
            let notify_msg = format!(
                "[Report from {}] {}\r",
                from_title, message
            );
            if let Ok(pty_id) = self.resolve_pty_id(&target_id) {
                let _ = self.pty.lock().write(&pty_id, notify_msg.as_bytes());
            }
        }

        Ok(ReportResult {
            report_id: report.id,
            orchestrator_pane_id: target_id,
        })
    }

    pub fn get_reports(&self, pane_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(pane_id, false);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_and_clear_reports(&self, pane_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(pane_id, true);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_pane_status(&self, pane_id: &str) -> Result<PaneStatusInfo, String> {
        let mux = self.mux.lock();
        let pane = mux
            .get_pane(pane_id)
            .ok_or(format!("Pane not found: {}", pane_id))?;

        let status = if let Some(ref pty_id) = pane.pty_id {
            let tracker = self.agent_tracker.lock();
            format!("{:?}", tracker.get_status(pty_id))
        } else {
            "Idle".to_string()
        };

        let children = mux.get_children_ids(pane_id);

        Ok(PaneStatusInfo {
            pane_id: pane.id.clone(),
            status,
            agent_type: pane.agent_type.clone(),
            role: pane.role,
            parent_pane_id: pane.parent_pane_id.clone(),
            can_spawn: pane.can_spawn,
            depth: pane.depth,
            children,
            cwd: pane.cwd.clone(),
            title: pane.title.clone(),
        })
    }

    pub fn list_agents(&self) -> Vec<serde_json::Value> {
        let mux = self.mux.lock();
        let tracker = self.agent_tracker.lock();
        let snap = mux.snapshot();
        snap.panes
            .values()
            .filter(|p| p.role.is_some() || p.agent_type.is_some())
            .map(|p| {
                let status = p
                    .pty_id
                    .as_ref()
                    .map(|pid| format!("{:?}", tracker.get_status(pid)))
                    .unwrap_or_else(|| "Idle".to_string());
                serde_json::json!({
                    "pane_id": p.id,
                    "title": p.title,
                    "agent_type": p.agent_type,
                    "role": p.role,
                    "parent_pane_id": p.parent_pane_id,
                    "can_spawn": p.can_spawn,
                    "depth": p.depth,
                    "status": status,
                    "cwd": p.cwd,
                })
            })
            .collect()
    }

    pub fn get_hierarchy(&self) -> Vec<serde_json::Value> {
        let mux = self.mux.lock();
        let tracker = self.agent_tracker.lock();
        let snap = mux.snapshot();

        fn build_node(
            pane_id: &str,
            snap: &crate::multiplexer::state::MultiplexerSnapshot,
            tracker: &PaneStatusTracker,
        ) -> Option<serde_json::Value> {
            let pane = snap.panes.get(pane_id)?;
            let status = pane
                .pty_id
                .as_ref()
                .map(|pid| format!("{:?}", tracker.get_status(pid)))
                .unwrap_or_else(|| "Idle".to_string());
            let children: Vec<serde_json::Value> = snap
                .panes
                .values()
                .filter(|p| p.parent_pane_id.as_deref() == Some(pane_id))
                .filter_map(|p| build_node(&p.id, snap, tracker))
                .collect();
            Some(serde_json::json!({
                "pane_id": pane.id,
                "title": pane.title,
                "role": pane.role,
                "depth": pane.depth,
                "status": status,
                "agent_type": pane.agent_type,
                "can_spawn": pane.can_spawn,
                "children": children,
            }))
        }

        snap.panes
            .values()
            .filter(|p| p.role == Some(AgentRole::Orchestrator))
            .filter_map(|p| build_node(&p.id, &snap, &tracker))
            .collect()
    }

    pub fn get_pane_children(&self, pane_id: &str) -> Vec<serde_json::Value> {
        let mux = self.mux.lock();
        let child_ids = mux.get_children_ids(pane_id);
        child_ids
            .iter()
            .filter_map(|cid| {
                let p = mux.get_pane(cid)?;
                Some(serde_json::json!({
                    "pane_id": p.id,
                    "title": p.title,
                    "role": p.role,
                    "depth": p.depth,
                    "agent_type": p.agent_type,
                }))
            })
            .collect()
    }
}

/// Walk parent chain to find an orchestrator. Max 3 hops.
fn find_orchestrator(
    mux: &crate::multiplexer::state::MultiplexerState,
    start_pane_id: &str,
) -> Result<String, String> {
    let mut current_id = start_pane_id.to_string();
    for _ in 0..3 {
        let pane = mux.get_pane(&current_id);
        match pane {
            Some(p) if p.role == Some(AgentRole::Orchestrator) => return Ok(current_id),
            Some(p) if p.parent_pane_id.is_some() => {
                current_id = p.parent_pane_id.clone().unwrap();
            }
            _ => break,
        }
    }
    Err("No orchestrator found in hierarchy".to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneStatusInfo {
    pub pane_id: String,
    pub status: String,
    pub agent_type: Option<String>,
    pub role: Option<AgentRole>,
    pub parent_pane_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
    pub children: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
}

#[cfg(test)]
mod tests {
    // ── 5. Report message format ──
    // Tests the format string used in submit_report (line 85-87):
    //   format!("[Report from {}] {}\r", from_title, message)

    fn format_report(from_title: &str, message: &str) -> String {
        format!("[Report from {}] {}\r", from_title, message)
    }

    #[test]
    fn report_format_basic() {
        let msg = format_report("Scout", "Task completed successfully");
        assert_eq!(msg, "[Report from Scout] Task completed successfully\r");
    }

    #[test]
    fn report_format_ends_with_cr() {
        let msg = format_report("Worker-1", "done");
        assert!(msg.ends_with('\r'), "Report message must end with \\r for PTY");
    }

    #[test]
    fn report_format_message_with_newlines() {
        // BASELINE: newlines inside message are NOT escaped — they pass through raw
        let msg = format_report("Lead", "line1\nline2\nline3");
        assert_eq!(msg, "[Report from Lead] line1\nline2\nline3\r");
        assert!(msg.contains('\n'), "Internal newlines are preserved");
        assert!(msg.ends_with('\r'), "Still ends with \\r");
    }

    #[test]
    fn report_format_empty_message() {
        let msg = format_report("Agent", "");
        assert_eq!(msg, "[Report from Agent] \r");
    }

    #[test]
    fn report_format_title_with_spaces() {
        let msg = format_report("My Agent Title", "ok");
        assert_eq!(msg, "[Report from My Agent Title] ok\r");
    }

    #[test]
    fn report_format_message_with_special_chars() {
        let msg = format_report("Bot", "found 3 files: a.rs, b.rs, c.rs");
        assert!(msg.starts_with("[Report from Bot] "));
    }
}
