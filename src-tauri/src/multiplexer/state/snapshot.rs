use std::collections::HashMap;

use super::{MultiplexerState, MultiplexerSnapshot};
use crate::multiplexer::types::{Pane, PaneType, Tab, TitleLevel, Workspace};

fn derive_agent_type_from_title(title: Option<&str>) -> Option<String> {
    let t = title?.to_ascii_lowercase();
    for id in ["claude", "codex", "opencode"] {
        if t.contains(id) {
            return Some(id.to_string());
        }
    }
    None
}

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

        fn pane_to_leaf_snapshot(pane_id: &str, panes: &HashMap<String, Pane>) -> SplitNodeSnapshot {
            let pane = panes.get(pane_id);
            SplitNodeSnapshot::Leaf {
                pane_id: pane_id.to_string(),
                cwd: pane.and_then(|p| p.cwd.clone()),
                title: pane.map(|p| p.title.clone()),
                title_level: pane.map(|p| format!("{:?}", p.title_level)),
                agent_type: pane.and_then(|p| p.agent_type.clone()),
                agent_type_locked: pane.map(|p| p.agent_type_locked).unwrap_or(false),
                claude_session_id: pane.and_then(|p| p.agent_session_id.clone()),
                role: pane.and_then(|p| p.role.map(|r| format!("{:?}", r))),
                parent_pane_id: pane.and_then(|p| p.parent_pane_id.clone()),
                can_spawn: pane.map(|p| p.can_spawn).unwrap_or(false),
                depth: pane.map(|p| p.depth).unwrap_or(0),
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
                    !self.panes.get(&tab.pane_id).map(|p| p.hidden).unwrap_or(false)
                }).map(|tab| {
                    TabSnapshot {
                        id: tab.id.clone(),
                        title: tab.title.clone(),
                        split_tree: pane_to_leaf_snapshot(&tab.pane_id, &self.panes),
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
    /// Old snapshots that recorded a multi-pane split tree collapse to the
    /// first leaf; the rest is dropped with a log line since split layouts
    /// are no longer supported.
    pub fn from_session_snapshot(snapshot: &crate::session::store::SessionSnapshot) -> Self {
        use crate::session::store::*;

        /// Walk an old SplitNodeSnapshot looking for the first Leaf. Anything
        /// past the first leaf in a Split branch is discarded — those tabs
        /// will come back as a single-pane after restore.
        fn first_leaf(node: &SplitNodeSnapshot) -> Option<&SplitNodeSnapshot> {
            match node {
                SplitNodeSnapshot::Leaf { .. } => Some(node),
                SplitNodeSnapshot::Split { children, .. } => {
                    children.iter().find_map(first_leaf)
                }
            }
        }

        fn install_leaf(leaf: &SplitNodeSnapshot, panes: &mut HashMap<String, Pane>) -> Option<String> {
            let SplitNodeSnapshot::Leaf {
                pane_id, cwd, title, title_level, agent_type, agent_type_locked,
                claude_session_id, role, parent_pane_id, can_spawn, depth,
            } = leaf else { return None };

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
            let restored_session_id = claude_session_id.as_ref().and_then(|sid| {
                if panes.values().any(|p| p.agent_session_id.as_deref() == Some(sid.as_str())) {
                    None
                } else {
                    Some(sid.clone())
                }
            });
            let restored_agent_type = agent_type
                .clone()
                .or_else(|| derive_agent_type_from_title(title.as_deref()));
            let restored_locked = *agent_type_locked
                || (!*agent_type_locked && restored_agent_type.is_some());
            panes.insert(pane_id.clone(), Pane {
                id: pane_id.clone(),
                pty_id: None,
                pane_type: PaneType::Terminal,
                title: restored_title,
                cwd: cwd.clone(),
                url: None,
                agent_type: restored_agent_type,
                agent_type_locked: restored_locked,
                agent_session_id: restored_session_id,
                agent_session_name: None,
                agent_session_model: None,
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
                restored_from_snapshot: true,
            });
            Some(pane_id.clone())
        }

        let mut panes = HashMap::new();
        let workspaces: Vec<Workspace> = snapshot.workspaces.iter().map(|ws| {
            Workspace {
                id: ws.id.clone(),
                name: ws.name.clone(),
                active_tab_id: ws.active_tab_id.clone(),
                tabs: ws.tabs.iter().filter_map(|tab| {
                    let leaf = first_leaf(&tab.split_tree)?;
                    let pane_id = install_leaf(leaf, &mut panes)?;
                    if matches!(&tab.split_tree, SplitNodeSnapshot::Split { .. }) {
                        log::warn!(
                            "[restore] tab {} had a multi-pane split — kept the first leaf, dropped the rest",
                            tab.id,
                        );
                    }
                    Some(Tab {
                        id: tab.id.clone(),
                        title: tab.title.clone(),
                        pane_id,
                    })
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
