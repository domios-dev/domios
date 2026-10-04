use std::sync::Arc;

use codeterm_types::pty::{BootStage, TabVerdict, TimelineState};

use super::*;
use crate::lock_hold::TimedRwLock;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{request_continue, TurnResumeSettlement};

const FIRST: &str = "start-1";
const SECOND: &str = "start-2";
const THIRD: &str = "start-3";

fn agent_tab(state: TimelineState) -> (MultiplexerState, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("fixture tab");
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("session-a".into());
    tab.session_binding_nonce = Some("nonce-1".into());
    tab.reported_session_binding_nonce = Some("nonce-1".into());
    tab.pty_id = Some("pty-1".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_state = Some(
        TabVerdict {
            state,
            subtype: None,
        }
        .into(),
    );
    (mux, tab_id)
}

fn set_state(mux: &mut MultiplexerState, tab_id: &str, state: TimelineState) -> bool {
    let tab = mux.get_tab_mut(tab_id).expect("tab");
    tab.agent_state = Some(
        TabVerdict {
            state,
            subtype: None,
        }
        .into(),
    );
    record_turn_edge(tab, FIRST)
}

/// The daemon stops: only the persisted snapshot crosses into the next start.
fn restart(mux: &MultiplexerState) -> MultiplexerState {
    MultiplexerState::from_session_snapshot(&mux.to_session_snapshot())
}

/// The restored tab relaunched and its agent proved the resumed session.
fn resumed_ready(mux: &mut MultiplexerState, tab_id: &str, session_id: &str) {
    let tab = mux.get_tab_mut(tab_id).expect("restored tab");
    tab.pty_id = Some("pty-2".into());
    tab.agent_session_id = Some(session_id.into());
    tab.session_binding_nonce = Some("nonce-2".into());
    tab.reported_session_binding_nonce = Some("nonce-2".into());
    tab.boot_stage = Some(BootStage::AgentReady);
}

fn shared(mux: MultiplexerState) -> MuxState {
    Arc::new(TimedRwLock::new("turn-resume-test", mux))
}

fn mark(mux: &MuxState, tab_id: &str) -> Option<crate::multiplexer::types::TurnResumeMark> {
    mux.read()
        .get_tab(tab_id)
        .and_then(|tab| tab.turn_resume.clone())
}

#[test]
fn working_state_persists_through_the_session_snapshot() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));

    let restored = restart(&mux);
    let mark = restored
        .get_tab(&tab_id)
        .and_then(|tab| tab.turn_resume.clone())
        .expect("working mark survives a stop");

    assert_eq!(mark.session_id, "session-a");
    assert_eq!(mark.start_epoch, FIRST);
    assert!(mark.owed_in(SECOND));
}

#[test]
fn idle_tab_owes_nothing_after_restart() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(!set_state(&mut mux, &tab_id, TimelineState::Idle));
    assert!(!set_state(&mut mux, &tab_id, TimelineState::Question));

    let mut restored = restart(&mux);
    resumed_ready(&mut restored, &tab_id, "session-a");
    let restored = shared(restored);

    assert!(owed_tab_ids(&restored, SECOND).is_empty());
    assert_eq!(step(&restored, &tab_id, SECOND), Step::Settled);
    assert!(mark(&restored, &tab_id).is_none());
}

#[test]
fn turn_that_ended_before_the_stop_owes_nothing() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));
    assert!(set_state(&mut mux, &tab_id, TimelineState::Idle));

    let restored = shared(restart(&mux));

    assert!(owed_tab_ids(&restored, SECOND).is_empty());
}

#[test]
fn continue_is_claimed_once_across_two_starts() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));

    let mut second = restart(&mux);
    let tab = second.get_tab_mut(&tab_id).expect("restored tab");
    tab.boot_stage = Some(BootStage::PtyStarting);
    let second = shared(second);
    assert_eq!(
        owed_tab_ids(&second, SECOND)
            .into_iter()
            .collect::<Vec<_>>(),
        vec![tab_id.clone()]
    );
    assert_eq!(step(&second, &tab_id, SECOND), Step::Waiting);

    resumed_ready(&mut second.write(), &tab_id, "session-a");
    let claim = Claim {
        session_id: "session-a".into(),
        prompt: None,
    };
    assert_eq!(step(&second, &tab_id, SECOND), Step::Claimed(claim));
    assert_eq!(step(&second, &tab_id, SECOND), Step::Settled);
    assert_eq!(
        mark(&second, &tab_id).and_then(|mark| mark.settled),
        Some(TurnResumeSettlement {
            start_epoch: SECOND.into(),
            outcome: TurnResumeOutcome::Sent,
        })
    );

    let mut third = restart(&second.read());
    resumed_ready(&mut third, &tab_id, "session-a");
    let third = shared(third);
    assert!(owed_tab_ids(&third, THIRD).is_empty());
    assert_eq!(step(&third, &tab_id, THIRD), Step::Settled);
}

