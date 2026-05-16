use super::{short_id, MultiplexerState};
use crate::multiplexer::types::{Pane, PaneType, TitleLevel};

impl MultiplexerState {
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
            agent_type_locked: false,
            agent_session_id: None,
            agent_session_name: None,
            agent_session_model: None,
            role: None,
            parent_pane_id: None,
            can_spawn: false,
            depth: 0,
            report_target_pane_id: None,
            spawned_by_pane_id: None,
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            auto_approve: false,
            protected: false,
            connected_services: vec![],
            agent_set_at: None,
            hidden: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
        };
        self.panes.insert(pane_id.clone(), pane);
        pane_id
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

    /// Pane IDs currently holding the given agent session id (typically 0 or 1).
    pub fn pane_ids_with_session(&self, session_id: &str) -> Vec<String> {
        self.panes.values()
            .filter(|p| p.agent_session_id.as_deref() == Some(session_id))
            .map(|p| p.id.clone())
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

    /// Resolve the General orchestrator pane (alias `"general"`, registered by
    /// `service::general_agent::workspace::ensure_general_workspace`) and set
    /// focus to it. Returns the new focused pane id, or `None` if the alias is
    /// not registered (in which case focus is left untouched — preferable to
    /// silently nulling, which is what surfaced the original "deselect to
    /// nothing" UX bug). Callers in service-layer close paths should compare
    /// pre/post focus and emit `pane-focused` if it actually changed.
    pub fn fallback_focus_to_general(&mut self) -> Option<String> {
        // Hard-coded alias mirrors `service::general_agent::GENERAL_ALIAS`.
        // Kept as a string literal to avoid pulling a service-layer constant
        // into the state layer; if the alias name ever changes, update both.
        let target = self.resolve_pane_id("general")?;
        self.focused_pane_id = Some(target.clone());
        Some(target)
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
        if level < pane.title_level {
            return Ok(());
        }
        pane.title = title.to_string();
        pane.title_level = level;
        Ok(())
    }

    /// Find which workspace contains a given pane.
    pub fn find_workspace_for_pane(&self, pane_id: &str) -> Option<String> {
        for ws in &self.workspaces {
            if ws.tabs.iter().any(|t| t.pane_id == pane_id) {
                return Some(ws.id.clone());
            }
        }
        None
    }

    /// Find which tab contains a given pane.
    pub fn find_tab_for_pane(&self, pane_id: &str) -> Option<String> {
        for ws in &self.workspaces {
            if let Some(tab) = ws.tabs.iter().find(|t| t.pane_id == pane_id) {
                return Some(tab.id.clone());
            }
        }
        None
    }

    /// Sidebar label for a pane — tab.title, falling back to pane.title, then short id.
    pub fn display_label_for_pane(&self, pane_id: &str) -> String {
        let pane_title = self.get_pane(pane_id).map(|p| p.title.clone()).unwrap_or_default();
        for ws in &self.workspaces {
            if let Some(tab) = ws.tabs.iter().find(|t| t.pane_id == pane_id) {
                let t = tab.title.trim();
                if !t.is_empty() && t != pane_title.trim() {
                    return t.to_string();
                }
                break;
            }
        }
        if !pane_title.trim().is_empty() {
            pane_title
        } else {
            pane_id.chars().take(8).collect()
        }
    }

    /// Close a pane: remove from the workspace tree and pane registry.
    /// Cascade-closes all children. Returns pty_ids that should be killed.
    pub fn close_pane(&mut self, pane_id: &str) -> Result<Vec<String>, String> {
        if let Some(pane) = self.panes.get(pane_id) {
            if pane.protected {
                return Err("Cannot close protected pane".to_string());
            }
        }

        let mut pty_ids = Vec::new();

        let child_ids = self.get_children_ids(pane_id);

        if let Some(pty_id) = self.panes.get(pane_id).and_then(|p| p.pty_id.clone()) {
            self.pty_to_pane.remove(&pty_id);
            pty_ids.push(pty_id);
        }

        self.remove_pane_from_tree(pane_id)?;

        self.panes.remove(pane_id);

        if self.focused_pane_id.as_deref() == Some(pane_id) {
            self.focused_pane_id = None;
            self.fallback_focus_to_general();
        }

        for cid in child_ids {
            if let Ok(child_ptys) = self.close_pane(&cid) {
                pty_ids.extend(child_ptys);
            }
        }

        Ok(pty_ids)
    }

    /// Remove the tab that owns a given pane. Single-pane-per-tab makes this
    /// a direct delete with no tree walking.
    fn remove_pane_from_tree(&mut self, pane_id: &str) -> Result<(), String> {
        for ws in &mut self.workspaces {
            if let Some(idx) = ws.tabs.iter().position(|tab| tab.pane_id == pane_id) {
                let closed_tab_id = ws.tabs[idx].id.clone();
                ws.tabs.remove(idx);
                if ws.active_tab_id.as_deref() == Some(&closed_tab_id) {
                    ws.active_tab_id = ws.tabs.last().map(|t| t.id.clone());
                }
                return Ok(());
            }
        }
        Err(format!("Pane {} not found in any tab", pane_id))
    }
}

#[cfg(test)]
mod display_label_tests {
    use super::*;

    #[test]
    fn display_label_prefers_tab_title_over_pane_title() {
        // Regression: `[from MY-CUSTOM-TITLE pane=2503fbce]` showed the raw
        // pane.title instead of the sidebar's friendly tab name. Recipients
        // had no way to map the prefix back to the tab they could see.
        let (mut state, pane_id) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let tab_id = state.find_tab_for_pane(&pane_id).unwrap();
        state.rename_tab(&ws_id, &tab_id, "Fibonacci").unwrap();
        state.rename_pane(&pane_id, "MY-CUSTOM-TITLE").unwrap();
        assert_eq!(state.display_label_for_pane(&pane_id), "Fibonacci");
    }

    #[test]
    fn display_label_falls_back_to_pane_title_when_tab_matches() {
        let (mut state, pane_id) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let tab_id = state.find_tab_for_pane(&pane_id).unwrap();
        state.rename_tab(&ws_id, &tab_id, "Same").unwrap();
        state.rename_pane(&pane_id, "Same").unwrap();
        // Tab and pane title are identical — no point parroting the same word,
        // fall back to pane.title so the resulting string isn't double-empty.
        assert_eq!(state.display_label_for_pane(&pane_id), "Same");
    }

    #[test]
    fn display_label_falls_back_to_short_id_when_everything_empty() {
        let (mut state, pane_id) = MultiplexerState::new();
        state.rename_pane(&pane_id, "").unwrap();
        let label = state.display_label_for_pane(&pane_id);
        // Default auto-generated tab title is non-empty, so the label is
        // either the tab title or the short id — either way, never empty.
        assert!(!label.is_empty(), "display label must never be empty (got {:?})", label);
    }
}
