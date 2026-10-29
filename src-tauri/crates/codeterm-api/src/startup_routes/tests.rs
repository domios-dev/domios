use super::*;
use axum::body::{to_bytes, Body};
use codeterm_service_core::{lock_hold::TimedRwLock, multiplexer::state::MultiplexerState};
use parking_lot::Mutex;
use tower::ServiceExt;

#[tokio::test]
async fn desktop_api_requires_owner_bearer_and_keeps_unknown_areas_starting() {
    let (mux, _) = MultiplexerState::new();
    let mut config = codeterm_service_core::config::schema::AppConfig::default();
    config.api.token = Some("fixture-owner".into());
    let events = Arc::new(codeterm_events::EventBroadcaster::new());
    let app = desktop_api(Desktop {
        desk: StartupDesk::new(Arc::new(TimedRwLock::new("mux", mux)), events.clone()),
        config: Arc::new(Mutex::new(config)),
        events,
        storage_error: None,
        account: None,
    });
    for token in [None, Some("foreign"), Some("fixture-owner")] {
        let mut request = axum::http::Request::builder().uri("/api/mux/state");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if token == Some("fixture-owner") {
                StatusCode::OK
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
    }
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/account/status")
                .header("authorization", "Bearer fixture-owner")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(value["error"]["code"], "starting");
    assert_eq!(value["error"]["subsystem"], "account");
}

#[tokio::test]
async fn root_frontend_resolves_and_legacy_web_is_gone_before_service_readiness() {
    let dist = tempfile::tempdir().unwrap();
    std::fs::create_dir(dist.path().join("assets")).unwrap();
    std::fs::write(
        dist.path().join("index.html"),
        "<script src='./assets/app.js'></script>",
    )
    .unwrap();
    std::fs::write(dist.path().join("assets/app.js"), "window.fixture=true;").unwrap();
    let (mux, _) = MultiplexerState::new();
    let mut config = codeterm_service_core::config::schema::AppConfig::default();
    config.api.token = Some("fixture-owner".into());
    let events = Arc::new(codeterm_events::EventBroadcaster::new());
    let desktop = Desktop {
        desk: StartupDesk::new(Arc::new(TimedRwLock::new("mux", mux)), events.clone()),
        config: Arc::new(Mutex::new(config)),
        events,
        storage_error: None,
        account: None,
    };
    let router = desktop_routes(desktop.clone(), Some(dist.path().to_path_buf()));
    let without_dist = desktop_routes(desktop, None);
    for (path, expected) in [
        ("/", "<script src='./assets/app.js'></script>"),
        ("/assets/app.js", "window.fixture=true;"),
    ] {
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 4096).await.unwrap().as_ref(),
            expected.as_bytes()
        );
    }

    for router in [router.clone(), without_dist] {
        for method in ["GET", "HEAD"] {
            for path in ["/web", "/web/", "/web/assets/app.js"] {
                let response = router
                    .clone()
                    .oneshot(
                        axum::http::Request::builder()
                            .method(method)
                            .uri(path)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::NOT_FOUND);
                assert!(to_bytes(response.into_body(), 4096).await.unwrap().is_empty());
            }
        }
    }
    let response = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
