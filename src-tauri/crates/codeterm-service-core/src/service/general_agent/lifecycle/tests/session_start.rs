use super::*;

#[test]
fn general_restart_keeps_its_provider_session_and_model_when_replacing_the_pty() {
    let mut mux = MultiplexerState::empty();
    super::super::super::workspace::ensure_general_workspace(&mut mux, true);
    let id = mux.resolve_tab_id(super::super::GENERAL_ALIAS).unwrap();
    {
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_session_id = Some("owned-session".into());
        tab.agent_session_model = Some("fixture/model".into());
        tab.pty_id = Some("retired-pty".into());
    }
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .general_agent
        .agent_provider = "fixture-provider".into();
    super::super::clear_stale_general_binding(&fixture.service, &fixture.service.mux, &id);
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("fixture-provider"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("owned-session"));
    assert_eq!(tab.agent_session_model.as_deref(), Some("fixture/model"));
    assert!(tab.pty_id.is_none());
}
