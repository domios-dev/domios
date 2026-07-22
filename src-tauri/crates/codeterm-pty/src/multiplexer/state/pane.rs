use super::{short_id, MultiplexerState};
use crate::multiplexer::types::{Pane, PaneBacking, PaneType, PendingDeliveryRecord, TitleLevel};

impl MultiplexerState {
    /// Create a new terminal pane and register it. Returns the pane ID.
    pub fn create_pane(&mut self) -> String {
        self.create_pane_with_pending_delivery(None)
    }

    pub fn create_pane_with_pending_delivery(
        &mut self,
        pending_delivery: Option<PendingDeliveryRecord>,
    ) -> String {
        let pane_id = short_id();
        let pane = Pane {
            id: pane_id.clone(),
            pty_id: None,
            backing: PaneBacking::default(),
            pane_type: PaneType::Terminal,
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
            session_binding_nonce: None,
            reported_session_binding_nonce: None,
            shell_integration_nonce: None,
            role: None,
            role_profile: None,
            team_id: None,
            watcher: None,
            parent_pane_id: None,
            pending_paired_planner_pane_id: None,
            can_spawn: false,
            depth: 0,
            report_target_pane_id: None,
            report_target_mesh_peer: None,
            spawned_by_pane_id: None,
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
            skip_snapshot: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
            pending_greeting: None,
            boot_stage: None,
            boot_failure_reason: None,
        };
        self.panes.insert(pane_id.clone(), pane);
        pane_id
    }

    pub fn get_pane(&self, id: &str) -> Option<&Pane> {
        self.panes.get(id)
    }

    /// `(pane_id, pty_id)` for every pane currently backed by a PTY. Cheap
    /// id-only clone for the multiplexed web output stream's per-tick sweep —
    /// avoids the heavy full `snapshot()` on the hot path.
    pub fn pty_backed_panes(&self) -> Vec<(String, String)> {
        self.panes
            .values()
            .filter_map(|p| p.pty_id.clone().map(|pty| (p.id.clone(), pty)))
            .collect()
    }

    /// Collect agent_session_ids from all panes except the given one.
    pub fn other_session_ids(&self, exclude_pane_id: &str) -> Vec<String> {
        self.panes
            .values()
            .filter(|p| p.id != exclude_pane_id)
            .filter_map(|p| p.agent_session_id.clone())
            .collect()
    }

    /// Pane IDs currently holding the given agent session id (typically 0 or 1).
    pub fn pane_ids_with_session(&self, session_id: &str) -> Vec<String> {
        self.panes
            .values()
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
        // the state layer; if the alias name ever changes, update both.
        let target = self.resolve_pane_id("general")?;
        self.focused_pane_id = Some(target.clone());
        Some(target)
    }

    /// Rename a pane at User level (highest priority).
    pub fn rename_pane(&mut self, pane_id: &str, title: &str) -> Result<(), String> {
        self.rename_pane_at_level(pane_id, title, TitleLevel::User)
    }

    /// Rename a pane at a specific priority level.
    /// Lower levels cannot overwrite higher ones.
    pub fn rename_pane_at_level(
        &mut self,
        pane_id: &str,
        title: &str,
        level: TitleLevel,
    ) -> Result<(), String> {
        let pane = self
            .panes
            .get_mut(pane_id)
            .ok_or_else(|| format!("Pane not found: {}", pane_id))?;
        if level < pane.title_level {
            return Ok(());
        }
        pane.title = title.to_string();
        pane.title_level = level;
        Ok(())
    }

    pub fn set_pane_description(
        &mut self,
        pane_id: &str,
        description: Option<String>,
    ) -> Result<(), String> {
        let pane = self
            .panes
            .get_mut(pane_id)
            .ok_or_else(|| format!("Pane not found: {}", pane_id))?;
        pane.description = description.clone();
        for ws in &mut self.workspaces {
            if let Some(tab) = ws.tabs.iter_mut().find(|t| t.pane_id == pane_id) {
                tab.description = description;
                break;
            }
        }
        Ok(())
    }

