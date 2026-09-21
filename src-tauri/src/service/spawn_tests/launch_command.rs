use super::*;

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn missing_provider_command_is_refused_before_task_tab_creation() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let initial_tabs = mux.snapshot().tabs.len();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_empty_launch_command("empty-launch"),
    ));
    let mut spec = agent_spec("empty-launch");
    if let TabCommand::Agent(agent) = &mut spec.command {
        agent.role = Some("Worker".into());
        agent.task = Some("assigned work".into());
        agent
            .extra_env
            .insert("CODETERM_PLAN_ID".into(), "plan".into());
        agent
            .extra_env
            .insert("CODETERM_TASK_ID".into(), "task".into());
        agent
            .extra_env
            .insert("CODETERM_ATTEMPT_ID".into(), "attempt".into());
    }

    let error = match fixture.service.spawn_tab(spec) {
        Ok(_) => panic!("a task-bound spawn without a provider command must fail"),
        Err(error) => error,
    };

    assert!(
        error.contains("did not resolve a launch command"),
        "{error}"
    );
    assert_eq!(
        fixture.service.mux.read().snapshot().tabs.len(),
        initial_tabs
    );
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn task_specific_empty_provider_command_is_refused_after_real_build_and_rolls_back_tab() {
    let (mux, parent_tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let initial_tabs = mux.snapshot().tabs.len();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_empty_launch_command_when_task_present(
            "task-sensitive-empty-launch",
        )
        .with_launch_binary("cmd.exe"),
    ));
    fixture
        .service
        .require_agent_provider_ready("task-sensitive-empty-launch")
        .expect("provider preflight without a task succeeds");

    let mut spec = agent_spec("task-sensitive-empty-launch");
    if let TabCommand::Agent(agent) = &mut spec.command {
        agent.role = Some("Worker".into());
        agent.task = Some("assigned work".into());
    }
    spec.parent_tab_id = Some(parent_tab_id);

    let error = match fixture.service.spawn_tab(spec) {
        Ok(_) => panic!("a task-specific missing provider command must fail"),
        Err(error) => error,
    };

    assert!(
        error.contains("did not resolve a launch command"),
        "{error}"
    );
    assert_eq!(
        fixture.service.mux.read().snapshot().tabs.len(),
        initial_tabs
    );
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn empty_launch_rollback_leaves_a_workspace_close_reservation_to_its_owner() {
    let mut mux = crate::multiplexer::state::MultiplexerState::empty();
    let workspace_id = mux.create_workspace("closing workspace".into(), None).id;
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let _reservation = mux.reserve_workspace_close(&workspace_id).unwrap();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    let error = fixture
        .service
        .rollback_empty_agent_launch(&tab_id, "empty-launch");

    assert!(
        error.contains("did not resolve a launch command"),
        "{error}"
    );
    assert!(error.contains("Workspace is closing"), "{error}");
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_some());
}
