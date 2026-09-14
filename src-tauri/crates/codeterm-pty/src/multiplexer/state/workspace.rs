use super::{short_id, MultiplexerState, PreparedWorkspaceClose, WorkspaceInfo};
use crate::multiplexer::types::Workspace;

impl MultiplexerState {
    pub fn create_workspace(&mut self, name: String, cwd: Option<String>) -> WorkspaceInfo {
        let workspace_id = short_id();

        let workspace = Workspace {
            task_keys: Default::default(),
            id: workspace_id.clone(),
            name: name.clone(),
            tabs: vec![],
            active_tab_id: None,
            cwd: cwd.clone(),
            protected: false,
            sort_order: self.next_workspace_sort_order(),
        };

        self.workspaces.push(workspace);

        WorkspaceInfo {
            id: workspace_id.clone(),
            name,
            tab_count: 0,
            is_active: false,
            cwd,
            protected: false,
            tab_ids: vec![],
            existing: None,
        }
    }

    pub fn get_workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// Live tab count for a single workspace (one tab per tab). Scoped to the
    /// given workspace only — other workspaces never contribute — so the tab
    /// budget can't couple unrelated workspaces. Unknown id → 0.
    pub fn tab_count_in_workspace(&self, id: &str) -> usize {
        self.get_workspace(id).map(|w| w.tabs.len()).unwrap_or(0)
    }

    /// Find a workspace whose `cwd` refers to the same folder as `query`.
    /// Uses `paths::normalize_cwd` on both sides so trailing slashes, case
    /// differences on Windows, and symlink variants all collapse to a match.
    pub fn find_workspace_by_cwd(&self, query: &str) -> Option<&Workspace> {
        let normalized = codeterm_paths::normalize_cwd(query);
        self.workspaces.iter().find(|w| {
            w.cwd
                .as_deref()
                .map(codeterm_paths::normalize_cwd)
                .map(|stored| stored == normalized)
                .unwrap_or(false)
        })
    }

