use codeterm_types::pty::{AgentLiveness, BootStage};

use super::{AgentRestartError, TabRestartSnapshot};
use crate::prelude::*;
use crate::service::ServiceContext;

fn provisional_launch_matches_tab(tab: &crate::multiplexer::types::Tab) -> bool {
    tab.provisional_launch.as_ref().is_none_or(|launch| {
        tab.pty_id.as_deref() == Some(launch.pty_id.as_str())
            && tab.session_binding_nonce.as_deref() == Some(launch.binding_nonce.as_str())
    })
}

fn failed_provider(tab: &crate::multiplexer::types::Tab) -> Option<&str> {
    if tab.launch_session_authenticated()
        || crate::service::boot_stage::first_turn_input_is_ready(tab)
    {
        return None;
    }
    if tab.agent_booting {
        let stale_boot_stage = match tab.boot_stage {
            Some(BootStage::PtyStarting) => true,
            Some(BootStage::LaunchDeadlineElapsed) => provisional_launch_matches_tab(tab),
            _ => false,
        };
        if !stale_boot_stage {
            return None;
        }
    }
    tab.restorable_launch
        .failed_provider
        .as_deref()
        .or_else(|| {
            matches!(
                tab.boot_stage,
                Some(
                    BootStage::PtyStarting
                        | BootStage::SpawnFailed
                        | BootStage::LaunchDeadlineElapsed
                        | BootStage::LaunchBindingUnconfirmed
                )
            )
            .then_some(tab.agent_type.as_deref())
            .flatten()
        })
}

fn latest_verified_rebind_session<'a>(
    records: &'a [crate::session::lifecycle::JournalRecord],
    tab_id: &str,
    provider: &str,
) -> Option<&'a str> {
    use crate::session::lifecycle::{AgentTypeWriteOrigin, JournalRecord, LifecycleTransition};

    records
        .iter()
        .rev()
        .find_map(|record| {
            if record.tab_id() != tab_id {
                return None;
            }
            match record {
                JournalRecord::Transition { entry } => match &entry.transition {
                    LifecycleTransition::AgentTypeWrite {
                        origin: AgentTypeWriteOrigin::Rebind,
                        new_provider,
                        session_id,
                        applied: true,
                        ..
                    } => Some((new_provider.as_deref(), session_id.as_deref())),
                    _ => None,
                },
                JournalRecord::Checkpoint { .. } => None,
            }
        })
        .and_then(|(recorded_provider, recorded_session)| {
            (recorded_provider == Some(provider))
                .then_some(recorded_session)
                .flatten()
        })
}

fn live_provider_process_present(
    svc: &ServiceContext,
    tab: &crate::multiplexer::types::Tab,
) -> bool {
    let Some(pty_id) = tab.pty_id.as_deref() else {
        return false;
    };
    if svc.pty.lock().get_session(pty_id).is_none() {
        return false;
    }
    svc.observe_provider_process_tree(&tab.id, pty_id)
        .is_some_and(|tree| {
            tree.state() == crate::service::process_tree::ProviderTreeState::Present
        })
}

fn tab_has_active_owner_or_launch(
    svc: &ServiceContext,
    tab: &crate::multiplexer::types::Tab,
) -> bool {
    crate::agent::live_state::observe_tab_liveness(svc, &tab.id) == AgentLiveness::Live
        || svc.tab_has_armed_launch(tab)
        || live_provider_process_present(svc, tab)
}

pub trait FailedLaunchExt {
    fn failed_launch_restart_snapshot(
        &self,
        tab_id: &str,
    ) -> Result<Option<TabRestartSnapshot>, AgentRestartError>;
    fn require_failed_launch_recovery_current(
        &self,
        snap: &TabRestartSnapshot,
    ) -> Result<(), AgentRestartError>;
}

impl FailedLaunchExt for ServiceContext {
    fn failed_launch_restart_snapshot(
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
        if tab_has_active_owner_or_launch(self, &tab) {
            return Ok(None);
        }
        if self
            .ext::<crate::service::agent_restart::RestartTracker>()
            .lock()
            .model_switch_busy(&tab.id)
        {
            return Err(AgentRestartError::BadRequest(
                "an agent restart or automatic recovery is already in progress".to_string(),
            ));
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
            self,
            &self.agent_registry,
            provider,
        )
        .map_err(AgentRestartError::BadRequest)?
        .is_some();
        let exact_session = tab
            .close_restore
            .as_ref()
            .is_some_and(|state| state.exact_session);
        let session_id = if exact_session
            || (tab.boot_stage == Some(BootStage::PtyStarting) && tab.agent_session_id.is_some())
        {
            let exists =
                self.restart_session_exists(&tab, provider, tab.agent_session_id.as_deref());
            if exists != Some(true) {
                return Err(AgentRestartError::BadRequest(
                    "the exact restored session is unavailable; use agent restart --fresh to explicitly start a new session".into(),
                ));
            }
            tab.agent_session_id.clone()
        } else {
            let records = self.lifecycle_journal.load().records;
            let selected = super::exit_recovery::select_journal_recovery_session(
                &records,
                &tab.id,
                tab.agent_session_id.as_deref(),
                &tab.prior_session_binding_nonces,
            )
            .or_else(|error| {
                tab.agent_session_id
                    .as_deref()
                    .filter(|current| {
                        latest_verified_rebind_session(&records, &tab.id, provider)
                            == Some(*current)
                    })
                    .map(str::to_string)
                    .ok_or(error)
            });
            let requires_verified_session = tab.boot_stage
                == Some(BootStage::LaunchDeadlineElapsed)
                || tab.agent_session_id.is_some()
                || tab.prior_session_binding_nonces.iter().any(|binding| {
                    binding.session_id.is_some() || binding.resume_target_session_id.is_some()
                });
            let selected = match selected {
                Ok(session) => Some(session),
                Err(error) if requires_verified_session => return Err(error),
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

    fn require_failed_launch_recovery_current(
        &self,
        snap: &TabRestartSnapshot,
    ) -> Result<(), AgentRestartError> {
        let tab = self
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .cloned()
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", snap.tab_id)))?;
        if failed_provider(&tab) != Some(snap.provider.as_str())
            || tab.pty_id.as_deref().unwrap_or_default() != snap.pty_id
            || tab.cwd != snap.cwd
            || tab_has_active_owner_or_launch(self, &tab)
        {
            return Err(AgentRestartError::BadRequest(
                "failed launch recovery superseded: tab launch or liveness changed; retry by intent"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
