use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

#[tokio::test]
async fn providerless_plugin_bubble_route_returns_http_200_null() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-bearer".into());
    let router = Router::new()
        .route(
            "/api/plugins/bubble",
            post(crate::web_routes::plugin_bubble),
        )
        .with_state(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/plugins/bubble")
                .header("authorization", "Bearer fixture-bearer")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"pluginId":"pebble","ctx":{"cwd":null,"tabId":"fixture-tab"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        &to_bytes(response.into_body(), 1024).await.unwrap()[..],
        b"null"
    );
}
