use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::json;

#[test]
fn socket_save_delete_append_archive_and_promote_use_authenticated_author() {
    let (mux, owner) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let svc = std::sync::Arc::new(fixture.service.clone());
    svc.ext::<codeterm_service_core::config::ConfigState>()
        .lock()
        .api
        .token = Some("host".into());
    let token = svc
        .frontend_pty_env(Some(&owner))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let request = |method: &str, params: serde_json::Value| {
        let response = super::super::respond_to_request(
            super::super::JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: method.into(),
                credential: token.clone(),
                params: Some(params),
                protocol_version: Some(json!(super::super::protocol::PROTOCOL_VERSION)),
            },
            0,
            &svc.mux,
            &svc.pty,
            &svc.notif,
            &svc.output_buf,
            &svc,
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        response.result.unwrap()
    };
    let saved = request(
        "mem.save",
        json!({"kind":"learning","title":"Authored learning","body":"original body","skip_extract":true}),
    );
    let id = saved["id"].as_str().unwrap();
    request(
        "mem.append",
        json!({"id":id,"text":"extra body","changed_by":"forged"}),
    );
    request("mem.learn_promote", json!({"id":id}));
    request("mem.archive", json!({"id":id,"reason":"unit regression"}));
    request("mem.delete", json!({"id":id}));
    let history = svc.mem.get_item_history(id).unwrap();
    assert_eq!(history.len(), 5);
    for row in history {
        assert_eq!(
            row.author.kind,
            codeterm_service_core::mem::store::MemAuthorKind::Owner
        );
    }
}
