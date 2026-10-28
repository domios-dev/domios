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

#[test]
fn own_pad_resolution_unblocks_named_reply_admission_and_foreign_pads_stay_denied() {
    let (fixture, concierge, token) = fixture();
    let foreign = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&concierge).unwrap();
        mux.create_tab(&workspace).unwrap()
    };
    let result = request(
        &fixture,
        &token,
        "tab.agent_pad",
        Some(json!({"tab_id":concierge})),
    )
    .result
    .unwrap();
    let pad = std::path::Path::new(result["path"].as_str().unwrap());
    assert!(pad.starts_with(fixture.temp.path()));
    assert_eq!(pad.file_name().unwrap().to_str(), Some(concierge.as_str()));
    std::fs::write(pad.join("answer.md"), b"opaque reply").unwrap();
    let reply = std::fs::read_to_string(pad.join("answer.md")).unwrap();
    assert_eq!(
        request(
            &fixture,
            &token,
            "tab.agent_pad",
            Some(json!({"tab_id":foreign}))
        )
        .error
        .unwrap()
        .code,
        -32003
    );
    assert!(!fixture
        .temp
        .path()
        .join("runtime/agent-pads")
        .join(foreign)
        .exists());
    let command = json!({"method":"POST","path":"/api/account/neighbors/runtime","body":{"operation":"send","friendship_id":"a".repeat(64),"kind":"answer","body":reply,"in_reply_to":"1".repeat(32)}});
    assert!(codeterm_service_core::async_runtime::block_on(
        fixture
            .service
            .authorize_neighbor_rpc_credential(&token, "api.request", Some(&command))
    )
    .is_ok());
}

#[test]
fn concierge_self_discovery_and_todo_reads_expose_only_the_allowed_surface() {
    let (fixture, concierge, token) = fixture();
    fixture.install_seeded_storage_desk();
    assert_eq!(
        request(&fixture, &token, "system.identify", None)
            .result
            .unwrap()["name"],
        codeterm_build_info::PRODUCT_NAME
    );
    let discovery = request(&fixture, &token, "system.capabilities", None)
        .result
        .unwrap();
    let methods = discovery["methods"].as_array().unwrap();
    assert!(methods.iter().any(|method| method == "tab.agent_pad"));
    assert!(!methods.iter().any(|method| method == "tab.get_output"
        || method == "model_provider.discover"
        || method == "notification.create"));
    assert_eq!(discovery["plugins"], json!([]));
    assert!(request(
        &fixture,
        &token,
        "tab.todo.list",
        Some(json!({"tab_id":concierge}))
    )
    .result
    .is_some());
    assert_eq!(
        request(
            &fixture,
            &token,
            "tab.todo.list",
            Some(json!({"tab_id":"foreign"}))
        )
        .error
        .unwrap()
        .code,
        -32003
    );
}

#[test]
fn pad_resolution_requires_an_explicit_own_target_and_current_credential_nonce() {
    let (fixture, concierge, token) = fixture();
    for params in [
        None,
        Some(json!({})),
        Some(json!({"tab_id":concierge,"caller_tab_id":"foreign"})),
    ] {
        assert_eq!(
            request(&fixture, &token, "tab.agent_pad", params)
                .error
                .unwrap()
                .code,
            -32003
        );
    }
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&concierge)
        .unwrap()
        .session_binding_nonce = Some("replacement-nonce".into());
    assert!(request(
        &fixture,
        &token,
        "tab.agent_pad",
        Some(json!({"tab_id":concierge}))
    )
    .error
    .is_some());
}
