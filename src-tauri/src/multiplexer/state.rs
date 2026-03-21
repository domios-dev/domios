use std::collections::HashMap;

use serde::Serialize;
use uuid::Uuid;

use super::types::{AgentRole, Orientation, Pane, PaneType, SplitNode, Tab, Workspace};

/// Lightweight workspace summary for listing.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub tab_count: usize,
    pub is_active: bool,
}

/// Central multiplexer state: workspaces, tabs, panes, and their relationships.
pub struct MultiplexerState {
    workspaces: Vec<Workspace>,
    panes: HashMap<String, Pane>,
    active_workspace_id: Option<String>,
    focused_pane_id: Option<String>,
}

impl MultiplexerState {
    /// Create a new state with a default "Main" workspace, one tab, and one pane.
    /// Returns (state, initial_pane_id).
    pub fn new() -> (Self, String) {
        let pane_id = Uuid::new_v4().to_string();
        let tab_id = Uuid::new_v4().to_string();
        let workspace_id = Uuid::new_v4().to_string();

        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            pane_type: PaneType::Terminal,
            title: "Terminal".to_string(),
            cwd: None,
            url: None,
            agent_type: None,
            claude_session_id: None,
            role: None,
            parent_pane_id: None,
            can_spawn: false,
            depth: 0,
        };

        let tab = Tab {
            id: tab_id.clone(),
            title: "Tab 1".to_string(),
            split_tree: SplitNode::Leaf {
                pane_id: pane_id.clone(),
            },
        };

        let workspace = Workspace {
            id: workspace_id.clone(),
            name: "Main".to_string(),
            tabs: vec![tab],
            active_tab_id: Some(tab_id),
        };

        let mut panes = HashMap::new();
        panes.insert(pane_id.clone(), pane);

        let state = Self {
            workspaces: vec![workspace],
            panes,
            active_workspace_id: Some(workspace_id),
            focused_pane_id: Some(pane_id.clone()),
        };

