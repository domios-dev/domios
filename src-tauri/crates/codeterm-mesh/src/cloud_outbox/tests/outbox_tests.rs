use std::fs;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use super::super::outbox::{CloudOutbox, CloudOutboxConfig};
use super::super::ports::{CloudOutboxPort, DrainPolicyDecision};
use super::super::types::{
    OutboundOperation, OutboxError, OutboxId, OutboxLimits, OutboxRecordStatus,
    StreamDeliveryOutcome, UnconfirmedReason, DELIVERY_UNCONFIRMED, OUTBOX_FULL,
    REFUSED_BEFORE_WRITE,
};
use super::support::{
    sample_request, ManualClock, ScriptedAction, ScriptedSink, SettledWaiter, SwitchablePolicy,
    TestTempDir,
};

#[tokio::test]
async fn durable_enqueue_survives_restart_and_drains_on_reverse_stream_attach() {
    let temp = TestTempDir::new("domios-outbox-restart");
    let clock = ManualClock::new(1_000);

    let (id1, id2) = {
        let outbox = CloudOutbox::open(
            temp.path(),
            CloudOutboxConfig {
                clock: clock.clone(),
                ..CloudOutboxConfig::default()
            },
        )
        .await
        .unwrap();

        let r1 = outbox
            .enqueue(sample_request(
                "acct-1",
                "cloud-1",
                "home-a",
                OutboundOperation::WorkerAck,
                b"ack-1",
            ))
            .await
            .unwrap();
        let r2 = outbox
            .enqueue(sample_request(
                "acct-1",
                "cloud-1",
                "home-a",
                OutboundOperation::WorkerReport,
                b"report-2",
            ))
            .await
            .unwrap();

        assert_eq!(outbox.snapshot().queued_count, 2);
        assert!(outbox.application_reply(&r1.id).is_none());
        assert!(matches!(
            outbox.record_status(&r1.id),
            Some(OutboxRecordStatus::Queued {
                bytes_written: Some(false),
                ..
            })
        ));
        (r1.id, r2.id)
    };

    // Plant an orphan temp file to prove recovery cleans it without dropping valid records.
    fs::write(temp.path().join("active").join(".orphan.1.0.tmp"), b"junk").unwrap();

    let waiter = SettledWaiter::new();
    let reopened = CloudOutbox::open(
        temp.path(),
        CloudOutboxConfig {
            clock: clock.clone(),
            observer: waiter.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(reopened.snapshot().queued_count, 2);
    assert!(!temp.path().join("active").join(".orphan.1.0.tmp").exists());

    let sink = ScriptedSink::new(
        10,
        "acct-1",
        "cloud-1",
        "home-a",
        vec![
            ScriptedAction::Immediate(StreamDeliveryOutcome::acked(Some(b"ack-reply".to_vec()))),
            ScriptedAction::Immediate(StreamDeliveryOutcome::acked(None)),
        ],
        StreamDeliveryOutcome::acked(None),
    );
    reopened.attach_reverse_stream(sink.clone()).await;

    let s1 = waiter.wait_for_settled(&id1).await;
    let s2 = waiter.wait_for_settled(&id2).await;

    assert!(matches!(
        s1,
        OutboxRecordStatus::Delivered {
            bytes_written: Some(true),
            has_application_reply: true,
            ..
        }
    ));
    assert!(matches!(
        s2,
        OutboxRecordStatus::Delivered {
            bytes_written: Some(true),
            has_application_reply: false,
            ..
        }
    ));
    assert_eq!(sink.delivered_ids(), vec![id1.clone(), id2.clone()]);
    assert_eq!(
        reopened.application_reply(&id1).unwrap().body,
        Some(b"ack-reply".to_vec())
    );
    assert!(reopened.application_reply(&id2).is_none());
    assert_eq!(reopened.snapshot().queued_count, 0);
}

#[tokio::test]
async fn parallel_home_streams_drain_independently_and_never_reroute_or_double_claim() {
    let temp = TestTempDir::new("domios-outbox-parallel");
    let waiter = SettledWaiter::new();
    let outbox = CloudOutbox::open(
        temp.path(),
        CloudOutboxConfig {
            observer: waiter.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    let receipts = outbox
        .enqueue_broadcast(
            sample_request(
                "acct-1",
                "cloud-1",
                "placeholder",
                OutboundOperation::Announce,
                b"hello-homes",
            ),
            vec!["home-a".into(), "home-b".into()],
        )
        .await
        .unwrap();
    assert_eq!(receipts.len(), 2);

    let sink_a = ScriptedSink::ack_all(100, "acct-1", "cloud-1", "home-a");
    let sink_b = ScriptedSink::ack_all(200, "acct-1", "cloud-1", "home-b");

    outbox.attach_reverse_stream(sink_a.clone()).await;
    outbox.attach_reverse_stream(sink_b.clone()).await;

    waiter.wait_for_settled(&receipts[0].id).await;
    waiter.wait_for_settled(&receipts[1].id).await;

    assert_eq!(sink_a.delivered_ids(), vec![receipts[0].id.clone()]);
    assert_eq!(sink_b.delivered_ids(), vec![receipts[1].id.clone()]);
    assert_eq!(waiter.claim_count_for(&receipts[0].id), 1);
    assert_eq!(waiter.claim_count_for(&receipts[1].id), 1);
}

#[tokio::test]
async fn stream_replacement_and_old_detach_preserve_replacement_and_at_most_once() {
    let temp = TestTempDir::new("domios-outbox-replace");
    let waiter = SettledWaiter::new();
    let outbox = CloudOutbox::open(
        temp.path(),
        CloudOutboxConfig {
            observer: waiter.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    let r1 = outbox
        .enqueue(sample_request(
            "acct-1",
            "cloud-1",
            "home-a",
            OutboundOperation::WorkerReport,
            b"first",
        ))
        .await
        .unwrap();
    let r2 = outbox
        .enqueue(sample_request(
            "acct-1",
            "cloud-1",
            "home-a",
            OutboundOperation::WorkerReport,
            b"second",
        ))
        .await
        .unwrap();

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let old_sink = ScriptedSink::new(
        1,
        "acct-1",
        "cloud-1",
        "home-a",
        vec![ScriptedAction::HoldUntilRelease {
            entered: entered.clone(),
            release: release.clone(),
            outcome: StreamDeliveryOutcome::unconfirmed_write_completed(
                UnconfirmedReason::StreamClosedBeforeAck,
                Some("replaced mid-flight".into()),
            ),
        }],
        StreamDeliveryOutcome::not_written("closed"),
    );

    let entered_wait = entered.notified();
    tokio::pin!(entered_wait);
    entered_wait.as_mut().enable();
    outbox.attach_reverse_stream(old_sink.clone()).await;
    tokio::time::timeout(Duration::from_secs(5), entered_wait)
        .await
        .expect("old sink entered deliver");

    // Replace with fresh stream_id=2 while r1 is in flight on stream_id=1.
    let new_sink = ScriptedSink::ack_all(2, "acct-1", "cloud-1", "home-a");
    outbox.attach_reverse_stream(new_sink.clone()).await;

    // Detaching old stream_id=1 must NOT detach replacement stream_id=2.
    outbox.detach_reverse_stream(1);
    assert_eq!(outbox.snapshot().attached_stream_count, 1);

    // Release old_sink -> r1 settles DeliveryUnconfirmed, never replayed; new_sink drains r2!
    release.notify_waiters();

    let status1 = waiter.wait_for_settled(&r1.id).await;
    let status2 = waiter.wait_for_settled(&r2.id).await;

    assert_eq!(status1.code(), DELIVERY_UNCONFIRMED);
    assert_eq!(status1.bytes_written(), Some(true));
    assert!(matches!(status2, OutboxRecordStatus::Delivered { .. }));

    assert_eq!(old_sink.delivered_ids(), vec![r1.id.clone()]);
    assert_eq!(new_sink.delivered_ids(), vec![r2.id.clone()]);
    assert_eq!(waiter.claim_count_for(&r1.id), 1);
}

#[tokio::test]
async fn not_written_requeues_while_unconfirmed_persists_and_never_replays() {
    let temp = TestTempDir::new("domios-outbox-outcomes");
    let waiter = SettledWaiter::new();
    let outbox = CloudOutbox::open(
        temp.path(),
        CloudOutboxConfig {
            observer: waiter.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    let r_retryable = outbox
        .enqueue(sample_request(
            "acct-1",
            "cloud-1",
            "home-a",
            OutboundOperation::Notification,
            b"zero-byte-fail",
        ))
        .await
        .unwrap();
    let r_uncertain = outbox
        .enqueue(sample_request(
            "acct-1",
            "cloud-1",
            "home-a",
            OutboundOperation::PlanRpc,
            b"uncertain-write",
        ))
        .await
        .unwrap();

    // First stream reports NotWritten for r_retryable -> returns to Queued.
    let sink1 = ScriptedSink::new(
        1,
        "acct-1",
        "cloud-1",
        "home-a",
        vec![ScriptedAction::Immediate(
            StreamDeliveryOutcome::not_written("stream closed before write"),
        )],
        StreamDeliveryOutcome::not_written("closed"),
    );
    outbox.attach_reverse_stream(sink1.clone()).await;
    sink1.wait_for_delivery_count(1).await;
    outbox.detach_reverse_stream(1);

    // Second stream acks r_retryable and reports Unconfirmed(None) for r_uncertain.
    let sink2 = ScriptedSink::new(
        2,
        "acct-1",
        "cloud-1",
        "home-a",
        vec![
            ScriptedAction::Immediate(StreamDeliveryOutcome::acked(None)),
            ScriptedAction::Immediate(StreamDeliveryOutcome::unconfirmed_unknown(
                UnconfirmedReason::WriteInterrupted,
                Some("reset mid-frame".into()),
            )),
        ],
        StreamDeliveryOutcome::acked(None),
    );
    outbox.attach_reverse_stream(sink2.clone()).await;

    let s1 = waiter.wait_for_settled(&r_retryable.id).await;
    let s2 = waiter.wait_for_settled(&r_uncertain.id).await;
    assert!(matches!(s1, OutboxRecordStatus::Delivered { .. }));
    assert_eq!(s2.code(), DELIVERY_UNCONFIRMED);
    assert_eq!(s2.bytes_written(), None);

    // Reopen outbox and attach a third stream: r_uncertain MUST remain DeliveryUnconfirmed and NEVER replay!
    drop(outbox);
    let reopened = CloudOutbox::open_default(temp.path()).await.unwrap();
    let status_after_restart = reopened.record_status(&r_uncertain.id).unwrap();
    assert_eq!(status_after_restart.code(), DELIVERY_UNCONFIRMED);
    assert_eq!(reopened.unconfirmed_records().len(), 1);

    let sink3 = ScriptedSink::ack_all(3, "acct-1", "cloud-1", "home-a");
    reopened.attach_reverse_stream(sink3.clone()).await;
    let probe = reopened
        .enqueue(sample_request(
            "acct-1",
            "cloud-1",
            "home-a",
            OutboundOperation::Announce,
            b"probe",
        ))
        .await
        .unwrap();
    sink3.wait_for_delivery_count(1).await;
    assert_eq!(sink3.delivered_ids(), vec![probe.id]);

    // Dismissing removes the unconfirmed receipt cleanly.
    assert!(reopened.dismiss_unconfirmed(&r_uncertain.id).await.unwrap());
    assert!(reopened.record_status(&r_uncertain.id).is_none());
    assert!(reopened.unconfirmed_records().is_empty());
}

#[tokio::test]
async fn drain_policy_defer_refuse_and_expiry_recheck_before_write() {
    let temp = TestTempDir::new("domios-outbox-policy");
    let clock = ManualClock::new(5_000);
    let policy = SwitchablePolicy::new(DrainPolicyDecision::Defer {
        reason: "membership_renewal_pending".into(),
    });
    let waiter = SettledWaiter::new();

    let outbox = CloudOutbox::open(
        temp.path(),
        CloudOutboxConfig {
            clock: clock.clone(),
            policy: policy.clone(),
            observer: waiter.clone(),
            ..CloudOutboxConfig::default()
        },
    )
    .await
    .unwrap();

    // Expired request is refused before write without ever calling sink.
    let mut expired_req = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::WorkerReport,
        b"expired",
    );
    expired_req.caller_context.expires_at_ms = Some(4_999);
    let r_expired = outbox.enqueue(expired_req).await.unwrap();

    let r_deferred = outbox
        .enqueue(sample_request(
            "acct-1",
            "cloud-1",
            "home-a",
            OutboundOperation::WorkerReport,
            b"deferred",
        ))
        .await
        .unwrap();

    let sink = ScriptedSink::ack_all(1, "acct-1", "cloud-1", "home-a");
    outbox.attach_reverse_stream(sink.clone()).await;

    let s_exp = waiter.wait_for_settled(&r_expired.id).await;
    assert_eq!(s_exp.code(), REFUSED_BEFORE_WRITE);
    assert_eq!(s_exp.bytes_written(), Some(false));
    assert!(sink.delivered_ids().is_empty());

    // Now policy allows -> notify_policy_changed wakes drain without polling!
    policy.set(DrainPolicyDecision::Allow);
    outbox.notify_policy_changed();

    let s_def = waiter.wait_for_settled(&r_deferred.id).await;
    assert!(matches!(s_def, OutboxRecordStatus::Delivered { .. }));
    assert_eq!(sink.delivered_ids(), vec![r_deferred.id]);
}

#[tokio::test]
async fn concurrent_enqueues_respect_capacity_without_overcommit() {
    let temp = TestTempDir::new("domios-outbox-concurrent-cap");
    let limits = OutboxLimits {
        max_records: 3,
        max_total_bytes: 64 * 1024,
        max_record_bytes: 8 * 1024,
        max_retained_settled: 16,
        max_retained_unconfirmed: 16,
    };
    let outbox = Arc::new(
        CloudOutbox::open(
            temp.path(),
            CloudOutboxConfig {
                limits,
                ..CloudOutboxConfig::default()
            },
        )
        .await
        .unwrap(),
    );

    let mut handles = Vec::new();
    for idx in 0..8usize {
        let outbox = Arc::clone(&outbox);
        handles.push(tokio::spawn(async move {
            outbox
                .enqueue_with_id(
                    OutboxId::parse(format!("conc-{idx}")).unwrap(),
                    sample_request(
                        "acct-1",
                        "cloud-1",
                        "home-a",
                        OutboundOperation::Notification,
                        b"msg",
                    ),
                )
                .await
        }));
    }

    let mut ok_count = 0usize;
    let mut full_count = 0usize;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => ok_count += 1,
            Err(error)
                if matches!(error, OutboxError::Full { .. }) && error.code() == OUTBOX_FULL =>
            {
                full_count += 1
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    assert_eq!(ok_count, 3);
    assert_eq!(full_count, 5);
    assert_eq!(outbox.snapshot().queued_count, 3);
}
