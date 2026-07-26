use super::{decide_agent_output_lifecycle, AgentOutputLifecycleDecision};
use crate::agent::offline_state_classifier::{PaneVerdict, TimelineState};
use crate::multiplexer::state::MultiplexerState;
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::plan::reconcile::PaneProbe;
use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::boot_stage::BootStage;
use crate::service::hierarchy::clear_planner_bindings_for_lifecycle_exit;
use crate::service::pane::clear_agent_identity_for_lifecycle_exit;
use crate::service::plan::PlanStore;

const SHELL_SUFFIXES: [char; 4] = ['>', '$', '%', '#'];

fn planner_probe(pane: &crate::multiplexer::types::Pane) -> PaneProbe {
    let chat_delivery_target = pane.chat_view && pane.pty_id.is_some();
    let agent_accepting_work = chat_delivery_target
        && pane.agent_state.as_ref().is_some_and(|state| {
            matches!(state.state, TimelineState::Working | TimelineState::Idle)
        });
    PaneProbe {
        exists: true,
        live_agent: pane.agent_type.is_some(),
        agent_session_id: pane.agent_session_id.clone(),
        session_binding_valid: pane.session_binding_nonce.is_some()
            && pane.session_binding_nonce == pane.reported_session_binding_nonce,
        chat_delivery_target,
        agent_accepting_work,
        role: None,
        parent_pane_id: None,
    }
}

fn seed_live_planner() -> (
    MultiplexerState,
    String,
    Option<PlannerRef>,
    OrchestratorRef,
) {
    let (mut mux, pane_id) = MultiplexerState::new();
    let pane = mux.get_pane_mut(&pane_id).unwrap();
    pane.pty_id = Some("pty-planner".into());
    pane.agent_type = Some("codex".into());
    pane.agent_type_locked = true;
    pane.agent_session_id = Some("session-pane-owned".into());
    pane.session_binding_nonce = Some("launch-nonce-pane-owned".into());
    pane.reported_session_binding_nonce = Some("launch-nonce-pane-owned".into());
    pane.agent_session_model = Some("gpt-5.6".into());
    pane.role_profile = Some("planner".into());
    pane.chat_view = true;
    pane.boot_stage = Some(BootStage::AgentReady);
    pane.agent_state = Some(
        PaneVerdict {
            state: TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let planner = Some(PlannerRef::Pane {
        pane_id: pane_id.clone(),
        session_id: Some("session-pane-owned".into()),
        title: Some("Planner".into()),
    });
    (
        mux,
        pane_id,
        planner,
        OrchestratorRef::from_pane("orchestrator"),
    )
}

fn assert_live_planner_identity(pane: &crate::multiplexer::types::Pane) {
    assert_eq!(pane.agent_type.as_deref(), Some("codex"));
    assert_eq!(pane.agent_session_id.as_deref(), Some("session-pane-owned"));
    assert_eq!(
        pane.session_binding_nonce.as_deref(),
        Some("launch-nonce-pane-owned")
    );
    assert_eq!(
        pane.reported_session_binding_nonce.as_deref(),
        Some("launch-nonce-pane-owned")
    );
    assert_eq!(pane.agent_session_model.as_deref(), Some("gpt-5.6"));
    assert_eq!(pane.boot_stage, Some(BootStage::AgentReady));
    assert!(pane.agent_type_locked);
    assert!(pane
        .agent_state
        .as_ref()
        .is_some_and(|state| { matches!(state.state, TimelineState::Working) }));
}

fn assert_planner_resolves(
    pane_id: &str,
    planner: &Option<PlannerRef>,
    owner: &OrchestratorRef,
    pane: &crate::multiplexer::types::Pane,
) {
    assert!(matches!(
        resolve_planner_ref(planner, owner, |_| planner_probe(pane)),
        PeerResolution::Planner { pane_id: ref resolved } if resolved == pane_id
    ));
}

#[test]
fn planner_shell_like_live_tui_preserves_identity_across_snapshot_reload() {
    let (mux, pane_id, planner, owner) = seed_live_planner();

    for suffix in SHELL_SUFFIXES {
        let frame = format!(
            "  GPT-5.6 Codex\n  Working in D:\\Developer\\codeterm\n\n› continue\nshell{suffix}"
        );
        assert_eq!(
            decide_agent_output_lifecycle(&frame, false),
            AgentOutputLifecycleDecision::PreserveIdentityAndLogShellLikeFrame
        );
        let pane = mux.get_pane(&pane_id).unwrap();
        assert_live_planner_identity(pane);
        assert_planner_resolves(&pane_id, &planner, &owner, pane);
    }

    let snapshot = mux.to_session_snapshot();
    let serialized = serde_json::to_string(&snapshot).unwrap();
    let restored_snapshot = serde_json::from_str(&serialized).unwrap();
    let mut restored = MultiplexerState::from_session_snapshot(&restored_snapshot);
    restored.get_pane_mut(&pane_id).unwrap().pty_id = Some("pty-restored".into());
    let pane = restored.get_pane(&pane_id).unwrap();
    assert_live_planner_identity(pane);
    assert_planner_resolves(&pane_id, &planner, &owner, pane);
}

#[test]
fn explicit_lifecycle_exit_clears_planner_identity_once() {
    let (mut mux, pane_id, planner, owner) = seed_live_planner();
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
    let pane = mux.get_pane_mut(&pane_id).unwrap();

    assert!(clear_agent_identity_for_lifecycle_exit(pane));
    assert_eq!(
        clear_planner_bindings_for_lifecycle_exit(&plan_store, &pane_id, 3),
        1
    );
    assert!(!clear_agent_identity_for_lifecycle_exit(pane));
    assert_eq!(
        clear_planner_bindings_for_lifecycle_exit(&plan_store, &pane_id, 4),
        0
    );
    assert!(pane.agent_type.is_none());
    assert!(pane.agent_session_id.is_none());
    assert!(pane.session_binding_nonce.is_none());
    assert!(pane.reported_session_binding_nonce.is_none());
    assert!(pane.agent_state.is_none());
    assert_eq!(pane.boot_stage, Some(BootStage::ShellReady));
    assert!(matches!(
        plan_store.get(&plan.id).unwrap().planner,
        Some(PlannerRef::Pane {
            session_id: None,
            ..
        })
    ));
    assert!(matches!(
        resolve_planner_ref(&planner, &owner, |_| planner_probe(pane)),
        PeerResolution::OrchestratorFallback { .. }
    ));
}
