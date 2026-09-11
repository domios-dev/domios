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

/// The first-run contradiction, end to end on a real ServiceContext: a tab
/// latched at `spawn_failed` whose agent then authenticates its own launch must
/// not keep reporting a failed spawn, and no message to the tab is involved.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn a_nonce_verified_binding_retires_a_latched_spawn_failure() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec![])]);
    {
        let mut g = fixture.service.mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
        tab.boot_failure_reason = Some("submit_unconfirmed: acknowledgement not observed".into());
    }

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab_id,
        "claude",
        "late-bound-sid",
        Some("launch-nonce"),
    )
    .expect("the launch's own agent must be allowed to bind");
    assert!(claim.nonce_verified);

    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady),
    );
    assert_eq!(tab.boot_failure_reason, None);
}

/// The over-correction guard: a report that cannot prove the launch leaves the
/// failure exactly where it was.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn an_unproven_report_leaves_a_spawn_failure_latched() {
    let (fixture, tab_id) = service_fixture_with_sessions(vec![("claude", vec![])]);
    {
        let mut g = fixture.service.mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some("some-sid".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
        tab.boot_failure_reason = Some("PtyEof".into());
    }

    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);

    let g = fixture.service.mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::SpawnFailed),
    );
    assert_eq!(tab.boot_failure_reason.as_deref(), Some("PtyEof"));
}
