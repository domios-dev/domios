use super::*;

use crate::transport_tabs::fixture;

#[test]
fn transport_bootstrap_contract_router_merges_once_with_domains() {
    let _: Router<AppState> = Router::new()
        .merge(super::router())
        .merge(crate::transport_tabs::router())
        .merge(crate::transport_capabilities::router())
        .merge(crate::transport_orchestration::router());
    assert!(serde_json::from_value::<Ready>(json!({"generation": 9})).is_ok());
    for body in [
        json!({}),
        json!({"generation": 9, "token": "untrusted"}),
        json!({"generation": -1}),
    ] {
        assert!(serde_json::from_value::<Ready>(body).is_err());
    }
}

#[tokio::test]
async fn transport_bootstrap_contract_auth_status_and_stale_readiness() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_host_runtime::test_fixture::HostFixture::new(mux);
    let state = fixture::app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    for (method, path) in [
        ("GET", "/api/frontend/bootstrap"),
        ("POST", "/api/frontend/ready"),
        ("GET", "/api/desktop/recovery"),
        ("GET", "/api/listener/status"),
        ("POST", "/api/listener/restart"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/frontend/bootstrap")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    let bootstrap: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        bootstrap["apiVersion"],
        codeterm_service_core::frontend_bundle_api_version::API_VERSION
    );
    assert_eq!(bootstrap["listener"]["phase"], "starting");
    assert!(bootstrap["generation"].is_u64());
    assert!(bootstrap.get("token").is_none());
    assert!(!String::from_utf8(body.to_vec())
        .unwrap()
        .contains("preview-test-token"));
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/frontend/ready")
                .header("authorization", "Bearer preview-test-token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"generation": u64::MAX}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"false"
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/listener/status")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    let mut expected = serde_json::to_value(state.rest_api.status()).unwrap();
    expected["bindError"] = Value::Null;
    assert_eq!(status, expected);
    assert!(!super::owner(&state, &HeaderMap::new()).is_ok());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/desktop/recovery")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let recovery: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert!(recovery["current"]["pid"].is_u64());
    assert!(recovery["crash_presentation"].is_string());
}

#[tokio::test]
async fn listener_restart_contract_body_completion_and_cancellation() {
    use futures_util::StreamExt;
    let (completed, mut completion) = tokio::sync::oneshot::channel();
    let response = restart_response(Bytes::from_static(b"accepted"), completed);
    tokio::pin!(response);
    assert_eq!(response.next().await.unwrap().unwrap(), "accepted");
    assert!(completion.try_recv().is_err());
    assert!(response.next().await.is_none());
    assert!(completion.await.is_ok());
    let (completed, completion) = tokio::sync::oneshot::channel();
    drop(restart_response(Bytes::new(), completed));
    assert!(completion.await.is_err());
}

#[test]
fn frontend_shell_contract_boot_diagnostics_accept_only_structured_metadata() {
    let value =
        json!({"stage":"bootstrap-consumed", "native":true, "injected":false, "mounted":false});
    assert!(serde_json::from_value::<BootDiagnostic>(value.clone()).is_ok());
    let mut unsafe_value = value;
    unsafe_value["token"] = json!("opaque-fixture");
    assert!(serde_json::from_value::<BootDiagnostic>(unsafe_value).is_err());
    assert!(serde_json::from_value::<BootDiagnostic>(
        json!({"stage":"arbitrary", "native":true, "injected":false, "mounted":false})
    )
    .is_err());
}

#[test]
fn frontend_shell_contract_chat_scroll_diagnostic_metadata() {
    let value = json!({"stage":"chat-scroll", "native":false, "injected":true, "mounted":true,
        "chat":{"scope":"tab:session", "edge":"window", "at":42,
            "state":{"newerCount":2, "followTail":false}}});
    let body = serde_json::from_value::<BootDiagnostic>(value).unwrap();
    assert!(matches!(body.stage, DiagnosticStage::ChatScroll));
    assert_eq!(body.chat.unwrap()["state"]["newerCount"], 2);
}
