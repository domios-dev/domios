use crate::agent::provider::{ProviderDescriptor, ProviderTransport};

fn eligible(provider: &ProviderDescriptor) -> bool {
    let runtime = provider.agent_runtime.as_ref();
    (provider.transport == ProviderTransport::Pty || runtime.is_some())
        && runtime.is_none_or(|runtime| runtime.launch_profiles.is_none())
}

pub fn validate_general_agent_provider(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) -> Result<(), String> {
    match registry.with_provider(provider_id, |provider| eligible(&provider.describe())) {
        Some(true) => Ok(()),
        Some(false) => Err(format!(
            "general_agent_provider_ineligible: provider {provider_id} requires a launch profile or lacks a managed agent lifecycle"
        )),
        None => Err(format!("Unknown provider: {provider_id}")),
    }
}

#[cfg(test)]
mod tests;
