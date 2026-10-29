use super::*;

const NOW: u64 = 10_000_000;

fn eligible() -> SleepFacts {
    SleepFacts {
        selected_in_any_client: false,
        general: false,
        protected: false,
        status: Some(TabStatus::Idle),
        active_turn: false,
        live_pty: false,
        last_pty_output_ms: None,
        last_turn: Some(LastTurnEvidence {
            session_id: "own-session".into(),
            completed: true,
        }),
        held_send: false,
        written_send: false,
        unconfirmed_send: false,
        parked_delivery: false,
        assigned_task: false,
        reporting_child: false,
        owned_background_terminal: false,
        bound_cron: false,
        pending_report: false,
        external_chat_subscriber: false,
        mesh_relay: false,
        last_activity_ms: Some(0),
        last_open_ms: Some(0),
        exact_resume_capability: true,
        persisted_session: Some(PersistedSession {
            session_id: "own-session".into(),
            restorable_launch: true,
            source: Some(PersistedSessionSource::Disk),
        }),
    }
}

fn decision(facts: &SleepFacts) -> SleepDecision {
    evaluate(&SleepPolicy::default(), facts, NOW)
}

#[test]
fn own_pty_stream_with_stale_idle_hook_never_sleeps() {
    let policy = SleepPolicy::default();
    let mut facts = eligible();
    facts.live_pty = true;
    for tick in 0..100 {
        let now = NOW + tick * 60_000;
        facts.last_pty_output_ms = Some(now);
        assert_eq!(
            evaluate(&policy, &facts, now),
            SleepDecision::Stay(StayReason::PtyActivity)
        );
    }
}

#[test]
fn active_turn_overrides_idle_hook_even_without_output() {
    let mut facts = eligible();
    facts.active_turn = true;
    facts.live_pty = true;
    facts.last_pty_output_ms = Some(0);
    assert_eq!(
        decision(&facts),
        SleepDecision::Stay(StayReason::ActiveTurn)
    );
}

#[test]
fn truly_idle_pty_may_sleep_only_with_old_output_evidence() {
    let mut facts = eligible();
    facts.live_pty = true;
    for output in [None, Some(NOW), Some(NOW + 1)] {
        facts.last_pty_output_ms = output;
        assert_eq!(
            decision(&facts),
            SleepDecision::Stay(StayReason::PtyActivity)
        );
    }
    let threshold = SleepPolicy::default().idle_minutes * 60_000;
    facts.last_pty_output_ms = Some(NOW - threshold);
    assert_eq!(
        decision(&facts),
        SleepDecision::Stay(StayReason::PtyActivity)
    );
    facts.last_pty_output_ms = Some(NOW - threshold - 1);
    assert_eq!(decision(&facts), SleepDecision::Sleep);
}

macro_rules! refuses_flag {
    ($name:ident, $field:ident, $value:expr, $reason:ident) => {
        #[test]
        fn $name() {
            let facts = SleepFacts {
                $field: $value,
                ..eligible()
            };
            assert_eq!(decision(&facts), SleepDecision::Stay(StayReason::$reason));
        }
    };
}

refuses_flag!(selected, selected_in_any_client, true, Selected);
refuses_flag!(general, general, true, General);
refuses_flag!(protected, protected, true, Protected);
refuses_flag!(held_send, held_send, true, HeldSend);
refuses_flag!(written_send, written_send, true, WrittenSend);
refuses_flag!(unconfirmed_send, unconfirmed_send, true, UnconfirmedSend);
refuses_flag!(parked_delivery, parked_delivery, true, ParkedDelivery);
refuses_flag!(assigned_task, assigned_task, true, AssignedTask);
refuses_flag!(reporting_child, reporting_child, true, ReportingChild);
refuses_flag!(
    background_terminal,
    owned_background_terminal,
    true,
    OwnedBackgroundTerminal
);
refuses_flag!(cron, bound_cron, true, BoundCron);
refuses_flag!(pending_report, pending_report, true, PendingReport);
refuses_flag!(
    subscriber,
    external_chat_subscriber,
    true,
    ExternalChatSubscriber
);
refuses_flag!(relay, mesh_relay, true, MeshRelay);
refuses_flag!(
    resume_unsupported,
    exact_resume_capability,
    false,
    ExactResumeUnsupported
);

#[test]
fn disabled() {
    let policy = SleepPolicy {
        enabled: false,
        ..SleepPolicy::default()
    };
    assert_eq!(
        evaluate(&policy, &eligible(), NOW),
        SleepDecision::Stay(StayReason::Disabled)
    );
}

#[test]
fn not_idle() {
    for status in [
        None,
        Some(TabStatus::Working),
        Some(TabStatus::Waiting),
        Some(TabStatus::Error),
        Some(TabStatus::Dead),
    ] {
        let facts = SleepFacts {
            status,
            ..eligible()
        };
        assert_eq!(decision(&facts), SleepDecision::Stay(StayReason::NotIdle));
    }
}

