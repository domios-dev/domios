use super::{short_id, MultiplexerState};
use crate::multiplexer::types::{PendingDeliveryRecord, Tab, TabBacking, TabType, TitleLevel};

impl MultiplexerState {
    /// Create a new terminal tab and register it. Returns the tab ID.
    pub fn create_tab_record(&mut self) -> String {
        self.create_tab_record_with_pending_delivery(None)
    }

    pub fn create_tab_record_with_pending_delivery(
        &mut self,
        pending_delivery: Option<PendingDeliveryRecord>,
    ) -> String {
        let tab_id = short_id();
        let tab = Tab {
            id: tab_id.clone(),
            sort_order: 0,
            pty_id: None,
            backing: TabBacking::default(),
            tab_type: TabType::Terminal,
            pending_delivery,
            title: "Terminal".to_string(),
            description: None,
            cwd: None,
            provisioned_cwd: None,
            url: None,
            agent_type: None,
            agent_type_locked: false,
            agent_session_id: None,
            agent_session_name: None,
            agent_session_model: None,
            agent_planned_model: None,
            agent_requested_model: None,
            agent_provider_resolved_model: None,
            model_fallback: None,
            model_identity_mismatch: None,
            reasoning_effort_identity: Default::default(),
            context_epoch_archives: vec![],
            session_binding_nonce: None,
            reported_session_binding_nonce: None,
            shell_integration_nonce: None,
            role: None,
            machine_agent_identity: None,
            role_profile: None,
            team_id: None,
            watcher: None,
            parent_tab_id: None,
            pending_paired_planner_tab_id: None,
            can_spawn: false,
            depth: 0,
            report_target_tab_id: None,
            report_target_mesh_peer: None,
            spawned_by_tab_id: None,
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            agent_state: None,
            auto_approve: false,
            protected: false,
            connected_services: vec![],
            agent_set_at: None,
            exit_suspected_at: None,
            agent_booting: false,
            hidden: false,
            audience: Default::default(),
            skip_snapshot: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
            pending_greeting: None,
            boot_stage: None,
            boot_failure_reason: None,
        };
        self.tabs.insert(tab_id.clone(), tab);
        tab_id
    }

    pub fn get_tab(&self, id: &str) -> Option<&Tab> {
        self.tabs.get(id)
    }

    /// `(tab_id, pty_id)` for every tab currently backed by a PTY. Cheap
    /// id-only clone for the multiplexed web output stream's per-tick sweep —
    /// avoids the heavy full `snapshot()` on the hot path.
    pub fn pty_backed_tabs(&self) -> Vec<(String, String)> {
        self.tabs
            .values()
            .filter_map(|p| p.pty_id.clone().map(|pty| (p.id.clone(), pty)))
            .collect()
    }

    /// Collect agent_session_ids from all tabs except the given one.
    pub fn other_session_ids(&self, exclude_tab_id: &str) -> Vec<String> {
        self.tabs
            .values()
            .filter(|p| p.id != exclude_tab_id)
            .filter_map(|p| p.agent_session_id.clone())
            .collect()
    }

    /// Tab IDs currently holding the given agent session id (typically 0 or 1).
    pub fn tab_ids_with_session(&self, session_id: &str) -> Vec<String> {
        self.tabs
            .values()
            .filter(|p| p.agent_session_id.as_deref() == Some(session_id))
            .map(|p| p.id.clone())
            .collect()
    }

    pub fn get_tab_mut(&mut self, id: &str) -> Option<&mut Tab> {
        self.tabs.get_mut(id)
    }

    pub fn remove_tab(&mut self, id: &str) -> Option<Tab> {
        self.tabs.remove(id)
    }

    pub fn set_focused_tab(&mut self, id: &str) -> Result<(), String> {
        if !self.tabs.contains_key(id) {
            return Err(format!("Tab not found: {}", id));
        }
        self.focused_tab_id = Some(id.to_string());
        Ok(())
    }

    pub fn get_focused_tab_id(&self) -> Option<&str> {
        self.focused_tab_id.as_deref()
    }

    /// Resolve the General orchestrator tab (alias `"general"`, registered by
    /// `service::general_agent::workspace::ensure_general_workspace`) and set
    /// focus to it. Returns the new focused tab id, or `None` if the alias is
    /// not registered (in which case focus is left untouched — preferable to
    /// silently nulling, which is what surfaced the original "deselect to
    /// nothing" UX bug). Callers in service-layer close paths should compare
    /// pre/post focus and emit `tab-focused` if it actually changed.
    pub fn fallback_focus_to_general(&mut self) -> Option<String> {
        // the state layer; if the alias name ever changes, update both.
        let target = self.resolve_tab_id("general")?;
        self.focused_tab_id = Some(target.clone());
        Some(target)
    }

