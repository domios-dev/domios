use super::{short_id, MultiplexerState, WorkspaceInfo, ORPHANED_CLOSED_CAP};
use crate::multiplexer::types::Workspace;

impl MultiplexerState {
    pub fn create_workspace(&mut self, name: String, cwd: Option<String>) -> WorkspaceInfo {
        let workspace_id = short_id();

        let workspace = Workspace {
            id: workspace_id.clone(),
            name: name.clone(),
            tabs: vec![],
            active_tab_id: None,
            cwd: cwd.clone(),
            protected: false,
            recently_closed: vec![],
        };

        self.workspaces.push(workspace);

        WorkspaceInfo {
            id: workspace_id.clone(),
            name,
            tab_count: 0,
            is_active: false,
            cwd,
            protected: false,
            pane_ids: vec![],
            existing: None,
        }
    }

    pub fn get_workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// Find a workspace whose `cwd` refers to the same folder as `query`.
    /// Uses `paths::normalize_cwd` on both sides so trailing slashes, case
    /// differences on Windows, and symlink variants all collapse to a match.
    pub fn find_workspace_by_cwd(&self, query: &str) -> Option<&Workspace> {
        let normalized = crate::paths::normalize_cwd(query);
        self.workspaces.iter().find(|w| {
            w.cwd
                .as_deref()
                .map(crate::paths::normalize_cwd)
                .map(|stored| stored == normalized)
                .unwrap_or(false)
        })
    }

    /// Same as `find_workspace_by_cwd` but returns an owned `WorkspaceInfo`
    /// so callers can drop the mux lock before acting on the result.
    pub fn find_workspace_info_by_cwd(&self, query: &str) -> Option<WorkspaceInfo> {
        let ws = self.find_workspace_by_cwd(query)?;
        let pane_ids: Vec<String> = ws.tabs.iter().map(|t| t.pane_id.clone()).collect();
        Some(WorkspaceInfo {
            id: ws.id.clone(),
            name: ws.name.clone(),
            tab_count: ws.tabs.len(),
            is_active: self.active_workspace_id.as_deref() == Some(&ws.id),
            cwd: ws.cwd.clone(),
            protected: ws.protected,
            pane_ids,
            existing: None,
        })
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

        // Preserve the workspace's recently-closed ring so tabs closed before
        // the workspace was deleted can still be reopened. Newest entries win
        // when the orphaned ring is already full.
        if !workspace.recently_closed.is_empty() {
            for entry in workspace.recently_closed {
                // Avoid exact duplicates (same pane_id already in orphaned ring).
                if !self
                    .orphaned_closed
                    .iter()
                    .any(|e| e.pane_id == entry.pane_id)
                {
                    self.orphaned_closed.insert(0, entry);
                }
            }
            self.orphaned_closed.truncate(ORPHANED_CLOSED_CAP);
        }

        let pane_ids: Vec<String> = workspace
            .tabs
            .iter()
            .map(|tab| tab.pane_id.clone())
            .collect();

        let pty_ids: Vec<String> = pane_ids
            .iter()
            .filter_map(|pid| self.panes.get(pid))
            .filter_map(|pane| pane.pty_id.clone())
            .collect();

        for pid in &pane_ids {
            if let Some(pane) = self.panes.remove(pid) {
                if let Some(pty_id) = &pane.pty_id {
                    self.pty_to_pane.remove(pty_id);
                }
            }
        }

        if self.active_workspace_id.as_deref() == Some(id) {
            self.active_workspace_id = self.workspaces.first().map(|w| w.id.clone());
        }

        if let Some(ref focused) = self.focused_pane_id {
            if pane_ids.contains(focused) {
                self.focused_pane_id = None;
                self.fallback_focus_to_general();
            }
        }

        Ok(pty_ids)
    }

    pub fn rename_workspace(&mut self, id: &str, name: &str) -> Result<(), String> {
        let ws = self
            .workspaces
            .iter_mut()
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
    pub fn move_tab(
        &mut self,
        tab_id: &str,
        from_workspace_id: &str,
        to_workspace_id: &str,
    ) -> Result<(), String> {
        if from_workspace_id == to_workspace_id {
            return Ok(());
        }

        let from_ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == from_workspace_id)
            .ok_or_else(|| format!("Source workspace not found: {}", from_workspace_id))?;

        let tab_idx = from_ws
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .ok_or_else(|| format!("Tab not found: {}", tab_id))?;

        if from_ws.tabs.len() <= 1 {
            return Err("Cannot move the last tab from a workspace".to_string());
        }

        let tab = from_ws.tabs.remove(tab_idx);

        if from_ws.active_tab_id.as_deref() == Some(tab_id) {
            from_ws.active_tab_id = from_ws.tabs.last().map(|t| t.id.clone());
        }

        let to_ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == to_workspace_id)
            .ok_or_else(|| format!("Destination workspace not found: {}", to_workspace_id))?;

        to_ws.tabs.push(tab);
        to_ws.active_tab_id = Some(tab_id.to_string());

        Ok(())
    }

    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.workspaces
            .iter()
            .map(|w| {
                let pane_ids: Vec<String> = w.tabs.iter().map(|t| t.pane_id.clone()).collect();
                WorkspaceInfo {
                    id: w.id.clone(),
                    name: w.name.clone(),
                    tab_count: w.tabs.len(),
                    is_active: self.active_workspace_id.as_deref() == Some(&w.id),
                    cwd: w.cwd.clone(),
                    protected: w.protected,
                    pane_ids,
                    existing: None,
                }
            })
            .collect()
    }
}
