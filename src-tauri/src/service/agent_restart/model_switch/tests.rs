use std::sync::Arc;

use parking_lot::Mutex;

use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

const SESSION: &str = "session-7f3a";
const NONCE: &str = "nonce-7f3a";
const PRIOR_MODEL: &str = "prior-model-a1";
const NEXT_MODEL: &str = "next-model-b2";
const EFFORT: &str = "effort-c3";

fn seeded_tab(provider: &str) -> (MultiplexerState, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.cwd = Some(std::env::temp_dir().display().to_string());
    tab.agent_type = Some(provider.into());
    tab.agent_type_locked = true;
    tab.pty_id = Some("pty-7f3a".into());
    tab.agent_session_id = Some(SESSION.into());
    tab.session_binding_nonce = Some(NONCE.into());
    tab.reported_session_binding_nonce = Some(NONCE.into());
    tab.agent_requested_model = Some(PRIOR_MODEL.into());
    tab.agent_provider_resolved_model = Some(PRIOR_MODEL.into());
    tab.restorable_launch = crate::multiplexer::types::RestorableLaunchParameters {
        captured: true,
        requested_model: Some(PRIOR_MODEL.into()),
        model: Some(PRIOR_MODEL.into()),
        requested_reasoning_effort: Some(EFFORT.into()),
        resolved_reasoning_effort: Some(EFFORT.into()),
        ..Default::default()
    };
    (mux, tab_id)
}

fn snapshot(tab_id: &str, provider: &str) -> TabRestartSnapshot {
    TabRestartSnapshot {
        tab_id: tab_id.into(),
        pty_id: "pty-7f3a".into(),
        provider: provider.into(),
        session_id: Some(SESSION.into()),
        cwd: Some(std::env::temp_dir().display().to_string()),
        auto_approve: false,
        role: None,
        role_profile: None,
    }
}

fn restart_attempted(fixture: &ServiceFixture, tab_id: &str) -> bool {
    let tracker = fixture.service.restart_tracker.lock();
    tracker.last_restart.contains_key(tab_id) || tracker.in_progress.contains(tab_id)
}

/// The capability alone decides the path. An unsupported provider is refused
/// before a restart claim exists, and the claim is the only road to the
/// graceful-exit write, so nothing reaches the tab and its record is untouched.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn agent_model_switch_uses_provider_capability() {
    let (mut mux, fixed_tab) = seeded_tab("fixed-7f3a");
    let workspace = mux.create_workspace("model switch".into(), None).id;
    let switchable_tab = mux.create_tab(&workspace).unwrap();
    {
        let tab = mux.get_tab_mut(&switchable_tab).unwrap();
        tab.agent_type = Some("switchable-7f3a".into());
        tab.agent_type_locked = true;
        tab.pty_id = Some("pty-8e4b".into());
    }
    let fixture = ServiceFixture::new(mux);
    let registry = &fixture.service.agent_registry;
    registry.register_provider(Box::new(MockProvider::with_session("fixed-7f3a", SESSION)));
    registry.register_provider(Box::new(
        MockProvider::with_session("switchable-7f3a", SESSION)
            .with_model_switch(ModelSwitchMode::RestartResume),
    ));
    let before = fixture.service.mux.read().get_tab(&fixed_tab).map(|tab| {
        (
            tab.restorable_launch.clone(),
            tab.agent_requested_model.clone(),
        )
    });

    let refused = fixture.service.agent_switch_model(&fixed_tab, NEXT_MODEL);

    assert!(
        matches!(&refused, Err(ModelSwitchError::Unsupported { provider }) if provider == "fixed-7f3a"),
        "{refused:?}"
    );
    assert!(!restart_attempted(&fixture, &fixed_tab));
    let after = fixture.service.mux.read().get_tab(&fixed_tab).map(|tab| {
        (
            tab.restorable_launch.clone(),
            tab.agent_requested_model.clone(),
        )
    });
    assert_eq!(before, after);

    let gated = fixture
        .service
        .agent_switch_model(&switchable_tab, NEXT_MODEL);
    assert!(
        !matches!(gated, Err(ModelSwitchError::Unsupported { .. })),
        "a restart_resume provider passes the capability gate: {gated:?}"
    );
    assert!(
        matches!(gated, Err(ModelSwitchError::NotSwitchable(_))),
        "an unobserved agent is refused before any relaunch: {gated:?}"
    );
    assert!(!restart_attempted(&fixture, &switchable_tab));
}

