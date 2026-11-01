use super::*;

/// Build effort provenance for a launch where the requested effort has already
/// passed capability validation. No silent downgrade ⇒ `resolved == requested`.
/// `actual` stays `None` — the launch flag is a request, not runtime proof.
pub(crate) fn resolved_effort_provenance(
    planned: Option<String>,
    requested: Option<String>,
) -> ReasoningEffortProvenance {
    let has_effort = planned.is_some() || requested.is_some();
    ReasoningEffortProvenance {
        planned,
        requested: requested.clone(),
        resolved: requested,
        actual: None,
        actual_source: has_effort.then(|| "provider-default-unverified".into()),
    }
}

/// An idle Team bootstrap is instructions, not work: a provider that can carry
/// it as a system prompt needs no visible starter turn to repeat it.
pub(super) fn prepare_manager_bootstrap(
    params: &mut SpawnAgentParams,
    system_prompt_capable: bool,
) -> Option<String> {
    let has_assignment = params
        .task
        .as_deref()
        .is_some_and(|task| !task.trim().is_empty());
    let bootstrap = params.manager_bootstrap.as_ref()?.compose();
    params.task = Some(match params.task.take() {
        Some(task) if !task.trim().is_empty() => format!("{bootstrap}\n\n{task}"),
        None => bootstrap.clone(),
        Some(_) => bootstrap.clone(),
    });
    params.starter_prompt = match (has_assignment, system_prompt_capable) {
        (true, _) => crate::agent::provider::StarterPrompt::AssignedTask,
        (false, true) => crate::agent::provider::StarterPrompt::NoStarter,
        (false, false) => crate::agent::provider::StarterPrompt::TeamBootstrap,
    };
    Some(bootstrap)
}

/// The real bound Team id a manager bootstrap carries, or None for an empty or
/// `UNBOUND_TEAM` placeholder so nothing is invented. Caller checks liveness.
pub(super) fn bootstrap_bound_team_id(
    bootstrap: Option<&crate::service::role_identity::ManagerBootstrapInputs>,
) -> Option<&str> {
    let team_id = bootstrap?.team_id.trim();
    (!team_id.is_empty() && team_id != crate::service::spawn_factory::UNBOUND_TEAM)
        .then_some(team_id)
}

pub(super) fn prepare_standard_starter_prompt(params: &mut SpawnAgentParams) {
    params.starter_prompt = crate::service::spawn_params::starter_prompt_for_creation(
        params.role.as_deref(),
        params.task.as_deref(),
    );
}

pub(super) fn apply_startup_handshake(
    params: &mut SpawnAgentParams,
    supervised: bool,
    carries_startup_attempt: bool,
) {
    if let Some(prompt) = crate::service::launch_delivery::startup_starter_prompt_for_class(
        supervised && !params.user_created,
        params.task_launch_authority.is_some(),
        false,
        carries_startup_attempt,
    ) {
        params.starter_prompt = prompt;
    }
}

pub(super) fn team_bootstrap_payload(system_prompt_capable: bool, body: String) -> String {
    if system_prompt_capable {
        return body;
    }
    crate::service::messenger::Messenger::from(
        crate::service::messenger::MessageEnvelope::codeterm_reminder("team-bootstrap"),
    )
    .wrap(&body)
}

pub(super) fn build_or_refuse_orchestrated_launch_command(
    service: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    params: &SpawnAgentParams,
    bg_task: Option<&str>,
    launch_session_id: Option<&str>,
) -> Result<super::super::PreparedAgentCommand, String> {
    let launch_cmd =
        crate::service::launch_session_detection::marked_prepared_agent_command_for_service(
            service,
            tab_id,
            agent_cmd,
            params,
            bg_task,
            launch_session_id,
            &service.agent_registry,
        );
    if launch_cmd.trim().is_empty() {
        return Err(service.rollback_empty_agent_launch_if_created(
            tab_id,
            agent_cmd,
            params.existing_tab_id.is_none(),
        ));
    }
    Ok(launch_cmd)
}

pub(super) fn planner_startup_payload(
    system_prompt_capable: bool,
    manager_prompt: Option<String>,
    onboarding: &str,
) -> String {
    match manager_prompt {
        Some(prompt) => format!("{prompt}\n\n## Planner startup\n\n{onboarding}"),
        None if system_prompt_capable => agent_system_prompt(Some(onboarding)),
        None => onboarding.to_string(),
    }
}

pub(super) fn spawn_fallback_spec(
    input: &crate::service::launch_delivery::SpawnInput<'_>,
    planner: bool,
    bootstrap: Option<&str>,
    task: Option<&str>,
    starter: Option<&str>,
) -> Option<crate::service::launch_delivery::FallbackDeliverySpec> {
    input.fallback_spec(if planner { task } else { bootstrap }, task, starter)
}

pub(super) fn compose_chatbackend_system_prompt(params: &mut SpawnAgentParams) -> Option<String> {
    let assignment = if params.role_kind() == Some(builtins::PLANNER) {
        match (params.system_prompt.take(), params.task.take()) {
            (Some(system), Some(task)) => Some(format!("{system}\n\n{task}")),
            (system, task) => system.or(task),
        }
    } else {
        params.system_prompt.take().or_else(|| params.task.take())
    };
    let assignment = match params.manager_bootstrap.as_ref() {
        Some(manager) => {
            let bootstrap = manager.compose();
            Some(match assignment {
                Some(assignment) if !assignment.trim().is_empty() => {
                    format!("{bootstrap}\n\n{assignment}")
                }
                _ => bootstrap,
            })
        }
        None => assignment,
    };
    role_onboarding_task(
        assignment.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
    )
}

#[cfg(test)]
mod tests;
