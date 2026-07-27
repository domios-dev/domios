//! Workspace & tab operations.

use super::ServiceContext;
use crate::multiplexer::state::WorkspaceInfo;
use crate::multiplexer::types::ClosedTab;
use crate::service::session_persist::{self, SessionPersistUrgency};

impl ServiceContext {
    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.mux.read().list_workspaces()
    }

    fn ensure_workspace_plans_loaded(&self, cwd: Option<&str>) {
        let Some(cwd) = cwd.filter(|c| !c.is_empty()) else {
            return;
        };
        let path = std::path::Path::new(cwd);
        if self
            .loaded_plan_projects
            .lock()
            .iter()
            .any(|root| path.starts_with(root))
        {
            return;
        }
        if let Err(e) = self.ensure_plan_project_loaded(path) {
            log::debug!("[workspace] no project plans loaded for {cwd}: {e}");
        }
    }

    pub fn create_workspace(
        &self,
        name: String,
        cwd: Option<String>,
        activate: bool,
    ) -> WorkspaceInfo {
        if let Some(cwd_str) = cwd.as_deref() {
            let existing = self.mux.write().find_workspace_info_by_cwd(cwd_str);
            if let Some(ws) = existing {
                if activate {
                    let _ = self.mux.write().set_active_workspace(&ws.id);
                }
                self.ensure_workspace_plans_loaded(Some(cwd_str));
                log::info!(
                    "[workspace] reused existing workspace '{}' ({}) for cwd={}",
                    ws.name,
                    ws.id,
                    cwd_str
                );
                return WorkspaceInfo {
                    existing: Some(true),
                    ..ws
                };
            }
        }

        let ws = self.mux.write().create_workspace(name, cwd.clone());
        if activate {
            let _ = self.mux.write().set_active_workspace(&ws.id);
        }
        self.ensure_workspace_plans_loaded(cwd.as_deref());
        self.emit_state_changed();
        ws
    }

    pub fn focus_workspace(&self, id: &str) -> Result<(), String> {
        self.mux.write().set_active_workspace(id)?;
        let cwd = self
            .mux
            .read()
            .list_workspaces()
            .into_iter()
            .find(|w| w.id == id)
            .and_then(|w| w.cwd);
        self.ensure_workspace_plans_loaded(cwd.as_deref());
        self.emit_state_changed();
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<(), String> {
        self.mux.write().rename_workspace(id, name)?;
        self.emit_state_changed();
        session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)?;
        Ok(())
    }

    /// True when any top-level tab of `workspace_id` is a coordinator. Reads
    /// the tab tab ids under one lock, then probes each after releasing it so
    /// no nested tab lock is held across the coordinator resolution.
    fn workspace_has_protected_tab(&self, workspace_id: &str) -> bool {
        let tab_ids: Vec<String> = {
            let mux = self.mux.read();
            match mux.get_workspace(workspace_id) {
                Some(ws) => ws.tabs.clone(),
                None => return false,
            }
        };
        tab_ids.iter().any(|pid| self.is_self_close_protected(pid))
    }

    /// Close a workspace on behalf of `caller_tab_id`. Refuses when a
    /// coordinator (Orchestrator/plan owner/Planner) targets the workspace it
    /// runs in; closing any OTHER workspace is unaffected. Fail-safe: an absent
    /// caller against a workspace that hosts a coordinator is refused.
    pub fn close_workspace_guarded(
        &self,
        caller_tab_id: Option<&str>,
        workspace_id: &str,
    ) -> Result<(), String> {
        let refuse = match caller_tab_id {
            Some(caller) => {
                let owns_it = self.mux.read().find_workspace_for_tab(caller).as_deref()
                    == Some(workspace_id);
                owns_it && self.is_self_close_protected(caller)
            }
            None => self.workspace_has_protected_tab(workspace_id),
        };
        if refuse {
            return Err(format!(
                "{}: a coordinator cannot close the workspace it runs in ({workspace_id}) — \
                 have your parent or another Tab close it, or move first.",
                crate::service::tab::SELF_CLOSE_REFUSAL_PREFIX
            ));
        }
        self.close_workspace(workspace_id)
    }

    pub fn close_workspace(&self, id: &str) -> Result<(), String> {
        let pre_focus = self.mux.read().get_focused_tab_id().map(str::to_string);
        let pty_ids = self.mux.write().close_workspace(id)?;
        let post_focus = self.mux.read().get_focused_tab_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_tab_focus_changed(&new);
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
            .write()
            .move_tab(tab_id, from_workspace_id, to_workspace_id)?;
        self.emit_state_changed();
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Reorder the top-level tabs of a workspace to match `ordered_tab_ids`.
    pub fn reorder_tab(
        &self,
        workspace_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        self.mux
            .write()
            .reorder_tabs(workspace_id, ordered_tab_ids)?;
        self.emit_state_changed();
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Reorder the children of `parent_tab_id` to match `ordered_tab_ids`.
    pub fn reorder_child(
        &self,
        parent_tab_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        self.mux
            .write()
            .reorder_children(parent_tab_id, ordered_tab_ids)?;
        self.emit_state_changed();
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Reorder workspaces to match `ordered_workspace_ids`.
    pub fn reorder_workspace(&self, ordered_workspace_ids: &[String]) -> Result<(), String> {
        self.mux.write().reorder_workspaces(ordered_workspace_ids)?;
        self.emit_state_changed();
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    pub fn create_tab(&self, workspace_id: &str) -> Result<String, String> {
        self.create_tab_with_activate(workspace_id, true)
    }

    pub fn create_tab_with_activate(
        &self,
        workspace_id: &str,
        activate: bool,
    ) -> Result<String, String> {
        let result = {
            let mut mux = self.mux.write();
            let tab_id = mux.create_tab(workspace_id)?;
            if activate {
                let _ = mux.set_active_tab(workspace_id, &tab_id);
            }
            tab_id
        };
        self.emit_state_changed();
        Ok(result)
    }

    pub fn set_active_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        self.mux.write().set_active_tab(workspace_id, tab_id)?;
        self.emit_state_changed();
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Atomically set active workspace and active tab in one lock acquisition,
    /// emitting a single mux-state-changed instead of two sequential ones.
    pub fn set_active_tab_and_workspace(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<(), String> {
        {
            let mut mux = self.mux.write();
            mux.set_active_workspace(workspace_id)?;
            mux.set_active_tab(workspace_id, tab_id)?;
        }
        self.emit_state_changed();
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Snapshot of a workspace's recently-closed ring (newest first).
    pub fn list_recently_closed(&self, workspace_id: &str) -> Vec<ClosedTab> {
        self.mux.read().list_recently_closed(workspace_id)
    }

    /// Reopen a recently-closed tab (browser-style). `index` picks the ring
    /// entry (newest = 0, default). Re-materializes the tab with its original
    /// backing in a fresh, activated tab; the restore path then resumes a PTY
    /// agent or lazily reloads a chatBackend transcript.
    pub fn reopen_closed_tab(
        &self,
        workspace_id: &str,
        index: Option<usize>,
    ) -> Result<String, String> {
        let tab_id = {
            let mut mux = self.mux.write();
            let result = mux.reopen_closed_tab(workspace_id, index)?;
            let _ = mux.set_active_tab(workspace_id, &result);
            result
        };
        self.emit_state_changed();
        Ok(tab_id)
    }

    /// Reopen a whole closed group (single / cascade subtree / workspace) by id.
    /// Recreates the workspace if needed, restores every tab in the group,
    /// relinks the hierarchy, and activates the resulting tab/workspace.
    /// Returns (workspace_id, tab_id).
    pub fn reopen_closed_group(&self, group_id: &str) -> Result<(String, String), String> {
        let result = {
            let mut mux = self.mux.write();
            let r = mux.reopen_closed_group(group_id)?;
            let _ = mux.set_active_workspace(&r.0);
            let _ = mux.set_active_tab(&r.0, &r.1);
            r
        };
        self.emit_state_changed();
        Ok(result)
    }

    /// Reopen a tab from the orphaned ring. Recreates the workspace and
    /// activates it. Returns (workspace_id, tab_id).
    pub fn reopen_orphaned_tab(&self, tab_id: &str) -> Result<(String, String), String> {
        let result = {
            let mut mux = self.mux.write();
            let r = mux.reopen_orphaned_tab(tab_id)?;
            let _ = mux.set_active_workspace(&r.0);
            let _ = mux.set_active_tab(&r.0, &r.1);
            r
        };
        self.emit_state_changed();
        Ok(result)
    }
}

#[cfg(test)]
#[path = "workspace_role_tests.rs"]
mod workspace_role_tests;
