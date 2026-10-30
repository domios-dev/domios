use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, TaskStatus};
use crate::service::plan::{NewTask, TaskLaneUpdate};
use crate::service::test_fixture::ServiceFixture;

fn close_case(cancelled: bool, unmerged: bool, dirty: bool) {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let canonical = root_path.join("project");
    std::fs::create_dir(&canonical).unwrap();
    git(&canonical, &["init", "-q", "-b", "main"]);
    git(&canonical, &["config", "user.name", "Test"]);
    git(
        &canonical,
        &["config", "user.email", "test@example.invalid"],
    );
    std::fs::write(canonical.join(".gitignore"), "target/\nnode_modules/\n").unwrap();
    git(&canonical, &["add", ".gitignore"]);
    git(&canonical, &["commit", "-qm", "fixture"]);
    let lane = root_path.join("project-worktrees/worker");
    git(
        &canonical,
        &[
            "worktree",
            "add",
            "-b",
            "lane/worker",
            lane.to_str().unwrap(),
        ],
    );
    if unmerged {
        git(&lane, &["commit", "--allow-empty", "-qm", "retained work"]);
    }
    if dirty {
        std::fs::write(lane.join("owned.txt"), "uncommitted source").unwrap();
    }
    std::fs::create_dir_all(lane.join("target/debug")).unwrap();
    std::fs::write(lane.join("target/debug/artifact"), "large output").unwrap();

    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&worker).unwrap().cwd = Some(lane.display().to_string());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .extensions
        .get_or_init(|| crate::service::lane_git_queue::LaneGitQueue::new(|job| job()));
    let store = &fixture.service.plan_store;
    let plan = store
        .create_plan_for_team(
            "Cleanup".into(),
            canonical.display().to_string(),
            OrchestratorRef::from_tab(&owner),
            "team-fixture".into(),
            vec![],
            1,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Worker lane".into(),
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
                tab_id: Some(Some(worker.clone())),
                worktree_path: Some(Some(lane.display().to_string())),
                branch: Some(Some("lane/worker".into())),
                destination: Some(Some("main".into())),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 4, |plan| {
            plan.tasks
                .iter_mut()
                .find(|item| item.id == task)
                .unwrap()
                .status = if cancelled {
                TaskStatus::Cancelled
            } else {
                TaskStatus::Done
            };
            Ok(())
        })
        .unwrap();

    fixture.service.settle_lane(&plan.id, &task);
    assert!(lane.exists(), "live Worker keeps the accepted lane");
    assert_eq!(
        store
            .show_task(&task)
            .unwrap()
            .task
            .lane
            .cleanup_status
            .as_deref(),
        Some(CLEANUP_HELD)
    );
    fixture.service.close_tab(&worker).unwrap();
    let task = store.show_task(&task).unwrap().task;
    let removed = !dirty && (cancelled || !unmerged);
    assert_eq!(!lane.exists(), removed);
    if removed {
        assert!(task.lane.worktree_path.is_none());
    }
    if dirty || (unmerged && !cancelled) {
        assert_eq!(task.lane.cleanup_status.as_deref(), Some(CLEANUP_RETAINED));
        assert!(task
            .lane
            .cleanup_hints
            .iter()
            .any(|hint| hint.contains("domios plan lane-remove")));
    }
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let branch = command
        .current_dir(&canonical)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/lane/worker"])
        .status()
        .unwrap()
        .success();
    assert_eq!(branch, dirty || unmerged);
    if dirty {
        assert_eq!(
            std::fs::read_to_string(lane.join("owned.txt")).unwrap(),
            "uncommitted source"
        );
    }
}

#[test]
fn accepted_merged_lane_is_removed_completely_after_worker_closes() {
    close_case(false, false, false);
}

#[test]
fn accepted_unmerged_lane_stays_after_worker_closes() {
    close_case(false, true, false);
}

#[test]
fn accepted_dirty_lane_stays_after_worker_closes() {
    close_case(false, false, true);
}

#[test]
fn cancelled_unmerged_lane_loses_only_its_clean_worktree() {
    close_case(true, true, false);
}

#[test]
fn cancelled_dirty_lane_stays_after_worker_closes() {
    close_case(true, true, true);
}
