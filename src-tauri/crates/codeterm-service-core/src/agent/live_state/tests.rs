use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn restored_prompt_without_a_session_is_live_with_a_live_pty() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "pty-restored-prompt-no-session";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("prompt-fixture".into());
        tab.pty_id = Some(pty_id.into());
        tab.boot_stage = Some(crate::service::boot_stage::BootStage::AwaitingOwnerPrompt);
        tab.agent_booting = true;
        tab.agent_session_id = None;
        tab.restored_from_snapshot = true;
        tab.pending_prompt = Some(crate::multiplexer::types::PendingPrompt {
            request_id: None,
            question: "prompt-token".into(),
            options: Vec::new(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        });
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    {
        let gate = fixture.service.pty.lock().spawn_gate();
        crate::pty::manager::build_session(gate, None, None, 80, 24, Default::default()).map(
            |built| {
                fixture
                    .service
                    .pty
                    .lock()
                    .register_session(Some(pty_id.into()), built)
            },
        )
    }
    .expect("spawn live backing PTY");

    assert_eq!(
        observe_tab_liveness(&fixture.service, &tab_id),
        AgentLiveness::Live
    );

    fixture
        .service
        .pty
        .lock()
        .kill(pty_id)
        .expect("stop live backing PTY");
}

#[test]
fn restore_wave_does_not_clear_identity_of_a_launching_tab() {
    let (mut mux, launching_tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let waiting_tab_id = mux.create_tab_record();
    let launching = mux.get_tab_mut(&launching_tab_id).unwrap();
    launching.agent_type = Some("resume-fixture".into());
    launching.agent_session_id = Some("saved-session".into());
    launching.session_binding_nonce = Some("launch-nonce".into());
    launching.pty_id = Some("resume-live-pty".into());
    launching.boot_stage = Some(crate::service::boot_stage::BootStage::SpawnFailed);
    launching.agent_booting = true;
    let waiting = mux.get_tab_mut(&waiting_tab_id).unwrap();
    waiting.agent_type = Some("unobserved-fixture".into());
    waiting.agent_session_id = Some("unobserved-session".into());
    waiting.boot_stage = Some(crate::service::boot_stage::BootStage::ShellReady);

    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    {
        let gate = fixture.service.pty.lock().spawn_gate();
        crate::pty::manager::build_session(gate, None, None, 80, 24, Default::default()).map(
            |built| {
                fixture
                    .service
                    .pty
                    .lock()
                    .register_session(Some("resume-live-pty".into()), built)
            },
        )
    }
    .expect("spawn live backing PTY");
    let restored_ids = [launching_tab_id.clone(), waiting_tab_id.clone()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let data_dir = tempfile::tempdir().expect("report tempdir");
    crate::crash::restore_report::arm(
        data_dir.path().to_path_buf(),
        vec![
            crate::crash::restore_report::RestorePane {
                tab_id: launching_tab_id.clone(),
                role: None,
                agent_type: Some("resume-fixture".into()),
                agent_session_id: Some("saved-session".into()),
                exited_session_id: None,
            },
            crate::crash::restore_report::RestorePane {
                tab_id: waiting_tab_id.clone(),
                role: None,
                agent_type: Some("unobserved-fixture".into()),
                agent_session_id: Some("unobserved-session".into()),
                exited_session_id: None,
            },
        ],
        &restored_ids,
    );

    fixture.service.settle_restore_report();
    let report_path = data_dir
        .path()
        .join("state")
        .join("recovery")
        .join("last_restore_report.json");
    let persisted: crate::crash::restore_report::RestoreReport =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    assert!(
        !persisted.finalized,
        "the fixture wave persists only inside its own tempdir"
    );
    assert_eq!(
        observe_tab_liveness(&fixture.service, &launching_tab_id),
        AgentLiveness::Unobserved,
        "an in-flight launch outranks a stale failure stage"
    );
    let in_flight = fixture
        .service
        .mux
        .read()
        .get_tab(&launching_tab_id)
        .cloned()
        .expect("launching restored tab remains present");
    assert!(in_flight.agent_type.is_some());
    assert_eq!(in_flight.agent_session_id.as_deref(), Some("saved-session"));

    {
        let mut mux = fixture.service.mux.write();
        let launching = mux.get_tab_mut(&launching_tab_id).unwrap();
        launching.agent_booting = false;
        launching.restored_from_snapshot = false;
        launching.boot_stage = Some(crate::service::boot_stage::BootStage::SpawnFailed);
    }
    fixture.service.settle_restore_report();
    assert_eq!(
        observe_tab_liveness(&fixture.service, &launching_tab_id),
        AgentLiveness::Absent,
        "the failed launch becomes absent only after it stops being in flight"
    );
    let failed = fixture
        .service
        .mux
        .read()
        .get_tab(&launching_tab_id)
        .cloned()
        .expect("failed restored tab remains present");
    assert_eq!(failed.agent_type.as_deref(), Some("resume-fixture"));
    assert_eq!(failed.agent_session_id.as_deref(), Some("saved-session"));
    assert_eq!(
        failed.boot_stage,
        Some(crate::service::boot_stage::BootStage::SpawnFailed)
    );

    crate::crash::restore_report::settle(
        &restored_ids,
        [
            (launching_tab_id, AgentLiveness::Live),
            (waiting_tab_id, AgentLiveness::Live),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
    );
    fixture
        .service
        .pty
        .lock()
        .kill("resume-live-pty")
        .expect("stop live backing PTY");
}

#[test]
fn restored_tab_not_cleared_before_relaunch_outcome() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("restore-fixture".into());
        tab.agent_session_id = Some("saved-session".into());
        tab.session_binding_nonce = Some("saved-nonce".into());
        tab.restored_from_snapshot = true;
        tab.protected = false;
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    assert!(!fixture
        .service
        .reconcile_tab_restored_without_agent(&tab_id));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.agent_type.as_deref() == Some("restore-fixture")
            && tab.agent_session_id.as_deref() == Some("saved-session")));

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.restored_from_snapshot = false;
        tab.boot_stage = Some(crate::service::boot_stage::BootStage::SpawnFailed);
    }
    assert!(!fixture
        .service
        .reconcile_tab_restored_without_agent(&tab_id));
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(tab.agent_type.as_deref(), Some("restore-fixture"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
    assert_eq!(
        tab.boot_stage,
        Some(crate::service::boot_stage::BootStage::SpawnFailed)
    );
}

#[test]
fn unobserved_liveness_pane_remains_relaunchable() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("restore-fixture".into());
        tab.agent_session_id = Some("saved-session".into());
        tab.restored_from_snapshot = true;
        tab.protected = false;
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    assert_eq!(
        observe_tab_liveness(&fixture.service, &tab_id),
        AgentLiveness::Unobserved
    );
    let claim = crate::service::pty_io::claim_restored_pty_boot(&fixture.service.mux, &tab_id)
        .unwrap()
        .expect("unobserved restored tab remains eligible for relaunch");
    assert_eq!(claim.session_id.as_deref(), Some("saved-session"));
    assert!(
        crate::service::pty_io::claim_restored_pty_boot(&fixture.service.mux, &tab_id)
            .unwrap()
            .is_none()
    );
}

