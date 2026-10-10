use super::{AgentRegistry, ServiceContext};
use crate::agent::provider::AgentLaunchParams;
use crate::service::orchestrator::types::SpawnAgentParams;

mod selection_args;

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
    if let Some((service, tab_id)) = notice_context {
        crate::service::directory_pre_trust::for_tab(
            service,
            registry,
            agent_cmd,
            tab_id,
            params.cwd.as_deref(),
        );
    }
    let resolved_model = params
        .model
        .as_deref()
        .filter(|model| !model.is_empty())
        .and_then(|model| {
            if registry.with_provider(agent_cmd, |_| ()).is_some() {
                let resolution = crate::service::spawn::resolve_fresh_agent_model_with_notice(
                    registry,
                    agent_cmd,
                    Some(model.to_string()),
                    params.resume_session_id.as_deref(),
                );
                if params.resume_session_id.is_some() {
                    if let Some(warning) = resolution.warning.as_deref() {
                        log::warn!(
                            "[model-resolution] restore tab={} provider={agent_cmd}: {warning}",
                            notice_context.map_or("unknown", |(_, tab_id)| tab_id)
                        );
                    }
                    if let (Some((service, tab_id)), Some(notice)) =
                        (notice_context, resolution.restore_notification.as_deref())
                    {
                        service.notify_model_resolution_warning(tab_id, agent_cmd, notice);
                    }
                }
                resolution.model
            } else {
                let model = crate::agent::model_catalog::resolve_model(agent_cmd, Some(model)).id;
                (!model.trim().is_empty()).then_some(model)
            }
        });
    let mut resolved_params = params.clone();
    resolved_params.model = resolved_model.clone();
    let model_arguments = params.model.as_deref().and_then(|model| {
        registry.with_provider(agent_cmd, |provider| provider.model_launch_args(model))
    });
    resolved_params.args = selection_args::without_managed_arguments(
        params.args.as_deref(),
        model_arguments.as_deref().unwrap_or_default(),
    );
    let mut args = resolved_params.args.clone().unwrap_or_default();
    if let Some((service, tab_id)) = notice_context {
        if let Some(session) = params
            .resume_session_id
            .as_deref()
            .filter(|_| !params.fork_session)
        {
            crate::service::launch_session_detection::register_resume_launch_marker(
                tab_id, session,
            );
        }
        if let Some(command) = crate::service::codex_rpc::launch::defer(
            service,
            tab_id,
            agent_cmd,
            &resolved_params,
            task,
            injected_session_id,
        ) {
            return command;
        }
    }
    let structured_prompt_server = params.structured_prompt_server.clone().or_else(|| {
        notice_context.and_then(|(service, tab_id)| {
            crate::service::structured_prompt::ensure_for_spawn(
                service,
                agent_cmd,
                tab_id,
                params.cwd.as_deref().unwrap_or_default(),
            )
        })
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
                supported.iter().any(|level| level.id == effort).then(|| {
                    provider
                        .reasoning_effort_launch_args_for_model(resolved_model.as_deref(), effort)
                })
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

    let command_task = registry.with_provider(agent_cmd, |provider| {
        crate::service::launch_delivery::command_task(provider, task)
    });
    let skip_permissions = params.permission_bypass_requested(agent_cmd);
    args.extend(crate::service::unattended_launch::grant_args(
        registry,
        &crate::service::unattended_launch::UnattendedLaunch {
            provider_id: agent_cmd,
            model: resolved_model.as_deref(),
            role: params.role.as_deref(),
            role_profile: params.role_profile.as_deref(),
            has_task: task.is_some(),
            full_access: skip_permissions,
            ..Default::default()
        },
    ));
    crate::service::launch_session_detection::append_sandbox_linux_network_args(
        registry,
        agent_cmd,
        skip_permissions,
        crate::agent::provider::current_platform_tag(),
        &mut args,
    );
    let canonical_project = notice_context.and_then(|(service, tab_id)| {
        params.cwd.as_deref().and_then(|cwd| {
            crate::service::launch_session_detection::canonical_project_for_launch(
                service, tab_id, params, cwd,
            )
        })
    });
    crate::service::launch_session_detection::append_sandbox_git_metadata_args(
        registry,
        agent_cmd,
        skip_permissions,
        params.cwd.as_deref(),
        canonical_project.as_deref(),
        notice_context
            .map(|(_, tab_id)| tab_id)
            .unwrap_or("unknown"),
        &mut args,
    );
    if let Some((service, tab_id)) = notice_context {
        crate::service::launch_session_detection::append_sandbox_agent_pad_arg(
            registry,
            agent_cmd,
            skip_permissions,
            params.cwd.as_deref(),
            &service.agent_pad_data_dir,
            tab_id,
            &mut args,
        );
    }

    if let Some((service, tab_id)) = notice_context {
        if let Some(tab) = service.mux.write().get_tab_mut(tab_id) {
            tab.restorable_launch.context_files = params.context_files.clone();
            tab.restorable_launch.context_skills = params.context_skills.clone();
        }
    }

    if params.resume_session_id.is_some() && params.fork_session {
        let Some(fork_args) = registry
            .with_provider(agent_cmd, |provider| provider.fork_session_args())
            .flatten()
        else {
            return String::new();
        };
        args.extend(fork_args);
    }
    if let Some(session_id) = params
        .resume_session_id
        .as_ref()
        .filter(|_| structured_prompt_server.is_none())
    {
        let resume = crate::agent::provider::AgentResumeParams {
            cwd: params.cwd.clone(),
            context_files: params.context_files.clone(),
            tool_less_instructions_path: None,
            session_id: session_id.clone(),
            skip_permissions,
            agent_name: params.agent_name.clone(),
            system_prompt: command_task
                .as_ref()
                .and_then(|task| task.system_prompt.clone()),
            instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
            args: args.clone(),
        };
        let command = if let Some((service, tab_id)) = notice_context {
            crate::service::task_files::capture(service, tab_id, || {
                registry
                    .with_provider(agent_cmd, |provider| {
                        crate::service::context_bundle::build_resume(
                            provider,
                            &resume,
                            params.context_skills.as_deref(),
                        )
                    })
                    .flatten()
            })
        } else {
            registry
                .with_provider(agent_cmd, |provider| {
                    crate::service::context_bundle::build_resume(
                        provider,
                        &resume,
                        params.context_skills.as_deref(),
                    )
                })
                .flatten()
        };
        return command.unwrap_or_default();
    }

    let launch_task = command_task.and_then(|task| task.text);
    let launch_params = AgentLaunchParams {
        context_files: params.context_files.clone(),
        tool_less_instructions_path: None,
        agent_name: params.agent_name.clone(),
        task: launch_task,
        starter_prompt: params.starter_prompt,
        args: if args.is_empty() {
            None
        } else {
            Some(args.clone())
        },
        skip_permissions,
        session_id: params
            .resume_session_id
            .clone()
            .or_else(|| injected_session_id.map(str::to_string)),
        structured_prompt_server,
        launch_marker: launch_marker.map(str::to_string),
        instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
    };
    let command = if let Some((service, tab_id)) = notice_context {
        crate::service::task_files::capture(service, tab_id, || {
            registry.with_provider(agent_cmd, |provider| {
                crate::service::context_bundle::build(
                    provider,
                    &launch_params,
                    params.context_skills.as_deref(),
                )
            })
        })
    } else {
        registry.with_provider(agent_cmd, |provider| {
            crate::service::context_bundle::build(
                provider,
                &launch_params,
                params.context_skills.as_deref(),
            )
        })
    };
    match command {
        Some(command) if command.trim().is_empty() => String::new(),
        Some(command) => crate::agent::stub_launch::apply(agent_cmd, command),
        None => {
            let mut parts = vec![agent_cmd.to_string()];
            parts.extend(args);
            parts.join(" ")
        }
    }
}

#[cfg(test)]
mod tests;
