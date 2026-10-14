use super::*;

#[test]
fn storage_completion_watch_routes_before_retirement_and_stays_deleted_after_reload() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let mut create_params = params(
        false,
        json!({"done":[{"id":"deliver","type":"notify","parameters":{}}]}),
    );
    create_params.until = Some("done".into());
    let watch = store
        .lock()
        .prepare(create_params, owner(Some("team")), false)
        .unwrap();
    let watch = commit(&store, |current| current.stage_insert(watch)).unwrap();
    let lease = due(&store).remove(0);
    let effects = Cell::new(0);
    run(&store, &lease, "done", "done", |_, _, _| {
        effects.set(effects.get() + 1);
        Ok(Status::Completed)
    });
    assert_eq!(effects.get(), 1);
    assert!(store.lock().get(&watch.id).unwrap().machine.stopped);
    crate::sentinel::cleanup::remove_terminal(&store).unwrap();
    assert!(store.lock().get(&watch.id).is_some());
    store.lock().release(&watch.id);
    crate::sentinel::cleanup::remove_terminal(&store).unwrap();
    assert!(store.lock().get(&watch.id).is_none());
    assert!(done(storage.sentinels().inventory()).value.is_empty());
    assert!(typed(&storage, &clock).lock().list().is_empty());
    close(storage);
}
