use super::orchestrator::SpawnAgentParams;
use super::spawn::{AgentSpawnSpec, SpawnTabSpec, TabCommand, TabVisibility};
use super::ServiceContext;
use crate::multiplexer::types::{WatcherPolicy, WatcherSpec};

pub(crate) fn starter_prompt_for_role(role: Option<&str>) -> crate::agent::provider::StarterPrompt {
    if role.is_none() {
        crate::agent::provider::StarterPrompt::NoStarter
    } else {
        crate::agent::provider::StarterPrompt::AssignedTask
    }
}

pub(crate) fn starter_prompt_for_creation(
    role: Option<&str>,
    task: Option<&str>,
) -> crate::agent::provider::StarterPrompt {
    if role.is_some() && task.is_some_and(|task| !task.trim().is_empty()) {
        crate::agent::provider::StarterPrompt::AssignedTask
    } else {
        crate::agent::provider::StarterPrompt::NoStarter
    }
}

impl SpawnTabSpec {
    pub fn from_agent_params(params: SpawnAgentParams) -> Self {
        Self::from_agent_params_command(params, None)
    }

    pub fn from_agent_params_with_chatbackends(
        chatbackends: &crate::chatbackend::ChatBackendRegistry,
        params: SpawnAgentParams,
    ) -> Self {
        let provider = params.agent.clone().unwrap_or_default();
        // Route to the ChatBackend tab path only for a USER-SPAWNABLE shell.
        let spawnable = !provider.is_empty()
            && chatbackends
                .get(&provider)
                .map(|b| b.spawnable())
                .unwrap_or(false);
        if spawnable {
            let watcher = watcher_spec_from_params(&params);
            let role = params.role.clone();
            let role_profile = params.role_profile.clone();
            let can_spawn = params.can_spawn;
            let config = chatbackend_spawn_config(
                params.system_prompt.as_deref(),
                params.preset.as_deref(),
                params.model.as_deref(),
                params.role_profile.as_deref(),
                params.charter.as_deref(),
                params.tick_interval_secs,
            );
            return Self::from_agent_params_command(
                params,
                Some(TabCommand::ChatBackend {
                    plugin_id: provider,
                    config,
                    role,
                    role_profile,
                    can_spawn,
                    watcher,
                }),
            );
        }
        Self::from_agent_params_command(params, None)
    }

    fn from_agent_params_command(params: SpawnAgentParams, command: Option<TabCommand>) -> Self {
        let task_launch_env = params.task_launch_env();
        let agent_command = command.unwrap_or_else(|| {
            TabCommand::Agent(AgentSpawnSpec {
                provider: params.agent.clone().unwrap_or_default(),
                planned_model: params.planned_model.clone(),
                model: params.model.clone(),
                reasoning_effort: params.reasoning_effort.clone(),
                task: params.task.clone(),
                agent_name: params.agent_name.clone(),
                extra_args: params.args.clone().unwrap_or_default(),
                provisioned_cwd: params.provisioned_cwd.clone(),
                role: params.role.clone(),
                role_profile: params.role_profile.clone(),
                can_spawn: params.can_spawn,
                auto_approve: params.skip_permissions.unwrap_or(true),
                report_target_tab_id: params.report_target_tab_id.clone(),
                worker_id: None,
                extra_env: task_launch_env,
                task_preamble: None,
                spawned_by: None,
                spawned_by_tab_id: params.spawned_by_tab_id.clone(),
                resume_session_id: params.resume_session_id.clone(),
                activate: params.activate,
            })
        });
        Self {
            workspace_id: params.workspace_id.clone(),
            parent_tab_id: params.parent_tab_id.clone(),
            team_id: None,
            creator_tab_id: None,
            cwd: params.cwd.clone(),
            title: params.title.clone(),
            description: params.description.clone(),
            visibility: TabVisibility::Visible,
            audience: params.audience,
            eager: false,
            command: agent_command,
        }
    }

