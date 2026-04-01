use std::collections::HashMap;

use serde::Serialize;
use uuid::Uuid;

use super::types::{AgentRole, Orientation, Pane, PaneType, SplitNode, Tab, TitleLevel, Workspace};

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

impl MultiplexerState {
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
            agent_session_id: None,
            role: Some(AgentRole::Orchestrator),
            parent_pane_id: None,
            can_spawn: true,
            depth: 1,
            report_target_pane_id: None,
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            auto_approve: false,
            protected: false,
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

    // ── Workspace ops ────────────────────────────────────────────────

    pub fn create_workspace(&mut self, name: String, cwd: Option<String>) -> WorkspaceInfo {
        let workspace_id = short_id();

        let workspace = Workspace {
            id: workspace_id.clone(),
            name: name.clone(),
            tabs: vec![],
            active_tab_id: None,
            cwd: cwd.clone(),
            protected: false,
        };

        self.workspaces.push(workspace);

        WorkspaceInfo {
            id: workspace_id.clone(),
            name,
            tab_count: 1,
            is_active: false,
            cwd,
            protected: false,
            pane_ids: vec![],
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

        if self.workspaces[idx].protected {
            return Err("Cannot close protected workspace".to_string());
        }

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

        // Remove panes from the pane registry and clean up reverse map
        for pid in &pane_ids {
            if let Some(pane) = self.panes.remove(pid) {
                if let Some(pty_id) = &pane.pty_id {
                    self.pty_to_pane.remove(pty_id);
                }
            }
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

    pub fn set_workspace_protected(&mut self, id: &str, protected: bool) {
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == id) {
            ws.protected = protected;
        }
    }

    pub fn move_workspace_to_front(&mut self, id: &str) {
        if let Some(idx) = self.workspaces.iter().position(|w| w.id == id) {
            if idx > 0 {
                let ws = self.workspaces.remove(idx);
                self.workspaces.insert(0, ws);
            }
        }
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
            .map(|w| {
                let pane_ids: Vec<String> = w.tabs.iter()
                    .flat_map(|t| t.split_tree.collect_pane_ids())
                    .collect();
                WorkspaceInfo {
                    id: w.id.clone(),
                    name: w.name.clone(),
                    tab_count: w.tabs.len(),
                    is_active: self.active_workspace_id.as_deref() == Some(&w.id),
                    cwd: w.cwd.clone(),
                    protected: w.protected,
                    pane_ids,
                }
            })
            .collect()
    }

    // ── Tab ops ──────────────────────────────────────────────────────

    /// Create a new tab in the given workspace. Returns (tab_id, new_pane_id).
    pub fn create_tab(&mut self, workspace_id: &str) -> Result<(String, String), String> {
        let pane_id = self.create_pane();
        let tab_id = short_id();

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
        // Only set active if this is the first tab (empty workspace)
        if workspace.active_tab_id.is_none() {
            workspace.active_tab_id = Some(tab_id.clone());
        }

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

        // Remove panes and clean up reverse map
        for pid in &pane_ids {
            if let Some(pane) = self.panes.remove(pid) {
                if let Some(pty_id) = &pane.pty_id {
                    self.pty_to_pane.remove(pty_id);
                }
            }
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
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            auto_approve: false,
            protected: false,
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
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            auto_approve: false,
            protected: false,
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
        // Only rename if new level >= current level
        if level < pane.title_level {
            return Ok(()); // silently skip — higher priority title already set
        }
        pane.title = title.to_string();
        pane.title_level = level;

        // Also update tab title
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
    /// Cascade-closes all children. Returns all pty_ids that should be killed.
    pub fn close_pane(&mut self, pane_id: &str) -> Result<Vec<String>, String> {
        if let Some(pane) = self.panes.get(pane_id) {
            if pane.protected {
                return Err("Cannot close protected pane".to_string());
            }
        }

        let mut pty_ids = Vec::new();

        // Collect children to cascade-close BEFORE removing this pane
        let child_ids = self.get_children_ids(pane_id);

        // Get the PTY ID before removing
        if let Some(pty_id) = self.panes.get(pane_id).and_then(|p| p.pty_id.clone()) {
            self.pty_to_pane.remove(&pty_id);
            pty_ids.push(pty_id);
        }

        // Remove from the split tree (and remove tabs that become empty)
        self.remove_pane_from_tree(pane_id)?;

        // Remove from pane registry
        self.panes.remove(pane_id);

        // Clear focus if this was the focused pane
        if self.focused_pane_id.as_deref() == Some(pane_id) {
            self.focused_pane_id = None;
        }

        // Cascade-close all children (recursively)
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
            // If tab's root leaf IS this pane, remove the whole tab
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
            // Otherwise remove from splits within tabs
            if ws.tabs.iter_mut().any(|tab| tab.split_tree.remove_leaf(pane_id)) {
                return Ok(());
            }
        }
        Err(format!("Pane {} not found in any split tree", pane_id))
    }

    /// Resolve a PTY ID to its pane ID (O(1) lookup).
    pub fn pane_id_for_pty(&self, pty_id: &str) -> Option<String> {
        self.pty_to_pane.get(pty_id).cloned()
    }

    /// Link a pane to a PTY instance.
    pub fn link_pty(&mut self, pane_id: &str, pty_id: String) {
        if let Some(pane) = self.panes.get_mut(pane_id) {
            // Remove old mapping if pane already had a pty_id
            if let Some(old_pty_id) = &pane.pty_id {
                self.pty_to_pane.remove(old_pty_id);
            }
            pane.pty_id = Some(pty_id.clone());
            self.pty_to_pane.insert(pty_id, pane_id.to_string());
        }
    }

    // ── Pane Alias Registry ──────────────────────────────────────────

    /// Register a human-readable alias for a pane ID.
    pub fn register_alias(&mut self, alias: &str, pane_id: &str) {
        self.pane_aliases.insert(alias.to_string(), pane_id.to_string());
    }

    /// Remove a pane alias.
    pub fn unregister_alias(&mut self, alias: &str) {
        self.pane_aliases.remove(alias);
    }

    /// Resolve an ID-or-alias to a pane ID. Checks aliases first, then direct pane lookup.
    /// Returns None if neither alias nor direct pane ID matches.
    pub fn resolve_pane_id(&self, id_or_alias: &str) -> Option<String> {
        // Check alias registry first
        if let Some(pane_id) = self.pane_aliases.get(id_or_alias) {
            // Verify the target pane still exists
            if self.panes.contains_key(pane_id) {
                return Some(pane_id.clone());
            }
        }
        // Fall back to direct pane ID lookup
        if self.panes.contains_key(id_or_alias) {
            return Some(id_or_alias.to_string());
        }
        None
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
            AgentRole::Orchestrator => {
                if parent_pane_id.is_some() {
                    return Err("Orchestrator cannot have a parent".to_string());
                }
                let existing = self.panes.get(pane_id).unwrap();
                if existing.parent_pane_id.is_some() {
                    return Err("Cannot become Orchestrator — already in a hierarchy".to_string());
                }
                let pane = self.panes.get_mut(pane_id).unwrap();
                pane.role = Some(AgentRole::Orchestrator);
                pane.parent_pane_id = None;
                pane.can_spawn = true;
                pane.depth = 1;
            }
            AgentRole::Lead => {
                let parent_id = parent_pane_id
                    .ok_or("Lead requires a parent_pane_id")?;
                let parent = self.panes.get(parent_id)
                    .ok_or(format!("Parent pane {} not found", parent_id))?;
                if parent.role != Some(AgentRole::Orchestrator) {
                    return Err("Lead's parent must be an Orchestrator".to_string());
                }
                if !parent.can_spawn {
                    return Err("Parent does not allow spawning".to_string());
                }
                let pane = self.panes.get_mut(pane_id).unwrap();
                pane.role = Some(AgentRole::Lead);
                pane.parent_pane_id = Some(parent_id.to_string());
                pane.can_spawn = true; // Leads can spawn Workers
                pane.depth = 2;
            }
            AgentRole::Worker => {
                let parent_id = parent_pane_id
                    .ok_or("Worker requires a parent_pane_id")?;
                let parent = self.panes.get(parent_id)
                    .ok_or(format!("Parent pane {} not found", parent_id))?;
                if parent.role != Some(AgentRole::Orchestrator) && parent.role != Some(AgentRole::Lead) {
                    return Err("Worker's parent must be an Orchestrator or Lead".to_string());
                }
                if !parent.can_spawn {
                    return Err("Parent does not allow spawning".to_string());
                }
                let parent_depth = parent.depth;
                let pane = self.panes.get_mut(pane_id).unwrap();
                pane.role = Some(AgentRole::Worker);
                pane.parent_pane_id = Some(parent_id.to_string());
                pane.can_spawn = false;
                pane.depth = parent_depth + 1;
            }
        }

        Ok(())
    }

    /// Clear a pane's orchestration role and orphan its children.
    /// Workers/SubWorkers with a parent cannot unlink themselves.
    pub fn clear_pane_role(&mut self, pane_id: &str) -> Result<(), String> {
        let pane = self.panes.get(pane_id)
            .ok_or(format!("Pane {} not found", pane_id))?;
        if pane.parent_pane_id.is_some() {
            return Err("Cannot unlink — this pane is a worker in a hierarchy".to_string());
        }
        let pane = self.panes.get_mut(pane_id).unwrap();
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
        // Only orchestrators and leads can spawn
        match pane.role {
            Some(AgentRole::Orchestrator) | Some(AgentRole::Lead) => {
                pane.can_spawn = can_spawn;
                Ok(())
            }
            _ => Err("Only Orchestrator or Lead can spawn children".to_string()),
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
                SplitNodeSnapshot::Leaf { pane_id, cwd, title, title_level, agent_type, claude_session_id, role, parent_pane_id, can_spawn, depth } => {
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
                                None // Already taken by another pane
                            } else {
                                Some(sid.clone())
                            }
                        }),
                        role: parsed_role,
                        parent_pane_id: parent_pane_id.clone(),
                        can_spawn: *can_spawn,
                        depth: *depth,
                        report_target_pane_id: None,
                        title_level: restored_level,
                        chat_view: false,
                        pending_prompt: None,
                        auto_approve: false,
                        protected: false,
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
    use crate::multiplexer::types::{AgentRole, Orientation, PaneType};

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
        let info = state.create_workspace("Work".to_string(), None);
        assert_eq!(info.name, "Work");
        assert_eq!(info.tab_count, 1);
        assert!(!info.is_active); // Not active yet
        assert_eq!(state.list_workspaces().len(), 2);
    }

    #[test]
    fn test_set_active_workspace() {
        let (mut state, _) = MultiplexerState::new();
        let info = state.create_workspace("Work".to_string(), None);
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
        // active_tab_id stays on original tab (create_tab doesn't auto-switch)
        assert!(ws.active_tab_id.is_some());
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
        assert_eq!(result, vec!["pty-123".to_string()]);
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
        let ws2 = state.create_workspace("Second".to_string(), None);
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

    // ── Agent Hierarchy Tests ─────────────────────────────────────────

    #[test]
    fn test_set_orchestrator_role() {
        let (mut state, pane_id) = MultiplexerState::new();
        state
            .set_pane_role(&pane_id, AgentRole::Orchestrator, None)
            .unwrap();
        let pane = state.get_pane(&pane_id).unwrap();
        assert_eq!(pane.role, Some(AgentRole::Orchestrator));
        assert_eq!(pane.depth, 1);
        assert!(pane.can_spawn);
        assert!(pane.parent_pane_id.is_none());
    }

    #[test]
    fn test_orchestrator_cannot_have_parent() {
        let (mut state, pane_id) = MultiplexerState::new();
        let result = state.set_pane_role(&pane_id, AgentRole::Orchestrator, Some("fake"));
        assert!(result.is_err());
    }

    #[test]
    fn test_worker_requires_orchestrator_parent() {
        let (mut state, orch_id) = MultiplexerState::new();
        state
            .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
            .unwrap();
        let worker_id = state
            .split_pane(&orch_id, Orientation::Horizontal)
            .unwrap();
        state
            .set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id))
            .unwrap();

        let worker = state.get_pane(&worker_id).unwrap();
        assert_eq!(worker.role, Some(AgentRole::Worker));
        assert_eq!(worker.depth, 2);
        assert!(!worker.can_spawn);
        assert_eq!(worker.parent_pane_id.as_deref(), Some(orch_id.as_str()));
    }

