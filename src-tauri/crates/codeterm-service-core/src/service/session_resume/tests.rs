use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::BootStage;

#[test]
fn session_resume_reuses_only_a_ready_authoritative_session() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.chat_view = true;
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("opaque-session".into());
    tab.session_binding_nonce = Some("opaque-nonce".into());
    tab.reported_session_binding_nonce = Some("opaque-nonce".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "opaque-provider".into(),
        session_id: "opaque-session".into(),
    };
    let fixture = ServiceFixture::new(mux);
    assert_eq!(
        fixture
            .service
            .existing_session_resume_tab("unbound-session")
            .unwrap(),
        None
    );
    assert_eq!(
        fixture
            .service
            .existing_session_resume_tab("opaque-session")
            .unwrap(),
        Some(tab_id.clone())
    );
    for (stage, booting, authenticated) in [
        (Some(BootStage::SpawnFailed), false, true),
        (None, true, true),
        (None, false, false),
    ] {
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.boot_stage = stage;
            tab.agent_booting = booting;
            tab.reported_session_binding_nonce = authenticated.then(|| "opaque-nonce".into());
        }
        let error = fixture
            .service
            .existing_session_resume_tab("opaque-session")
            .unwrap_err();
        assert!(
            error.contains(&format!("domios agent restart --tab {tab_id}")),
            "{error}"
        );
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .agent_session_id
                .as_deref(),
            Some("opaque-session")
        );
    }
}
