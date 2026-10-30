use super::*;

async fn bound_fixture() -> (
    codeterm_service_core::service::test_fixture::ServiceFixture,
    AppState,
    String,
    String,
) {
    let (mut mux, general) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(
                serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
            )
            .unwrap(),
        )
        .await;
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let token = state
        .svc
        .frontend_pty_env(Some(&concierge))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    (fixture, state, concierge, token)
}

async fn request(
    router: axum::Router,
    token: &str,
    command: serde_json::Value,
) -> axum::response::Response {
    use tower::ServiceExt;
    router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/account/neighbors/files")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(command.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn authenticated_concierge_can_resolve_only_own_verified_saved_files_and_revocation_stops_reads(
) {
    let (fixture, state, concierge, token) = bound_fixture().await;
    let friendship = "a".repeat(64);
    let file = "1".repeat(32);
    let path = fixture
        .seed_neighbor_saved_file(&friendship, &file, b"opaque")
        .await;
    let other = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&concierge).unwrap();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some("other-session".into());
        tab.session_binding_nonce = Some("other-nonce".into());
        tab.reported_session_binding_nonce = Some("other-nonce".into());
        id
    };
    fixture
        .seed_neighbor_concierge(
            &other,
            &"b".repeat(64),
            serde_json::from_value(
                serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
            )
            .unwrap(),
        )
        .await;
    let foreign_path = fixture
        .seed_neighbor_saved_file(&"b".repeat(64), &file, b"private")
        .await;
    let router = crate::routes::create_router(state);
    let response = request(
        router.clone(),
        &token,
        serde_json::json!({"operation":"list","friendship_id":friendship}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["files"].as_array().unwrap().len(), 1);
    assert!(value.get("folder").is_none());
    let response = request(
        router.clone(),
        &token,
        serde_json::json!({"operation":"open","friendship_id":friendship,"file_id":file}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        std::path::Path::new(value["path"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        path.canonicalize().unwrap()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"opaque");
    for operation in [
        serde_json::json!({"operation":"list","friendship_id":"b".repeat(64)}),
        serde_json::json!({"operation":"open","friendship_id":"b".repeat(64),"file_id":file}),
        serde_json::json!({"operation":"open_folder","friendship_id":friendship}),
        serde_json::json!({"operation":"send","friendship_id":friendship,"kind":"question","body":"opaque","paths":[path]}),
        serde_json::json!({"operation":"set_daily_limit","friendship_id":friendship,"file_daily_limit_bytes":null}),
    ] {
        assert_eq!(
            request(router.clone(), &token, operation).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    fixture.seed_neighbor_concierge(&concierge, &friendship, serde_json::from_value(serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false,"allow_files":false})).unwrap()).await;
    for operation in [
        serde_json::json!({"operation":"list","friendship_id":friendship}),
        serde_json::json!({"operation":"open","friendship_id":friendship,"file_id":file}),
    ] {
        let response = request(router.clone(), &token, operation).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(std::fs::read(path).unwrap(), b"opaque");
    assert_eq!(std::fs::read(foreign_path).unwrap(), b"private");
}

#[tokio::test]
async fn authenticated_file_route_refuses_path_components_before_local_file_access() {
    let (_fixture, state, _, token) = bound_fixture().await;
    let router = crate::routes::create_router(state);
    for file in [
        "../escape",
        "/absolute",
        "\\\\server\\share",
        "C:\\private\\item",
        "CON",
        "NUL",
        "a/b",
        "a\\b",
        "a:stream",
    ] {
        let response = request(
            router.clone(),
            &token,
            serde_json::json!({"operation":"open","friendship_id":"a".repeat(64),"file_id":file}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{file}");
    }
}

#[tokio::test]
async fn file_refusals_preserve_quota_fields_and_http_status() {
    for (reason, status) in [
        ("file_too_large", StatusCode::PAYLOAD_TOO_LARGE),
        ("sender_quota", StatusCode::TOO_MANY_REQUESTS),
        ("file_daily_limit", StatusCode::TOO_MANY_REQUESTS),
        ("files_disabled", StatusCode::SERVICE_UNAVAILABLE),
        ("file_not_ready", StatusCode::CONFLICT),
        ("file_not_found", StatusCode::NOT_FOUND),
    ] {
        let response = neighbor_failure(format!("{reason}: {{\"reason\":\"{reason}\",\"limit_bytes\":52428800,\"limit_files\":25,\"resets_at\":1234}}"));
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
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
    let response = crate::account_routes::neighbor_files(State(state), None, Json(input))
        .await
        .unwrap_err();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
