use crate::agent::live_state::observe_tab_liveness;
use crate::agent::offline_state_classifier::{TabVerdict, TimelineState};
use crate::crash::lifecycle::ShutdownReason;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::plan::reconcile::TabProbe;
use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::boot_stage::BootStage;
use crate::service::hierarchy::clear_planner_bindings_for_lifecycle_exit;
use crate::service::plan::PlanStore;
use crate::service::tab::clear_agent_identity_for_lifecycle_exit;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::AgentLiveness;

const SHELL_SUFFIXES: [char; 4] = ['>', '$', '%', '#'];

fn planner_probe(tab: &crate::multiplexer::types::Tab) -> TabProbe {
    let chat_delivery_target = tab.chat_view && tab.pty_id.is_some();
    let agent_accepting_work = chat_delivery_target
        && tab.agent_state.as_ref().is_some_and(|state| {
            matches!(state.state, TimelineState::Working | TimelineState::Idle)
        });
    TabProbe {
        exists: true,
        live_agent: tab.agent_type.is_some(),
        agent_session_id: tab.agent_session_id.clone(),
        session_binding_valid: tab.session_binding_nonce.is_some()
            && tab.session_binding_nonce == tab.reported_session_binding_nonce,
        chat_delivery_target,
        agent_accepting_work,
        role: None,
        parent_tab_id: None,
    }
}

fn seed_live_planner() -> (
    MultiplexerState,
    String,
    Option<PlannerRef>,
    OrchestratorRef,
) {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, "pty-planner".into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.agent_type_locked = true;
    tab.agent_session_id = Some("session-tab-owned".into());
    tab.session_binding_nonce = Some("launch-nonce-tab-owned".into());
    tab.reported_session_binding_nonce = Some("launch-nonce-tab-owned".into());
    tab.agent_session_model = Some("gpt-5.6".into());
    tab.role_profile = Some("planner".into());
    tab.chat_view = true;
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let planner = Some(PlannerRef::Tab {
        tab_id: tab_id.clone(),
        session_id: Some("session-tab-owned".into()),
        title: Some("Planner".into()),
    });
    (
        mux,
        tab_id,
        planner,
        OrchestratorRef::from_tab("orchestrator"),
    )
}

fn assert_live_planner_identity(tab: &crate::multiplexer::types::Tab) {
    assert_eq!(tab.agent_type.as_deref(), Some("codex"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("session-tab-owned"));
    assert_eq!(
        tab.session_binding_nonce.as_deref(),
        Some("launch-nonce-tab-owned")
    );
    assert_eq!(
        tab.reported_session_binding_nonce.as_deref(),
        Some("launch-nonce-tab-owned")
    );
    assert_eq!(tab.agent_session_model.as_deref(), Some("gpt-5.6"));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.agent_type_locked);
    assert!(tab
        .agent_state
        .as_ref()
        .is_some_and(|state| { matches!(state.state, TimelineState::Working) }));
}

