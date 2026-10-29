use super::*;

mod reservation;
pub(in crate::service::agent_restart) use reservation::SessionlessTeardownError;

impl ModelSwitchLaunch {
    pub(in crate::service::agent_restart) fn is_sessionless(&self) -> bool {
        self.session_id.is_empty()
    }

    #[cfg(test)]
    pub(in crate::service::agent_restart) fn sessionless_is_current(
        &self,
        svc: &ServiceContext,
        snap: &TabRestartSnapshot,
    ) -> bool {
        svc.sessionless_model_switch_ready(snap)
            && self.sessionless_attempt.as_ref().is_some_and(|attempt| {
                svc.mux.read().get_tab(&snap.tab_id).is_some_and(|tab| {
                    crate::service::launch_readiness::attempt_owns_tab(tab, attempt)
                })
            })
    }
}

impl ServiceContext {
    pub(in crate::service::agent_restart) fn sessionless_model_switch_ready(
        &self,
        snap: &TabRestartSnapshot,
    ) -> bool {
        if snap.session_id.is_some() || !self.lazy_first_turn_restart_ready(&snap.tab_id) {
            return false;
        }
        self.mux.read().get_tab(&snap.tab_id).is_some_and(|tab| {
            tab.agent_session_id.is_none()
                && tab.pty_id.as_deref() == Some(snap.pty_id.as_str())
                && tab.agent_type.as_deref() == Some(snap.provider.as_str())
                && tab.provisional_launch.as_ref().is_some_and(|launch| {
                    launch.first_turn_admitted_send_id.is_none()
                        && !launch.first_turn_payload_written
                })
        })
    }

    pub(super) fn switch_model_before_first_turn(
        &self,
        snap: TabRestartSnapshot,
        requested: &str,
        effort: Option<String>,
        previous_model: Option<String>,
        force: bool,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        self.require_background_work_safe(&snap.tab_id, force)?;
        let mut launch = ModelSwitchLaunch::prepare(self, &snap, "", requested)?;
        launch.params.reasoning_effort = effort.clone();
        launch.launch.requested_reasoning_effort = effort.clone();
        launch.launch.resolved_reasoning_effort = effort;
        let launched_model = launch.launched_model().to_string();
        let restart = self.restart_live_agent(
            snap,
            force,
            DEFAULT_GRACE_SECS,
            Some(&launch),
            super::super::RestartMode::SkipExit,
        )?;
        Ok(ModelSwitchOutcome {
            tab_id: restart.tab_id.clone(),
            provider: restart.provider.clone(),
            session_id: String::new(),
            restart: Some(restart),
            mechanism: ModelSwitchMode::RestartResume,
            fallback_reason: None,
            previous_model,
            requested_model: requested.to_string(),
            launched_model,
            confirmation_detail: Some("model will be confirmed by the first user turn".into()),
            confirmation: ModelSwitchConfirmation::Pending,
        })
    }
}

#[cfg(test)]
mod tests;
