use crate::plan::{ExecutionIntent, OrchestratorRef, Plan, PlanMode, TaskLaneMetadata, TaskStatus};
use crate::service_plan::{NewTask, PlanStore, TaskLaneUpdate};
use std::sync::{Arc, Barrier};

fn setup() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "L".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_lane".into();
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "t".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, plan.id, task)
}

#[test]
fn fills_empty_tab_branch_and_mirrors_assigned() {
    let (_d, store, plan, task) = setup();
    store
        .fill_task_lane_if_empty(&plan, &task, Some("tab-1"), Some("lane/x"), None, 5)
        .unwrap();
    let p = store.get(&plan).unwrap();
    let t = p.tasks.iter().find(|t| t.id == task).unwrap();
    assert_eq!(t.lane.tab_id.as_deref(), Some("tab-1"));
    assert_eq!(t.assigned_tab_id.as_deref(), Some("tab-1"));
    assert_eq!(t.lane.branch.as_deref(), Some("lane/x"));
}

#[test]
fn never_overwrites_existing_values() {
    let (_d, store, plan, task) = setup();
    store
        .fill_task_lane_if_empty(&plan, &task, Some("first"), Some("branch-a"), None, 5)
        .unwrap();
    store
        .fill_task_lane_if_empty(&plan, &task, Some("second"), Some("branch-b"), None, 6)
        .unwrap();
    let p = store.get(&plan).unwrap();
    let t = p.tasks.iter().find(|t| t.id == task).unwrap();
    assert_eq!(t.assigned_tab_id.as_deref(), Some("first"));
    assert_eq!(t.lane.branch.as_deref(), Some("branch-a"));
}

#[test]
fn revert_dispatch_clears_tab_and_returns_pending() {
    let (_d, store, plan, task) = setup();
    store
        .assign_task_with_role(&plan, &task, "worker-tab", None, Some("worker"), 5)
        .unwrap();
    store
        .update_task(
            &plan,
            &task,
            None,
            None,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            6,
        )
        .unwrap();
    store.revert_task_dispatch(&plan, &task, 7).unwrap();
    let p = store.get(&plan).unwrap();
    let t = p.tasks.iter().find(|t| t.id == task).unwrap();
    assert!(t.assigned_tab_id.is_none());
    assert!(t.lane.tab_id.is_none());
    assert_eq!(t.status, crate::plan::TaskStatus::Pending);
}

#[test]
fn blank_args_are_noop() {
    let (_d, store, plan, task) = setup();
    store
        .fill_task_lane_if_empty(&plan, &task, Some("  "), Some(""), None, 5)
        .unwrap();
    let p = store.get(&plan).unwrap();
    let t = p.tasks.iter().find(|t| t.id == task).unwrap();
    assert!(t.assigned_tab_id.is_none());
    assert!(t.lane.branch.is_none());
}

#[test]
fn explicit_tab_update_overwrites_assigned_tab() {
    // #209 regression: an explicit lane tab update must rebind the executor,
    // not silently no-op because assigned_tab_id was already set.
    let (_d, store, plan, task) = setup();
    store
        .fill_task_lane_if_empty(&plan, &task, Some("old-tab"), None, None, 5)
        .unwrap();
    let update = crate::service_plan::TaskLaneUpdate {
        tab_id: Some(Some("new-tab".into())),
        ..Default::default()
    };
    store.update_task_lane(&plan, &task, update, 6).unwrap();
    let p = store.get(&plan).unwrap();
    let t = p.tasks.iter().find(|t| t.id == task).unwrap();
    assert_eq!(t.lane.tab_id.as_deref(), Some("new-tab"));
    assert_eq!(t.assigned_tab_id.as_deref(), Some("new-tab"));
}

#[test]
fn implicit_lane_update_still_fills_only_empty() {
    let (_d, store, plan, task) = setup();
    store
        .fill_task_lane_if_empty(&plan, &task, Some("keeper"), None, None, 5)
        .unwrap();
    let update = crate::service_plan::TaskLaneUpdate {
        branch: Some(Some("lane/x".into())),
        ..Default::default()
    };
    store.update_task_lane(&plan, &task, update, 6).unwrap();
    let p = store.get(&plan).unwrap();
    let t = p.tasks.iter().find(|t| t.id == task).unwrap();
    assert_eq!(t.assigned_tab_id.as_deref(), Some("keeper"));
    assert_eq!(t.lane.branch.as_deref(), Some("lane/x"));
}

