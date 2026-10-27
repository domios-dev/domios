use super::*;

#[tokio::test]
async fn concierge_capability_refusals_keep_typed_http_403() {
    for reason in [
        "neighbor_memory_not_allowed",
        "neighbor_memory_read_only",
        "neighbor_secrets_not_allowed",
        "neighbor_files_not_allowed",
        "neighbor_capability_denied",
        "neighbor_memory_area_denied",
        "tab_requests_not_allowed",
    ] {
        let response = neighbor_capability_failure(format!("{reason}: opaque"));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["reason"], reason);
        assert_eq!(value["code"], reason);
    }
}

async fn concierge_fixture(
    ready: bool,
) -> (
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
    tab.agent_session_id = ready.then(|| "own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = ready.then(|| "own-nonce".into());
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
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    let token = state
        .svc
        .frontend_pty_env(Some(&concierge))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    (fixture, state, concierge, token)
}

#[tokio::test]
async fn authenticated_http_allowlist_runs_before_every_route_and_custom_auth_exemption() {
    use axum::{body::Body, http::Request, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;
    let (_fixture, state, concierge, token) = concierge_fixture(true).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let downstream = calls.clone();
    let router = Router::new()
        .fallback(move || {
            let calls = downstream.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }
        })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for (method, path) in [
        ("GET", "/api/tabs/foreign/text"),
        ("POST", "/api/tabs/foreign/send"),
        ("PATCH", "/api/config"),
        ("GET", "/api/cron"),
        ("POST", "/api/mesh/relay"),
        ("POST", "/api/mesh/relay-to"),
        ("GET", "/api/mesh/relay-asset/opaque"),
        ("POST", "/api/notifications"),
        ("POST", "/api/plugins/opaque/run"),
        ("POST", "/api/bridge/send"),
        ("GET", "/api/secrets/opaque"),
        ("POST", "/api/mem/items"),
        ("POST", "/api/tabs"),
        ("POST", "/api/tabs/foreign/prompt/respond"),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "downstream called: {path}");
    }
    for path in [
        "/api/mem/search".to_owned(),
        format!("/api/tabs/{concierge}/agent/runtime/questions/opaque/answer"),
    ] {
        let method = if path.contains("/questions/") {
            "POST"
        } else {
            "GET"
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let request = Request::builder()
        .uri("/api/mem/search")
        .header("authorization", format!("Bearer {token}"))
        .header("x-codeterm-tab-id", "foreign")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn own_launch_hooks_work_before_attestation_but_foreign_hooks_and_self_approval_stay_denied()
{
    use axum::{body::Body, http::Request, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tower::ServiceExt;
    let (fixture, state, concierge, token) = concierge_fixture(false).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let downstream = calls.clone();
    let router = Router::new()
        .fallback(move || {
            let calls = downstream.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }
        })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for event in ["session_start", "stop", "context", "permission"] {
        for (target, status) in [
            (concierge.as_str(), StatusCode::NO_CONTENT),
            ("foreign", StatusCode::FORBIDDEN),
        ] {
            let request = Request::builder()
                .method("POST")
                .uri(format!("/api/tabs/{target}/hook/{event}"))
                .header("authorization", format!("Bearer {token}"))
                .header("x-codeterm-binding-nonce", "own-nonce")
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                router.clone().oneshot(request).await.unwrap().status(),
                status
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let wrong_nonce = Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{concierge}/hook/session_start"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-codeterm-binding-nonce", "foreign-nonce")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(wrong_nonce).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&concierge).unwrap();
        tab.agent_session_id = Some("own-session".into());
        tab.reported_session_binding_nonce = Some("own-nonce".into());
    }
    for path in [
        format!("/api/tabs/{concierge}/prompt/respond"),
        format!("/api/tabs/{concierge}/hook/unknown"),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn real_history_route_refuses_other_friendships_without_identity_headers() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let (_fixture, state, _, token) = concierge_fixture(true).await;
    let router = crate::routes::create_router(state);
    for (friendship, status) in [
        ("a".repeat(64), StatusCode::OK),
        ("b".repeat(64), StatusCode::FORBIDDEN),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/api/account/neighbors/runtime")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"operation":"history","friendship_id":friendship,"limit":20})
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            status
        );
    }
}

#[tokio::test]
async fn a_ready_credential_is_revoked_when_its_session_is_removed_before_a_hook() {
    use axum::{body::Body, http::Request, Router};
    use tower::ServiceExt;
    let (fixture, state, concierge, token) = concierge_fixture(true).await;
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&concierge)
        .unwrap()
        .agent_session_id = None;
    let router = Router::new()
        .fallback(|| async { StatusCode::NO_CONTENT })
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{concierge}/hook/session_start"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-codeterm-binding-nonce", "own-nonce")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
