use super::*;
use axum::{
    body::{to_bytes, Bytes, HttpBody},
    middleware,
    routing::{get, post},
    Router,
};
use futures_util::{stream, StreamExt};
use tower::ServiceExt;

fn key(client: &str) -> RequestKey {
    RequestKey {
        method: "POST".into(),
        route: "/api/tabs/{id}/chat-raw/start".into(),
        client_id: client.into(),
    }
}

#[test]
fn minute_buckets_separate_clients_and_count_only_observed_bytes() {
    let census = RequestCensus::default();
    let first = key("client-a");
    let second = key("client-b");
    census.observe(10, &first, Observation::Request);
    census.observe(10, &first, Observation::Request);
    census.observe(10, &first, Observation::RequestBytes(7));
    census.observe(10, &first, Observation::ResponseBytes(11));
    census.observe(10, &second, Observation::Request);
    census.observe(11, &first, Observation::ResponseBytes(13));
    let minutes = census.minutes.lock();
    assert_eq!(
        minutes[&10][&first],
        Counts {
            requests: 2,
            request_bytes: 7,
            response_bytes: 11
        }
    );
    assert_eq!(minutes[&10][&second].requests, 1);
    assert_eq!(minutes[&11][&first].requests, 0);
    assert_eq!(minutes[&11][&first].response_bytes, 13);
}

#[test]
fn census_retains_ten_minutes_and_bounds_untrusted_dimensions_without_losing_totals() {
    let census = RequestCensus::default();
    census.observe(10, &key("old"), Observation::Request);
    for index in 0..DIMENSIONS_PER_MINUTE + 3 {
        census.observe(20, &key(&index.to_string()), Observation::Request);
    }
    let minutes = census.minutes.lock();
    assert!(!minutes.contains_key(&10));
    assert_eq!(minutes[&20].len(), DIMENSIONS_PER_MINUTE + 1);
    assert_eq!(
        minutes[&20]
            .values()
            .map(|counts| counts.requests)
            .sum::<u64>(),
        DIMENSIONS_PER_MINUTE as u64 + 3
    );
    assert_eq!(
        minutes[&20]
            .iter()
            .find(|(key, _)| key.route == "<overflow>")
            .unwrap()
            .1
            .requests,
        3
    );
}

#[tokio::test]
async fn router_counts_body_bytes_and_groups_tab_ids_by_matched_route() {
    let census = Arc::new(RequestCensus::default());
    let observed = census.clone();
    let router = Router::new()
        .route(
            "/api/tabs/{id}/chat-raw/start",
            post(|body: Bytes| async move { body }),
        )
        .layer(middleware::from_fn(move |request, next| {
            record_with(request, next, observed.clone())
        }));
    for id in ["one", "two"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tabs/{id}/chat-raw/start?private=opaque"))
                    .header("X-CodeTerm-Client-Id", "client-a")
                    .body(Body::from("opaque"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.body().size_hint().exact(), Some(6));
        assert_eq!(to_bytes(response.into_body(), 64).await.unwrap(), "opaque");
    }
    let snapshot = census.snapshot();
    let entries = snapshot["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["route"], "/api/tabs/{id}/chat-raw/start");
    assert_eq!(entries[0]["client_id"], "client-a");
    assert_eq!(entries[0]["requests"], 2);
    assert_eq!(entries[0]["request_bytes"], 12);
    assert_eq!(entries[0]["response_bytes"], 12);
    assert!(!snapshot.to_string().contains("private"));
}

#[tokio::test]
async fn streaming_body_is_forwarded_before_eof_and_unconsumed_bytes_are_not_counted() {
    let census = Arc::new(RequestCensus::default());
    let observed = census.clone();
    let router = Router::new()
        .route(
            "/stream",
            get(|| async {
                Body::from_stream(
                    stream::iter([
                        Ok::<_, std::io::Error>(Bytes::from_static(b"first")),
                        Ok(Bytes::from_static(b"second")),
                    ])
                    .chain(stream::pending()),
                )
            }),
        )
        .layer(middleware::from_fn(move |request, next| {
            record_with(request, next, observed.clone())
        }));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(census.snapshot()["entries"][0]["response_bytes"], 0);
    let mut stream = response.into_body().into_data_stream();
    assert_eq!(stream.next().await.unwrap().unwrap(), "first");
    drop(stream);
    assert_eq!(census.snapshot()["entries"][0]["response_bytes"], 5);
}

#[test]
fn missing_or_oversized_client_identity_is_one_unknown_dimension() {
    for value in [None, Some("".to_owned()), Some("x".repeat(65))] {
        let mut request = Request::builder();
        if let Some(value) = value {
            request = request.header("x-codeterm-client-id", value);
        }
        assert_eq!(client_id(&request.body(Body::empty()).unwrap()), "unknown");
    }
}
