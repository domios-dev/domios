use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::NewTask;

fn store_with_backend_section() -> (PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    let plan_id = plan.id.clone();
    store.save(&mut plan, 1).unwrap();
    store
        .add_section(&plan_id, "Backend", "", None, &[], None, Some("backend"), 2)
        .unwrap();
    (store, plan_id)
}

#[test]
fn add_task_auto_links_by_group() {
    let (store, plan_id) = store_with_backend_section();
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "Implement route".into(),
                kind: "implement".into(),
                group: Some("backend".into()),
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.section_ids.len(), 1);
}

#[test]
fn ease_task_uses_the_only_section_when_no_area_matches() {
    let (store, plan_id) = store_with_backend_section();
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "Unrelated task".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(
        store.show_task(&task_id).unwrap().task.section_ids,
        vec![plan.sections[0].id.clone()]
    );
}

#[test]
fn relink_preview_does_not_mutate_and_apply_links() {
    let (store, plan_id) = store_with_backend_section();
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "Backend follow-up".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .update_task(
            &plan_id,
            &task_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![]),
            None,
            None,
            &[],
            None,
            None,
            4,
        )
        .unwrap();

    let preview = store.relink_unlinked_tasks(&plan_id, false, 5).unwrap();
    assert_eq!(preview.suggestions.len(), 1);
    assert!(store.get(&plan_id).unwrap().tasks[0].section_ids.is_empty());

    let applied = store.relink_unlinked_tasks(&plan_id, true, 6).unwrap();
    assert_eq!(applied.applied.len(), 1);
    assert_eq!(store.get(&plan_id).unwrap().tasks[0].section_ids.len(), 1);
}
