use super::*;

/// A Worker tab that reached `awaiting_worker_ack`: one live, session-bound
/// attempt whose task is still pending, which is the state `agent ack` meets.
fn dispatched_worker_task(
    store: &PlanStore,
    project: &str,
    team_id: &str,
    tab_id: &str,
    base: u64,
) -> (String, String, String) {
    let plan = store
        .create_plan_for_team(
            format!("{team_id} dispatch"),
            project.to_string(),
            OrchestratorRef::from_tab("orch"),
            team_id.to_string(),
            vec![],
            base,
        )
        .unwrap();
    let mut approved = store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    store.save(&mut approved, base + 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: format!("{team_id} worker"),
                ..Default::default()
            },
            base + 2,
        )
        .unwrap();
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, tab_id, "Ada", lane.clone(), base + 3)
        .unwrap();
    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            tab_id,
            &attempt_id,
            &format!("session-{tab_id}"),
            &format!("nonce-{tab_id}"),
            lane,
            base + 4,
        )
        .unwrap();
    (plan.id, task_id, attempt_id)
}

#[test]
fn stale_subject_env_still_acknowledges_the_tab_bound_attempt() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, attempt_id) =
        dispatched_worker_task(&store, &project, "current", "tab-worker", 1);
    // The stale subject must name a REAL other task: an id that exists nowhere
    // would be rejected as absent and never prove the subject is only a hint.
    let (stale_plan_id, stale_task_id, _) =
        dispatched_worker_task(&store, &project, "stale", "tab-other", 100);
    assert_ne!(stale_plan_id, plan_id);
    assert_ne!(stale_task_id, task_id);

    let acknowledged = store
        .acknowledge_live_task_attempt_for_tab(
            "tab-worker",
            Some(&stale_plan_id),
            Some(&stale_task_id),
            200,
        )
        .unwrap();

    assert_eq!(acknowledged.plan_id, plan_id);
    assert_eq!(acknowledged.task_id, task_id);
    assert_eq!(acknowledged.attempt.id, attempt_id);
    assert_eq!(acknowledged.attempt.acknowledged_at, Some(200));
    assert_eq!(
        store.show_task(&task_id).unwrap().task.status,
        TaskStatus::InProgress
    );
    // The subject named by the stale environment is untouched.
    let stale = store.show_task(&stale_task_id).unwrap().task;
    assert_eq!(stale.status, TaskStatus::Pending);
    assert_eq!(stale.execution_attempts[0].acknowledged_at, None);
}

#[test]
fn respawned_worker_acknowledges_its_live_in_progress_attempt() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, first_attempt_id) =
        dispatched_worker_task(&store, &project, "respawn", "tab-first", 1);
    store
        .acknowledge_live_task_attempt_for_tab("tab-first", None, None, 10)
        .unwrap();
    // Retirement is the fence the respawn must survive rather than bypass.
    let retirement = store
        .retire_stale_task_owner(&plan_id, &task_id, None, |_| false, 11)
        .unwrap();
    assert_eq!(retirement.retired_attempt_ids, vec![first_attempt_id]);

    let lane = store.show_task(&task_id).unwrap().task.lane;
    let respawn_attempt_id = store
        .begin_task_dispatch(&plan_id, &task_id, "tab-respawn", "Ada", lane.clone(), 12)
        .unwrap();
    store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            "tab-respawn",
            &respawn_attempt_id,
            "session-respawn",
            "nonce-respawn",
            lane,
            13,
        )
        .unwrap();
    // Proven, not assumed: the respawn meets ack while the task is in_progress.
    let before_ack = store.show_task(&task_id).unwrap().task;
    assert_eq!(before_ack.status, TaskStatus::InProgress);
    assert_eq!(
        before_ack.status_label.as_deref(),
        Some("awaiting_worker_ack")
    );

    let acknowledged = store
        .acknowledge_live_task_attempt_for_tab("tab-respawn", None, None, 14)
        .unwrap();

    assert_eq!(acknowledged.attempt.id, respawn_attempt_id);
    assert_eq!(acknowledged.attempt.acknowledged_at, Some(14));
    let after = store.show_task(&task_id).unwrap().task;
    assert_eq!(after.status, TaskStatus::InProgress);
    assert_eq!(after.status_label.as_deref(), Some("acknowledged"));
    assert_eq!(after.assigned_tab_id.as_deref(), Some("tab-respawn"));
}

