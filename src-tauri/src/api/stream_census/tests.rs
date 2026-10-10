use super::*;
use axum::{body::Body, extract::Query, http::Request, routing::get, Json, Router};
use futures_util::{stream, FutureExt};
use tower::ServiceExt;

fn assert_closed(census: &StreamCensus, opened: u64) {
    assert_eq!(
        census.snapshot().totals,
        Counts {
            live: 0,
            opened,
            closed: opened
        }
    );
}

#[test]
fn guard_drop_and_panic_unwind_close_every_kind_and_scope() {
    let census = StreamCensus::default();
    let mut opened = 0;
    for kind in StreamKind::ALL {
        for scope in [
            StreamScope::Local,
            StreamScope::PeerRelayed {
                peer_instance_id: "peer-a".into(),
            },
        ] {
            {
                let _guard = census.open(kind, scope.clone());
                assert_eq!(census.snapshot().totals.live, 1);
            }
            opened += 1;
            assert_closed(&census, opened);
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = census.open(kind, scope);
                panic!("stream owner unwound");
            }));
            assert!(unwind.is_err());
            opened += 1;
            assert_closed(&census, opened);
        }
    }
}

#[tokio::test]
async fn normal_end_error_disconnect_and_unpolled_cancel_release_guards() {
    let census = StreamCensus::default();
    let completed = track(
        stream::iter([1, 2]),
        census.open(StreamKind::Events, StreamScope::Local),
    );
    let mut completed = Box::pin(completed);
    assert_eq!(completed.next().await, Some(1));
    assert_eq!(completed.next().await, Some(2));
    assert_eq!(completed.next().await, None);
    assert_closed(&census, 1);

    let unpolled = track(
        stream::pending::<()>(),
        census.open(StreamKind::Events, StreamScope::Local),
    );
    assert_eq!(census.snapshot().totals.live, 1);
    drop(unpolled);
    assert_closed(&census, 2);

    let mut pending = Box::pin(track(
        stream::pending::<()>(),
        census.open(StreamKind::Surface, StreamScope::Local),
    ));
    assert!(pending.next().now_or_never().is_none());
    drop(pending);
    assert_closed(&census, 3);

    let error = track(
        stream::iter([Err::<(), _>("upstream closed")]),
        census.open(StreamKind::Output, StreamScope::Local),
    );
    let mut error = Box::pin(error);
    assert_eq!(error.next().await, Some(Err("upstream closed")));
    drop(error);
    assert_closed(&census, 4);
}

#[tokio::test]
async fn task_abort_drops_the_stream_guard() {
    let census = StreamCensus::default();
    let guarded = track(
        stream::pending::<()>(),
        census.open(StreamKind::Output, StreamScope::Local),
    );
    let (ready, started) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut guarded = Box::pin(guarded);
        assert!(guarded.next().now_or_never().is_none());
        ready.send(()).unwrap();
        guarded.next().await;
    });
    started.await.unwrap();
    assert_eq!(census.snapshot().totals.live, 1);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_closed(&census, 1);
}

#[tokio::test]
async fn panic_while_polling_releases_the_guard_on_unwind() {
    let census = StreamCensus::default();
    let guarded = track(
        stream::poll_fn(|_| -> std::task::Poll<Option<()>> {
            panic!("upstream poll panicked");
        }),
        census.open(StreamKind::Events, StreamScope::Local),
    );
    let result = std::panic::AssertUnwindSafe(async move {
        let mut guarded = Box::pin(guarded);
        guarded.next().await;
    })
    .catch_unwind()
    .await;
    assert!(result.is_err());
    assert_closed(&census, 1);
}

#[test]
fn snapshots_keep_peer_identity_and_monotonic_totals_after_close() {
    let census = StreamCensus::default();
    let a = census.open(
        StreamKind::Events,
        StreamScope::PeerRelayed {
            peer_instance_id: "a".into(),
        },
    );
    let b = census.open(
        StreamKind::Events,
        StreamScope::PeerRelayed {
            peer_instance_id: "b".into(),
        },
    );
    let value = serde_json::to_value(census.snapshot()).unwrap();
    assert_eq!(value["live"], 2);
    let entries = value["entries"].as_array().unwrap();
    for peer in ["a", "b"] {
        let entry = entries
            .iter()
            .find(|entry| entry["peer_instance_id"] == peer)
            .unwrap();
        assert_eq!(entry["scope"], "peer_relayed");
        assert_eq!(entry["kind"], "events");
        assert_eq!(entry["live"], 1);
    }
    drop((a, b));
    assert_closed(&census, 2);
    assert_eq!(census.snapshot().entries.len(), 6);
}

#[tokio::test]
async fn events_router_counts_response_before_poll_and_client_drop() {
    let census = Arc::new(StreamCensus::default());
    let events = Arc::new(crate::api::events::EventBroadcaster::new());
    let publisher = events.clone();
    let stream_census = census.clone();
    let stats_census = census.clone();
    let router = Router::new()
        .route(
            "/api/events/stream",
            get(
                move |Query(query): Query<crate::api::web_routes::EventsStreamQuery>| {
                    let events = events.clone();
                    let census = stream_census.clone();
                    async move { crate::api::web_routes::events_sse(events, query, &census) }
                },
            ),
        )
        .route(
            "/api/debug/events/stats",
            get(move || {
                let census = stats_census.clone();
                async move {
                    Json(crate::api::debug_routes::event_stats_snapshot(
                        &crate::service::MuxEmitStats::default(),
                        false,
                        0,
                        &census,
                    ))
                }
            }),
        );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/events/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let stats = router
        .oneshot(
            Request::builder()
                .uri("/api/debug/events/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(stats.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["streams"]["live"], 1);
    assert_eq!(value["streams"]["opened"], 1);
    assert_eq!(value["streams"]["closed"], 0);
    publisher.publish_generic("census-probe", serde_json::json!({ "probe": "delivered" }));
    let mut client = response.into_body().into_data_stream();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(1), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = std::str::from_utf8(&frame).unwrap();
    assert!(text.contains("census-probe"));
    assert!(text.contains("delivered"));
    assert_eq!(census.snapshot().totals.live, 1);
    drop(client);
    assert_closed(&census, 1);
}

#[test]
fn concurrent_peer_opens_share_one_counter_and_balance_after_join() {
    let census = StreamCensus::default();
    let opened = std::sync::Barrier::new(9);
    let release = std::sync::Barrier::new(9);
    std::thread::scope(|threads| {
        for _ in 0..8 {
            threads.spawn(|| {
                let _guard = census.open(
                    StreamKind::Events,
                    StreamScope::PeerRelayed {
                        peer_instance_id: "shared-peer".into(),
                    },
                );
                opened.wait();
                release.wait();
            });
        }
        opened.wait();
        let snapshot = census.snapshot();
        release.wait();
        assert_eq!(
            snapshot.totals,
            Counts {
                live: 8,
                opened: 8,
                closed: 0
            }
        );
        assert_eq!(snapshot.entries.len(), 5);
    });
    assert_closed(&census, 8);
}
