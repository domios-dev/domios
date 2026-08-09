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
        Some(agent_system_prompt(assignment.as_deref()))
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
    let mut sections = vec![
        crate::prompts::AGENT_CODETERM_PRIMER.trim().to_string(),
        crate::prompts::AGENT_PAD_RULE.trim().to_string(),
        crate::prompts::RICH_RENDERING_GUIDANCE.trim().to_string(),
    ];
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
    let mut sections = vec![crate::service::persona::guidance_for(
        crate::service::persona::Occasion::WorkerLaunchIdentity,
        Some(crate::multiplexer::types::AgentRole::Worker),
        role_profile,
        identity,
    )];
    sections.push(crate::prompts::AGENT_CODETERM_PRIMER.trim().to_string());
    sections.push(crate::prompts::WORKER_CODETERM_CONTEXT.trim().to_string());
    sections.push(crate::prompts::AGENT_PAD_RULE.trim().to_string());
    sections.push(crate::prompts::RICH_RENDERING_GUIDANCE.trim().to_string());
    if !assignment.trim().is_empty() {
        sections.push(format!("## Assignment\n\n{}", assignment.trim()));
    }
    sections.join("\n\n")
}

#[cfg(test)]
pub(crate) fn spawn_onboarding_task(params: &SpawnAgentParams) -> Option<String> {
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
    role_launch_task(
        params.task.as_deref(),
        params.role.as_deref(),
        params.role_profile.as_deref(),
        system_prompt_capable,
    )
}
