use super::*;
use crate::multiplexer::state::MultiplexerState;

#[test]
fn account_restart_binding_rejects_each_stale_identity_dimension() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("exact-thread".into());
    tab.session_binding_nonce = Some("exact-nonce".into());
    tab.reported_session_binding_nonce = Some("exact-nonce".into());
    tab.restorable_launch.account_id = Some("account-a".into());
    tab.restorable_launch.account_namespace = Some("namespace-a".into());
    tab.lifecycle_epoch = Some(7);
    let expected = AccountRestartBinding::capture(tab, Some(3)).unwrap();
    let original = tab.clone();
    let changes: Vec<Box<dyn Fn(&mut Tab)>> = vec![
        Box::new(|tab| tab.agent_type = Some("other-provider".into())),
        Box::new(|tab| tab.agent_session_id = Some("other-thread".into())),
        Box::new(|tab| tab.session_binding_nonce = Some("other-nonce".into())),
        Box::new(|tab| tab.reported_session_binding_nonce = None),
        Box::new(|tab| tab.restorable_launch.account_id = None),
        Box::new(|tab| tab.restorable_launch.account_namespace = Some("other-namespace".into())),
        Box::new(|tab| tab.lifecycle_epoch = Some(8)),
    ];
    for change in changes {
        let mut changed = original.clone();
        change(&mut changed);
        assert!(expected.validate(&changed, Some(3)).is_err());
    }
    assert!(expected.validate(&original, Some(4)).is_err());
    assert!(expected.validate(&original, None).is_err());
    assert!(expected.validate(&original, Some(3)).is_ok());
}

#[test]
fn account_restart_binding_distinguishes_default_storage_and_refuses_missing_session() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("exact-thread".into());
    tab.session_binding_nonce = Some("exact-nonce".into());
    tab.reported_session_binding_nonce = Some("exact-nonce".into());
    let expected = AccountRestartBinding::capture(tab, None).unwrap();
    tab.restorable_launch.account_id = Some("default".into());
    assert!(expected.validate(tab, None).is_err());
    tab.agent_session_id = None;
    assert!(AccountRestartBinding::capture(tab, None).is_err());
}
