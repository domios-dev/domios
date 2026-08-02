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

    assert_eq!(event["kind"], "worker_ack_startup_failed");
    assert_eq!(event["deadline_seconds"], 120);
    assert_eq!(event["expected_command"], "codeterm agent ack");
    assert!(event["fallback"]
        .as_str()
        .unwrap()
        .contains("Inspect the preserved lane"));
    assert!(event["fallback"]
        .as_str()
        .unwrap()
        .contains("replacement Worker"));
}

#[test]
fn worker_ack_transitions_exact_pending_task_to_in_progress() {
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
    assert_eq!(task.status_label.as_deref(), Some("acknowledged"));
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
fn worker_ack_replay_or_stale_attempt_does_not_transition_task() {
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

    let mut blocked_plan = store.get(&plan.id).unwrap();
    let blocked_task = blocked_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    blocked_task.status = TaskStatus::Blocked;
    blocked_task.status_label = Some("dependency".into());
    store.save(&mut blocked_plan, 5).unwrap();
    assert!(store
        .acknowledge_task_attempt(&plan.id, &task_id, "tab-worker", &attempt_id, 6)
        .is_err());
    let still_blocked = store.show_task(&task_id).unwrap().task;
    assert_eq!(still_blocked.status, TaskStatus::Blocked);
    assert_eq!(still_blocked.execution_attempts[0].acknowledged_at, None);

    let mut reassigned_plan = store.get(&plan.id).unwrap();
    let reassigned_task = reassigned_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    reassigned_task.status = TaskStatus::Pending;
    reassigned_task.status_label = Some("starting".into());
    reassigned_task.assigned_tab_id = Some("replacement-tab".into());
    reassigned_task.lane.tab_id = Some("replacement-tab".into());
    store.save(&mut reassigned_plan, 7).unwrap();
    assert!(store
        .acknowledge_task_attempt(&plan.id, &task_id, "tab-worker", &attempt_id, 8)
        .is_err());
    let still_reassigned = store.show_task(&task_id).unwrap().task;
    assert_eq!(
        still_reassigned.assigned_tab_id.as_deref(),
        Some("replacement-tab")
    );
    assert_eq!(still_reassigned.execution_attempts[0].acknowledged_at, None);

    let mut restored_plan = store.get(&plan.id).unwrap();
    let restored_task = restored_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    restored_task.assigned_tab_id = Some("tab-worker".into());
    restored_task.lane.tab_id = Some("tab-worker".into());
    store.save(&mut restored_plan, 9).unwrap();

    let ack = store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 10)
        .unwrap();
    assert_eq!(ack.attempt.acknowledged_at, Some(10));
    let acknowledged = store.show_task(&task_id).unwrap().task;
    assert_eq!(acknowledged.status, TaskStatus::InProgress);
    assert_eq!(acknowledged.status_label.as_deref(), Some("acknowledged"));
    let first_changed_at = acknowledged.status_changed_at;

    let replay = store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 11)
        .unwrap();
    assert_eq!(replay.attempt.acknowledged_at, Some(10));
    let after_replay = store.show_task(&task_id).unwrap().task;
    assert_eq!(after_replay.status_changed_at, first_changed_at);
    assert_eq!(after_replay.status_label.as_deref(), Some("acknowledged"));
    assert!(store
        .acknowledge_task_attempt(&plan.id, &task_id, "stale-tab", &attempt_id, 12)
        .is_err());
    let after_stale = store.show_task(&task_id).unwrap().task;
    assert_eq!(after_stale.status_changed_at, first_changed_at);
    assert_eq!(after_stale.execution_attempts[0].acknowledged_at, Some(10));

    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            "session-worker",
            lane,
            13,
        )
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label.as_deref(), Some("acknowledged"));
    assert_eq!(
        task.execution_attempts.last().unwrap().acknowledged_at,
        Some(10)
    );

    let mut runtime_projection = store.get(&plan.id).unwrap();
    runtime_projection
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap()
        .status_label = None;
    store.save(&mut runtime_projection, 14).unwrap();
    let replay_after_readiness = store
        .acknowledge_task_attempt(&plan.id, &task_id, "tab-worker", &attempt_id, 15)
        .unwrap();
    assert_eq!(replay_after_readiness.attempt.acknowledged_at, Some(10));
    let after_ready_replay = store.show_task(&task_id).unwrap().task;
    assert_eq!(after_ready_replay.status, TaskStatus::InProgress);
    assert_eq!(after_ready_replay.status_label, None);
}

