use super::{chat_send_text, send_text, SendTextBody};
use axum::extract::{Json, Path, State};
use axum::http::{HeaderMap, StatusCode};
use codeterm_entitlements::{compiled_free, LimitGate};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;

fn locked_and_sibling() -> (ServiceFixture, String, String) {
    let (mut mux, sibling) = MultiplexerState::new();
    let general = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&general, true);
    let newest = ["a", "b", "c"]
        .map(|name| mux.create_workspace(name.into(), None).id)
        .last()
        .cloned()
        .unwrap();
    let locked = mux.create_tab(&newest).unwrap();
    mux.link_pty(&locked, "locked-pty".into());
    mux.set_limit_gate(LimitGate::new(compiled_free(), true));
    (ServiceFixture::new(mux), locked, sibling)
}

fn bearer() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        "Bearer preview-test-token".parse().unwrap(),
    );
    headers
}

fn body(peer: Option<&str>) -> SendTextBody {
    serde_json::from_value(serde_json::json!({
        "text": "opaque", "source_tab": "sender", "source_peer": peer
    }))
    .unwrap()
}

#[tokio::test]
async fn local_and_mesh_relayed_sends_to_a_locked_tab_are_typed_zero_write_refusals() {
    let (fixture, locked, _) = locked_and_sibling();
    let writes = fixture.record_agent_deliveries(&locked);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    for peer in [None, Some("relaying-peer")] {
        let rejection = send_text(
            State(state.clone()),
            bearer(),
            Path(locked.clone()),
            Json(body(peer)),
        )
        .await
        .unwrap_err();
        assert_eq!(rejection.status, StatusCode::FORBIDDEN);
        assert_eq!(rejection.body["error"]["code"], "plan_limit_locked");
        assert_eq!(rejection.body["error"]["bytes_written"], false);
    }
    let chat = chat_send_text(
        State(state.clone()),
        bearer(),
        Path(locked.clone()),
        Json(body(None)),
    )
    .await
    .err()
    .expect("chat send to a locked tab is refused");
    assert_eq!(chat.status, StatusCode::FORBIDDEN);
    assert_eq!(chat.body["error"]["code"], "plan_limit_locked");
    assert!(writes.texts.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&locked));
    assert!(state
        .svc
        .reconcile_user_send_statuses(&locked)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn user_pty_input_to_a_locked_tab_is_a_typed_zero_write_refusal() {
    let (fixture, _, _) = locked_and_sibling();
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let response = crate::pty_routes::pty_write(
        State(state),
        bearer(),
        Path("locked-pty".into()),
        Json(serde_json::from_value(serde_json::json!({ "data": "x" })).unwrap()),
    )
    .await
    .err()
    .expect("PTY input to a locked tab is refused");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["error"]["code"], "plan_limit_locked");
    assert_eq!(json["error"]["bytes_written"], false);
}
