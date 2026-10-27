use super::*;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};

async fn fixture() -> (ServiceFixture, AppState, String, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(
                json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
            )
            .unwrap(),
        )
        .await;
    let state = crate::test_support::app_state(&fixture);
    let token = state
        .svc
        .frontend_pty_env(Some(&concierge))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    (fixture, state, concierge, token)
}

async fn call(
    state: AppState,
    token: &str,
    method: &str,
    params: Option<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    dispatch(
        State(state),
        ConnectInfo("127.0.0.1:1234".parse().unwrap()),
        headers,
        Json(RpcRequest {
            method: method.into(),
            params,
            protocol_version: Some(json!(1)),
        }),
    )
    .await
}

#[tokio::test]
async fn loopback_cli_rpc_enforces_credential_allowlist_even_when_caller_params_are_missing() {
    let (_fixture, state, _, token) = fixture().await;
    for method in [
        "model_provider.discover",
        "mem.secret.get",
        "mem.save",
        "tab.new",
        "tab.get_output",
        "tab.send_text",
        "cron.list",
        "cron.trigger",
        "plugin:opaque.execute",
        "bridge.send",
        "notification.create",
    ] {
        for params in [
            None,
            Some(json!({})),
            Some(json!({"caller_tab_id":"foreign"})),
        ] {
            let (status, body) = call(state.clone(), &token, method, params)
                .await
                .unwrap_err();
            assert_eq!(status, StatusCode::FORBIDDEN, "{method}");
            assert_eq!(body.0["error"]["code"], -32003);
        }
    }
    for (method, path) in [
        ("PATCH", "/api/config"),
        ("POST", "/api/mesh/relay-to"),
        ("GET", "/api/tabs/foreign/text"),
    ] {
        assert_eq!(call(state.clone(), &token, "api.request", Some(json!({"method":method,"path":path,"headers":{"authorization":"Bearer owner-secret"}}))).await.unwrap_err().0, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn loopback_rpc_memory_grant_reaches_dispatch_but_revocation_stops_it_before_dispatch() {
    let (fixture, state, concierge, token) = fixture().await;
    codeterm_service_core::ports::socket::register_test_stub();
    let (status, body) = call(
        state.clone(),
        &token,
        "mem.search",
        Some(json!({"q":"opaque"})),
    )
    .await
    .unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body.0["error"]["message"], "rpc bridge test stub");
    fixture.seed_neighbor_concierge(&concierge, &"a".repeat(64), serde_json::from_value(json!({"allowed":true,"scope":"opaque","allow_tab_requests":false,"allow_memory":false})).unwrap()).await;
    let (status, body) = call(state, &token, "mem.search", Some(json!({"q":"opaque"})))
        .await
        .unwrap_err();
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0["error"]["data"]["reason"],
        "neighbor_memory_not_allowed"
    );
}