fn assert_planner_resolves(
    tab_id: &str,
    planner: &Option<PlannerRef>,
    owner: &OrchestratorRef,
    tab: &crate::multiplexer::types::Tab,
) {
    assert!(matches!(
        resolve_planner_ref(planner, owner, |_| planner_probe(tab)),
        PeerResolution::Planner { tab_id: ref resolved } if resolved == tab_id
    ));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn planner_shell_like_live_tui_preserves_identity_across_snapshot_reload() {
    let (mux, tab_id, planner, owner) = seed_live_planner();
    let harness = ServiceFixture::new(mux);
    let plan = harness
        .service
        .plan_store
        .create_plan_for_team(
            "Live Planner".into(),
            harness.temp.path().display().to_string(),
            owner.clone(),
            "team-live-planner".into(),
            vec![],
            1,
        )
        .unwrap();
    harness
        .service
        .plan_store
        .set_planner_ref(&plan.id, planner, 2)
        .unwrap();

    for suffix in SHELL_SUFFIXES {
        let frame = format!(
            "\x1b[2J\x1b[H  GPT-5.6 Codex\r\n  Working in D:\\Developer\\codeterm\r\n\r\n› continue\r\nshell{suffix}"
        );
        {
            let mut screen_state = harness.service.screen_state.lock();
            screen_state.process("pty-planner", frame.as_bytes());
            assert!(super::screen_ends_in_shell_prompt(
                &screen_state.get_screen_text("pty-planner").unwrap()
            ));
        }
        harness
            .service
            .process_pty_output("pty-planner", frame.as_bytes());
        let stored_planner = harness.service.plan_store.get(&plan.id).unwrap().planner;
        let mux = harness.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_live_planner_identity(tab);
        assert_planner_resolves(&tab_id, &stored_planner, &owner, tab);
    }

    let mux = harness.service.mux.read();
    let snapshot = mux.to_session_snapshot();
    drop(mux);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    let restored_snapshot = serde_json::from_str(&serialized).unwrap();
    let mut restored = MultiplexerState::from_session_snapshot(&restored_snapshot);
    restored.link_pty(&tab_id, "pty-restored".into());
    let stored_planner = harness.service.plan_store.get(&plan.id).unwrap().planner;
    let tab = restored.get_tab(&tab_id).unwrap();
    assert_live_planner_identity(tab);
    assert_planner_resolves(&tab_id, &stored_planner, &owner, tab);
}

#[test]
fn explicit_lifecycle_exit_clears_planner_identity_once() {
    let (mut mux, tab_id, planner, owner) = seed_live_planner();
    let dir = tempfile::tempdir().unwrap();
    let plan_store = PlanStore::new_for_test(dir.path());
    let plan = plan_store
        .create_plan_for_team(
            "Planner lifecycle".into(),
            dir.path().display().to_string(),
            owner.clone(),
            "team-planner".into(),
            vec![],
            1,
        )
        .unwrap();
    plan_store
        .set_planner_ref(&plan.id, planner.clone(), 2)
        .unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();

    assert!(clear_agent_identity_for_lifecycle_exit(tab));
    assert_eq!(
        clear_planner_bindings_for_lifecycle_exit(&plan_store, &tab_id, 3),
        1
    );
    assert!(!clear_agent_identity_for_lifecycle_exit(tab));
    assert_eq!(
        clear_planner_bindings_for_lifecycle_exit(&plan_store, &tab_id, 4),
        0
    );
    assert!(tab.agent_type.is_none());
    assert!(tab.agent_session_id.is_none());
    assert!(tab.session_binding_nonce.is_none());
    assert!(tab.reported_session_binding_nonce.is_none());
    assert!(tab.agent_state.is_none());
    assert_eq!(tab.boot_stage, Some(BootStage::ShellReady));
    assert!(matches!(
        plan_store.get(&plan.id).unwrap().planner,
        Some(PlannerRef::Tab {
            session_id: None,
            ..
        })
    ));
    assert!(matches!(
        resolve_planner_ref(&planner, &owner, |_| planner_probe(tab)),
        PeerResolution::OrchestratorFallback { .. }
    ));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn command_finished_marker_makes_a_ready_agent_observably_absent() {
    let (mut mux, tab_id, _, _) = seed_live_planner();
    mux.get_tab_mut(&tab_id).unwrap().shell_integration_nonce = Some("nonce-current".into());
    let harness = ServiceFixture::new(mux);
    harness
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some("pty-planner".into()),
            None,
            None,
            80,
            24,
            Default::default(),
        )
        .expect("spawn live backing PTY");

    assert_eq!(
        observe_tab_liveness(&harness.service, &tab_id),
        AgentLiveness::Live
    );

    harness
        .service
        .process_pty_output("pty-planner", b"\x1b]633;D\x07");
    assert_eq!(
        observe_tab_liveness(&harness.service, &tab_id),
        AgentLiveness::Live
    );
    assert!(harness
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .agent_type
        .is_some());
    harness
        .service
        .process_pty_output("pty-planner", b"\x1b]633;E;;nonce-wrong\x07\x1b]633;D\x07");
    assert_eq!(
        observe_tab_liveness(&harness.service, &tab_id),
        AgentLiveness::Live
    );
    assert!(harness
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .agent_type
        .is_some());
    harness.service.process_pty_output(
        "pty-planner",
        b"\x1b]633;E;;nonce-current\x07\x1b]633;D\x07",
    );

    assert_eq!(
        observe_tab_liveness(&harness.service, &tab_id),
        AgentLiveness::Absent
    );
    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(tab.agent_type.is_none());
    assert_eq!(tab.boot_stage, Some(BootStage::ShellReady));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restore_contract_provider_exit_during_shutdown_keeps_the_resume_identity() {
    let (mux, tab_id, _, _) = seed_live_planner();
    let harness = ServiceFixture::new(mux);
    let cause = "provider process exited to a surviving shell";

    for reason in [
        ShutdownReason::Update,
        ShutdownReason::UserExit,
        ShutdownReason::Restart,
        ShutdownReason::Signal,
    ] {
        assert!(!harness
            .service
            .clear_tab_agent_identity_for_lifecycle_exit_during(&tab_id, cause, Some(reason)));
        assert_live_planner_identity(harness.service.mux.read().get_tab(&tab_id).unwrap());
    }

    let serialized =
        serde_json::to_string(&harness.service.mux.read().to_session_snapshot()).unwrap();
    let restored_snapshot = serde_json::from_str(&serialized).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&restored_snapshot);
    assert_live_planner_identity(restored.get_tab(&tab_id).unwrap());

    assert!(harness
        .service
        .clear_tab_agent_identity_for_lifecycle_exit_during(&tab_id, cause, None));
    assert!(harness
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .agent_session_id
        .is_none());
}
