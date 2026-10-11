use super::*;

#[test]
fn plan_reminder_choice_churning_ticks_only_call_and_journal_at_cadence() {
    let f = fixture(ReminderKind::Owner, Reply::Choice("nothing_changed", 0.99));
    f.host.owner_view.lock().hard.clear();
    for minute in 0..60 {
        {
            let mut view = f.host.owner_view.lock();
            view.projection
                .insert("stride:progress".into(), minute.to_string());
            view.delta
                .insert("stride:progress".into(), minute.to_string());
            view.workers[0].activity = if minute % 2 == 0 { "working" } else { "idle" }.into();
        }
        f.admit(ReminderKind::Owner, minute * 60);
    }
    assert_eq!(f.model.calls(), 2);
    let decisions = f.decisions();
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions[0]["time"], 0);
    assert_eq!(decisions[1]["time"], 1800);
    assert_eq!(f.emits(), 0);
}

#[test]
fn plan_reminder_choice_new_event_calls_once_inside_cadence() {
    let f = fixture(ReminderKind::Owner, Reply::Choice("nothing_changed", 0.99));
    f.admit(ReminderKind::Owner, 0);
    f.host
        .owner_view
        .lock()
        .hard
        .push(HardWake::NewReport("pushed-report".into()));
    f.admit(ReminderKind::Owner, 60);
    for minute in 2..10 {
        f.host
            .owner_view
            .lock()
            .projection
            .insert("stride:progress".into(), minute.to_string());
        f.admit(ReminderKind::Owner, minute * 60);
    }
    assert_eq!(f.model.calls(), 2);
    assert_eq!(f.decisions().len(), 2);
    assert_eq!(f.decisions()[1]["time"], 60);
}

#[test]
fn plan_reminder_choice_cached_tick_adds_no_journal_reflex() {
    let f = fixture(ReminderKind::Owner, approve());
    assert_eq!(f.admit(ReminderKind::Owner, 0), ReminderOutcome::Accepted);
    let journal = f.journal();
    for now in [60, 1800, 3600] {
        assert_eq!(
            f.admit(ReminderKind::Owner, now),
            ReminderOutcome::Suppressed("same_snapshot")
        );
    }
    assert_eq!(f.journal(), journal);
    assert_eq!(f.model.calls(), 1);
}
