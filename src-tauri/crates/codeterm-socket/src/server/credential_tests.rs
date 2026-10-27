// WHAT: credential authentication at the external socket boundary and REST bridge.
// WHY: OS-user checks and an injected host bearer allowed caller impersonation.
// ORACLE: missing, foreign and retired authority refuses before dispatch effects.
use super::*;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use std::sync::Arc;

fn request(svc: &ServiceState, credential: &str, method: &str, params: Value) -> JsonRpcResponse {
    respond_to_request(
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(9)),
            method: method.into(),
            credential: credential.into(),
            params: Some(params),
            protocol_version: Some(json!(protocol::PROTOCOL_VERSION)),
        },
        0,
        &svc.mux,
        &svc.pty,
        &svc.notif,
        &svc.output_buf,
        svc,
    )
}

#[test]
fn external_socket_refuses_missing_and_retired_launch_credentials() {
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .ext::<codeterm_service_core::config::ConfigState>()
        .lock()
        .api
        .token = Some("host-fixture".into());
    let svc = Arc::new(fixture.service.clone());
    let credential = svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    for invalid in ["", "unknown-credential"] {
        assert_eq!(
            request(&svc, invalid, "tab.list", json!({}))
                .error
                .unwrap()
                .code,
            -32003
        );
    }
    assert!(request(&svc, "host-fixture", "tab.list", json!({}))
        .result
        .is_some());
    assert!(request(&svc, &credential, "tab.list", json!({}))
        .result
        .is_some());
    svc.rotate_session_binding_nonce(&tab);
    assert_eq!(
        request(&svc, &credential, "tab.list", json!({}))
            .error
            .unwrap()
            .code,
        -32003
    );
    let current = svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    assert!(request(&svc, &current, "tab.list", json!({}))
        .result
        .is_some());
    svc.close_tab(&tab).unwrap();
    assert_eq!(
        request(&svc, &current, "tab.list", json!({}))
            .error
            .unwrap()
            .code,
        -32003
    );
}

#[test]
fn external_socket_refuses_cross_tab_report_and_sender_claims() {
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let foreign = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = Arc::new(fixture.service.clone());
    let credential = svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    for (method, params) in [
        (
            "agent.report",
            json!({"tab_id":foreign,"caller_tab_id":foreign,"message":"opaque-report"}),
        ),
        (
            "tab.send_text",
            json!({"tab_id":tab,"source_tab":foreign,"text":"opaque-payload"}),
        ),
        ("plan.get", json!({"caller_tab_id":foreign})),
    ] {
        assert_eq!(
            request(&svc, &credential, method, params)
                .error
                .unwrap()
                .code,
            -32003,
            "{method}"
        );
    }
}

#[test]
fn local_api_bridge_cannot_promote_a_tab_credential_to_the_host() {
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let foreign = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = codeterm_api::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("host-fixture".into());
    let svc = state.svc.clone();
    let credential = svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    assert!(svc
        .set_ext(codeterm_service_core::ports::rest::LocalApiRouter(
            codeterm_api::routes::create_router_with_frontend(state, None)
        ))
        .is_ok());
    let refused = request(
        &svc,
        &credential,
        "api.request",
        json!({
            "method":"POST","path":"/api/batch",
            "authenticated_api_credential":"host-fixture",
            "headers":{"authorization":"Bearer host-fixture","content-type":"application/json"},
            "body":"{\"commands\":[]}"
        }),
    );
    assert_eq!(refused.result.unwrap()["status"], 403);
    let forged = request(
        &svc,
        &credential,
        "api.request",
        json!({
            "method":"GET","path":"/api/config",
            "headers":{"x-codeterm-tab-id":foreign}
        }),
    );
    assert_eq!(forged.result.unwrap()["status"], 403);
    let owner = request(
        &svc,
        "host-fixture",
        "api.request",
        json!({
            "method":"POST","path":"/api/batch",
            "headers":{"content-type":"application/json"},"body":"{\"commands\":[]}"
        }),
    );
    assert_eq!(owner.result.unwrap()["status"], 200);
    assert!(dispatch_mesh_rpc("api.request", None, &svc, "peer").is_err());
}
