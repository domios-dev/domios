use crate::agent::{provider::ModelSwitchMode, test_provider::MockProvider};
use crate::multiplexer::{state::MultiplexerState, types::PendingModelSwitchObservation};
use crate::service::test_fixture::ServiceFixture;

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn next_turn_first_own_record_settles_match_or_failure() {
    for actual in [None, Some("vendor/target"), Some("vendor/old")] {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_session_id = Some("owned-session".into());
        tab.session_binding_nonce = Some("binding".into());
        tab.reported_session_binding_nonce = Some("binding".into());
        tab.cwd = Some("/fixture/project".into());
        tab.agent_requested_model = Some("vendor/target".into());
        tab.agent_provider_resolved_model = Some("vendor/target".into());
        let observation = PendingModelSwitchObservation {
            session_id: "owned-session".into(),
            launched_model: "vendor/target".into(),
            transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary {
                boundary_found: true,
                ..Default::default()
            },
        };
        tab.pending_model_switch_observation = Some(observation.clone());
        let fixture = ServiceFixture::new(mux);
        let mut provider = MockProvider::new("fixture-provider")
            .with_model_switch(ModelSwitchMode::NextTurn)
            .with_session_model("foreign-session", "vendor/target");
        if let Some(actual) = actual {
            provider = provider.with_session_model("owned-session", actual);
        }
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));
        fixture
            .service
            .record_post_launch_turn_model(&tab_id, observation.clone(), "stale");
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .pending_model_switch_observation,
            Some(observation.clone())
        );
        fixture
            .service
            .record_post_launch_turn_model(&tab_id, observation, "binding");
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(
            tab.pending_model_switch_observation.is_some(),
            actual.is_none()
        );
        assert_eq!(tab.agent_session_model.as_deref(), actual);
        assert_eq!(
            tab.model_identity_mismatch.is_some(),
            actual == Some("vendor/old")
        );
        if let Some(mismatch) = &tab.model_identity_mismatch {
            assert_eq!(mismatch.requested_model, "vendor/target");
            assert_eq!(mismatch.actual_model, "vendor/old");
        }
    }
}
