use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::NewTask;
#[test]
fn task_files_persist_creation_and_replacement_and_refuse_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "opaque".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let first = TaskFiles {
        edit: vec!["new.rs".into()],
        ..Default::default()
    };
    let id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "opaque".into(),
                files: first.clone(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert_eq!(store.inspect_task_for_test(&id).unwrap().task.files, first);
    let second = TaskFiles {
        read: vec!["read.md".into()],
        ..Default::default()
    };
    store
        .update_task_files(&plan.id, &id, second.clone(), 3)
        .unwrap();
    assert!(store
        .update_task_files(
            &plan.id,
            &id,
            TaskFiles {
                edit: vec!["x".into()],
                read: vec!["x".into()]
            },
            4
        )
        .is_err());
    assert_eq!(store.inspect_task_for_test(&id).unwrap().task.files, second);
}

#[test]
fn live_shared_file_updates_keep_owned_paths_and_refuse_peer_conflicts_before_write() {
    use crate::plan::{TaskStatus, WorkerPlacement};
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "shared files".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    for (id, path) in [("mine", "owned.rs"), ("peer", "peer.rs")] {
        let mut task = crate::service_plan::load_audit::task(id, TaskStatus::InProgress, None);
        task.files.edit = vec![path.into()];
        let mut attempt = crate::service_plan::load_audit::attempt(&format!("attempt-{id}"), id);
        attempt.placement = WorkerPlacement::Shared;
        task.execution_attempts = vec![attempt];
        plan.tasks.push(task);
    }
    plan.tasks[0].handle = Some("mine-alias".into());
    store.save(&mut plan, 1).unwrap();
    let before = store.inspect_task_for_test("mine").unwrap().task;
    crate::service_plan::load_audit::start(&store);
    for paths in [vec!["new.rs"], vec!["owned.rs", "peer.rs"]] {
        let files = TaskFiles {
            edit: paths.into_iter().map(str::to_string).collect(),
            ..Default::default()
        };
        assert!(store
            .update_task_files(&plan.id, "mine-alias", files, 2)
            .is_err());
    }
    assert!(store.plan_loads_for_test().is_empty());
    assert_eq!(store.inspect_task_for_test("mine").unwrap().task, before);
    let expanded = TaskFiles {
        edit: vec!["owned.rs".into(), "new.rs".into()],
        ..Default::default()
    };
    store
        .update_task_files(&plan.id, "mine-alias", expanded.clone(), 3)
        .unwrap();
    assert_eq!(
        store.inspect_task_for_test("mine").unwrap().task.files,
        expanded
    );
}
