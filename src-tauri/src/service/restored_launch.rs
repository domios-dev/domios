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
