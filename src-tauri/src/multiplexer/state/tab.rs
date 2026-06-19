use super::{names, short_id, MultiplexerState};
use crate::multiplexer::types::Tab;

impl MultiplexerState {
    /// Create a new tab in the given workspace. Returns (tab_id, new_pane_id).
    pub fn create_tab(&mut self, workspace_id: &str) -> Result<(String, String), String> {
        let workspace_idx = self
            .workspaces
            .iter()
            .position(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
        let title = names::generate_unique_tab_name(
            self.workspaces[workspace_idx]
                .tabs
                .iter()
                .map(|tab| tab.title.as_str()),
        );
        let pane_id = self.create_pane();
        let tab_id = short_id();

        let tab = Tab {
            id: tab_id.clone(),
            title,
            description: None,
            pane_id: pane_id.clone(),
        };

        let workspace = &mut self.workspaces[workspace_idx];

        workspace.tabs.push(tab);
        // Only set active if this is the first tab (empty workspace)
        if workspace.active_tab_id.is_none() {
            workspace.active_tab_id = Some(tab_id.clone());
        }

        Ok((tab_id, pane_id))
    }

    /// Close a tab and return PTY IDs that need to be killed.
    pub fn close_tab(&mut self, workspace_id: &str, tab_id: &str) -> Result<Vec<String>, String> {
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
        let pane_ids = vec![tab.pane_id.clone()];

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

        if workspace.active_tab_id.as_deref() == Some(tab_id) {
            workspace.active_tab_id = workspace.tabs.last().map(|t| t.id.clone());
        }

        if let Some(ref focused) = self.focused_pane_id {
            if pane_ids.contains(focused) {
                self.focused_pane_id = None;
                self.fallback_focus_to_general();
            }
        }

        Ok(pty_ids)
    }

    pub fn rename_tab(
        &mut self,
        workspace_id: &str,
        tab_id: &str,
        title: &str,
    ) -> Result<(), String> {
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
        let tab = ws
            .tabs
            .iter_mut()
            .find(|t| t.id == tab_id)
            .ok_or_else(|| format!("Tab not found: {}", tab_id))?;
        tab.title = title.to_string();
        Ok(())
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
}
