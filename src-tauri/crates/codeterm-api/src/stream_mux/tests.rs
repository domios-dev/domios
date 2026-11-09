use super::*;
use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderValue, Method},
    routing::{get, post},
    Extension, Router,
};
use codeterm_service_core::service::test_fixture::ServiceFixture;
use futures_util::StreamExt;
use serde_json::Value;
use std::{sync::atomic::AtomicUsize, time::Duration};
use tokio::sync::Notify;
use tower::ServiceExt;

mod cancellation;
mod liveness;

fn app(domains: Router<crate::routes::AppState>, state: crate::routes::AppState) -> Router {
    crate::routes::apply_middleware(domains.merge(super::router()), state)
}

fn fixture() -> (ServiceFixture, crate::routes::AppState) {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("mux-test-token".into());
    (fixture, state)
}

fn headers() -> HeaderMap {
    HeaderMap::from_iter([
        (
            "authorization".parse().unwrap(),
            HeaderValue::from_static("Bearer mux-test-token"),
        ),
        (
            "x-codeterm-client-id".parse().unwrap(),
            HeaderValue::from_static("fixture-client"),
        ),
    ])
}

fn request(session: &str, id: &str, method: &str, path: &str, body: Body) -> Request {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/stream-mux/{session}/requests/{id}"))
        .header("x-domios-mux-method", method)
        .header("x-domios-mux-path", path)
        .header("x-domios-mux-class", "interactive")
        .header("x-domios-mux-cancellable", "1")
        .header("content-type", "application/json")
        .body(body)
        .unwrap();
    request.headers_mut().extend(headers());
    request
}

async fn open() -> (
    String,
    impl futures_core::Stream<Item = Result<Bytes, axum::Error>> + Unpin,
) {
    let id = format!("test-{}", uuid::Uuid::new_v4().simple());
    let response = stream_mux_open(
        headers(),
        Query(MuxSessionQuery {
            session: id.clone(),
        }),
    )
    .await
    .unwrap();
    let mut body = response.into_body().into_data_stream();
    assert_eq!(body.next().await.unwrap().unwrap(), ": mux\n\n");
    (id, body)
}

async fn frame(
    body: &mut (impl futures_core::Stream<Item = Result<Bytes, axum::Error>> + Unpin),
) -> Value {
    let bytes = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .expect("frame deadline")
        .unwrap()
        .unwrap();
    assert!(bytes.len() <= scheduler::model::FRAME_CAP);
    serde_json::from_slice(&bytes[6..bytes.len() - 2]).unwrap()
}

