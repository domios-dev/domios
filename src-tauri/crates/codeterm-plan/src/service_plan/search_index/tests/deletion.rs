use super::*;

#[test]
fn delete_releases_the_plan_writer_while_its_index_project_is_busy() {
    let (root, store) = store_and_root();
    let store = Arc::new(store);
    let plan = store
        .create_plan(
            "Delete fixture".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let team_plan = store
        .create_plan_for_team(
            "Remaining Team".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            "fixture-team".into(),
            vec![],
            1,
        )
        .unwrap();
    flush_index_queue_for_test(root.path());
    let project = project_scope(root.path());
    let queue = index_queue();
    assert!(queue
        .state
        .lock()
        .in_flight_projects
        .insert(project.clone()));
    let deleting_store = store.clone();
    let id = plan.id.clone();
    let (finished, completion) = mpsc::channel();
    let deleting = std::thread::spawn(move || {
        let result = deleting_store.delete(&id).and_then(|()| {
            deleting_store
                .close_team_task_attempts_transaction("fixture-team", &[], 2)
                .map(|_| ())
                .map_err(|error| std::io::Error::other(error.to_string()))
        });
        let _ = finished.send(result);
        root
    });
    let result = completion.recv_timeout(Duration::from_millis(500));
    queue.state.lock().in_flight_projects.remove(&project);
    queue.ready.notify_all();
    let root = deleting.join().unwrap();
    result
        .expect("deletion and the next mutation must not wait for indexing")
        .unwrap();
    assert!(store.get(&plan.id).is_none());
    let closed_team_plan = store.get(&team_plan.id).unwrap();
    assert!(closed_team_plan.team_id.is_none());
    assert_eq!(closed_team_plan.status, crate::plan::PlanStatus::Archived);
    assert!(store.database.load_plan(&plan.id).unwrap().is_none());
    flush_index_queue_for_test(root.path());
    let hits = store
        .search_plan_index(
            root.path(),
            &PlanIndexQuery {
                query: "Delete fixture".into(),
                kind: None,
                scope: None,
                limit: Some(10),
            },
        )
        .unwrap();
    assert!(hits
        .iter()
        .all(|hit| hit.plan_id.as_deref() != Some(plan.id.as_str())));
}
