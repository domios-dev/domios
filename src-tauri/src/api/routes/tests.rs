#[cfg(debug_assertions)]
use std::sync::Mutex;

use axum::{
    body::Body,
    http::{header, Method, Request},
    routing::get,
    Router,
};
use tower::ServiceExt;
use tower_http::catch_panic::CatchPanicLayer;

#[cfg(debug_assertions)]
static VITE_PORT_ENV_LOCK: Mutex<()> = Mutex::new(());

#[cfg(debug_assertions)]
fn vite_port_for_instance(instance_name: &str) -> u16 {
    1420 + (instance_name.bytes().map(u16::from).sum::<u16>() % 200) + 1
}

#[cfg(debug_assertions)]
fn app_for_vite_port(vite_port: u16) -> Router {
    let _guard = VITE_PORT_ENV_LOCK.lock().unwrap();
    let previous = std::env::var_os("CODETERM_VITE_PORT");
    unsafe { std::env::set_var("CODETERM_VITE_PORT", vite_port.to_string()) };
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(super::cors_layer(false, &[]));
    match previous {
        Some(value) => unsafe { std::env::set_var("CODETERM_VITE_PORT", value) },
        None => unsafe { std::env::remove_var("CODETERM_VITE_PORT") },
    }
    app
}

#[cfg(debug_assertions)]
async fn preflight(app: Router, origin: &str) -> axum::http::Response<axum::body::Body> {
    app.oneshot(
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

fn allowed_origin(response: &axum::http::Response<axum::body::Body>) -> Option<&str> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|value| value.to_str().ok())
}

#[test]
fn cors_origins_always_include_the_app_webview() {
    let configured = ["https://example.test"]
        .into_iter()
        .filter_map(|origin| origin.parse::<axum::http::HeaderValue>().ok())
        .collect::<Vec<_>>();
    let origins = super::with_app_webview_origins(configured);
    for expected in super::APP_WEBVIEW_ORIGINS {
        assert!(
            origins.iter().any(|origin| origin == expected),
            "{expected} must be allowed: the app window is cross-origin to its own daemon, so without it every raw fetch from the UI is rejected",
        );
    }
    assert!(
        origins
            .iter()
            .any(|origin| origin == "https://example.test"),
        "configured origins must survive the append",
    );
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

#[tokio::test]
#[cfg(debug_assertions)]
async fn derived_origin_admitted_for_playwright_instance() {
    let vite_port = vite_port_for_instance("playwright");
    let origin = format!("http://127.0.0.1:{vite_port}");
    let response = preflight(app_for_vite_port(vite_port), &origin).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(allowed_origin(&response), Some(origin.as_str()));
}

#[tokio::test]
#[cfg(debug_assertions)]
async fn derived_origin_admitted_for_differently_named_instance() {
    let instance_name = "remote-render";
    let vite_port = vite_port_for_instance(instance_name);
    let origin = format!("http://127.0.0.1:{vite_port}");
    let response = preflight(app_for_vite_port(vite_port), &origin).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(allowed_origin(&response), Some(origin.as_str()));
}

#[tokio::test]
#[cfg(debug_assertions)]
async fn derived_origin_admitted_refuses_an_unrelated_origin() {
    let vite_port = vite_port_for_instance("playwright");
    let unrelated_origin = format!("http://127.0.0.1:{}", vite_port + 1);
    let response = preflight(app_for_vite_port(vite_port), &unrelated_origin).await;
    assert!(allowed_origin(&response).is_none());
}

#[tokio::test]
async fn derived_origin_admitted_keeps_release_webview_policy_unchanged() {
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(super::cors_layer(false, &[]));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::ORIGIN, "http://tauri.localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed_origin(&response), Some("http://tauri.localhost"));

    let denied = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::ORIGIN, "https://configured.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(allowed_origin(&denied).is_none());
}
