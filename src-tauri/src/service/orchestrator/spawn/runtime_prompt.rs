use super::{role_onboarding_task, SpawnAgentParams};

pub(super) fn compose_runtime_system_prompt(params: &mut SpawnAgentParams) -> Option<String> {
    let identity = match params.manager_bootstrap.as_ref() {
        Some(manager) => role_onboarding_task(
            Some(&manager.compose()),
            params.role.as_deref(),
            params.role_profile.as_deref(),
        ),
        None => crate::service::orchestrator::prompt_compose::restorable_identity_task(
            params.task.as_deref(),
            params.role.as_deref(),
            params.role_profile.as_deref(),
        ),
    };
    Some(
        identity
            .into_iter()
            .chain(params.system_prompt.take())
            .filter(|fragment| !fragment.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}

#[cfg(test)]
mod tests;