    /// Rename a tab at User level (highest priority).
    pub fn rename_tab(&mut self, tab_id: &str, title: &str) -> Result<(), String> {
        self.rename_tab_at_level(tab_id, title, TitleLevel::User)
    }

    /// Rename a tab at a specific priority level.
    /// Lower levels cannot overwrite higher ones.
    pub fn rename_tab_at_level(
        &mut self,
        tab_id: &str,
        title: &str,
        level: TitleLevel,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(tab_id)
            .ok_or_else(|| format!("Tab not found: {}", tab_id))?;
        if level < tab.title_level {
            return Ok(());
        }
        tab.title = title.to_string();
        tab.title_level = level;
        Ok(())
    }

    pub fn set_tab_description(
        &mut self,
        tab_id: &str,
        description: Option<String>,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(tab_id)
            .ok_or_else(|| format!("Tab not found: {}", tab_id))?;
        tab.description = description;
        Ok(())
    }

    /// Find an agent tab whose CWD starts with `project_root`. Used by the
    /// runaway-sweeper to nudge the owning lane tab.
    pub fn find_agent_tab_for_project(&self, project_root: &str) -> Option<String> {
        self.tabs
            .values()
            .find(|p| {
                p.agent_type.is_some()
                    && p.cwd
                        .as_deref()
                        .map(|cwd| cwd.starts_with(project_root))
                        .unwrap_or(false)
            })
            .map(|p| p.id.clone())
    }

    /// Find which workspace contains a given tab.
    pub fn find_workspace_for_tab(&self, tab_id: &str) -> Option<String> {
        for ws in &self.workspaces {
            if ws.tabs.iter().any(|candidate| candidate == tab_id) {
                return Some(ws.id.clone());
            }
        }
        None
    }

    /// Resolve a workspace membership to its authoritative tab id.
    pub fn find_tab_for_tab(&self, tab_id: &str) -> Option<String> {
        self.find_workspace_for_tab(tab_id)
            .map(|_| tab_id.to_string())
    }

    /// Sidebar label for a tab — tab.title, falling back to short id.
    pub fn display_label_for_tab(&self, tab_id: &str) -> String {
        let resolved = self
            .resolve_tab_id(tab_id)
            .unwrap_or_else(|| tab_id.to_string());
        let tab_title = self
            .get_tab(&resolved)
            .map(|p| p.title.clone())
            .unwrap_or_default();
        if !tab_title.trim().is_empty() {
            tab_title
        } else {
            resolved.chars().take(8).collect()
        }
    }

    /// Close exactly one tab and return the PTY which should be killed.
    ///
    /// Hierarchy edges express coordination, not ownership: a child keeps
    /// running when its parent closes. Direct children are detached before the
    /// parent is removed, preserving their role and Team membership.
    pub fn close_tab(&mut self, tab_id: &str) -> Result<Vec<String>, String> {
        self.record_single_close(tab_id);
        self.close_tab_inner(tab_id)
    }

    /// Close a hierarchy subtree for the explicit Close Team action only.
    /// Ordinary tab close must use `close_tab`.
    pub fn close_tab_for_team_close(&mut self, tab_id: &str) -> Result<Vec<String>, String> {
        self.record_cascade_close(tab_id);
        self.close_tab_cascade_inner(tab_id)
    }

    fn close_tab_inner(&mut self, tab_id: &str) -> Result<Vec<String>, String> {
        if let Some(tab) = self.tabs.get(tab_id) {
            if tab.protected {
                return Err("Cannot close protected Tab".to_string());
            }
        }

        let mut pty_ids = Vec::new();

        if let Some(pty_id) = self.tabs.get(tab_id).and_then(|p| p.pty_id.clone()) {
            self.pty_to_tab.remove(&pty_id);
            pty_ids.push(pty_id);
        }

        self.remove_tab_from_tree(tab_id)?;

        for child_id in self.get_children_ids(tab_id) {
            if let Some(child) = self.tabs.get_mut(&child_id) {
                child.parent_tab_id = None;
                child.depth = 0;
                if child.report_target_tab_id.as_deref() == Some(tab_id) {
                    child.report_target_tab_id = None;
                }
            }
        }
        self.tabs.remove(tab_id);

        if self.focused_tab_id.as_deref() == Some(tab_id) {
            self.focused_tab_id = None;
            self.fallback_focus_to_general();
        }

        Ok(pty_ids)
    }

    fn close_tab_cascade_inner(&mut self, tab_id: &str) -> Result<Vec<String>, String> {
        let child_ids = self.get_children_ids(tab_id);
        let mut pty_ids = self.close_tab_inner(tab_id)?;
        for child_id in child_ids {
            if self.tabs.contains_key(&child_id) {
                pty_ids.extend(self.close_tab_cascade_inner(&child_id)?);
            }
        }
        Ok(pty_ids)
    }

