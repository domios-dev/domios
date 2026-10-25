use super::*;
use axum::{body::Body, http::Request, middleware};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

#[tokio::test]
async fn runtime_login_read_requires_authentication_and_never_creates_a_runtime() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("test-token".into());
    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/tabs/{tab}/agent/runtime/login"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tabs/{tab}/agent/runtime/login"))
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .agent_runtime_state
        .is_none());
}

#[test]
fn runtime_login_response_preserves_opaque_values_without_http_caching() {
    let value = serde_json::json!({"login":{"status":"pending","url":"https://example.invalid/opaque-path","userCode":"opaque-token","expiresAtMs":5000}});
    let (headers, Json(body)) = login_response(value);
    assert_eq!(headers[axum::http::header::CACHE_CONTROL], "no-store");
    assert_eq!(body["login"]["url"], "https://example.invalid/opaque-path");
    assert_eq!(body["login"]["userCode"], "opaque-token");
    assert_eq!(body["login"]["expiresAtMs"], 5000);
}
