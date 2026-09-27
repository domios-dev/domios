pub(super) struct RestoredFreshLaunchInput {
    pub params: crate::service::SpawnAgentParams,
    pub launch_parameters: crate::multiplexer::types::RestorableLaunchParameters,
    pub launch_task: Option<String>,
}

pub(super) fn restored_fresh_launch_input(
    registry: &crate::agent::registry::AgentRegistry,
    mut params: crate::service::SpawnAgentParams,
    mut launch_parameters: crate::multiplexer::types::RestorableLaunchParameters,
) -> RestoredFreshLaunchInput {
    let legacy_snapshot = !launch_parameters.captured;
    params.task = None;
    params.starter_prompt = crate::agent::provider::StarterPrompt::NoStarter;
    params.agent_name = launch_parameters.agent_name.clone();
    params.args = launch_parameters.args.clone();
    params.model = launch_parameters.model.clone().or(params.model);
    params.reasoning_effort = launch_parameters
        .resolved_reasoning_effort
        .clone()
        .or_else(|| launch_parameters.requested_reasoning_effort.clone());
    let provider_id = params.agent.as_deref().expect("restored launch provider");
    let (prompt_capable, launch_task) = crate::service::restored_launch::restored_launch_task(
        registry,
        provider_id,
        params.role.as_deref(),
        params.role_profile.as_deref(),
        &launch_parameters,
    );
    params.system_prompt = prompt_capable.then(|| launch_task.clone()).flatten();
    launch_parameters.captured = true;
    launch_parameters.identity_instructions = prompt_capable.then(|| launch_task.clone()).flatten();
    launch_parameters.model = params.model.clone();
    if launch_parameters.requested_model.is_none() {
        launch_parameters.requested_model = params.model.clone();
    }
    if launch_parameters.requested_reasoning_effort.is_none() {
        launch_parameters.requested_reasoning_effort = params.reasoning_effort.clone();
    }
    launch_parameters.resolved_reasoning_effort = params.reasoning_effort.clone();
    if legacy_snapshot {
        launch_parameters.permission_bypass_requested =
            params.permission_bypass_requested(provider_id);
    }
    RestoredFreshLaunchInput {
        params,
        launch_parameters,
        launch_task,
    }
}

impl RestoredFreshLaunchInput {
    /// `None` falls back to fresh launch when the provider cannot resume.
    /// Fresh launches carry a marker; resumes use the existing session id.
    pub(super) fn command(
        &self,
        service: &crate::service::ServiceContext,
        tab_id: &str,
    ) -> Option<String> {
        let provider_id = self.params.agent.as_deref()?;
        let registry = &service.agent_registry;
        let resumable = registry.with_provider(provider_id, |provider| {
            match self.params.resume_session_id.as_deref() {
                Some(session_id) => provider
                    .build_resume_command(
                        session_id,
                        self.params.permission_bypass_requested(provider_id),
                    )
                    .is_some(),
                None => true,
            }
        })?;
        if !resumable {
            return None;
        }
        Some(match self.params.resume_session_id {
            Some(_) => crate::service::build_agent_command_for_service(
                service,
                tab_id,
                provider_id,
                &self.params,
                self.launch_task.as_deref(),
                None,
                registry,
            ),
            None => crate::service::launch_session_detection::marked_agent_command_for_service(
                service,
                tab_id,
                provider_id,
                &self.params,
                self.launch_task.as_deref(),
                None,
                registry,
            ),
        })
    }
}
