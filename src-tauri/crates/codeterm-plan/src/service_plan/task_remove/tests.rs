use super::*;
use crate::service_plan::NewTask;

fn setup() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Remove Dependency".into(),
            dir.path().display().to_string(),
            Default::default(),
            vec![],
            1,
        )
        .unwrap();
    let dependency = store
        .add_task(
            &plan.id,
            NewTask {
                name: "dependency".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let dependent = store
        .add_task(
            &plan.id,
            NewTask {
                name: "dependent".into(),
                depends_on: vec![dependency.clone()],
                ..Default::default()
            },
            3,
        )
        .unwrap();
    (dir, store, plan.id, dependency, dependent)
}

#[test]
fn rm_task_rejects_referenced_dependency_without_mutation() {
    let (_dir, store, plan_id, dependency, dependent) = setup();
    let err = store.rm_task(&plan_id, &dependency, 4).unwrap_err();
    match err {
        PlanError::DependencyTaskReferenced {
            task_id,
            dependent_task_ids,
        } => {
            assert_eq!(task_id, dependency);
            assert_eq!(dependent_task_ids, vec![dependent.clone()]);
        }
        other => panic!("unexpected error: {other}"),
    }
    let plan = store.get(&plan_id).unwrap();
    assert!(plan.tasks.iter().any(|task| task.id == dependency));
    assert_eq!(
        plan.tasks
            .iter()
            .find(|task| task.id == dependent)
            .unwrap()
            .depends_on,
        vec![dependency]
    );
}

#[test]
fn rm_task_succeeds_after_dependency_cleared() {
    let (_dir, store, plan_id, dependency, dependent) = setup();
    store
        .update_task_dependencies(&plan_id, &dependent, vec![], 4)
        .unwrap();
    store.rm_task(&plan_id, &dependency, 5).unwrap();
    let plan = store.get(&plan_id).unwrap();
    assert!(!plan.tasks.iter().any(|task| task.id == dependency));
}
