use crate::agent::registry::AgentRegistry;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::RestorableLaunchParameters;
use crate::service::SpawnAgentParams;

pub(crate) fn direct_restore_capable(provider: &dyn crate::agent::provider::AgentProvider) -> bool {
    provider.declares_launch_spec()
}

pub fn restored_resume_session_id(
    resume_sessions_enabled: bool,
    session_id: Option<&str>,
) -> Option<&str> {
    session_id.filter(|_| resume_sessions_enabled)
}

pub fn restored_session_target(tab: &crate::multiplexer::types::Tab) -> Option<&str> {
    tab.agent_session_id.as_deref().or_else(|| {
        crate::multiplexer::types::latest_prior_session(&tab.prior_session_binding_nonces)
    })
}

pub fn restored_session_resumable(presence: Option<bool>) -> bool {
    presence != Some(false)
}

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
    let (team_id, neighbor_root) = {
        let mux = mux.read();
        let tab = mux.get_tab(tab_id);
        (
            tab.and_then(|tab| tab.team_id.clone()),
            tab.and_then(|tab| tab.neighbor_authority.as_ref())
                .is_some_and(|authority| authority.root_tab_id.is_none()),
        )
    };
    let identity_instructions = launch_prompt_capable(registry, capture.provider_id)
        .then(|| {
            if neighbor_root {
                return capture.task.map(str::to_owned);
            }
            crate::service::orchestrator::restorable_identity_task(
                capture.task,
                capture.role,
                capture.role_profile,
                team_id.as_deref(),
            )
        })
        .flatten();
    let mut mux = mux.write();
    let tab = mux.get_tab_mut(tab_id);
    let previous = tab.as_deref().map(|tab| &tab.restorable_launch);
    let launch = RestorableLaunchParameters {
        account_notices: previous
            .map(|launch| launch.account_notices.clone())
            .unwrap_or_default(),
        account_namespace: previous.and_then(|launch| launch.account_namespace.clone()),
        account_id: previous.and_then(|launch| launch.account_id.clone()),
        host_approval_choice: previous.and_then(|launch| launch.host_approval_choice),
        selected_session_mode: previous.and_then(|launch| launch.selected_session_mode.clone()),
        execution_sandbox: previous.and_then(|launch| launch.execution_sandbox.clone()),
        exited_chat_binding: previous.and_then(|launch| launch.exited_chat_binding.clone()),
        captured: true,
        requested_model: capture.requested_model,
        model: capture.model,
        requested_reasoning_effort: capture.reasoning_effort.clone(),
        resolved_reasoning_effort: capture.reasoning_effort,
        args: capture.args,
        agent_name: capture.agent_name,
        identity_instructions,
        permission_bypass_requested: capture.permission_bypass_requested,
        runtime_launch_options: tab.as_deref().and_then(|tab| {
            captured_runtime_launch_options(
                capture.provider_id,
                tab.session_binding_nonce.as_deref(),
                tab.restorable_launch.runtime_launch_options.as_ref(),
            )
        }),
        ..Default::default()
    };
    if let Some(tab) = tab {
        tab.restorable_launch = launch.clone();
    }
    launch
}

fn captured_runtime_launch_options(
    provider: &str,
    nonce: Option<&str>,
    options: Option<&crate::multiplexer::types::RestorableRuntimeLaunchOptions>,
) -> Option<crate::multiplexer::types::RestorableRuntimeLaunchOptions> {
    options
        .filter(|options| {
            options.provider_id == provider && nonce == Some(options.binding_nonce.as_str())
        })
        .cloned()
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

pub fn restored_launch_task(
    registry: &AgentRegistry,
    provider_id: &str,
    role: Option<&str>,
    role_profile: Option<&str>,
    team_id: Option<&str>,
    launch: &RestorableLaunchParameters,
) -> (bool, Option<String>) {
    let prompt_capable = launch_prompt_capable(registry, provider_id);
    let task = if prompt_capable {
        launch.identity_instructions.clone().or_else(|| {
            crate::service::orchestrator::restorable_identity_task(
                None,
                role,
                role_profile,
                team_id,
            )
        })
    } else {
        crate::service::orchestrator::provider_launch_task(
            registry,
            provider_id,
            None,
            role,
            role_profile,
            team_id,
        )
    };
    let task = if launch
        .execution_sandbox
        .as_ref()
        .is_some_and(|policy| policy.intent == crate::plan::ExecutionIntent::ReadOnly)
    {
        task.map(crate::prompts::with_read_only_worker_guidance)
    } else {
        task
    };
    (prompt_capable, task)
}

pub fn restore_permission_bypass_authorization(
    svc: &super::ServiceContext,
    registry: &AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    launch: &RestorableLaunchParameters,
) -> Result<Option<crate::service::permission_policy::PermissionBypassAuthorization>, String> {
    svc.neighbors()
        .require_neighbor_restore_grant(tab_id, launch.permission_bypass_requested)?;
    if launch.captured && !launch.permission_bypass_requested {
        return Ok(None);
    }
    if launch.captured
        && launch.permission_bypass_requested
        && launch.execution_sandbox.is_none()
        && svc
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.neighbor_authority.is_some())
    {
        return crate::service::permission_policy::authorize_permission_bypass(
            registry,
            provider_id,
            true,
        )
        .map(Some)
        .ok_or_else(|| codeterm_neighbors::Error::MachineAccessUnsupported.to_string());
    }
    let worker = svc
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.role == Some(crate::multiplexer::types::AgentRole::Worker));
    let authorization = crate::service::permission_policy::spawn_permission_bypass_for(
        svc,
        registry,
        provider_id,
        worker,
        launch.execution_sandbox.is_some(),
    )?;
    if launch.captured && launch.permission_bypass_requested && authorization.is_none() {
        log::warn!(
            "[restore] Tab {tab_id} permission bypass denied by current policy; resuming without it"
        );
    }
    Ok(authorization)
}

