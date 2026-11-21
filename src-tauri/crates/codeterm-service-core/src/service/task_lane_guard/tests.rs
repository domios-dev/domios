use super::{canonical_dispatch_remedy, live_canonical_writer, task_bound_mutating_spawn_ref};
use crate::plan::{
    ExecutionIntent, OrchestratorRef, Plan, Task, TaskExecutionAttempt, TaskExecutionIntent,
    TaskExecutorTermination, TaskStatus, WorkerPlacement,
};
use crate::service::orchestrator::SpawnAgentParams;

#[test]
fn taskless_builtin_worker_with_plan_context_skips_task_bound_guard() {
    for kind in ["qa", "release_manager", "reviewer"] {
        for intent in [ExecutionIntent::Mutating, ExecutionIntent::ReadOnly] {
            let params = SpawnAgentParams {
                role: Some("Worker".into()),
                role_profile: Some(kind.into()),
                plan_id: Some("plan-bound-context".into()),
                execution_intent: Some(intent),
                cwd: Some("C:/repo".into()),
                ..Default::default()
            };

            assert_eq!(
                task_bound_mutating_spawn_ref(&params),
                None,
                "{kind} with Plan context stays on parent-directed task-less behavior"
            );
        }
    }

    let task_bound = SpawnAgentParams {
        task_id: Some("task-bound".into()),
        execution_intent: Some(ExecutionIntent::Mutating),
        ..Default::default()
    };
    assert_eq!(
        task_bound_mutating_spawn_ref(&task_bound),
        Some("task-bound")
    );
}

#[test]
fn canonical_dispatch_remedy_preserves_selected_builtin_worker_kind() {
    for kind in ["qa", "release_manager", "reviewer"] {
        assert_eq!(
                canonical_dispatch_remedy("plan", "task", Some(kind)),
                format!(
                    "Use parent-authorized canonical placement so its exclusive attempt is reserved before any tab exists: domios plan task dispatch plan task --execution-intent mutating --canonical-writer --kind {kind}"
                )
            );
    }
    assert_eq!(
            canonical_dispatch_remedy("plan", "task", Some("unavailable-overlay")),
            "Use parent-authorized canonical placement so its exclusive attempt is reserved before any tab exists: domios plan task dispatch plan task --execution-intent mutating --canonical-writer"
        );
}

#[test]
fn live_canonical_writer_excludes_readonly_nonrepository_and_confirmed_cancelled_attempts() {
    let mut plan = Plan::new(
        "Authority".into(),
        "C:/repo".into(),
        OrchestratorRef::from_tab("owner"),
    );
    let base_task = |id: &str, status: TaskStatus| {
        let mut task: Task = serde_json::from_value(serde_json::json!({
            "id": id,
            "name": id,
            "type": "implement",
            "status": status,
        }))
        .unwrap();
        task.execution_intent = TaskExecutionIntent::RepositoryMutation;
        task
    };

    let mut ro = base_task("task_ro", TaskStatus::InProgress);
    ro.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt_ro".into(),
        executor_tab_id: "tab-ro".into(),
        placement: WorkerPlacement::ReadOnly,
        execution_intent: Some(ExecutionIntent::ReadOnly),
        ..Default::default()
    });
    let mut nonrepo = base_task("task_nr", TaskStatus::InProgress);
    nonrepo.execution_intent = TaskExecutionIntent::NonRepository;
    nonrepo.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt_nr".into(),
        executor_tab_id: "tab-nr".into(),
        placement: WorkerPlacement::CanonicalWriter,
        execution_intent: Some(ExecutionIntent::Mutating),
        ..Default::default()
    });
    let mut ro_intent = base_task("task_ro_intent", TaskStatus::InProgress);
    ro_intent.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt_ro_intent".into(),
        executor_tab_id: "tab-ro-intent".into(),
        placement: WorkerPlacement::CanonicalWriter,
        execution_intent: Some(ExecutionIntent::ReadOnly),
        ..Default::default()
    });
    let mut foreign_checkout = base_task("task_foreign", TaskStatus::InProgress);
    foreign_checkout
        .execution_attempts
        .push(TaskExecutionAttempt {
            id: "attempt_foreign".into(),
            executor_tab_id: "tab-foreign".into(),
            placement: WorkerPlacement::CanonicalWriter,
            execution_intent: Some(ExecutionIntent::Mutating),
            checkout_path: Some("C:/other-repo".into()),
            ..Default::default()
        });
    let mut cancelled_confirmed = base_task("task_cc", TaskStatus::Cancelled);
    cancelled_confirmed
        .execution_attempts
        .push(TaskExecutionAttempt {
            id: "attempt_cc".into(),
            executor_tab_id: "tab-cc".into(),
            session_id: Some("sess-cc".into()),
            session_binding_nonce: Some("nonce-cc".into()),
            placement: WorkerPlacement::CanonicalWriter,
            execution_intent: Some(ExecutionIntent::Mutating),
            executor_termination: Some(TaskExecutorTermination {
                attempt_id: "attempt_cc".into(),
                executor_tab_id: "tab-cc".into(),
                session_id: Some("sess-cc".into()),
                session_binding_nonce: Some("nonce-cc".into()),
                confirmed_at: 42,
            }),
            ..Default::default()
        });

    plan.tasks = vec![
        ro,
        nonrepo,
        ro_intent,
        foreign_checkout,
        cancelled_confirmed,
    ];
    assert_eq!(live_canonical_writer(&plan), None);

    let mut cancelled_live = base_task("task_cl", TaskStatus::Cancelled);
    cancelled_live.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt_cl".into(),
        executor_tab_id: "tab-cl".into(),
        session_id: Some("sess-cl".into()),
        session_binding_nonce: Some("nonce-cl".into()),
        placement: WorkerPlacement::CanonicalWriter,
        execution_intent: Some(ExecutionIntent::Mutating),
        ..Default::default()
    });
    plan.tasks.push(cancelled_live);
    assert_eq!(
        live_canonical_writer(&plan),
        Some(("task_cl", "attempt_cl")),
        "status-only cancellation must not release a live mutating canonical writer"
    );

    plan.tasks
        .last_mut()
        .unwrap()
        .execution_attempts[0]
        .executor_termination = Some(TaskExecutorTermination {
        attempt_id: "attempt_cl".into(),
        executor_tab_id: "tab-cl".into(),
        session_id: Some("sess-cl".into()),
        session_binding_nonce: Some("wrong-nonce".into()),
        confirmed_at: 43,
    });
    assert_eq!(
        live_canonical_writer(&plan),
        Some(("task_cl", "attempt_cl")),
        "mismatched termination proof must not release a live mutating canonical writer"
    );

    plan.tasks
        .last_mut()
        .unwrap()
        .execution_attempts[0]
        .executor_termination
        .as_mut()
        .unwrap()
        .session_binding_nonce = Some("nonce-cl".into());
    assert_eq!(live_canonical_writer(&plan), None);

    plan.project_path.clear();
    let mut unrecorded = base_task("task_unrecorded", TaskStatus::InProgress);
    unrecorded.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt_unrecorded".into(),
        executor_tab_id: "tab-u".into(),
        placement: WorkerPlacement::CanonicalWriter,
        execution_intent: Some(ExecutionIntent::Mutating),
        ..Default::default()
    });
    plan.tasks = vec![unrecorded];
    assert_eq!(
        live_canonical_writer(&plan),
        Some(("task_unrecorded", "attempt_unrecorded")),
        "non-empty canonical_project_path still anchors checkout authority"
    );

    plan.canonical_project_path.clear();
    assert_eq!(live_canonical_writer(&plan), None);
}