#[test]
fn lane_metadata_update_never_changes_assignment_or_closes_attempts() {
    let (_dir, store, plan, task) = setup();
    store
        .with_plan(&plan, 3, |plan| {
            let task_ref = plan
                .tasks
                .iter_mut()
                .find(|candidate| candidate.id == task)
                .unwrap();
            task_ref.lane.tab_id = Some("stale-lane-tab".into());
            Ok(())
        })
        .unwrap();
    update_task_with_lane(
        &store,
        &plan,
        &task,
        TaskLaneUpdate {
            destination: Some(Some("release/pre-dispatch".into())),
            ..Default::default()
        },
        3,
    );
    assert!(store
        .show_task(&task)
        .unwrap()
        .task
        .assigned_tab_id
        .is_none());

    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan,
            &task,
            "worker-tab",
            "Worker",
            TaskLaneMetadata::default(),
            ExecutionIntent::ReadOnly,
            None,
            Some("codex"),
            3,
        )
        .unwrap();
    assert_eq!(
        store
            .show_task(&task)
            .unwrap()
            .task
            .lane
            .destination
            .as_deref(),
        Some("release/pre-dispatch")
    );
    let startup_metadata = TaskLaneUpdate {
        destination: Some(Some("release/startup".into())),
        cleanup_status: Some(Some("pending".into())),
        cleanup_hints: Some(vec!["retain lane".into()]),
        ..Default::default()
    };

    update_task_with_lane(&store, &plan, &task, startup_metadata, 4);
    let starting = store.show_task(&task).unwrap().task;
    assert_eq!(starting.assigned_tab_id.as_deref(), Some("worker-tab"));
    assert_eq!(starting.lane.tab_id.as_deref(), Some("worker-tab"));
    assert_eq!(starting.execution_attempts.len(), 1);
    assert_eq!(starting.execution_attempts[0].id, attempt_id);
    assert_eq!(starting.execution_attempts[0].closed_at, None);
    assert_eq!(starting.execution_attempts[0].acknowledged_at, None);
    assert_eq!(
        starting.lane.destination.as_deref(),
        Some("release/startup")
    );
    assert_eq!(starting.lane.cleanup_status.as_deref(), Some("pending"));
    assert_eq!(starting.lane.cleanup_hints, vec!["retain lane".to_string()]);

    store
        .finish_task_dispatch(
            &plan,
            &task,
            "worker-tab",
            &attempt_id,
            "session",
            "nonce",
            TaskLaneMetadata::default(),
            5,
        )
        .unwrap();
    store
        .acknowledge_task_attempt(&plan, &task, "worker-tab", &attempt_id, 6)
        .unwrap();
    store
        .update_task_lane(
            &plan,
            &task,
            TaskLaneUpdate {
                destination: Some(Some("release/acknowledged".into())),
                cleanup_status: Some(Some("complete".into())),
                cleanup_hints: Some(vec!["preserve acknowledged lane".into()]),
                ..Default::default()
            },
            7,
        )
        .unwrap();

    let acknowledged = store.show_task(&task).unwrap().task;
    assert_eq!(acknowledged.assigned_tab_id.as_deref(), Some("worker-tab"));
    assert_eq!(acknowledged.lane.tab_id.as_deref(), Some("worker-tab"));
    assert_eq!(acknowledged.execution_attempts.len(), 1);
    assert_eq!(acknowledged.execution_attempts[0].closed_at, None);
    assert_eq!(acknowledged.execution_attempts[0].acknowledged_at, Some(6));
    assert_eq!(
        acknowledged.lane.destination.as_deref(),
        Some("release/acknowledged")
    );
    assert_eq!(
        acknowledged.lane.cleanup_status.as_deref(),
        Some("complete")
    );
    assert_eq!(
        acknowledged.lane.cleanup_hints,
        vec!["preserve acknowledged lane".to_string()]
    );
}

