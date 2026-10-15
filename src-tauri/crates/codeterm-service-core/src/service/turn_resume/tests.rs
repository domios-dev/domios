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
fn shutdown_freeze_keeps_the_working_mark_when_idle_edge_arrives() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Working);
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    assert!(record_turn_resume_edge_if_active(tab, FIRST, false, true));
    let marked = tab.turn_resume.clone();
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into(),
    );

    assert!(!record_turn_resume_edge_if_active(tab, FIRST, true, true));
    assert_eq!(tab.turn_resume, marked);
}

#[test]
fn disabled_resume_settles_owed_mark_without_claiming_it() {
    let (mut state, tab_id) = agent_tab(TimelineState::Working);
    assert!(set_state(&mut state, &tab_id, TimelineState::Working));
    let mux = shared(restart(&state));
    let owed = owed_tab_ids(&mux, SECOND);

    assert_eq!(settle_disabled_marks(&mux, &owed, SECOND), 1);
    assert_eq!(
        mark(&mux, &tab_id)
            .and_then(|mark| mark.settled)
            .map(|settlement| settlement.outcome),
        Some(TurnResumeOutcome::Skipped(DISABLED.into()))
    );
    assert_eq!(step(&mux, &tab_id, SECOND), Step::Settled);
}

#[test]
fn after_update_prompt_replaces_the_owed_general_agent_prompt_once() {
    let (mut state, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut state, &tab_id, TimelineState::Working));
    let mut restored = restart(&state);
    let tab = restored.get_tab_mut(&tab_id).unwrap();
    let sender = crate::multiplexer::types::TurnResumeSender {
        tab_id: "requester".into(),
        label: "Requester".into(),
        from_agent: Some(true),
        from_model: None,
    };
    crate::multiplexer::types::request_continue_for_restored_start(
        &mut tab.turn_resume,
        "session-a",
        SECOND,
        "opaque-after-update".into(),
        Some(sender.clone()),
    );
    resumed_ready(&mut restored, &tab_id, "session-a");
    let mux = shared(restored);

    assert_eq!(
        step(&mux, &tab_id, SECOND),
        Step::Claimed(Claim {
            session_id: "session-a".into(),
            prompt: Some("opaque-after-update".into()),
            sender: Some(sender),
            pty_id: "pty-2".into(),
            binding_nonce: "nonce-2".into(),
        })
    );
    assert_eq!(step(&mux, &tab_id, SECOND), Step::Settled);
}

#[test]
fn pending_after_update_is_armed_and_sent_once_instead_of_the_owed_prompt() {
    use std::cell::RefCell;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("post-update-message.txt");
    let sender = crate::multiplexer::types::TurnResumeSender {
        tab_id: "requester".into(),
        label: "Requester".into(),
        from_agent: Some(true),
        from_model: None,
    };
    let message = crate::service::post_update::PendingMessage {
        text: "opaque-after-update".into(),
        sender: Some(sender.clone()),
    };
    std::fs::write(&path, serde_json::to_vec(&message).unwrap()).unwrap();
    let pending = crate::service::post_update::pending_at(&path).expect("stashed request");

    let (mut state, tab_id) = agent_tab(TimelineState::Working);
    assert!(set_state(&mut state, &tab_id, TimelineState::Working));
    let mut restored = restart(&state);
    restored.register_alias(crate::service::general_agent::GENERAL_ALIAS, &tab_id);
    let mux = shared(restored);
    let events = RefCell::new(Vec::new());
    let armed = arm_after_update_message(
        &mux,
        SECOND,
        Some(&pending),
        || {
            assert_eq!(
                mark(&mux, &tab_id).and_then(|mark| mark.prompt),
                Some("opaque-after-update".into())
            );
            events.borrow_mut().push("arm_persist");
            true
        },
        || {
            events.borrow_mut().push("consume");
            true
        },
    )
    .expect("after-update request is durable")
    .expect("General Agent tab was armed");
    assert_eq!(armed, tab_id);
    {
        let mut mux = mux.write();
        resumed_ready(&mut mux, &tab_id, "session-a");
    }

    let wave = Wave::new(SECOND, owed_tab_ids(&mux, SECOND));
    let claim = match advance_consider(&mux, &tab_id, &wave, false) {
        Some(Step::Claimed(claim)) => claim,
        other => panic!("expected one claim, got {other:?}"),
    };
    let mut sends = Vec::new();
    deliver_claim(
        &claim,
        || {
            events.borrow_mut().push("claim_persist");
            true
        },
        || {
            events.borrow_mut().push("binding");
            true
        },
        |body, _claim| {
            events.borrow_mut().push("send");
            sends.push(body.to_string());
            Ok(())
        },
    )
    .expect("one claimed delivery");

    assert_eq!(
        *events.borrow(),
        ["arm_persist", "consume", "claim_persist", "binding", "send"]
    );
    assert_eq!(sends, ["opaque-after-update"]);
    assert!(advance_consider(&mux, &tab_id, &wave, false).is_none());
    assert_eq!(step(&mux, &tab_id, SECOND), Step::Settled);
}

