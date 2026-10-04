use super::*;

fn launched_tab(provider: &str, nonce: Option<&str>) -> (MuxState, String) {
    let (mux, tab_id) = shared_mux();
    {
        let mut state = mux.write();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some(provider.into());
        tab.session_binding_nonce = nonce.map(str::to_string);
    }
    (mux, tab_id)
}

fn bound_session(mux: &MuxState, tab_id: &str) -> Option<String> {
    mux.read().get_tab(tab_id).unwrap().agent_session_id.clone()
}

#[test]
fn detected_session_binds_the_launch_it_names() {
    let (mux, tab_id) = launched_tab("claude", Some("nonce-1"));
    let registry = registry(&["claude"]);

    let claim =
        bind_detected_session(&mux, &registry, &tab_id, "claude", "sid-a", "nonce-1").unwrap();

    assert!(claim.nonce_verified);
    assert!(claim.changed);
    assert_eq!(bound_session(&mux, &tab_id).as_deref(), Some("sid-a"));
}

#[test]
fn detection_for_a_rotated_launch_is_refused() {
    let (mux, tab_id) = launched_tab("claude", Some("nonce-2"));

    let err = bind_detected_session(
        &mux,
        &registry(&["claude"]),
        &tab_id,
        "claude",
        "sid-a",
        "nonce-1",
    )
    .unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(bound_session(&mux, &tab_id), None);
}

#[test]
fn detection_without_a_launch_nonce_is_refused() {
    let (mux, tab_id) = launched_tab("claude", None);

    let err = bind_detected_session(
        &mux,
        &registry(&["claude"]),
        &tab_id,
        "claude",
        "sid-a",
        "nonce-1",
    )
    .unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(bound_session(&mux, &tab_id), None);
}

#[test]
fn hook_binding_stays_primary_over_detection() {
    let (mux, tab_id) = launched_tab("claude", Some("nonce-1"));
    let registry = registry(&["claude"]);
    bind_reported_session(&mux, &registry, &tab_id, "claude", "sid-a", Some("nonce-1")).unwrap();

    let err =
        bind_detected_session(&mux, &registry, &tab_id, "claude", "sid-b", "nonce-1").unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(bound_session(&mux, &tab_id).as_deref(), Some("sid-a"));
}

#[test]
fn detection_for_another_providers_tab_is_refused() {
    let (mux, tab_id) = launched_tab("codex", Some("nonce-1"));

    let err = bind_detected_session(
        &mux,
        &registry(&["claude", "codex"]),
        &tab_id,
        "claude",
        "sid-a",
        "nonce-1",
    )
    .unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(bound_session(&mux, &tab_id), None);
}

#[test]
fn detection_cannot_claim_another_tabs_session() {
    let (mux, first) = launched_tab("claude", Some("nonce-1"));
    let registry = registry(&["claude"]);
    bind_reported_session(&mux, &registry, &first, "claude", "sid-a", Some("nonce-1")).unwrap();
    let second = {
        let mut state = mux.write();
        let workspace = state.create_workspace("second".into(), None).id;
        let second = state.create_tab(&workspace).unwrap();
        let tab = state.get_tab_mut(&second).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.session_binding_nonce = Some("nonce-2".into());
        second
    };

    let err =
        bind_detected_session(&mux, &registry, &second, "claude", "sid-a", "nonce-2").unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(bound_session(&mux, &second), None);
}
