use super::{names, MultiplexerState};
use crate::multiplexer::types::{TabBacking, TitleLevel};
use crate::session::lifecycle::{LifecycleEpoch, LifecycleKey};

impl MultiplexerState {
    /// Bind a tab to the lifecycle operation that now owns its authority. Only
    /// the journal writer calls this; nothing else may mint lineage.
    pub fn stamp_lifecycle(
        &mut self,
        tab_id: &str,
        epoch: LifecycleEpoch,
        operation_id: &str,
    ) -> bool {
        match self.tabs.get_mut(tab_id) {
            Some(tab) => {
                tab.lifecycle_epoch = Some(epoch.0);
                tab.lifecycle_operation_id = Some(operation_id.to_string());
                true
            }
            None => false,
        }
    }

    /// The operation a tab is currently bound to, if any. A tab with no lineage
    /// cannot journal a transition and so can never earn a checkpoint.
    pub fn lifecycle_key(&self, tab_id: &str) -> Option<LifecycleKey> {
        let tab = self.tabs.get(tab_id)?;
        Some(LifecycleKey::new(
            tab_id,
            LifecycleEpoch(tab.lifecycle_epoch?),
            tab.lifecycle_operation_id.clone()?,
        ))
    }

    pub fn create_tab(&mut self, workspace_id: &str) -> Result<String, String> {
        self.ensure_workspace_membership_mutable(workspace_id)?;
        let workspace_idx = self
            .workspaces
            .iter()
            .position(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;
        let existing_titles = self.workspaces[workspace_idx]
            .tabs
            .iter()
            .filter_map(|tab_id| self.tabs.get(tab_id).map(|tab| tab.title.as_str()));
        let title = names::generate_unique_tab_name(existing_titles);
        let tab_id = self.create_tab_record();
        let sort_order = self.next_tab_sort_order(workspace_id);
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.title = title;
            tab.title_level = TitleLevel::Default;
            tab.sort_order = sort_order;
        }
        let workspace = &mut self.workspaces[workspace_idx];
        workspace.tabs.push(tab_id.clone());
        if workspace.active_tab_id.is_none() {
            workspace.active_tab_id = Some(tab_id.clone());
        }
        Ok(tab_id)
    }

    pub fn claim_existing_tab(&self, workspace_id: &str, tab_id: &str) -> Result<String, String> {
        let workspace = self
            .get_workspace(workspace_id)
            .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;
        if !workspace.tabs.iter().any(|candidate| candidate == tab_id) {
            return Err(format!(
                "Tab not found in workspace {workspace_id}: {tab_id}"
            ));
        }
        let tab = self
            .tabs
            .get(tab_id)
            .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
        if tab.agent_type.is_some() || !matches!(tab.backing, TabBacking::Pty) {
            return Err(format!("Tab {tab_id} is not a blank picker tab"));
        }
        if tab.protected {
            return Err(format!("Tab {tab_id} is protected and cannot be claimed"));
        }
        Ok(tab_id.to_string())
    }

    pub fn rollback_spawn_tab(&mut self, workspace_id: &str, tab_id: &str) {
        if self
            .ensure_workspace_membership_mutable(workspace_id)
            .is_err()
        {
            return;
        }
        if let Some(workspace) = self
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == workspace_id)
        {
            workspace.tabs.retain(|candidate| candidate != tab_id);
            if workspace.active_tab_id.as_deref() == Some(tab_id) {
                workspace.active_tab_id = workspace.tabs.last().cloned();
            }
        }
        if let Some(tab) = self.tabs.remove(tab_id) {
            if let Some(pty_id) = tab.pty_id {
                self.pty_to_tab.remove(&pty_id);
            }
        }
        if self.focused_tab_id.as_deref() == Some(tab_id) {
            self.focused_tab_id = None;
            self.fallback_focus_to_general();
        }
    }

    pub fn set_active_tab(&mut self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        let workspace = self
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;
        if !workspace.tabs.iter().any(|candidate| candidate == tab_id) {
            return Err(format!("Tab not found: {tab_id}"));
        }
        workspace.active_tab_id = Some(tab_id.to_string());
        Ok(())
    }
}
