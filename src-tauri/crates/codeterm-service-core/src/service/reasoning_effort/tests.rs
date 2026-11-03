use super::*;

#[test]
fn stale_refresh_cannot_replace_a_newer_confirmed_command_or_binding() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("provider".into());
    tab.agent_session_id = Some("own".into());
    apply_session_owned_actual(tab, "provider", "own", None, Some("low".into()));
    let observation = EffortObservation::capture(tab);
    persist_launch_identity(tab, Some("medium"), Some("medium"));
    tab.reasoning_effort_identity.actual = Some("medium".into());
    tab.reasoning_effort_identity.actual_source = Some("session-command".into());
    assert!(!apply_observed_actual(
        tab,
        "provider",
        "own",
        None,
        Some("low".into()),
        Some(&observation)
    ));
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("medium")
    );
    let observation = EffortObservation::capture(tab);
    assert!(apply_observed_actual(
        tab,
        "provider",
        "own",
        None,
        Some("medium".into()),
        Some(&observation)
    ));
    let observation = EffortObservation::capture(tab);
    assert!(apply_observed_actual(
        tab,
        "provider",
        "own",
        None,
        Some("high".into()),
        Some(&observation)
    ));
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("high")
    );
    let observation = EffortObservation::capture(tab);
    tab.session_binding_nonce = Some("new-launch".into());
    assert!(!apply_observed_actual(
        tab,
        "provider",
        "own",
        None,
        Some("low".into()),
        Some(&observation)
    ));
}

#[test]
fn session_owned_effort_attestation_never_uses_global_latest() {
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("owned-session".into());
    persist_launch_identity(tab, Some("high"), Some("medium"));

    assert_eq!(
        tab.reasoning_effort_identity.planned.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("medium")
    );
    assert_eq!(
        tab.reasoning_effort_identity.resolved.as_deref(),
        Some("medium")
    );

    assert!(!apply_session_owned_actual(
        tab,
        "claude",
        "latest-foreign-session",
        None,
        Some("max".into()),
    ));
    assert_eq!(tab.reasoning_effort_identity.actual, None);
    assert_eq!(
        tab.reasoning_effort_identity.actual_source.as_deref(),
        Some("provider-default-unverified")
    );
    assert!(apply_session_owned_actual(
        tab,
        "claude",
        "owned-session",
        None,
        Some("high".into()),
    ));
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.actual_source.as_deref(),
        Some("session-owned")
    );
}

#[test]
fn non_plan_launch_keeps_planned_effort_null() {
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    persist_launch_identity(tab, None, Some("high"));

    assert_eq!(tab.reasoning_effort_identity.planned, None);
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.resolved.as_deref(),
        Some("high")
    );
}

#[test]
fn attempt_records_actual_effort_for_claude_and_codex() {
    use crate::agent::test_provider::MockProvider;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
    use crate::service::plan::NewTask;

    let (mut mux, claude_tab) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&claude_tab).unwrap();
    let codex_tab = mux.create_tab(&workspace_id).unwrap();
    let identities = [
        ("claude", "claude-model", "claude-session", "high"),
        ("codex", "codex-model", "codex-session", "xhigh"),
    ];
    for (tab_id, (provider, model, session, _)) in
        [&claude_tab, &codex_tab].into_iter().zip(identities)
    {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(format!("nonce-{session}"));
        tab.reported_session_binding_nonce = Some(format!("nonce-{session}"));
        tab.agent_provider_resolved_model = Some(model.into());
        tab.cwd = Some("/fixture/project".into());
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    for (provider, model, session, actual) in identities {
        fixture.service.agent_registry.register_provider(Box::new(
            MockProvider::with_model_reasoning_efforts(
                provider,
                model,
                &["low", "medium", "high", "xhigh"],
            )
            .with_session_reasoning_effort(session, actual)
            .with_session_reasoning_effort("foreign-session", "medium"),
        ));
    }

    let mut plan = Plan::new(
        "Session effort identity".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&claude_tab),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    for (tab_id, (provider, model, session, actual)) in
        [&claude_tab, &codex_tab].into_iter().zip(identities)
    {
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: format!("{provider} task"),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let nonce = format!("nonce-{session}");
        fixture
            .service
            .plan_store
            .assign_task_with_runtime_binding(
                &plan.id,
                &task_id,
                tab_id,
                None,
                None,
                Some(&AssignmentRuntimeBinding {
                    session_id: session.into(),
                    session_binding_nonce: nonce,
                    provider: Some(provider.into()),
                    model: Some(model.into()),
                    session_model: Some(model.into()),
                    reasoning_effort: Some("medium".into()),
                }),
                None,
                3,
            )
            .unwrap();
        fixture.service.refresh_session_reasoning_effort_once(
            tab_id,
            provider,
            "/fixture/project",
            session,
        );
        let task = fixture
            .service
            .plan_store
            .get(&plan.id)
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(
            task.execution_attempts[0]
                .actual_reasoning_effort
                .as_deref(),
            Some(actual)
        );
        assert_eq!(
            task.execution_attempts[0].reasoning_effort.as_deref(),
            Some("medium")
        );
    }
}
