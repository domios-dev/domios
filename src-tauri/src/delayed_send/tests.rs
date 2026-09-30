use super::*;

fn queue() -> (tempfile::TempDir, DelayedSendQueue) {
    let dir = tempfile::tempdir().unwrap();
    let queue = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
    (dir, queue)
}

#[test]
fn reminder_envelope_carries_creator_metadata() {
    let msg = DelayedMessage {
        id: "dmsg-7".into(),
        tab_id: "target-tab".into(),
        workspace_id: None,
        text: "hi".into(),
        scheduled_at_ms: 2000,
        created_at_ms: 1000,
        state: DelayedMessageState::Sending,
        error: None,
        session_id_at_create: None,
        created_by_tab: Some("e8a0ac20-aaaa".into()),
        created_by_label: Some("Main".into()),
        created_by_model: Some("opus".into()),
        recurrence: None,
        series_start_at_ms: None,
        tick_index: 0,
        skipped_ticks: 0,
        team_id: None,
        last_tick_unconfirmed: false,
        last_tick_send_sequence: 0,
    };
    let envelope = reminder_envelope(&msg);
    assert_eq!(
            format!(
                "{}{}",
                crate::service::messenger::Messenger::pty_prefix(&envelope),
                msg.text
            ),
            "-=-codeterm:reminder from=Main tab=e8a0ac20 model=opus set_at=1000 fire_at=2000 id=dmsg-7-=-hi"
        );
}

#[test]
fn persists_and_restores_messages() {
    let (dir, queue) = queue();
    let msg = queue
        .enqueue(
            "tab-a".into(),
            Some("ws".into()),
            Some("sid-1".into()),
            "hello".into(),
            Some(100),
            ReminderCreator::default(),
        )
        .unwrap();
    drop(queue);

    let reopened = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
    let messages = reopened.list(Some("tab-a"));
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, msg.id);
    assert_eq!(messages[0].text, "hello");
    assert_eq!(messages[0].state, DelayedMessageState::Queued);
}

#[test]
fn recurring_series_survives_queue_reload() {
    let (dir, queue) = queue();
    let msg = queue
        .enqueue_with_recurrence_at_sequence(
            "tab-a".into(),
            Some("ws".into()),
            Some("sid-1".into()),
            "tick".into(),
            Some(100_000),
            ReminderCreator::default(),
            Some(RecurrenceRule {
                every_ms: 60_000,
                until_ms: None,
                count: Some(3),
            }),
            Some("team-a".into()),
            4,
        )
        .unwrap();
    queue
        .finish_tick_at_sequence(&msg.id, 100_000, None, true, Some(7))
        .unwrap();
    drop(queue);

    let reopened = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
    let restored = reopened.list(Some("tab-a")).pop().unwrap();
    assert_eq!(restored.recurrence.unwrap().every_ms, 60_000);
    assert_eq!(restored.series_start_at_ms, Some(100_000));
    assert_eq!(restored.scheduled_at_ms, 160_000);
    assert_eq!(restored.tick_index, 1);
    assert_eq!(restored.skipped_ticks, 1);
    assert_eq!(restored.team_id.as_deref(), Some("team-a"));
    assert_eq!(restored.last_tick_send_sequence, 0);
}

#[test]
fn recurring_tick_is_skipped_while_previous_is_pending() {
    let (_dir, queue) = queue();
    let msg = queue
        .enqueue_with_recurrence(
            "tab".into(),
            None,
            None,
            "tick".into(),
            Some(100_000),
            ReminderCreator::default(),
            Some(RecurrenceRule {
                every_ms: 60_000,
                until_ms: None,
                count: Some(3),
            }),
            None,
        )
        .unwrap();
    queue.claim_due(100_000).unwrap();
    queue.finish_tick(&msg.id, 100_000, None, true).unwrap();
    let next = queue.list(Some("tab")).pop().unwrap();
    assert_eq!(next.state, DelayedMessageState::Queued);
    assert_eq!(next.scheduled_at_ms, 160_000);
    assert_eq!(next.skipped_ticks, 1);
    assert!(queue.claim_due(159_999).unwrap().is_empty());
}

#[test]
fn old_failed_send_does_not_suppress_recurring_ticks() {
    let ledger = crate::service::chat_send_transaction::ChatSendLedger::default();
    let old = ledger.try_start("tab", "old failure", None).unwrap();
    let old = ledger
        .finish(&old.send_id, Err("old failed send".into()))
        .unwrap();
    assert_eq!(
        old.state,
        crate::service::chat_send_transaction::ChatSendState::Failed
    );
    assert!(!has_unprocessed_send_after_tick(
        &ledger.reconcile("tab"),
        old.sequence
    ));

    let current = ledger.try_start("tab", "current failure", None).unwrap();
    let current = ledger
        .finish(&current.send_id, Err("current failed send".into()))
        .unwrap();
    let history = ledger.reconcile("tab");
    assert!(has_unprocessed_send_after_tick(&history, old.sequence));
    assert!(!has_unprocessed_send_after_tick(&history, current.sequence));
}

