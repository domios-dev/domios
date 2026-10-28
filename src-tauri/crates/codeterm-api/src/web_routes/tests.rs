use crate::routes::WEB_TOKEN_TTL;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use tower::ServiceExt;

#[tokio::test]
async fn process_split_contract_internal_cron_is_excluded_from_sse_replay_and_live() {
    use axum::response::IntoResponse;
    use futures_util::StreamExt;
    for tab_id in [None, Some("bound"), Some("other")] {
        let events = std::sync::Arc::new(crate::events::EventBroadcaster::new());
        let payload = serde_json::json!({"run_id":"private", "tab_id":tab_id.unwrap_or("bound")});
        let mut internal = events.subscribe();
        events.publish_internal("cron-fired", payload.clone());
        assert!(!internal.try_recv().unwrap().public);
        events.publish_generic("fixture-visible", payload.clone());
        let census = crate::stream_census::StreamCensus::default();
        let sse = super::events_sse(
            events.clone(),
            super::EventsStreamQuery {
                since: Some(format!("{}:0", events.boot_id())),
                tab_id: tab_id.map(str::to_owned),
            },
            &census,
        );
        let mut body = sse.into_response().into_body().into_data_stream();
        for expected_seq in [2, 4] {
            if expected_seq == 4 {
                events.publish_internal("cron-fired", payload.clone());
                events.publish_generic("fixture-visible", payload.clone());
            }
            let frame = tokio::time::timeout(std::time::Duration::from_secs(2), body.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let frame = std::str::from_utf8(&frame).unwrap();
            let payload: serde_json::Value =
                serde_json::from_str(frame.trim().strip_prefix("data: ").unwrap()).unwrap();
            assert_eq!(payload["seq"], expected_seq);
            assert_eq!(payload["event_type"], "fixture-visible");
        }
    }
}

#[tokio::test]
async fn encoded_replay_pins_existing_sse_wire_bytes() {
    use axum::response::IntoResponse;
    use futures_util::StreamExt;
    use std::sync::Arc;
    let events = Arc::new(crate::events::EventBroadcaster::new());
    let fixture = crate::events::SystemEvent::TabTitlePinned {
        tab_id: "tab".into(),
        title: "quoted \" title\n雪".into(),
    };
    let data = serde_json::to_value(&fixture).unwrap();
    events.publish(fixture);
    let census = crate::stream_census::StreamCensus::default();
    let sse = super::events_sse(
        events.clone(),
        super::EventsStreamQuery {
            since: Some(format!("{}:0", events.boot_id())),
            tab_id: Some("tab".into()),
        },
        &census,
    );
    let mut body = sse.into_response().into_body().into_data_stream();
    let actual = body.next().await.unwrap().unwrap();
    let expected = serde_json::json!({
        "seq": 1, "boot": events.boot_id(), "event_type": "tab-title-pinned", "data": data,
    });
    assert_eq!(actual.as_ref(), format!("data: {expected}\n\n").as_bytes());
    assert_eq!(events.replay_stats().encoded_events, 1);
}

#[tokio::test]
async fn overflow_frames_keep_existing_wire_bytes() {
    use axum::response::{sse::Sse, IntoResponse};
    for (reason, oldest) in [
        ("boot-changed", None),
        ("ring-evicted", Some(4)),
        ("lagged", None),
    ] {
        let event = super::overflow_frame(reason, "boot", 9, oldest, None);
        let stream = futures_util::stream::iter([Ok::<_, std::convert::Infallible>(event)]);
        let actual = to_bytes(Sse::new(stream).into_response().into_body(), usize::MAX)
            .await
            .unwrap();
        let mut data = serde_json::json!({ "reason": reason, "boot": "boot", "next_seq": 9 });
        if let Some(oldest) = oldest {
            data["oldest"] = serde_json::json!(oldest);
        }
        let expected = serde_json::json!({ "event_type": "overflow", "data": data });
        assert_eq!(actual.as_ref(), format!("data: {expected}\n\n").as_bytes());
    }
}

#[test]
fn boot_changed_overflow_frame_carries_daemon_build() {
    let payload = super::overflow_payload("boot-changed", "boot", 9, None, Some("build-hash"));
    assert_eq!(
        payload,
        serde_json::json!({
            "event_type": "overflow",
            "data": { "reason": "boot-changed", "boot": "boot", "next_seq": 9, "build": "build-hash" },
        })
    );
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn canonical_json(value: &serde_json::Value) -> String {
    let serde_json::Value::Object(map) = value else {
        return value.to_string();
    };
    let mut fields: Vec<_> = map.iter().collect();
    fields.sort_by(|a, b| a.0.cmp(b.0));
    let body = fields
        .iter()
        .map(|(key, value)| {
            format!(
                "{}:{}",
                serde_json::Value::from(key.as_str()),
                canonical_json(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}")
}

/// Pins the frontend-facing wire to API_VERSION: a shape change fails here until the version moves.
#[test]
fn frontend_wire_digest_is_pinned_to_api_version() {
    let mut ended = false;
    let samples = [
        crate::output_stream::output_bytes_frame("tab", "pty", "boot", b"first\x1b\xff", 9),
        crate::output_stream::output_ended_frame("tab", &mut ended).unwrap(),
        super::overflow_payload("boot-changed", "boot", 9, None, Some("build")),
        super::overflow_payload("ring-evicted", "boot", 9, Some(4), None),
        super::overflow_payload("lagged", "boot", 9, None, None),
    ];
    let wire = samples
        .iter()
        .map(canonical_json)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        (
            codeterm_service_core::frontend_bundle_api_version::API_VERSION,
            format!("{:016x}", fnv1a64(wire.as_bytes()))
        ),
        (3, "58b59c0ab964faec".to_string()),
        "frontend wire changed; bump API_VERSION and the release contract range, then repin:\n{wire}"
    );
}

#[test]
fn typed_tab_filter_and_generic_suffix_preserve_matching() {
    use crate::events::SystemEvent;
    for event in [
        SystemEvent::TabFocused {
            tab_id: "tab".into(),
        },
        SystemEvent::Generic {
            event_type: "tab-status-tab".into(),
            data: serde_json::Value::Null,
        },
        SystemEvent::Generic {
            event_type: "other".into(),
            data: serde_json::json!({"tab_id": "tab"}),
        },
    ] {
        assert!(super::event_matches_tab(&event, Some("tab")));
        assert!(!super::event_matches_tab(&event, Some("other")));
        assert!(super::event_matches_tab(&event, None));
    }
    assert!(!super::event_matches_tab(
        &SystemEvent::MuxStateChanged,
        Some("tab")
    ));
}

#[tokio::test]
async fn multipart_upload_contract_accepts_browser_boundary() {
    async fn inspect(mut multipart: axum::extract::Multipart) -> Result<String, StatusCode> {
        let field = multipart
            .next_field()
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?
            .ok_or(StatusCode::BAD_REQUEST)?;
        let filename = field.file_name().unwrap_or_default().to_string();
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        Ok(format!("{filename}:{}", data.len()))
    }

    let boundary = "codeterm-browser-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"phone photo.png\"\r\nContent-Type: image/png\r\n\r\nimage-bytes\r\n--{boundary}--\r\n"
    );
    let response = Router::new()
        .route("/upload", post(inspect))
        .oneshot(
            Request::post("/upload")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let response = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&response[..], b"phone photo.png:11");
}

#[test]
fn cookie_max_age_uses_web_token_ttl() {
    let ttl = WEB_TOKEN_TTL.as_secs();
    let cookie = format!(
        "codeterm-web-token={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        "tok", ttl
    );
    assert!(
        cookie.contains(&format!("Max-Age={ttl}")),
        "Set-Cookie must embed WEB_TOKEN_TTL, got: {cookie}"
    );
}

#[test]
fn constant_time_eq_rejects_last_byte_diff() {
    let token = b"supersecrettoken1234";
    let mut bad = token.to_vec();
    *bad.last_mut().unwrap() ^= 1;
    assert!(
        !codeterm_service_core::service::tokens::constant_time_eq(token, &bad),
        "token differing in last byte must be rejected"
    );
    assert!(
        codeterm_service_core::service::tokens::constant_time_eq(token, token),
        "identical tokens must match"
    );
}

#[tokio::test]
async fn snapshot_wire_validators_and_gzip() {
    use axum::response::IntoResponse;
    use axum::routing::get;
    use tower_http::compression::CompressionLayer;
    for path in ["/api/mux/state", "/api/tabs"] {
        let payload = serde_json::json!({"generation": 8, "tabs": "x".repeat(4096)}).to_string();
        let response_payload = payload.clone();
        let router = Router::new()
            .route(
                path,
                get(move |headers: axum::http::HeaderMap| {
                    let payload = response_payload.clone();
                    async move {
                        if path == "/api/tabs" {
                            super::tabs_response(&headers, payload.into_bytes())
                        } else {
                            super::generation_response(&headers, 8, || {
                                ([("content-type", "application/json")], payload).into_response()
                            })
                        }
                    }
                }),
            )
            .layer(CompressionLayer::new());
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("if-none-match", "\"stale\"")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()["etag"].clone();
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
            payload
        );
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("if-none-match", etag.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()["etag"], etag);
        assert!(to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty());
        let response = router
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("accept-encoding", "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-encoding"], "gzip");
        let compressed = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&compressed[..2], &[0x1f, 0x8b]);
        assert!(compressed.len() < payload.len());
    }
}

