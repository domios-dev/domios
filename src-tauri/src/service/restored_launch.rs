use crate::agent::registry::AgentRegistry;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::RestorableLaunchParameters;
use crate::service::SpawnAgentParams;

pub(crate) struct RestorableLaunchCapture<'a> {
    pub provider_id: &'a str,
    pub requested_model: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub args: Option<Vec<String>>,
    pub agent_name: Option<String>,
    pub task: Option<&'a str>,
    pub role: Option<&'a str>,
    pub role_profile: Option<&'a str>,
    pub permission_bypass_requested: bool,
}

pub(crate) fn capture_tab_launch_parameters(
    mux: &MuxState,
    tab_id: &str,
    registry: &AgentRegistry,
    capture: RestorableLaunchCapture<'_>,
) -> RestorableLaunchParameters {
    let identity_instructions = launch_prompt_capable(registry, capture.provider_id)
        .then(|| {
            crate::service::orchestrator::restorable_identity_task(
                capture.task,
                capture.role,
                capture.role_profile,
            )
        })
        .flatten();
    let launch = RestorableLaunchParameters {
        captured: true,
        requested_model: capture.requested_model,
        model: capture.model,
        requested_reasoning_effort: capture.reasoning_effort.clone(),
        resolved_reasoning_effort: capture.reasoning_effort,
        args: capture.args,
        agent_name: capture.agent_name,
        identity_instructions,
        permission_bypass_requested: capture.permission_bypass_requested,
    };
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.restorable_launch = launch.clone();
    }
    launch
}

pub(crate) fn launch_prompt_capable(registry: &AgentRegistry, provider_id: &str) -> bool {
    registry
        .with_provider(provider_id, |provider| {
            matches!(
                crate::service::launch_delivery::mode_for_descriptor(
                    &provider.system_prompt_delivery(),
                ),
                crate::service::launch_delivery::LaunchDeliveryMode::SystemPrompt
            )
        })
        .unwrap_or(false)
}

pub(crate) fn restored_launch_task(
    registry: &AgentRegistry,
    provider_id: &str,
    role: Option<&str>,
    role_profile: Option<&str>,
    launch: &RestorableLaunchParameters,
) -> (bool, Option<String>) {
    let prompt_capable = launch_prompt_capable(registry, provider_id);
    let task = if prompt_capable {
        launch.identity_instructions.clone().or_else(|| {
            crate::service::orchestrator::restorable_identity_task(None, role, role_profile)
        })
    } else {
        crate::service::orchestrator::provider_launch_task(
            registry,
            provider_id,
            None,
            role,
            role_profile,
        )
    };
    (prompt_capable, task)
}

pub(crate) fn restore_permission_bypass_authorization(
    registry: &AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    launch: &RestorableLaunchParameters,
) -> Result<Option<crate::service::permission_policy::PermissionBypassAuthorization>, String> {
    if launch.captured && !launch.permission_bypass_requested {
        return Ok(None);
    }
    let authorization =
        crate::service::permission_policy::configured_permission_bypass(registry, provider_id)?;
    if launch.captured && launch.permission_bypass_requested && authorization.is_none() {
        log::warn!(
            "[restore] Tab {tab_id} permission bypass denied by current policy; resuming without it"
        );
    }
    Ok(authorization)
}

pub(crate) fn persist_restored_launch_identity(
    mux: &MuxState,
    tab_id: &str,
    provider_id: &str,
    params: &SpawnAgentParams,
    launch: &RestorableLaunchParameters,
) {
    let mut mux = mux.write();
    let Some(tab) = mux.get_tab_mut(tab_id) else {
        return;
    };
    tab.agent_requested_model = launch.requested_model.clone();
    tab.agent_provider_resolved_model = params.model.clone();
    tab.agent_session_model = None;
    tab.agent_session_model_identity = None;
    tab.model_identity_mismatch = None;
    if let Some(fallback) = tab.model_fallback.as_mut() {
        fallback.actual_model = None;
    }
    let planned_effort = tab.reasoning_effort_identity.planned.clone();
    crate::service::reasoning_effort::persist_launch_identity(
        tab,
        planned_effort.as_deref(),
        params.reasoning_effort.as_deref(),
    );
    tab.auto_approve = params.permission_bypass_requested(provider_id);
    tab.restorable_launch = launch.clone();
    tab.restorable_launch.captured = true;
}

pub(crate) struct RestoredFreshLaunchInput {
    pub params: crate::service::SpawnAgentParams,
    pub launch_parameters: crate::multiplexer::types::RestorableLaunchParameters,
    pub launch_task: Option<String>,
}

pub(crate) fn restored_fresh_launch_input(
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
    let (prompt_capable, launch_task) = restored_launch_task(
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
    pub(crate) fn command(
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

#[cfg(test)]
mod tests {
    use super::{capture_tab_launch_parameters, RestorableLaunchCapture};

    #[test]
    fn capture_keeps_worker_identity_without_assignment() {
        const IDENTITY: &str = "opaque-worker-identity-02a1";
        const ASSIGNMENT: &str = "opaque-worker-assignment-f3b2";
        let mut state = crate::multiplexer::state::MultiplexerState::empty();
        let workspace_id = state.create_workspace("launch capture".into(), None).id;
        let tab_id = state.create_tab(&workspace_id).unwrap();
        let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
            "restorable-launch-capture",
            state,
        ));
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("capture-provider")
                .with_session_config_delivery(),
        ));
        let task =
            crate::service::plan::worker_identity::attach_worker_identity(IDENTITY, ASSIGNMENT);

        let launch = capture_tab_launch_parameters(
            &mux,
            &tab_id,
            &registry,
            RestorableLaunchCapture {
                provider_id: "capture-provider",
                requested_model: None,
                model: None,
                reasoning_effort: None,
                args: None,
                agent_name: None,
                task: Some(&task),
                role: Some("Worker"),
                role_profile: None,
                permission_bypass_requested: false,
            },
        );

        assert!(launch.captured);
        let identity = launch.identity_instructions.unwrap();
        assert!(identity.contains(IDENTITY));
        assert!(!identity.contains(ASSIGNMENT));
        assert_eq!(
            mux.read()
                .get_tab(&tab_id)
                .unwrap()
                .restorable_launch
                .identity_instructions
                .as_deref(),
            Some(identity.as_str())
        );
    }
}
