use super::*;
use axum::{body::Body, http::Request, routing::get, Json, Router};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test(flavor = "current_thread")]
async fn held_network_probe_is_shared_and_does_not_block_rest() {
    let reads = Arc::new(Reads::default());
    let identity = Arc::new(());
    let key = Key::new(&identity, 17777, Kind::Network);
    let calls = Arc::new(AtomicUsize::new(0));
    let (release, dependency) = std::sync::mpsc::sync_channel(1);
    let dependency = Arc::new(Mutex::new(Some(dependency)));
    let (entered, started) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered)));
    let (admitted, mut admissions) = tokio::sync::mpsc::unbounded_channel();
    let router = Router::new()
        .route("/api/health", get(|| async { StatusCode::OK }))
        .route(
            "/api/network",
            get({
                let reads = reads.clone();
                let calls = calls.clone();
                move || {
                    let reads = reads.clone();
                    let calls = calls.clone();
                    let dependency = dependency.clone();
                    let entered = entered.clone();
                    let admitted = admitted.clone();
                    async move {
                        admitted.send(()).unwrap();
                        reads
                            .read(key, move || {
                                calls.fetch_add(1, Ordering::SeqCst);
                                let dependency =
                                    dependency.lock().take().expect("one probe producer");
                                entered.lock().take().unwrap().send(()).unwrap();
                                dependency
                                    .recv_timeout(Duration::from_secs(2))
                                    .expect("probe release");
                                serde_json::json!({"transport":"ready"})
                            })
                            .await
                            .map(Json)
                    }
                }
            }),
        );
    let request = || {
        Request::builder()
            .uri("/api/network")
            .body(Body::empty())
            .unwrap()
    };
    let first = tokio::spawn(router.clone().oneshot(request()));
    tokio::time::timeout(Duration::from_secs(1), started)
        .await
        .unwrap()
        .unwrap();
    admissions.recv().await.unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let second = tokio::spawn(router.clone().oneshot(request()));
    tokio::time::timeout(Duration::from_secs(1), admissions.recv())
        .await
        .unwrap()
        .unwrap();
    let health = tokio::time::timeout(
        Duration::from_millis(250),
        router.oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .expect("REST must answer while the probe dependency is held")
    .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    assert!(!second.is_finished());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    release.send(()).unwrap();
    let response = second.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        serde_json::json!({"transport":"ready"})
    );
    let next = reads
        .read(key, || serde_json::json!({"transport":"changed"}))
        .await
        .unwrap();
    assert_eq!(
        next,
        serde_json::json!({"transport":"changed"}),
        "completed reads are not cached"
    );
}