#[tokio::test]
async fn ack_precedes_domain_completion_and_status_headers_and_body_use_mux() {
    let (_fixture, state) = fixture();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let router = app(
        Router::new().route(
            "/api/fixture",
            post({
                let entered = entered.clone();
                let release = release.clone();
                move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        (
                            StatusCode::CREATED,
                            [("x-fixture", "retained"), ("set-cookie", "never-on-wire")],
                            "hello 雪",
                        )
                    }
                }
            }),
        ),
        state,
    );
    let (session, mut body) = open().await;
    let ack = tokio::time::timeout(
        Duration::from_secs(2),
        router.oneshot(request(
            &session,
            "r",
            "POST",
            "/api/fixture",
            Body::empty(),
        )),
    )
    .await
    .expect("ack must not await handler")
    .unwrap();
    assert_eq!(ack.status(), StatusCode::ACCEPTED);
    entered.notified().await;
    release.notify_one();
    let head = frame(&mut body).await;
    assert_eq!(head["status"], 201);
    assert!(head["headers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|pair| pair[0] == "x-fixture" && pair[1] == "retained"));
    assert!(!head.to_string().contains("never-on-wire"));
    let chunk = frame(&mut body).await;
    assert_eq!(chunk["chunk"], "hello 雪");
    assert_eq!(chunk["enc"], "utf8");
    assert_eq!(frame(&mut body).await["end"]["reason"], "eof");
}

#[tokio::test]
async fn envelope_dispatch_reenters_auth_and_propagates_startup_and_inner_limits() {
    let (_fixture, state) = fixture();
    let router = app(Router::new()
        .route("/api/auth-fixture", get(|| async { "secret" }))
        .route("/api/starting-fixture", post(|| async {
            (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error":{"code":"starting","subsystem":"fixture","request_executed":false}})))
        }))
        .route("/api/body-fixture", post(|body: String| async move { body }).layer(axum::extract::DefaultBodyLimit::max(4))), state.clone());
    let dispatcher = dispatch::Dispatcher::new(router.clone(), state.svc.clone());
    let (session, mut body) = open().await;
    let mut unauthenticated = request(&session, "auth", "GET", "/api/auth-fixture", Body::empty());
    unauthenticated.headers_mut().remove("authorization");
    let ack = stream_mux_request(
        Extension(dispatcher),
        Path((session.clone(), "auth".into())),
        unauthenticated,
    )
    .await
    .unwrap();
    assert_eq!(ack.0, StatusCode::ACCEPTED);
    assert_eq!(frame(&mut body).await["status"], 401);
    while frame(&mut body).await.get("end").is_none() {}
    for (id, path, bytes, expected) in [
        ("starting", "/api/starting-fixture", "", 503),
        ("limit", "/api/body-fixture", "too much", 413),
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(request(&session, id, "POST", path, Body::from(bytes)))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
        assert_eq!(frame(&mut body).await["status"], expected);
        let mut payload = String::new();
        loop {
            let frame = frame(&mut body).await;
            if frame.get("end").is_some() {
                break;
            }
            if let Some(chunk) = frame["chunk"].as_str() {
                payload.push_str(chunk);
            }
        }
        if expected == 503 {
            assert!(payload.contains("\"request_executed\":false"));
        }
    }
}

#[tokio::test]
async fn predispatch_refusals_bind_owner_reject_recursion_duplicates_unknown_and_size() {
    let (_fixture, state) = fixture();
    let hold = Arc::new(Notify::new());
    let router = app(
        Router::new().route(
            "/api/held",
            get({
                let hold = hold.clone();
                move || {
                    let hold = hold.clone();
                    async move {
                        hold.notified().await;
                        "done"
                    }
                }
            }),
        ),
        state,
    );
    let (session, body) = open().await;
    for (id, method, path, status) in [
        ("recursive", "POST", "/api/%73tream-mux", 400),
        ("held", "GET", "/api/held", 202),
        ("held", "GET", "/api/held", 409),
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(request(&session, id, method, path, Body::empty()))
                .await
                .unwrap()
                .status()
                .as_u16(),
            status
        );
    }
    let mut foreign = request(&session, "foreign", "GET", "/api/held", Body::empty());
    foreign
        .headers_mut()
        .insert("x-codeterm-client-id", HeaderValue::from_static("other"));
    assert_eq!(
        router.clone().oneshot(foreign).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let mut large = request(&session, "large", "POST", "/api/held", Body::empty());
    large.headers_mut().insert(
        "content-length",
        HeaderValue::from_str(&(spool::ENVELOPE_LIMIT + 1).to_string()).unwrap(),
    );
    assert_eq!(
        router.clone().oneshot(large).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("unknown", "u", "GET", "/api/held", Body::empty()))
            .await
            .unwrap()
            .status(),
        StatusCode::GONE
    );
    for index in 0..63 {
        assert_eq!(
            router
                .clone()
                .oneshot(request(
                    &session,
                    &format!("held-{index}"),
                    "GET",
                    "/api/held",
                    Body::empty()
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
    }
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                &session,
                "over-cap",
                "GET",
                "/api/held",
                Body::empty()
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let removed = SESSIONS.lock().get(&session).unwrap().removed.clone();
    drop(body);
    tokio::time::timeout(Duration::from_secs(2), removed.notified())
        .await
        .unwrap();
    assert!(!SESSIONS.lock().contains_key(&session));
}

#[test]
fn binary_and_escaped_text_frames_respect_wire_cap_and_filter_authority_headers() {
    let id = "c".repeat(64);
    let bytes = Bytes::from(vec![0xff; spool::IO_CHUNK]);
    let binary = frames::encode_chunk(&id, &bytes, false, 0);
    assert!(binary.len() <= scheduler::model::FRAME_CAP);
    let text = frames::encode_chunk(&id, &vec![0; 8 * 1024], true, 0);
    assert!(text.len() <= scheduler::model::FRAME_CAP);
    assert_eq!(spool::UPLOAD_LIMIT, 151 * 1024 * 1024);
    let head = frames::encode_head(
        "c",
        StatusCode::OK,
        &HeaderMap::from_iter([
            (
                "connection".parse().unwrap(),
                HeaderValue::from_static("x-private"),
            ),
            (
                "x-private".parse().unwrap(),
                HeaderValue::from_static("hidden-hop"),
            ),
            (
                "authorization".parse().unwrap(),
                HeaderValue::from_static("hidden-token"),
            ),
            (
                "set-cookie".parse().unwrap(),
                HeaderValue::from_static("hidden-cookie"),
            ),
            (
                "etag".parse().unwrap(),
                HeaderValue::from_static("retained"),
            ),
        ]),
    );
    let wire = String::from_utf8(head.to_vec()).unwrap();
    assert!(!wire.contains("hidden-"));
    assert!(wire.contains("retained"));
}

#[tokio::test]
async fn spooling_replays_exact_bytes_off_runtime_and_enforces_the_envelope_bound() {
    let bytes = vec![42u8; 128 * 1024 + 3];
    let stored = spool::receive(Body::from(bytes.clone()), 256 * 1024)
        .await
        .unwrap();
    assert!(matches!(stored, spool::Stored::File(_)));
    let received = axum::body::to_bytes(stored.into_body(), 256 * 1024)
        .await
        .unwrap();
    assert_eq!(received.as_ref(), bytes);
    let refused = spool::receive(Body::from(vec![0u8; 65]), 64).await;
    assert!(matches!(refused, Err((StatusCode::PAYLOAD_TOO_LARGE, _))));
}

#[test]
fn response_head_advertises_exact_serialized_body_bytes_for_client_buffering() {
    let response = Response::new(Body::from(vec![b'x'; 820_186]));
    let head = frames::encode_response_head("snapshot", &response);
    let wire: Value = serde_json::from_slice(&head[6..head.len() - 2]).unwrap();
    assert!(wire["headers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|pair| pair == &serde_json::json!(["content-length", "820186"])));
}
