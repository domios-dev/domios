use super::{NewTask, PlanStore};
use crate::plan::{MilestoneKind, OrchestratorRef};

fn store_with_task() -> (PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Timed".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
            vec![MilestoneKind {
                key: "status_update".into(),
                label: "Status".into(),
                color: None,
            }],
            1,
        )
        .unwrap();
    (store, plan.id)
}

#[test]
fn task_timestamps_track_create_update_and_status_transition() {
    let (store, id) = store_with_task();
    let task_id = store
        .add_task(
            &id,
            NewTask {
                name: "Timed".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            20,
        )
        .unwrap();
    let created = store
        .get(&id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(created.created_at, 20);
    assert_eq!(created.updated_at, 20);
    assert_eq!(created.status_changed_at, 20);

    store
        .update_task(
            &id,
            &task_id,
            Some("Renamed"),
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
            None,
            None,
            &[],
            None,
            None,
            25,
        )
        .unwrap();
    let renamed = store
        .get(&id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(renamed.created_at, 20);
    assert_eq!(renamed.updated_at, 25);
    assert_eq!(renamed.status_changed_at, 20);

    store
        .fill_task_lane_if_empty(&id, &task_id, Some("tab-1"), None, None, 29)
        .unwrap();
    store
        .update_task(
            &id,
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
            30,
        )
        .unwrap();
    let transitioned = store
        .get(&id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(transitioned.updated_at, 30);
    assert_eq!(transitioned.status_changed_at, 30);
}