        (state, pane_id)
    }

    // ── Workspace ops ────────────────────────────────────────────────

    pub fn create_workspace(&mut self, name: String) -> WorkspaceInfo {
        let pane_id = self.create_pane();
        let tab_id = Uuid::new_v4().to_string();
        let workspace_id = Uuid::new_v4().to_string();

        let tab = Tab {
            id: tab_id.clone(),
            title: "Tab 1".to_string(),
            split_tree: SplitNode::Leaf {
                pane_id: pane_id.clone(),
            },
        };

        let workspace = Workspace {
            id: workspace_id.clone(),
            name: name.clone(),
            tabs: vec![tab],
            active_tab_id: Some(tab_id),
        };

        self.workspaces.push(workspace);

        WorkspaceInfo {
            id: workspace_id,
            name,
            tab_count: 1,
            is_active: false,
        }
    }

    pub fn get_workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn get_active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace_id
            .as_ref()
            .and_then(|id| self.get_workspace(id))
    }

    pub fn set_active_workspace(&mut self, id: &str) -> Result<(), String> {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return Err(format!("Workspace not found: {}", id));
        }
        self.active_workspace_id = Some(id.to_string());
        Ok(())
    }

    /// Close a workspace and return PTY IDs that need to be killed.
    pub fn close_workspace(&mut self, id: &str) -> Result<Vec<String>, String> {
        let idx = self
            .workspaces
            .iter()
            .position(|w| w.id == id)
            .ok_or_else(|| format!("Workspace not found: {}", id))?;

        let workspace = self.workspaces.remove(idx);

        // Collect all pane IDs from all tabs
        let pane_ids: Vec<String> = workspace
            .tabs
            .iter()
            .flat_map(|tab| tab.split_tree.collect_pane_ids())
            .collect();

        // Collect PTY IDs before removing panes
        let pty_ids: Vec<String> = pane_ids
            .iter()
            .filter_map(|pid| self.panes.get(pid))
            .filter_map(|pane| pane.pty_id.clone())
            .collect();

        // Remove panes from the pane registry
        for pid in &pane_ids {
            self.panes.remove(pid);
        }

        // If the closed workspace was active, switch to another
        if self.active_workspace_id.as_deref() == Some(id) {
            self.active_workspace_id = self.workspaces.first().map(|w| w.id.clone());
        }

        // Clear focused pane if it was in this workspace
        if let Some(ref focused) = self.focused_pane_id {
            if pane_ids.contains(focused) {
                self.focused_pane_id = None;
            }
        }

        Ok(pty_ids)
    }

    /// Rename a workspace.
    pub fn rename_workspace(&mut self, id: &str, name: &str) -> Result<(), String> {
        let ws = self.workspaces.iter_mut()
            .find(|w| w.id == id)
            .ok_or_else(|| format!("Workspace not found: {}", id))?;
        ws.name = name.to_string();
        Ok(())
    }

    /// Move a tab from one workspace to another. Returns error if not found.
    pub fn move_tab(&mut self, tab_id: &str, from_workspace_id: &str, to_workspace_id: &str) -> Result<(), String> {
        if from_workspace_id == to_workspace_id {
            return Ok(());
        }

        // Find and remove tab from source workspace
        let from_ws = self.workspaces.iter_mut()
            .find(|w| w.id == from_workspace_id)
            .ok_or_else(|| format!("Source workspace not found: {}", from_workspace_id))?;

        let tab_idx = from_ws.tabs.iter()
            .position(|t| t.id == tab_id)
            .ok_or_else(|| format!("Tab not found: {}", tab_id))?;

        // Don't allow moving the last tab
        if from_ws.tabs.len() <= 1 {
            return Err("Cannot move the last tab from a workspace".to_string());
        }

        let tab = from_ws.tabs.remove(tab_idx);

        // Fix active tab in source if needed
        if from_ws.active_tab_id.as_deref() == Some(tab_id) {
            from_ws.active_tab_id = from_ws.tabs.last().map(|t| t.id.clone());
        }

        // Add tab to destination workspace
        let to_ws = self.workspaces.iter_mut()
            .find(|w| w.id == to_workspace_id)
            .ok_or_else(|| {
                // Tab was already removed — this is bad, but shouldn't happen in practice
                format!("Destination workspace not found: {}", to_workspace_id)
            })?;

        to_ws.tabs.push(tab);
        to_ws.active_tab_id = Some(tab_id.to_string());

        Ok(())
    }

    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.workspaces
            .iter()
            .map(|w| WorkspaceInfo {
                id: w.id.clone(),
                name: w.name.clone(),
                tab_count: w.tabs.len(),
                is_active: self.active_workspace_id.as_deref() == Some(&w.id),
            })
            .collect()
    }

    // ── Tab ops ──────────────────────────────────────────────────────

    /// Create a new tab in the given workspace. Returns (tab_id, new_pane_id).
    pub fn create_tab(&mut self, workspace_id: &str) -> Result<(String, String), String> {
        let pane_id = self.create_pane();
        let tab_id = Uuid::new_v4().to_string();

        let tab_number = {
            let ws = self
                .workspaces
                .iter()
                .find(|w| w.id == workspace_id)
                .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
            ws.tabs.len() + 1
        };

        let tab = Tab {
            id: tab_id.clone(),
            title: format!("Tab {}", tab_number),
            split_tree: SplitNode::Leaf {
                pane_id: pane_id.clone(),
            },
        };

        let workspace = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

        workspace.tabs.push(tab);
        workspace.active_tab_id = Some(tab_id.clone());

        Ok((tab_id, pane_id))
    }

    /// Close a tab and return PTY IDs that need to be killed.
    pub fn close_tab(
        &mut self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<Vec<String>, String> {
        let workspace = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

        let tab_idx = workspace
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .ok_or_else(|| format!("Tab not found: {}", tab_id))?;

        let tab = workspace.tabs.remove(tab_idx);
        let pane_ids = tab.split_tree.collect_pane_ids();

        // Collect PTY IDs before removing panes
        let pty_ids: Vec<String> = pane_ids
            .iter()
            .filter_map(|pid| self.panes.get(pid))
            .filter_map(|pane| pane.pty_id.clone())
            .collect();

        // Remove panes
        for pid in &pane_ids {
            self.panes.remove(pid);
        }

        // Update active tab
        if workspace.active_tab_id.as_deref() == Some(tab_id) {
            workspace.active_tab_id = workspace.tabs.last().map(|t| t.id.clone());
        }

        // Clear focused pane if it was in this tab
        if let Some(ref focused) = self.focused_pane_id {
            if pane_ids.contains(focused) {
                self.focused_pane_id = None;
            }
        }

        Ok(pty_ids)
    }

    pub fn set_active_tab(&mut self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        let workspace = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;

        if !workspace.tabs.iter().any(|t| t.id == tab_id) {
            return Err(format!("Tab not found: {}", tab_id));
        }

        workspace.active_tab_id = Some(tab_id.to_string());
        Ok(())
    }

    // ── Pane ops ─────────────────────────────────────────────────────

    /// Create a new terminal pane and register it. Returns the pane ID.
    pub fn create_pane(&mut self) -> String {
        let pane_id = Uuid::new_v4().to_string();
        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            pane_type: PaneType::Terminal,
            title: "Terminal".to_string(),
            cwd: None,
            url: None,
            agent_type: None,
            claude_session_id: None,
            role: None,
            parent_pane_id: None,
            can_spawn: false,
            depth: 0,
        };
        self.panes.insert(pane_id.clone(), pane);
        pane_id
    }

    /// Create a new browser pane and register it. Returns the pane ID.
    pub fn create_browser_pane(&mut self, url: Option<String>) -> String {
        let pane_id = Uuid::new_v4().to_string();
        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            pane_type: PaneType::Browser,
            title: "Browser".to_string(),
            cwd: None,
            url,
            agent_type: None,
            claude_session_id: None,
            role: None,
            parent_pane_id: None,
            can_spawn: false,
            depth: 0,
        };
        self.panes.insert(pane_id.clone(), pane);
        pane_id
    }

    /// Open a browser pane alongside the focused pane (or in a new tab).
    /// Returns the new pane ID.
    pub fn open_browser(&mut self, url: Option<String>) -> Result<String, String> {
        let new_pane_id = self.create_browser_pane(url);

        // Try to split alongside the focused pane
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

        // Fallback: add as new tab in active workspace
        let ws_id = self.active_workspace_id.clone()
            .ok_or_else(|| "No active workspace".to_string())?;

        let tab_id = Uuid::new_v4().to_string();
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

    /// Rename a pane and its containing tab.
    pub fn rename_pane(&mut self, pane_id: &str, title: &str) -> Result<(), String> {
        let pane = self.panes.get_mut(pane_id)
            .ok_or_else(|| format!("Pane not found: {}", pane_id))?;
        pane.title = title.to_string();

        // Also update tab title if this pane is in a tab
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

    // ── Split ops ────────────────────────────────────────────────────

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

        // Find the tab containing this pane and mutate its split tree
        let found = self.workspaces.iter_mut().any(|ws| {
            ws.tabs
                .iter_mut()
                .any(|tab| tab.split_tree.split_leaf(pane_id, &new_pane_id, orientation))
        });

        if !found {
            // Cleanup: remove the pane we just created
            self.panes.remove(&new_pane_id);
            return Err(format!("Pane {} not found in any split tree", pane_id));
        }

        Ok(new_pane_id)
    }

    /// Close a pane: remove from split tree and pane registry.
    /// Returns the pty_id that should be killed (if the pane had one).
    pub fn close_pane(&mut self, pane_id: &str) -> Result<Option<String>, String> {
        // Get the PTY ID before removing
        let pty_id = self
            .panes
            .get(pane_id)
            .and_then(|p| p.pty_id.clone());

        // Remove from the split tree
        let found = self.workspaces.iter_mut().any(|ws| {
            ws.tabs
                .iter_mut()
                .any(|tab| tab.split_tree.remove_leaf(pane_id))
        });

        if !found {
            return Err(format!("Pane {} not found in any split tree", pane_id));
        }

        // Remove from pane registry
        self.panes.remove(pane_id);

        // Orphan any children that pointed to this pane
        self.orphan_children(pane_id);

        // Clear focus if this was the focused pane
        if self.focused_pane_id.as_deref() == Some(pane_id) {
            self.focused_pane_id = None;
        }

        Ok(pty_id)
    }

    /// Link a pane to a PTY instance.
    pub fn link_pty(&mut self, pane_id: &str, pty_id: String) {
        if let Some(pane) = self.panes.get_mut(pane_id) {
            pane.pty_id = Some(pty_id);
        }
    }

    // ── Agent Hierarchy ──────────────────────────────────────────────

    /// Set a pane's orchestration role with validation.
    pub fn set_pane_role(
        &mut self,
        pane_id: &str,
        role: AgentRole,
        parent_pane_id: Option<&str>,
    ) -> Result<(), String> {
        // Pane must exist
        if !self.panes.contains_key(pane_id) {
            return Err(format!("Pane {} not found", pane_id));
        }

        match role {
            AgentRole::Conductor => {
                // Conductor: no parent, depth 1
                if parent_pane_id.is_some() {
                    return Err("Conductor cannot have a parent".to_string());
                }
                let pane = self.panes.get_mut(pane_id).unwrap();
                pane.role = Some(AgentRole::Conductor);
                pane.parent_pane_id = None;
                pane.can_spawn = true;
                pane.depth = 1;
            }
            AgentRole::Worker => {
                let parent_id = parent_pane_id
                    .ok_or("Worker requires a parent_pane_id")?;
                let parent = self.panes.get(parent_id)
                    .ok_or(format!("Parent pane {} not found", parent_id))?;
                if parent.role != Some(AgentRole::Conductor) {
                    return Err("Worker's parent must be a Conductor".to_string());
                }
                if !parent.can_spawn {
                    return Err("Parent conductor does not allow spawning".to_string());
                }
                let pane = self.panes.get_mut(pane_id).unwrap();
                pane.role = Some(AgentRole::Worker);
                pane.parent_pane_id = Some(parent_id.to_string());
                pane.can_spawn = false;
                pane.depth = 2;
            }
            AgentRole::SubWorker => {
                let parent_id = parent_pane_id
                    .ok_or("SubWorker requires a parent_pane_id")?;
                let parent = self.panes.get(parent_id)
                    .ok_or(format!("Parent pane {} not found", parent_id))?;
                if parent.role != Some(AgentRole::Worker) {
                    return Err("SubWorker's parent must be a Worker".to_string());
                }
                if !parent.can_spawn {
                    return Err("Parent worker does not allow spawning".to_string());
                }
                let pane = self.panes.get_mut(pane_id).unwrap();
                pane.role = Some(AgentRole::SubWorker);
                pane.parent_pane_id = Some(parent_id.to_string());
                pane.can_spawn = false;
                pane.depth = 3;
            }
        }

        Ok(())
    }

    /// Clear a pane's orchestration role and orphan its children.
    pub fn clear_pane_role(&mut self, pane_id: &str) -> Result<(), String> {
        let pane = self.panes.get_mut(pane_id)
            .ok_or(format!("Pane {} not found", pane_id))?;
        pane.role = None;
        pane.parent_pane_id = None;
        pane.can_spawn = false;
        pane.depth = 0;
        self.orphan_children(pane_id);
        Ok(())
    }

    /// Set whether a pane can spawn child agents.
    pub fn set_can_spawn(&mut self, pane_id: &str, can_spawn: bool) -> Result<(), String> {
        let pane = self.panes.get_mut(pane_id)
            .ok_or(format!("Pane {} not found", pane_id))?;
        if pane.role.is_none() {
            return Err("Pane has no role".to_string());
        }
        // Only conductors and workers can spawn
        match pane.role {
            Some(AgentRole::Conductor) | Some(AgentRole::Worker) => {
                pane.can_spawn = can_spawn;
                Ok(())
            }
            _ => Err("Only Conductor or Worker can spawn children".to_string()),
        }
    }

    /// Get IDs of direct children of a pane.
    pub fn get_children_ids(&self, pane_id: &str) -> Vec<String> {
        self.panes.values()
            .filter(|p| p.parent_pane_id.as_deref() == Some(pane_id))
            .map(|p| p.id.clone())
            .collect()
    }

    /// Remove role/parent from all children that point to the given pane.
    fn orphan_children(&mut self, pane_id: &str) {
        let child_ids: Vec<String> = self.get_children_ids(pane_id);
        for cid in &child_ids {
            // Recursively orphan grandchildren first
            self.orphan_children(cid);
            if let Some(child) = self.panes.get_mut(cid) {
                child.role = None;
                child.parent_pane_id = None;
                child.can_spawn = false;
                child.depth = 0;
            }
        }
    }

    // ── Snapshot ─────────────────────────────────────────────────────

    /// Return all data for frontend hydration.
    pub fn snapshot(&self) -> MultiplexerSnapshot {
        MultiplexerSnapshot {
            workspaces: self.workspaces.clone(),
            panes: self.panes.clone(),
            active_workspace_id: self.active_workspace_id.clone(),
            focused_pane_id: self.focused_pane_id.clone(),
        }
    }
}

