use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::agent_lifecycle::AccountRestartBinding;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn account_selection_waits_for_exact_pty_cleanup_and_retirement() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("exact-session".into());
    tab.session_binding_nonce = Some("old-nonce".into());
    tab.reported_session_binding_nonce = Some("old-nonce".into());
    tab.pty_id = Some("old-pty".into());
    tab.lifecycle_epoch = Some(7);
    let target = AccountRestartTarget {
        terminal_turn: None,
        expected: AccountRestartBinding::capture(tab, None).unwrap(),
        account_id: Some("next-account".into()),
        account_namespace: Some("next-namespace".into()),
        invocation: Some(
            codeterm_plugin::provider_account::InvocationContext::new(
                "opaque-provider".into(),
                "next-account".into(),
                Default::default(),
                vec![],
                vec![],
            )
            .unwrap(),
        ),
    };
    let snap = TabRestartSnapshot {
        tab_id: id.clone(),
        pty_id: "old-pty".into(),
        provider: "opaque-provider".into(),
        target_provider: None,
        session_id: Some("exact-session".into()),
        cwd: None,
        auto_approve: false,
        role: None,
        role_profile: None,
        handoff_history: None,
    };
    let fixture = ServiceFixture::new(mux);
    assert!(commit_pty(&fixture.service, &snap, &target).is_err());
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.remember_superseded_binding_nonce();
        tab.agent_type = None;
        tab.agent_session_id = None;
        tab.session_binding_nonce = None;
        tab.reported_session_binding_nonce = None;
        tab.lifecycle_epoch = None;
    }
    assert!(commit_pty(&fixture.service, &snap, &target).is_err());
    fixture.service.mux.write().get_tab_mut(&id).unwrap().pty_id = None;
    commit_pty(&fixture.service, &snap, &target).unwrap();
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .restorable_launch
            .account_id
            .as_deref(),
        Some("next-account")
    );
    assert!(commit_pty(&fixture.service, &snap, &target).is_err());
}
