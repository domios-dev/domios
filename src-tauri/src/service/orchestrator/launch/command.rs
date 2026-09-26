use super::{AgentRegistry, ServiceContext};
use crate::agent::provider::AgentLaunchParams;
use crate::service::orchestrator::types::SpawnAgentParams;

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

pub(crate) fn build_agent_command_for_service(
    service: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
) -> String {
    build_agent_command_inner(
        Some((service, tab_id)),
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
        None,
    )
}

pub(crate) fn build_agent_command_with_launch_marker(
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
    launch_marker: Option<&str>,
) -> String {
    build_agent_command_inner(
        None,
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
        launch_marker,
    )
}

pub(crate) fn build_agent_command_with_launch_marker_for_service(
    service: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
    launch_marker: Option<&str>,
) -> String {
    build_agent_command_inner(
        Some((service, tab_id)),
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
        launch_marker,
    )
}

fn build_agent_command_inner(
    notice_context: Option<(&ServiceContext, &str)>,
    agent_cmd: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &AgentRegistry,
    launch_marker: Option<&str>,
) -> String {
    let mut args: Vec<String> = params.args.clone().unwrap_or_default();
    let resolved_model = params
        .model
        .as_deref()
        .filter(|model| !model.is_empty())
        .map(|model| {
            if registry.with_provider(agent_cmd, |_| ()).is_some() {
                crate::service::spawn::resolve_fresh_agent_model(
                    registry,
                    agent_cmd,
                    Some(model.to_string()),
                    None,
                )
                .unwrap_or_default()
            } else {
                crate::agent::model_catalog::resolve_model(agent_cmd, Some(model)).id
            }
        });
    if let Some(model) = resolved_model.as_deref() {
        let model_args = registry
            .with_provider(agent_cmd, |provider| provider.model_launch_args(model))
            .unwrap_or_else(|| vec!["--model".into(), model.to_string()]);
        args.extend(model_args);
    }
    if let Some(effort) = params
        .reasoning_effort
        .as_deref()
        .filter(|effort| !effort.is_empty())
    {
        let effort_args = if let Some((service, tab_id)) = notice_context {
            service.reasoning_effort_launch_args_or_notice(
                tab_id,
                agent_cmd,
                resolved_model.as_deref(),
                Some(effort),
            )
        } else {
            match registry.with_provider(agent_cmd, |provider| {
                let supported = provider.reasoning_efforts_for_model(resolved_model.as_deref());
                supported
                    .iter()
                    .any(|level| level.id == effort)
                    .then(|| provider.reasoning_effort_launch_args(effort))
            }) {
                Some(Some(args)) => args,
                _ => {
                    log::warn!(
                        "[reasoning-effort] provider={agent_cmd} model={} requested={effort}; omitted from launch without service notice context",
                        resolved_model.as_deref().unwrap_or("provider default")
                    );
                    Vec::new()
                }
            }
        };
        args.extend(effort_args);
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

    if let Some(session_id) = &params.resume_session_id {
        let resume = crate::agent::provider::AgentResumeParams {
            session_id: session_id.clone(),
            skip_permissions: params.permission_bypass_requested(agent_cmd),
            system_prompt: system_prompt.clone(),
            args: args.clone(),
        };
        if let Some(command) = registry
            .with_provider(agent_cmd, |provider| {
                provider.build_resume_command_with_context(&resume)
            })
            .flatten()
        {
            return command;
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
        task: launch_task,
        starter_prompt: params.starter_prompt,
        args: if args.is_empty() {
            None
        } else {
            Some(args.clone())
        },
        skip_permissions: params.permission_bypass_requested(agent_cmd),
        session_id: injected_session_id.map(str::to_string),
        structured_prompt_server: params.structured_prompt_server.clone(),
        launch_marker: launch_marker.map(str::to_string),
        instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
    };
    match registry.with_provider(agent_cmd, |provider| {
        provider.build_launch_command(&launch_params)
    }) {
        Some(command) if command.trim().is_empty() => String::new(),
        Some(command) => crate::agent::stub_launch::apply(agent_cmd, command),
        None => {
            let mut parts = vec![agent_cmd.to_string()];
            parts.extend(args);
            parts.join(" ")
        }
    }
}
