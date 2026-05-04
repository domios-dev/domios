use super::{short_id, MultiplexerState};
use crate::multiplexer::types::{SplitNode, Tab};

impl MultiplexerState {
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
}
