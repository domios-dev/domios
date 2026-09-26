use super::{AgentLaunchParams, AgentRegistry};
use crate::service::orchestrator::types::SpawnAgentParams;

/// Build a single shell command that launches the agent.
/// Uses the AgentRegistry for known providers; falls back to simple command for unknown agents.
pub(crate) fn build_agent_command(
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
) -> String {
    build_agent_command_with_launch_marker(
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
        None,
    )
}

/// `launch_marker` is the inert per-launch token a provider may carry into its
/// own session record for hook-independent binding.
pub(crate) fn build_agent_command_with_launch_marker(
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
    launch_marker: Option<&str>,
) -> String {
    let mut args: Vec<String> = params.args.clone().unwrap_or_default();
    if let Some(ref model) = params.model {
        if !model.is_empty() {
            let resolved = crate::agent::model_catalog::resolve_model(agent_cmd, Some(model));
            let model_args = registry
                .with_provider(agent_cmd, |p| p.model_launch_args(&resolved.id))
                .unwrap_or_else(|| vec!["--model".into(), resolved.id]);
            args.extend(model_args);
        }
    }
    if let Some(ref effort) = params.reasoning_effort {
        if !effort.is_empty() {
            if let Some(effort_args) = registry
                .with_provider(agent_cmd, |p| {
                    p.supports_reasoning_effort(effort)
                        .then(|| p.reasoning_effort_launch_args(effort))
                })
                .flatten()
            {
                args.extend(effort_args);
            }
        }
    }

    let system_prompt = registry
        .with_provider(agent_cmd, |provider| {
            crate::service::launch_delivery::content_for_descriptor(
                &provider.system_prompt_delivery(),
                task,
            )
            .system_prompt
        })
        .flatten();

    if let Some(ref sid) = params.resume_session_id {
        let resume = crate::agent::provider::AgentResumeParams {
            session_id: sid.clone(),
            skip_permissions: params.permission_bypass_requested(agent_cmd),
            system_prompt: system_prompt.clone(),
            args: args.clone(),
        };
        if let Some(cmd) = registry
            .with_provider(agent_cmd, |p| p.build_resume_command_with_context(&resume))
            .flatten()
        {
            return cmd;
        }
    }

    let launch_task = registry
        .with_provider(agent_cmd, |provider| {
            system_prompt.clone().or_else(|| {
                matches!(
                    provider.task_delivery(),
                    crate::agent::provider::TaskDelivery::InlineFlag
                )
                .then(|| task.map(str::to_string))
                .flatten()
            })
        })
        .unwrap_or_default();
    let launch_params = AgentLaunchParams {
        agent_name: params.agent_name.clone(),
        task: launch_task.clone(),
        starter_prompt: params.starter_prompt,
        args: if args.is_empty() {
            None
        } else {
            Some(args.clone())
        },
        skip_permissions: params.permission_bypass_requested(agent_cmd),
        session_id: injected_session_id.map(std::string::ToString::to_string),
        structured_prompt_server: params.structured_prompt_server.clone(),
        launch_marker: launch_marker.map(str::to_string),
        instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
    };
    match registry.with_provider(agent_cmd, |p| p.build_launch_command(&launch_params)) {
        Some(command) if command.trim().is_empty() => String::new(),
        Some(command) => crate::agent::stub_launch::apply(agent_cmd, command),
        None => {
            let mut parts: Vec<String> = vec![agent_cmd.to_string()];
            parts.extend(args);
            parts.join(" ")
        }
    }
}