#[test]
fn invalid_recurrence_edit_is_atomic_and_schedule_edits_recheck_until() {
    let (_dir, queue) = queue();
    let rule = RecurrenceRule {
        every_ms: 60_000,
        until_ms: Some(160_000),
        count: None,
    };
    let msg = queue
        .enqueue_with_recurrence(
            "tab".into(),
            None,
            None,
            "original".into(),
            Some(100_000),
            ReminderCreator::default(),
            Some(rule),
            None,
        )
        .unwrap();

    assert!(queue
        .update_with_recurrence(&msg.id, Some("mutated".into()), Some(200_000), None, None)
        .is_err());
    let unchanged = queue.list(Some("tab")).pop().unwrap();
    assert_eq!(unchanged.text, "original");
    assert_eq!(unchanged.scheduled_at_ms, 100_000);
    assert_eq!(
        unchanged.recurrence.as_ref().unwrap().until_ms,
        Some(160_000)
    );
    assert!(queue
        .update_with_recurrence(&msg.id, None, Some(170_000), None, None)
        .is_err());
}

#[test]
fn missing_target_stops_series_and_records_reason() {
    let (_dir, queue) = queue();
    let msg = queue
        .enqueue_with_recurrence(
            "closed-tab".into(),
            None,
            None,
            "tick".into(),
            Some(100_000),
            ReminderCreator::default(),
            Some(RecurrenceRule {
                every_ms: 60_000,
                until_ms: None,
                count: None,
            }),
            None,
        )
        .unwrap();
    queue.claim_due(100_000).unwrap();

    let stopped = queue
        .stop_recurring_for_missing_target(&msg.id)
        .unwrap()
        .unwrap();
    assert_eq!(stopped.state, DelayedMessageState::Failed);
    assert_eq!(
        stopped.error.as_deref(),
        Some("target tab no longer exists; recurring series stopped")
    );
    assert!(stopped.recurrence.is_some());
    assert!(queue.claim_due(u64::MAX).unwrap().is_empty());
}

#[test]
fn agent_ready_event_releases_delayed_delivery_waiter() {
    let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let waiter_ready = ready.clone();
    let waiter = std::thread::spawn(move || {
        crate::service::boot_wait::wait_until(
            std::time::Instant::now() + std::time::Duration::from_secs(2),
            || waiter_ready.load(std::sync::atomic::Ordering::Acquire),
        )
    });
    ready.store(true, std::sync::atomic::Ordering::Release);
    crate::service::boot_wait::notify_boot_changed();
    assert!(waiter.join().unwrap());
}

#[test]
fn delayed_failure_kept_not_deleted() {
    let (dir, queue) = queue();
    let msg = queue
        .enqueue(
            "tab-a".into(),
            None,
            None,
            "persist failure".into(),
            Some(1),
            ReminderCreator::default(),
        )
        .unwrap();
    queue.claim_due(u64::MAX).unwrap();
    queue
        .mark_failed(&msg.id, "AgentReady deadline elapsed".into())
        .unwrap();
    let reopened = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
    let failed = reopened.list(None).pop().unwrap();
    assert_eq!(failed.state, DelayedMessageState::Failed);
    assert_eq!(failed.error.as_deref(), Some("AgentReady deadline elapsed"));
}

#[test]
fn failed_recurring_tick_keeps_future_ticks() {
    let (_dir, queue) = queue();
    let msg = queue
        .enqueue_with_recurrence(
            "tab-a".into(),
            None,
            None,
            "series payload".into(),
            Some(100_000),
            ReminderCreator::default(),
            Some(RecurrenceRule {
                every_ms: 60_000,
                until_ms: None,
                count: Some(3),
            }),
            None,
        )
        .unwrap();
    queue.claim_due(100_000).unwrap();
    queue
        .finish_tick(
            &msg.id,
            100_000,
            Some("AgentReady deadline elapsed".into()),
            false,
        )
        .unwrap();
    let remaining = queue.list(None).pop().unwrap();
    assert_eq!(remaining.state, DelayedMessageState::Queued);
    assert_eq!(remaining.scheduled_at_ms, 160_000);
    assert_eq!(remaining.tick_index, 1);
    assert_eq!(
        remaining.error.as_deref(),
        Some("AgentReady deadline elapsed")
    );
}

