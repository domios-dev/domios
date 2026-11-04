use super::*;
use crate::plan::{OrchestratorRef, Plan};

#[test]
fn task_tag_removal_persists_exact_matches_and_preserves_other_tags() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "Tags".into(),
        root.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    let id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Task".into(),
                tags: vec![
                    "build-breaking".into(),
                    "work:backend".into(),
                    "build-breaking-extra".into(),
                ],
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .remove_task_tags(
            &plan.id,
            &id,
            &["build-breaking".into(), "missing".into()],
            3,
        )
        .unwrap();
    let task = store.inspect_task_for_test(&id).unwrap().task;
    assert_eq!(task.tags, ["work:backend", "build-breaking-extra"]);
    store
        .remove_task_tags(&plan.id, &id, &task.tags, 4)
        .unwrap();
    drop(store);
    let reopened = PlanStore::new_for_test(root.path());
    assert!(reopened
        .inspect_task_for_test(&id)
        .unwrap()
        .task
        .tags
        .is_empty());
}
