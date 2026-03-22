//! Workspace & tab operations.

use crate::multiplexer::state::WorkspaceInfo;
use super::ServiceContext;

impl ServiceContext {
    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.mux.lock().list_workspaces()
    }

    pub fn create_workspace(&self, name: String) -> WorkspaceInfo {
        let ws = self.mux.lock().create_workspace(name);
        self.emit_state_changed();
        ws
    }

    pub fn focus_workspace(&self, id: &str) -> Result<(), String> {
        self.mux.lock().set_active_workspace(id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<(), String> {
        self.mux.lock().rename_workspace(id, name)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn close_workspace(&self, id: &str) -> Result<(), String> {
        let pty_ids = self.mux.lock().close_workspace(id)?;
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        Ok(())
    }

    pub fn move_tab(
        &self,
        tab_id: &str,
        from_workspace_id: &str,
        to_workspace_id: &str,
    ) -> Result<(), String> {
        self.mux
            .lock()
            .move_tab(tab_id, from_workspace_id, to_workspace_id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn create_tab(&self, workspace_id: &str) -> Result<(String, String), String> {
        let result = {
            let mut mux = self.mux.lock();
            let (tab_id, pane_id) = mux.create_tab(workspace_id)?;
            let _ = mux.set_active_tab(workspace_id, &tab_id);
            (tab_id, pane_id)
        };
        self.emit_state_changed();
        Ok(result)
    }

    pub fn close_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        let pty_ids = self.mux.lock().close_tab(workspace_id, tab_id)?;
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        Ok(())
    }

    pub fn set_active_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        self.mux.lock().set_active_tab(workspace_id, tab_id)?;
        self.emit_state_changed();
        Ok(())
    }
}
