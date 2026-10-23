use axum::{
    body::Body,
    http::{header, Method, Request},
    routing::get,
    Router,
};
use tower::ServiceExt;
use tower_http::catch_panic::CatchPanicLayer;

mod frontend_registration;
mod transport_registration;

fn cors_app() -> Router {
    Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(super::cors_layer(
            &["http://127.0.0.1:1463".into(), "tauri://localhost".into()],
            &[],
        ))
}

async fn preflight(origin: &str, method: &str, headers: &str) -> axum::response::Response {
    preflight_on(cors_app(), origin, method, headers).await
}

async fn preflight_on(
    app: Router,
    origin: &str,
    method: &str,
    headers: &str,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, method)
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, headers)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

fn allowed_origin(response: &axum::response::Response) -> Option<&str> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|value| value.to_str().ok())
}

#[tokio::test]
async fn frontend_cors_authorization_and_binding_preflight_passes_without_wildcards() {
    for origin in ["http://127.0.0.1:1463", "tauri://localhost"] {
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            let response = preflight(
                origin,
                method,
                "authorization,content-type,x-codeterm-tab-id,x-codeterm-binding-nonce",
            )
            .await;
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            assert_eq!(allowed_origin(&response), Some(origin));
            let headers = response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
                .to_str()
                .unwrap();
            for expected in super::FRONTEND_REQUEST_HEADERS {
                assert!(headers.split(',').any(|value| value.trim() == expected));
            }
            let methods = response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS]
                .to_str()
                .unwrap();
            assert!(methods.split(',').any(|value| value.trim() == method));
            assert!(!headers.contains('*'));
            assert!(!methods.contains('*'));
        }
    }
}

#[tokio::test]
async fn frontend_cors_foreign_origin_is_refused() {
    for origin in [
        "http://127.0.0.1:1464",
        "http://localhost:1463",
        "https://foreign.example",
    ] {
        let response = preflight(origin, "GET", "authorization,content-type").await;
        assert!(allowed_origin(&response).is_none());
    }
}

#[tokio::test]
async fn frontend_cors_undeclared_method_and_header_are_refused() {
    let response = preflight("http://127.0.0.1:1463", "TRACE", "x-foreign").await;
    let headers = response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
        .to_str()
        .unwrap();
    assert!(!headers.split(',').any(|value| value.trim() == "x-foreign"));
    let methods = response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS]
        .to_str()
        .unwrap();
    assert!(!methods.split(',').any(|value| value.trim() == "TRACE"));
}

#[tokio::test]
async fn catch_panic_layer_keeps_router_responsive_after_handler_panic() {
    let app = Router::new()
        .route(
            "/panic",
            get(|| async {
                panic!("synthetic handler panic");
                #[allow(unreachable_code)]
                "never"
            }),
        )
        .route("/health", get(|| async { "ok" }))
        .layer(CatchPanicLayer::new());

    let panic_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/panic")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("panic should be converted to HTTP 500 response");
    assert_eq!(
        panic_response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );

    let health_response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router must keep serving requests after a handler panic");
    assert_eq!(health_response.status(), axum::http::StatusCode::OK);
}

#[test]
fn frontend_cors_configured_entries_require_exact_http_origins() {
    assert!(super::configured_origin("https://trusted.example").is_some());
    assert!(super::configured_origin("http://127.0.0.1:1463").is_some());
    for entry in [
        "*",
        "https://*.example",
        "https://trusted.example/",
        "https://trusted.example/path",
        "https://trusted.example?token=secret",
        "https://trusted.example#secret",
        "https://user:secret@trusted.example",
        "tauri://localhost",
        "null",
        " https://trusted.example",
    ] {
        assert!(
            super::configured_origin(entry).is_none(),
            "non-origin entry admitted"
        );
    }
}

#[tokio::test]
async fn frontend_cors_configured_origin_is_exact_and_invalid_entries_never_become_wildcards() {
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(super::cors_layer(
            &["http://127.0.0.1:1463".into()],
            &[
                "https://trusted.example".into(),
                "*".into(),
                "https://foreign.example/path".into(),
            ],
        ));
    let allowed = preflight_on(
        app.clone(),
        "https://trusted.example",
        "GET",
        "authorization,content-type",
    )
    .await;
    assert_eq!(allowed_origin(&allowed), Some("https://trusted.example"));
    for origin in [
        "https://foreign.example",
        "https://sub.trusted.example",
        "http://trusted.example",
    ] {
        let denied = preflight_on(app.clone(), origin, "GET", "authorization,content-type").await;
        assert!(allowed_origin(&denied).is_none());
    }
}

#[tokio::test]
async fn orphan_reopen_route_preserves_typed_plan_refusal() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&first).unwrap();
    let orphan = mux
        .create_workspace("Orphan".into(), Some("/orphan".into()))
        .id;
    let closed = mux.create_tab(&orphan).unwrap();
    mux.close_workspace(&orphan).unwrap();
    for _ in 0..34 {
        mux.create_tab(&workspace).unwrap();
    }
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-owner".into());
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", "Bearer fixture-owner".parse().unwrap());
    let (status, axum::Json(body)) = super::restore_orphaned_tab(
        axum::extract::State(state),
        headers,
        axum::Json(super::RestoreOrphanedTab {
            tab_id: closed.clone(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "free_limit_reached");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with("free_limit_reached: "));
    assert_eq!(fixture.service.mux.read().all_tabs().count(), 35);
    assert!(fixture
        .service
        .mux
        .read()
        .snapshot()
        .recently_closed
        .iter()
        .any(|row| row.tab.tab_id == closed));
}

#[tokio::test]
async fn locked_pty_spawn_returns_typed_403_without_a_process() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    for _ in 0..35 {
        mux.create_tab(&workspace).unwrap();
    }
    let tab = mux.locks().agent_tabs.iter().next().unwrap().to_owned();
    let fixture = ServiceFixture::new(mux);
    let app = Router::new()
        .route(
            "/api/pty/spawn",
            axum::routing::post(crate::pty_routes::pty_spawn),
        )
        .layer(axum::middleware::from_fn(
            crate::plan_quota_status::typed_plan_refusal,
        ))
        .with_state(crate::test_support::app_state(&fixture));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/pty/spawn")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tab_id": tab, "predicted_id": "refused-http-pty", "cols": 80, "rows": 24,
                        "command": "must-not-run"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "plan_limit_locked");
    assert!(fixture
        .service
        .pty
        .lock()
        .get_session("refused-http-pty")
        .is_none());
    assert!(fixture.service.pty_ownership.launch_claim(&tab).is_none());
}
