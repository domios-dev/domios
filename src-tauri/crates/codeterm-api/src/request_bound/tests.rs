use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::get,
    Router,
};
use tower::ServiceExt;

#[tokio::test(start_paused = true)]
async fn stuck_snapshot_producer_returns_typed_unavailable_within_bound() {
    let router = Router::new().route(
        "/api/plugins/snapshot",
        get(|| async {
            bounded(
                PLUGIN_DEADLINE,
                std::future::pending::<Result<Json<serde_json::Value>, String>>(),
            )
            .await
        }),
    );
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/plugins/snapshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap()).unwrap();
    assert_eq!(body["ok"], false);
    assert_eq!(body["code"], "plugin_unavailable");
}

#[test]
fn chat_start_slot_rejects_concurrency_and_releases_on_worker_exit() {
    let slot = RequestSlot::acquire("test-chat-start:one".into()).unwrap();
    let error = RequestSlot::acquire("test-chat-start:one".into())
        .err()
        .unwrap();
    assert_eq!(error.1 .0["code"], "request_busy");
    let other = RequestSlot::acquire("test-chat-start:two".into()).unwrap();
    drop(slot);
    assert!(RequestSlot::acquire("test-chat-start:one".into()).is_ok());
    drop(other);
}

#[tokio::test(start_paused = true)]
async fn snapshot_slot_survives_request_deadline_until_blocking_work_exits() {
    let key = "test-plugin-snapshot".to_string();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let work_key = key.clone();
    let request = tokio::spawn(blocking(work_key, move |_slot| {
        started.send(()).unwrap();
        wait.recv().unwrap();
        Ok(())
    }));
    ready.await.unwrap();
    tokio::time::advance(PLUGIN_DEADLINE).await;
    assert_eq!(
        request.await.unwrap().unwrap_err().1 .0["code"],
        "plugin_unavailable"
    );
    assert_eq!(
        RequestSlot::acquire(key.clone()).err().unwrap().1 .0["code"],
        "request_busy"
    );
    release.send(()).unwrap();
}

#[tokio::test]
async fn chat_start_waiter_acquires_only_after_own_slot_is_released() {
    let slot = RequestSlot::acquire("test-chat-wait:one".into()).unwrap();
    let other = RequestSlot::acquire("test-chat-wait:other".into()).unwrap();
    let waiting = RequestSlot::acquire_when_available("test-chat-wait:one".into());
    tokio::pin!(waiting);
    assert!(futures_util::poll!(waiting.as_mut()).is_pending());
    drop(other);
    assert!(futures_util::poll!(waiting.as_mut()).is_pending());
    drop(slot);
    let acquired = waiting.await.unwrap();
    assert!(RequestSlot::acquire("test-chat-wait:one".into()).is_err());
    drop(acquired);
    assert!(RequestSlot::acquire("test-chat-wait:one".into()).is_ok());
}

#[tokio::test(start_paused = true)]
async fn chat_start_waiter_fails_within_bound_without_releasing_the_producer() {
    let slot = RequestSlot::acquire("test-chat-wait:deadline".into()).unwrap();
    let error = RequestSlot::acquire_when_available("test-chat-wait:deadline".into())
        .await
        .err()
        .unwrap();
    assert_eq!(error.1 .0["code"], "request_busy");
    assert!(RequestSlot::acquire("test-chat-wait:deadline".into()).is_err());
    drop(slot);
    assert!(RequestSlot::acquire("test-chat-wait:deadline".into()).is_ok());
}
