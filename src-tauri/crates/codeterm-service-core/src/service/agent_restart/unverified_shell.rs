use codeterm_types::pty::AgentLiveness;

use super::{AgentRestartError, TabRestartSnapshot};
use crate::service::ServiceContext;

impl ServiceContext {
    pub(super) fn unverified_launch_shell_snapshot(
        &self,
        tab_id: &str,
    ) -> Result<Option<TabRestartSnapshot>, AgentRestartError> {
        let resolved = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
        if crate::agent::live_state::observe_tab_liveness(self, &resolved)
            != AgentLiveness::Unobserved
        {
            return Ok(None);
        }
        let candidate = self.mux.read().get_tab(&resolved).is_some_and(|tab| {
            tab.agent_type.is_some()
                && tab.pty_id.is_some()
                && tab.boot_stage == Some(codeterm_types::pty::BootStage::LaunchBindingUnconfirmed)
                && !tab.agent_booting
        });
        if !candidate {
            return Ok(None);
        }
        let mut snap = self.snapshot_tab_restart_state(&resolved)?;
        if !self.is_unverified_launch_shell(&snap) {
            return Ok(None);
        }
        snap.session_id = None;
        Ok(Some(snap))
    }

    pub(super) fn is_unverified_launch_shell(&self, snap: &TabRestartSnapshot) -> bool {
        let tab_fields = {
            let mux = self.mux.read();
            mux.get_tab(&snap.tab_id).map(|tab| {
                (
                    tab.agent_type.clone(),
                    tab.pty_id.clone(),
                    tab.boot_stage,
                    tab.agent_booting,
                    tab.prior_session_binding_nonces.clone(),
                )
            })
        };
        let Some((provider, pty_id, boot_stage, agent_booting, lineage)) = tab_fields else {
            return false;
        };
        if provider
            .as_deref()
            .is_some_and(|provider| provider != snap.provider)
            || pty_id.as_deref() != Some(snap.pty_id.as_str())
            || boot_stage != Some(codeterm_types::pty::BootStage::LaunchBindingUnconfirmed)
            || agent_booting
            || crate::agent::live_state::observe_tab_liveness(self, &snap.tab_id)
                != AgentLiveness::Unobserved
        {
            return false;
        }
        let verified = crate::service::agent_restart::exit_recovery::verified_session_ids(
            &self.lifecycle_journal.load().records,
            &snap.tab_id,
        );
        if crate::service::agent_restart::exit_recovery::select_recovery_session(
            &lineage, &verified,
        )
        .is_ok()
        {
            return false;
        }
        if self.pty.lock().get_session(&snap.pty_id).is_none() {
            return false;
        }
        self.screen_state
            .get_screen_text(&snap.pty_id)
            .is_some_and(|screen| {
                codeterm_pty::pty::screen_state::screen_has_shell_readiness_prompt(&screen)
            })
    }

    pub(super) fn snapshot_tab_restart_state(
        &self,
        tab_id: &str,
    ) -> Result<TabRestartSnapshot, AgentRestartError> {
        let (resolved, tab) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
            let tab = mux
                .get_tab(&resolved)
                .cloned()
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
            (resolved, tab)
        };

        let provider = tab
            .agent_type
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("no agent in this Tab".to_string()))?;
        let pty_id = tab
            .pty_id
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("Tab has no PTY".to_string()))?;

        if self
            .agent_registry
            .with_provider(&provider, |_| ())
            .is_none()
        {
            return Err(AgentRestartError::BadRequest(format!(
                "unknown agent provider: {}",
                provider
            )));
        }

        let auto_approve = crate::service::permission_policy::configured_permission_bypass_for(
            self,
            &self.agent_registry,
            &provider,
        )
        .map_err(AgentRestartError::BadRequest)?
        .is_some();
        {
            let mut mux = self.mux.write();
            let live_tab = mux.get_tab_mut(&resolved).ok_or_else(|| {
                AgentRestartError::NotFound(format!("Tab {resolved} disappeared"))
            })?;
            if live_tab.pty_id != tab.pty_id
                || live_tab.agent_type != tab.agent_type
                || live_tab.agent_session_id != tab.agent_session_id
                || live_tab.session_binding_nonce != tab.session_binding_nonce
                || live_tab.reported_session_binding_nonce != tab.reported_session_binding_nonce
                || live_tab.role != tab.role
                || live_tab.role_profile != tab.role_profile
                || live_tab.team_id != tab.team_id
                || live_tab.cwd != tab.cwd
            {
                return Err(AgentRestartError::BadRequest(
                    "restart snapshot conflict: launch or placement changed; retry by intent"
                        .into(),
                ));
            }
            live_tab.auto_approve = auto_approve;
        }

        Ok(TabRestartSnapshot {
            tab_id: resolved,
            pty_id,
            provider,
            target_provider: None,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
            handoff_history: None,
        })
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ServiceContext {
    pub fn snapshot_restart_state_for_test(&self, tab: &str) -> Result<(), String> {
        self.snapshot_tab_restart_state(tab)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }
}
