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
        if !provider.is_empty() && chatbackends.get(&provider).is_some() {
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
    pub(crate) fn available_agent_or_shell_ids(&self) -> Vec<String> {
        let mut ids = self.agent_registry.provider_ids();
        ids.extend(self.chatbackend_registry.ids());
        ids.sort();
        ids.dedup();
        ids
    }
}
