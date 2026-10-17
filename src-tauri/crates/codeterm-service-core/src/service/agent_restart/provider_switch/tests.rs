use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn unavailable_target_refuses_before_retiring_or_mutating_source() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("source-fixture".into());
    tab.agent_session_id = Some("source-thread".into());
    tab.session_binding_nonce = Some("source-nonce".into());
    tab.reported_session_binding_nonce = Some("source-nonce".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("source-fixture"),
    ));
    assert!(fixture
        .service
        .switch_provider_in_place(&id, "missing-target", None, None, false)
        .is_err());
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("source-fixture"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("source-thread"));
    assert_eq!(tab.session_binding_nonce.as_deref(), Some("source-nonce"));
}

#[test]
fn host_handoff_carries_identity_and_archive_reference_as_data() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.title = "Opaque".into();
    tab.description = Some("description-token".into());
    tab.cwd = Some("cwd-token".into());
    tab.agent_type = Some("source-token".into());
    tab.agent_session_id = Some("session-token".into());
    let fixture = ServiceFixture::new(mux);
    let tab = fixture.service.mux.read().get_tab(&id).cloned().unwrap();
    let input = fixture
        .service
        .provider_switch_input(&tab, &[], std::path::Path::new("archive-token"))
        .unwrap();
    let context: serde_json::Value =
        serde_json::from_str(input.split("\n\n").nth(1).unwrap()).unwrap();
    assert_eq!(context["tab_id"], id);
    assert_eq!(context["title"], tab.title);
    assert_eq!(context["description"], "description-token");
    assert_eq!(context["previous_provider"], "source-token");
    assert_eq!(context["previous_session_id"], "session-token");
    assert_eq!(context["previous_transcript_path"], "archive-token");
    assert_eq!(context["cwd"], "cwd-token");
    assert!(!context["pad_path"].as_str().unwrap().is_empty());
    assert!(input.len() <= provider_handoff::input::MAX_INPUT_BYTES);
}
