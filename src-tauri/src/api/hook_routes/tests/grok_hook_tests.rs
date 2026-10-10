use super::*;

#[test]
fn session_start_hook_binds_grok_compatible_session_id() {
    let body = serde_json::json!({
        "hookEventName": "session_start",
        "hook_event_name": "SessionStart",
        "sessionId": "sid-grok"
    });
    let sid = session_start_session_id(&body).expect("Grok hook session id");
    let provider_id = "grok";
    let registry = registry_with_sessions(vec![(provider_id, vec!["sid-grok"])]);
    let (mux, tab_id) = shared_mux();
    {
        let mut mux = mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some(provider_id.into());
        tab.session_binding_nonce = Some("grok-launch-nonce".into());
    }

    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        provider_id,
        &sid,
        Some("grok-launch-nonce"),
    )
    .expect("Grok hook report binds to its launched tab");
    assert!(claim.changed);
    assert!(claim.nonce_verified);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("sid-grok")
    );
}

#[test]
fn marked_code_term_session_start_requires_binding_nonce() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-codeterm-hook",
        "codeterm-hook-v3".parse().expect("valid marker header"),
    );

    assert!(marked_hook_missing_binding_nonce(&headers, None));
    assert!(marked_hook_missing_binding_nonce(&headers, Some(" ")));
    assert!(!marked_hook_missing_binding_nonce(
        &headers,
        Some("launch-nonce")
    ));
    assert!(!marked_hook_missing_binding_nonce(&HeaderMap::new(), None));
}
