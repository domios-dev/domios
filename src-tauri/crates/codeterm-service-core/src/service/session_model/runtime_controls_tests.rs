use super::*;

#[test]
fn runtime_confirmation_projects_all_controls_without_changing_requested_identity() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_session_id = Some("thread".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    tab.agent_requested_model = Some("requested".into());
    let settings = codeterm_plugin::agent_runtime::RuntimeSettings {
        model: Some("actual".into()),
        effort: Some("high".into()),
        mode: Some("approval-mode".into()),
        ..Default::default()
    };
    apply_runtime_controls(tab, "thread", &settings);
    assert_eq!(tab.agent_session_model.as_deref(), Some("actual"));
    assert_eq!(tab.agent_provider_resolved_model.as_deref(), Some("actual"));
    assert_eq!(tab.agent_requested_model.as_deref(), Some("requested"));
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.structured_runtime_mode.as_deref(),
        Some("approval-mode")
    );
    assert_eq!(
        tab.agent_session_model_identity
            .as_ref()
            .unwrap()
            .session_binding_nonce,
        "nonce"
    );
}

#[test]
fn fast_tier_projection_requires_confirmation_and_retains_own_binding() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.session_binding_nonce = Some("nonce".into());
    tab.agent_runtime_config = Some(serde_json::json!({"fastMode":true}));
    apply_runtime_controls(tab, "thread", &Default::default());
    assert!(tab
        .agent_runtime_config
        .as_ref()
        .unwrap()
        .get("confirmedFastMode")
        .is_none());
    for enabled in [true, false] {
        apply_runtime_controls(
            tab,
            "thread",
            &codeterm_plugin::agent_runtime::RuntimeSettings {
                fast_mode: Some(enabled),
                ..Default::default()
            },
        );
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["confirmedFastMode"],
            serde_json::json!({"enabled":enabled,"sessionId":"thread","bindingNonce":"nonce"})
        );
    }
}
