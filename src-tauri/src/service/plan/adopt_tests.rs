use std::path::{Path, PathBuf};

use super::adopt::AdoptPlanRequest;
use super::task_queries::TaskListFilter;
use super::{NewTask, PlanStore};
use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanStatus, PlannerRef, TaskStatus};

fn make_source_plan(root: &Path) -> (PlanStore, String, String, PathBuf) {
    let store = PlanStore::new_for_test(root);
    let mut plan = Plan::new(
        "Adopt me".into(),
        root.display().to_string(),
        OrchestratorRef {
            tab_id: Some("old-owner".into()),
            session_id: Some("old-session".into()),
            title: Some("old title".into()),
        },
    );
    plan.id = "pl_adoptme".into();
    plan.mode = PlanMode::Execution;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "old-planner".into(),
        session_id: Some("planner-session".into()),
        title: Some("planner".into()),
    });
    plan.heartbeat.current_focus = Some("old focus".into());
    plan.heartbeat.active_lanes = vec!["old lane".into()];
    plan.heartbeat.rules = vec!["keep durable rule".into()];
    store.save(&mut plan, 10).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Task with sidecar".into(),
                handle: Some("task-with-sidecar".into()),
                body: "Preserve this body".into(),
                acceptance_criteria: vec!["Body survives".into()],
                test_plan: Some("Read task list".into()),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 30, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InProgress;
            task.assigned_tab_id = Some("old-worker".into());
            task.lane.tab_id = Some("old-worker".into());
            task.lane.workspace_id = Some("old-workspace".into());
            task.lane.workspace_name = Some("Old workspace".into());
            task.lane.worktree_path = Some(root.join("old-lane").display().to_string());
            task.lane.branch = Some("lane/old".into());
            task.taken_by_role = Some("Worker".into());
            task.peer = Some("old-peer".into());
            Ok(())
        })
        .unwrap();
    let source_dir = root
        .join(".codeterm")
        .join("plans")
        .join("active")
        .join("pl_adoptme");
    std::fs::write(
        source_dir.join("tasks").join("task-with-sidecar.jsonl"),
        "{\"kind\":\"stride\"}\n",
    )
    .unwrap();
    (store, plan.id, task_id, source_dir)
}

#[test]
fn adopt_plan_preserves_task_files_and_clears_stale_runtime_bindings() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (_source_store, plan_id, task_id, source_dir) = make_source_plan(source.path());
    let target_store = PlanStore::new_for_test(target.path());

    let report = target_store
        .adopt_plan(
            AdoptPlanRequest {
                source: source_dir.clone(),
                target_project: target.path().to_path_buf(),
                owner: OrchestratorRef::from_tab("new-owner"),
                dry_run: false,
                overwrite: false,
                remove_source: false,
            },
            100,
        )
        .unwrap();

    assert_eq!(report.plan_id, plan_id);
    assert_eq!(report.task_count, 1);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("task Tab/lane bindings")));
    let target_root = target.path().canonicalize().unwrap();
    let adopted = target_store.get(&plan_id).unwrap();
    assert_eq!(adopted.project_path, target_root.display().to_string());
    assert_eq!(
        adopted.canonical_project_path,
        target_root.display().to_string()
    );
    assert_eq!(adopted.owner.tab_id.as_deref(), Some("new-owner"));
    assert_eq!(adopted.status, PlanStatus::Archived);
    assert_eq!(adopted.team_id, None);
    assert_eq!(adopted.planner, None);
    assert_eq!(adopted.heartbeat.current_focus, None);
    assert!(adopted.heartbeat.active_lanes.is_empty());
    assert_eq!(adopted.heartbeat.rules, vec!["keep durable rule"]);
    let task = adopted
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.body, "Preserve this body");
    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.lane.tab_id, None);
    assert_eq!(task.lane.workspace_id, None);
    assert_eq!(task.lane.worktree_path, None);
    assert_eq!(task.lane.branch.as_deref(), Some("lane/old"));
    assert_eq!(task.taken_by_role, None);
    assert_eq!(task.peer, None);

    let target_dir = target
        .path()
        .join(".codeterm")
        .join("plans")
        .join("archive")
        .join(&plan_id);
    assert!(target_dir
        .join("tasks")
        .join("task-with-sidecar.yaml")
        .is_file());
    assert!(target_dir
        .join("tasks")
        .join("task-with-sidecar.jsonl")
        .is_file());
    assert!(target_store
        .list_index()
        .iter()
        .any(|entry| entry.plan_id == plan_id));
    assert_eq!(
        target_store
            .list_tasks(TaskListFilter {
                plan_id: Some(plan_id.clone()),
                ..Default::default()
            })
            .len(),
        1
    );
}

#[test]
fn adopt_plan_dry_run_reports_without_writing() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (_source_store, plan_id, _task_id, source_dir) = make_source_plan(source.path());
    let target_store = PlanStore::new_for_test(target.path());

    let report = target_store
        .adopt_plan(
            AdoptPlanRequest {
                source: source_dir,
                target_project: target.path().to_path_buf(),
                owner: OrchestratorRef::from_tab("new-owner"),
                dry_run: true,
                overwrite: false,
                remove_source: true,
            },
            100,
        )
        .unwrap();

    assert!(report.dry_run);
    assert!(!report.moved);
    assert_eq!(report.plan_id, plan_id);
    assert_eq!(report.task_count, 1);
    assert!(target_store.get(&plan_id).is_none());
    assert!(!target
        .path()
        .join(".codeterm")
        .join("plans")
        .join("archive")
        .join(&plan_id)
        .join("plan.yaml")
        .is_file());
    assert!(source_dir
        .join("tasks")
        .join("task-with-sidecar.yaml")
        .is_file());
    assert!(source_dir
        .join("tasks")
        .join("task-with-sidecar.jsonl")
        .is_file());
}

#[test]
fn adopt_plan_rejects_existing_destination_without_overwrite() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (_source_store, plan_id, _task_id, source_dir) = make_source_plan(source.path());
    let target_store = PlanStore::new_for_test(target.path());
    let mut existing = Plan::new(
        "Existing".into(),
        target.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    existing.id = plan_id.clone();
    target_store.save(&mut existing, 1).unwrap();

    let err = target_store
        .adopt_plan(
            AdoptPlanRequest {
                source: source_dir,
                target_project: target.path().to_path_buf(),
                owner: OrchestratorRef::from_tab("new-owner"),
                dry_run: false,
                overwrite: false,
                remove_source: false,
            },
            100,
        )
        .unwrap_err();

    assert!(err.to_string().contains("already exists"));
}