/// The relaunch resumes the tab's own session with the new model's launch
/// args, and committing it rewrites the spawn record a daemon restore reads.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn agent_model_switch_updates_spawn_record_for_restore() {
    const PROVIDER: &str = "switchable-9d5c";
    let (mux, tab_id) = seeded_tab(PROVIDER);
    let fixture = ServiceFixture::new(mux);
    let resumes = Arc::new(Mutex::new(Vec::new()));
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session(PROVIDER, SESSION)
            .with_model_switch(ModelSwitchMode::RestartResume)
            .with_resume_context_log(resumes.clone()),
    ));
    let snap = snapshot(&tab_id, PROVIDER);

    let launch = ModelSwitchLaunch::prepare(&fixture.service, &snap, SESSION, NEXT_MODEL)
        .expect("switch launch prepares");
    assert_eq!(launch.launched_model(), NEXT_MODEL);
    let (_command, _task) = launch
        .command(&fixture.service, &snap)
        .expect("switch relaunch command");
    let resumed = resumes.lock().last().cloned().expect("relaunch resumes");
    assert_eq!(resumed.session_id, SESSION);
    assert!(
        resumed.args.iter().any(|arg| arg == NEXT_MODEL),
        "{resumed:?}"
    );
    assert!(
        !resumed.args.iter().any(|arg| arg == PRIOR_MODEL),
        "{resumed:?}"
    );

    launch.commit(&fixture.service, &tab_id);

    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.restorable_launch.model.as_deref(), Some(NEXT_MODEL));
    assert_eq!(
        tab.restorable_launch.requested_model.as_deref(),
        Some(NEXT_MODEL)
    );
    assert_eq!(
        tab.restorable_launch.resolved_reasoning_effort.as_deref(),
        Some(EFFORT)
    );
    assert_eq!(tab.agent_requested_model.as_deref(), Some(NEXT_MODEL));
    assert_eq!(
        tab.agent_provider_resolved_model.as_deref(),
        Some(NEXT_MODEL)
    );
    assert_eq!(
        recorded_launch_parameters(tab).model.as_deref(),
        Some(NEXT_MODEL)
    );

    let encoded = serde_json::to_string(&mux.to_session_snapshot()).unwrap();
    let snapshot: crate::session::SessionSnapshot = serde_json::from_str(&encoded).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(
        restored
            .get_tab(&tab_id)
            .and_then(|tab| tab.restorable_launch.model.clone())
            .as_deref(),
        Some(NEXT_MODEL)
    );
}

/// A pre-switch report never satisfies observation for a relaunched model.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn agent_model_switch_observes_only_the_current_binding_report() {
    const PROVIDER: &str = "switchable-2b7e";
    let (mut mux, tab_id) = seeded_tab(PROVIDER);
    mux.get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_model_identity = Some(crate::multiplexer::types::SessionModelIdentity {
        session_id: SESSION.into(),
        session_binding_nonce: "nonce-before-switch".into(),
        launch_report: Some(crate::multiplexer::types::LaunchModelReport {
            model: NEXT_MODEL.into(),
            transcript_model_at_launch: None,
        }),
        model_switch_turn_confirmed: false,
    });
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session(PROVIDER, SESSION)
            .with_model_switch(ModelSwitchMode::RestartResume),
    ));

    let stale = fixture.service.await_model_switch_observation(
        &tab_id,
        PROVIDER,
        SESSION,
        NEXT_MODEL,
        std::time::Duration::ZERO,
    );
    assert_eq!(stale, None);

    fixture
        .service
        .record_launch_reported_model(&tab_id, PROVIDER, SESSION, NEXT_MODEL);
    let observed = fixture.service.await_model_switch_observation(
        &tab_id,
        PROVIDER,
        SESSION,
        NEXT_MODEL,
        std::time::Duration::ZERO,
    );
    assert_eq!(
        observed,
        Some(ModelSwitchObservation {
            model: NEXT_MODEL.into(),
            evidence: ModelSwitchEvidence::SessionStartReport,
        })
    );
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn agent_model_switch_waits_past_a_mismatching_start_report_for_the_first_turn() {
    const PROVIDER: &str = "switchable-first-turn-2b7e";
    let (mut mux, tab_id) = seeded_tab(PROVIDER);
    mux.get_tab_mut(&tab_id)
        .unwrap()
        .pending_model_switch_observation =
        Some(crate::multiplexer::types::PendingModelSwitchObservation {
            session_id: SESSION.into(),
            launched_model: NEXT_MODEL.into(),
            transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary {
                last_entry_uuid: None,
            },
        });
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session(PROVIDER, SESSION)
            .with_model_switch(ModelSwitchMode::RestartResume),
    ));
    fixture
        .service
        .record_launch_reported_model(&tab_id, PROVIDER, SESSION, PRIOR_MODEL);

    let service = fixture.service.clone();
    let waiter_tab = tab_id.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        service.await_model_switch_observation(
            &waiter_tab,
            PROVIDER,
            SESSION,
            NEXT_MODEL,
            std::time::Duration::from_secs(2),
        )
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_model = Some(NEXT_MODEL.into());
        tab.agent_session_model_identity = Some(crate::multiplexer::types::SessionModelIdentity {
            session_id: SESSION.into(),
            session_binding_nonce: NONCE.into(),
            launch_report: None,
            model_switch_turn_confirmed: true,
        });
        tab.pending_model_switch_observation = None;
    }
    crate::service::boot_wait::notify_boot_changed();

    assert_eq!(
        waiter.join().unwrap(),
        Some(ModelSwitchObservation {
            model: NEXT_MODEL.into(),
            evidence: ModelSwitchEvidence::FirstPostRelaunchTurn,
        })
    );
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn model_switch_without_model_evidence_returns_pending_confirmation() {
    const PROVIDER: &str = "switchable-pending-2b7e";
    let (mut mux, tab_id) = seeded_tab(PROVIDER);
    mux.get_tab_mut(&tab_id)
        .unwrap()
        .pending_model_switch_observation =
        Some(crate::multiplexer::types::PendingModelSwitchObservation {
            session_id: SESSION.into(),
            launched_model: NEXT_MODEL.into(),
            transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary {
                last_entry_uuid: None,
            },
        });
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session(PROVIDER, SESSION)
            .with_model_switch(ModelSwitchMode::RestartResume),
    ));

    let observation = fixture.service.await_model_switch_observation(
        &tab_id,
        PROVIDER,
        SESSION,
        NEXT_MODEL,
        std::time::Duration::ZERO,
    );
    assert_eq!(observation, None);
    let confirmation = model_switch_confirmation(
        &fixture.service.agent_registry,
        PROVIDER,
        NEXT_MODEL,
        observation,
    )
    .expect("an unobserved model leaves confirmation pending");

    assert_eq!(confirmation, ModelSwitchConfirmation::Pending);
}
