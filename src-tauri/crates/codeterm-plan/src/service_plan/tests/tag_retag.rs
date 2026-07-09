use super::store_with_task;

#[test]
fn update_task_retag_replaces_facet_and_regenerates_handle() {
    let (store, id, task_id) = store_with_task(&["status_update"]);
    store
        .update_task(
            &id,
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
            None,
            None,
            None,
            &["area:nav".into(), "pan-zoom".into()],
            Some("area:nav"),
            None,
            80,
        )
        .unwrap();
    store
        .update_task(
            &id,
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
            None,
            None,
            None,
            &["area:ui".into()],
            Some("area:ui"),
            None,
            81,
        )
        .unwrap();
    let task = store
        .get(&id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .unwrap();
    assert_eq!(task.tags, vec!["area:ui", "pan-zoom"]);
    assert_eq!(task.handle.as_deref(), Some("ui-pan-zoom"));
}
