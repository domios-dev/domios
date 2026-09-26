use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
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
    fixture
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some("resume-live-pty".into()),
            None,
            None,
            80,
            24,
            Default::default(),
        )
        .expect("spawn live backing PTY");
    let restored_ids = [launching_tab_id.clone(), waiting_tab_id.clone()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    crate::crash::restore_report::arm(
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
    assert_eq!(failed.agent_type, None);
    assert_eq!(failed.agent_session_id, None);
    assert_eq!(
        failed
            .prior_session_binding_nonces
            .last()
            .and_then(|entry| entry.session_id.as_deref()),
        Some("saved-session"),
        "failed identity is retired to session lineage even while another pane keeps the wave pending"
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
