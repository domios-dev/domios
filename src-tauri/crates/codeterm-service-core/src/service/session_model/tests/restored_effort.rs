use crate::agent::test_provider::MockProvider;
use crate::multiplexer::{state::MultiplexerState, types::PendingModelSwitchObservation};
use crate::service::{reasoning_effort, test_fixture::ServiceFixture};

#[test]
fn restored_next_turn_confirms_effort_without_settings_change() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("owned-session".into());
    tab.session_binding_nonce = Some("restored-binding".into());
    tab.reported_session_binding_nonce = Some("restored-binding".into());
    tab.cwd = Some("/fixture/project".into());
    tab.agent_provider_resolved_model = Some("fixture-model".into());
    reasoning_effort::persist_launch_identity(tab, None, Some("low"));
    let observation = PendingModelSwitchObservation {
        session_id: "owned-session".into(),
        launched_model: "fixture-model".into(),
        transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary {
            last_entry_uuid: Some("before-restore".into()),
            last_entry_timestamp: None,
            last_entry_type: None,
            boundary_found: true,
        },
    };
    tab.pending_model_switch_observation = Some(observation.clone());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_model_reasoning_efforts(
            "fixture-provider",
            "fixture-model",
            &["low", "high"],
        )
        .with_session_model("owned-session", "fixture-model")
        .with_session_reasoning_effort("owned-session", "low")
        .with_session_reasoning_effort("foreign-session", "high"),
    ));

    fixture
        .service
        .record_post_launch_turn_model(&tab_id, observation.clone(), "stale-binding");
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .reasoning_effort_identity
            .actual,
        None
    );
    fixture
        .service
        .record_post_launch_turn_model(&tab_id, observation, "restored-binding");
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.reasoning_effort_identity.actual.as_deref(), Some("low"));
    assert_eq!(
        tab.reasoning_effort_identity.actual_source.as_deref(),
        Some("session-owned")
    );
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("low")
    );
    assert_eq!(
        tab.reasoning_effort_identity.resolved.as_deref(),
        Some("low")
    );
    assert!(tab.pending_model_switch_observation.is_none());
}
