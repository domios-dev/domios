use super::*;
use std::task::{Context, Wake, Waker};
use std::time::Duration;

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn blocked_request_snapshot_identifies_route_worker_and_poll_phase() {
    let registry = Arc::new(Registry::default());
    let observed = registry.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("rest-fixture-worker".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(track_with(observed, "GET", "/api/tabs/{id}/chat", async {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            }));
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot_from(&registry)).unwrap();
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    let request = &snapshot["in_flight"][0];
    assert_eq!(request["method"], "GET");
    assert_eq!(request["route"], "/api/tabs/{id}/chat");
    assert_eq!(request["phase"], "polling");
    assert!(request["thread"]
        .as_str()
        .unwrap()
        .contains("rest-fixture-worker"));
    assert!(request["age_ms"].is_u64());
    assert!(request["phase_age_ms"].is_u64());
    assert!(registry.requests.lock().unwrap().is_empty());
}

#[test]
fn cancelled_request_releases_activity_and_capacity_is_bounded() {
    let registry = Arc::new(Registry::default());
    let waker = Waker::from(Arc::new(Noop));
    let mut context = Context::from_waker(&waker);
    let mut request = Box::pin(track_with(
        registry.clone(),
        "GET",
        "/pending",
        std::future::pending::<()>(),
    ));
    assert!(request.as_mut().poll(&mut context).is_pending());
    assert!(snapshot_from(&registry).contains("waiting"));
    drop(request);
    assert!(registry.requests.lock().unwrap().is_empty());
    let retained: Vec<_> = (0..MAX_REQUESTS + 1)
        .map(|_| Activity::start(registry.clone(), "GET", "/bounded"))
        .collect();
    assert_eq!(registry.requests.lock().unwrap().len(), MAX_REQUESTS);
    assert_eq!(registry.omitted.load(Ordering::Relaxed), 1);
    drop(retained);
    assert!(registry.requests.lock().unwrap().is_empty());
}
