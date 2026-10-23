use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::remote_worker_transfer::apply_remote_worker_transfer;

fn request(tab_id: &str) -> RemoteWorkerTransfer {
    RemoteWorkerTransfer {
        operation_id: "operation".into(),
        worker_tab_id: tab_id.into(),
        worker_session_id: "session".into(),
        worker_provider: "fixture-provider".into(),
        expected_parent_tab_id: "parent".into(),
        destination_tab_id: "destination".into(),
        destination_team_id: None,
        caller_tab_id: Some("parent".into()),
        owner_confirmed: false,
    }
}

#[test]
fn fake_peer_wires_authenticated_origin_and_payload_into_the_worker_mutation() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("session".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    tab.report_target_mesh_peer = Some("authenticated-origin".into());
    tab.report_target_tab_id = Some("parent".into());
    let before_session = tab.agent_session_id.clone();
    let before_nonce = tab.session_binding_nonce.clone();
    let receipt = transfer_for_peer(
        true,
        "authenticated-origin",
        &request(&id),
        |peer, request| apply_remote_worker_transfer(tab, peer, request),
    )
    .unwrap();
    assert_eq!(receipt.origin_peer_id, "authenticated-origin");
    assert_eq!(receipt.worker_tab_id, id);
    assert_eq!(tab.report_target_tab_id.as_deref(), Some("destination"));
    assert_eq!(tab.agent_session_id, before_session);
    assert_eq!(tab.session_binding_nonce, before_nonce);
}

#[test]
fn restricted_peer_is_refused_before_any_worker_lookup_or_mutation() {
    let result = transfer_for_peer(false, "restricted", &request("worker"), |_, _| {
        panic!("restricted peers must not reach the transfer service")
    });
    assert_eq!(result.unwrap_err().0, StatusCode::FORBIDDEN);
}

#[test]
fn worker_authority_refusal_is_preserved_as_conflict() {
    let result = transfer_for_peer(true, "origin", &request("worker"), |_, _| {
        Err("current parent changed".into())
    });
    assert_eq!(
        result.unwrap_err(),
        (StatusCode::CONFLICT, "current parent changed".into())
    );
}