#[test]
fn dispatch_records_computed_destination_when_none_is_stored() {
    let (_dir, store, plan, task) = setup();
    let computed = TaskLaneMetadata {
        destination: Some("release/computed".into()),
        ..Default::default()
    };
    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan,
            &task,
            "worker-tab",
            "Worker",
            computed.clone(),
            ExecutionIntent::ReadOnly,
            None,
            Some("codex"),
            3,
        )
        .unwrap();
    let starting = store.show_task(&task).unwrap().task;
    assert_eq!(starting.lane.destination.as_deref(), Some("release/computed"));
    store
        .finish_task_dispatch(
            &plan,
            &task,
            "worker-tab",
            &attempt_id,
            "session",
            "nonce",
            TaskLaneMetadata {
                destination: Some("release/other".into()),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let ready = store.show_task(&task).unwrap().task;
    assert_eq!(ready.lane.destination.as_deref(), Some("release/computed"));
    assert_eq!(
        ready.execution_attempts[0].destination.as_deref(),
        Some("release/computed")
    );
}

fn update_task_with_lane(
    store: &PlanStore,
    plan: &str,
    task: &str,
    lane: TaskLaneUpdate,
    now_ms: u64,
) {
    store
        .update_task_with_lane(
            plan,
            task,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            Some(lane),
            now_ms,
        )
        .unwrap();
}

fn set_status(
    store: &PlanStore,
    plan: &str,
    task: &str,
    status: &str,
    label: Option<Option<&str>>,
    ts: u64,
) -> Result<(), crate::service_plan::PlanError> {
    store.update_task(
        plan,
        task,
        None,
        None,
        None,
        None,
        Some(status),
        label,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        None,
        None,
        ts,
    )
}

#[test]
fn in_progress_requires_tab_invariant() {
    let (_d, store, plan, task) = setup();
    let err = set_status(&store, &plan, &task, "in_progress", None, 5).unwrap_err();
    assert!(matches!(
        err,
        crate::service_plan::PlanError::InProgressRequiresTab
    ));
    store
        .fill_task_lane_if_empty(&plan, &task, Some("tab-1"), None, None, 6)
        .unwrap();
    set_status(&store, &plan, &task, "in_progress", None, 7).unwrap();
}

#[test]
fn lane_and_in_progress_commit_as_one_plan_mutation() {
    let (_d, store, plan, task) = setup();
    let error = set_status(&store, &plan, &task, "in_progress", None, 5).unwrap_err();
    assert!(matches!(
        error,
        crate::service_plan::PlanError::InProgressRequiresTab
    ));

    store
        .update_task_with_lane(
            &plan,
            &task,
            None,
            None,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            Some(TaskLaneUpdate {
                tab_id: Some(Some("worker-tab".into())),
                ..Default::default()
            }),
            6,
        )
        .unwrap();

    let task = store.show_task(&task).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.lane.tab_id.as_deref(), Some("worker-tab"));
    assert_eq!(task.assigned_tab_id.as_deref(), Some("worker-tab"));
}

#[test]
fn rejected_status_does_not_leave_a_half_applied_lane() {
    let (_d, store, plan, task) = setup();
    let error = store
        .update_task_with_lane(
            &plan,
            &task,
            None,
            None,
            None,
            None,
            Some("blocked"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            Some(TaskLaneUpdate {
                tab_id: Some(Some("must-not-commit".into())),
                ..Default::default()
            }),
            6,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        crate::service_plan::PlanError::BlockedRequiresLabel
    ));
    let task = store.show_task(&task).unwrap().task;
    assert!(task.lane.tab_id.is_none());
    assert!(task.assigned_tab_id.is_none());
    assert_eq!(task.status, TaskStatus::Pending);
}

#[test]
fn late_validation_failure_rolls_back_lane_and_status_together() {
    let (_d, store, plan, task) = setup();
    let error = store
        .update_task_with_lane(
            &plan,
            &task,
            None,
            None,
            None,
            None,
            Some("in_progress"),
            None,
            Some("not-a-priority"),
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            Some(TaskLaneUpdate {
                tab_id: Some(Some("must-roll-back".into())),
                ..Default::default()
            }),
            6,
        )
        .unwrap_err();
    assert!(matches!(error, crate::service_plan::PlanError::BadPriority));
    let task = store.show_task(&task).unwrap().task;
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.lane.tab_id.is_none());
    assert!(task.assigned_tab_id.is_none());
}

#[test]
fn concurrent_same_plan_task_mutations_keep_exact_status_buckets() {
    let (_d, store, plan, first_task) = setup();
    let mut tasks = vec![first_task];
    for index in 1..8 {
        tasks.push(
            store
                .add_task(
                    &plan,
                    NewTask {
                        name: format!("task-{index}"),
                        acceptance_criteria: vec!["observable result".into()],
                        test_plan: Some("cargo test exact".into()),
                        ..Default::default()
                    },
                    3 + index,
                )
                .unwrap(),
        );
    }
    let store = Arc::new(store);
    let start = Arc::new(Barrier::new(tasks.len()));
    let statuses = [
        ("pending", None),
        ("pending", None),
        ("pending", None),
        ("in_review", None),
        ("blocked", Some("waiting on dependency")),
        ("done", None),
        ("done", None),
        ("done", None),
    ];
    let mut workers = Vec::new();
    for (index, (task, (status, label))) in tasks.into_iter().zip(statuses).enumerate() {
        let store = Arc::clone(&store);
        let plan = plan.clone();
        let start = Arc::clone(&start);
        workers.push(std::thread::spawn(move || {
            start.wait();
            store.update_task(
                &plan,
                &task,
                None,
                None,
                None,
                None,
                Some(status),
                label.map(Some),
                None,
                Some(&format!("concurrent-mutation-{index}")),
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                100 + index as u64,
            )
        }));
    }
    for worker in workers {
        worker.join().unwrap().unwrap();
    }

    let plan = store.get(&plan).unwrap();
    let counts = plan.tasks.iter().fold([0usize; 4], |mut counts, task| {
        match task.status {
            TaskStatus::Pending => counts[0] += 1,
            TaskStatus::InReview => counts[1] += 1,
            TaskStatus::Blocked => counts[2] += 1,
            TaskStatus::Done => counts[3] += 1,
            _ => panic!("unexpected status: {:?}", task.status),
        }
        counts
    });
    assert_eq!(counts, [3, 1, 1, 3]);
}