    pub(crate) fn to_agent_params(&self) -> Result<SpawnAgentParams, String> {
        let agent = match &self.command {
            TabCommand::Agent(a) => a,
            TabCommand::Shell => {
                return Err("to_agent_params called on Shell spec".into());
            }
            TabCommand::ChatBackend { .. } => {
                return Err("to_agent_params called on ChatBackend spec".into());
            }
        };
        let args = agent.extra_args.clone();
        let starter_prompt =
            starter_prompt_for_creation(agent.role.as_deref(), agent.task.as_deref());
        Ok(SpawnAgentParams {
            agent: Some(agent.provider.clone()),
            agent_name: agent.agent_name.clone(),
            args: if args.is_empty() { None } else { Some(args) },
            cwd: self.cwd.clone(),
            provisioned_cwd: None,
            title: self.title.clone(),
            title_provenance: None,
            description: self.description.clone(),
            role: agent.role.clone(),
            role_profile: agent.role_profile.clone(),
            parent_tab_id: self.parent_tab_id.clone(),
            plan_id: None,
            can_spawn: agent.can_spawn,
            task: agent.task.clone(),
            starter_prompt,
            task_id: None,
            task_launch_authority: None,
            plan_task: None,
            execution_intent: None,
            provider_reserve_pct: None,
            workspace_id: self.workspace_id.clone(),
            force_tab: None,
            existing_tab_id: None,
            skip_orchestrator_onboarding: None,
            report_target_tab_id: agent.report_target_tab_id.clone(),
            // AgentSpawnSpec (sync/local spawn path) carries no remote target;
            // mesh workers spawn via the async HTTP path which sets it directly.
            report_target_mesh_peer: None,
            skip_permissions: Some(agent.auto_approve),
            spawned_by_tab_id: agent.spawned_by_tab_id.clone(),
            model: agent.model.clone(),
            planned_model: agent.planned_model.clone(),
            reasoning_effort: agent.reasoning_effort.clone(),
            system_prompt: None,
            manager_bootstrap: None,
            preset: None,
            resume_session_id: agent.resume_session_id.clone(),
            charter: None,
            tick_interval_secs: None,
            nudge_cooldown_secs: None,
            max_actions_per_tick: None,
            activate: agent.activate,
            audience: self.audience,
            structured_prompt_server: None,
        })
    }
}

pub(crate) fn chatbackend_spawn_config(
    system_prompt: Option<&str>,
    preset: Option<&str>,
    model: Option<&str>,
    role_profile: Option<&str>,
    charter: Option<&str>,
    tick_interval_secs: Option<u64>,
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
    if role_profile == Some("watcher") {
        let charter = charter.unwrap_or_default().trim();
        config.insert("mode".into(), serde_json::json!("watcher"));
        if !charter.is_empty() {
            let mut engine = serde_json::Map::new();
            engine.insert("kind".into(), serde_json::json!("machine"));
            engine.insert("charter".into(), serde_json::json!(charter));
            if let Some(value) = tick_interval_secs {
                engine.insert("tickIntervalSecs".into(), serde_json::json!(value));
            }
            config.insert("engine".into(), serde_json::Value::Object(engine));
        }
    }
    serde_json::Value::Object(config)
}

pub(crate) fn reject_direct_watcher_chatbackend_config(
    config: &serde_json::Value,
) -> Result<(), String> {
    if config.get("mode").and_then(|v| v.as_str()) == Some("watcher") {
        Err("watcher Tabs spawn via codeterm agent spawn --role Watcher".into())
    } else {
        Ok(())
    }
}