    /// Remove the tab that owns a given tab. Single-tab-per-tab makes this
    /// a direct delete with no tree walking.
    fn remove_tab_from_tree(&mut self, tab_id: &str) -> Result<(), String> {
        for ws in &mut self.workspaces {
            if let Some(idx) = ws.tabs.iter().position(|candidate| candidate == tab_id) {
                let closed_tab_id = ws.tabs[idx].clone();
                ws.tabs.remove(idx);
                if ws.active_tab_id.as_deref() == Some(&closed_tab_id) {
                    ws.active_tab_id = ws.tabs.last().cloned();
                }
                return Ok(());
            }
        }
        Err(format!("Tab {} not found in any tab", tab_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryRecord};

    #[test]
    fn fresh_spawn_attaches_record_only_when_pending() {
        let mut state = MultiplexerState::empty();
        let pending =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
        let with_pending = state.create_tab_record_with_pending_delivery(Some(pending.clone()));
        let without_pending = state.create_tab_record_with_pending_delivery(None);

        assert_eq!(
            state.get_tab(&with_pending).unwrap().pending_delivery,
            Some(pending)
        );
        assert_eq!(
            state.get_tab(&without_pending).unwrap().pending_delivery,
            None
        );
    }
}

#[cfg(test)]
mod display_label_tests {
    use super::*;

    #[test]
    fn display_label_uses_canonical_tab_title() {
        let (mut state, tab_id) = MultiplexerState::new();
        let tab_id = state.find_tab_for_tab(&tab_id).unwrap();
        state.rename_tab(&tab_id, "Fibonacci").unwrap();
        assert_eq!(state.display_label_for_tab(&tab_id), "Fibonacci");
    }

    #[test]
    fn display_label_respects_user_pin_over_agent_title() {
        let (mut state, tab_id) = MultiplexerState::new();
        state.rename_tab(&tab_id, "Euler").unwrap();
        state
            .rename_tab_at_level(&tab_id, "claude", TitleLevel::Agent)
            .unwrap();
        assert_eq!(state.display_label_for_tab(&tab_id), "Euler");
    }

    #[test]
    fn auto_generated_tab_name_is_canonical() {
        let (mut state, _root) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let tab_id = state.create_tab(&ws_id).unwrap();
        let generated = state.get_tab(&tab_id).unwrap().title.clone();
        assert_ne!(generated, "claude");
        assert_eq!(state.display_label_for_tab(&tab_id), generated);
    }

    /// Reproduces the web/CLI agent-spawn title decision: `create_tab` picks a
    /// friendly name, then the spawn path decides whether to override it via
    /// `creation_title_override`. A no-title, no-agent-name spawn (the "+ New
    /// session" case that regressed to showing "cursor"/"codex") must keep the
    /// generated name — never the raw provider id.
    #[test]
    fn agent_spawn_with_no_explicit_title_keeps_generated_name() {
        let (mut state, _root) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let tab_id = state.create_tab(&ws_id).unwrap();
        let generated = state.get_tab(&tab_id).unwrap().title.clone();

        let provider_id = "cursor";
        let override_decision =
            crate::multiplexer::spawn_title::creation_title_override(None, None);
        assert!(
            override_decision.is_none(),
            "no title/agent_name supplied: spawn must not compute a fallback title"
        );
        if let Some((title, level)) = override_decision {
            state.rename_tab_at_level(&tab_id, &title, level).unwrap();
        }

        let final_title = state.get_tab(&tab_id).unwrap().title.clone();
        assert_eq!(
            final_title, generated,
            "no-title agent spawn must keep the create_tab-generated name"
        );
        assert_ne!(
            final_title, provider_id,
            "the raw provider id must never become the Tab title"
        );
    }

    #[test]
    fn agent_spawn_with_agent_name_overrides_generated_name_at_user_level() {
        let (mut state, _root) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let tab_id = state.create_tab(&ws_id).unwrap();

        let (title, level) =
            crate::multiplexer::spawn_title::creation_title_override(None, Some("Watson")).unwrap();
        state.rename_tab_at_level(&tab_id, &title, level).unwrap();

        let tab = state.get_tab(&tab_id).unwrap();
        assert_eq!(tab.title, "Watson");
        assert_eq!(tab.title_level, TitleLevel::User);
    }

    #[test]
    fn display_label_falls_back_to_short_id_when_everything_empty() {
        let (mut state, tab_id) = MultiplexerState::new();
        state.rename_tab(&tab_id, "").unwrap();
        let label = state.display_label_for_tab(&tab_id);
        assert!(
            !label.is_empty(),
            "display label must never be empty (got {:?})",
            label
        );
    }
}
