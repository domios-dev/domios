use super::*;
use crate::plan::{Milestone, OrchestratorRef};
use crate::service_plan::{NewTask, PlanStore};

fn plan_with_tasks(store: &PlanStore, root: &Path) -> (String, Vec<String>) {
    let plan = store
        .create_plan(
            "Incremental units".into(),
            root.display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let mut task_ids = Vec::new();
    for name in ["alpha", "beta", "gamma"] {
        task_ids.push(
            store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: name.into(),
                        body: format!("{name} body"),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap(),
        );
    }
    store
        .with_plan(&plan.id, 3, |plan| {
            for task in &mut plan.tasks {
                task.milestones.push(Milestone {
                    id: format!("{}-stride", task.id),
                    from: OrchestratorRef::default(),
                    kind: "note".into(),
                    text: format!("{} stride", task.name),
                    ts: 3,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
            }
            Ok(())
        })
        .unwrap();
    flush_index_queue_for_test(root);
    (plan.id, task_ids)
}

fn unit_body(root: &Path, id: &str) -> Option<String> {
    let conn = Connection::open(project_index_path(root)).unwrap();
    conn.query_row(
        "SELECT body_inline FROM mem_items WHERE id = ?1",
        [id],
        |row| row.get(0),
    )
    .ok()
}

#[test]
fn a_task_scope_rebuilds_only_listed_tasks_and_drops_removed_ones() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, tasks) = plan_with_tasks(&store, root.path());
    let mut plan = store.get(&plan_id).unwrap();
    plan.tasks[0].body = "alpha changed but not listed".into();
    plan.tasks[1].body = "beta changed".into();
    plan.tasks.remove(2);

    mirror_plan_scope(
        &plan,
        &PlanIndexScope::Tasks(HashSet::from([tasks[1].clone()])),
        false,
    )
    .unwrap();

    let task_unit = |task: &str| stable_id(&plan_id, "task", task);
    let stride_unit = |task: &str| stable_id(&plan_id, "stride", &format!("{task}-stride"));
    assert!(unit_body(root.path(), &task_unit(&tasks[0]))
        .unwrap()
        .contains("alpha body"));
    assert!(unit_body(root.path(), &task_unit(&tasks[1]))
        .unwrap()
        .contains("beta changed"));
    assert!(unit_body(root.path(), &stride_unit(&tasks[0])).is_some());
    assert!(unit_body(root.path(), &task_unit(&tasks[2])).is_none());
    assert!(unit_body(root.path(), &stride_unit(&tasks[2])).is_none());
}

#[test]
fn a_store_mutation_of_one_task_reindexes_only_that_task() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, tasks) = plan_with_tasks(&store, root.path());
    let untouched = stable_id(&plan_id, "task", &tasks[0]);
    Connection::open(project_index_path(root.path()))
        .unwrap()
        .execute(
            "UPDATE mem_items SET body_inline = 'sentinel' WHERE id = ?1",
            [&untouched],
        )
        .unwrap();

    store
        .with_plan(&plan_id, 4, |plan| {
            plan.tasks[1].body = "beta rewritten".into();
            Ok(())
        })
        .unwrap();
    flush_index_queue_for_test(root.path());

    assert_eq!(
        unit_body(root.path(), &untouched).as_deref(),
        Some("sentinel")
    );
    assert!(
        unit_body(root.path(), &stable_id(&plan_id, "task", &tasks[1]))
            .unwrap()
            .contains("beta rewritten")
    );
}

#[test]
fn queued_task_scopes_merge_and_full_wins() {
    let alpha = PlanIndexScope::Tasks(HashSet::from(["a".to_string()]));
    let beta = PlanIndexScope::Tasks(HashSet::from(["b".to_string()]));
    assert_eq!(
        alpha.clone().union(beta),
        PlanIndexScope::Tasks(HashSet::from(["a".to_string(), "b".to_string()]))
    );
    assert_eq!(alpha.union(PlanIndexScope::Full), PlanIndexScope::Full);
}
