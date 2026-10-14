use super::*;
use crate::lock_hold::TimedRwLock;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::request_continue;
use crate::service::boot_stage::BootStage;
use crate::service::turn_resume::{step, Step};
use std::sync::Arc;

#[test]
fn restored_owed_continue_waits_for_first_composer_then_claims_once() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("sqlite-lazy-fixture".into());
    tab.agent_session_id = Some("own-saved-session".into());
    tab.pty_id = Some("restored-pty".into());
    tab.session_binding_nonce = Some("current-nonce".into());
    tab.reported_session_binding_nonce = Some("current-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    request_continue(
        &mut tab.turn_resume,
        "own-saved-session",
        "previous-start",
        "opaque-continue".into(),
    );
    let mux = Arc::new(TimedRwLock::new("restore-composer-test", mux));

    for session_bound in [false, true] {
        assert_eq!(
            decide_launch_input(
                LaunchInputObservation {
                    session_bound,
                    composer_ready: false,
                    ..input_observation()
                },
                LaunchInputMode::LazySessionStart,
            ),
            Ok(None),
        );
        assert_eq!(step(&mux, &tab_id, "current-start"), Step::Waiting);
    }
    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                session_bound: true,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
    );
    mux.write().get_tab_mut(&tab_id).unwrap().boot_stage = Some(BootStage::AgentReady);
    let Step::Claimed(claim) = step(&mux, &tab_id, "current-start") else {
        panic!("composer-ready own session must release the owed original");
    };
    assert_eq!(claim.session_id, "own-saved-session");
    assert_eq!(claim.pty_id, "restored-pty");
    assert_eq!(claim.binding_nonce, "current-nonce");
    assert_eq!(claim.prompt.as_deref(), Some("opaque-continue"));
    assert_eq!(step(&mux, &tab_id, "current-start"), Step::Settled);
}
