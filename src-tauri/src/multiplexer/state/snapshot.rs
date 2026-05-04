use std::collections::HashMap;

use super::{MultiplexerState, MultiplexerSnapshot};
use crate::multiplexer::types::{Orientation, Pane, PaneType, SplitNode, Tab, TitleLevel, Workspace};

impl MultiplexerState {
    /// Return all data for frontend hydration.
    pub fn snapshot(&self) -> MultiplexerSnapshot {
        MultiplexerSnapshot {
            workspaces: self.workspaces.clone(),
            panes: self.panes.clone(),
            active_workspace_id: self.active_workspace_id.clone(),
            focused_pane_id: self.focused_pane_id.clone(),
        }
    }

    /// Create a session snapshot for persistence.
    pub fn to_session_snapshot(&self) -> crate::session::store::SessionSnapshot {
        use crate::session::store::*;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn split_to_snapshot(node: &SplitNode, panes: &HashMap<String, Pane>) -> SplitNodeSnapshot {
            match node {
                SplitNode::Leaf { pane_id } => {
                    let pane = panes.get(pane_id);
                    let cwd = pane.and_then(|p| p.cwd.clone());
                    let agent_type = pane.and_then(|p| p.agent_type.clone());
                    let claude_session_id = pane.and_then(|p| p.agent_session_id.clone());
                    let role = pane.and_then(|p| p.role.map(|r| format!("{:?}", r)));
                    let parent_pane_id = pane.and_then(|p| p.parent_pane_id.clone());
                    let can_spawn = pane.map(|p| p.can_spawn).unwrap_or(false);
                    let depth = pane.map(|p| p.depth).unwrap_or(0);
                    let title = pane.map(|p| p.title.clone());
                    let title_level = pane.map(|p| format!("{:?}", p.title_level));
                    SplitNodeSnapshot::Leaf {
                        pane_id: pane_id.clone(),
                        cwd,
                        title,
                        title_level,
                        agent_type,
                        claude_session_id,
                        role,
                        parent_pane_id,
                        can_spawn,
                        depth,
                    }
                }
                SplitNode::Split { orientation, ratio, children } => SplitNodeSnapshot::Split {
                    orientation: format!("{:?}", orientation),
                    ratio: *ratio,
                    children: children.iter().map(|c| split_to_snapshot(c, panes)).collect(),
                },
            }
        }

        let workspaces = self.workspaces.iter().map(|ws| {
            WorkspaceSnapshot {
                id: ws.id.clone(),
                name: ws.name.clone(),
                active_tab_id: ws.active_tab_id.clone(),
                cwd: ws.cwd.clone(),
                protected: ws.protected,
                tabs: ws.tabs.iter().filter(|tab| {
                    // Skip hidden worker panes — they're ephemeral and shouldn't
                    // persist across restarts. Prevents ghost tabs in sidebar.
                    let pane_ids = tab.split_tree.collect_pane_ids();
                    !pane_ids.iter().any(|pid| {
                        self.panes.get(pid).map(|p| p.hidden).unwrap_or(false)
                    })
                }).map(|tab| {
                    TabSnapshot {
                        id: tab.id.clone(),
                        title: tab.title.clone(),
                        split_tree: split_to_snapshot(&tab.split_tree, &self.panes),
                    }
                }).collect(),
            }
        }).collect();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        SessionSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp,
            workspaces,
            active_workspace_id: self.active_workspace_id.clone(),
        }
    }

    /// Restore state from a session snapshot. Creates workspaces/tabs/panes
    /// without PTYs — frontend will spawn PTYs when panes become visible.
    pub fn from_session_snapshot(snapshot: &crate::session::store::SessionSnapshot) -> Self {
        use crate::session::store::*;

        fn snapshot_to_split(node: &SplitNodeSnapshot, panes: &mut HashMap<String, Pane>) -> SplitNode {
            match node {
                SplitNodeSnapshot::Leaf { pane_id, cwd, title, title_level, agent_type: _agent_type, claude_session_id, role, parent_pane_id, can_spawn, depth } => {
                    let parsed_role = role.as_deref().and_then(|r| match r {
                        "Orchestrator" | "Conductor" => Some(crate::multiplexer::types::AgentRole::Orchestrator),
                        "Lead" => Some(crate::multiplexer::types::AgentRole::Lead),
                        "Worker" | "SubWorker" => Some(crate::multiplexer::types::AgentRole::Worker),
                        _ => None,
                    });
                    let restored_title = title.as_deref().unwrap_or("Terminal").to_string();
                    let restored_level = match title_level.as_deref() {
                        Some("User") => TitleLevel::User,
                        Some("Agent") => TitleLevel::Agent,
                        Some("Auto") => TitleLevel::Auto,
                        _ => if role.is_some() { TitleLevel::User } else { TitleLevel::Default },
                    };
                    panes.insert(pane_id.clone(), Pane {
                        id: pane_id.clone(),
                        pty_id: None,
                        pane_type: PaneType::Terminal,
                        title: restored_title,
                        cwd: cwd.clone(),
                        url: None,
                        // Don't restore agent_type — let backend detect from PTY output.
                        // Restoring it causes false positives when Claude isn't running.
                        agent_type: None,
                        // Deduplicate: only assign session_id if no other pane already has it
                        agent_session_id: claude_session_id.as_ref().and_then(|sid| {
                            if panes.values().any(|p| p.agent_session_id.as_deref() == Some(sid.as_str())) {
                                None
                            } else {
                                Some(sid.clone())
                            }
                        }),
                        role: parsed_role,
                        parent_pane_id: parent_pane_id.clone(),
                        can_spawn: *can_spawn,
                        depth: *depth,
                        report_target_pane_id: None,
                        spawned_by_pane_id: None,
                        title_level: restored_level,
                        chat_view: false,
                        pending_prompt: None,
                        auto_approve: false,
                        protected: false,
            connected_services: vec![],
            agent_set_at: None,
            hidden: false,
            show_in_tabbar: true,
            worker_id: None,
                    });
                    SplitNode::Leaf { pane_id: pane_id.clone() }
                }
                SplitNodeSnapshot::Split { orientation, ratio, children } => {
                    let orient = if orientation == "Vertical" {
                        Orientation::Vertical
                    } else {
                        Orientation::Horizontal
                    };
                    SplitNode::Split {
                        orientation: orient,
                        ratio: *ratio,
                        children: children.iter().map(|c| snapshot_to_split(c, panes)).collect(),
                    }
                }
            }
        }

        let mut panes = HashMap::new();
        let workspaces: Vec<Workspace> = snapshot.workspaces.iter().map(|ws| {
            Workspace {
                id: ws.id.clone(),
                name: ws.name.clone(),
                active_tab_id: ws.active_tab_id.clone(),
                tabs: ws.tabs.iter().map(|tab| {
                    Tab {
                        id: tab.id.clone(),
                        title: tab.title.clone(),
                        split_tree: snapshot_to_split(&tab.split_tree, &mut panes),
                    }
                }).collect(),
                cwd: ws.cwd.clone(),
                protected: ws.protected,
            }
        }).collect();

        MultiplexerState {
            workspaces,
            panes,
            active_workspace_id: snapshot.active_workspace_id.clone(),
            focused_pane_id: None,
            pty_to_pane: HashMap::new(),
            pane_aliases: HashMap::new(),
        }
    }
}
