use super::*;

#[test]
fn completed_launch_effort_preserves_authenticated_runtime_observation() {
    let (mut state, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    persist_launch_identity(tab, None, Some("low"));
    assert!(apply_session_owned_actual(
        tab,
        "fixture-provider",
        "own-session",
        Some("own-nonce"),
        Some("high".into()),
    ));
    persist_completed_launch_identity(tab, Some("medium"), Some("low"));
    assert_eq!(
        tab.reasoning_effort_identity.planned.as_deref(),
        Some("medium")
    );
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("low")
    );
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.actual_source.as_deref(),
        Some(SESSION_OWNED)
    );
}

#[test]
fn completed_launch_effort_clears_unverified_or_stale_observation() {
    for (session, nonce, reported, source) in [
        (
            Some("own-session"),
            Some("new-nonce"),
            Some("old-nonce"),
            SESSION_OWNED,
        ),
        (None, Some("own-nonce"), Some("own-nonce"), SESSION_OWNED),
        (Some("own-session"), None, None, SESSION_OWNED),
        (
            Some("own-session"),
            Some("own-nonce"),
            Some("own-nonce"),
            UNVERIFIED,
        ),
    ] {
        let (mut state, id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = state.get_tab_mut(&id).unwrap();
        tab.agent_session_id = session.map(str::to_owned);
        tab.session_binding_nonce = nonce.map(str::to_owned);
        tab.reported_session_binding_nonce = reported.map(str::to_owned);
        tab.reasoning_effort_identity.actual = Some("high".into());
        tab.reasoning_effort_identity.actual_source = Some(source.into());
        persist_completed_launch_identity(tab, None, Some("low"));
        assert_eq!(tab.reasoning_effort_identity.actual, None);
        assert_eq!(
            tab.reasoning_effort_identity.actual_source.as_deref(),
            Some(UNVERIFIED)
        );
    }
}