#[test]
fn last_turn_not_completed() {
    for last_turn in [
        None,
        Some(LastTurnEvidence {
            session_id: "own-session".into(),
            completed: false,
        }),
    ] {
        let facts = SleepFacts {
            last_turn,
            ..eligible()
        };
        assert_eq!(
            decision(&facts),
            SleepDecision::Stay(StayReason::LastTurnNotCompleted)
        );
    }
}

#[test]
fn foreign_turn_evidence() {
    let mut facts = eligible();
    facts.last_turn.as_mut().unwrap().session_id = "foreign-session".into();
    assert_eq!(
        decision(&facts),
        SleepDecision::Stay(StayReason::ForeignTurnEvidence)
    );
}

#[test]
fn missing_session() {
    let mut facts = eligible();
    facts.persisted_session = None;
    assert_eq!(
        decision(&facts),
        SleepDecision::Stay(StayReason::MissingSession)
    );
    for session_id in ["", "   "] {
        let mut facts = eligible();
        facts.persisted_session.as_mut().unwrap().session_id = session_id.into();
        assert_eq!(
            decision(&facts),
            SleepDecision::Stay(StayReason::MissingSession)
        );
    }
}

#[test]
fn missing_launch() {
    let mut facts = eligible();
    facts.persisted_session.as_mut().unwrap().restorable_launch = false;
    assert_eq!(
        decision(&facts),
        SleepDecision::Stay(StayReason::MissingRestorableLaunch)
    );
}

#[test]
fn missing_source() {
    let mut facts = eligible();
    facts.persisted_session.as_mut().unwrap().source = None;
    assert_eq!(
        decision(&facts),
        SleepDecision::Stay(StayReason::MissingSessionSource)
    );
}

macro_rules! refuses_clock {
    ($name:ident, $field:ident, $reason:ident) => {
        #[test]
        fn $name() {
            for clock in [
                Some(NOW + 1),
                Some(NOW),
                Some(NOW - 3_600_000),
                Some(NOW - 3_599_999),
            ] {
                let facts = SleepFacts {
                    $field: clock,
                    ..eligible()
                };
                assert_eq!(decision(&facts), SleepDecision::Stay(StayReason::$reason));
            }
            let facts = SleepFacts {
                $field: Some(NOW - 3_600_001),
                ..eligible()
            };
            assert_eq!(decision(&facts), SleepDecision::Sleep);
        }
    };
}

refuses_clock!(activity_clock, last_activity_ms, ActivityNotOldEnough);
refuses_clock!(open_clock, last_open_ms, OpenNotOldEnough);

#[test]
fn available_clocks_fall_back_and_missing_both_stays_awake() {
    for (activity, opened, expected) in [
        (Some(0), None, SleepDecision::Sleep),
        (None, Some(0), SleepDecision::Sleep),
        (
            Some(0),
            Some(NOW),
            SleepDecision::Stay(StayReason::OpenNotOldEnough),
        ),
        (
            Some(NOW),
            Some(0),
            SleepDecision::Stay(StayReason::ActivityNotOldEnough),
        ),
        (
            None,
            None,
            SleepDecision::Stay(StayReason::ActivityNotOldEnough),
        ),
    ] {
        let facts = SleepFacts {
            last_activity_ms: activity,
            last_open_ms: opened,
            ..eligible()
        };
        assert_eq!(decision(&facts), expected);
    }
}

#[test]
fn sleeps_with_exact_persisted_session_and_all_proofs() {
    for source in [
        PersistedSessionSource::Disk,
        PersistedSessionSource::RuntimeStore,
    ] {
        let mut facts = eligible();
        facts.persisted_session.as_mut().unwrap().source = Some(source);
        assert_eq!(decision(&facts), SleepDecision::Sleep);
    }
}

#[test]
fn configured_threshold_and_overflow() {
    let policy = SleepPolicy {
        enabled: true,
        idle_minutes: 180,
    };
    assert_eq!(
        evaluate(&policy, &eligible(), NOW),
        SleepDecision::Stay(StayReason::ActivityNotOldEnough)
    );
    let policy = SleepPolicy {
        enabled: true,
        idle_minutes: u64::MAX,
    };
    assert_eq!(
        evaluate(&policy, &eligible(), u64::MAX),
        SleepDecision::Stay(StayReason::ActivityNotOldEnough)
    );
}

#[test]
fn changed_snapshot_refuses_after_candidate_was_eligible() {
    let mut facts = eligible();
    assert_eq!(decision(&facts), SleepDecision::Sleep);
    facts.unconfirmed_send = true;
    assert_eq!(
        decision(&facts),
        SleepDecision::Stay(StayReason::UnconfirmedSend)
    );
}
