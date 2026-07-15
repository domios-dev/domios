use serde::Serialize;

use crate::multiplexer::types::AgentRole;
use crate::service::ServiceContext;

#[derive(Debug, Clone, Serialize)]
pub struct PaneStatusInfo {
    pub pane_id: String,
    pub status: String,
    pub agent_type: Option<String>,
    pub agent_session_id: Option<String>,
    pub role: Option<AgentRole>,
    pub parent_pane_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
    pub children: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
    pub team_id: Option<String>,
}

impl ServiceContext {
    pub fn get_pane_status(&self, pane_id: &str) -> Result<PaneStatusInfo, String> {
        let mux = self.mux.write();
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
            agent_session_id: pane.agent_session_id.clone(),
            role: pane.role,
            parent_pane_id: pane.parent_pane_id.clone(),
            can_spawn: pane.can_spawn,
            depth: pane.depth,
            children,
            cwd: pane.cwd.clone(),
            title: pane.title.clone(),
            team_id: pane.team_id.clone(),
        })
    }
}
