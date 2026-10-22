use super::{NewTask, PlanStore};
use crate::plan::{OrchestratorRef, Plan};

fn store_with_task() -> (tempfile::TempDir, PlanStore, Plan, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Global assets".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Keep evidence".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (dir, store, plan, task_id)
}

#[test]
fn attachment_storage_is_global_to_the_database_not_project_sidecars() {
    let (dir, store, plan, task_id) = store_with_task();
    let source = dir.path().join("evidence.md");
    std::fs::write(&source, "proof").unwrap();

    let attachment = store
        .attach_to_task(&plan.id, &task_id, &source, 3)
        .unwrap();
    let stored = store.attachment_abs_path(&plan, &attachment.path).unwrap();

    assert_eq!(std::fs::read_to_string(&stored).unwrap(), "proof");
    assert!(stored.starts_with(dir.path().join(".domios").join("plans").join(&plan.id)));
    assert!(!dir
        .path()
        .join(".domios/plans/active")
        .join(&plan.id)
        .join(&attachment.path)
        .exists());
    assert!(store.attachment_abs_path(&plan, "../secret").is_none());
}

#[test]
fn deleting_a_plan_removes_its_global_assets() {
    let (dir, store, plan, task_id) = store_with_task();
    let source = dir.path().join("evidence.txt");
    std::fs::write(&source, "proof").unwrap();
    let attachment = store
        .attach_to_task(&plan.id, &task_id, &source, 3)
        .unwrap();
    let stored = store.attachment_abs_path(&plan, &attachment.path).unwrap();
    assert!(stored.exists());

    store.delete(&plan.id).unwrap();

    assert!(!stored.exists());
    assert!(!dir
        .path()
        .join(".domios")
        .join("plans")
        .join(&plan.id)
        .exists());
}

#[test]
fn invalid_report_attachment_does_not_publish_a_plan_reference() {
    let (dir, store, plan, task_id) = store_with_task();
    let source = dir.path().join("empty.mov");
    std::fs::write(&source, []).unwrap();
    assert!(store
        .attach_to_task(&plan.id, &task_id, &source, 3)
        .is_err());
    assert!(store
        .attach_to_task(&plan.id, &task_id, &dir.path().join("missing"), 4)
        .is_err());
    let loaded = store.get(&plan.id).unwrap();
    assert!(loaded
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap()
        .attachments
        .is_empty());
}
