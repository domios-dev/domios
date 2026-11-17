use super::*;

pub(super) fn switch_worker_fixture() -> (RuntimeFixture, String, String) {
    prepared_switch_worker_fixture(true)
}

pub(super) fn prepared_switch_worker_fixture(storage: bool) -> (RuntimeFixture, String, String) {
    let fixture = RuntimeFixture::prepared(SOURCE, storage);
    let source_transcript = fixture.temp.path().join("source-thread.jsonl");
    std::fs::write(&source_transcript, []).unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session_file(
            "source-provider",
            source_transcript,
        ),
    ));
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_type = Some("source-provider".into());
        tab.agent_session_id = Some("source-thread".into());
        tab.session_binding_nonce = Some("source-nonce".into());
        tab.reported_session_binding_nonce = Some("source-nonce".into());
        tab.backing = TabBacking::Pty;
        tab.pty_id = Some("source-pty-generation".into());
        tab.lifecycle_epoch = Some(1);
        tab.agent_booting = false;
    }
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "switch fixture".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "opaque work".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task,
            &fixture.tab,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: "source-thread".into(),
                session_binding_nonce: "source-nonce".into(),
                provider: Some("source-provider".into()),
                model: None,
                session_model: None,
                reasoning_effort: None,
            }),
            None,
            3,
        )
        .unwrap();
    store
        .edit_plan_fixture(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|entry| entry.id == task)
                .unwrap();
            task.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
            task.lane.worktree_path = Some(fixture.temp.path().display().to_string());
            Ok(())
        })
        .unwrap();
    let attempt = store
        .inspect_task_for_test(&task)
        .unwrap()
        .task
        .execution_attempts[0]
        .id
        .clone();
    store
        .acknowledge_task_attempt(&plan.id, &task, &fixture.tab, &attempt, 4)
        .unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .task_binding = Some(TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task.clone()),
        attempt_id: Some(attempt.clone()),
        ..Default::default()
    });
    (fixture, task, attempt)
}
