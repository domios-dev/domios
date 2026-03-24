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
            let _ = mux.set_can_spawn(pane_id, cs);
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

    /// Walk up the hierarchy to find the conductor, then store the report.
    pub fn submit_report(&self, pane_id: &str, message: &str) -> Result<ReportResult, String> {
        let (from_title, conductor_id) = {
            let mux = self.mux.lock();
            let pane = mux.get_pane(pane_id).ok_or("Pane not found")?;
            let title = pane.title.clone();
            let conductor_id = find_conductor(&mux, pane_id)?;
            (title, conductor_id)
        };

        let report = self
            .report_store
            .lock()
            .add(&conductor_id, pane_id, &from_title, message);

        // Broadcast webhook + Tauri event
        self.events.broadcast(
            "agent.report",
            serde_json::json!({
                "conductor_pane_id": conductor_id,
                "report": report,
            }),
        );
        let _ = self.app_handle.emit(
            "agent-report",
            serde_json::json!({
                "conductor_pane_id": conductor_id,
                "from_pane_id": pane_id,
                "from_title": from_title,
                "message": message,
            }),
        );

        // Auto-notify conductor: send report summary as text into conductor's PTY.
        // This way Claude in the conductor pane sees the report immediately (push, not poll).
        {
            let notify_msg = format!(
                "\n[Report from {}] {}\n",
                from_title, message
            );
            if let Ok(pty_id) = self.resolve_pty_id(&conductor_id) {
                let _ = self.pty.lock().write(&pty_id, notify_msg.as_bytes());
            }
        }

        Ok(ReportResult {
            report_id: report.id,
            conductor_pane_id: conductor_id,
        })
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
            .filter(|p| p.role == Some(AgentRole::Conductor))
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

/// Walk parent chain to find a conductor. Max 3 hops.
fn find_conductor(
    mux: &crate::multiplexer::state::MultiplexerState,
    start_pane_id: &str,
) -> Result<String, String> {
    let mut current_id = start_pane_id.to_string();
    for _ in 0..3 {
        let pane = mux.get_pane(&current_id);
        match pane {
            Some(p) if p.role == Some(AgentRole::Conductor) => return Ok(current_id),
            Some(p) if p.parent_pane_id.is_some() => {
                current_id = p.parent_pane_id.clone().unwrap();
            }
            _ => break,
        }
    }
    Err("No conductor found in hierarchy".to_string())
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
