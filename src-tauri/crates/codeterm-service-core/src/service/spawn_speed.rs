use crate::agent::registry::AgentRegistry;
use codeterm_plugin::agent_runtime::{RuntimeServiceTier, RuntimeServiceTierSelection};

pub fn requested_service_tier(
    tiers: &[RuntimeServiceTier],
    requested: &str,
) -> Result<RuntimeServiceTierSelection, String> {
    if !tiers.is_empty() && requested.eq_ignore_ascii_case("default") {
        return Ok(RuntimeServiceTierSelection { id: None });
    }
    tiers
        .iter()
        .find(|tier| tier.id == requested)
        .map(|tier| RuntimeServiceTierSelection {
            id: Some(tier.id.clone()),
        })
        .ok_or_else(|| format!("Unsupported service tier: {requested}"))
}

pub fn validate_agent_speed(
    registry: &AgentRegistry,
    provider: &str,
    speed: Option<&str>,
) -> Result<Option<RuntimeServiceTierSelection>, String> {
    let Some(speed) = speed else { return Ok(None) };
    registry
        .with_provider(provider, |provider| {
            let tiers = provider
                .agent_runtime()
                .map(|capability| capability.service_tiers)
                .unwrap_or_default();
            requested_service_tier(&tiers, speed).map(Some)
        })
        .unwrap_or_else(|| Err(format!("Unknown provider: {provider}")))
}

#[cfg(test)]
mod tests;
