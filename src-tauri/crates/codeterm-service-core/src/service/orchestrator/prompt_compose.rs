//! Launch-prompt composition. Which fragments a launched agent receives, in
//! which order, for each role — separate from the PTY launch lifecycle that
//! delivers them.

use crate::agent::registry::AgentRegistry;
use crate::service::orchestrator::types::SpawnAgentParams;

#[cfg(test)]
#[path = "prompt_compose/tests.rs"]
mod tests;

fn role_assignment_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    // Resolve the pair, not the role alone: `from_str_compat` folds the
    // Orchestrator literal into Manager and the kind is unrecoverable after.
    let resolved = codeterm_types::agent::restore_role(role, role_profile);
    crate::service::persona::deliver(
        crate::service::persona::Occasion::LaunchAssignment,
        resolved.as_ref().map(|resolved| resolved.role),
        resolved
            .as_ref()
            .and_then(|resolved| resolved.role_profile.as_deref()),
        None,
        task,
    )
}

pub(crate) fn role_onboarding_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    let assignment = role_assignment_task(task, role, role_profile);
    let resolved = codeterm_types::agent::restore_role(role, role_profile);
    let kind = resolved
        .as_ref()
        .and_then(|resolved| resolved.role_profile.as_deref());
    if resolved.as_ref().map(|resolved| resolved.role)
        == Some(crate::multiplexer::types::AgentRole::Worker)
    {
        Some(worker_agent_system_prompt(assignment.as_deref(), kind))
    } else {
        let mut prompt = agent_system_prompt(assignment.as_deref());
        if resolved.is_none() {
            prompt.push_str("\n\n");
            prompt.push_str(crate::prompts::MEM_MANAGEMENT.trim());
        }
        Some(prompt)
    }
}

pub(crate) fn role_launch_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
    system_prompt_capable: bool,
) -> Option<String> {
    if system_prompt_capable || role.is_some() {
        role_onboarding_task(task, role, role_profile)
    } else {
        role_assignment_task(task, role, role_profile)
    }
}

pub(crate) fn provider_launch_task(
    registry: &AgentRegistry,
    provider_id: &str,
    assignment: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    if registry
        .with_provider(provider_id, |provider| provider.is_tool_less())
        .unwrap_or(false)
    {
        return tool_less_prompt(assignment);
    }
    let system_prompt_capable = registry
        .with_provider(provider_id, |provider| {
            matches!(
                crate::service::launch_delivery::mode_for_descriptor(
                    &provider.system_prompt_delivery(),
                ),
                crate::service::launch_delivery::LaunchDeliveryMode::SystemPrompt
            )
        })
        .unwrap_or(false);
    role_launch_task(assignment, role, role_profile, system_prompt_capable)
}

pub(crate) fn agent_system_prompt(assignment: Option<&str>) -> String {
    agent_system_prompt_with_plugins(
        assignment,
        crate::service::plugin_prompt::built_in_plugins_section().as_deref(),
    )
}

fn primer_sections(built_in_plugins: Option<&str>) -> Vec<String> {
    let mut sections = vec![crate::prompts::AGENT_CODETERM_PRIMER.trim().to_string()];
    sections.extend(built_in_plugins.map(str::to_string));
    sections
}

fn agent_system_prompt_with_plugins(
    assignment: Option<&str>,
    built_in_plugins: Option<&str>,
) -> String {
    let mut sections = primer_sections(built_in_plugins);
    sections.extend([
        crate::prompts::UTILITY_TAB_GUIDANCE.trim().to_string(),
        crate::prompts::AGENT_PAD_RULE.trim().to_string(),
        crate::prompts::RICH_RENDERING_GUIDANCE.trim().to_string(),
    ]);
    if let Some(assignment) = assignment.filter(|text| !text.trim().is_empty()) {
        sections.push(format!("## Assignment\n\n{assignment}"));
    }
    sections.join("\n\n")
}

/// The task-owned persona is the Worker preset's overlay: it enters the launch
/// prompt through the same seam manager kinds use, never a private path.
fn worker_agent_system_prompt(assignment: Option<&str>, role_profile: Option<&str>) -> String {
    let (identity, assignment) = assignment
        .map(crate::service::plan::worker_identity::split_worker_identity)
        .unwrap_or((None, ""));
    worker_agent_system_prompt_from_identity(identity, assignment, role_profile)
}