#[test]
fn blocked_requires_label_invariant() {
    let (_d, store, plan, task) = setup();
    let err = set_status(&store, &plan, &task, "blocked", None, 5).unwrap_err();
    assert!(matches!(
        err,
        crate::service_plan::PlanError::BlockedRequiresLabel
    ));
    set_status(
        &store,
        &plan,
        &task,
        "blocked",
        Some(Some("waiting on QA")),
        6,
    )
    .unwrap();
}

#[test]
fn explicit_dependencies_roundtrip_without_parking() {
    let (_d, store, plan, dependency) = setup();
    let child = store
        .add_task(
            &plan,
            crate::service_plan::NewTask {
                name: "child".into(),
                depends_on: vec![dependency.clone()],
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let p = store.get(&plan).unwrap();
    let child = p.tasks.iter().find(|t| t.id == child).unwrap();
    assert_eq!(child.depends_on, vec![dependency]);
    assert_eq!(child.status, crate::plan::TaskStatus::Pending);
}

#[test]
fn update_dependencies_sets_and_clears_roundtrip() {
    let (_d, store, plan, dependency) = setup();
    let child = store
        .add_task(
            &plan,
            crate::service_plan::NewTask {
                name: "child".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .update_task_dependencies(&plan, &child, vec![dependency.clone()], 4)
        .unwrap();
    let p = store.get(&plan).unwrap();
    assert_eq!(
        p.tasks.iter().find(|t| t.id == child).unwrap().depends_on,
        vec![dependency.clone()]
    );
    store
        .update_task_dependencies(&plan, &child, vec![], 5)
        .unwrap();
    let p = store.get(&plan).unwrap();
    assert!(p
        .tasks
        .iter()
        .find(|t| t.id == child)
        .unwrap()
        .depends_on
        .is_empty());
}

#[test]
fn invalid_dependency_ids_self_and_cycles_are_rejected() {
    let (_d, store, plan, parent) = setup();
    let child = store
        .add_task(
            &plan,
            crate::service_plan::NewTask {
                name: "child".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let err = store
        .update_task_dependencies(&plan, &child, vec!["missing".into()], 4)
        .unwrap_err();
    assert!(matches!(
        err,
        crate::service_plan::PlanError::DependencyTaskNotFound { .. }
    ));
    let err = store
        .update_task_dependencies(&plan, &child, vec![child.clone()], 5)
        .unwrap_err();
    assert!(matches!(
        err,
        crate::service_plan::PlanError::DependencySelfReference
    ));
    store
        .update_task_dependencies(&plan, &child, vec![parent.clone()], 6)
        .unwrap();
    let err = store
        .update_task_dependencies(&plan, &parent, vec![child.clone()], 7)
        .unwrap_err();
    assert!(matches!(
        err,
        crate::service_plan::PlanError::DependencyCycle
    ));
}

#[test]
fn depends_on_resolves_task_handle_to_canonical_id() {
    let (_d, store, plan, _parent) = setup();
    let handle_target = store
        .add_task(
            &plan,
            crate::service_plan::NewTask {
                name: "target".into(),
                handle: Some("target-handle".into()),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let child = store
        .add_task(
            &plan,
            crate::service_plan::NewTask {
                name: "child".into(),
                depends_on: vec!["target-handle".into()],
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let p = store.get(&plan).unwrap();
    let child_task = p.tasks.iter().find(|t| t.id == child).unwrap();
    assert_eq!(child_task.depends_on, vec![handle_target.clone()]);

    let other = store
        .add_task(
            &plan,
            crate::service_plan::NewTask {
                name: "other".into(),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    store
        .update_task_dependencies(&plan, &other, vec!["target-handle".into()], 6)
        .unwrap();
    let p = store.get(&plan).unwrap();
    assert_eq!(
        p.tasks.iter().find(|t| t.id == other).unwrap().depends_on,
        vec![handle_target]
    );
}
