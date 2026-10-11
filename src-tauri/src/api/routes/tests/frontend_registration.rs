use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

use crate::api::transport_tabs::fixture;

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
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn frontend_bundle_routes_contract_central_bearer_registration() {
    use crate::{
        api::routes::create_router_with_frontend, frontend_bundle::FrontendBundles,
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    use std::sync::Arc;
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let root = tempfile::tempdir().unwrap();
    let bundles = Arc::new(
        FrontendBundles::open(
            root.path(),
            2,
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
