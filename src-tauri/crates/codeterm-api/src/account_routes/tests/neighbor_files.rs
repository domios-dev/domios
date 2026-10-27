use super::*;

#[tokio::test]
async fn file_refusals_preserve_quota_fields_and_http_status() {
    for (reason, status) in [
        ("file_too_large", StatusCode::PAYLOAD_TOO_LARGE),
        ("sender_quota", StatusCode::TOO_MANY_REQUESTS),
        ("file_daily_limit", StatusCode::TOO_MANY_REQUESTS),
        ("files_disabled", StatusCode::SERVICE_UNAVAILABLE),
        ("recipient_files_unsupported", StatusCode::CONFLICT),
        ("file_not_found", StatusCode::NOT_FOUND),
    ] {
        let response = neighbor_failure(format!("{reason}: {{\"reason\":\"{reason}\",\"limit_bytes\":52428800,\"limit_files\":25,\"resets_at\":1234}}"));
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["reason"], reason);
        assert_eq!(body["code"], reason);
        assert_eq!(body["limit_bytes"], 52_428_800);
        assert_eq!(body["limit_files"], 25);
        assert_eq!(body["resets_at"], 1234);
    }
}

#[tokio::test]
async fn file_endpoint_rejects_nonlocal_callers_before_dispatch() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let input = serde_json::json!({"operation":"list","friendship_id":"a".repeat(64)});
    let response = crate::account_routes::neighbor_files(State(state), None, Json(input)).await.unwrap_err();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
