use super::*;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};

#[test]
fn process_split_contract_socket_and_local_api_share_service_authority() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("socket-fixture".into(), None).id;
    let fixture = ServiceFixture::new(mux);
    let storage = fixture.install_seeded_storage_desk();
    let state = codeterm_api::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("socket-portable-token".into());
    let svc = state.svc.clone();
    assert!(svc.set_ext(state.rest_api.clone()).is_ok());
    let router = codeterm_api::routes::create_router_with_frontend(state, None);
    assert!(svc
        .set_ext(codeterm_service_core::ports::rest::LocalApiRouter(router))
        .is_ok());
    let mut events = svc.events.subscribe();
    let response = respond_to_request(
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            credential: fixture
                .service
                .configuration()
                .api
                .token
                .unwrap_or_default(),
            id: Some(serde_json::json!(71)),
            protocol_version: Some(serde_json::json!(protocol::PROTOCOL_VERSION)),
            method: "workspace.rename".into(),
            params: Some(serde_json::json!({"workspace_id":workspace,"name":"socket-renamed"})),
        },
        0,
        &svc.mux,
        &svc.pty,
        &svc.notif,
        &svc.output_buf,
        &svc,
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(response.id, Some(serde_json::json!(71)));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_workspace(&workspace)
            .unwrap()
            .name,
        "socket-renamed"
    );
    codeterm_service_core::async_runtime::block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if events.recv().await.unwrap().frame.event_type == "mux-state-changed" {
                    break;
                }
            }
        })
        .await
        .unwrap();
    });
    let read = dispatch_service_rpc(
        "api.request",
        Some(serde_json::json!({
            "method":"GET", "path":"/api/workspaces", "headers":{"authorization":"Bearer forged"}
        })),
        &svc,
    )
    .unwrap();
    assert_eq!(read["status"], 200);
    assert!(read["body"].as_str().unwrap().contains("socket-renamed"));
    let change = dispatch_service_rpc(
        "api.request",
        Some(serde_json::json!({
            "method":"POST", "path":format!("/api/workspaces/{workspace}/rename"),
            "headers":{"content-type":"application/json"},
            "body":serde_json::json!({"name":"local-api-renamed"}).to_string()
        })),
        &svc,
    )
    .unwrap();
    assert_eq!(change["status"], 200);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_workspace(&workspace)
            .unwrap()
            .name,
        "local-api-renamed"
    );
    codeterm_service_core::service::session_persist::flush_session_persistence(&fixture.service)
        .unwrap();
    let persisted =
        codeterm_service_core::session::store::StorageDeskAdapter::from_storage(storage)
            .state()
            .unwrap();
    assert_eq!(
        persisted
            .snapshot
            .workspaces
            .iter()
            .find(|entry| entry.record.id.as_str() == workspace)
            .unwrap()
            .record
            .name,
        "local-api-renamed"
    );
    let health = dispatch_service_rpc(
        "api.request",
        Some(serde_json::json!({"method":"GET","path":"/api/health"})),
        &svc,
    )
    .unwrap();
    assert_eq!(health["status"], 200);
    let native = dispatch_service_rpc(
        "api.request",
        Some(serde_json::json!({"method":"POST","path":"/api/app/restart"})),
        &svc,
    )
    .unwrap();
    assert_eq!(native["status"], 503);
    assert_eq!(native["body"], "native_capability_unavailable");
    assert!(dispatch_mesh_rpc("rest_api.restart", None, &svc, "peer").is_err());
    codeterm_service_core::service::session_persist::stop_worker(&fixture.service).unwrap();
}