fn watcher_spec_from_params(params: &SpawnAgentParams) -> Option<WatcherSpec> {
    (params.role_profile.as_deref() == Some("watcher")).then(|| WatcherSpec {
        charter: params
            .charter
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string(),
        policy: WatcherPolicy::clamped(
            params.tick_interval_secs,
            params.nudge_cooldown_secs,
            params.max_actions_per_tick,
        ),
        ..Default::default()
    })
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
    /// the ChatBackend tab path (a spawnable shell), `Ok(false)` for an ordinary
    /// agent provider, and a clean `Err` for a registered-but-not-spawnable
    /// chatBackend (e.g. the NPC backend) or an unknown id.
    pub(crate) fn classify_spawn_target_for_params(
        &self,
        agent_cmd: &str,
        params: &SpawnAgentParams,
    ) -> Result<bool, String> {
        validate_watcher_spawn_target(
            &self.provider_registry,
            &self.chatbackend_registry,
            agent_cmd,
            params,
        )?;
        let watcher = params.role_profile.as_deref() == Some("watcher");
        match self.provider_registry.transport_of(agent_cmd) {
            Some(crate::agent::provider::ProviderTransport::ChatBackend) => {
                if !self.provider_registry.is_spawnable_chat_backend(agent_cmd) {
                    return Err(format!(
                        "'{}' is a chat backend that is not user-spawnable",
                        agent_cmd
                    ));
                }
                if watcher
                    && !self
                        .chatbackend_registry
                        .get(agent_cmd)
                        .map(|b| b.watcher_capable())
                        .unwrap_or(false)
                {
                    return Err(format!(
                        "watcher spawn requires a watcher-capable chatBackend provider; '{}' is not watcher-capable",
                        agent_cmd
                    ));
                }
                Ok(true)
            }
            Some(crate::agent::provider::ProviderTransport::Pty) => {
                if watcher {
                    return Err(format!(
                        "watcher spawn requires a watcher-capable chatBackend provider; '{}' is a PTY provider",
                        agent_cmd
                    ));
                }
                self.require_agent_provider_ready(agent_cmd)?;
                Ok(false)
            }
            None => {
                if watcher {
                    return Err(format!(
                        "watcher spawn requires a watcher-capable chatBackend provider; '{}' is not a chatBackend provider",
                        agent_cmd
                    ));
                }
                self.require_agent_provider_ready(agent_cmd)?;
                Ok(false)
            }
        }
    }
}

