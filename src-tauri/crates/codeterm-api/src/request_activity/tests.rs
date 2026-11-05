use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::get,
    Router,
};
use futures_util::FutureExt;
use tower::ServiceExt;

#[tokio::test]
async fn rest_activity_uses_route_templates_and_cleans_up_cancelled_requests() {
    let router = Router::new()
        .route(
            "/activity-fixture/{id}",
            get(|| async { std::future::pending::<StatusCode>().await }),
        )
        .layer(middleware::from_fn(super::record));
    let mut request = Box::pin(
        router.oneshot(
            Request::builder()
                .uri("/activity-fixture/private-value?token=private-secret")
                .header("Authorization", "Bearer private-header")
                .body(Body::empty())
                .unwrap(),
        ),
    );
    assert!(request.as_mut().now_or_never().is_none());
    let snapshot = codeterm_service_core::crash::request_activity::snapshot();
    drop(request);
    assert!(snapshot.contains("/activity-fixture/{id}"));
    assert!(!snapshot.contains("private-value"));
    assert!(!snapshot.contains("private-secret"));
    assert!(!snapshot.contains("private-header"));
    assert!(!codeterm_service_core::crash::request_activity::snapshot()
        .contains("/activity-fixture/{id}"));
}
