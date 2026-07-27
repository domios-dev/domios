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
        let existing_titles = self.workspaces[workspace_idx]
            .tabs
            .iter()
            .filter_map(|tab| self.panes.get(&tab.pane_id).map(|pane| pane.title.as_str()));
        let title = names::generate_unique_tab_name(existing_titles);
        let pane_id = self.create_pane();
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.title = title;
            pane.title_level = crate::multiplexer::types::TitleLevel::Default;
        }
        let tab_id = short_id();
        let sort_order = self.next_tab_sort_order(workspace_id);

        let tab = Tab {
            id: tab_id.clone(),
            description: None,
            pane_id: pane_id.clone(),
            sort_order,
        };

        let workspace = &mut self.workspaces[workspace_idx];

        workspace.tabs.push(tab);
        // Only set active if this is the first tab (empty workspace)
        if workspace.active_tab_id.is_none() {
            workspace.active_tab_id = Some(tab_id.clone());
        }

        Ok((tab_id, pane_id))
    }

    /// Claim an existing blank tab for a spawn instead of creating a new one
    /// — used by the new-session picker's in-place pair spawn so confirming
    /// the picker transforms that same tab rather than leaving an orphan
    /// empty tab behind. Only claimable while the pane has no agent/backend
    /// attached yet (still the picker's blank placeholder). Returns the same
    /// (tab_id, pane_id) pair `create_tab` would have minted fresh.
    pub fn claim_existing_tab(
        &mut self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<(String, String), String> {
        let pane_id = self
            .get_workspace(workspace_id)
            .and_then(|ws| ws.tabs.iter().find(|t| t.id == tab_id))
            .map(|t| t.pane_id.clone())
            .ok_or_else(|| format!("Tab not found in workspace {}: {}", workspace_id, tab_id))?;
        let pane = self
            .panes
            .get(&pane_id)
            .ok_or_else(|| format!("Tab not found: {}", pane_id))?;
        if pane.agent_type.is_some()
            || !matches!(pane.backing, crate::multiplexer::types::PaneBacking::Pty)
        {
            return Err(format!("Tab {} is not a blank picker tab", tab_id));
        }
        if pane.protected {
            return Err(format!("Tab {} is protected and cannot be claimed", tab_id));
        }
        Ok((tab_id.to_string(), pane_id))
    }

    /// Close a tab and return PTY IDs that need to be killed.
    pub fn close_tab(&mut self, workspace_id: &str, tab_id: &str) -> Result<Vec<String>, String> {
        // Record into the closed-history store while the pane still exists.
        let closing_pane_id = self
            .get_workspace(workspace_id)
            .and_then(|ws| ws.tabs.iter().find(|t| t.id == tab_id))
            .map(|t| t.pane_id.clone());
        if let Some(pane_id) = closing_pane_id.as_ref() {
            self.record_single_close(&pane_id);
            for pane in self.panes.values_mut() {
                if pane.parent_pane_id.as_deref() == Some(pane_id.as_str()) {
                    pane.parent_pane_id = None;
                    pane.depth = 0;
                    if pane.report_target_pane_id.as_deref() == Some(pane_id.as_str()) {
                        pane.report_target_pane_id = None;
                    }
                }
            }
        }

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

    /// Unwind a tab + its pane created by an atomic spawn whose `launch` then
    /// failed, leaving no blank focused tab behind. Unlike [`close_tab`] this does
    /// NOT record the pane into the closed-pane history — a never-launched pane is
    /// not a user "recently closed" tab and must not pollute that list.
    ///
    /// [`close_tab`]: Self::close_tab
    pub fn rollback_spawn_tab(&mut self, workspace_id: &str, tab_id: &str, pane_id: &str) {
        if let Some(workspace) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            workspace.tabs.retain(|t| t.id != tab_id);
            if workspace.active_tab_id.as_deref() == Some(tab_id) {
                workspace.active_tab_id = workspace.tabs.last().map(|t| t.id.clone());
            }
        }
        if let Some(pane) = self.panes.remove(pane_id) {
            if let Some(pty_id) = &pane.pty_id {
                self.pty_to_pane.remove(pty_id);
            }
        }
        if self.focused_pane_id.as_deref() == Some(pane_id) {
            self.focused_pane_id = None;
            self.fallback_focus_to_general();
        }
    }

    pub fn rename_tab(
        &mut self,
        workspace_id: &str,
        tab_id: &str,
        title: &str,
    ) -> Result<(), String> {
        let pane_id = {
            let ws = self
                .workspaces
                .iter()
                .find(|w| w.id == workspace_id)
                .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
            ws.tabs
                .iter()
                .find(|t| t.id == tab_id)
                .map(|tab| tab.pane_id.clone())
                .ok_or_else(|| format!("Tab not found: {}", tab_id))?
        };
        self.rename_pane(&pane_id, title)
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
