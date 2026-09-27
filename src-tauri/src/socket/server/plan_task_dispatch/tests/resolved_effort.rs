use super::*;
use crate::service::test_fixture::ServiceFixture;

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn attempt_records_resolved_reasoning_effort() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().reasoning_effort_identity =
        crate::multiplexer::types::ReasoningEffortIdentity {
            planned: None,
            requested: Some("medium".into()),
            resolved: Some("xhigh".into()),
            actual: None,
            actual_source: None,
        };
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "resolved effort".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "resolved effort attempt".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = store
        .begin_task_dispatch_with_reserved_attempt_and_gates_and_observation(
            &plan.id,
            &task_id,
            &tab_id,
            "Worker",
            TaskLaneMetadata::default(),
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            Some("claude"),
            "attempt-resolved-effort",
            Some(vec![]),
            crate::plan::WorkerPlacement::IsolatedLane,
            None,
            None,
            None,
            Some("medium".into()),
            None,
            false,
            true,
            3,
        )
        .unwrap();
    fixture
        .service
        .persist_dispatched_attempt_reasoning_effort(&plan.id, &task_id, &tab_id, &attempt_id, 4)
        .unwrap();

    let attempt = store
        .show_task(&task_id)
        .unwrap()
        .task
        .execution_attempts
        .into_iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(attempt.reasoning_effort.as_deref(), Some("xhigh"));
}
