use super::*;

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn missing_provider_command_is_refused_before_task_tab_creation() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let initial_tabs = mux.snapshot().tabs.len();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_empty_launch_command(
                "empty-launch",
            ),
        ));
    let mut spec = agent_spec("empty-launch");
    if let TabCommand::Agent(agent) = &mut spec.command {
        agent.role = Some("Worker".into());
        agent.task = Some("assigned work".into());
        agent.extra_env.insert("CODETERM_PLAN_ID".into(), "plan".into());
        agent.extra_env.insert("CODETERM_TASK_ID".into(), "task".into());
        agent.extra_env.insert("CODETERM_ATTEMPT_ID".into(), "attempt".into());
    }

    let error = match fixture.service.spawn_tab(spec) {
        Ok(_) => panic!("a task-bound spawn without a provider command must fail"),
        Err(error) => error,
    };

    assert!(error.contains("did not resolve a launch command"), "{error}");
    assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), initial_tabs);
}
