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
            if supported.iter().any(|level| level.id == effort) {
                return Ok(());
            }
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
        p.validate_reasoning_effort(Some(effort))
            .map_err(|rejection| rejection.to_string())
    }) {
        Some(Ok(())) => Ok(()),
        Some(Err(rejection)) => Err(rejection.to_string()),
        None => Err(format!(
            "unknown provider '{provider_id}': cannot validate reasoning effort '{effort}'"
        )),
    }
}
