use serde::Serialize;

use crate::multiplexer::types::AgentRole;
use crate::service::ServiceContext;

#[derive(Debug, Clone, Serialize)]
pub struct TabStatusInfo {
    pub tab_id: String,
    pub status: String,
    pub agent_type: Option<String>,
    pub agent_session_id: Option<String>,
    pub role: Option<AgentRole>,
    pub parent_tab_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
    pub children: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
    pub team_id: Option<String>,
}

impl ServiceContext {
    pub fn get_tab_status(&self, tab_id: &str) -> Result<TabStatusInfo, String> {
        let mux = self.mux.write();
        let tab = mux
            .get_tab(tab_id)
            .ok_or(format!("Tab not found: {}", tab_id))?;
        let status = if let Some(ref pty_id) = tab.pty_id {
            let tracker = self.agent_tracker.lock();
            format!("{:?}", tracker.get_status(pty_id))
        } else {
            "Idle".to_string()
        };
        let children = mux.get_children_ids(tab_id);
        Ok(TabStatusInfo {
            tab_id: tab.id.clone(),
            status,
            agent_type: tab.agent_type.clone(),
            agent_session_id: tab.agent_session_id.clone(),
            role: tab.role,
            parent_tab_id: tab.parent_tab_id.clone(),
            can_spawn: tab.can_spawn,
            depth: tab.depth,
            children,
            cwd: tab.cwd.clone(),
            title: tab.title.clone(),
            team_id: tab.team_id.clone(),
        })
    }
}
