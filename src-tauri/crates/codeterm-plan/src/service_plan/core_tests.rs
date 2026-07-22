use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanStatus, TaskStatus};

fn plan_path(_root: &std::path::Path, plan: &Plan) -> std::path::PathBuf {
    // Persistence is yaml now; the canonical on-disk file is plan.yaml.
    let dir = super::store_v2::plan_state_dir(plan).unwrap();
    super::store_v2::plan_yaml_path(&dir)
}

#[test]
fn store_saves_loads_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "T".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();
    let path = plan_path(dir.path(), &p);
    assert!(path.exists());
    let loaded = store.get(&p.id).unwrap();
    assert_eq!(loaded.title, "T");
    assert!(store.list_index().iter().any(|e| e.plan_id == p.id));
    assert_eq!(loaded.updated_at, 1000);
}

#[test]
fn reload_from_disk_replaces_memory_and_index() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Original".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();

    let path = plan_path(dir.path(), &p);
    let mut edited: Plan = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edited.title = "Edited".into();
    edited.updated_at = 2000;
    std::fs::write(&path, serde_yaml::to_string(&edited).unwrap()).unwrap();

    store.reload_from_disk(&p.id).unwrap();

    assert_eq!(store.get(&p.id).unwrap().title, "Edited");
    let entry = store
        .list_index()
        .into_iter()
        .find(|e| e.plan_id == p.id)
        .unwrap();
    assert_eq!(entry.title, "Edited");
    assert_eq!(entry.updated_at, 2000);
}

#[test]
fn in_review_status_persists_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Review".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "worker lane".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["it works".into()],
                test_plan: Some("run focused test".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .update_task(
            &p.id,
            &task_id,
            None,
            None,
            None,
            None,
            Some("in_review"),
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
            3,
        )
        .unwrap();

    store.plans.write().clear();
    store.reload_from_disk(&p.id).unwrap();

    let loaded = store.get(&p.id).unwrap();
    let task = loaded.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::InReview);
}

#[test]
fn task_quality_fields_persist_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Quality".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "criteria lane".into(),
                kind: "implement".into(),
                acceptance_criteria: vec![
                    "Task JSON contains structured acceptance criteria".into(),
                    "CLI shows each criterion".into(),
                ],
                test_plan: Some(
                    "cargo test service::plan::core_tests::task_quality_fields_persist_and_reload"
                        .into(),
                ),
                correctness: Some("quality fields round-trip unchanged".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store.plans.write().clear();
    store.reload_from_disk(&p.id).unwrap();

    let loaded = store.get(&p.id).unwrap();
    let task = loaded.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(
        task.acceptance_criteria,
        vec![
            "Task JSON contains structured acceptance criteria",
            "CLI shows each criterion"
        ]
    );
    assert_eq!(
        task.test_plan.as_deref(),
        Some("cargo test service::plan::core_tests::task_quality_fields_persist_and_reload")
    );
    assert_eq!(
        task.correctness.as_deref(),
        Some("quality fields round-trip unchanged")
    );
}

#[test]
fn reload_from_disk_preserves_plan_arc_for_concurrent_holders() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Original".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();
    let held = store.plans.read().get(&p.id).cloned().unwrap();

    let path = plan_path(dir.path(), &p);
    let mut edited: Plan = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edited.title = "Edited".into();
    edited.updated_at = 2000;
    std::fs::write(&path, serde_yaml::to_string(&edited).unwrap()).unwrap();

    store.reload_from_disk(&p.id).unwrap();

    let current = store.plans.read().get(&p.id).cloned().unwrap();
    assert!(std::sync::Arc::ptr_eq(&held, &current));
    assert_eq!(held.lock().title, "Edited");
}