#[test]
fn owed_mark_survives_restored_turn_edges_until_settled() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));
    let mut restored = restart(&mux);

    let tab = restored.get_tab_mut(&tab_id).expect("restored tab");
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into(),
    );
    assert!(!record_turn_edge(tab, SECOND));

    let restored = shared(restart(&restored));
    assert!(owed_tab_ids(&restored, SECOND).contains(&tab_id));
}

#[test]
fn resumed_on_a_different_session_is_skipped_and_recorded() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));
    let mut restored = restart(&mux);
    resumed_ready(&mut restored, &tab_id, "session-fresh");
    let restored = shared(restored);

    assert_eq!(
        step(&restored, &tab_id, SECOND),
        Step::Skipped(SESSION_NOT_RESUMED)
    );
    assert_eq!(step(&restored, &tab_id, SECOND), Step::Settled);
    assert_eq!(
        mark(&restored, &tab_id)
            .and_then(|mark| mark.settled)
            .map(|s| s.outcome),
        Some(TurnResumeOutcome::Skipped(SESSION_NOT_RESUMED.into()))
    );
}

#[test]
fn tab_restored_as_a_shell_expires_with_its_reason() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));
    let mut restored = restart(&mux);
    let tab = restored.get_tab_mut(&tab_id).expect("restored tab");
    tab.agent_type = None;
    tab.agent_session_id = None;
    tab.boot_stage = Some(BootStage::ShellReady);
    let restored = shared(restored);

    assert_eq!(step(&restored, &tab_id, SECOND), Step::Waiting);
    assert_eq!(expire(&restored, &tab_id, SECOND), Some(NO_AGENT_SESSION));
    assert_eq!(expire(&restored, &tab_id, SECOND), None);
    assert!(owed_tab_ids(&restored, SECOND).is_empty());
}

#[test]
fn unauthenticated_resume_waits_then_expires_unverified() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));
    let mut restored = restart(&mux);
    resumed_ready(&mut restored, &tab_id, "session-a");
    restored
        .get_tab_mut(&tab_id)
        .expect("restored tab")
        .reported_session_binding_nonce = None;
    let restored = shared(restored);

    assert_eq!(step(&restored, &tab_id, SECOND), Step::Waiting);
    assert_eq!(expire(&restored, &tab_id, SECOND), Some(SESSION_UNVERIFIED));
}

#[test]
fn chat_backend_tab_never_receives_a_pty_continue() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "opaque-plugin".into(),
        session_id: "session-a".into(),
    };
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    assert!(!record_turn_edge(tab, FIRST));

    request_continue(&mut tab.turn_resume, "session-a", FIRST, "opaque".into());
    let mux = shared(mux);
    assert_eq!(
        step(&mux, &tab_id, SECOND),
        Step::Skipped(NO_PTY_AGENT_SESSION)
    );
}

#[test]
fn caller_request_and_working_mark_yield_one_continue() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));
    let tab = mux.get_tab_mut(&tab_id).expect("tab");
    request_continue(
        &mut tab.turn_resume,
        "session-a",
        FIRST,
        "opaque-request".into(),
    );
    assert!(!set_state(&mut mux, &tab_id, TimelineState::Idle));

    let mut restored = restart(&mux);
    resumed_ready(&mut restored, &tab_id, "session-a");
    let restored = shared(restored);

    let claims: Vec<Step> = (0..3)
        .map(|_| step(&restored, &tab_id, SECOND))
        .filter(|outcome| matches!(outcome, Step::Claimed(_)))
        .collect();
    assert_eq!(
        claims,
        vec![Step::Claimed(Claim {
            session_id: "session-a".into(),
            prompt: Some("opaque-request".into()),
        })]
    );
}

#[test]
fn wave_reports_unsettled_tabs_at_its_failure_deadline() {
    let wave = Wave::new(SECOND, ["tab-a".to_string(), "tab-b".to_string()].into());

    wave.finish("tab-a");
    assert!(!wave.is_pending("tab-a"));
    assert_eq!(wave.wait_drained(Instant::now()), vec!["tab-b".to_string()]);

    wave.finish("tab-b");
    assert!(wave
        .wait_drained(Instant::now() + READY_DEADLINE)
        .is_empty());
}
