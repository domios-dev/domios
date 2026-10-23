use super::*;

#[test]
fn storage_completion_watch_routes_before_retirement_and_stays_deleted_after_reload() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let mut create_params = params(
        false,
        json!({"done":[{"id":"deliver","uses":"notify","with":codeterm_sentinel::test_support::literal_params(json!({}))}]}),
    );
    create_params["steps"][1]["until"] = json!("done");
    let watch = store
        .lock()
        .prepare_document(create_params, owner(Some("team")), false)
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
    let retained = store.lock().get(&watch.id).unwrap().clone();
    assert_eq!(retained.cleanup.stopped_at, Some(1_000));
    assert_eq!(retained.cleanup.deletes_at, Some(1_801_000));
    crate::cleanup::remove_terminal(&store).unwrap();
    assert!(store.lock().get(&watch.id).is_some());
    store.lock().release(&watch.id);
    crate::cleanup::remove_terminal(&store).unwrap();
    assert_eq!(values(store.lock().list()), values(&[retained.clone()]));
    let reloaded = typed(&storage, &clock);
    assert_eq!(values(reloaded.lock().list()), values(&[retained]));
    clock.set_ms(1_800_999);
    crate::cleanup::remove_terminal(&reloaded).unwrap();
    assert!(reloaded.lock().get(&watch.id).is_some());
    clock.set_ms(1_801_000);
    crate::cleanup::remove_terminal(&reloaded).unwrap();
    assert!(reloaded.lock().get(&watch.id).is_none());
    assert!(done(storage.sentinels().inventory()).value.is_empty());
    assert!(typed(&storage, &clock).lock().list().is_empty());
    close(storage);
}
