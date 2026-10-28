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
fn socket_spawn_requires_a_granted_workspace_and_rejects_unsafe_flags_without_a_child() {
    let (fixture, concierge, token) = fixture();
    let workspace = fixture
        .service
        .mux
        .read()
        .find_workspace_for_tab(&concierge)
        .unwrap();
    codeterm_service_core::async_runtime::block_on(fixture.seed_neighbor_concierge(&concierge,&"a".repeat(64),serde_json::from_value(json!({"allowed":true,"scope":"opaque","allow_tab_requests":true,"workspaces":[workspace]})).unwrap()));
    let before = fixture.service.mux.read().snapshot().tabs.len();
    for params in [
        json!({"workspace_id":"foreign"}),
        json!({"workspace_id":workspace,"args":["opaque-option"]}),
        json!({"workspace_id":workspace,"caller_tab_id":"foreign"}),
    ] {
        let response = request(&fixture, &token, "agent.spawn", Some(params));
        assert!(response.error.is_some());
        assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
    }
}

#[test]
fn concierge_socket_lists_only_granted_workspace_agents_and_denies_foreign_sends() {
    let (fixture, concierge, token) = fixture();
    let (workspace, allowed, shell) = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.create_workspace("opaque-project".into(), None).id;
        let allowed = mux.create_tab(&workspace).unwrap();
        mux.get_tab_mut(&allowed).unwrap().agent_type = Some("fixture".into());
        let shell = mux.create_tab(&workspace).unwrap();
        (workspace, allowed, shell)
    };
    codeterm_service_core::async_runtime::block_on(
        fixture.seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(
                json!({"allowed":true,"scope":"opaque","workspaces":[workspace]}),
            )
            .unwrap(),
        ),
    );
    fixture.install_seeded_storage_desk();
    let rows = request(
        &fixture,
        &token,
        "tab.list",
        Some(json!({"agents_only":false})),
    )
    .result
    .unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], allowed);
    assert!(rows[0].get("session_binding_nonce").is_none());
    assert!(rows[0].get("other_workspace_tab_count").is_none());
    for target in [&concierge, &shell, &"missing".to_owned()] {
        let response = request(
            &fixture,
            &token,
            "tab.send_text",
            Some(json!({"tab_id":target,"text":"opaque"})),
        );
        assert_eq!(response.error.unwrap().code, -32003);
    }
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

