use super::*;

impl ServiceContext {
    pub(super) fn claim_general_provider_switch(
        &self,
        tab: &Tab,
    ) -> Result<Option<crate::service::general_agent::launch_lock::LaunchGuard>, String> {
        if !crate::service::general_agent::session_contract::owns_main(&self.mux.read(), tab) {
            return Ok(None);
        }
        crate::service::general_agent::launch_lock::LaunchGuard::acquire()
            .map(Some)
            .ok_or("provider_switch_general_busy: General launch is already active".into())
    }

    pub(super) fn general_provider_switch_prompt(&self, tab: &Tab, target: &str) -> Option<String> {
        crate::service::general_agent::is_general_agent(tab).then(|| {
            crate::service::general_agent::instructions::render_system_prompt(
                self.configuration().general_agent.capability_hints,
                target,
                Some(&self.mem),
            )
        })
    }

    pub(super) fn persist_general_provider_switch(
        &self,
        tab: &Tab,
        provider: &str,
        launch: &RestorableLaunchParameters,
        session: &str,
    ) -> Result<(), String> {
        self.persist_general_provider_switch_with(
            tab,
            provider,
            launch,
            crate::config::save_config_pub,
        )?;
        if crate::service::general_agent::session_contract::owns_main(&self.mux.read(), tab) {
            crate::service::general_agent::persist_switched_session(provider, session);
        }
        Ok(())
    }

    fn persist_general_provider_switch_with(
        &self,
        tab: &Tab,
        provider: &str,
        launch: &RestorableLaunchParameters,
        save: impl FnOnce(&crate::config::schema::AppConfig) -> Result<(), String>,
    ) -> Result<(), String> {
        if !crate::service::general_agent::session_contract::owns_main(&self.mux.read(), tab) {
            return Ok(());
        }
        let config = self.ext::<crate::config::ConfigState>();
        let mut current = config.lock();
        let mut next = current.clone();
        next.general_agent.agent_provider = provider.into();
        next.general_agent.model = launch.model.clone();
        next.general_agent.reasoning_effort = launch.resolved_reasoning_effort.clone();
        save(&next).map_err(|error| {
            format!("provider_switch_default_persist_failed: target is running; {error}")
        })?;
        *current = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
