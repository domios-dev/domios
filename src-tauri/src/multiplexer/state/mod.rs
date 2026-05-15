use std::collections::HashMap;

use serde::Serialize;
use uuid::Uuid;

use super::types::{AgentRole, Pane, PaneType, SplitNode, Tab, TitleLevel, Workspace};

mod alias;
mod names;
mod pane;
mod role;
mod snapshot;
mod tab;
mod workspace;

#[cfg(test)]
mod tests;

/// Generate a short 8-char hex ID (from UUID v4).
fn short_id() -> String {
    let uuid = Uuid::new_v4();
    let bytes = uuid.as_bytes();
    format!("{:02x}{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2], bytes[3])
}

/// Lightweight workspace summary for listing.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub tab_count: usize,
    pub is_active: bool,
    pub cwd: Option<String>,
    pub protected: bool,
    pub pane_ids: Vec<String>,
}

/// Central multiplexer state: workspaces, tabs, panes, and their relationships.
pub struct MultiplexerState {
    workspaces: Vec<Workspace>,
    panes: HashMap<String, Pane>,
    active_workspace_id: Option<String>,
    focused_pane_id: Option<String>,
    /// Reverse lookup: pty_id -> pane_id for O(1) resolution.
    pty_to_pane: HashMap<String, String>,
    /// Alias registry: human-readable name -> pane_id.
    pane_aliases: HashMap<String, String>,
}

/// Full state snapshot for frontend hydration.
#[derive(Debug, Clone, Serialize)]
pub struct MultiplexerSnapshot {
    pub workspaces: Vec<Workspace>,
    pub panes: HashMap<String, Pane>,
    pub active_workspace_id: Option<String>,
    pub focused_pane_id: Option<String>,
}

impl MultiplexerState {
    /// Create an empty state with no workspaces.
    /// Used when General Agent will create its own workspace.
    pub fn empty() -> Self {
        Self {
            workspaces: vec![],
            panes: HashMap::new(),
            active_workspace_id: None,
            focused_pane_id: None,
            pty_to_pane: HashMap::new(),
            pane_aliases: HashMap::new(),
        }
    }

    /// Create a new state with a default "Main" workspace, one tab, and one pane.
    /// Returns (state, initial_pane_id).
    pub fn new() -> (Self, String) {
        let pane_id = short_id();
        let tab_id = short_id();
        let workspace_id = short_id();

        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            pane_type: PaneType::Terminal,
            title: "Terminal".to_string(),
            cwd: None,
            url: None,
            agent_type: None,
            agent_type_locked: false,
            agent_session_id: None,
            agent_session_name: None,
            role: Some(AgentRole::Orchestrator),
            parent_pane_id: None,
            can_spawn: true,
            depth: 1,
            report_target_pane_id: None,
            spawned_by_pane_id: None,
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            auto_approve: false,
            protected: false,
            connected_services: vec![],
            agent_set_at: None,
            hidden: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
        };

        let tab = Tab {
            id: tab_id.clone(),
            title: names::generate_tab_name(),
            split_tree: SplitNode::Leaf {
                pane_id: pane_id.clone(),
            },
        };

        let workspace = Workspace {
            id: workspace_id.clone(),
            name: "Main".to_string(),
            tabs: vec![tab],
            active_tab_id: Some(tab_id),
            cwd: None,
            protected: false,
        };

        let mut panes = HashMap::new();
        panes.insert(pane_id.clone(), pane);

        let state = Self {
            workspaces: vec![workspace],
            panes,
            active_workspace_id: Some(workspace_id),
            focused_pane_id: Some(pane_id.clone()),
            pty_to_pane: HashMap::new(),
            pane_aliases: HashMap::new(),
        };

        (state, pane_id)
    }
}
