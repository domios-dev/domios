use std::sync::{mpsc, Arc};
use std::time::Duration;

use codeterm_service_core::lock_hold::TimedRwLock;
use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::multiplexer::state::MultiplexerState;

use super::workspace_cwds;

#[test]
fn workspace_snapshot_releases_mux_before_plan_loading() {
    let (mut state, _) = MultiplexerState::new();
    state.create_workspace("Restored".into(), Some("opaque-project".into()));
    let mux: MuxState = Arc::new(TimedRwLock::new("startup-plan-restore-test", state));

    assert_eq!(workspace_cwds(&mux), vec!["opaque-project"]);

    let writer_mux = mux.clone();
    let (writer_acquired_tx, writer_acquired_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let _guard = writer_mux.write();
        writer_acquired_tx.send(()).unwrap();
    });

    writer_acquired_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("workspace snapshot kept the mux read guard alive");
    writer.join().unwrap();
}

fn archived_plan_with_finished_lane(
    store: &codeterm_plan::service_plan::PlanStore,
    owner: &str,
    project: &std::path::Path,
    cleanup_status: Option<&str>,
) -> (String, String) {
    use codeterm_service_core::plan::{OrchestratorRef, PlanStatus, TaskStatus};
    use codeterm_service_core::service::plan::{NewTask, TaskLaneUpdate};

    let plan = store
        .create_plan_for_team(
            format!("Finished lane {cleanup_status:?}"),
            project.display().to_string(),
            OrchestratorRef::from_tab(owner),
            format!("team-{}", uuid::Uuid::new_v4().simple()),
            vec![],
            1,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Lane".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .update_task_lane(
            &plan.id,
            &task,
            TaskLaneUpdate {
                worktree_path: Some(Some(project.join("gone-lane").display().to_string())),
                branch: Some(Some("lane/gone".into())),
                cleanup_status: Some(cleanup_status.map(str::to_string)),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 4, |plan| {
            plan.status = PlanStatus::Archived;
            plan.team_id = None;
            plan.tasks[0].status = TaskStatus::Done;
            Ok(())
        })
        .unwrap();
    (plan.id, task)
}

#[test]
fn startup_perf_restore_plans_without_cloning_the_plan_store() {
    use codeterm_service_core::plan::{OrchestratorRef, PlanStatus};
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let (mut state, owner) = MultiplexerState::new();
    let projects: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    for (index, project) in projects.iter().enumerate() {
        state.create_workspace(
            format!("Project {index}"),
            Some(project.path().display().to_string()),
        );
    }
    let fixture = ServiceFixture::new(state);
    let store = &fixture.service.plan_store;
    let dangling = store
        .create_plan_for_team(
            "Dangling".into(),
            projects[0].path().display().to_string(),
            OrchestratorRef::from_tab(&owner),
            "closed-team".into(),
            vec![],
            1,
        )
        .unwrap();
    let (unsettled, unsettled_task) =
        archived_plan_with_finished_lane(store, &owner, projects[1].path(), None);
    let (settled, _) =
        archived_plan_with_finished_lane(store, &owner, projects[2].path(), Some("removed"));
    let clones_before = store.full_plan_clones();
    store.take_plan_touches();

    super::restore(&fixture.service);

    let touched = store.take_plan_touches();
    assert!(
        !touched.contains(&settled),
        "startup opened a plan no sweep selects: {touched:?}"
    );
    assert_eq!(
        store.full_plan_clones() - clones_before,
        0,
        "startup plan restore cloned the whole plan store"
    );
    assert_eq!(
        store.get(&dangling.id).unwrap().status,
        PlanStatus::Archived,
        "a plan bound to a closed Team is still archived at startup"
    );
    let lane = store
        .get(&unsettled)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == unsettled_task)
        .unwrap()
        .lane;
    assert!(
        lane.cleanup_status.is_some(),
        "a finished lane is still settled at startup"
    );
    assert!(fixture.service.plans_startup_loaded());
}
