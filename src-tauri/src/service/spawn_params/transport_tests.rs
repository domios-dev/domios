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
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn spawn_rejects_transport_mismatched_flags() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new(PROVIDER).with_launch_binary("cmd.exe"),
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
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
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
