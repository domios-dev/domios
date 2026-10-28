use super::*;
use crate::service::neighbors::storage::Settings;

impl ServiceContext {
    pub(in super::super) fn neighbor_launch_options(
        &self,
        settings: &Settings,
        friendship: &str,
    ) -> ConciergeOptions {
        let Some(tab) = settings.bindings.get(friendship) else {
            return ConciergeOptions::default();
        };
        settings
            .initial_contexts
            .get(friendship)
            .filter(|context| &context.gateway_tab == tab)
            .and_then(|context| context.launch_options.clone())
            .unwrap_or_else(|| self.neighbor_tab_options(tab))
    }

    pub(super) fn neighbor_tab_options(&self, id: &str) -> ConciergeOptions {
        let mux = self.mux.read();
        let Some(tab) = mux.get_tab(id) else {
            return ConciergeOptions::default();
        };
        let service_tier = tab
            .agent_runtime_config
            .as_ref()
            .and_then(|config| config.get("serviceTier"))
            .map(|selection| {
                selection
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_owned()
            });
        ConciergeOptions {
            reasoning_effort: tab.restorable_launch.requested_reasoning_effort.clone(),
            service_tier,
        }
    }

    pub(in super::super) fn validate_neighbor_launch_options(
        &self,
        provider: &str,
        model: Option<&str>,
        options: &ConciergeOptions,
    ) -> Result<(), String> {
        crate::service::spawn::validate_agent_reasoning_effort(
            &self.agent_registry,
            provider,
            model,
            options.reasoning_effort.as_deref(),
        )?;
        crate::service::spawn_speed::validate_agent_speed(
            &self.agent_registry,
            provider,
            options.service_tier.as_deref(),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
