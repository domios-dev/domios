use super::*;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};

#[tokio::test]
async fn cli_rpc_spawn_forwards_only_the_granted_workspace_to_the_launch_boundary() {
    let (fixture, state, concierge, token) = fixture().await;
    let workspace = state
        .svc
        .mux
        .read()
        .find_workspace_for_tab(&concierge)
        .unwrap();
    fixture.seed_neighbor_concierge(&concierge,&"a".repeat(64),serde_json::from_value(json!({"allowed":true,"scope":"opaque","allow_tab_requests":true,"workspaces":[workspace]})).unwrap()).await;
    codeterm_service_core::ports::socket::register_test_stub();
    let (status, body) = call(
        state.clone(),
        &token,
        "agent.spawn",
        Some(json!({"workspace_id":workspace})),
    )
    .await
    .unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body.0["error"]["message"], "rpc bridge test stub");
    for params in [
        None,
        Some(json!({"workspace_id":"foreign"})),
        Some(json!({"workspace_id":workspace,"caller_tab_id":"foreign"})),
    ] {
        assert_eq!(
            call(state.clone(), &token, "agent.spawn", params)
                .await
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn cli_rpc_workspace_grant_lists_agents_and_only_forwards_allowed_sends() {
    let (fixture, state, concierge, token) = fixture().await;
    let (workspace, allowed, shell) = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.create_workspace("opaque-project".into(), None).id;
        let allowed = mux.create_tab(&workspace).unwrap();
        mux.get_tab_mut(&allowed).unwrap().agent_type = Some("fixture".into());
        let shell = mux.create_tab(&workspace).unwrap();
        (workspace, allowed, shell)
    };
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(
                json!({"allowed":true,"scope":"opaque","workspaces":[workspace]}),
            )
            .unwrap(),
        )
        .await;
    fixture.install_seeded_storage_desk();
    codeterm_service_core::ports::socket::register_test_stub();
    let rows = call(state.clone(), &token, "tab.list", None)
        .await
        .unwrap()
        .0;
    assert_eq!(rows["result"].as_array().unwrap().len(), 1);
    assert_eq!(rows["result"][0]["id"], allowed);
    for (target, expected) in [
        (&allowed, StatusCode::BAD_REQUEST),
        (&shell, StatusCode::FORBIDDEN),
        (&concierge, StatusCode::FORBIDDEN),
    ] {
        let (status, body) = call(
            state.clone(),
            &token,
            "tab.send_text",
            Some(json!({"tab_id":target,"text":"opaque"})),
        )
        .await
        .unwrap_err();
        assert_eq!(status, expected);
        if target == &allowed {
            assert_eq!(body.0["error"]["message"], "rpc bridge test stub");
        }
    }
    assert_eq!(
        call(
            state.clone(),
            &token,
            "tab.send_text",
            Some(json!({"tab_id":allowed,"caller_tab_id":"foreign"}))
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(json!({"allowed":true,"scope":"opaque","workspaces":null}))
                .unwrap(),
        )
        .await;
    assert_eq!(
        call(state, &token, "tab.list", None).await.unwrap_err().0,
        StatusCode::FORBIDDEN
    );
}

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

#[tokio::test]
async fn loopback_self_reads_reach_dispatch_only_for_the_credential_owner_and_discovery_is_filtered(
) {
    let (_fixture, state, concierge, token) = fixture().await;
    codeterm_service_core::ports::socket::register_test_stub();
    for method in ["tab.agent_pad", "tab.todo.list"] {
        let (status, body) = call(
            state.clone(),
            &token,
            method,
            Some(json!({"tab_id":concierge})),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.0["error"]["message"], "rpc bridge test stub");
        assert_eq!(
            call(
                state.clone(),
                &token,
                method,
                Some(json!({"tab_id":"foreign"}))
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let Json(response) = call(state, &token, "system.capabilities", None)
        .await
        .unwrap();
    assert_eq!(response["result"]["plugins"], json!([]));
    assert!(response["result"]["methods"]
        .as_array()
        .unwrap()
        .iter()
        .all(|method| method != "tab.get_output"));
}
