use crate::multiplexer::{state::MultiplexerState, types::TaskBinding};
use crate::plan::{OrchestratorRef, Plan, PlanMode};
use crate::service::{
    plan::{task_assignment::AssignmentRuntimeBinding, NewTask},
    test_fixture::ServiceFixture,
};

#[test]
fn restarted_worker_attempt_is_carried_before_ack() {
    worker_attempt_carry(None, false);
}

#[test]
fn provider_switch_worker_attempt_is_carried_before_ack() {
    worker_attempt_carry(Some("target-provider"), false);
}

#[test]
fn provider_switch_resolves_acknowledged_attempt_from_live_spawn_metadata() {
    worker_attempt_carry(Some("target-provider"), true);
}

fn worker_attempt_carry(target: Option<&str>, omit_attempt: bool) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("provider".into());
    tab.agent_session_id = Some("thread".into());
    tab.session_binding_nonce = Some("old-nonce".into());
    tab.reported_session_binding_nonce = Some("old-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "restart carry".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task_id,
            &tab_id,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: "thread".into(),
                session_binding_nonce: "old-nonce".into(),
                provider: Some("provider".into()),
                model: None,
                session_model: None,
                reasoning_effort: None,
            }),
            None,
            3,
        )
        .unwrap();
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .task_binding = Some(TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task_id.clone()),
        attempt_id: (!omit_attempt).then(|| attempt_id.clone()),
        spawn_handle: Some("opaque-spawn-metadata".into()),
        bound_at: 3,
    });

    if omit_attempt {
        store
            .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
                &tab_id,
                Some(&plan.id),
                Some(&task_id),
                Some(("thread", "old-nonce")),
                4,
            )
            .unwrap();
    }
    let source_acknowledged_at =
        store.show_task(&task_id).unwrap().task.execution_attempts[0].acknowledged_at;
    assert_eq!(source_acknowledged_at.is_some(), omit_attempt);

    let authority = match target {
        Some(target) => fixture.service.authorize_provider_switch_binding(
            &tab_id,
            "provider",
            target,
            "old-nonce",
        ),
        None => fixture.service.authorize_runtime_restart_binding(
            &tab_id,
            "provider",
            "old-nonce",
            false,
        ),
    }
    .unwrap()
    .expect("the current task attempt is authorized for restart");
    let resolved = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .task_binding
        .clone()
        .unwrap();
    assert_eq!(resolved.attempt_id.as_deref(), Some(attempt_id.as_str()));
    assert_eq!(
        resolved.spawn_handle.as_deref(),
        Some("opaque-spawn-metadata")
    );
    store
        .with_plan(&plan.id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
            task.lane.worktree_path = Some(fixture.temp.path().display().to_string());
            Ok(())
        })
        .unwrap();
    let _claim = fixture.service.claim_restart(&tab_id, false).unwrap();
    fixture
        .service
        .stage_runtime_restart_binding(&tab_id, Some(authority));
    for event in [
        crate::service::worker_interruption::WorkerInterruptionEvent::ForcedContextReset,
        crate::service::worker_interruption::WorkerInterruptionEvent::ProcessExited,
    ] {
        assert!(!fixture.service.signal_worker_interruption(&tab_id, event));
        let source = store.show_task(&task_id).unwrap().task;
        assert!(source.execution_attempts[0].closed_at.is_none());
        assert!(!source.interruption.handoff_required);
    }

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        if let Some(target) = target {
            tab.agent_type = Some(target.into());
            tab.agent_session_id = Some("new-thread".into());
        }
        tab.reasoning_effort_identity.resolved = Some("opaque-effort".into());
        tab.reasoning_effort_identity.actual = Some("opaque-effort".into());
        tab.session_binding_nonce = Some("new-nonce".into());
        tab.reported_session_binding_nonce = Some("new-nonce".into());
    }
    let thread = if target.is_some() {
        "new-thread"
    } else {
        "thread"
    };
    assert!(fixture
        .service
        .carry_authorized_runtime_restart_binding(&tab_id, thread, "new-nonce")
        .unwrap());
    fixture
        .service
        .plan_store
        .mark_live_task_attempt_runtime_ready_for_tab(&tab_id, thread, 4)
        .unwrap();
    assert!(fixture
        .service
        .finalize_authorized_runtime_restart_binding(&tab_id, thread, "new-nonce")
        .unwrap());

    let rebound = store.show_task(&task_id).unwrap().task;
    assert_eq!(rebound.execution_attempts.len(), 1);
    assert_eq!(rebound.execution_attempts[0].id, attempt_id);
    assert_eq!(
        rebound.execution_attempts[0].reasoning_effort.as_deref(),
        Some("opaque-effort")
    );
    assert_eq!(
        rebound.execution_attempts[0]
            .actual_reasoning_effort
            .as_deref(),
        Some("opaque-effort")
    );
    assert_eq!(
        rebound.execution_attempts[0].acknowledged_at,
        source_acknowledged_at
    );
    assert_eq!(
        rebound.execution_attempts[0].provider.as_deref(),
        Some(target.unwrap_or("provider"))
    );
    assert_eq!(
        rebound.execution_attempts[0].session_id.as_deref(),
        Some(thread)
    );
    assert_eq!(
        rebound.execution_attempts[0]
            .session_binding_nonce
            .as_deref(),
        Some("new-nonce")
    );
    let acknowledged = store
        .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
            &tab_id,
            Some(&plan.id),
            Some(&task_id),
            Some((thread, "new-nonce")),
            5,
        )
        .unwrap();
    assert_eq!(acknowledged.attempt.id, attempt_id);
    assert_eq!(acknowledged.plan_id, plan.id);
    assert_eq!(acknowledged.task_id, task_id);
    assert_eq!(
        acknowledged.already_acknowledged,
        source_acknowledged_at.is_some()
    );
    let expected_acknowledged_at = source_acknowledged_at.or(Some(5));
    assert_eq!(
        acknowledged.attempt.acknowledged_at,
        expected_acknowledged_at
    );
    let persisted = store.show_task(&task_id).unwrap().task;
    assert_eq!(persisted.execution_attempts.len(), 1);
    assert_eq!(persisted.execution_attempts[0].id, attempt_id);
    assert_eq!(
        persisted.execution_attempts[0].acknowledged_at,
        expected_acknowledged_at
    );
}
