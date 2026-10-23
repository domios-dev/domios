use super::*;
use crate::multiplexer::state::MultiplexerState;

fn fixture() -> (Tab, RemoteWorkerTransfer) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("session".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    tab.report_target_mesh_peer = Some("origin".into());
    tab.report_target_tab_id = Some("parent".into());
    tab.role = Some(AgentRole::Worker);
    let request = RemoteWorkerTransfer {
        worker_tab_id: id,
        worker_session_id: "session".into(),
        expected_parent_tab_id: "parent".into(),
        destination_tab_id: "destination".into(),
        caller_tab_id: Some("parent".into()),
        owner_confirmed: false,
    };
    (tab.clone(), request)
}

#[test]
fn transfer_changes_only_the_report_target_and_preserves_identity() {
    let (mut tab, request) = fixture();
    let mut expected = serde_json::to_value(&tab).unwrap();
    expected["report_target_tab_id"] = serde_json::json!("destination");
    let receipt = apply_remote_worker_transfer(&mut tab, "origin", &request).unwrap();
    assert_eq!(serde_json::to_value(&tab).unwrap(), expected);
    assert_eq!(receipt.worker_session_id, "session");
    assert_eq!(receipt.previous_parent_tab_id, "parent");
    assert_eq!(receipt.destination_tab_id, "destination");
}

#[test]
fn only_the_current_parent_or_explicit_owner_can_transfer() {
    let (tab, mut request) = fixture();
    for caller in [None, Some("outsider".into()), Some("destination".into())] {
        request.caller_tab_id = caller;
        assert!(authorize_remote_worker_transfer(&tab, "origin", &request).is_err());
    }
    request.caller_tab_id = None;
    request.owner_confirmed = true;
    assert!(authorize_remote_worker_transfer(&tab, "origin", &request).is_ok());
}

#[test]
fn peer_cannot_transfer_a_local_or_foreign_worker_even_as_owner() {
    let (tab, mut request) = fixture();
    request.owner_confirmed = true;
    assert!(authorize_remote_worker_transfer(&tab, "foreign", &request).is_err());
    let mut local = tab.clone();
    local.report_target_mesh_peer = None;
    assert!(authorize_remote_worker_transfer(&local, "origin", &request).is_err());
    let mut parented = tab.clone();
    parented.parent_tab_id = Some("local-parent".into());
    assert!(authorize_remote_worker_transfer(&parented, "origin", &request).is_err());
    let mut teamed = tab;
    teamed.team_id = Some("local-team".into());
    assert!(authorize_remote_worker_transfer(&teamed, "origin", &request).is_err());
}

#[test]
fn stale_session_parent_and_unverified_identity_are_refused_without_mutation() {
    let (tab, request) = fixture();
    let before = serde_json::to_value(&tab).unwrap();
    let mut stale_session = request.clone();
    stale_session.worker_session_id = "replacement".into();
    let mut stale_parent = request.clone();
    stale_parent.expected_parent_tab_id = "old-parent".into();
    let mut wrong_tab = request.clone();
    wrong_tab.worker_tab_id = "other-worker".into();
    let mut empty_destination = request.clone();
    empty_destination.destination_tab_id = " ".into();
    for request in [stale_session, stale_parent, wrong_tab, empty_destination] {
        let mut candidate = tab.clone();
        assert!(apply_remote_worker_transfer(&mut candidate, "origin", &request).is_err());
        assert_eq!(serde_json::to_value(candidate).unwrap(), before);
    }
    let mut unverified = tab;
    unverified.reported_session_binding_nonce = Some("foreign".into());
    assert!(authorize_remote_worker_transfer(&unverified, "origin", &request).is_err());
}

#[test]
fn task_bound_worker_requires_its_plan_ownership_path() {
    let (mut tab, request) = fixture();
    tab.task_binding = Some(Default::default());
    assert!(authorize_remote_worker_transfer(&tab, "origin", &request).is_err());
}

#[test]
fn roleless_mesh_worker_can_transfer_without_becoming_a_local_child() {
    let (mut tab, request) = fixture();
    tab.role = None;
    apply_remote_worker_transfer(&mut tab, "origin", &request).unwrap();
    assert_eq!(tab.parent_tab_id, None);
    assert_eq!(tab.team_id, None);
    assert_eq!(tab.role, None);
    assert_eq!(tab.report_target_mesh_peer.as_deref(), Some("origin"));
}
