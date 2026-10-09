use codeterm_types::pty::{AgentLiveness, BootStage};

use super::{AgentRestartError, TabRestartSnapshot};
use crate::service::ServiceContext;

fn failed_provider(tab: &crate::multiplexer::types::Tab) -> Option<&str> {
    if tab.agent_booting {
        return None;
    }
    tab.restorable_launch
        .failed_provider
        .as_deref()
        .or_else(|| {
            matches!(
                tab.boot_stage,
                Some(
                    BootStage::SpawnFailed
                        | BootStage::LaunchDeadlineElapsed
                        | BootStage::LaunchBindingUnconfirmed
                )
            )
            .then_some(tab.agent_type.as_deref())
            .flatten()
        })
}

impl ServiceContext {
    pub(super) fn failed_launch_restart_snapshot(
        &self,
        tab_id: &str,
    ) -> Result<Option<TabRestartSnapshot>, AgentRestartError> {
        let tab = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {tab_id} not found")))?;
            mux.get_tab(&resolved)
                .cloned()
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {resolved} not found")))?
        };
        let Some(provider) = failed_provider(&tab) else {
            return Ok(None);
        };
        if crate::agent::live_state::observe_tab_liveness(self, &tab.id) == AgentLiveness::Live {
            return Ok(None);
        }
        let transport = self
            .agent_registry
            .with_provider(provider, |provider| provider.describe().transport);
        if transport != Some(crate::agent::provider::ProviderTransport::Pty) {
            return Err(AgentRestartError::BadRequest(format!(
                "failed launch provider {provider} is not an available PTY provider"
            )));
        }
        let auto_approve = crate::service::permission_policy::configured_permission_bypass_for(
            &self.app_handle,
            &self.agent_registry,
            provider,
        )
        .map_err(AgentRestartError::BadRequest)?
        .is_some();
        let exact_session = tab
            .close_restore
            .as_ref()
            .is_some_and(|state| state.exact_session);
        let session_id = if exact_session {
            let exists =
                self.restart_session_exists(&tab, provider, tab.agent_session_id.as_deref());
            if exists != Some(true) {
                return Err(AgentRestartError::BadRequest(
                    "the exact restored session is unavailable; use agent restart --fresh to explicitly start a new session".into(),
                ));
            }
            tab.agent_session_id.clone()
        } else {
            let selected = super::exit_recovery::select_journal_recovery_session(
                &crate::session::lifecycle::TabLifecycleJournal::shared()
                    .load()
                    .records,
                &tab.id,
                tab.agent_session_id.as_deref(),
                &tab.prior_session_binding_nonces,
            );
            let saved_session = tab.agent_session_id.is_some()
                || tab.prior_session_binding_nonces.iter().any(|binding| {
                    binding.session_id.is_some() || binding.resume_target_session_id.is_some()
                });
            let selected = match selected {
                Ok(session) => Some(session),
                Err(error) if saved_session => return Err(error),
                Err(_) => None,
            };
            if let Some(session_id) = selected.as_deref() {
                let owner = super::exit_recovery::recovery_provider(
                    &self.agent_registry,
                    tab.cwd.as_deref(),
                    session_id,
                )?;
                if owner != provider {
                    return Err(AgentRestartError::BadRequest(
                        "recovery session belongs to a different provider".into(),
                    ));
                }
            }
            selected
        };
        Ok(Some(TabRestartSnapshot {
            tab_id: tab.id.clone(),
            pty_id: tab.pty_id.clone().unwrap_or_default(),
            provider: provider.to_string(),
            target_provider: None,
            session_id,
            cwd: tab.cwd.clone(),
            auto_approve,
            role: tab.role.map(|role| format!("{role:?}")),
            role_profile: tab.role_profile.clone(),
            handoff_history: None,
        }))
    }
}

#[cfg(test)]
mod tests;