pub(crate) fn validate_watcher_spawn_target(
    providers: &crate::provider::ProviderRegistry,
    chatbackends: &crate::chatbackend::ChatBackendRegistry,
    agent_cmd: &str,
    params: &SpawnAgentParams,
) -> Result<(), String> {
    if params.role_profile.as_deref() != Some("watcher") {
        return Ok(());
    }
    // No charter is fine: watcher-capable plugins fall back to their shipped
    // default charter (the out-of-the-box orchestration health watcher).
    match providers.transport_of(agent_cmd) {
        Some(crate::agent::provider::ProviderTransport::ChatBackend) => {
            if chatbackends
                .get(agent_cmd)
                .map(|b| b.watcher_capable())
                .unwrap_or(false)
            {
                Ok(())
            } else {
                Err(format!(
                    "watcher spawn requires a watcher-capable chatBackend provider; '{}' is not watcher-capable",
                    agent_cmd
                ))
            }
        }
        Some(crate::agent::provider::ProviderTransport::Pty) => Err(format!(
            "watcher spawn requires a watcher-capable chatBackend provider; '{}' is a PTY provider",
            agent_cmd
        )),
        None => Err(format!(
            "watcher spawn requires a watcher-capable chatBackend provider; '{}' is not a chatBackend provider",
            agent_cmd
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_launch_has_no_manager_bootstrap() {
        let spec = SpawnTabSpec::from_agent_params(SpawnAgentParams {
            agent: Some("worker-provider".into()),
            role: Some("Worker".into()),
            task: Some("worker task fixture".into()),
            ..Default::default()
        });
        let params = spec.to_agent_params().unwrap();
        assert_eq!(params.role.as_deref(), Some("Worker"));
        assert!(params.manager_bootstrap.is_none());
        assert!(params.system_prompt.is_none());
        assert_eq!(
            crate::service::launch_delivery::fallback_delivery_spec(
                params
                    .manager_bootstrap
                    .as_ref()
                    .map(|value| value.compose())
                    .as_deref(),
                params.task.as_deref(),
                None,
            )
            .unwrap()
            .channel,
            crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput
        );
    }

    #[test]
    fn assigned_worker_task_authority_reaches_pty_spec() {
        let mut params = SpawnAgentParams {
            agent: Some("worker-provider".into()),
            role: Some("Worker".into()),
            plan_id: Some("assigned-plan".into()),
            task_id: Some("assigned-task".into()),
            plan_task: Some("assigned-task".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            provider_reserve_pct: None,
            ..Default::default()
        };
        params.task_launch_authority = Some(crate::service::TaskLaunchAuthority {
            plan_id: "assigned-plan".into(),
            task_id: "assigned-task".into(),
            lane_base: Some("assigned-base".into()),
        });
        let spec = SpawnTabSpec::from_agent_params(params);
        let TabCommand::Agent(agent) = spec.command else {
            panic!("worker must use the PTY agent path");
        };

        assert_eq!(
            agent.extra_env.get("CODETERM_PLAN_ID").map(String::as_str),
            Some("assigned-plan")
        );
        assert_eq!(
            agent.extra_env.get("CODETERM_TASK_ID").map(String::as_str),
            Some("assigned-task")
        );
        assert_eq!(
            agent
                .extra_env
                .get("CODETERM_LANE_BASE")
                .map(String::as_str),
            Some("assigned-base")
        );
    }

    #[test]
    fn roleless_spawn_does_not_promote_inherited_task_defaults() {
        let spec = SpawnTabSpec::from_agent_params(SpawnAgentParams {
            plan_id: Some("inherited-plan".into()),
            task_id: Some("inherited-task".into()),
            ..Default::default()
        });
        let TabCommand::Agent(agent) = spec.command else {
            panic!("probe must use the PTY agent path");
        };

        assert!(agent.extra_env.is_empty());
    }

    #[test]
    fn standalone_new_session_remains_no_starter() {
        let roleless = SpawnTabSpec::from_agent_params(SpawnAgentParams::default())
            .to_agent_params()
            .unwrap();
        assert_eq!(
            roleless.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );

        let rest = SpawnAgentParams::from_json(&serde_json::json!({"agent": "codex"})).unwrap();
        assert!(rest.role.is_none());
        assert!(rest.task.is_none());
        assert_eq!(
            rest.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );
    }

    #[test]
    fn fresh_team_creation_cardinality_survives_spawn_spec_roundtrip() {
        for (task, expected) in [
            (None, crate::agent::provider::StarterPrompt::NoStarter),
            (
                Some("assigned brief".to_string()),
                crate::agent::provider::StarterPrompt::AssignedTask,
            ),
        ] {
            let spec = SpawnTabSpec::from_agent_params(SpawnAgentParams {
                role: Some("Worker".into()),
                task: task.clone(),
                ..Default::default()
            });
            let json = serde_json::to_string(&spec).unwrap();
            let restored: SpawnTabSpec = serde_json::from_str(&json).unwrap();
            let params = restored.to_agent_params().unwrap();

            assert_eq!(params.role.as_deref(), Some("Worker"));
            assert_eq!(params.task, task);
            assert_eq!(params.starter_prompt, expected);
        }
    }

    #[test]
    fn worker_without_task_does_not_inherit_assigned_starter() {
        let worker = SpawnTabSpec::from_agent_params(SpawnAgentParams {
            role: Some("Worker".into()),
            ..Default::default()
        })
        .to_agent_params()
        .unwrap();

        assert_eq!(
            worker.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );
    }

    #[test]
    fn blank_task_never_becomes_assigned_starter() {
        assert_eq!(
            starter_prompt_for_creation(Some("Worker"), Some("   ")),
            crate::agent::provider::StarterPrompt::NoStarter
        );
    }
}
