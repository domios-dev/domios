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