#[test]
fn superseded_ack_error_names_retired_and_live_attempts() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, retired_attempt_id) =
        dispatched_worker_task(&store, &project, "superseded", "tab-retired", 1);
    store
        .acknowledge_live_task_attempt_for_tab("tab-retired", None, None, 10)
        .unwrap();
    store
        .retire_stale_task_owner(&plan_id, &task_id, None, |_| false, 11)
        .unwrap();
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let live_attempt_id = store
        .begin_task_dispatch(&plan_id, &task_id, "tab-live", "Ada", lane.clone(), 12)
        .unwrap();
    store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            "tab-live",
            &live_attempt_id,
            "session-live",
            "nonce-live",
            lane,
            13,
        )
        .unwrap();

    let error = store
        .acknowledge_live_task_attempt_for_tab("tab-retired", None, None, 14)
        .unwrap_err()
        .to_string();

    assert!(error.contains(crate::service::plan::ACK_CAUSE_ATTEMPT_SUPERSEDED));
    assert!(error.contains(&retired_attempt_id), "{error}");
    assert!(error.contains(&live_attempt_id), "{error}");
}

#[test]
fn a_closed_attempt_without_a_replacement_is_not_reported_as_superseded() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, attempt_id) =
        dispatched_worker_task(&store, &project, "no-replacement", "tab-retired", 1);
    store
        .acknowledge_live_task_attempt_for_tab("tab-retired", None, None, 10)
        .unwrap();
    store
        .retire_stale_task_owner(&plan_id, &task_id, None, |_| false, 11)
        .unwrap();

    let error = store
        .acknowledge_live_task_attempt_for_tab("tab-retired", None, None, 12)
        .unwrap_err()
        .to_string();

    assert!(error.contains(crate::service::plan::ACK_CAUSE_NO_LIVE_ATTEMPT));
    assert!(!error.contains(crate::service::plan::ACK_CAUSE_ATTEMPT_SUPERSEDED));
    assert!(error.contains(&attempt_id), "{error}");
}

#[test]
fn retired_attempt_stays_fenced_from_dispatch_ready_and_ack() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, retired_attempt_id) =
        dispatched_worker_task(&store, &project, "fence", "tab-worker", 1);
    store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 10)
        .unwrap();
    store
        .retire_stale_task_owner(&plan_id, &task_id, None, |_| false, 11)
        .unwrap();
    // The same tab respawns, so the late calls clear the assigned-tab rung and
    // reach the retirement fence itself.
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let live_attempt_id = store
        .begin_task_dispatch(&plan_id, &task_id, "tab-worker", "Ada", lane.clone(), 12)
        .unwrap();
    assert_ne!(live_attempt_id, retired_attempt_id);
    assert_eq!(
        store
            .show_task(&task_id)
            .unwrap()
            .task
            .assigned_tab_id
            .as_deref(),
        Some("tab-worker")
    );

    assert!(store
        .finish_task_dispatch(
            &plan_id,
            &task_id,
            "tab-worker",
            &retired_attempt_id,
            "session-late",
            "nonce-late",
            lane,
            13,
        )
        .is_err());
    assert!(store
        .acknowledge_task_attempt(&plan_id, &task_id, "tab-worker", &retired_attempt_id, 14)
        .is_err());
    assert!(!store.worker_startup_attempt_owns(
        &plan_id,
        &task_id,
        "tab-worker",
        &retired_attempt_id,
        None,
    ));

    let after = store.show_task(&task_id).unwrap().task;
    let retired = after
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == retired_attempt_id)
        .unwrap();
    assert!(retired.retired_at.is_some());
    assert_eq!(retired.session_id.as_deref(), Some("session-tab-worker"));
    let live = after
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == live_attempt_id)
        .unwrap();
    assert_eq!(live.acknowledged_at, None);
    assert_eq!(live.session_id, None);
}

#[test]
fn acknowledge_failure_names_the_paused_task_not_the_tab() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task_id, _) = dispatched_worker_task(&store, &project, "paused", "tab-worker", 1);
    let mut paused_plan = store.get(&plan_id).unwrap();
    let paused = paused_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    paused.status = TaskStatus::Paused;
    paused.status_label = Some("operator pause".into());
    store.save(&mut paused_plan, 10).unwrap();

    let error = store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 11)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains(crate::service::plan::ACK_CAUSE_TASK_NOT_OPEN),
        "{error}"
    );
    assert!(
        !error.contains(crate::service::plan::ACK_CAUSE_NO_LIVE_ATTEMPT),
        "{error}"
    );
    assert!(error.contains(task_id.as_str()), "{error}");
    assert_eq!(
        store.show_task(&task_id).unwrap().task.execution_attempts[0].acknowledged_at,
        None
    );
}

