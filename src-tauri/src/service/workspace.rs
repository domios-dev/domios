//! Workspace & tab operations.

use crate::multiplexer::state::WorkspaceInfo;
use super::ServiceContext;

impl ServiceContext {
    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.mux.lock().list_workspaces()
    }

    pub fn create_workspace(&self, name: String, cwd: Option<String>, activate: bool) -> WorkspaceInfo {
        if let Some(cwd_str) = cwd.as_deref() {
            let existing = self.mux.lock().find_workspace_info_by_cwd(cwd_str);
            if let Some(ws) = existing {
                if activate {
                    let _ = self.mux.lock().set_active_workspace(&ws.id);
                }
                let _ = self.create_tab_with_activate(&ws.id, activate);
                log::info!(
                    "[workspace] reused existing workspace '{}' ({}) for cwd={}",
                    ws.name,
                    ws.id,
                    cwd_str
                );
                return ws;
            }
        }

        let ws = self.mux.lock().create_workspace(name, cwd);
        if activate {
            let _ = self.mux.lock().set_active_workspace(&ws.id);
        }
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
        let pre_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.lock().close_workspace(id)?;
        let post_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_pane_focus_changed(&new);
            }
        }
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
        self.create_tab_with_activate(workspace_id, true)
    }

    pub fn create_tab_with_activate(&self, workspace_id: &str, activate: bool) -> Result<(String, String), String> {
        let result = {
            let mut mux = self.mux.lock();
            let (tab_id, pane_id) = mux.create_tab(workspace_id)?;
            if activate {
                let _ = mux.set_active_tab(workspace_id, &tab_id);
            }
            (tab_id, pane_id)
        };
        self.emit_state_changed();
        Ok(result)
    }

    pub fn close_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        let pre_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.lock().close_tab(workspace_id, tab_id)?;
        let post_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_pane_focus_changed(&new);
            }
        }
        Ok(())
    }

    pub fn set_active_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        self.mux.lock().set_active_tab(workspace_id, tab_id)?;
        self.emit_state_changed();
        Ok(())
    }
}
