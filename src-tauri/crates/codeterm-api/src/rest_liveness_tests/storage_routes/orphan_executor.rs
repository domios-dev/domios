use super::*;
use codeterm_service_core::multiplexer::types::{AgentRole, TaskBinding};
use codeterm_service_core::plan::{OrchestratorRef, Plan};
use codeterm_service_core::service::plan::task_assignment::AssignmentRuntimeBinding;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn executor_close_releases_mux_while_termination_sql_is_held() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&owner))
        .unwrap();
    let tab = mux.get_tab_mut(&worker).unwrap();
    tab.agent_session_id = Some("executor-session".into());
    tab.session_binding_nonce = Some("executor-nonce".into());
    tab.reported_session_binding_nonce = Some("executor-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let store = state.svc.plan_store.clone();
    let mut plan = Plan::new(
        "Executor close SQL".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            codeterm_service_core::service::plan::NewTask {
                name: "Close safely".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task,
            &worker,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: "executor-session".into(),
                session_binding_nonce: "executor-nonce".into(),
                provider: None,
                model: None,
                session_model: None,
                reasoning_effort: None,
            }),
            None,
            3,
        )
        .unwrap();
    let attempt = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .iter()
        .find(|item| item.id == task)
        .unwrap()
        .execution_attempts
        .last()
        .unwrap()
        .id
        .clone();
    state
        .svc
        .mux
        .write()
        .get_tab_mut(&worker)
        .unwrap()
        .task_binding = Some(TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task.clone()),
        attempt_id: Some(attempt.clone()),
        ..Default::default()
    });
    let held_store = store.clone();
    let held = HeldDependency::new(move |entered, release| {
        held_store.with_mutation_writer_held_for_test(|| {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(10)).unwrap();
        });
    })
    .await;
    let service = state.svc.clone();
    let closing = worker.clone();
    let operation = tokio::task::spawn_blocking(move || service.close_tab(&closing));
    let observer = store.clone();
    assert!(tokio::task::spawn_blocking(
        move || observer.wait_for_queued_mutation_for_test(Duration::from_secs(2))
    )
    .await
    .unwrap());
    assert!(!operation.is_finished());
    let available = state.svc.mux.try_write().is_some();
    crate::rest_liveness_tests::assert_health_list_send_respond(state.clone()).await;
    drop(held);
    operation.await.unwrap().unwrap();
    assert!(available, "termination SQL wait must release mux");
    assert!(state.svc.mux.read().get_tab(&worker).is_none());
    let current = store.get(&plan.id).unwrap();
    let persisted = current
        .tasks
        .iter()
        .find(|item| item.id == task)
        .unwrap()
        .execution_attempts
        .iter()
        .find(|item| item.id == attempt)
        .unwrap();
    assert!(persisted.executor_termination.is_some());
    assert!(persisted.retired_at.is_some());
}