    /// Find an agent pane whose CWD starts with `project_root`. Used by the
    /// runaway-sweeper to nudge the owning lane pane.
    pub fn find_agent_pane_for_project(&self, project_root: &str) -> Option<String> {
        self.panes
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

    /// Sidebar label for a pane — pane.title, falling back to short id.
    pub fn display_label_for_pane(&self, pane_id: &str) -> String {
        let resolved = self
            .resolve_pane_id(pane_id)
            .unwrap_or_else(|| pane_id.to_string());
        let pane_title = self
            .get_pane(&resolved)
            .map(|p| p.title.clone())
            .unwrap_or_default();
        if !pane_title.trim().is_empty() {
            pane_title
        } else {
            resolved.chars().take(8).collect()
        }
    }

    /// Close exactly one pane and return the PTY which should be killed.
    ///
    /// Hierarchy edges express coordination, not ownership: a child keeps
    /// running when its parent closes. Direct children are detached before the
    /// parent is removed, preserving their role and Team membership.
    pub fn close_pane(&mut self, pane_id: &str) -> Result<Vec<String>, String> {
        self.record_single_close(pane_id);
        self.close_pane_inner(pane_id)
    }

    /// Close a hierarchy subtree for the explicit Close Team action only.
    /// Ordinary pane close must use `close_pane`.
    pub fn close_pane_for_team_close(&mut self, pane_id: &str) -> Result<Vec<String>, String> {
        self.record_cascade_close(pane_id);
        self.close_pane_cascade_inner(pane_id)
    }

    fn close_pane_inner(&mut self, pane_id: &str) -> Result<Vec<String>, String> {
        if let Some(pane) = self.panes.get(pane_id) {
            if pane.protected {
                return Err("Cannot close protected pane".to_string());
            }
        }

        let mut pty_ids = Vec::new();

        if let Some(pty_id) = self.panes.get(pane_id).and_then(|p| p.pty_id.clone()) {
            self.pty_to_pane.remove(&pty_id);
            pty_ids.push(pty_id);
        }

        self.remove_pane_from_tree(pane_id)?;

        for child_id in self.get_children_ids(pane_id) {
            if let Some(child) = self.panes.get_mut(&child_id) {
                child.parent_pane_id = None;
                child.depth = 0;
                if child.report_target_pane_id.as_deref() == Some(pane_id) {
                    child.report_target_pane_id = None;
                }
            }
        }
        self.panes.remove(pane_id);

        if self.focused_pane_id.as_deref() == Some(pane_id) {
            self.focused_pane_id = None;
            self.fallback_focus_to_general();
        }

        Ok(pty_ids)
    }

    fn close_pane_cascade_inner(&mut self, pane_id: &str) -> Result<Vec<String>, String> {
        let child_ids = self.get_children_ids(pane_id);
        let mut pty_ids = self.close_pane_inner(pane_id)?;
        for child_id in child_ids {
            if self.panes.contains_key(&child_id) {
                pty_ids.extend(self.close_pane_cascade_inner(&child_id)?);
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
mod tests {
    use super::*;
    use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryRecord};

    #[test]
    fn fresh_spawn_attaches_record_only_when_pending() {
        let mut state = MultiplexerState::empty();
        let pending =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
        let with_pending = state.create_pane_with_pending_delivery(Some(pending.clone()));
        let without_pending = state.create_pane_with_pending_delivery(None);

        assert_eq!(
            state.get_pane(&with_pending).unwrap().pending_delivery,
            Some(pending)
        );
        assert_eq!(
            state.get_pane(&without_pending).unwrap().pending_delivery,
            None
        );
    }
}

#[cfg(test)]
mod display_label_tests {
    use super::*;

    #[test]
    fn display_label_uses_canonical_pane_title() {
        let (mut state, pane_id) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let tab_id = state.find_tab_for_pane(&pane_id).unwrap();
        state.rename_tab(&ws_id, &tab_id, "Fibonacci").unwrap();
        assert_eq!(state.display_label_for_pane(&pane_id), "Fibonacci");
    }

    #[test]
    fn display_label_respects_user_pin_over_agent_title() {
        let (mut state, pane_id) = MultiplexerState::new();
        state.rename_pane(&pane_id, "Euler").unwrap();
        state
            .rename_pane_at_level(&pane_id, "claude", TitleLevel::Agent)
            .unwrap();
        assert_eq!(state.display_label_for_pane(&pane_id), "Euler");
    }

    #[test]
    fn auto_generated_pane_name_is_canonical() {
        let (mut state, _root) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let (_tab_id, pane_id) = state.create_tab(&ws_id).unwrap();
        let generated = state.get_pane(&pane_id).unwrap().title.clone();
        assert_ne!(generated, "claude");
        assert_eq!(state.display_label_for_pane(&pane_id), generated);
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
        let (_tab_id, pane_id) = state.create_tab(&ws_id).unwrap();
        let generated = state.get_pane(&pane_id).unwrap().title.clone();

        let provider_id = "cursor";
        let override_decision =
            crate::multiplexer::spawn_title::creation_title_override(None, None);
        assert!(
            override_decision.is_none(),
            "no title/agent_name supplied: spawn must not compute a fallback title"
        );
        if let Some((title, level)) = override_decision {
            state.rename_pane_at_level(&pane_id, &title, level).unwrap();
        }

        let final_title = state.get_pane(&pane_id).unwrap().title.clone();
        assert_eq!(
            final_title, generated,
            "no-title agent spawn must keep the create_tab-generated name"
        );
        assert_ne!(
            final_title, provider_id,
            "the raw provider id must never become the pane title"
        );
    }

    #[test]
    fn agent_spawn_with_agent_name_overrides_generated_name_at_user_level() {
        let (mut state, _root) = MultiplexerState::new();
        let ws_id = state.workspaces.first().map(|w| w.id.clone()).unwrap();
        let (_tab_id, pane_id) = state.create_tab(&ws_id).unwrap();

        let (title, level) =
            crate::multiplexer::spawn_title::creation_title_override(None, Some("Watson")).unwrap();
        state.rename_pane_at_level(&pane_id, &title, level).unwrap();

        let pane = state.get_pane(&pane_id).unwrap();
        assert_eq!(pane.title, "Watson");
        assert_eq!(pane.title_level, TitleLevel::User);
    }

    #[test]
    fn display_label_falls_back_to_short_id_when_everything_empty() {
        let (mut state, pane_id) = MultiplexerState::new();
        state.rename_pane(&pane_id, "").unwrap();
        let label = state.display_label_for_pane(&pane_id);
        assert!(
            !label.is_empty(),
            "display label must never be empty (got {:?})",
            label
        );
    }
}
