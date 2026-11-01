use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

use crate::transport_tabs::fixture;

#[tokio::test]
async fn frontend_bundle_routes_contract_bundled_index_and_relative_assets() {
    let dist = tempfile::tempdir().unwrap();
    std::fs::create_dir(dist.path().join("assets")).unwrap();
    std::fs::write(
        dist.path().join("index.html"),
        "<main>Bundled shell</main><script src='./assets/app.js'></script>",
    )
    .unwrap();
    std::fs::write(dist.path().join("assets/app.js"), "window.bundled=true;").unwrap();
    let router: axum::Router = super::super::bundled_frontend_routes(dist.path());
    for (path, expected) in [
        (
            "/bundled/index.html",
            "<main>Bundled shell</main><script src='./assets/app.js'></script>",
        ),
        ("/bundled/assets/app.js", "window.bundled=true;"),
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            to_bytes(response.into_body(), 16384)
                .await
                .unwrap()
                .as_ref(),
            expected.as_bytes()
        );
    }
}

#[tokio::test]
async fn missing_frontend_index_returns_branded_self_contained_html() {
    use crate::routes::create_router;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let dist = tempfile::tempdir().unwrap();
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let mut state = fixture::app_state(&fixture);
    state.web_dist = Some(dist.path().to_path_buf());
    let response = create_router(state)
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(response.headers()["cache-control"], "no-cache");
    let html = String::from_utf8(
        to_bytes(response.into_body(), 16_384)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("The interface could not load"));
    assert!(html.contains("M8 8H32C46 8 56 18 56 32S46 56 32 56H24L40 40V24H24V40L8 56Z"));
    assert!(!html.contains("asset not found: index.html"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("https://"));
}

#[tokio::test]
async fn frontend_bundle_routes_contract_central_bearer_registration() {
    use crate::routes::create_router_with_frontend;
    use codeterm_frontend_bundle::FrontendBundles;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use std::sync::Arc;
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let root = tempfile::tempdir().unwrap();
    let bundles = Arc::new(
        FrontendBundles::open(
            root.path(),
            2,
            "0.0.0",
            None,
            "https://example.test/frontend-manifest.json".into(),
            false,
            Arc::new(|_, _| {}),
        )
        .unwrap(),
    );
    let router = create_router_with_frontend(fixture::app_state(&fixture), Some(bundles));
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/frontend/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/frontend/status")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(status["updatesEnabled"], false);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/frontend/check")
                .header("authorization", "Bearer preview-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn root_frontend_resolves_and_legacy_web_is_gone_in_full_router() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let dist = tempfile::tempdir().unwrap();
    std::fs::create_dir(dist.path().join("assets")).unwrap();
    std::fs::write(
        dist.path().join("index.html"),
        "<script src='./assets/app.js'></script>",
    )
    .unwrap();
    std::fs::write(dist.path().join("assets/app.js"), "window.fixture=true;").unwrap();
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let mut state = fixture::app_state(&fixture);
    state.web_dist = Some(dist.path().to_path_buf());
    let router = crate::routes::create_router(state.clone());
    state.web_dist = None;
    let without_dist = crate::routes::create_router(state);
    for (path, expected) in [
        ("/", "<script src='./assets/app.js'></script>"),
        ("/assets/app.js", "window.fixture=true;"),
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), 4096).await.unwrap().as_ref(),
            expected.as_bytes()
        );
    }
    for router in [router, without_dist] {
        for method in ["GET", "HEAD"] {
            for path in ["/web", "/web/", "/web/assets/app.js"] {
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
                assert_eq!(response.status(), StatusCode::NOT_FOUND);
                assert!(to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap()
                    .is_empty());
            }
        }
    }
}