fn evidence(
    stage: crate::service::boot_stage::BootStage,
    in_flight: bool,
    pty_live: Option<bool>,
) -> LivenessEvidence {
    LivenessEvidence {
        declares_agent: true,
        backing: AgentBacking::Pty { pty_live },
        boot_stage: Some(stage),
        boot_in_flight: in_flight,
        provider_exit_observed: false,
        pending_prompt: false,
    }
}

#[test]
fn restore_classifier_counts_failed_and_unbound_launches_as_observed() {
    use crate::service::boot_stage::BootStage;
    use RestoreVerdict::{Observed, Unbound};

    let rows = [
        // stage, in flight, pty, snapshot pending, authenticated, deadline, verdict
        (
            BootStage::SpawnFailed,
            false,
            None,
            true,
            false,
            false,
            Observed(AgentLiveness::Absent),
        ),
        (
            BootStage::SpawnFailed,
            false,
            None,
            false,
            false,
            false,
            Observed(AgentLiveness::Absent),
        ),
        (
            BootStage::SpawnFailed,
            true,
            Some(true),
            false,
            false,
            false,
            Observed(AgentLiveness::Unobserved),
        ),
        (
            BootStage::AgentReady,
            false,
            Some(true),
            false,
            false,
            true,
            Unbound,
        ),
        (
            BootStage::LaunchWritten,
            false,
            Some(true),
            false,
            false,
            true,
            Unbound,
        ),
        (
            BootStage::LaunchBindingUnconfirmed,
            false,
            Some(true),
            false,
            false,
            true,
            Unbound,
        ),
        (
            BootStage::AgentReady,
            false,
            Some(true),
            false,
            false,
            false,
            Observed(AgentLiveness::Unobserved),
        ),
        (
            BootStage::LaunchWritten,
            true,
            Some(true),
            false,
            false,
            true,
            Observed(AgentLiveness::Unobserved),
        ),
        (
            BootStage::AgentReady,
            false,
            Some(true),
            true,
            false,
            true,
            Observed(AgentLiveness::Unobserved),
        ),
        (
            BootStage::ShellReady,
            false,
            Some(true),
            false,
            false,
            true,
            Observed(AgentLiveness::Unobserved),
        ),
        (
            BootStage::AgentReady,
            false,
            Some(true),
            false,
            true,
            true,
            Observed(AgentLiveness::Live),
        ),
    ];
    for (stage, in_flight, pty_live, snapshot_pending, authenticated, deadline, expected) in rows {
        let verdict = classify_restore_facts(RestoreFacts {
            evidence: evidence(stage, in_flight, pty_live),
            snapshot_pending,
            authenticated,
            first_turn_ready: false,
            binding_deadline_elapsed: deadline,
        });
        assert_eq!(
            verdict, expected,
            "stage={stage:?} in_flight={in_flight} pty={pty_live:?} pending={snapshot_pending} auth={authenticated} deadline={deadline}"
        );
    }
    assert_eq!(Unbound.liveness(), AgentLiveness::Absent);
    assert!(
        !Unbound.reconciles(),
        "an unbound launch keeps its identity for recovery"
    );
    assert!(Observed(AgentLiveness::Absent).reconciles());
}
