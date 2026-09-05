use super::*;
use crate::service::provider_identity::AgentTypeWriteOutcome;
use crate::service::test_fixture::ServiceFixture;

fn service_fixture_with_sessions(
    items: Vec<(&'static str, Vec<&'static str>)>,
) -> (ServiceFixture, String) {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    register_hook_providers(fixture.service.agent_registry.as_ref(), items);
    (fixture, tab_id)
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn reported_session_rejects_wrong_provider_for_locked_tab() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![
        ("claude", vec!["sid-a", "sid-b", "ses_shared"]),
        ("codex", vec!["sid-a", "sid-b", "ses_shared"]),
    ]);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .cwd = Some("/tmp/project".into());

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "sid-a",
        None,
    )
    .unwrap();
    assert!(matches!(
        fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "claude".into(),
            Some(&tab_id),
            claim.identity_evidence(),
        ),
        Some(AgentTypeWriteOutcome::Applied { .. })
    ));

    let err = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "codex",
        "sid-b",
        None,
    )
    .unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("sid-a")
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn reported_session_binds_sid_owned_by_provider() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec!["claude-real"])]);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .cwd = Some("/tmp/project".into());

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "claude-real",
        Some("nonce-1"),
    )
    .unwrap();
    assert!(matches!(
        fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "claude".into(),
            Some(&tab_id),
            claim.identity_evidence(),
        ),
        Some(AgentTypeWriteOutcome::Applied { .. })
    ));

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("claude-real"));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn fresh_launch_with_matching_nonce_accepts_unpersisted_session() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec![])]);
    {
        let mut g = fixture.service.mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "fresh-unpersisted-sid",
        Some("launch-nonce"),
    )
    .expect("fresh launch with matching nonce must be accepted");
    assert!(claim.changed);
    assert!(claim.nonce_verified);
    assert!(matches!(
        fixture.service.write_hook_tab_agent_type_from_claim(
            &tab_id,
            "claude".into(),
            Some(&tab_id),
            claim.identity_evidence(),
        ),
        Some(AgentTypeWriteOutcome::Applied { .. })
    ));

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(
        tab.agent_session_id.as_deref(),
        Some("fresh-unpersisted-sid")
    );
}
