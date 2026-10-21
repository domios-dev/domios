use super::*;

pub(super) fn build_relaunch_command(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    provider: &str,
    session_present: bool,
    launch_task: Option<&str>,
) -> Option<crate::service::orchestrator::PreparedAgentCommand> {
    crate::service::directory_pre_trust::for_tab(
        svc,
        &svc.agent_registry,
        provider,
        &snap.tab_id,
        snap.cwd.as_deref(),
    );
    let auto_approve = match crate::service::permission_policy::configured_permission_bypass_for(
        &svc,
        &svc.agent_registry,
        provider,
    ) {
        Ok(authorization) => authorization.is_some(),
        Err(error) => {
            log::error!(
                "[agent-restart] refusing policy downgrade for provider={provider}: {error}"
            );
            return None;
        }
    };
    let session_id = snap.session_id.as_deref();
    let cwd = snap.cwd.as_deref().unwrap_or_default();
    let canonical_project = snap.cwd.as_deref().and_then(|cwd| {
        crate::service::launch_session_detection::canonical_project_for_tab(
            &svc.plan_store,
            &snap.tab_id,
            cwd,
        )
    });
    let structured_prompt_server = svc
        .agent_registry
        .with_provider(provider, |p| p.needs_structured_prompt_server())
        .unwrap_or(false)
        .then(|| {
            crate::service::structured_prompt::ensure_for_spawn(svc, provider, &snap.tab_id, cwd)
        })
        .flatten();
    crate::service::task_files::capture(svc, &snap.tab_id, || {
        build_relaunch_prepared_with_registry(
            &svc.agent_registry,
            provider,
            session_id,
            auto_approve,
            session_present,
            structured_prompt_server,
            launch_task,
            &snap.tab_id,
            snap.cwd.as_deref(),
            canonical_project.as_deref(),
        )
    })
}

#[cfg(test)]
pub(super) fn build_relaunch_command_with_registry(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    session_id: Option<&str>,
    auto_approve: bool,
    session_present: bool,
    structured_prompt_server: Option<StructuredPromptServer>,
    launch_task: Option<&str>,
    tab_id: &str,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
) -> Option<String> {
    build_relaunch_prepared_with_registry(
        registry,
        provider,
        session_id,
        auto_approve,
        session_present,
        structured_prompt_server,
        launch_task,
        tab_id,
        cwd,
        canonical_project,
    )
    .map(|command| command.shell_command)
}

fn build_relaunch_prepared_with_registry(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    session_id: Option<&str>,
    auto_approve: bool,
    session_present: bool,
    structured_prompt_server: Option<StructuredPromptServer>,
    launch_task: Option<&str>,
    tab_id: &str,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
) -> Option<crate::service::orchestrator::PreparedAgentCommand> {
    let starter_prompt = crate::agent::provider::StarterPrompt::NoStarter;
    if session_present && structured_prompt_server.is_some() {
        if let Some(sid) = session_id {
            let mut args = Vec::new();
            crate::service::launch_session_detection::append_sandbox_git_metadata_args(
                registry,
                provider,
                auto_approve,
                cwd,
                canonical_project,
                tab_id,
                &mut args,
            );
            crate::service::launch_session_detection::append_sandbox_linux_network_args(
                registry,
                provider,
                auto_approve,
                crate::agent::provider::current_platform_tag(),
                &mut args,
            );
            crate::service::launch_session_detection::append_sandbox_agent_pad_arg(
                registry,
                provider,
                auto_approve,
                cwd,
                &crate::paths::data_dir(),
                tab_id,
                &mut args,
            );
            return registry.with_provider(provider, |p| {
                crate::service::tool_less_instructions::build_prepared(
                    p,
                    &AgentLaunchParams {
                        context_files: None,
                        tool_less_instructions_path: None,
                        agent_name: None,
                        task: None,
                        starter_prompt,
                        args: (!args.is_empty()).then_some(args),
                        skip_permissions: auto_approve,
                        host_owned: crate::agent::DOMIOS_HOST_OWNED_CAPABILITIES,
                        session_id: Some(sid.to_string()),
                        structured_prompt_server: structured_prompt_server.clone(),
                        launch_marker: None,
                        instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
                    },
                    true,
                )
            });
        }
    }
    if session_present {
        if let Some(sid) = session_id {
            let system_prompt = registry
                .with_provider(provider, |p| {
                    crate::service::launch_delivery::content_for_descriptor(
                        &p.system_prompt_delivery(),
                        launch_task,
                    )
                    .system_prompt
                })
                .flatten();
            let mut args = Vec::new();
            crate::service::launch_session_detection::append_sandbox_git_metadata_args(
                registry,
                provider,
                auto_approve,
                cwd,
                canonical_project,
                tab_id,
                &mut args,
            );
            crate::service::launch_session_detection::append_sandbox_linux_network_args(
                registry,
                provider,
                auto_approve,
                crate::agent::provider::current_platform_tag(),
                &mut args,
            );
            crate::service::launch_session_detection::append_sandbox_agent_pad_arg(
                registry,
                provider,
                auto_approve,
                cwd,
                &crate::paths::data_dir(),
                tab_id,
                &mut args,
            );
            let resume = crate::agent::provider::AgentResumeParams {
                cwd: cwd.map(str::to_string),
                context_files: None,
                tool_less_instructions_path: None,
                session_id: sid.to_string(),
                skip_permissions: auto_approve,
                host_owned: crate::agent::DOMIOS_HOST_OWNED_CAPABILITIES,
                agent_name: None,
                system_prompt,
                instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
                args,
            };
            if let Some(cmd) = registry
                .with_provider(provider, |p| {
                    crate::service::context_bundle::build_resume_prepared(p, &resume, None, true)
                })
                .flatten()
            {
                return Some(cmd);
            }
        }
    }
    let mut params = AgentLaunchParams {
        context_files: None,
        tool_less_instructions_path: None,
        agent_name: None,
        task: launch_task.map(str::to_string),
        starter_prompt,
        args: None,
        skip_permissions: auto_approve,
        host_owned: crate::agent::DOMIOS_HOST_OWNED_CAPABILITIES,
        session_id: None,
        structured_prompt_server,
        launch_marker: None,
        instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
    };
    if let Some(cwd) = cwd {
        let mut args = Vec::new();
        crate::service::launch_session_detection::append_sandbox_git_metadata_args(
            registry,
            provider,
            auto_approve,
            Some(cwd),
            canonical_project,
            tab_id,
            &mut args,
        );
        crate::service::launch_session_detection::append_sandbox_linux_network_args(
            registry,
            provider,
            auto_approve,
            crate::agent::provider::current_platform_tag(),
            &mut args,
        );
        crate::service::launch_session_detection::append_sandbox_agent_pad_arg(
            registry,
            provider,
            auto_approve,
            Some(cwd),
            &crate::paths::data_dir(),
            tab_id,
            &mut args,
        );
        params.args = (!args.is_empty()).then_some(args);
    }
    crate::service::launch_session_detection::mark_fresh_launch(tab_id, &mut params);
    registry.with_provider(provider, |p| {
        crate::service::tool_less_instructions::build_prepared(p, &params, true)
    })
}
