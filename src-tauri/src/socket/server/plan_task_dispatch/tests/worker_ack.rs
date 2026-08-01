use super::*;

#[test]
fn timeout_event_is_actionable_without_authorizing_a_resend() {
    let event = worker_ack_timeout_event(
        "plan",
        "task",
        "tab",
        "attempt",
        "codeterm plan task dispatch plan task",
    );

    assert_eq!(event["kind"], "worker_ack_timeout");
    assert_eq!(event["deadline_seconds"], 60);
    assert_eq!(event["expected_command"], "codeterm agent ack");
    assert!(event["fallback"]
        .as_str()
        .unwrap()
        .contains("Inspect the existing tab once"));
    assert!(event["fallback"]
        .as_str()
        .unwrap()
        .contains("Do not blind-resend"));
}

#[test]
fn runtime_ready_then_acknowledged_enters_progress() {
    let (root, project) = git_project();
    let resolved = resolve_dispatch_lane(
        None,
        project.to_str().unwrap(),
        &TaskLaneMetadata::default(),
    )
    .unwrap();
    let provision_count = Cell::new(1);
    let lane = provision_lane_for_test(&resolved.spawn_cwd, "dispatch_success").unwrap();
    let actual = lane.verify().unwrap();
    assert_eq!(provision_count.get(), 1);
    assert_eq!(git_worktree_count(&project), 2);

    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Dispatch".into(),
            project.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut approved = store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    store.save(&mut approved, 2).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                lane: TaskLaneMetadata {
                    workspace_id: Some("workspace".into()),
                    destination: Some("master".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let original = store.show_task(&task_id).unwrap().task;
    let lane_metadata = lane_metadata_for_dispatch(&original.lane, &actual);
    let attempt_id = store
        .begin_task_dispatch(
            &plan.id,
            &task_id,
            "tab-worker",
            "Ada",
            lane_metadata.clone(),
            4,
        )
        .unwrap();
    let starting = store.show_task(&task_id).unwrap().task;
    assert_eq!(starting.status, TaskStatus::Pending);
    assert_eq!(starting.status_label.as_deref(), Some("starting"));

    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            "session-worker",
            lane_metadata,
            5,
        )
        .unwrap();
    let awaiting = store.show_task(&task_id).unwrap().task;
    assert_eq!(awaiting.status, TaskStatus::Pending);
    assert_eq!(
        awaiting.status_label.as_deref(),
        Some("awaiting_worker_ack")
    );

    store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 6)
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    let attempt = task.execution_attempts.last().unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label, None);
    assert_eq!(attempt.acknowledged_at, Some(6));
    assert_eq!(attempt.session_id.as_deref(), Some("session-worker"));
    assert_eq!(attempt.worktree_path, task.lane.worktree_path);
    assert_eq!(attempt.branch, task.lane.branch);

    let repeated = store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 7)
        .unwrap();
    assert_eq!(repeated.attempt.acknowledged_at, Some(6));
    lane.cleanup().unwrap();
}

#[test]
fn acknowledged_then_runtime_ready_enters_progress() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Dispatch".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut approved = store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    store.save(&mut approved, 2).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                lane: TaskLaneMetadata {
                    workspace_id: Some("workspace".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, "tab-worker", "Ada", lane.clone(), 4)
        .unwrap();

    let ack = store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 5)
        .unwrap();
    assert_eq!(ack.attempt.acknowledged_at, Some(5));
    assert_eq!(
        store.show_task(&task_id).unwrap().task.status,
        TaskStatus::Pending
    );

    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            "session-worker",
            lane,
            6,
        )
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label, None);
    assert_eq!(
        task.execution_attempts.last().unwrap().acknowledged_at,
        Some(5)
    );
}
