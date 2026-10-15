use super::*;

const INTERVAL: u64 = 1800;

fn floor_rows(f: &Fixture) -> usize {
    f.decisions()
        .iter()
        .filter(|row| row["decided_by"] == "cadence")
        .count()
}

#[test]
fn plan_reminder_floor_wakes_owner_without_supporter_once_per_interval() {
    let f = fixture(ReminderKind::Owner, approve());
    f.host.cfg.lock().adapter = None;
    for (now, outcome) in [
        (0, ReminderOutcome::Suppressed("no_adapter")),
        (INTERVAL - 1, ReminderOutcome::Suppressed("no_adapter")),
        (INTERVAL, ReminderOutcome::Accepted),
        (INTERVAL + 60, ReminderOutcome::Suppressed("no_adapter")),
        (2 * INTERVAL, ReminderOutcome::Accepted),
    ] {
        assert_eq!(f.admit(ReminderKind::Owner, now), outcome, "{now}");
    }
    assert_eq!(f.emits(), 2);
    assert_eq!(f.model.calls(), 0);
    assert_eq!(floor_rows(&f), 2);
}

#[test]
fn plan_reminder_floor_overrides_supporter_errors_and_invalid_answers() {
    let replies = [
        Reply::Choice("other", 0.99),
        Reply::Choice("action_needed", 0.79),
        Reply::Undeclared,
        Reply::Error(DecisionError::Deadline),
        Reply::Error(DecisionError::Unavailable("opaque".into())),
        Reply::Error(DecisionError::Http(402)),
    ];
    for reply in replies {
        let f = fixture(ReminderKind::Owner, reply.clone());
        assert!(matches!(
            f.admit(ReminderKind::Owner, 0),
            ReminderOutcome::Suppressed(_)
        ));
        assert_eq!(
            f.admit(ReminderKind::Owner, INTERVAL),
            ReminderOutcome::Accepted,
            "{reply:?}"
        );
        assert_eq!((f.model.calls(), f.emits()), (1, 1), "{reply:?}");
    }
}

#[test]
fn plan_reminder_floor_wakes_unchanged_owner_despite_supporter_denial() {
    for denial in ["nothing_changed", "status_only"] {
        let f = fixture(ReminderKind::Owner, Reply::Choice(denial, 0.99));
        f.host.owner_view.lock().hard = every_hard_wake();
        for minute in 0..=120 {
            f.admit(ReminderKind::Owner, minute * 60);
        }
        assert_eq!((f.model.calls(), f.emits()), (1, 4), "{denial}");
        assert_eq!(floor_rows(&f), 4, "{denial}");
    }
}

#[test]
fn plan_reminder_floor_restarts_after_an_approved_delivery() {
    let f = fixture(ReminderKind::Owner, approve());
    assert_eq!(f.admit(ReminderKind::Owner, 0), ReminderOutcome::Accepted);
    f.host
        .owner_view
        .lock()
        .hard
        .push(HardWake::FailedGate("early-gate".into()));
    assert_eq!(f.admit(ReminderKind::Owner, 600), ReminderOutcome::Accepted);
    assert_ne!(
        f.admit(ReminderKind::Owner, INTERVAL),
        ReminderOutcome::Accepted
    );
    assert_eq!(
        f.admit(ReminderKind::Owner, 600 + INTERVAL),
        ReminderOutcome::Accepted
    );
    assert_eq!(floor_rows(&f), 1);
}

#[test]
fn plan_reminder_floor_failure_waits_for_the_next_interval() {
    let f = fixture(ReminderKind::Owner, approve());
    f.host.cfg.lock().adapter = None;
    f.admit(ReminderKind::Owner, 0);
    *f.host.emit_error.lock() = Some("opaque transport failure".into());
    assert_eq!(
        f.admit(ReminderKind::Owner, INTERVAL),
        ReminderOutcome::Failed("opaque transport failure".into())
    );
    *f.host.emit_error.lock() = None;
    assert_eq!(
        f.admit(ReminderKind::Owner, INTERVAL + 60),
        ReminderOutcome::Suppressed("no_adapter")
    );
    assert_eq!(
        f.admit(ReminderKind::Owner, 2 * INTERVAL),
        ReminderOutcome::Accepted
    );
    assert_eq!(f.emits(), 2, "one failed and one accepted emitter call");
}

#[test]
fn plan_reminder_floor_stays_silent_when_only_blocked_work_remains() {
    let f = fixture(ReminderKind::Owner, approve());
    f.host.cfg.lock().adapter = None;
    f.edit_plan(|plan| {
        plan.tasks[0].status = TaskStatus::Blocked;
        plan.tasks[0].pause_reason = Some("owner decision".into());
    });
    for now in [0, INTERVAL, 10 * INTERVAL] {
        assert_eq!(
            f.admit(ReminderKind::Owner, now),
            ReminderOutcome::Suppressed("no_adapter")
        );
    }
    assert_eq!(f.emits(), 0);
}

#[test]
fn plan_reminder_floor_never_wakes_other_kinds_without_supporter() {
    for kind in SUPPORTER_ONLY {
        let f = fixture(kind, approve());
        f.host.cfg.lock().adapter = None;
        for now in [0, INTERVAL, 10 * INTERVAL] {
            assert_eq!(
                f.admit(kind, now),
                ReminderOutcome::Suppressed("no_adapter"),
                "{kind:?}"
            );
        }
        assert_eq!(f.emits(), 0, "{kind:?}");
    }
}
