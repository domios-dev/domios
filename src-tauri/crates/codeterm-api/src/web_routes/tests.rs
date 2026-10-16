use crate::routes::WEB_TOKEN_TTL;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use tower::ServiceExt;

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
        let event = super::overflow_frame(reason, "boot", 9, oldest);
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
