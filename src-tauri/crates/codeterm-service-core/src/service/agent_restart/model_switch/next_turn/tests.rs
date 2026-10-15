use super::*;

#[test]
fn next_turn_request_keeps_observed_identity_until_new_turn_evidence() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_model = Some("observed-model".into());
    let boundary = codeterm_types::agent::SessionModelTranscriptBoundary {
        last_entry_uuid: Some("prior-turn".into()),
        last_entry_timestamp: Some("123".into()),
        last_entry_type: None,
        boundary_found: true,
    };
    record_live_request_identity(tab, "requested-model", Some("high"));
    record_pending_turn(tab, "own-session", "requested-model", boundary.clone());
    assert_eq!(tab.agent_session_model.as_deref(), Some("observed-model"));
    assert_eq!(
        tab.agent_provider_resolved_model.as_deref(),
        Some("requested-model")
    );
    let pending = tab.pending_model_switch_observation.as_ref().unwrap();
    assert_eq!(pending.session_id, "own-session");
    assert_eq!(pending.transcript_boundary, boundary);
    let document = serde_json::to_value(tab).unwrap();
    assert_eq!(
        document["pending_model_switch_observation"]["launched_model"],
        "requested-model"
    );
    assert_eq!(document["agent_session_model"], "observed-model");
}