#[test]
fn reload_from_disk_does_not_overwrite_newer_in_flight_plan_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(PlanStore::new_for_test(dir.path()));
    let mut p = Plan::new(
        "Original".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut p, 1000).unwrap();
    let held = store.plans.read().get(&p.id).cloned().unwrap();

    let path = plan_path(dir.path(), &p);
    let mut stale: Plan = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    stale.title = "Stale disk snapshot".into();
    stale.updated_at = 3000;
    std::fs::write(&path, serde_yaml::to_string(&stale).unwrap()).unwrap();

    let mut guard = held.lock();
    let reload_store = store.clone();
    let plan_id = p.id.clone();
    let reload = std::thread::spawn(move || reload_store.reload_from_disk(&plan_id));
    std::thread::sleep(std::time::Duration::from_millis(25));

    guard.title = "Newer in-flight mutation".into();
    guard.updated_at = 3000;
    let written_path = PlanStore::write_atomic(&guard).unwrap();
    store.note_plan_write(&guard, &written_path);
    drop(guard);

    reload.join().unwrap().unwrap();

    let current = store.get(&p.id).unwrap();
    assert_eq!(current.title, "Newer in-flight mutation");
    assert_eq!(current.updated_at, 3000);
    let entry = store
        .list_index()
        .into_iter()
        .find(|entry| entry.plan_id == p.id)
        .unwrap();
    assert_eq!(entry.title, "Newer in-flight mutation");
    assert_eq!(entry.updated_at, 3000);
}

#[test]
fn scan_project_loads_plan_written_directly_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path());
    let mut plan = Plan::new(
        "Persisted".into(),
        project.display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    plan.created_at = 1000;
    plan.updated_at = 2000;
    let plan_id = plan.id.clone();
    let path = plan_path(dir.path(), &plan);
    std::fs::write(&path, serde_yaml::to_string(&plan).unwrap()).unwrap();

    let loaded_project = store.scan_project(dir.path()).unwrap();
    let plans: Vec<_> = store
        .list_index()
        .into_iter()
        .filter_map(|e| store.get(&e.plan_id))
        .filter(|p| p.project_path == loaded_project.display().to_string())
        .collect();

    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].id, plan_id);
    assert_eq!(plans[0].title, "Persisted");
}

#[test]
fn approved_plan_flips_to_execution_on_first_in_progress_task() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Modes".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    p.mode = crate::plan::PlanMode::Approved;
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "first lane".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert_eq!(
        store.get(&p.id).unwrap().mode,
        crate::plan::PlanMode::Approved,
        "adding a pending task must not flip mode"
    );

    store
        .set_task_assignment(&task_id, Some("worker-pane"), Some("worker"), 3)
        .unwrap();
    store
        .update_task(
            &p.id,
            &task_id,
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
            4,
        )
        .unwrap();

    assert_eq!(
        store.get(&p.id).unwrap().mode,
        crate::plan::PlanMode::Execution,
        "first in_progress task must flip approved -> execution"
    );
}

#[test]
fn take_binds_pane_and_atomically_starts_task() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Take".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    p.mode = crate::plan::PlanMode::Approved;
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "lane".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let fresh = store.get(&p.id).unwrap();
    let task = fresh.tasks.iter().find(|x| x.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.assigned_pane_id.is_none());

    // Taking a task binds the pane AND starts it in one step — no separate flip.
    store
        .set_task_assignment(&task_id, Some("worker-pane"), Some("worker"), 3)
        .unwrap();

    let taken = store.get(&p.id).unwrap();
    let task = taken.tasks.iter().find(|x| x.id == task_id).unwrap();
    assert_eq!(
        task.status,
        TaskStatus::InProgress,
        "take must flip pending -> in_progress atomically"
    );
    assert_eq!(task.assigned_pane_id.as_deref(), Some("worker-pane"));
    assert_eq!(
        taken.mode,
        crate::plan::PlanMode::Execution,
        "auto-start flips approved -> execution"
    );
}

#[test]
fn archive_moves_plan_folder_between_buckets() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Move".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("abc"),
    );
    store.save(&mut plan, 1).unwrap();
    let active_path = plan_path(dir.path(), &plan);
    assert!(active_path.exists());

    plan.status = PlanStatus::Archived;
    store.save(&mut plan, 2).unwrap();
    let archive_path = plan_path(dir.path(), &plan);
    assert!(archive_path.exists());
    assert!(!active_path.exists());
}
