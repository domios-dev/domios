use super::{short_id, MultiplexerState};
use crate::multiplexer::types::{Orientation, Pane, PaneType, SplitNode, Tab, TitleLevel};

impl MultiplexerState {
    /// Create a new terminal pane and register it. Returns the pane ID.
    pub fn create_pane(&mut self) -> String {
        let pane_id = short_id();
        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            pane_type: PaneType::Terminal,
            title: "Terminal".to_string(),
            cwd: None,
            url: None,
            agent_type: None,
            agent_session_id: None,
            role: None,
            parent_pane_id: None,
            can_spawn: false,
            depth: 0,
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
            project_map_workspace_id: None,
        };
        self.panes.insert(pane_id.clone(), pane);
        pane_id
    }

    /// Create a new browser pane and register it. Returns the pane ID.
    pub fn create_browser_pane(&mut self, url: Option<String>) -> String {
        let pane_id = short_id();
        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            pane_type: PaneType::Browser,
            title: "Browser".to_string(),
            cwd: None,
            url,
            agent_type: None,
            agent_session_id: None,
            role: None,
            parent_pane_id: None,
            can_spawn: false,
            depth: 0,
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
            project_map_workspace_id: None,
        };
        self.panes.insert(pane_id.clone(), pane);
        pane_id
    }

    pub fn create_project_map_pane(&mut self, workspace_id: String) -> String {
        let pane_id = short_id();
        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            pane_type: PaneType::ProjectMap,
            title: "Project Map".to_string(),
            cwd: None,
            url: None,
            agent_type: None,
            agent_session_id: None,
            role: None,
            parent_pane_id: None,
            can_spawn: false,
            depth: 0,
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
            project_map_workspace_id: Some(workspace_id),
        };
        self.panes.insert(pane_id.clone(), pane);
        pane_id
    }

    pub fn find_project_map_pane(&self, workspace_id: &str) -> Option<String> {
        self.panes.values()
            .find(|p| p.pane_type == PaneType::ProjectMap
                && p.project_map_workspace_id.as_deref() == Some(workspace_id))
            .map(|p| p.id.clone())
    }

    pub fn open_project_map(&mut self, workspace_id: &str) -> Result<String, String> {
        if let Some(existing) = self.find_project_map_pane(workspace_id) {
            self.focused_pane_id = Some(existing.clone());
            return Ok(existing);
        }

        let workspace_exists = self.workspaces.iter().any(|w| w.id == workspace_id);
        if !workspace_exists {
            return Err(format!("Workspace not found: {}", workspace_id));
        }

        let new_pane_id = self.create_project_map_pane(workspace_id.to_string());

        let tab_id = short_id();
        let tab = Tab {
            id: tab_id.clone(),
            title: "Project Map".to_string(),
            split_tree: SplitNode::Leaf {
                pane_id: new_pane_id.clone(),
            },
        };

        let workspace = self.workspaces.iter_mut()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
        workspace.tabs.push(tab);
        workspace.active_tab_id = Some(tab_id);

        self.focused_pane_id = Some(new_pane_id.clone());
        Ok(new_pane_id)
    }

    /// Open a browser pane alongside the focused pane (or in a new tab).
    /// Returns the new pane ID.
    pub fn open_browser(&mut self, url: Option<String>) -> Result<String, String> {
        let new_pane_id = self.create_browser_pane(url);

        if let Some(ref focused_id) = self.focused_pane_id.clone() {
            let found = self.workspaces.iter_mut().any(|ws| {
                ws.tabs
                    .iter_mut()
                    .any(|tab| tab.split_tree.split_leaf(focused_id, &new_pane_id, Orientation::Horizontal))
            });
            if found {
                return Ok(new_pane_id);
            }
        }

        let ws_id = self.active_workspace_id.clone()
            .ok_or_else(|| "No active workspace".to_string())?;

        let tab_id = short_id();
        let tab = Tab {
            id: tab_id.clone(),
            title: "Browser".to_string(),
            split_tree: SplitNode::Leaf {
                pane_id: new_pane_id.clone(),
            },
        };

        let workspace = self.workspaces.iter_mut()
            .find(|w| w.id == ws_id)
            .ok_or_else(|| "Active workspace not found".to_string())?;

        workspace.tabs.push(tab);
        workspace.active_tab_id = Some(tab_id);

        Ok(new_pane_id)
    }

    pub fn get_pane(&self, id: &str) -> Option<&Pane> {
        self.panes.get(id)
    }

    /// Collect agent_session_ids from all panes except the given one.
    pub fn other_session_ids(&self, exclude_pane_id: &str) -> Vec<String> {
        self.panes.values()
            .filter(|p| p.id != exclude_pane_id)
            .filter_map(|p| p.agent_session_id.clone())
            .collect()
    }

    /// Pane IDs currently holding the given agent session id (typically 0 or 1).
    pub fn pane_ids_with_session(&self, session_id: &str) -> Vec<String> {
        self.panes.values()
            .filter(|p| p.agent_session_id.as_deref() == Some(session_id))
            .map(|p| p.id.clone())
            .collect()
    }

    pub fn get_pane_mut(&mut self, id: &str) -> Option<&mut Pane> {
        self.panes.get_mut(id)
    }

    pub fn remove_pane(&mut self, id: &str) -> Option<Pane> {
        self.panes.remove(id)
    }

    pub fn set_focused_pane(&mut self, id: &str) -> Result<(), String> {
        if !self.panes.contains_key(id) {
            return Err(format!("Pane not found: {}", id));
        }
        self.focused_pane_id = Some(id.to_string());
        Ok(())
    }

    pub fn get_focused_pane_id(&self) -> Option<&str> {
        self.focused_pane_id.as_deref()
    }

    /// Resolve the General orchestrator pane (alias `"general"`, registered by
    /// `service::general_agent::workspace::ensure_general_workspace`) and set
    /// focus to it. Returns the new focused pane id, or `None` if the alias is
    /// not registered (in which case focus is left untouched — preferable to
    /// silently nulling, which is what surfaced the original "deselect to
    /// nothing" UX bug). Callers in service-layer close paths should compare
    /// pre/post focus and emit `pane-focused` if it actually changed.
    pub fn fallback_focus_to_general(&mut self) -> Option<String> {
        // Hard-coded alias mirrors `service::general_agent::GENERAL_ALIAS`.
        // Kept as a string literal to avoid pulling a service-layer constant
        // into the state layer; if the alias name ever changes, update both.
        let target = self.resolve_pane_id("general")?;
        self.focused_pane_id = Some(target.clone());
        Some(target)
    }

    /// Rename a pane and its containing tab. Pins the title to block OSC auto-rename.
    /// Rename a pane at User level (highest priority).
    pub fn rename_pane(&mut self, pane_id: &str, title: &str) -> Result<(), String> {
        self.rename_pane_at_level(pane_id, title, TitleLevel::User)
    }

    /// Rename a pane at a specific priority level.
    /// Lower levels cannot overwrite higher ones.
    pub fn rename_pane_at_level(&mut self, pane_id: &str, title: &str, level: TitleLevel) -> Result<(), String> {
        let pane = self.panes.get_mut(pane_id)
            .ok_or_else(|| format!("Pane not found: {}", pane_id))?;
        if level < pane.title_level {
            return Ok(());
        }
        pane.title = title.to_string();
        pane.title_level = level;

        for ws in &mut self.workspaces {
            for tab in &mut ws.tabs {
                let ids = tab.split_tree.collect_pane_ids();
                if ids.contains(&pane_id.to_string()) {
                    tab.title = title.to_string();
                }
            }
        }
        Ok(())
    }

    /// Find which workspace contains a given pane.
    pub fn find_workspace_for_pane(&self, pane_id: &str) -> Option<String> {
        for ws in &self.workspaces {
            for tab in &ws.tabs {
                if tab.split_tree.contains_pane(pane_id) {
                    return Some(ws.id.clone());
                }
            }
        }
        None
    }

    /// Find which tab contains a given pane.
    pub fn find_tab_for_pane(&self, pane_id: &str) -> Option<String> {
        for ws in &self.workspaces {
            for tab in &ws.tabs {
                if tab.split_tree.contains_pane(pane_id) {
                    return Some(tab.id.clone());
                }
            }
        }
        None
    }

    /// Split an existing pane. Creates a new pane next to it.
    /// Returns the new pane ID.
    pub fn split_pane(
        &mut self,
        pane_id: &str,
        orientation: Orientation,
    ) -> Result<String, String> {
        if !self.panes.contains_key(pane_id) {
            return Err(format!("Pane not found: {}", pane_id));
        }

        let new_pane_id = self.create_pane();

        let found = self.workspaces.iter_mut().any(|ws| {
            ws.tabs
                .iter_mut()
                .any(|tab| tab.split_tree.split_leaf(pane_id, &new_pane_id, orientation))
        });

        if !found {
            self.panes.remove(&new_pane_id);
            return Err(format!("Pane {} not found in any split tree", pane_id));
        }

        Ok(new_pane_id)
    }

    /// Close a pane: remove from split tree and pane registry.
    /// Cascade-closes all children. Returns all pty_ids that should be killed.
    pub fn close_pane(&mut self, pane_id: &str) -> Result<Vec<String>, String> {
        if let Some(pane) = self.panes.get(pane_id) {
            if pane.protected {
                return Err("Cannot close protected pane".to_string());
            }
        }

        let mut pty_ids = Vec::new();

        let child_ids = self.get_children_ids(pane_id);

        if let Some(pty_id) = self.panes.get(pane_id).and_then(|p| p.pty_id.clone()) {
            self.pty_to_pane.remove(&pty_id);
            pty_ids.push(pty_id);
        }

        self.remove_pane_from_tree(pane_id)?;

        self.panes.remove(pane_id);

        if self.focused_pane_id.as_deref() == Some(pane_id) {
            self.focused_pane_id = None;
            self.fallback_focus_to_general();
        }

        for cid in child_ids {
            if let Ok(child_ptys) = self.close_pane(&cid) {
                pty_ids.extend(child_ptys);
            }
        }

        Ok(pty_ids)
    }

    /// Remove a pane from its tab's split tree. Removes the tab entirely if it was the only leaf.
    fn remove_pane_from_tree(&mut self, pane_id: &str) -> Result<(), String> {
        for ws in &mut self.workspaces {
            let tab_idx = ws.tabs.iter().position(|tab| {
                matches!(&tab.split_tree, SplitNode::Leaf { pane_id: pid } if pid == pane_id)
            });
            if let Some(idx) = tab_idx {
                ws.tabs.remove(idx);
                if !ws.tabs.is_empty() {
                    ws.active_tab_id = Some(ws.tabs.last().unwrap().id.clone());
                } else {
                    ws.active_tab_id = None;
                }
                return Ok(());
            }
            if ws.tabs.iter_mut().any(|tab| tab.split_tree.remove_leaf(pane_id)) {
                return Ok(());
            }
        }
        Err(format!("Pane {} not found in any split tree", pane_id))
    }
}
