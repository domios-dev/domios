use super::*;

const STALE_TAB: &str = "tab-attempt-a";
const LIVE_TAB: &str = "tab-attempt-b";

fn approved_task(store: &PlanStore, project: &Path) -> (String, Task) {
    let plan = store
        .create_plan_for_team(
            "Late attempt".into(),
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
                ..Default::default()
            },
            3,
        )
        .unwrap();
    (plan.id, store.show_task(&task_id).unwrap().task)
}

/// Drives the dispatch startup producer exactly as `DispatchStartup::run` does.
#[allow(clippy::too_many_arguments)]
fn run_stale_startup(
    store: &PlanStore,
    plan_id: &str,
    original: &Task,
    attempt_id: &str,
    lane: crate::service::ProvisionedWorkerLane,
    provenance: WorkerLaneProvenance,
    ready: Result<(), DispatchWaitFailure>,
    closed: &Cell<bool>,
) -> serde_json::Value {
    let project = provenance.canonical_cwd.clone();
    complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| ready,
        |_| Ok("session-a".into()),
        || Ok(Some(provenance.clone())),
        |lane, session| {
            store
                .finish_task_dispatch(
                    plan_id,
                    &original.id,
                    STALE_TAB,
                    attempt_id,
                    session,
                    "nonce-a",
                    lane_metadata_for_dispatch(
                        &original.lane,
                        lane.as_ref().expect("repository provenance"),
                        &project,
                    ),
                    20,
                )
                .map_err(|error| error.to_string())
        },
        || {
            roll_back_dispatch_attempt(
                store,
                plan_id,
                original,
                &PlanMode::Approved,
                STALE_TAB,
                attempt_id,
                Some(lane),
                || {
                    closed.set(true);
                    Ok(())
                },
                21,
            )
        },
    )
    .unwrap_err()
    .data
    .unwrap()
}

#[test]
fn late_attempt_failure_cannot_reset_a_task_owned_by_a_newer_attempt() {
    let (root, project) = git_project();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, original) = approved_task(&store, &project);
    let lane = provision_lane_for_test(project.to_str().unwrap(), "late-attempt").unwrap();
    let provenance = lane.verify().unwrap();
    let worktree_path = provenance.worktree_path.clone();
    let lane_metadata =
        lane_metadata_for_dispatch(&original.lane, &provenance, project.to_str().unwrap());
    let stale_attempt = store
        .begin_task_dispatch(
            &plan_id,
            &original.id,
            STALE_TAB,
            "Ada",
            lane_metadata.clone(),
            4,
        )
        .unwrap();
    store
        .retire_stale_task_owner(&plan_id, &original.id, None, |_| false, 5)
        .unwrap();
    let live_attempt = store
        .begin_task_dispatch(
            &plan_id,
            &original.id,
            LIVE_TAB,
            "Ada",
            lane_metadata.clone(),
            6,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan_id,
            &original.id,
            LIVE_TAB,
            &live_attempt,
            "session-b",
            "nonce-b",
            lane_metadata,
            7,
        )
        .unwrap();
    let before = store.show_task(&original.id).unwrap().task;
    let closed = Cell::new(false);

    let failure = run_stale_startup(
        &store,
        &plan_id,
        &original,
        &stale_attempt,
        lane,
        provenance,
        Ok(()),
        &closed,
    );

    assert_eq!(failure["stage"], "task_commit");
    assert!(closed.get(), "the stale attempt's own tab is still closed");
    assert!(
        Path::new(&worktree_path).exists(),
        "the worktree the newer attempt holds must survive"
    );
    let after = store.show_task(&original.id).unwrap().task;
    assert_eq!(after, before, "no status reset from the superseded attempt");
    assert_eq!(after.assigned_tab_id.as_deref(), Some(LIVE_TAB));
    assert_eq!(store.get(&plan_id).unwrap().mode, PlanMode::Execution);
    let live = after
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.closed_at.is_none())
        .map(|attempt| attempt.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(live, vec![live_attempt.as_str()]);
    let stale = after
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == stale_attempt)
        .expect("the stale attempt stays in history");
    assert!(stale.closed_at.is_some() && stale.retired_at.is_some());
}

#[test]
fn late_attempt_failure_of_the_current_attempt_still_rolls_back() {
    let (root, project) = git_project();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, original) = approved_task(&store, &project);
    let lane = provision_lane_for_test(project.to_str().unwrap(), "current-attempt").unwrap();
    let provenance = lane.verify().unwrap();
    let worktree_path = provenance.worktree_path.clone();
    let attempt = store
        .begin_task_dispatch(
            &plan_id,
            &original.id,
            STALE_TAB,
            "Ada",
            lane_metadata_for_dispatch(&original.lane, &provenance, project.to_str().unwrap()),
            4,
        )
        .unwrap();
    let closed = Cell::new(false);

    let failure = run_stale_startup(
        &store,
        &plan_id,
        &original,
        &attempt,
        lane,
        provenance,
        Err(DispatchWaitFailure::Definitive(
            "provider rejected launch".into(),
        )),
        &closed,
    );

    assert_eq!(failure["stage"], "agent_ready");
    assert!(closed.get());
    assert!(!Path::new(&worktree_path).exists());
    assert_eq!(store.show_task(&original.id).unwrap().task, original);
    assert_eq!(store.get(&plan_id).unwrap().mode, PlanMode::Approved);
}
