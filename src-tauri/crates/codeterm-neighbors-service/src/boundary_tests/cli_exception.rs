use crate::prelude::*;
use crate::service::ServiceContext;
use codeterm_service_core::service::agent_runtime::cli_exception::tests::fixture;
use codeterm_service_core::service::agent_runtime::cli_exception::*;
use std::path::Path;

#[test]
fn full_concierge_skips_optional_exception_only_with_current_host_bypass_grant() {
    use crate::multiplexer::{state::MultiplexerState, types::NeighborAuthority};
    use crate::service::{test_fixture::ServiceFixture, SpawnAgentParams};
    use codeterm_neighbors::permissions::MachineAccess;

    let (_directory, mut context, policy, _executable) = fixture();
    let (mut mux, tab_id) = MultiplexerState::new();
    context.tab_id = tab_id.clone();
    context.settings.mode = Some("full-access".into());
    let authority = NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    };
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("opaque-pty".into());
        tab.session_binding_nonce = Some(context.binding_nonce.clone());
        tab.neighbor_authority = Some(authority.clone());
    }
    let service = crate::test_support::fixture(mux);
    service.register_neighbor_advisory_provider("opaque-pty");
    let svc = &service.service;
    svc.set_neighbor_friend_receiver_for_test(&authority.friendship_id, Some(true), 1);
    // A full-looking reported mode cannot replace missing confined launch authority.
    assert!(sandbox(svc, &context).is_err());
    svc.mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .restorable_launch
        .permission_bypass_requested = true;
    assert!(sandbox(svc, &context).is_err());
    let mut params = SpawnAgentParams {
        neighbor_authority: Some(authority.clone()),
        ..Default::default()
    };
    svc.configure_neighbor_launch_access("opaque-pty", MachineAccess::Full, &mut params)
        .unwrap();
    svc.neighbors()
        .prepare_neighbor_restore_grant(&tab_id, "opaque-pty", &params)
        .unwrap();
    assert_eq!(sandbox(svc, &context).unwrap(), None);
    assert!(!Path::new(&context.cwd).join(".runtime-policy").exists());
    {
        let mut mux = svc.mux.write();
        mux.get_tab_mut(&tab_id)
            .unwrap()
            .restorable_launch
            .execution_sandbox = Some(policy);
    }
    assert!(sandbox(svc, &context).is_err());
    svc.mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .restorable_launch
        .execution_sandbox = None;
    let mut foreign = context.clone();
    foreign.binding_nonce = "foreign-binding".into();
    assert!(sandbox(svc, &foreign).is_err());
    svc.set_neighbor_friend_receiver_for_test(&authority.friendship_id, Some(false), 2);
    assert!(sandbox(svc, &context).is_err());
}