fn worker_agent_system_prompt_from_identity(
    identity: Option<&str>,
    assignment: &str,
    role_profile: Option<&str>,
) -> String {
    worker_agent_system_prompt_with_plugins(
        identity,
        assignment,
        role_profile,
        crate::service::plugin_prompt::built_in_plugins_section().as_deref(),
    )
}

fn worker_agent_system_prompt_with_plugins(
    identity: Option<&str>,
    assignment: &str,
    role_profile: Option<&str>,
    built_in_plugins: Option<&str>,
) -> String {
    let mut sections = vec![crate::service::persona::guidance_for(
        crate::service::persona::Occasion::WorkerLaunchIdentity,
        Some(crate::multiplexer::types::AgentRole::Worker),
        role_profile,
        identity,
    )];
    sections.extend(primer_sections(built_in_plugins));
    sections.push(crate::prompts::WORKER_CODETERM_CONTEXT.trim().to_string());
    sections.push(crate::prompts::AGENT_PAD_RULE.trim().to_string());
    sections.push(crate::prompts::RICH_RENDERING_GUIDANCE.trim().to_string());
    if !assignment.trim().is_empty() {
        sections.push(format!("## Assignment\n\n{}", assignment.trim()));
    }
    sections.join("\n\n")
}

pub(crate) fn restorable_identity_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    let resolved = codeterm_types::agent::restore_role(role, role_profile);
    if resolved.as_ref().map(|resolved| resolved.role)
        == Some(crate::multiplexer::types::AgentRole::Worker)
    {
        let identity = task
            .and_then(|task| crate::service::plan::worker_identity::split_worker_identity(task).0);
        Some(worker_agent_system_prompt_from_identity(
            identity,
            "",
            role_profile,
        ))
    } else {
        role_onboarding_task(None, role, role_profile)
    }
}

#[cfg(test)]
pub fn spawn_onboarding_task(params: &SpawnAgentParams) -> Option<String> {
    role_onboarding_task(
        params.task.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
    )
}

pub(crate) fn spawn_launch_task(
    params: &SpawnAgentParams,
    system_prompt_capable: bool,
) -> Option<String> {
    let prompt = role_launch_task(
        params.task.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
        system_prompt_capable,
    );
    let target = (
        params.report_target_mesh_peer.as_deref().map(str::trim),
        params.report_target_tab_id.as_deref().map(str::trim),
    );
    let (Some(peer), Some(tab)) = target else {
        return prompt;
    };
    if peer.is_empty() || tab.is_empty() {
        return prompt;
    }
    let peer = params
        .ownership
        .as_ref()
        .filter(|owner| owner.peer_name == peer || owner.peer_id == peer)
        .map(|owner| owner.peer_id.trim())
        .filter(|id| !id.is_empty())
        .unwrap_or(peer);
    let route = serde_json::json!({"mesh_peer": peer, "tab_id": tab});
    let context = format!(
        "## Host return route\n\n{route}\n\nSend reports to this exact remote tab using `codeterm send` with `--mesh` and `--tab` from this record, `--require-agent` and `--from-pad`. This daemon-supplied route overrides peer names in the assignment; do not guess or rediscover it."
    );
    Some(match prompt {
        Some(prompt) => format!("{prompt}\n\n{context}"),
        None => context,
    })
}

pub(crate) fn tool_less_prompt(assignment: Option<&str>) -> Option<String> {
    assignment.map(|assignment| {
        let (identity, assignment) =
            crate::service::plan::worker_identity::split_worker_identity(assignment);
        let assignment = assignment
            .strip_prefix(crate::service::plan::worker_identity::WORKER_ACK_INSTRUCTION)
            .unwrap_or(assignment)
            .trim_start();
        [identity, Some(assignment)]
            .into_iter()
            .flatten()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}

pub(crate) fn spawn_launch_task_for_provider(
    registry: &AgentRegistry,
    params: &SpawnAgentParams,
    system_prompt_capable: bool,
) -> Option<String> {
    if registry
        .with_provider(params.agent.as_deref().unwrap_or_default(), |provider| {
            provider.is_tool_less()
        })
        .unwrap_or(false)
    {
        return tool_less_prompt(params.task.as_deref());
    }
    spawn_launch_task(params, system_prompt_capable)
}
