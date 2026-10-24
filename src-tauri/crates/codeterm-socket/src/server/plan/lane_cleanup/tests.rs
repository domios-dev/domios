use super::*;

#[test]
fn lane_cleanup_preview_and_other_routes_do_not_wait_for_running_deletion() {
    let (fixture, plan_id, _, _) = codeterm_service_core::service::plan_authority_fixture();
    let svc = Arc::new(fixture.service.clone());
    let plan = svc.plan_store.get(&plan_id).unwrap();
    let deleting = Arc::new(Job {
        plan_id: plan_id.clone(),
        confirm: true,
        state: Mutex::new(Progress::default()),
        changed: Condvar::new(),
    });
    jobs(&svc)
        .unwrap()
        .0
        .lock()
        .jobs
        .insert("deleting".into(), deleting.clone());
    assert!(start(&svc, plan.clone(), true, false, true).is_err());
    let preview = start(&svc, plan, false, false, false).unwrap();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["lanes"], json!([]));
    assert!(deleting.state.lock().result.is_none());
    assert!(crate::server::dispatch_service_rpc("system.identify", None, &svc).is_ok());

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let reader_svc = svc.clone();
    let reader = std::thread::spawn(move || {
        entered_tx.send(()).unwrap();
        next(&reader_svc, "deleting", 0).unwrap()
    });
    entered_rx.recv().unwrap();
    deleting
        .state
        .lock()
        .events
        .push(json!({"lane": {"action": "checking"}, "finished": false}));
    deleting.changed.notify_all();
    let update = reader.join().unwrap();
    assert_eq!(update["cursor"], 1);
    assert_eq!(update["done"], false);
    assert_eq!(update["events"][0]["lane"]["action"], "checking");
    deleting.state.lock().result = Some(Ok(json!({"lanes": []})));
    deleting.changed.notify_all();
    assert_eq!(next(&svc, "deleting", 1).unwrap()["done"], true);
    assert!(!jobs(&svc).unwrap().0.lock().jobs.contains_key("deleting"));
}
