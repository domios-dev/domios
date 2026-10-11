use codeterm_types::pty::{AgentLiveness, BootStage};

use super::{AgentRestartError, RestartAttemptError, TabRestartSnapshot};
use crate::multiplexer::types::Tab;
use crate::service::ServiceContext;

fn fresh_restart_guard(
    active: bool,
    force: bool,
    exact_session: bool,
    session_exists: Option<bool>,
) -> Result<(), AgentRestartError> {
    if active && !force {
        return Err(AgentRestartError::BadRequest(
            "fresh restart refused while the agent is live or launching; use --force".into(),
        ));
    }
    if exact_session && session_exists != Some(false) {
        return Err(AgentRestartError::BadRequest(
            "fresh restart refused: the exact restored session exists or could not be checked"
                .into(),
        ));
    }
    Ok(())
}

fn clear_fresh_session_binding(tab: &mut Tab) {
    tab.agent_session_id = None;
    tab.agent_session_name = None;
    tab.session_binding_nonce = None;
    tab.reported_session_binding_nonce = None;
    tab.restored_from_snapshot = false;
    tab.prior_session_binding_nonces.clear();
    if let Some(restore) = tab.close_restore.as_mut() {
        restore.exact_session = false;
    }
}

impl ServiceContext {
    pub(super) fn restart_session_exists(
        &self,
        tab: &Tab,
        provider: &str,
        session_id: Option<&str>,
    ) -> Option<bool> {
        let cwd = tab
            .close_restore
            .as_ref()
            .filter(|restore| restore.exact_session)
            .and_then(|restore| restore.session_cwd.as_deref())
            .or(tab.cwd.as_deref());
        session_id.zip(cwd).and_then(|(session, cwd)| {
            self.agent_registry
                .with_provider(provider, |p| p.session_exists(cwd, session) == Some(true))
        })
    }

    pub(super) fn require_exact_session_restart(
        &self,
        snap: &TabRestartSnapshot,
    ) -> Result<(), AgentRestartError> {
        let exact = self
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .and_then(|tab| tab.close_restore.as_ref())
            .is_some_and(|restore| restore.exact_session);
        if exact && !self.session_file_present(snap) {
            return Err(AgentRestartError::BadRequest(
                "the exact restored session is unavailable; an explicit --fresh action is required"
                    .into(),
            ));
        }
        Ok(())
    }

    pub(super) fn fresh_restart_snapshot(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<TabRestartSnapshot, AgentRestartError> {
        let tab = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {tab_id} not found")))?;
            mux.get_tab(&resolved)
                .cloned()
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {resolved} not found")))?
        };
        let provider = tab
            .agent_type
            .as_deref()
            .or(tab.restorable_launch.failed_provider.as_deref())
            .ok_or_else(|| {
                AgentRestartError::BadRequest("no recorded provider in this Tab".into())
            })?;
        if self
            .agent_registry
            .with_provider(provider, |p| p.describe().transport)
            != Some(crate::agent::provider::ProviderTransport::Pty)
        {
            return Err(AgentRestartError::BadRequest(
                "fresh restart requires an available PTY provider".into(),
            ));
        }
        let snap = TabRestartSnapshot {
            tab_id: tab.id.clone(),
            pty_id: tab.pty_id.clone().unwrap_or_default(),
            provider: provider.into(),
            target_provider: None,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve: crate::service::permission_policy::configured_permission_bypass_for(
                &self.app_handle,
                &self.agent_registry,
                provider,
            )
            .map_err(AgentRestartError::BadRequest)?
            .is_some(),
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
            handoff_history: None,
        };
        self.require_fresh_restart_allowed(&snap, force)?;
        Ok(snap)
    }

    pub(super) fn require_fresh_restart_allowed(
        &self,
        snap: &TabRestartSnapshot,
        force: bool,
    ) -> Result<(), AgentRestartError> {
        let tab = self
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .cloned()
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", snap.tab_id)))?;
        if tab.agent_session_id != snap.session_id
            || tab.cwd != snap.cwd
            || tab
                .agent_type
                .as_deref()
                .or(tab.restorable_launch.failed_provider.as_deref())
                != Some(snap.provider.as_str())
        {
            return Err(AgentRestartError::BadRequest(
                "Tab changed before fresh restart".into(),
            ));
        }
        let exact_session = tab
            .close_restore
            .as_ref()
            .is_some_and(|restore| restore.exact_session);
        let session_exists =
            self.restart_session_exists(&tab, &snap.provider, tab.agent_session_id.as_deref());
        let liveness = crate::agent::live_state::observe_tab_liveness(self, &tab.id);
        let active = tab.agent_booting
            || liveness == AgentLiveness::Live
            || (liveness == AgentLiveness::Unobserved
                && tab.boot_stage != Some(BootStage::SpawnFailed));
        fresh_restart_guard(active, force, exact_session, session_exists)
    }

    pub(super) fn commit_fresh_restart(
        &self,
        snap: &TabRestartSnapshot,
    ) -> Result<(), RestartAttemptError> {
        let unavailable_restore = {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(&snap.tab_id).ok_or_else(|| {
                AgentRestartError::NotFound(format!("Tab {} not found", snap.tab_id))
            })?;
            let exact = tab
                .close_restore
                .as_ref()
                .is_some_and(|restore| restore.exact_session);
            clear_fresh_session_binding(tab);
            exact
        };
        let cause = if unavailable_restore {
            "fresh session after unavailable restore"
        } else {
            "explicit fresh session restart"
        };
        self.record_lifecycle(
            &snap.tab_id,
            crate::session::lifecycle::LifecycleTransition::RolledBack {
                reason: format!(
                    "{cause}, old session {}",
                    snap.session_id.as_deref().unwrap_or("none")
                ),
            },
        );
        self.schedule_session_persist_debounced();
        self.emit_state_changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