impl MultiplexerState {
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
                    let claude_session_id = pane.and_then(|p| p.claude_session_id.clone());
                    let role = pane.and_then(|p| p.role.map(|r| format!("{:?}", r)));
                    let parent_pane_id = pane.and_then(|p| p.parent_pane_id.clone());
                    let can_spawn = pane.map(|p| p.can_spawn).unwrap_or(false);
                    let depth = pane.map(|p| p.depth).unwrap_or(0);
                    SplitNodeSnapshot::Leaf {
                        pane_id: pane_id.clone(),
                        cwd,
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
                tabs: ws.tabs.iter().map(|tab| {
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
                SplitNodeSnapshot::Leaf { pane_id, cwd, agent_type, claude_session_id, role, parent_pane_id, can_spawn, depth } => {
                    let parsed_role = role.as_deref().and_then(|r| match r {
                        "Conductor" => Some(crate::multiplexer::types::AgentRole::Conductor),
                        "Worker" => Some(crate::multiplexer::types::AgentRole::Worker),
                        "SubWorker" => Some(crate::multiplexer::types::AgentRole::SubWorker),
                        _ => None,
                    });
                    panes.insert(pane_id.clone(), Pane {
                        id: pane_id.clone(),
                        pty_id: None,
                        pane_type: PaneType::Terminal,
                        title: "Terminal".to_string(),
                        cwd: cwd.clone(),
                        url: None,
                        agent_type: agent_type.clone(),
                        claude_session_id: claude_session_id.clone(),
                        role: parsed_role,
                        parent_pane_id: parent_pane_id.clone(),
                        can_spawn: *can_spawn,
                        depth: *depth,
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
            }
        }).collect();

        MultiplexerState {
            workspaces,
            panes,
            active_workspace_id: snapshot.active_workspace_id.clone(),
            focused_pane_id: None,
        }
    }
}

/// Full state snapshot for frontend hydration.
#[derive(Debug, Clone, Serialize)]
pub struct MultiplexerSnapshot {
    pub workspaces: Vec<Workspace>,
    pub panes: HashMap<String, Pane>,
    pub active_workspace_id: Option<String>,
    pub focused_pane_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::PaneType;

    #[test]
    fn test_new_state_has_default_workspace() {
        let (state, initial_pane_id) = MultiplexerState::new();
        assert_eq!(state.list_workspaces().len(), 1);
        assert!(state.get_pane(&initial_pane_id).is_some());
        assert_eq!(
            state.list_workspaces()[0].name,
            "Main"
        );
        assert!(state.list_workspaces()[0].is_active);
    }

    #[test]
    fn test_new_state_has_focused_pane() {
        let (state, initial_pane_id) = MultiplexerState::new();
        assert_eq!(state.get_focused_pane_id(), Some(initial_pane_id.as_str()));
    }

    #[test]
    fn test_new_state_pane_is_terminal() {
        let (state, initial_pane_id) = MultiplexerState::new();
        let pane = state.get_pane(&initial_pane_id).unwrap();
        assert_eq!(pane.pane_type, PaneType::Terminal);
        assert!(pane.pty_id.is_none());
    }

    #[test]
    fn test_create_workspace() {
        let (mut state, _) = MultiplexerState::new();
        let info = state.create_workspace("Work".to_string());
        assert_eq!(info.name, "Work");
        assert_eq!(info.tab_count, 1);
        assert!(!info.is_active); // Not active yet
        assert_eq!(state.list_workspaces().len(), 2);
    }

    #[test]
    fn test_set_active_workspace() {
        let (mut state, _) = MultiplexerState::new();
        let info = state.create_workspace("Work".to_string());
        assert!(state.set_active_workspace(&info.id).is_ok());

        let workspaces = state.list_workspaces();
        let active = workspaces.iter().find(|w| w.is_active).unwrap();
        assert_eq!(active.id, info.id);
    }

    #[test]
    fn test_set_active_workspace_not_found() {
        let (mut state, _) = MultiplexerState::new();
        let result = state.set_active_workspace("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_tab() {
        let (mut state, _) = MultiplexerState::new();
        let ws_id = state.list_workspaces()[0].id.clone();

        let result = state.create_tab(&ws_id);
        assert!(result.is_ok());

        let (tab_id, pane_id) = result.unwrap();
        assert!(!tab_id.is_empty());
        assert!(!pane_id.is_empty());
        assert!(state.get_pane(&pane_id).is_some());

        let ws = state.get_workspace(&ws_id).unwrap();
        assert_eq!(ws.tabs.len(), 2);
        assert_eq!(ws.active_tab_id, Some(tab_id));
    }

    #[test]
    fn test_create_tab_invalid_workspace() {
        let (mut state, _) = MultiplexerState::new();
        let result = state.create_tab("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_split_pane() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        let result = state.split_pane(&initial_pane_id, Orientation::Horizontal);
        assert!(result.is_ok());

        let new_pane_id = result.unwrap();
        assert!(state.get_pane(&new_pane_id).is_some());

        // Verify the split tree has both panes
        let ws = state.get_active_workspace().unwrap();
        let tab = &ws.tabs[0];
        let pane_ids = tab.split_tree.collect_pane_ids();
        assert_eq!(pane_ids.len(), 2);
        assert!(pane_ids.contains(&initial_pane_id));
        assert!(pane_ids.contains(&new_pane_id));
    }

    #[test]
    fn test_split_pane_not_found() {
        let (mut state, _) = MultiplexerState::new();
        let result = state.split_pane("nonexistent", Orientation::Horizontal);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_pane_collapses_split() {
        let (mut state, initial_pane_id) = MultiplexerState::new();

        // Split to create a second pane
        let new_pane_id = state
            .split_pane(&initial_pane_id, Orientation::Horizontal)
            .unwrap();

        // Close the new pane
        let result = state.close_pane(&new_pane_id);
        assert!(result.is_ok());

        // Verify the split tree collapsed back to a single leaf
        let ws = state.get_active_workspace().unwrap();
        let tab = &ws.tabs[0];
        let pane_ids = tab.split_tree.collect_pane_ids();
        assert_eq!(pane_ids.len(), 1);
        assert_eq!(pane_ids[0], initial_pane_id);

        // New pane should be removed from the registry
        assert!(state.get_pane(&new_pane_id).is_none());
    }

    #[test]
    fn test_close_pane_returns_pty_id() {
        let (mut state, initial_pane_id) = MultiplexerState::new();

        // Split and link a PTY to the new pane
        let new_pane_id = state
            .split_pane(&initial_pane_id, Orientation::Horizontal)
            .unwrap();
        state.link_pty(&new_pane_id, "pty-123".to_string());

        let result = state.close_pane(&new_pane_id).unwrap();
        assert_eq!(result, Some("pty-123".to_string()));
    }

    #[test]
    fn test_close_pane_clears_focus() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        let new_pane_id = state
            .split_pane(&initial_pane_id, Orientation::Horizontal)
            .unwrap();

        state.set_focused_pane(&new_pane_id).unwrap();
        assert_eq!(state.get_focused_pane_id(), Some(new_pane_id.as_str()));

        state.close_pane(&new_pane_id).unwrap();
        assert_eq!(state.get_focused_pane_id(), None);
    }

    #[test]
    fn test_close_workspace_returns_pty_ids() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        state.link_pty(&initial_pane_id, "pty-abc".to_string());

        let ws_id = state.list_workspaces()[0].id.clone();
        let pty_ids = state.close_workspace(&ws_id).unwrap();
        assert_eq!(pty_ids, vec!["pty-abc"]);
        assert_eq!(state.list_workspaces().len(), 0);
    }

    #[test]
    fn test_close_workspace_switches_active() {
        let (mut state, _) = MultiplexerState::new();
        let ws2 = state.create_workspace("Second".to_string());
        let ws1_id = state.list_workspaces()[0].id.clone();

        // Close the first workspace
        state.close_workspace(&ws1_id).unwrap();

        // Second workspace should become active
        let workspaces = state.list_workspaces();
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].id, ws2.id);
        assert!(workspaces[0].is_active);
    }

    #[test]
    fn test_close_workspace_not_found() {
        let (mut state, _) = MultiplexerState::new();
        let result = state.close_workspace("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_close_tab() {
        let (mut state, _) = MultiplexerState::new();
        let ws_id = state.list_workspaces()[0].id.clone();
        let (tab2_id, tab2_pane_id) = state.create_tab(&ws_id).unwrap();

        state.link_pty(&tab2_pane_id, "pty-tab2".to_string());

        let pty_ids = state.close_tab(&ws_id, &tab2_id).unwrap();
        assert_eq!(pty_ids, vec!["pty-tab2"]);

        let ws = state.get_workspace(&ws_id).unwrap();
        assert_eq!(ws.tabs.len(), 1);
        assert!(state.get_pane(&tab2_pane_id).is_none());
    }

    #[test]
    fn test_set_focused_pane() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        let new_pane_id = state
            .split_pane(&initial_pane_id, Orientation::Horizontal)
            .unwrap();

        assert!(state.set_focused_pane(&new_pane_id).is_ok());
        assert_eq!(state.get_focused_pane_id(), Some(new_pane_id.as_str()));
    }

    #[test]
    fn test_set_focused_pane_not_found() {
        let (mut state, _) = MultiplexerState::new();
        let result = state.set_focused_pane("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_nested_splits() {
        let (mut state, initial_pane_id) = MultiplexerState::new();

        // Split right
        let pane_2 = state
            .split_pane(&initial_pane_id, Orientation::Horizontal)
            .unwrap();

        // Split pane_2 vertically (top/bottom)
        let pane_3 = state
            .split_pane(&pane_2, Orientation::Vertical)
            .unwrap();

        // Should have 3 panes in the tree
        let ws = state.get_active_workspace().unwrap();
        let pane_ids = ws.tabs[0].split_tree.collect_pane_ids();
        assert_eq!(pane_ids.len(), 3);
        assert!(pane_ids.contains(&initial_pane_id));
        assert!(pane_ids.contains(&pane_2));
        assert!(pane_ids.contains(&pane_3));
    }

    #[test]
    fn test_link_pty() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        state.link_pty(&initial_pane_id, "pty-xyz".to_string());

        let pane = state.get_pane(&initial_pane_id).unwrap();
        assert_eq!(pane.pty_id, Some("pty-xyz".to_string()));
    }

    #[test]
    fn test_snapshot() {
        let (state, initial_pane_id) = MultiplexerState::new();
        let snap = state.snapshot();
        assert_eq!(snap.workspaces.len(), 1);
        assert!(snap.panes.contains_key(&initial_pane_id));
        assert!(snap.active_workspace_id.is_some());
        assert_eq!(snap.focused_pane_id, Some(initial_pane_id));
    }

    #[test]
    fn test_get_pane_mut() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        {
            let pane = state.get_pane_mut(&initial_pane_id).unwrap();
            pane.title = "Custom Title".to_string();
        }
        let pane = state.get_pane(&initial_pane_id).unwrap();
        assert_eq!(pane.title, "Custom Title");
    }

    #[test]
    fn test_set_active_tab() {
        let (mut state, _) = MultiplexerState::new();
        let ws_id = state.list_workspaces()[0].id.clone();
        let ws = state.get_workspace(&ws_id).unwrap();
        let tab1_id = ws.tabs[0].id.clone();

        let (tab2_id, _) = state.create_tab(&ws_id).unwrap();

        // Switch back to tab 1
        assert!(state.set_active_tab(&ws_id, &tab1_id).is_ok());
        let ws = state.get_workspace(&ws_id).unwrap();
        assert_eq!(ws.active_tab_id, Some(tab1_id));

        // Invalid tab
        assert!(state.set_active_tab(&ws_id, "nonexistent").is_err());
        // Invalid workspace
        assert!(state.set_active_tab("nonexistent", &tab2_id).is_err());
    }
}