pub fn persist_restored_launch_identity(
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
    tab.restorable_launch.failed_provider = None;
}

pub struct RestoredFreshLaunchInput {
    pub params: crate::service::SpawnAgentParams,
    pub launch_parameters: crate::multiplexer::types::RestorableLaunchParameters,
    pub launch_task: Option<String>,
}

pub fn restored_fresh_launch_input(
    registry: &crate::agent::registry::AgentRegistry,
    mut params: crate::service::SpawnAgentParams,
    mut launch_parameters: crate::multiplexer::types::RestorableLaunchParameters,
) -> RestoredFreshLaunchInput {
    let legacy_snapshot = !launch_parameters.captured;
    if let Some(policy) = &launch_parameters.execution_sandbox {
        params.sandbox =
            Some(crate::service::execution_sandbox::requested_label(&policy.intent).into());
        params.cwd = Some(policy.cwd.clone());
        params.skip_permissions = Some(false);
        params.permission_bypass_authorization = None;
    }
    if launch_parameters.captured {
        params.skip_permissions = Some(
            launch_parameters.permission_bypass_requested
                && params.permission_bypass_authorization.is_some(),
        );
    }
    params.account_id = Some(
        launch_parameters
            .account_id
            .clone()
            .unwrap_or_else(|| "default".into()),
    );
    params.context_files = launch_parameters.context_files.clone();
    params.account_namespace = launch_parameters.account_namespace.clone();
    params.context_skills = launch_parameters.context_skills.clone();
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
        params.team_id.as_deref(),
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
    pub fn command(
        &self,
        service: &crate::service::ServiceContext,
        tab_id: &str,
    ) -> Option<String> {
        let invocation = crate::service::provider_accounts::invocation(
            service,
            self.params.agent.as_deref()?,
            self.params.account_id.as_deref(),
        )
        .ok()?;
        codeterm_plugin::provider_account::with_context(invocation, || {
            self.command_scoped(service, tab_id)
        })
    }
    fn command_scoped(
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
    pub fn prepared_command(
        &self,
        service: &crate::service::ServiceContext,
        tab_id: &str,
    ) -> Option<crate::service::orchestrator::PreparedAgentCommand> {
        let invocation = crate::service::provider_accounts::invocation(
            service,
            self.params.agent.as_deref()?,
            self.params.account_id.as_deref(),
        )
        .ok()?;
        codeterm_plugin::provider_account::with_context(invocation, || {
            self.prepared_command_scoped(service, tab_id)
        })
    }
    pub fn prepared_command_with_invocation(
        &self,
        service: &crate::service::ServiceContext,
        tab_id: &str,
        invocation: Option<std::sync::Arc<codeterm_plugin::provider_account::InvocationContext>>,
    ) -> Option<crate::service::orchestrator::PreparedAgentCommand> {
        codeterm_plugin::provider_account::with_context(invocation, || {
            self.prepared_command_scoped(service, tab_id)
        })
    }

    fn prepared_command_scoped(
        &self,
        service: &crate::service::ServiceContext,
        tab_id: &str,
    ) -> Option<crate::service::orchestrator::PreparedAgentCommand> {
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
            Some(_) => crate::service::orchestrator::prepare_agent_command_for_service(
                service,
                tab_id,
                provider_id,
                &self.params,
                self.launch_task.as_deref(),
                None,
                registry,
                None,
            ),
            None => {
                crate::service::launch_session_detection::marked_prepared_agent_command_for_service(
                    service,
                    tab_id,
                    provider_id,
                    &self.params,
                    self.launch_task.as_deref(),
                    None,
                    registry,
                )
            }
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "restored_launch/neighbor_tests.rs"]
mod neighbor_tests;
