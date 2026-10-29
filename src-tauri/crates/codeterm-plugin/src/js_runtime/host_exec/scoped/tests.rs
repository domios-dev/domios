use super::*;
#[test]
fn scoped_deadline_returns_a_typed_unknown_dispatch_result_and_cancels_once() {
    let plugin = format!("scoped-{}", uuid::Uuid::new_v4().simple());
    let (started, entered) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let (finished, done) = std::sync::mpsc::channel();
    let cancelled = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = cancelled.clone();
    let timeout =
        serde_json::json!({"error":"deadline","requestMayHaveStarted":true,"status":null});
    let job = start_scoped_json_job(
        &plugin,
        "epoch",
        Instant::now(),
        Arc::new(move || {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            release.send(()).unwrap();
        }),
        timeout.clone(),
        move || {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(2)).unwrap();
            finished.send(()).unwrap();
            "{}".into()
        },
    )
    .unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(!abort_scoped_json_job("foreign", "epoch", &job));
    assert!(!abort_scoped_json_job(&plugin, "foreign", &job));
    let WaitResult::Completed(value) = wait_json(&plugin, &job) else {
        panic!("typed deadline")
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&value).unwrap(),
        timeout
    );
    done.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(cancelled.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn scoped_worker_panic_preserves_a_typed_conservative_failure() {
    let plugin = format!("scoped-panic-{}", uuid::Uuid::new_v4().simple());
    let job = start_scoped_json_job(
        &plugin,
        "epoch",
        Instant::now() + Duration::from_secs(2),
        Arc::new(|| {}),
        serde_json::json!({"error":"deadline","requestMayHaveStarted":true,"status":null}),
        || panic!("opaque worker failure"),
    )
    .unwrap();
    let WaitResult::Completed(value) = wait_json(&plugin, &job) else {
        panic!("typed completion")
    };
    let result: serde_json::Value = serde_json::from_str(&value).unwrap();
    assert_eq!(result["error"], "transport_unconfirmed");
    assert_eq!(result["requestMayHaveStarted"], true);
    assert_eq!(result["status"], serde_json::Value::Null);
    assert!(!value.contains("opaque worker failure"));
}
