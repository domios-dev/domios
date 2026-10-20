use super::*;
use axum::{
    http::{Request, StatusCode},
    routing::get,
};
use tower::ServiceExt;

#[tokio::test]
async fn listener_restart_contract_ends_idle_and_relay_sse_bodies_cleanly() {
    let generation = GenerationStreams::new();
    let router = generation.router(
        Router::new()
            .route(
                "/sse",
                get(|| async {
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        Body::from_stream(futures_util::stream::pending::<
                            Result<axum::body::Bytes, std::io::Error>,
                        >()),
                    )
                }),
            )
            .route("/json", get(|| async { "retained response" })),
    );
    let first = router
        .clone()
        .oneshot(Request::builder().uri("/sse").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let relay = router
        .clone()
        .oneshot(Request::builder().uri("/sse").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let ordinary = router
        .clone()
        .oneshot(Request::builder().uri("/json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    generation.close();
    for body in [first.into_body(), relay.into_body()] {
        let mut stream = body.into_data_stream();
        assert!(stream.next().await.is_none());
    }
    assert_eq!(
        axum::body::to_bytes(ordinary.into_body(), 1024)
            .await
            .unwrap(),
        "retained response"
    );
    let late = router
        .oneshot(Request::builder().uri("/sse").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(late.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn listener_restart_contract_shutdown_wins_over_buffered_items_and_is_generation_scoped() {
    let old = GenerationStreams::new();
    let new = GenerationStreams::new();
    let old_stream = until_closed(futures_util::stream::iter([1, 2]), old.0.subscribe());
    let new_stream = until_closed(futures_util::stream::iter([3, 4]), new.0.subscribe());
    tokio::pin!(old_stream, new_stream);
    assert_eq!(old_stream.next().await, Some(1));
    old.close();
    assert_eq!(old_stream.next().await, None);
    assert_eq!(new_stream.collect::<Vec<_>>().await, vec![3, 4]);
}
