use super::*;
use crate::service_plan::paths;
use crate::service_plan::task_queries::TaskListEmptyReason;
use codeterm_util::process_util;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_git(cwd: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    process_util::no_window_std(&mut cmd);
    let out = cmd.current_dir(cwd).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "test@example.com"]);
    run_git(root, &["config", "user.name", "Test User"]);
    std::fs::write(root.join("README.md"), "root\n").unwrap();
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
}

struct WorktreeFixture {
    tmp: tempfile::TempDir,
    store: PlanStore,
    main: PathBuf,
    linked: PathBuf,
}

/// The store root marker lives in `main` only, so `linked` is not a descendant
/// of any marked root and `project_root` must resolve it through real git.
fn worktree_fixture() -> WorktreeFixture {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    let linked = tmp.path().join("linked");
    init_repo(&main);
    run_git(
        &main,
        &[
            "worktree",
            "add",
            linked.to_str().unwrap(),
            "-b",
            "lane-test",
        ],
    );
    let store = PlanStore::new_for_test(&main);
    assert!(
        paths::test_project_root(&linked).is_none(),
        "linked worktree must not short-circuit project_root"
    );
    assert_ne!(
        linked.canonicalize().unwrap(),
        main.canonicalize().unwrap(),
        "a path-equality check must be able to fail here"
    );
    WorktreeFixture {
        tmp,
        store,
        main,
        linked,
    }
}