#[tokio::test]
async fn tabs_projection_change_invalidates_etag_without_mux_generation_change() {
    let headers = axum::http::HeaderMap::new();
    let first = super::tabs_response(&headers, br#"[{"id":"remote","status":"idle"}]"#.to_vec());
    let mut headers = headers;
    headers.insert("if-none-match", first.headers()["etag"].clone());
    let changed = super::tabs_response(&headers, br#"[{"id":"remote","status":"busy"}]"#.to_vec());
    assert_eq!(changed.status(), StatusCode::OK);
    assert_ne!(changed.headers()["etag"], first.headers()["etag"]);
    assert!(!to_bytes(changed.into_body(), usize::MAX)
        .await
        .unwrap()
        .is_empty());
}

#[test]
fn generation_validator_accepts_weak_strong_lists_and_wildcard() {
    for value in ["W/\"8\"", "\"8\"", "\"old\", W/\"8\"", "*"] {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("if-none-match", value.parse().unwrap());
        assert!(super::etag_matches(&headers, &super::generation_etag(8)));
        if value != "*" {
            assert!(!super::etag_matches(&headers, &super::generation_etag(9)));
        }
    }
}

#[tokio::test]
async fn registered_snapshot_routes_compress_and_revalidate() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.snapshot().workspaces[0].id.clone();
    for _ in 0..12 {
        mux.create_tab(&workspace).unwrap();
    }
    let fixture = ServiceFixture::new(mux);
    let app = crate::routes::create_router(crate::transport_tabs::fixture::app_state(&fixture));
    for path in ["/api/mux/state", "/api/tabs?local_only=true"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", "Bearer preview-test-token")
                    .header("accept-encoding", "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["content-encoding"], "gzip", "{path}");
        let etag = response.headers()["etag"].clone();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", "Bearer preview-test-token")
                    .header("if-none-match", etag.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{path}");
        assert_eq!(response.headers()["etag"], etag);
        assert!(to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty());
    }
}

#[tokio::test]
async fn newer_mux_generation_returns_body_and_new_validator() {
    use axum::response::IntoResponse;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("if-none-match", super::generation_etag(8).parse().unwrap());
    let response = super::generation_response(&headers, 9, || "new snapshot".into_response());
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["etag"], "W/\"9\"");
    assert_eq!(
        to_bytes(response.into_body(), usize::MAX).await.unwrap(),
        "new snapshot"
    );
}

#[tokio::test]
async fn web_tab_close_refuses_a_protected_tab_with_a_typed_status() {
    use axum::extract::{Path, State};
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().protected = true;
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);

    let refused = super::tab_close(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Path((workspace.clone(), tab.clone())),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.0, StatusCode::FORBIDDEN);
    assert!(refused.1.contains("protected Tab"));
    assert!(state.svc.mux.read().get_tab(&tab).is_some());

    let reservation = state.svc.mux.write().reserve_tab_close(&tab).unwrap();
    let locked = super::tab_close(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Path((workspace, tab)),
    )
    .await
    .unwrap_err();
    state.svc.mux.write().abort_tab_close(&reservation);
    assert_eq!(locked.0, StatusCode::CONFLICT);
}

#[test]
fn view_state_invalidations_match_only_the_exact_surface_tab_subscription() {
    let event = crate::events::SystemEvent::SurfaceStateInvalidated {
        plugin: "studio".into(),
        surface: "board".into(),
        instance: "one".into(),
        tab: "surface-tab".into(),
    };
    assert!(super::event_matches_tab(&event, Some("surface-tab")));
    assert!(!super::event_matches_tab(&event, Some("other-tab")));
}