    /// Same as `find_workspace_by_cwd` but returns an owned `WorkspaceInfo`
    /// so callers can drop the mux lock before acting on the result.
    pub fn find_workspace_info_by_cwd(&self, query: &str) -> Option<WorkspaceInfo> {
        let ws = self.find_workspace_by_cwd(query)?;
        let tab_ids = ws.tabs.clone();
        Some(WorkspaceInfo {
            id: ws.id.clone(),
            name: ws.name.clone(),
            tab_count: ws.tabs.len(),
            is_active: self.active_workspace_id.as_deref() == Some(&ws.id),
            cwd: ws.cwd.clone(),
            protected: ws.protected,
            tab_ids,
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
        let prepared = self.reserve_workspace_close(id)?;
        self.close_prepared_workspace(prepared)
    }

    /// Persist a prepared restore group and remove its unchanged workspace.
    pub fn close_prepared_workspace(
        &mut self,
        prepared: PreparedWorkspaceClose,
    ) -> Result<Vec<String>, String> {
        let id = prepared.workspace_id();
        if !self.workspace_close_is_reserved_by(&prepared) {
            return Err(format!("Workspace close reservation lost: {id}"));
        }
        let idx = self.workspaces.iter().position(|w| w.id == id);
        let Some(idx) = idx else {
            self.release_workspace_close_reservation(&prepared);
            return Err(format!("Workspace not found: {id}"));
        };

        if self.workspaces[idx].protected {
            self.release_workspace_close_reservation(&prepared);
            return Err("Cannot close protected workspace".to_string());
        }
        if self.workspaces[idx].tabs.as_slice() != prepared.tab_ids() {
            self.release_workspace_close_reservation(&prepared);
            return Err(format!("Workspace changed while closing: {id}"));
        }
        self.record_prepared_workspace_close(&prepared);

        let workspace = self.workspaces.remove(idx);

        let tab_ids = workspace.tabs;

        let pty_ids: Vec<String> = tab_ids
            .iter()
            .filter_map(|pid| self.tabs.get(pid))
            .filter_map(|tab| tab.pty_id.clone())
            .collect();

        for pid in &tab_ids {
            if let Some(tab) = self.tabs.remove(pid) {
                if let Some(pty_id) = &tab.pty_id {
                    self.pty_to_tab.remove(pty_id);
                }
            }
        }

        if self.active_workspace_id.as_deref() == Some(id) {
            self.active_workspace_id = self.workspaces.first().map(|w| w.id.clone());
        }

        if let Some(ref focused) = self.focused_tab_id {
            if tab_ids.contains(focused) {
                self.focused_tab_id = None;
                self.fallback_focus_to_general();
            }
        }

        self.release_workspace_close_reservation(&prepared);

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
        if self.ensure_workspace_membership_mutable(id).is_err() {
            return;
        }
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == id) {
            ws.protected = protected;
        }
    }

    /// Move a workspace to the front and pin it to the lowest `sort_order` (0),
    /// renumbering the rest by their existing order. Used for the General
    /// (protected) workspace so it stays at the top of the sidebar.
    pub fn move_workspace_to_front(&mut self, id: &str) {
        if let Some(idx) = self.workspaces.iter().position(|w| w.id == id) {
            if idx > 0 {
                let ws = self.workspaces.remove(idx);
                self.workspaces.insert(0, ws);
            }
        }
        let mut rest: Vec<&mut Workspace> =
            self.workspaces.iter_mut().filter(|w| w.id != id).collect();
        rest.sort_by_key(|w| w.sort_order);
        for (i, w) in rest.into_iter().enumerate() {
            w.sort_order = (i + 1) as i32;
        }
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == id) {
            ws.sort_order = 0;
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

        self.ensure_workspace_membership_mutable(from_workspace_id)?;
        self.ensure_workspace_membership_mutable(to_workspace_id)?;

        let from_idx = self
            .workspaces
            .iter()
            .position(|w| w.id == from_workspace_id)
            .ok_or_else(|| format!("Source workspace not found: {}", from_workspace_id))?;
        let tab_idx = self.workspaces[from_idx]
            .tabs
            .iter()
            .position(|candidate| candidate == tab_id)
            .ok_or_else(|| format!("Tab not found: {}", tab_id))?;

        if self.workspaces[from_idx].tabs.len() <= 1 {
            return Err("Cannot move the last tab from a workspace".to_string());
        }

        let to_idx = self
            .workspaces
            .iter()
            .position(|w| w.id == to_workspace_id)
            .ok_or_else(|| format!("Destination workspace not found: {}", to_workspace_id))?;

        let next_sort_order = self.workspaces[to_idx]
            .tabs
            .iter()
            .filter_map(|candidate| self.tabs.get(candidate).map(|tab| tab.sort_order))
            .max()
            .unwrap_or(-1)
            + 1;
        let moved_tab_id = {
            let from_ws = &mut self.workspaces[from_idx];
            let moved_tab_id = from_ws.tabs.remove(tab_idx);
            if from_ws.active_tab_id.as_deref() == Some(tab_id) {
                from_ws.active_tab_id = from_ws.tabs.last().cloned();
            }
            moved_tab_id
        };
        if let Some(tab) = self.tabs.get_mut(&moved_tab_id) {
            tab.sort_order = next_sort_order;
        }
        let to_ws = &mut self.workspaces[to_idx];
        to_ws.tabs.push(moved_tab_id);
        to_ws.active_tab_id = Some(tab_id.to_string());

        Ok(())
    }

    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.workspaces
            .iter()
            .map(|w| {
                let tab_ids = w.tabs.clone();
                WorkspaceInfo {
                    id: w.id.clone(),
                    name: w.name.clone(),
                    tab_count: w.tabs.len(),
                    is_active: self.active_workspace_id.as_deref() == Some(&w.id),
                    cwd: w.cwd.clone(),
                    protected: w.protected,
                    tab_ids,
                    existing: None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tab_count_tests {
    use super::*;

    #[test]
    fn tab_count_is_scoped_to_one_workspace() {
        let (mut state, _root) = MultiplexerState::new();
        let small = state.create_workspace("small".into(), None).id;
        let crowded = state.create_workspace("crowded".into(), None).id;

        state.create_tab(&small).unwrap();
        for _ in 0..25 {
            state.create_tab(&crowded).unwrap();
        }

        // The crowded workspace's 25 tabs must never leak into the small one.
        assert_eq!(state.tab_count_in_workspace(&small), 1);
        assert_eq!(state.tab_count_in_workspace(&crowded), 25);
    }

    #[test]
    fn tab_count_unknown_workspace_is_zero() {
        let (state, _root) = MultiplexerState::new();
        assert_eq!(state.tab_count_in_workspace("does-not-exist"), 0);
    }
}

#[cfg(test)]
mod move_tab_transaction_tests {
    use super::*;

    #[test]
    fn unknown_destination_leaves_source_and_tab_unchanged() {
        let (mut state, root) = MultiplexerState::new();
        let source_id = state.find_workspace_for_tab(&root).unwrap();
        let moved = state.create_tab(&source_id).unwrap();
        state.set_active_tab(&source_id, &moved).unwrap();
        state.get_tab_mut(&moved).unwrap().sort_order = 41;

        let before_tabs = state.get_workspace(&source_id).unwrap().tabs.clone();
        let before_active = state
            .get_workspace(&source_id)
            .unwrap()
            .active_tab_id
            .clone();
        let before_sort_order = state.get_tab(&moved).unwrap().sort_order;

        let error = state
            .move_tab(&moved, &source_id, "missing-destination")
            .unwrap_err();

        assert!(error.contains("Destination workspace not found"));
        assert_eq!(state.get_workspace(&source_id).unwrap().tabs, before_tabs);
        assert_eq!(
            state.get_workspace(&source_id).unwrap().active_tab_id,
            before_active
        );
        assert_eq!(state.get_tab(&moved).unwrap().sort_order, before_sort_order);
        assert_eq!(
            state.find_workspace_for_tab(&moved).as_deref(),
            Some(source_id.as_str())
        );
    }

    #[test]
    fn last_tab_guard_precedes_destination_validation() {
        let (mut state, only_tab) = MultiplexerState::new();
        let source_id = state.find_workspace_for_tab(&only_tab).unwrap();

        let error = state
            .move_tab(&only_tab, &source_id, "missing-destination")
            .unwrap_err();

        assert_eq!(error, "Cannot move the last tab from a workspace");
        assert_eq!(
            state.find_workspace_for_tab(&only_tab).as_deref(),
            Some(source_id.as_str())
        );
    }

    #[test]
    fn changed_workspace_rejects_prepared_close_without_recording_restore_group() {
        let (mut state, first_tab) = MultiplexerState::new();
        let workspace_id = state.find_workspace_for_tab(&first_tab).unwrap();
        let prepared = state.reserve_workspace_close(&workspace_id).unwrap();
        let synthetic_tab = "concurrent-internal-mutation".to_string();
        state
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == workspace_id)
            .unwrap()
            .tabs
            .push(synthetic_tab.clone());

        let error = state.close_prepared_workspace(prepared).unwrap_err();

        assert!(error.contains("Workspace changed while closing"));
        assert!(state.get_workspace(&workspace_id).is_some());
        assert!(state.get_tab(&first_tab).is_some());
        assert!(state
            .get_workspace(&workspace_id)
            .unwrap()
            .tabs
            .contains(&synthetic_tab));
        assert!(state.recent_closed_view().is_empty());
    }
}
