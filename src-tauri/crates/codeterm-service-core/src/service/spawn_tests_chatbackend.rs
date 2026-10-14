use super::*;
use crate::agent::registry::AgentRegistry;
use crate::chatbackend::{ChatBackendError, ChatBackendProvider};
use crate::provider::ProviderRegistry;
use crate::service::SpawnAgentParams;
use std::sync::Arc;

use super::tests::{registry_with, spawn_params, ModelProvider};

struct SpawnRouteShell {
    id: &'static str,
    watcher: bool,
}

impl ChatBackendProvider for SpawnRouteShell {
    fn id(&self) -> &str {
        self.id
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn watcher_capable(&self) -> bool {
        self.watcher
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
        Ok(crate::agent::chat_types::StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn spawn_agent_params_for_registered_chatbackend_routes_to_chatbackend_config() {
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register(
        "local-shell",
        Arc::new(SpawnRouteShell {
            id: "local-shell",
            watcher: false,
        }),
    );

    let spec = SpawnTabSpec::from_agent_params_with_chatbackends(
        &registry,
        SpawnAgentParams {
            agent: Some("local-shell".into()),
            system_prompt: Some("You are terse.".into()),
            preset: Some("ops".into()),
            model: Some("qwen".into()),
            ..spawn_params("local-shell")
        },
    );

    match spec.command {
        TabCommand::ChatBackend {
            plugin_id, config, ..
        } => {
            assert_eq!(plugin_id, "local-shell");
            assert_eq!(config["systemPrompt"], "You are terse.");
            assert_eq!(config["preset"], "ops");
            assert_eq!(config["model"], "qwen");
        }
        other => panic!("expected chatBackend command, got {other:?}"),
    }
}

#[test]
fn chatbackend_session_config_carries_composed_manager_bootstrap() {
    let content = crate::service::role_identity::ManagerBootstrapInputs {
        role: "Planner".into(),
        role_kind: "planner".into(),
        team_id: "team_alpha".into(),
        team_name: "Alpha".into(),
        plan_id: "pl_alpha".into(),
        project: "CodeTerm".into(),
        coordination: None,
    }
    .compose();
    let config = chatbackend_spawn_config(Some(&content), None, None, None, None, None);
    assert_eq!(config["systemPrompt"], content);
}

#[test]
fn watcher_spawn_params_route_to_machine_watcher_config_and_spec() {
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register(
        "local-shell",
        Arc::new(SpawnRouteShell {
            id: "local-shell",
            watcher: true,
        }),
    );

    let spec = SpawnTabSpec::from_agent_params_with_chatbackends(
        &registry,
        SpawnAgentParams {
            agent: Some("local-shell".into()),
            role: Some("Manager".into()),
            role_profile: Some("watcher".into()),
            charter: Some("Watch the group.".into()),
            tick_interval_secs: Some(5),
            nudge_cooldown_secs: Some(0),
            max_actions_per_tick: Some(99),
            ..spawn_params("local-shell")
        },
    );

    match spec.command {
        TabCommand::ChatBackend {
            plugin_id,
            config,
            role,
            role_profile,
            can_spawn,
            watcher,
        } => {
            assert_eq!(plugin_id, "local-shell");
            assert_eq!(role.as_deref(), Some("Manager"));
            assert_eq!(role_profile.as_deref(), Some("watcher"));
            assert_eq!(can_spawn, None);
            assert_eq!(config["mode"], "watcher");
            assert_eq!(config["engine"]["kind"], "machine");
            assert_eq!(config["engine"]["charter"], "Watch the group.");
            assert_eq!(config["engine"]["tickIntervalSecs"], 5);

            let watcher = watcher.expect("watcher profile must persist a WatcherSpec");
            assert_eq!(watcher.charter, "Watch the group.");
            assert_eq!(watcher.policy.tick_interval_secs, 30);
            assert_eq!(watcher.policy.nudge_cooldown_secs, 60);
            assert_eq!(watcher.policy.max_actions_per_tick, 5);
        }
        other => panic!("expected chatBackend command, got {other:?}"),
    }
}

#[test]
fn watcher_spawn_config_omits_engine_when_charter_is_empty() {
    let config =
        chatbackend_spawn_config(None, None, None, Some("watcher"), Some("   "), Some(120));

    assert_eq!(config["mode"], "watcher");
    assert!(config.get("engine").is_none());
}

#[test]
fn watcher_spawn_validation_rejects_pty_provider_via_params_entry() {
    let agents = Arc::new(registry_with(ModelProvider::new("pty-agent", &["model-a"])));
    let backends = Arc::new(crate::chatbackend::ChatBackendRegistry::new());
    let providers = ProviderRegistry::new(agents, backends.clone());
    let params = SpawnAgentParams {
        role_profile: Some("watcher".into()),
        charter: Some("Watch the group.".into()),
        ..spawn_params("pty-agent")
    };

    let err =
        validate_watcher_spawn_target(&providers, &backends, "pty-agent", &params).unwrap_err();
    assert!(
        err.contains("watcher-capable chatBackend provider") && err.contains("PTY provider"),
        "unexpected error: {err}"
    );
}

#[test]
fn watcher_spawn_without_charter_uses_plugin_default() {
    let agents = Arc::new(AgentRegistry::new());
    let backends = Arc::new(crate::chatbackend::ChatBackendRegistry::new());
    backends.register(
        "local-shell",
        Arc::new(SpawnRouteShell {
            id: "local-shell",
            watcher: true,
        }),
    );
    let providers = ProviderRegistry::new(agents, backends.clone());
    let params = SpawnAgentParams {
        role_profile: Some("watcher".into()),
        charter: Some("   ".into()),
        ..spawn_params("local-shell")
    };

    validate_watcher_spawn_target(&providers, &backends, "local-shell", &params)
        .expect("bare watcher spawn is valid: the plugin falls back to its shipped charter");
}

#[test]
fn watcher_spawn_validation_rejects_non_watcher_capable_chatbackend() {
    let agents = Arc::new(AgentRegistry::new());
    let backends = Arc::new(crate::chatbackend::ChatBackendRegistry::new());
    backends.register(
        "local-shell",
        Arc::new(SpawnRouteShell {
            id: "local-shell",
            watcher: false,
        }),
    );
    let providers = ProviderRegistry::new(agents, backends.clone());
    let params = SpawnAgentParams {
        role_profile: Some("watcher".into()),
        charter: Some("Watch the group.".into()),
        ..spawn_params("local-shell")
    };

    let err =
        validate_watcher_spawn_target(&providers, &backends, "local-shell", &params).unwrap_err();
    assert!(
        err.contains("watcher-capable chatBackend provider"),
        "unexpected error: {err}"
    );
}

struct BackendOnlyShell;

impl ChatBackendProvider for BackendOnlyShell {
    fn id(&self) -> &str {
        "npc"
    }

    // spawnable() defaults to false — a bare `chatBackend: true` backend.

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
        Ok(crate::agent::chat_types::StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn spawn_agent_params_for_non_spawnable_chatbackend_does_not_route_to_chatbackend() {
    let registry = crate::chatbackend::ChatBackendRegistry::new();
    registry.register("npc", Arc::new(BackendOnlyShell));

    let spec = SpawnTabSpec::from_agent_params_with_chatbackends(
        &registry,
        SpawnAgentParams {
            agent: Some("npc".into()),
            ..spawn_params("npc")
        },
    );

    match spec.command {
        TabCommand::Agent(agent) => assert_eq!(agent.provider, "npc"),
        other => panic!("non-spawnable backend must not route to ChatBackend, got {other:?}"),
    }
}

#[test]
fn spawn_agent_params_for_non_chatbackend_keeps_agent_path() {
    let registry = crate::chatbackend::ChatBackendRegistry::new();

    let spec = SpawnTabSpec::from_agent_params_with_chatbackends(
        &registry,
        SpawnAgentParams {
            agent: Some("pty-agent".into()),
            system_prompt: Some("ignored by PTY providers".into()),
            preset: Some("ignored".into()),
            ..spawn_params("pty-agent")
        },
    );

    match spec.command {
        TabCommand::Agent(agent) => {
            assert_eq!(agent.provider, "pty-agent");
            assert_eq!(agent.model, None);
        }
        other => panic!("expected PTY agent command, got {other:?}"),
    }
}
