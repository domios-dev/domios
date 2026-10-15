//! WHAT: the one bounded Enter an unbound first-turn launch gets before it fails.
//! WHY: a restored Codex draft parked in the composer used to fail the launch
//! with SessionBindingNotObserved although one Enter would have bound it.
//! ORACLE: the due verdict, the ack wait it opens, and the superseded refusal.
use super::super::bootstrap::{nudge_unconfirmed_draft, unconfirmed_draft_nudge_due};
use crate::service::boot_stage::BootStage;
use crate::service::launch_readiness::{LaunchAttempt, LaunchSettlement};
use std::cell::Cell;

fn provisional() -> LaunchSettlement {
    LaunchSettlement::Provisional {
        stage: BootStage::LaunchDeadlineElapsed,
        reason: "launch readiness deadline elapsed".into(),
    }
}

#[test]
fn draft_nudge_is_due_only_for_an_unconfirmed_unbound_first_turn_launch() {
    assert!(unconfirmed_draft_nudge_due(
        false,
        true,
        &provisional(),
        false
    ));
    assert!(!unconfirmed_draft_nudge_due(
        true,
        true,
        &provisional(),
        false
    ));
    assert!(!unconfirmed_draft_nudge_due(
        false,
        false,
        &provisional(),
        false
    ));
    assert!(!unconfirmed_draft_nudge_due(
        false,
        true,
        &provisional(),
        true
    ));
    assert!(!unconfirmed_draft_nudge_due(
        false,
        true,
        &LaunchSettlement::Ready,
        false
    ));
    assert!(!unconfirmed_draft_nudge_due(
        false,
        true,
        &LaunchSettlement::Superseded,
        false
    ));
}

#[test]
fn draft_nudge_presses_enter_once_and_waits_for_the_binding() {
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;

    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("first-turn").with_session_starts_on_first_user_turn(),
    ));
    let _pty = fixture.bind_test_pty_session(&tab_id, "restored-session", "launch-nonce");
    let pty_id = {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("first-turn".into());
        tab.reported_session_binding_nonce = None;
        tab.pty_id.clone().unwrap()
    };
    let draft = "resume the interrupted work";
    fixture
        .service
        .screen_state
        .lock()
        .process(&pty_id, format!("› {draft}").as_bytes());
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: pty_id.clone(),
        binding_nonce: "launch-nonce".into(),
    };

    let superseded = LaunchAttempt {
        binding_nonce: "older-nonce".into(),
        ..attempt.clone()
    };
    let waited = Cell::new(0);
    assert!(!nudge_unconfirmed_draft(
        &fixture.service,
        &superseded,
        draft,
        |_| {
            waited.set(waited.get() + 1);
            true
        }
    ));
    assert_eq!(waited.get(), 0, "a superseded launch never presses Enter");

    assert!(nudge_unconfirmed_draft(
        &fixture.service,
        &attempt,
        draft,
        |wait| {
            assert!(!wait.is_zero());
            waited.set(waited.get() + 1);
            true
        }
    ));
    assert_eq!(waited.get(), 1);

    assert!(!nudge_unconfirmed_draft(
        &fixture.service,
        &attempt,
        "",
        |_| {
            waited.set(waited.get() + 1);
            true
        }
    ));
    assert_eq!(waited.get(), 1, "no draft means no Enter and no wait");
}
