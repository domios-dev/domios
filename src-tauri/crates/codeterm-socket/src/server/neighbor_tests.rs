use super::*;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use std::sync::Arc;

fn fixture() -> (ServiceFixture, String, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = ServiceFixture::new(mux);
    codeterm_service_core::async_runtime::block_on(
        fixture.seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(
                json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
            )
            .unwrap(),
        ),
    );
    let token = fixture
        .service
        .frontend_pty_env(Some(&concierge))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    (fixture, concierge, token)
}

#[test]
fn native_socket_allows_memory_reads_until_the_friend_grant_is_revoked() {
    let (fixture, concierge, token) = fixture();
    let response = request(
        &fixture,
        &token,
        "mem.search",
        Some(json!({"q":"opaque","supporter":false})),
    );
    assert!(response.result.is_some(), "{:?}", response.error);
    codeterm_service_core::async_runtime::block_on(fixture.seed_neighbor_concierge(&concierge, &"a".repeat(64), serde_json::from_value(json!({"allowed":true,"scope":"opaque","allow_tab_requests":false,"allow_memory":false})).unwrap()));
    assert_eq!(
        request(&fixture, &token, "mem.search", Some(json!({"q":"opaque"})))
            .error
            .unwrap()
            .code,
        -32003
    );
}

fn request(
    fixture: &ServiceFixture,
    token: &str,
    method: &str,
    params: Option<Value>,
) -> JsonRpcResponse {
    let request: JsonRpcRequest = serde_json::from_value(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params,"credential":token,"protocol_version":protocol::PROTOCOL_VERSION})).unwrap();
    let service = Arc::new(fixture.service.clone());
    respond_to_request(
        request,
        0,
        &service.mux,
        &service.pty,
        &service.notif,
        &service.output_buf,
        &service,
    )
}

#[test]
fn authenticated_concierge_socket_denies_early_bindings_and_omitted_or_forged_actor_before_dispatch(
) {
    let (fixture, concierge, token) = fixture();
    let config = serde_json::to_value(fixture.service.configuration()).unwrap();
    for method in [
        "model_provider.discover",
        "mem.secret.get",
        "mem.save",
        "tab.new",
        "tab.get_output",
        "tab.send_text",
        "cron.list",
        "cron.update",
        "cron.trigger",
        "plugin:opaque.execute",
        "bridge.send",
        "notification.create",
        "config.set",
        "mesh.relay",
    ] {
        for params in [
            None,
            Some(json!({})),
            Some(json!({"caller_tab_id":"foreign"})),
        ] {
            let response = request(&fixture, &token, method, params);
            assert_eq!(response.error.unwrap().code, -32003, "{method}");
        }
    }
    assert!(fixture.service.mux.read().get_tab(&concierge).is_some());
    assert_eq!(
        serde_json::to_value(fixture.service.configuration()).unwrap(),
        config
    );
    assert!(request(
        &fixture,
        "invalid-token",
        "mem.search",
        Some(json!({"q":"opaque"}))
    )
    .error
    .is_some());
}

#[test]
fn cli_api_request_wire_cannot_downgrade_the_concierge_to_local_or_relay_foreign_routes() {
    let (fixture, _, token) = fixture();
    for (method, path) in [
        ("PATCH", "/api/config"),
        ("POST", "/api/mesh/relay-to"),
        ("GET", "/api/tabs/foreign/text"),
        ("POST", "/api/notifications"),
        ("GET", "/api/secrets/opaque"),
    ] {
        let response = request(
            &fixture,
            &token,
            "api.request",
            Some(
                json!({"method":method,"path":path,"headers":{"authorization":"Bearer owner-secret","x-codeterm-tab-id":"foreign"}}),
            ),
        );
        assert_eq!(response.error.unwrap().code, -32003, "{path}");
    }
}