#[test]
fn atomic_dispatch_carries_assigned_authority_into_pty_launch() {
    let lane = TaskLaneMetadata {
        base: Some("assigned-base".into()),
        ..Default::default()
    };
    let env = task_launch_authority_for_dispatch(
        "assigned-plan",
        "assigned-task",
        "attempt-assigned",
        &lane,
    )
    .env();

    assert_eq!(
        env.get("CODETERM_PLAN_ID").map(String::as_str),
        Some("assigned-plan")
    );
    assert_eq!(
        env.get("CODETERM_TASK_ID").map(String::as_str),
        Some("assigned-task")
    );
    assert_eq!(
        env.get("CODETERM_ATTEMPT_ID").map(String::as_str),
        Some("attempt-assigned")
    );
    assert_eq!(
        env.get("CODETERM_LANE_BASE").map(String::as_str),
        Some("assigned-base")
    );
}

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
    assert_eq!(
        event["deadline_seconds"],
        event["deadline_basis"]["selected_phase_seconds"]
            .as_u64()
            .unwrap()
            * 2
    );
    assert_eq!(event["expected_command"], "codeterm agent ack");
    assert_eq!(
        event["tab_state"],
        crate::service::worker_ack::TAB_STATE_ALIVE_AWAITING_INSPECTION
    );
    assert_eq!(
        event["attempt_state"],
        crate::service::worker_ack::ATTEMPT_STATE_FINALIZED
    );
    assert_eq!(
        event["recovery"]["capability"],
        crate::prompts::INTERACTIVE_CHILD_PTY_CAPABILITY_ID
    );
    assert_eq!(event["recovery"]["target_tab_id"], "tab");
    assert_eq!(event["recovery"]["attempt_id"], "attempt");
    assert!(event["recovery"]["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .any(|primitive| primitive.as_str() == Some("codeterm tab get-text --tab tab")));
    assert!(event.get("cleanup").is_none());
    assert!(event.get("cleanup_error").is_none());
}

#[test]
fn worker_ack_transitions_exact_pending_task_to_in_progress() {
    let (root, project) = git_project();
    let resolved = resolve_dispatch_lane(
        None,
        Some(project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
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
    let lane_metadata =
        lane_metadata_for_dispatch(&original.lane, &actual, project.to_str().unwrap());
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
            "nonce-worker",
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
fn plan_store_acknowledges_live_attempt_without_saved_session_binding() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Unbound attempt".into(),
            project,
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
                name: "worker without saved binding".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let lane = store.show_task(&task_id).unwrap().task.lane;
    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, "tab-worker", "Worker", lane, 4)
        .unwrap();
    let pending = store.show_task(&task_id).unwrap().task;
    assert_eq!(pending.execution_attempts[0].id, attempt_id);
    assert_eq!(pending.execution_attempts[0].session_id, None);
    let acknowledged = store
        .acknowledge_live_task_attempt_for_tab(
            "tab-worker",
            Some(&plan.id),
            Some(&task_id),
            5,
        )
        .expect("a live attempt can be acknowledged before a session id is saved");

    assert_eq!(acknowledged.attempt.acknowledged_at, Some(5));
    assert_eq!(acknowledged.attempt.session_id, None);
    let persisted = store.show_task(&task_id).unwrap().task;
    assert_eq!(persisted.status, TaskStatus::InProgress);
    assert_eq!(persisted.execution_attempts[0].acknowledged_at, Some(5));
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
    assert!(!ack.already_acknowledged);
    assert_eq!(ack.attempt.acknowledged_at, Some(10));
    let acknowledged = store.show_task(&task_id).unwrap().task;
    assert_eq!(acknowledged.status, TaskStatus::InProgress);
    assert_eq!(acknowledged.status_label.as_deref(), Some("acknowledged"));
    let first_changed_at = acknowledged.status_changed_at;
    let first_task_updated_at = acknowledged.updated_at;
    let first_plan_updated_at = store.get(&plan.id).unwrap().updated_at;

    let replay = store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 11)
        .unwrap();
    assert!(replay.already_acknowledged);
    assert_eq!(replay.attempt.acknowledged_at, Some(10));
    let after_replay = store.show_task(&task_id).unwrap().task;
    assert_eq!(after_replay.status_changed_at, first_changed_at);
    assert_eq!(after_replay.status_label.as_deref(), Some("acknowledged"));
    assert_eq!(after_replay.updated_at, first_task_updated_at);
    assert_eq!(after_replay.assigned_tab_id.as_deref(), Some("tab-worker"));
    assert_eq!(after_replay.lane.tab_id.as_deref(), Some("tab-worker"));
    assert_eq!(after_replay.execution_attempts.len(), 1);
    assert_eq!(store.get(&plan.id).unwrap().updated_at, first_plan_updated_at);

    let third_ack = store
        .acknowledge_live_task_attempt_for_tab("tab-worker", None, None, 12)
        .unwrap();
    assert!(third_ack.already_acknowledged);
    assert_eq!(third_ack.attempt.id, attempt_id);
    assert_eq!(third_ack.attempt.acknowledged_at, Some(10));
    assert_eq!(
        store.show_task(&task_id).unwrap().task.updated_at,
        first_task_updated_at
    );
    assert_eq!(
        store.get(&plan.id).unwrap().updated_at,
        first_plan_updated_at
    );
    assert!(store
        .acknowledge_task_attempt(&plan.id, &task_id, "stale-tab", &attempt_id, 13)
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
            "nonce-worker",
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
fn worker_ack_final_deadline_finalizes_attempt_for_redispatch() {
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
        .mark_worker_startup_deadline(&plan.id, &task_id, "tab-worker", &attempt_id, false, 6,)
        .unwrap());
    assert_eq!(
        store
            .show_task(&task_id)
            .unwrap()
            .task
            .status_label
            .as_deref(),
        Some("startup_unbound")
    );
    assert!(store
        .mark_worker_startup_deadline(&plan.id, &task_id, "tab-worker", &attempt_id, true, 7,)
        .unwrap());
    assert!(!store
        .mark_worker_startup_deadline(&plan.id, &task_id, "tab-worker", &attempt_id, true, 8,)
        .unwrap());
    let failed = store.show_task(&task_id).unwrap().task;
    assert_eq!(failed.status_label.as_deref(), Some("startup_failed"));
    assert_eq!(failed.assigned_tab_id, None);
    assert_eq!(failed.lane.tab_id, None);
    assert_eq!(failed.execution_attempts[0].closed_at, None);
    assert_eq!(failed.execution_attempts[1].closed_at, Some(7));
    let event = worker_ack_timeout_event(
        &plan.id,
        &task_id,
        "tab-worker",
        &attempt_id,
        "dispatch command",
    );
    assert_eq!(
        event["tab_state"],
        crate::service::worker_ack::TAB_STATE_ALIVE_AWAITING_INSPECTION
    );
    assert_eq!(
        event["attempt_state"],
        crate::service::worker_ack::ATTEMPT_STATE_FINALIZED
    );
    let replacement_attempt = store
        .begin_task_dispatch_with_observation(
            &plan.id,
            &task_id,
            "tab-worker",
            "Ada",
            failed.lane.clone(),
            store.dispatch_startup_owner_candidate(&plan.id, &task_id),
            false,
            9,
        )
        .unwrap();
    assert_ne!(replacement_attempt, attempt_id);
    let redispatched = store.show_task(&task_id).unwrap().task;
    assert_eq!(redispatched.assigned_tab_id.as_deref(), Some("tab-worker"));
    assert_eq!(
        redispatched.execution_attempts.last().unwrap().closed_at,
        None
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
        .mark_worker_startup_deadline(&plan.id, &task_id, "tab-worker", &attempt_id, false, 6,)
        .unwrap());
    assert!(!store
        .mark_worker_startup_deadline(&plan.id, &task_id, "tab-worker", &attempt_id, true, 7,)
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
    assert_eq!(
        store.show_task(&task_id).unwrap().task.assigned_tab_id,
        None
    );

    store
        .acknowledge_task_attempt(&plan.id, &task_id, &attempts[0].0, &attempts[0].1, 20)
        .unwrap();
    store
        .acknowledge_task_attempt(&plan.id, &task_id, &attempts[1].0, &attempts[1].1, 21)
        .unwrap();
    // Progression is authorized by the exact live attempt, so the competitive
    // task still carries no single task-level executor tab.
    let progressed = store.show_task(&task_id).unwrap().task;
    assert_eq!(progressed.status, TaskStatus::InProgress);
    assert_eq!(progressed.assigned_tab_id, None);
    assert_eq!(progressed.lane.tab_id, None);
    assert_eq!(progressed.execution_attempts[0].acknowledged_at, Some(20));
    assert_eq!(progressed.execution_attempts[1].acknowledged_at, Some(21));
    assert!(store
        .acknowledge_task_attempt(&plan.id, &task_id, &attempts[2].0, "attempt-absent", 22)
        .is_err());
    assert!(store
        .acknowledge_task_attempt(&plan.id, &task_id, "foreign-tab", &attempts[2].1, 22)
        .is_err());
    assert_eq!(
        store.show_task(&task_id).unwrap().task.execution_attempts[2].acknowledged_at,
        None
    );
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
        .mark_worker_startup_deadline(&plan.id, &task_id, &attempts[2].0, &attempts[2].1, true, 23,)
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
        .mark_worker_startup_deadline(&plan.id, &task_id, &attempts[2].0, &attempts[2].1, true, 25,)
        .unwrap());
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label.as_deref(), Some("competitive"));
    assert_eq!(task.execution_attempts[0].closed_at, None);
    assert_eq!(task.execution_attempts[1].closed_at, None);
    assert_eq!(task.execution_attempts[2].closed_at, Some(25));
}