#[test]
fn stale_previous_edge_records_the_current_idle_state_change() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Working);
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    assert!(record_turn_resume_edge_if_active(tab, FIRST, false, true));
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into(),
    );

    assert!(record_turn_resume_edge_if_active(tab, FIRST, false, true));
    assert!(tab.turn_resume.is_none());
}

#[test]
fn explicit_resume_requires_enabled_setting_and_the_callers_own_tab() {
    assert_eq!(request_enabled(false), Err(DISABLED));
    assert!(request_enabled(true).is_ok());
    assert_eq!(
        request_targets_caller("tab-a", "tab-b"),
        Err(NOT_CALLER_TAB)
    );
    assert!(request_targets_caller("tab-a", "tab-a").is_ok());
}

#[test]
fn delivery_persists_and_pins_the_binding_before_one_send() {
    use std::cell::RefCell;

    let events = RefCell::new(Vec::new());
    let mut sends = 0;
    let result = persist_then_send(
        || {
            events.borrow_mut().push("persist");
            true
        },
        || {
            events.borrow_mut().push("binding");
            true
        },
        || {
            events.borrow_mut().push("send");
            sends += 1;
            Ok(())
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(*events.borrow(), ["persist", "binding", "send"]);
    assert_eq!(sends, 1);
}

#[test]
fn delivery_refuses_without_send_when_claim_persistence_fails() {
    use std::cell::Cell;

    let sent = Cell::new(false);
    let result = persist_then_send(
        || false,
        || true,
        || {
            sent.set(true);
            Ok(())
        },
    );

    assert_eq!(result, Err(DeliveryFailure::ClaimNotPersisted));
    assert!(!sent.get());
}

#[test]
fn delivery_refuses_without_send_when_the_pinned_session_changes() {
    use std::cell::Cell;

    let sent = Cell::new(false);
    let result = persist_then_send(
        || true,
        || false,
        || {
            sent.set(true);
            Ok(())
        },
    );

    assert_eq!(result, Err(DeliveryFailure::SessionChanged));
    assert!(!sent.get());
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
        sender: None,
        pty_id: "pty-2".into(),
        binding_nonce: "nonce-2".into(),
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
fn consideration_drains_one_tab_after_its_single_resume_claim() {
    let (mut state, tab_id) = agent_tab(TimelineState::Working);
    assert!(set_state(&mut state, &tab_id, TimelineState::Working));
    let mut restored = restart(&state);
    resumed_ready(&mut restored, &tab_id, "session-a");
    let mux = shared(restored);
    let wave = Wave::new(SECOND, owed_tab_ids(&mux, SECOND));

    assert!(matches!(
        advance_consider(&mux, &tab_id, &wave, false),
        Some(Step::Claimed(Claim {
            session_id,
            pty_id,
            binding_nonce,
            ..
        })) if session_id == "session-a" && pty_id == "pty-2" && binding_nonce == "nonce-2"
    ));
    assert!(wave.pending_ids().is_empty());
    assert!(advance_consider(&mux, &tab_id, &wave, false).is_none());
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
            sender: None,
            pty_id: "pty-2".into(),
            binding_nonce: "nonce-2".into(),
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

#[test]
fn live_resume_keeps_owed_continue_past_deadline_then_claims_once() {
    let (mut mux, tab_id) = agent_tab(TimelineState::Idle);
    assert!(set_state(&mut mux, &tab_id, TimelineState::Working));
    let mut restored = restart(&mux);
    restored.get_tab_mut(&tab_id).unwrap().boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    let restored = shared(restored);
    let wave = Wave::new(SECOND, owed_tab_ids(&restored, SECOND));
    assert_eq!(
        settle_deadline(&restored, &tab_id, &wave, true),
        DeadlineOutcome::Retained
    );
    assert!(wave.is_pending(&tab_id));
    assert_eq!(step(&restored, &tab_id, SECOND), Step::Waiting);
    resumed_ready(&mut restored.write(), &tab_id, "session-a");
    assert!(matches!(
        advance_consider(&restored, &tab_id, &wave, false),
        Some(Step::Claimed(_))
    ));
    assert_eq!(advance_consider(&restored, &tab_id, &wave, false), None);
}
