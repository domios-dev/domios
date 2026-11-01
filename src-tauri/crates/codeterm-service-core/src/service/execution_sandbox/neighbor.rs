use super::*;

pub(super) fn apply_roots(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    runtime: bool,
    policy: &mut Option<ExecutionSandboxPolicy>,
) -> Result<bool, String> {
    if !svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|tab| tab.neighbor_authority.is_some())
    {
        return Ok(false);
    }
    let Some(policy) = policy else {
        return Ok(true);
    };
    if policy.intent == ExecutionIntent::ReadOnly {
        let mode = runtime
            .then(|| {
                svc.agent_registry
                    .with_provider(provider, |provider| {
                        provider
                            .execution_sandbox_capability()
                            .and_then(|capability| capability.runtime_read_only_writable_roots_mode)
                    })
                    .flatten()
            })
            .flatten();
        let Some(mode) = mode else {
            return Ok(true);
        };
        policy.runtime_mode = Some(mode);
    }
    let pad = svc.ensure_agent_pad(tab)?;
    let path = pad
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let root = path
        .to_str()
        .map(codeterm_paths::strip_verbatim_prefix)
        .ok_or("Neighbor AgentPad must be Unicode")?;
    policy.writable_roots = vec![root];
    policy.environment.clear();
    Ok(true)
}

#[cfg(test)]
mod tests;
