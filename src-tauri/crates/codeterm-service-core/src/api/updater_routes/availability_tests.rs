use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::get,
    Router,
};
use tower::ServiceExt;

const SNAPSHOT: &str =
    include_str!("../../../../../../tests/fixtures/update-availability/snapshot.json");

#[tokio::test]
async fn availability_json_contract_preserves_prerelease_and_escaped_errors() {
    for installed in ["1.14.9-bs.3", "1.14.9-dev.4"] {
        let item = codeterm_plugins::AvailabilityItem::release(
            "app",
            codeterm_plugins::UpdateKind::App,
            installed,
            "1.14.8",
            true,
        );
        assert_eq!(item.reason, "local_ahead");
        assert_eq!(item.state, codeterm_plugins::AvailabilityState::UpToDate);
        let mut snapshot: crate::update_state::AvailabilitySnapshot =
            serde_json::from_str(SNAPSHOT).unwrap();
        snapshot.items = vec![item];
        let expected = serde_json::to_value(&snapshot).unwrap();
        let router = Router::new().route(
            "/api/update/availability",
            get(move || {
                let snapshot = snapshot.clone();
                async move { availability_response(Ok(snapshot)) }
            }),
        );
        let response = router
            .oneshot(
                Request::get("/api/update/availability?force=false")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        let body = to_bytes(response.into_body(), 65536).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn availability_json_contract_failure_has_non_success_status_and_json_body() {
    let detail = "manifest \"unavailable\"\nretry later";
    let router = Router::new().route(
        "/api/update/availability",
        get(move || async move { availability_response(Err(detail.into())) }),
    );
    let response = router
        .oneshot(
            Request::get("/api/update/availability?force=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"error": detail})
    );
}

#[tokio::test]
async fn availability_json_contract_real_handler_rejects_invalid_query_as_json() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let app =
        crate::api::routes::create_router(crate::api::transport_tabs::fixture::app_state(&fixture));
    let response = app
        .oneshot(
            Request::get("/api/update/availability?force=invalid")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"].is_string());
}
