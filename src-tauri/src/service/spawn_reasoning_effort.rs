use crate::agent::registry::AgentRegistry;

/// Validate against model levels when present and use the provider fallback otherwise.
pub(crate) fn validate_agent_reasoning_effort(
    registry: &AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<(), String> {
    let Some(effort) = effort.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(());
    };
    match registry.with_provider(provider_id, |p| {
        if let Some(model) = model {
            let supported = p.reasoning_efforts_for_model(Some(model));
            if !supported.iter().any(|level| level.id == effort) {
                let levels = supported
                    .iter()
                    .map(|level| level.id.as_str())
                    .collect::<Vec<_>>();
                let levels = if levels.is_empty() {
                    "none".to_string()
                } else {
                    levels.join(", ")
                };
                return Err(format!(
                    "model '{model}' does not support reasoning effort '{effort}'; supported: {levels}"
                ));
            }
        } else {
            p.validate_reasoning_effort(Some(effort))
                .map_err(|rejection| rejection.to_string())?;
        }
        // An advertised level the provider cannot put on its command line would be dropped at launch.
        if p.reasoning_effort_launch_args(effort).is_empty() {
            return Err(format!(
                "provider '{provider_id}' declares no launch flag for reasoning effort '{effort}'; omit --reasoning-effort"
            ));
        }
        Ok(())
    }) {
        Some(result) => result,
        None => Err(format!(
            "unknown provider '{provider_id}': cannot validate reasoning effort '{effort}'"
        )),
    }
}