#[test]
fn restart_recovery_skips_an_uncertain_tick_before_resuming() {
    let (dir, queue) = queue();
    let msg = queue
        .enqueue_with_recurrence(
            "tab".into(),
            None,
            None,
            "tick".into(),
            Some(100_000),
            ReminderCreator::default(),
            Some(RecurrenceRule {
                every_ms: 60_000,
                until_ms: None,
                count: Some(3),
            }),
            None,
        )
        .unwrap();
    queue.claim_due(100_000).unwrap();
    drop(queue);

    let reopened = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
    let recovered = reopened.list(Some("tab")).pop().unwrap();
    assert!(recovered.last_tick_unconfirmed);
    reopened.finish_tick(&msg.id, 100_000, None, true).unwrap();
    let next = reopened.list(Some("tab")).pop().unwrap();
    assert!(!next.last_tick_unconfirmed);
    assert_eq!(next.scheduled_at_ms, 160_000);
    assert_eq!(next.skipped_ticks, 1);
}

#[test]
fn deleting_team_series_removes_only_team_bound_records() {
    let (_dir, queue) = queue();
    let create_series = |id_team: Option<&str>| {
        queue
            .enqueue_with_recurrence(
                "tab".into(),
                None,
                None,
                "tick".into(),
                Some(100_000),
                ReminderCreator::default(),
                Some(RecurrenceRule {
                    every_ms: 60_000,
                    until_ms: None,
                    count: Some(3),
                }),
                id_team.map(str::to_string),
            )
            .unwrap()
    };
    let team_series = create_series(Some("team-a"));
    let other_series = create_series(Some("team-b"));
    let removed = queue.delete_team_series("team-a").unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].id, team_series.id);
    let remaining = queue.list(None);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, other_series.id);
}

#[test]
fn claim_due_uses_schedule_then_creation_order() {
    let (_dir, queue) = queue();
    let second = queue
        .enqueue(
            "tab".into(),
            None,
            None,
            "second".into(),
            Some(100),
            ReminderCreator::default(),
        )
        .unwrap();
    let first = queue
        .enqueue(
            "tab".into(),
            None,
            None,
            "first".into(),
            Some(50),
            ReminderCreator::default(),
        )
        .unwrap();
    let due = queue.claim_due(100).unwrap();
    assert_eq!(
        due.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec![first.id.as_str(), second.id.as_str(),]
    );
    assert!(queue
        .list(Some("tab"))
        .iter()
        .all(|msg| msg.state == DelayedMessageState::Sending));
}

#[test]
fn failed_message_can_be_updated_back_to_queued() {
    let (_dir, queue) = queue();
    let msg = queue
        .enqueue(
            "tab".into(),
            None,
            None,
            "old".into(),
            Some(100),
            ReminderCreator::default(),
        )
        .unwrap();
    queue.mark_failed(&msg.id, "missing Tab".into()).unwrap();
    let updated = queue
        .update(&msg.id, Some("new".into()), Some(200), None)
        .unwrap();
    assert_eq!(updated.text, "new");
    assert_eq!(updated.scheduled_at_ms, 200);
    assert_eq!(updated.state, DelayedMessageState::Queued);
    assert_eq!(updated.error, None);
}

#[test]
fn stashed_messages_are_never_claimed_by_scheduler() {
    let (_dir, queue) = queue();
    let stashed = queue
        .enqueue(
            "tab".into(),
            None,
            None,
            "park me".into(),
            None,
            ReminderCreator::default(),
        )
        .unwrap();
    assert_eq!(stashed.state, DelayedMessageState::Stashed);
    let due = queue.claim_due(now_ms() + 60_000).unwrap();
    assert!(due.is_empty());
    assert_eq!(
        queue.list(Some("tab"))[0].state,
        DelayedMessageState::Stashed
    );
}

#[test]
fn stashed_message_reschedules_into_queue() {
    let (_dir, queue) = queue();
    let stashed = queue
        .enqueue(
            "tab".into(),
            None,
            None,
            "later".into(),
            None,
            ReminderCreator::default(),
        )
        .unwrap();
    let queued = queue.update(&stashed.id, None, Some(500), None).unwrap();
    assert_eq!(queued.state, DelayedMessageState::Queued);
    assert_eq!(queued.scheduled_at_ms, 500);
}

#[test]
fn queued_message_can_be_stashed() {
    let (_dir, queue) = queue();
    let msg = queue
        .enqueue(
            "tab".into(),
            None,
            None,
            "hold".into(),
            Some(500),
            ReminderCreator::default(),
        )
        .unwrap();
    let stashed = queue.update(&msg.id, None, None, Some(true)).unwrap();
    assert_eq!(stashed.state, DelayedMessageState::Stashed);
    let due = queue.claim_due(now_ms() + 60_000).unwrap();
    assert!(due.is_empty());
}