    #[test]
    fn test_worker_parent_must_be_orchestrator() {
        let (mut state, pane_id) = MultiplexerState::new();
        let other_id = state
            .split_pane(&pane_id, Orientation::Horizontal)
            .unwrap();
        // other_id has no role — can't be parent
        let result = state.set_pane_role(&pane_id, AgentRole::Worker, Some(&other_id));
        assert!(result.is_err());
    }

    #[test]
    fn test_worker_parent_must_allow_spawn() {
        let (mut state, orch_id) = MultiplexerState::new();
        state
            .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
            .unwrap();
        // Disable spawning
        state.set_can_spawn(&orch_id, false).unwrap();

        let worker_id = state
            .split_pane(&orch_id, Orientation::Horizontal)
            .unwrap();
        let result = state.set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id));
        assert!(result.is_err());
    }

    #[test]
    fn test_subworker_requires_worker_parent() {
        // Lead under Orchestrator, Worker under Lead
        let (mut state, orch_id) = MultiplexerState::new();
        state.set_pane_role(&orch_id, AgentRole::Orchestrator, None).unwrap();

        let lead_id = state.split_pane(&orch_id, Orientation::Horizontal).unwrap();
        state.set_pane_role(&lead_id, AgentRole::Lead, Some(&orch_id)).unwrap();

        let worker_id = state.split_pane(&lead_id, Orientation::Vertical).unwrap();
        state.set_pane_role(&worker_id, AgentRole::Worker, Some(&lead_id)).unwrap();

        let worker = state.get_pane(&worker_id).unwrap();
        assert_eq!(worker.role, Some(AgentRole::Worker));
        assert_eq!(worker.depth, 3);
        assert!(!worker.can_spawn);
    }

    #[test]
    fn test_get_children_ids() {
        let (mut state, orch_id) = MultiplexerState::new();
        state
            .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
            .unwrap();
        let w1 = state
            .split_pane(&orch_id, Orientation::Horizontal)
            .unwrap();
        state
            .set_pane_role(&w1, AgentRole::Worker, Some(&orch_id))
            .unwrap();
        let w2 = state
            .split_pane(&orch_id, Orientation::Horizontal)
            .unwrap();
        state
            .set_pane_role(&w2, AgentRole::Worker, Some(&orch_id))
            .unwrap();

        let children = state.get_children_ids(&orch_id);
        assert_eq!(children.len(), 2);
        assert!(children.contains(&w1));
        assert!(children.contains(&w2));
    }

    #[test]
    fn test_clear_role_orphans_children() {
        let (mut state, orch_id) = MultiplexerState::new();
        state
            .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
            .unwrap();
        let worker_id = state
            .split_pane(&orch_id, Orientation::Horizontal)
            .unwrap();
        state
            .set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id))
            .unwrap();

        // Clear orchestrator role
        state.clear_pane_role(&orch_id).unwrap();

        // Orchestrator should have no role
        let orch = state.get_pane(&orch_id).unwrap();
        assert!(orch.role.is_none());
        assert_eq!(orch.depth, 0);

        // Worker should be orphaned
        let worker = state.get_pane(&worker_id).unwrap();
        assert!(worker.role.is_none());
        assert!(worker.parent_pane_id.is_none());
        assert_eq!(worker.depth, 0);
    }

    #[test]
    fn test_close_pane_cascades_children() {
        let (mut state, orch_id) = MultiplexerState::new();
        state
            .set_pane_role(&orch_id, AgentRole::Orchestrator, None)
            .unwrap();
        state.link_pty(&orch_id, "pty-orch".to_string());

        let worker_id = state
            .split_pane(&orch_id, Orientation::Horizontal)
            .unwrap();
        state
            .set_pane_role(&worker_id, AgentRole::Worker, Some(&orch_id))
            .unwrap();
        state.link_pty(&worker_id, "pty-worker".to_string());

        // Close orchestrator should cascade to worker
        let pty_ids = state.close_pane(&orch_id).unwrap();
        assert!(pty_ids.contains(&"pty-orch".to_string()));
        assert!(pty_ids.contains(&"pty-worker".to_string()));
        assert!(state.get_pane(&orch_id).is_none());
        assert!(state.get_pane(&worker_id).is_none());
    }

    #[test]
    fn test_set_can_spawn_requires_role() {
        let (mut state, _) = MultiplexerState::new();
        // Create a plain pane without a role
        let pane_id = state.create_pane();
        let result = state.set_can_spawn(&pane_id, true);
        assert!(result.is_err()); // no role
    }

    #[test]
    fn test_set_can_spawn_on_orchestrator() {
        let (mut state, pane_id) = MultiplexerState::new();
        state
            .set_pane_role(&pane_id, AgentRole::Orchestrator, None)
            .unwrap();
        assert!(state.get_pane(&pane_id).unwrap().can_spawn);

        state.set_can_spawn(&pane_id, false).unwrap();
        assert!(!state.get_pane(&pane_id).unwrap().can_spawn);

        state.set_can_spawn(&pane_id, true).unwrap();
        assert!(state.get_pane(&pane_id).unwrap().can_spawn);
    }

    #[test]
    fn test_rename_pane_pins_title() {
        let (mut state, pane_id) = MultiplexerState::new();
        assert_eq!(state.get_pane(&pane_id).unwrap().title_level, TitleLevel::Default);

        state.rename_pane(&pane_id, "Custom").unwrap();
        let pane = state.get_pane(&pane_id).unwrap();
        assert_eq!(pane.title, "Custom");
        assert_eq!(pane.title_level, TitleLevel::User);
    }

    #[test]
    fn test_rename_pane_updates_tab_title() {
        let (mut state, pane_id) = MultiplexerState::new();
        state.rename_pane(&pane_id, "My Tab").unwrap();

        let ws = state.get_active_workspace().unwrap();
        assert_eq!(ws.tabs[0].title, "My Tab");
    }

    #[test]
    fn test_move_tab() {
        let (mut state, _) = MultiplexerState::new();
        let ws1_id = state.list_workspaces()[0].id.clone();
        let ws2 = state.create_workspace("Second".to_string(), None);

        // Create a second tab in ws1
        let (tab2_id, _) = state.create_tab(&ws1_id).unwrap();

        // Move it to ws2
        state.move_tab(&tab2_id, &ws1_id, &ws2.id).unwrap();

        let ws1 = state.get_workspace(&ws1_id).unwrap();
        let ws2 = state.get_workspace(&ws2.id).unwrap();
        assert_eq!(ws1.tabs.len(), 1); // only first tab remains
        assert_eq!(ws2.tabs.len(), 1); // moved tab (workspace starts empty)
    }

    #[test]
    fn test_move_last_tab_fails() {
        let (mut state, _) = MultiplexerState::new();
        let ws1_id = state.list_workspaces()[0].id.clone();
        let ws2 = state.create_workspace("Second".to_string(), None);
        let tab_id = state.get_workspace(&ws1_id).unwrap().tabs[0].id.clone();

        let result = state.move_tab(&tab_id, &ws1_id, &ws2.id);
        assert!(result.is_err()); // can't move last tab
    }

    #[test]
    fn test_session_snapshot_roundtrip() {
        let (mut state, pane_id) = MultiplexerState::new();
        state.link_pty(&pane_id, "pty-1".to_string());
        if let Some(pane) = state.get_pane_mut(&pane_id) {
            pane.cwd = Some("/test/path".to_string());
            pane.agent_type = Some("claude".to_string());
        }
        state
            .set_pane_role(&pane_id, AgentRole::Orchestrator, None)
            .unwrap();

        // Create second workspace
        let ws2 = state.create_workspace("Second".to_string(), None);

        // Snapshot
        let snap = state.to_session_snapshot();
        assert_eq!(snap.workspaces.len(), 2);

        // Restore
        let restored = MultiplexerState::from_session_snapshot(&snap);
        let ws_list = restored.list_workspaces();
        assert_eq!(ws_list.len(), 2);

        // Check pane state was preserved
        let pane = restored.get_pane(&pane_id).unwrap();
        assert_eq!(pane.cwd.as_deref(), Some("/test/path"));
        // agent_type is NOT restored (re-detected from PTY output to prevent false positives)
        assert_eq!(pane.agent_type, None);
        assert_eq!(pane.role, Some(AgentRole::Orchestrator));
        assert_eq!(pane.depth, 1);
        assert!(pane.can_spawn);
        // PTY should not be restored
        assert!(pane.pty_id.is_none());
    }

    #[test]
    fn test_session_snapshot_preserves_hierarchy() {
        let (mut state, cond_id) = MultiplexerState::new();
        state
            .set_pane_role(&cond_id, AgentRole::Orchestrator, None)
            .unwrap();
        let worker_id = state
            .split_pane(&cond_id, Orientation::Horizontal)
            .unwrap();
        state
            .set_pane_role(&worker_id, AgentRole::Worker, Some(&cond_id))
            .unwrap();

        let snap = state.to_session_snapshot();
        let restored = MultiplexerState::from_session_snapshot(&snap);

        let cond = restored.get_pane(&cond_id).unwrap();
        assert_eq!(cond.role, Some(AgentRole::Orchestrator));

        let worker = restored.get_pane(&worker_id).unwrap();
        assert_eq!(worker.role, Some(AgentRole::Worker));
        assert_eq!(worker.parent_pane_id.as_deref(), Some(cond_id.as_str()));
        assert_eq!(worker.depth, 2);
    }

    #[test]
    fn test_browser_pane() {
        let (mut state, _) = MultiplexerState::new();
        let pane_id = state.create_browser_pane(Some("https://example.com".to_string()));
        let pane = state.get_pane(&pane_id).unwrap();
        assert_eq!(pane.pane_type, PaneType::Browser);
        assert_eq!(pane.url.as_deref(), Some("https://example.com"));
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

    // ── Protected Workspace/Pane Tests ───────────────────────────────

    #[test]
    fn test_protected_workspace_cannot_close() {
        let (mut state, _) = MultiplexerState::new();
        let ws_id = state.list_workspaces()[0].id.clone();
        // Mark workspace as protected
        let ws = state.workspaces.iter_mut().find(|w| w.id == ws_id).unwrap();
        ws.protected = true;

        let result = state.close_workspace(&ws_id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Cannot close protected workspace");
        // Workspace still exists
        assert_eq!(state.list_workspaces().len(), 1);
    }

    #[test]
    fn test_protected_pane_cannot_close() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        let new_pane_id = state.split_pane(&initial_pane_id, Orientation::Horizontal).unwrap();
        // Mark the new pane as protected
        state.get_pane_mut(&new_pane_id).unwrap().protected = true;

        let result = state.close_pane(&new_pane_id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Cannot close protected pane");
        // Pane still exists
        assert!(state.get_pane(&new_pane_id).is_some());
    }

    // ── Pane Alias Tests ─────────────────────────────────────────────

    #[test]
    fn test_alias_register_and_resolve() {
        let (mut state, pane_id) = MultiplexerState::new();
        state.register_alias("main-terminal", &pane_id);

        let resolved = state.resolve_pane_id("main-terminal");
        assert_eq!(resolved, Some(pane_id.clone()));
    }

    #[test]
    fn test_alias_not_found_falls_back_to_id() {
        let (state, pane_id) = MultiplexerState::new();
        // No alias registered — should resolve direct pane ID
        let resolved = state.resolve_pane_id(&pane_id);
        assert_eq!(resolved, Some(pane_id));
    }

    #[test]
    fn test_resolve_nonexistent_returns_none() {
        let (state, _) = MultiplexerState::new();
        let resolved = state.resolve_pane_id("does-not-exist");
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_alias_unregister() {
        let (mut state, pane_id) = MultiplexerState::new();
        state.register_alias("my-pane", &pane_id);
        assert!(state.resolve_pane_id("my-pane").is_some());

        state.unregister_alias("my-pane");
        // Alias no longer resolves
        assert_eq!(state.resolve_pane_id("my-pane"), None);
    }

    #[test]
    fn test_alias_to_deleted_pane_returns_none() {
        let (mut state, initial_pane_id) = MultiplexerState::new();
        let new_pane_id = state.split_pane(&initial_pane_id, Orientation::Horizontal).unwrap();
        state.register_alias("ephemeral", &new_pane_id);

        // Close the pane
        state.close_pane(&new_pane_id).unwrap();

        // Alias target is gone — resolve returns None
        assert_eq!(state.resolve_pane_id("ephemeral"), None);
    }
}
