use crate::prelude::*;
use codeterm_service_core::service::agent_runtime::cli_exception::tests::fixture;
use codeterm_service_core::service::agent_runtime::cli_exception::*;
use std::path::Path;

#[test]
fn read_only_concierge_cli_exception_preserves_pad_roots_and_network_confinement() {
    use crate::multiplexer::{state::MultiplexerState, types::NeighborAuthority};
    use codeterm_types::plan::ExecutionIntent;

    let (directory, mut context, mut policy, _) = fixture();
    let pad = directory.path().join("own-pad");
    std::fs::create_dir(&pad).unwrap();
    policy.intent = ExecutionIntent::ReadOnly;
    policy.runtime_mode = Some("read-only-with-writable-roots".into());
    policy.writable_roots = vec![pad.to_string_lossy().into_owned()];
    context.settings.mode = policy.runtime_mode.clone();
    context.settings.additional_directories = policy.writable_roots.clone();
    let (mut mux, tab_id) = MultiplexerState::new();
    context.tab_id = tab_id.clone();
    let target = mux.get_tab_mut(&tab_id).unwrap();
    target.session_binding_nonce = Some(context.binding_nonce.clone());
    target.neighbor_authority = Some(NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    });
    target.restorable_launch.execution_sandbox = Some(policy.clone());
    let fixture = crate::test_support::fixture(mux);
    let svc = &fixture.service;
    assert_eq!(sandbox(svc, &context).unwrap(), Some(policy.clone()));
    for settings in [
        codeterm_plugin::agent_runtime::RuntimeSettings {
            network_access: Some(true),
            ..context.settings.clone()
        },
        codeterm_plugin::agent_runtime::RuntimeSettings {
            additional_directories: vec![context.cwd.clone()],
            ..context.settings.clone()
        },
        codeterm_plugin::agent_runtime::RuntimeSettings {
            mode: Some("full-access".into()),
            ..context.settings.clone()
        },
    ] {
        let mut changed = context.clone();
        changed.settings = settings;
        assert!(sandbox(svc, &changed).is_err());
    }
    assert_eq!(sandbox(svc, &context).unwrap(), Some(policy));
}

#[test]
fn full_concierge_skips_optional_exception_only_with_current_host_bypass_grant() {
    use crate::multiplexer::{state::MultiplexerState, types::NeighborAuthority};
    use crate::service::SpawnAgentParams;
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
