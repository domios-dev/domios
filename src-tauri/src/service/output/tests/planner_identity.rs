use super::diagnose_shell_prompt_without_agent_markers;
use crate::agent::offline_state_classifier::{PaneVerdict, TimelineState};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::plan::reconcile::PaneProbe;
use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::boot_stage::BootStage;
use crate::service::pane::clear_agent_identity_for_lifecycle_exit;

const LIVE_TUI_SHELL_LOOKING_FRAME: &str =
    "  GPT-5.6 Codex\n  Working in D:\\Developer\\codeterm\n\n› continue\nPS D:\\Developer\\codeterm>";

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
    assert!(pane.agent_state.is_some());
    assert!(pane.pending_prompt.is_some());
}

#[test]
fn live_planner_shell_frame_preserves_identity_until_explicit_lifecycle_exit() {
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
    pane.pending_prompt = Some(PendingPrompt {
        question: "Continue?".into(),
        options: vec![PromptOption {
            label: "Yes".into(),
            value: "yes".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    });

    let planner = Some(PlannerRef::Pane {
        pane_id: pane_id.clone(),
        session_id: Some("session-pane-owned".into()),
        title: Some("Planner".into()),
    });
    let owner = OrchestratorRef::from_pane("orchestrator");

    assert!(diagnose_shell_prompt_without_agent_markers(
        LIVE_TUI_SHELL_LOOKING_FRAME,
        false,
    ));
    let pane = mux.get_pane(&pane_id).unwrap();
    assert_live_planner_identity(pane);
    assert!(matches!(
        resolve_planner_ref(&planner, &owner, |_| planner_probe(pane)),
        PeerResolution::Planner { pane_id: ref resolved } if resolved == &pane_id
    ));

    let pane = mux.get_pane_mut(&pane_id).unwrap();
    assert!(
        clear_agent_identity_for_lifecycle_exit(pane),
        "the explicit lifecycle owner performs the destructive transition"
    );
    assert!(
        !clear_agent_identity_for_lifecycle_exit(pane),
        "replayed lifecycle exit is idempotent"
    );
    assert!(pane.agent_type.is_none());
    assert!(pane.agent_session_id.is_none());
    assert!(pane.session_binding_nonce.is_none());
    assert!(pane.reported_session_binding_nonce.is_none());
    assert!(pane.agent_state.is_none());
    assert!(pane.pending_prompt.is_none());
    assert_eq!(pane.boot_stage, Some(BootStage::ShellReady));
    assert!(matches!(
        resolve_planner_ref(&planner, &owner, |_| planner_probe(pane)),
        PeerResolution::OrchestratorFallback { .. }
    ));
}