fn plan_owned_by_main(fixture: &WorktreeFixture, name: &str) -> (String, String) {
    let plan = fixture
        .store
        .create_plan(
            "Lane".into(),
            fixture.main.display().to_string(),
            OrchestratorRef::from_tab("owner"),
            vec![],
            1,
        )
        .unwrap();
    let task_id = fixture
        .store
        .add_task(
            &plan.id,
            NewTask {
                name: name.into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (plan.id, task_id)
}

#[test]
fn listing_from_a_linked_worktree_returns_the_main_worktree_plan_tasks() {
    let fixture = worktree_fixture();
    let (plan_id, task_id) = plan_owned_by_main(&fixture, "open");

    let listed = fixture.store.list_tasks_with_reason(TaskListFilter {
        project_path: Some(fixture.linked.display().to_string()),
        plan_id: Some(plan_id),
        active_only: true,
        ..Default::default()
    });

    assert_eq!(listed.tasks.len(), 1);
    assert_eq!(listed.tasks[0].task.id, task_id);
    assert_eq!(listed.empty_reason, None);
}

#[test]
fn a_filter_miss_from_a_linked_worktree_is_not_a_project_mismatch() {
    let fixture = worktree_fixture();
    let (plan_id, _) = plan_owned_by_main(&fixture, "open");
    let filter = TaskListFilter {
        project_path: Some(fixture.linked.display().to_string()),
        plan_id: Some(plan_id),
        status: Some(TaskStatus::Done),
        ..Default::default()
    };

    let unfiltered = fixture.store.list_tasks_with_reason(TaskListFilter {
        status: None,
        ..filter.clone()
    });
    let listed = fixture.store.list_tasks_with_reason(filter);

    assert_eq!(
        unfiltered.tasks.len(),
        1,
        "the status filter is the only cut"
    );
    assert!(listed.tasks.is_empty());
    assert_eq!(
        listed.empty_reason,
        Some(TaskListEmptyReason::NoMatchForFilters)
    );
}

#[test]
fn a_plan_in_another_project_reports_the_mismatch_reason() {
    let fixture = worktree_fixture();
    let other = fixture.tmp.path().join("other");
    init_repo(&other);
    let (plan_id, _) = plan_owned_by_main(&fixture, "open");
    let filter = TaskListFilter {
        plan_id: Some(plan_id.clone()),
        active_only: true,
        ..Default::default()
    };

    let from_own_project = fixture.store.list_tasks_with_reason(TaskListFilter {
        project_path: Some(fixture.main.display().to_string()),
        ..filter.clone()
    });
    let listed = fixture.store.list_tasks_with_reason(TaskListFilter {
        project_path: Some(other.display().to_string()),
        ..filter
    });

    assert_eq!(
        from_own_project.tasks.len(),
        1,
        "the project filter is the only cut"
    );
    assert_eq!(
        listed.empty_reason,
        Some(TaskListEmptyReason::PlanOutsideProject {
            plan_id,
            plan_project: paths::project_root(&fixture.main).display().to_string(),
            requested_project: paths::project_root(&other).display().to_string(),
        })
    );
}

#[test]
fn a_plan_whose_tasks_are_all_inactive_reports_the_all_flag_reason() {
    let (store, plan_id, task_id) = store_with_task(&["status_update"]);
    store
        .pause_task(
            &plan_id,
            &task_id,
            "operator hold",
            crate::plan::AgentRef::from_tab("orch"),
            3,
        )
        .unwrap();
    let filter = TaskListFilter {
        plan_id: Some(plan_id),
        active_only: true,
        ..Default::default()
    };

    let relaxed = store.list_tasks_with_reason(TaskListFilter {
        active_only: false,
        ..filter.clone()
    });
    let listed = store.list_tasks_with_reason(filter);

    assert_eq!(relaxed.tasks.len(), 1, "--all still reveals the task");
    assert_eq!(
        listed.empty_reason,
        Some(TaskListEmptyReason::AllTasksInactive { hidden: 1 })
    );
}

#[test]
fn an_archived_plan_reports_archived_not_a_missing_project() {
    let (store, plan_id, _) = store_with_task(&["status_update"]);
    store.archive(&plan_id, 99).unwrap();

    let listed = store.list_tasks_with_reason(TaskListFilter {
        plan_id: Some(plan_id.clone()),
        active_only: false,
        ..Default::default()
    });

    assert_eq!(
        listed.empty_reason,
        Some(TaskListEmptyReason::PlanArchived { plan_id })
    );
}

#[test]
fn a_plan_without_tasks_reports_an_empty_plan_not_a_filter_miss() {
    let (store, _, _) = store_with_task(&["status_update"]);
    let project = store.list_full().remove(0).project_path;
    let empty = store
        .create_plan(
            "Empty".into(),
            project,
            OrchestratorRef::from_tab("owner"),
            vec![],
            5,
        )
        .unwrap();

    let listed = store.list_tasks_with_reason(TaskListFilter {
        plan_id: Some(empty.id.clone()),
        active_only: true,
        ..Default::default()
    });

    assert_eq!(
        listed.empty_reason,
        Some(TaskListEmptyReason::NoTasksInScope {
            plan_id: Some(empty.id)
        })
    );
}

#[test]
fn a_project_holding_no_plans_reports_no_plans_in_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(tmp.path());
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    store
        .create_plan(
            "Here".into(),
            tmp.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
            vec![],
            1,
        )
        .unwrap();

    let listed = store.list_tasks_with_reason(TaskListFilter {
        project_path: Some(elsewhere.display().to_string()),
        active_only: true,
        ..Default::default()
    });

    assert_eq!(
        listed.empty_reason,
        Some(TaskListEmptyReason::NoPlansInScope {
            requested_project: Some(paths::project_root(&elsewhere).display().to_string())
        })
    );
}

#[test]
fn an_unknown_plan_id_reports_plan_not_found() {
    let (store, _, _) = store_with_task(&["status_update"]);

    let listed = store.list_tasks_with_reason(TaskListFilter {
        plan_id: Some("pl_absent".into()),
        active_only: true,
        ..Default::default()
    });

    assert_eq!(
        listed.empty_reason,
        Some(TaskListEmptyReason::PlanNotFound {
            plan_id: "pl_absent".into()
        })
    );
}

#[test]
fn task_queries_exclude_archived_plans() {
    let (store, id, _) = store_with_task(&["status_update"]);
    store.archive(&id, 99).unwrap();

    let tasks = store.list_tasks(TaskListFilter {
        active_only: false,
        ..Default::default()
    });

    assert!(tasks.is_empty());
    assert!(store.coverage_report(&id).is_none());
}

#[test]
fn task_queries_filter_by_status() {
    let (store, id, blocked_id) = store_with_task(&["status_update"]);
    store
        .add_task(
            &id,
            NewTask {
                name: "still-pending".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .update_task(
            &id,
            &blocked_id,
            None,
            None,
            None,
            None,
            Some("blocked"),
            Some(Some("waiting on dependency")),
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
            3,
        )
        .unwrap();

    let tasks = store.list_tasks(TaskListFilter {
        active_only: false,
        status: Some(TaskStatus::Blocked),
        ..Default::default()
    });

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task.id, blocked_id);
}

#[test]
fn active_task_query_excludes_paused_but_keeps_pending() {
    let (store, id, paused_id) = store_with_task(&["status_update"]);
    let pending_id = store
        .add_task(
            &id,
            NewTask {
                name: "dependency-waiting".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .pause_task(
            &id,
            &paused_id,
            "operator hold",
            crate::plan::AgentRef::from_tab("orch"),
            3,
        )
        .unwrap();

    let active = store.list_tasks(TaskListFilter {
        active_only: true,
        ..Default::default()
    });
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].task.id, pending_id);

    let paused = store.list_tasks(TaskListFilter {
        status: Some(TaskStatus::Paused),
        ..Default::default()
    });
    assert_eq!(paused.len(), 1);
    assert_eq!(paused[0].task.id, paused_id);
    assert_eq!(
        paused[0].task.pause_reason.as_deref(),
        Some("operator hold")
    );
}

#[test]
fn new_task_defaults_to_actionable_pending() {
    let (store, plan_id, _) = store_with_task(&["status_update"]);
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "new task".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            4,
        )
        .unwrap();

    let detail = store.show_task(&task_id).expect("new task detail");
    assert_eq!(detail.task.status, TaskStatus::Pending);
    assert_eq!(
        detail.presentation,
        crate::service_plan::TaskPresentation::ActionablePending
    );
}

#[test]
fn task_detail_marks_unfinished_dependency_as_waiting_not_paused() {
    let (store, plan_id, dependency_id) = store_with_task(&["status_update"]);
    store
        .update_task(
            &plan_id,
            &dependency_id,
            None,
            None,
            None,
            None,
            Some("blocked"),
            Some(Some("external prerequisite")),
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
            4,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "after dependency".into(),
                kind: "implement".into(),
                depends_on: vec![dependency_id.clone()],
                competition: None,
                ..Default::default()
            },
            5,
        )
        .unwrap();

    let detail = store.show_task(&task_id).expect("dependent task detail");
    assert_eq!(detail.task.status, TaskStatus::Pending);
    assert_eq!(
        detail.presentation,
        crate::service_plan::TaskPresentation::WaitingOnDependencies
    );
    assert_eq!(detail.dependency_waits.len(), 1);
    assert_eq!(detail.dependency_waits[0].task_id, dependency_id);
}
