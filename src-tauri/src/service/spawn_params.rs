use super::orchestrator::SpawnAgentParams;
use super::spawn::{AgentSpawnSpec, PaneCommand, PaneVisibility, SpawnPaneSpec};
use super::ServiceContext;

impl SpawnPaneSpec {
    pub fn from_agent_params(params: SpawnAgentParams) -> Self {
        Self::from_agent_params_command(params, None)
    }

    pub fn from_agent_params_with_chatbackends(
        chatbackends: &crate::chatbackend::ChatBackendRegistry,
        params: SpawnAgentParams,
    ) -> Self {
        let provider = params.agent.clone().unwrap_or_default();
        // Route to the ChatBackend pane path only for a USER-SPAWNABLE shell.
        // A bare `chatBackend: true` backend (e.g. the NPC plugin) is registered
        // but not user-openable, so it must not be spawned as a pane here.
        let spawnable = !provider.is_empty()
            && chatbackends
                .get(&provider)
                .map(|b| b.spawnable())
                .unwrap_or(false);
        if spawnable {
            let config = chatbackend_spawn_config(
                params.system_prompt.as_deref(),
                params.preset.as_deref(),
                params.model.as_deref(),
            );
            return Self::from_agent_params_command(
                params,
                Some(PaneCommand::ChatBackend {
                    plugin_id: provider,
                    config,
                }),
            );
        }
        Self::from_agent_params_command(params, None)
    }

    fn from_agent_params_command(params: SpawnAgentParams, command: Option<PaneCommand>) -> Self {
        let agent_command = command.unwrap_or_else(|| {
            PaneCommand::Agent(AgentSpawnSpec {
                provider: params.agent.clone().unwrap_or_default(),
                model: params.model.clone(),
                task: params.task.clone(),
                agent_name: params.agent_name.clone(),
                extra_args: params.args.clone().unwrap_or_default(),
                role: params.role.clone(),
                can_spawn: params.can_spawn,
                auto_approve: params.skip_permissions.unwrap_or(true),
                report_target_pane_id: params.report_target_pane_id.clone(),
                worker_id: None,
                extra_env: Default::default(),
                task_preamble: None,
                spawned_by: None,
                spawned_by_pane_id: params.spawned_by_pane_id.clone(),
                resume_session_id: params.resume_session_id.clone(),
                activate: params.activate,
            })
        });
        Self {
            workspace_id: params.workspace_id.clone(),
            parent_pane_id: params.parent_pane_id.clone(),
            cwd: params.cwd.clone(),
            title: params.title.clone(),
            description: params.description.clone(),
            visibility: PaneVisibility::Visible,
            eager: false,
            command: agent_command,
        }
    }

    pub(crate) fn to_agent_params(&self) -> Result<SpawnAgentParams, String> {
        let agent = match &self.command {
            PaneCommand::Agent(a) => a,
            PaneCommand::Shell => {
                return Err("to_agent_params called on Shell spec".into());
            }
            PaneCommand::ChatBackend { .. } => {
                return Err("to_agent_params called on ChatBackend spec".into());
            }
        };
        let args = agent.extra_args.clone();
        Ok(SpawnAgentParams {
            agent: Some(agent.provider.clone()),
            agent_name: agent.agent_name.clone(),
            args: if args.is_empty() { None } else { Some(args) },
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            role: agent.role.clone(),
            parent_pane_id: self.parent_pane_id.clone(),
            can_spawn: agent.can_spawn,
            task: agent.task.clone(),
            workspace_id: self.workspace_id.clone(),
            force_tab: None,
            report_target_pane_id: agent.report_target_pane_id.clone(),
            skip_permissions: Some(agent.auto_approve),
            spawned_by_pane_id: agent.spawned_by_pane_id.clone(),
            model: agent.model.clone(),
            system_prompt: None,
            preset: None,
            resume_session_id: agent.resume_session_id.clone(),
            activate: agent.activate,
            structured_prompt_server: None,
        })
    }
}

pub(crate) fn chatbackend_spawn_config(
    system_prompt: Option<&str>,
    preset: Option<&str>,
    model: Option<&str>,
) -> serde_json::Value {
    let mut config = serde_json::Map::new();
    if let Some(value) = system_prompt.filter(|v| !v.trim().is_empty()) {
        config.insert("systemPrompt".into(), serde_json::json!(value));
    }
    if let Some(value) = preset.filter(|v| !v.trim().is_empty()) {
        config.insert("preset".into(), serde_json::json!(value));
    }
    if let Some(value) = model.filter(|v| !v.trim().is_empty()) {
        config.insert("model".into(), serde_json::json!(value));
    }
    serde_json::Value::Object(config)
}

impl ServiceContext {
    pub(crate) fn require_agent_provider_ready(&self, provider_id: &str) -> Result<(), String> {
        match self.agent_registry.wait_for_provider_startup(provider_id) {
            crate::agent::registry::ProviderStartupStatus::Ready => Ok(()),
            crate::agent::registry::ProviderStartupStatus::LoadingTimedOut => Err(format!(
                "Agent provider '{}' is still loading; retry shortly",
                provider_id
            )),
            crate::agent::registry::ProviderStartupStatus::Unknown => {
                let available = self.available_agent_or_shell_ids().join(", ");
                Err(format!(
                    "Unknown agent provider: '{}'. Available: [{}]",
                    provider_id, available
                ))
            }
        }
    }

    pub(crate) fn available_agent_or_shell_ids(&self) -> Vec<String> {
        self.provider_registry.spawnable_ids()
    }

    /// Validate a user-requested spawn target so the sync (socket-RPC) and async
    /// (HTTP) spawn entry points agree. Returns `Ok(true)` when the id routes to
    /// the ChatBackend pane path (a spawnable shell), `Ok(false)` for an ordinary
    /// agent provider, and a clean `Err` for a registered-but-not-spawnable
    /// chatBackend (e.g. the NPC backend) or an unknown id.
    pub(crate) fn classify_spawn_target(&self, agent_cmd: &str) -> Result<bool, String> {
        match self.provider_registry.transport_of(agent_cmd) {
            Some(crate::agent::provider::ProviderTransport::ChatBackend) => {
                if self.provider_registry.is_spawnable_chat_backend(agent_cmd) {
                    Ok(true)
                } else {
                    Err(format!(
                        "'{}' is a chat backend that is not user-spawnable",
                        agent_cmd
                    ))
                }
            }
            Some(crate::agent::provider::ProviderTransport::Pty) => {
                self.require_agent_provider_ready(agent_cmd)?;
                Ok(false)
            }
            None => {
                self.require_agent_provider_ready(agent_cmd)?;
                Ok(false)
            }
        }
    }
}
