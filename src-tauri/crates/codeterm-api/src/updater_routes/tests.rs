use super::{validate_after_update_enabled, validate_initiating_tab};

#[test]
fn after_update_request_is_refused_when_resume_is_disabled() {
    assert_eq!(validate_after_update_enabled(true, false), Err("disabled"));
    assert!(validate_after_update_enabled(false, false).is_ok());
    assert!(validate_after_update_enabled(true, true).is_ok());
}

#[test]
fn auto_update_initiating_tab_must_match_authenticated_caller() {
    assert_eq!(
        validate_initiating_tab(Some("caller"), Some("other")),
        Err("not_caller_tab")
    );
    assert!(validate_initiating_tab(Some("caller"), Some("caller")).is_ok());
    assert!(validate_initiating_tab(None, None).is_ok());
}

#[tokio::test]
async fn auto_install_without_native_host_returns_actionable_typed_refusal() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    assert!(codeterm_commands::application_host::require(&state.svc).is_err());
    let app = crate::routes::create_router(state);
    let response = app
        .oneshot(
            Request::post("/api/update/install?method=auto")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(response.headers()["content-type"], "application/json");
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "native_capability_unavailable");
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("attached to this server"));
    assert!(message.contains("https://github.com/domios-dev/domios/releases/latest"));
    assert!(message.contains("domios update install --from-file <installer-path>"));
}
