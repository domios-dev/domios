use super::*;

#[test]
fn plan_reminder_choice_blocked_skips_worker_and_notifies_owner_once() {
    let f = fixture(ReminderKind::Worker, Reply::Choice("blocked", 0.99));
    assert_eq!(
        f.admit(ReminderKind::Worker, 100),
        ReminderOutcome::Suppressed("recipient_blocked")
    );
    assert_eq!((f.model.calls(), f.emits()), (1, 0));
    let notices = f.host.notices.lock().clone();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].owner_tab, f.owner);
    assert!(notices[0]
        .notice
        .event_key
        .starts_with("plan-blocker:plan-a:"));
    assert!(notices[0].notice.pause_plan);
    assert_eq!(
        notices[0]
            .notice
            .actions
            .iter()
            .map(|action| action.label())
            .collect::<Vec<_>>(),
        vec!["wait", "pause plan", "decide"]
    );

    f.edit_plan(|plan| {
        plan.tasks
            .push(task("unrelated", TaskStatus::InProgress, None));
    });
    assert_eq!(
        f.admit(ReminderKind::Worker, 101),
        ReminderOutcome::Suppressed("same_snapshot")
    );
    assert_eq!((f.model.calls(), f.emits()), (1, 0));
    assert_eq!(f.host.notices.lock().len(), 1);
}

#[test]
fn plan_reminder_choice_action_needed_cannot_wake_a_blocked_worker() {
    let f = fixture(ReminderKind::Worker, approve());
    f.edit_plan(|plan| {
        plan.tasks[0].status = TaskStatus::Blocked;
        plan.tasks[0].pause_reason = Some("owner decision".into());
    });

    assert_eq!(
        f.admit(ReminderKind::Worker, 100),
        ReminderOutcome::Suppressed("recipient_not_actionable")
    );
    assert_eq!((f.model.calls(), f.emits()), (1, 0));
}

#[test]
fn plan_reminder_choice_blocker_notice_key_stays_stable_across_plan_changes() {
    let f = fixture(ReminderKind::Owner, Reply::Choice("blocked", 0.99));
    f.edit_plan(|plan| {
        plan.tasks[0].status = TaskStatus::Blocked;
        plan.tasks[0].pause_reason = Some("external blocker".into());
    });
    assert_eq!(
        f.admit(ReminderKind::Owner, 100),
        ReminderOutcome::Suppressed("recipient_blocked")
    );
    let first_key = f.host.notices.lock()[0].notice.event_key.clone();
    f.host
        .owner_view
        .lock()
        .delta
        .insert("stride:unrelated".into(), "changed".into());
    assert_eq!(
        f.admit(ReminderKind::Owner, 101),
        ReminderOutcome::Suppressed("recipient_blocked")
    );
    let notices = f.host.notices.lock();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].notice.event_key, first_key);
    assert_eq!((f.model.calls(), f.emits()), (2, 0));
}

#[test]
fn plan_reminder_choice_backoff_ignores_other_tasks_and_resets_for_own_report() {
    let f = fixture(ReminderKind::Worker, approve());
    assert_eq!(
        f.admit(ReminderKind::Worker, 100),
        ReminderOutcome::Accepted
    );
    f.edit_plan(|plan| {
        let mut unrelated = task("unrelated", TaskStatus::InProgress, None);
        unrelated.status_label = Some("unrelated change".into());
        plan.tasks.push(unrelated);
    });
    assert_eq!(
        f.admit(ReminderKind::Worker, 101),
        ReminderOutcome::Suppressed("same_snapshot")
    );
    assert_eq!((f.model.calls(), f.emits()), (1, 1));

    f.edit_plan(|plan| {
        let mut report = crate::plan::TaskExecutionReportSnapshot::default();
        report.id = "report-own".into();
        plan.tasks[0].execution_attempts[0].report = Some(report);
    });
    assert_eq!(
        f.admit(ReminderKind::Worker, 102),
        ReminderOutcome::Accepted
    );
    assert_eq!((f.model.calls(), f.emits()), (2, 2));
}