#[test]
fn secret_cli_preflight_refuses_neighbor_credentials_without_entering_the_secret_backend() {
    let (fixture, concierge, token) = fixture();
    let backend_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = backend_calls.clone();
    fixture
        .service
        .set_ext(codeterm_service_core::ports::rest::LocalApiRouter(
            axum::Router::new().fallback(move || {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    axum::http::StatusCode::NO_CONTENT
                }
            }),
        ))
        .unwrap_or_else(|_| panic!("local backend router already installed"));
    for path in [
        "/api/secrets/status",
        "/api/secrets/auto-unlock",
        "/api/secrets/items/opaque/value",
    ] {
        let error = request(
            &fixture,
            &token,
            "api.request",
            Some(json!({"method":"POST","path":path,"body":"opaque-secret-input"})),
        )
        .error
        .unwrap();
        assert_eq!(error.code, -32003);
        assert_eq!(error.data.unwrap()["reason"], "neighbor_capability_denied");
    }
    for params in [
        None,
        Some(json!({})),
        Some(json!({"caller_tab_id":"foreign"})),
    ] {
        let response = request(&fixture, &token, "mem.secret.authorize", params);
        let error = response.error.unwrap();
        assert_eq!(error.code, -32003);
        assert!(error.message.starts_with("neighbor_capability_denied:"));
        assert_eq!(error.data.unwrap()["reason"], "neighbor_capability_denied");
    }
    let ordinary = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&concierge).unwrap();
        mux.create_tab(&workspace).unwrap()
    };
    let token = fixture
        .service
        .frontend_pty_env(Some(&ordinary))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let response = request(&fixture, &token, "mem.secret.authorize", None);
    assert_eq!(response.result.unwrap()["authorized"], true);
    assert_eq!(backend_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

fn neighbor_save_matrix(case: &str) {
    let (fixture, concierge, token) = fixture();
    codeterm_service_core::async_runtime::block_on(
        codeterm_service_core::mem_ops::neighbor_save::matrix::run(
            case,
            &fixture,
            &concierge,
            |method, body| {
                let service = fixture.service.clone();
                let token = token.clone();
                async move {
                    tokio::task::spawn_blocking(move || {
                let (method, token) = if method == "owner" { ("mem.neighbor_save".to_string(), "owner-secret".to_string()) } else { (method, token) };
                let req: JsonRpcRequest = serde_json::from_value(json!({"jsonrpc":"2.0","id":1,"method":method,"params":body,"credential":token,"protocol_version":protocol::PROTOCOL_VERSION})).unwrap();
                let svc = Arc::new(service);
                let response = respond_to_request(req, 0, &svc.mux, &svc.pty, &svc.notif, &svc.output_buf, &svc);
                match response.error { Some(error) => { assert_eq!(error.code, -32003); Err((403, error.message)) }, None => Ok(response.result.unwrap()) }
            }).await.unwrap()
                }
            },
        ),
    );
}

#[test]
fn neighbor_save_saved_in_granted_area() {
    neighbor_save_matrix("saved_in_granted_area");
}

#[test]
fn neighbor_save_area_b_refused() {
    neighbor_save_matrix("area_b_refused");
}

#[test]
fn neighbor_save_empty_write_list() {
    neighbor_save_matrix("empty_write_list");
}

#[test]
fn neighbor_save_mem_save_denied() {
    neighbor_save_matrix("mem_save_denied");
}

#[test]
fn neighbor_save_forged_author_ignored() {
    neighbor_save_matrix("forged_author_ignored");
}

#[test]
fn neighbor_save_rule_refused() {
    neighbor_save_matrix("rule_refused");
}

#[test]
fn neighbor_save_preference_refused() {
    neighbor_save_matrix("preference_refused");
}

#[test]
fn neighbor_save_skill_refused() {
    neighbor_save_matrix("skill_refused");
}

#[test]
fn neighbor_save_script_refused() {
    neighbor_save_matrix("script_refused");
}

#[test]
fn neighbor_save_forced_fields() {
    neighbor_save_matrix("forced_fields");
}

#[test]
fn neighbor_save_credentials_title() {
    neighbor_save_matrix("credentials_title");
}

#[test]
fn neighbor_save_credentials_body() {
    neighbor_save_matrix("credentials_body");
}

#[test]
fn neighbor_save_credentials_summary() {
    neighbor_save_matrix("credentials_summary");
}

#[test]
fn neighbor_save_credentials_tags() {
    neighbor_save_matrix("credentials_tags");
}

#[test]
fn neighbor_save_over_cap_body() {
    neighbor_save_matrix("over_cap_body");
}

#[test]
fn neighbor_save_over_cap_title() {
    neighbor_save_matrix("over_cap_title");
}

#[test]
fn neighbor_save_over_cap_total() {
    neighbor_save_matrix("over_cap_total");
}

#[test]
fn neighbor_save_revoked_area() {
    neighbor_save_matrix("revoked_area");
}

#[test]
fn neighbor_save_paused_friend() {
    neighbor_save_matrix("paused_friend");
}

#[test]
fn neighbor_save_owner_refused() {
    neighbor_save_matrix("owner_refused");
}

#[test]
fn neighbor_save_never_extracts() {
    neighbor_save_matrix("never_extracts");
}

#[test]
fn neighbor_save_missing_friend_author() {
    neighbor_save_matrix("missing_friend_author");
}

#[test]
fn neighbor_save_replace_inside_grant() {
    neighbor_save_matrix("replace_inside_grant");
}

#[test]
fn neighbor_save_replace_outside_grant() {
    neighbor_save_matrix("replace_outside_grant");
}

#[test]
fn neighbor_save_replace_pinned() {
    neighbor_save_matrix("replace_pinned");
}
