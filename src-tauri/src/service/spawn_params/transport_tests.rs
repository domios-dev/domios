use super::chat_backend_only_fields;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::orchestrator::SpawnAgentParams;
use crate::service::test_fixture::ServiceFixture;

const PROVIDER: &str = "pty-transport-fixture";

fn pty_params() -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(PROVIDER.into()),
        ..Default::default()
    }
}

fn mismatched_cases() -> Vec<(&'static str, SpawnAgentParams)> {
    vec![
        (
            "--system",
            SpawnAgentParams {
                system_prompt: Some("x".into()),
                ..pty_params()
            },
        ),
        (
            "--preset",
            SpawnAgentParams {
                preset: Some("x".into()),
                ..pty_params()
            },
        ),
        (
            "--charter",
            SpawnAgentParams {
                charter: Some("x".into()),
                ..pty_params()
            },
        ),
        (
            "--interval",
            SpawnAgentParams {
                tick_interval_secs: Some(90),
                ..pty_params()
            },
        ),
        (
            "--nudge-cooldown",
            SpawnAgentParams {
                nudge_cooldown_secs: Some(600),
                ..pty_params()
            },
        ),
        (
            "--max-actions",
            SpawnAgentParams {
                max_actions_per_tick: Some(3),
                ..pty_params()
            },
        ),
    ]
}

#[test]
fn chat_backend_only_fields_names_exactly_the_set_flags() {
    assert!(chat_backend_only_fields(&pty_params()).is_empty());
    for (flag, params) in mismatched_cases() {
        assert_eq!(chat_backend_only_fields(&params), vec![flag]);
    }
}

#[test]
fn spawn_rejects_transport_mismatched_flags() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new(PROVIDER)
            .with_launch_binary(crate::agent::test_provider::test_shell_binary()),
    ));

    for (flag, params) in mismatched_cases() {
        let error = fixture
            .service
            .classify_spawn_target_for_params(PROVIDER, &params)
            .expect_err(flag);
        assert!(error.contains(flag), "{flag}: {error}");
    }
    assert_eq!(
        fixture
            .service
            .classify_spawn_target_for_params(PROVIDER, &pty_params()),
        Ok(false)
    );

    let before = fixture.service.mux.read().snapshot().tabs.len();
    let (_, params) = mismatched_cases().remove(1);
    assert!(fixture.service.spawn_agent_sync(params).is_err());
    assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
}

struct ShellFixture;

impl crate::chatbackend::ChatBackendProvider for ShellFixture {
    fn id(&self) -> &str {
        "shell-transport-fixture"
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn open_session(
        &self,
        _: serde_json::Value,
    ) -> Result<String, crate::chatbackend::ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError>
    {
        Ok(crate::agent::chat_types::StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn list_models(
        &self,
    ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn chat_backend_spawn_keeps_its_fields_and_refuses_pty_launch_args() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let shell = "shell-transport-fixture";
    fixture
        .service
        .chatbackend_registry
        .register(shell, std::sync::Arc::new(ShellFixture));
    let shell_params = |params: SpawnAgentParams| SpawnAgentParams {
        agent: Some(shell.into()),
        ..params
    };

    for (flag, params) in mismatched_cases().into_iter().take(2) {
        assert_eq!(
            fixture
                .service
                .classify_spawn_target_for_params(shell, &shell_params(params)),
            Ok(true),
            "{flag} belongs to a chatBackend shell"
        );
    }
    let error = fixture
        .service
        .classify_spawn_target_for_params(
            shell,
            &shell_params(SpawnAgentParams {
                args: Some(vec!["--launch-arg".into()]),
                ..Default::default()
            }),
        )
        .expect_err("PTY launch args have no chatBackend meaning");
    assert!(error.contains("--args"), "{error}");
}

#[test]
fn chatbackend_metadata_retains_prepared_worker_params_without_exposing_env() {
    let registry =
        crate::service::spawn_capability::test_backend::registry_with_spawnable("runtime-fixture");
    let mut params = SpawnAgentParams {
        agent: Some("runtime-fixture".into()),
        role: Some("Worker".into()),
        task: Some("opaque-task".into()),
        system_prompt: Some("opaque-system".into()),
        model: Some("opaque-model".into()),
        reasoning_effort: Some("opaque-effort".into()),
        ..Default::default()
    };
    params.task_launch_authority = Some(crate::service::TaskLaunchAuthority {
        plan_id: "plan-exact".into(),
        task_id: "task-exact".into(),
        attempt_id: "attempt-exact".into(),
        lane_base: Some("base-exact".into()),
    });
    let mut spec =
        crate::service::spawn::SpawnTabSpec::from_agent_params_with_chatbackends(&registry, params);
    spec.bind_worker_identity("runtime-fixture", "worker-exact")
        .unwrap();
    let metadata = spec.chatbackend_launch.as_ref().unwrap();
    assert_eq!(metadata.agent.worker_id.as_deref(), Some("worker-exact"));
    assert_eq!(metadata.agent.task.as_deref(), Some("opaque-task"));
    assert_eq!(
        metadata.agent.extra_env["CODETERM_ATTEMPT_ID"],
        "attempt-exact"
    );
    assert_eq!(
        metadata
            .params
            .task_launch_authority
            .as_ref()
            .unwrap()
            .attempt_id,
        "attempt-exact"
    );
    let crate::service::spawn::TabCommand::ChatBackend { config, .. } = &spec.command else {
        panic!("chat backend")
    };
    assert_eq!(config["systemPrompt"], "opaque-system");
    assert_eq!(config["effort"], "opaque-effort");
    let wire = serde_json::to_value(&spec).unwrap();
    assert!(wire.get("chatbackend_launch").is_none());
    assert!(!wire.to_string().contains("attempt-exact"));
}

#[test]
fn direct_runtime_routing_preserves_hidden_worker_env_and_task() {
    let mut spec = crate::service::spawn::SpawnTabSpec {
        visibility: crate::service::spawn::TabVisibility::Hidden,
        command: crate::service::spawn::TabCommand::Agent(crate::service::spawn::AgentSpawnSpec {
            provider: "runtime-fixture".into(),
            worker_id: Some("worker-exact".into()),
            task: Some("opaque-task".into()),
            extra_env: std::collections::HashMap::from([("PATH".into(), "opaque-path".into())]),
            ..Default::default()
        }),
        ..Default::default()
    };
    spec.chatbackend_launch = Some(Box::new(crate::service::spawn::ChatBackendLaunchMetadata {
        agent: spec.agent_launch_mut().unwrap().clone(),
        params: SpawnAgentParams {
            system_prompt: Some("opaque-system".into()),
            preset: Some("opaque-preset".into()),
            task: Some("opaque-task".into()),
            ..Default::default()
        },
        owner_linkage: None,
    }));
    spec.route_agent_to_chatbackend().unwrap();
    assert_eq!(
        spec.visibility,
        crate::service::spawn::TabVisibility::Hidden
    );
    let metadata = spec.chatbackend_launch.as_ref().unwrap();
    assert_eq!(metadata.agent.worker_id.as_deref(), Some("worker-exact"));
    assert_eq!(metadata.agent.extra_env["PATH"], "opaque-path");
    let crate::service::spawn::TabCommand::ChatBackend { config, .. } = &spec.command else {
        panic!("chat backend")
    };
    assert_eq!(config["systemPrompt"], "opaque-system");
    assert_eq!(config["preset"], "opaque-preset");
    assert_eq!(metadata.agent.task.as_deref(), Some("opaque-task"));
}