#[test]
fn sending_messages_reset_to_queued_on_open() {
    let (dir, queue) = queue();
    let msg = queue
        .enqueue(
            "tab".into(),
            None,
            None,
            "hello".into(),
            Some(100),
            ReminderCreator::default(),
        )
        .unwrap();
    queue.claim_now(&msg.id).unwrap();
    drop(queue);

    let reopened = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
    assert_eq!(
        reopened.list(Some("tab"))[0].state,
        DelayedMessageState::Queued
    );
}

#[test]
fn open_normalizes_pane_era_records_and_isolates_malformed_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delayed_send.json");
    let fixture = serde_json::json!({
        "version": 2,
        "messages": [
            {
                "id": "dmsg-pane-era",
                "paneId": "target-tab",
                "workspaceId": "workspace",
                "text": "resume",
                "scheduledAtMs": 200,
                "createdAtMs": 100,
                "state": "queued",
                "error": null,
                "sessionIdAtCreate": "session",
                "createdByPane": "creator-tab",
                "createdByLabel": "Creator",
                "createdByModel": "model"
            },
            {
                "id": "dmsg-malformed",
                "paneId": 42,
                "text": ["not", "a", "message"]
            }
        ]
    })
    .to_string();
    fs::write(&path, &fixture).unwrap();

    let queue = DelayedSendQueue::open(path.clone()).unwrap();
    let messages = queue.list(None);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].tab_id, "target-tab");
    assert_eq!(messages[0].created_by_tab.as_deref(), Some("creator-tab"));
    assert_eq!(messages[0].recurrence, None);
    assert_eq!(messages[0].skipped_ticks, 0);
    assert_eq!(fs::read_to_string(path).unwrap(), fixture);
}

#[test]
fn v2_queue_file_loads_with_recurrence_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delayed_send.json");
    fs::write(
            &path,
            r#"{"version":2,"messages":[{"id":"dmsg-v2","tabId":"tab","text":"old","scheduledAtMs":100,"createdAtMs":50,"state":"queued","error":null,"sessionIdAtCreate":null}] }"#,
        )
        .unwrap();
    let queue = DelayedSendQueue::open(path).unwrap();
    let old = queue.list(None).pop().unwrap();
    assert_eq!(old.recurrence, None);
    assert_eq!(old.series_start_at_ms, None);
    assert_eq!(old.skipped_ticks, 0);
    assert_eq!(old.team_id, None);
    assert_eq!(old.last_tick_send_sequence, 0);
}

fn enqueue_one(queue: &DelayedSendQueue, text: &str) -> Result<DelayedMessage, String> {
    queue.enqueue(
        "tab".into(),
        None,
        None,
        text.into(),
        Some(100),
        ReminderCreator::default(),
    )
}

#[test]
fn corrupt_queue_is_preserved_and_recovered_from_backup() {
    let (dir, queue) = queue();
    enqueue_one(&queue, "kept").unwrap();
    drop(queue);
    let path = dir.path().join("delayed_send.json");
    let full = fs::read(&path).unwrap();
    let truncated = full[..full.len() / 2].to_vec();
    fs::write(&path, &truncated).unwrap();

    let reopened = DelayedSendQueue::open(path.clone()).unwrap();

    assert_eq!(reopened.list(None)[0].text, "kept");
    let copies = queue_file(&path).corrupt_copies();
    assert_eq!(fs::read(&copies[0]).unwrap(), truncated);
}

#[test]
fn corrupt_queue_without_backup_is_never_saved_over() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delayed_send.json");
    let broken = b"{\"version\":3,\"messages\":[{\"id\":".to_vec();
    fs::write(&path, &broken).unwrap();

    let queue = DelayedSendQueue::open(path.clone()).unwrap();

    assert!(queue.list(None).is_empty());
    assert!(enqueue_one(&queue, "new").is_err());
    assert!(!path.exists());
    let copies = queue_file(&path).corrupt_copies();
    assert_eq!(fs::read(&copies[0]).unwrap(), broken);
}

#[test]
fn malformed_record_is_preserved_before_the_next_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delayed_send.json");
    let fixture = r#"{"version":2,"messages":[{"id":"dmsg-bad","paneId":42}]}"#;
    fs::write(&path, fixture).unwrap();
    let queue = DelayedSendQueue::open(path.clone()).unwrap();

    enqueue_one(&queue, "new").unwrap();

    let copies = queue_file(&path).corrupt_copies();
    assert_eq!(fs::read_to_string(&copies[0]).unwrap(), fixture);
    assert_eq!(DelayedSendQueue::open(path).unwrap().list(None).len(), 1);
}