#[test]
fn worker_ack_final_deadline_projects_one_startup_failure_cleanup() {
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
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, "tab-worker", "Ada", lane, 4)
        .unwrap();
    let mut with_older_attempt = store.get(&plan.id).unwrap();
    let task = with_older_attempt
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    let mut older_attempt = task.execution_attempts[0].clone();
    older_attempt.id = "attempt-older-same-tab".into();
    task.execution_attempts.insert(0, older_attempt);
    store.save(&mut with_older_attempt, 5).unwrap();

    assert!(store
        .mark_worker_startup_deadline(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            false,
            6,
        )
        .unwrap());
    assert_eq!(
        store.show_task(&task_id).unwrap().task.status_label.as_deref(),
        Some("startup_unbound")
    );
    assert!(store
        .mark_worker_startup_deadline(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            true,
            7,
        )
        .unwrap());
    assert!(!store
        .mark_worker_startup_deadline(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            true,
            8,
        )
        .unwrap());
    let failed = store.show_task(&task_id).unwrap().task;
    assert_eq!(failed.status_label.as_deref(), Some("startup_failed"));
    assert_eq!(failed.assigned_tab_id, None);
    assert_eq!(failed.lane.tab_id, None);
    assert_eq!(failed.execution_attempts[0].closed_at, None);
    assert_eq!(failed.execution_attempts[1].closed_at, Some(7));
    assert!(store
        .mark_worker_startup_cleanup_failed(&plan.id, &task_id, &attempt_id, 9)
        .unwrap());
    assert_eq!(
        store.show_task(&task_id).unwrap().task.status_label.as_deref(),
        Some("startup_cleanup_failed")
    );
}

#[test]
fn blocked_before_worker_ack_deadline_receives_no_startup_mutation() {
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
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, "tab-worker", "Ada", lane, 4)
        .unwrap();
    let mut blocked_plan = store.get(&plan.id).unwrap();
    let blocked = blocked_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    blocked.status = TaskStatus::Blocked;
    blocked.status_label = Some("dependency".into());
    store.save(&mut blocked_plan, 5).unwrap();

    assert!(!store
        .mark_worker_startup_deadline(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            false,
            6,
        )
        .unwrap());
    assert!(!store
        .mark_worker_startup_deadline(
            &plan.id,
            &task_id,
            "tab-worker",
            &attempt_id,
            true,
            7,
        )
        .unwrap());
    let unchanged = store.show_task(&task_id).unwrap().task;
    assert_eq!(unchanged.status, TaskStatus::Blocked);
    assert_eq!(unchanged.status_label.as_deref(), Some("dependency"));
    assert_eq!(unchanged.assigned_tab_id.as_deref(), Some("tab-worker"));
    assert_eq!(unchanged.execution_attempts[0].closed_at, None);
}

#[test]
fn competition_ack_reminder_and_cleanup_use_exact_attempt_authority() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Competition".into(),
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
                name: "competitive worker".into(),
                competition: Some(crate::plan::TaskCompetitionMetadata {
                    enabled: true,
                    attempts: Some(3),
                    ..Default::default()
                }),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let drafts = store
        .build_task_compete_drafts(
            &plan.id,
            &task_id,
            crate::service::plan::task_attempts::TaskCompeteRequest {
                attempts: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
    let mut attempts = Vec::new();
    for (index, draft) in drafts.iter().enumerate() {
        let tab_id = format!("competitor-{index}");
        let created = store
            .record_task_compete_attempt(
                &plan.id,
                &task_id,
                &tab_id,
                draft,
                Some("worker"),
                4 + index as u64,
            )
            .unwrap();
        store
            .mark_task_attempt_runtime_ready(
                &plan.id,
                &task_id,
                &tab_id,
                &created.attempt.id,
                &format!("session-{index}"),
                10 + index as u64,
            )
            .unwrap();
        attempts.push((tab_id, created.attempt.id));
    }
    assert_eq!(store.show_task(&task_id).unwrap().task.assigned_tab_id, None);

    store
        .acknowledge_task_attempt(&plan.id, &task_id, &attempts[0].0, &attempts[0].1, 20)
        .unwrap();
    store
        .acknowledge_task_attempt(&plan.id, &task_id, &attempts[1].0, &attempts[1].1, 21)
        .unwrap();
    assert!(store.worker_startup_attempt_owns(
        &plan.id,
        &task_id,
        &attempts[2].0,
        &attempts[2].1,
        Some("session-2"),
    ));
    assert!(!store.worker_startup_attempt_owns(
        &plan.id,
        &task_id,
        &attempts[2].0,
        &attempts[2].1,
        Some("session-wrong"),
    ));

    let mut blocked_plan = store.get(&plan.id).unwrap();
    let task = blocked_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = TaskStatus::Blocked;
    task.status_label = Some("judge dependency".into());
    store.save(&mut blocked_plan, 22).unwrap();
    assert!(!store.worker_startup_attempt_owns(
        &plan.id,
        &task_id,
        &attempts[2].0,
        &attempts[2].1,
        Some("session-2"),
    ));
    assert!(!store
        .mark_worker_startup_deadline(
            &plan.id,
            &task_id,
            &attempts[2].0,
            &attempts[2].1,
            true,
            23,
        )
        .unwrap());

    let mut resumed_plan = store.get(&plan.id).unwrap();
    let task = resumed_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = TaskStatus::InProgress;
    task.status_label = Some("competitive".into());
    store.save(&mut resumed_plan, 24).unwrap();
    assert!(store
        .mark_worker_startup_deadline(
            &plan.id,
            &task_id,
            &attempts[2].0,
            &attempts[2].1,
            true,
            25,
        )
        .unwrap());
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label.as_deref(), Some("competitive"));
    assert_eq!(task.execution_attempts[0].closed_at, None);
    assert_eq!(task.execution_attempts[1].closed_at, None);
    assert_eq!(task.execution_attempts[2].closed_at, Some(25));
}
