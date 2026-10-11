use super::*;
use crate::mesh::types::AccessLevel;
use crate::pty::manager::OutputBuffer;
use crate::resize_owner::ResizeOwnerRegistry;

#[test]
fn transport_tabs_contract_body_and_remote_target_refusal() {
    let body: CwdBody = serde_json::from_value(json!({ "cwd": "/workspace" })).unwrap();
    assert_eq!(body.cwd, "/workspace");
    assert!(serde_json::from_value::<CwdBody>(json!({ "cwd": 42 })).is_err());
    assert!(local_target(&CallerIdentity::Local, "tab").is_ok());
    assert_eq!(
        local_target(&CallerIdentity::Local, "mesh:peer/tab")
            .unwrap_err()
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn transport_tabs_contract_authentication_refusal() {
    let service = CallerIdentity::ServicePush {
        service_id: "service".into(),
    };
    let peer = CallerIdentity::Peer {
        instance_id: "peer".into(),
        access_level: AccessLevel::Full,
        relay_protocol_version: crate::mesh::types::RELAY_PROTOCOL_VERSION,
    };
    for caller in [service, peer] {
        assert_eq!(
            local_target(&caller, "tab").unwrap_err().0,
            StatusCode::FORBIDDEN
        );
    }
}

#[test]
fn transport_tabs_contract_attach_replay_and_truncation() {
    let mut output = OutputBuffer::new(4);
    output.append("pty", b"first");
    output.append("pty", &[0x1b, 0xff, 0x82, 0xac]);
    let (bytes, offset) = output.read_since("pty", 0);
    assert_eq!(bytes, vec![0x1b, 0xff, 0x82, 0xac]);
    assert_eq!(offset, 9);
    for _client in ["desktop", "browser"] {
        let (replay, next) = output.read_since("pty", 0);
        assert_eq!(replay, bytes);
        assert_eq!(next, offset);
        assert!(output.read_since("pty", next).0.is_empty());
    }
    let frame = crate::api::output_stream::output_bytes_frame("tab", "pty", "boot", &bytes, offset);
    assert_eq!(
        frame,
        json!({
            "tab_id": "tab", "pty_id": "pty", "boot": "boot",
            "output": String::from_utf8_lossy(&bytes), "data": bytes, "offset": 9
        })
    );
}

#[test]
fn transport_tabs_contract_resize_ownership() {
    let mut owners = ResizeOwnerRegistry::default();
    let first = owners.claim("tab".into(), "pty".into(), "desktop".into(), 120, 40, false);
    let second = owners.claim("tab".into(), "pty".into(), "browser".into(), 80, 24, false);
    assert_eq!(second.client_id, first.client_id);
    assert_eq!(second.cols, 120);
    assert!(!owners.is_owner("pty", "browser"));
    let forced = owners.claim("tab".into(), "pty".into(), "browser".into(), 80, 24, true);
    assert_eq!(forced.client_id, "browser");
    assert!(!owners.is_owner("pty", "desktop"));
}

#[path = "fixture.rs"]
mod fixture;

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn transport_tabs_contract_cwd_persistence_envelopes_and_status() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        middleware,
    };
    use tower::ServiceExt;

    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().pty_id = Some("retained-pty".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .output_buf
        .lock()
        .append("retained-pty", b"\x1b[31mraw\x1b[0m");
    let state = fixture::app_state(&fixture);
    let app = router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::api::auth::auth_middleware,
        ))
        .with_state(state);
    for auth in [None, Some("Bearer wrong")] {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/tabs/{tab}/cwd"))
            .header("content-type", "application/json");
        if let Some(auth) = auth {
            request = request.header("authorization", auth);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(r#"{"cwd":"/rejected"}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_ne!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab)
                .unwrap()
                .cwd
                .as_deref(),
            Some("/rejected")
        );
    }
    let request = |method, path: String, body: &'static str| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", "Bearer preview-test-token")
            .body(Body::from(body))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            format!("/api/tabs/{tab}/cwd"),
            r#"{"cwd":"/persisted"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({ "ok": true })
    );
    let snapshot = fixture.service.mux.read().to_session_snapshot();
    let storage =
        crate::session::store::StorageDeskAdapter::open(fixture.temp.path().join("persisted-desk"))
            .unwrap();
    storage.import_snapshot(&snapshot, Vec::new()).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&storage.snapshot().unwrap());
    assert_eq!(
        restored.get_tab(&tab).unwrap().cwd.as_deref(),
        Some("/persisted")
    );
    let response = app
        .clone()
        .oneshot(request("GET", format!("/api/tabs/{tab}/history"), ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["output"],
        "\x1b[31mraw\x1b[0m"
    );
    for (path, body, status) in [
        (
            format!("/api/tabs/{tab}/cwd"),
            "{}",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/api/tabs/missing/cwd".into(),
            r#"{"cwd":"/any"}"#,
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("POST", path, body))
                .await
                .unwrap()
                .status(),
            status
        );
    }
}

#[test]
fn transport_tabs_contract_ended_is_emitted_once() {
    let mut ended = false;
    assert_eq!(
        crate::api::output_stream::output_ended_frame("tab", &mut ended),
        Some(json!({ "tab_id": "tab", "ended": true }))
    );
    assert!(ended);
    assert_eq!(
        crate::api::output_stream::output_ended_frame("tab", &mut ended),
        None
    );
}

#[tokio::test]
async fn transport_tabs_contract_cancellation_releases_output_without_killing_pty() {
    use futures_util::StreamExt;
    use std::sync::Arc;
    let activity = crate::service::output_stream_activity::channel();
    let output = Arc::new(crate::lock_hold::TimedMutex::new(
        "output_buf",
        OutputBuffer::new(1024),
    ));
    let baseline = Arc::strong_count(&output);
    {
        let stream = crate::api::output_stream::tab_output_frames(
            activity.subscribe(),
            output.clone(),
            Some("shared-pty".into()),
            0,
        );
        futures_util::pin_mut!(stream);
        assert!(futures_util::poll!(stream.next()).is_pending());
        assert_eq!(Arc::strong_count(&output), baseline + 1);
    }
    assert_eq!(Arc::strong_count(&output), baseline);
    output.lock().append("shared-pty", b"still running");
    assert_eq!(
        output.lock().read_since("shared-pty", 0).0,
        b"still running"
    );
}
