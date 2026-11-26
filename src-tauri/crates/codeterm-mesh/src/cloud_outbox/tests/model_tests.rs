use serde_json::json;

use crate::crown::{AuthenticatedAgent, AuthenticatedOrigin};

use super::super::contract::{
    INNER_NOTIFICATION_ROUTE, INNER_RPC_ROUTE, OUTER_ANNOUNCE_ROUTE, OUTER_RELAY_ROUTE,
    OUTER_RPC_ROUTE, WORKER_ACK_RPC_METHOD, WORKER_REPORT_RPC_METHOD,
};
use super::super::model::{
    NextDrainAction, OutboxModel, PersistedActiveState, PersistedOutboxRecord,
};
use super::super::types::{
    CallerContext, InstallationPair, OutboundOperation, OutboundPeerRequest, OutboxError, OutboxId,
    OutboxLimitKind, OutboxLimits, OutboxRecordStatus, StreamDeliveryOutcome, UnconfirmedReason,
    DELIVERY_UNCONFIRMED, OUTBOX_FULL, PAYLOAD_TOO_LARGE,
};
use super::support::sample_request;

#[test]
fn fifo_ordering_and_target_isolation() {
    let mut model = OutboxModel::new(OutboxLimits::default()).expect("valid limits");
    let pair_a = InstallationPair::new("cloud-1", "home-a");
    let pair_b = InstallationPair::new("cloud-1", "home-b");

    let r1 = model
        .reserve_enqueue(
            sample_request(
                "acct-1",
                "cloud-1",
                "home-a",
                OutboundOperation::WorkerAck,
                b"a1",
            ),
            Some(OutboxId::parse("rec-a1").unwrap()),
            100,
        )
        .unwrap();
    model.commit_enqueue(r1);

    let r2 = model
        .reserve_enqueue(
            sample_request(
                "acct-1",
                "cloud-1",
                "home-b",
                OutboundOperation::WorkerReport,
                b"b1",
            ),
            Some(OutboxId::parse("rec-b1").unwrap()),
            101,
        )
        .unwrap();
    model.commit_enqueue(r2);

    let r3 = model
        .reserve_enqueue(
            sample_request(
                "acct-1",
                "cloud-1",
                "home-a",
                OutboundOperation::Notification,
                b"a2",
            ),
            Some(OutboxId::parse("rec-a2").unwrap()),
            102,
        )
        .unwrap();
    model.commit_enqueue(r3);

    model.attach_stream(pair_a.clone(), "acct-1".into(), 10);

    // Home-A stream receives rec-a1 first, never rec-b1.
    let first = model.begin_next_drain(&pair_a, "acct-1", 10);
    let NextDrainAction::Evaluate(frame1) = first else {
        panic!("expected evaluate for rec-a1, got {first:?}");
    };
    assert_eq!(frame1.id.as_str(), "rec-a1");
    assert_eq!(frame1.request.target_installation_id, "home-a");

    // While rec-a1 is evaluating/in-flight, head-of-line blocks rec-a2 for pair_a.
    assert_eq!(
        model.begin_next_drain(&pair_a, "acct-1", 10),
        NextDrainAction::WaitForInFlight
    );

    let claimed = model
        .prepare_claim(&pair_a, &frame1.id, 10, 105)
        .expect("claim rec-a1");
    assert!(matches!(
        claimed.state,
        PersistedActiveState::InFlight { stream_id: 10, .. }
    ));

    // Settle rec-a1 acked -> rec-a2 becomes next for pair_a.
    model.settle_acked(&frame1.id, Some(b"ok".to_vec()), 110);
    let second = model.begin_next_drain(&pair_a, "acct-1", 10);
    let NextDrainAction::Evaluate(frame2) = second else {
        panic!("expected evaluate for rec-a2, got {second:?}");
    };
    assert_eq!(frame2.id.as_str(), "rec-a2");
    model.prepare_claim(&pair_a, &frame2.id, 10, 111).unwrap();
    model.settle_acked(&frame2.id, None, 112);

    // Pair_a is now idle; rec-b1 is still queued and NEVER rerouted to home-a.
    assert_eq!(
        model.begin_next_drain(&pair_a, "acct-1", 10),
        NextDrainAction::Idle
    );
    assert!(matches!(
        model.record_status(&OutboxId::parse("rec-b1").unwrap()),
        Some(OutboxRecordStatus::Queued {
            bytes_written: Some(false),
            ..
        })
    ));

    // Wrong account on pair_b stream cannot drain acct-1's rec-b1.
    model.attach_stream(pair_b.clone(), "acct-other".into(), 20);
    assert_eq!(
        model.begin_next_drain(&pair_b, "acct-other", 20),
        NextDrainAction::Idle
    );
}

#[test]
fn bounds_refuse_count_bytes_and_single_record_with_typed_codes() {
    let base = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::PlanRpc,
        b"12345",
    );
    let single_size = base.accounted_bytes();

    let limits = OutboxLimits {
        max_records: 2,
        max_total_bytes: single_size * 2,
        max_record_bytes: single_size + 4,
        max_retained_settled: 16,
        max_retained_unconfirmed: 16,
    };
    let mut model = OutboxModel::new(limits).unwrap();

    // Single record too large -> payload_too_large.
    let huge = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::PlanRpc,
        b"1234567890",
    );
    let err = model.reserve_enqueue(huge, None, 10).unwrap_err();
    assert_eq!(err.code(), PAYLOAD_TOO_LARGE);
    assert!(matches!(err, OutboxError::PayloadTooLarge { .. }));

    // First record fits.
    let r1 = model.reserve_enqueue(base.clone(), None, 11).unwrap();
    model.commit_enqueue(r1);

    // Slightly larger second record exceeds total bytes -> outbox_full(Bytes).
    let medium = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::PlanRpc,
        b"1234567",
    );
    let err = model.reserve_enqueue(medium, None, 12).unwrap_err();
    assert_eq!(err.code(), OUTBOX_FULL);
    assert!(matches!(
        err,
        OutboxError::Full {
            limit_kind: OutboxLimitKind::Bytes,
            ..
        }
    ));

    // Exact-fit second record succeeds.
    let r2 = model.reserve_enqueue(base.clone(), None, 13).unwrap();
    model.commit_enqueue(r2);

    // Third record exceeds count -> outbox_full(Count).
    let tiny = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::PlanRpc,
        b"1",
    );
    let err = model.reserve_enqueue(tiny, None, 14).unwrap_err();
    assert_eq!(err.code(), OUTBOX_FULL);
    assert!(matches!(
        err,
        OutboxError::Full {
            limit_kind: OutboxLimitKind::Count,
            current: 2,
            limit: 2,
        }
    ));
}

#[test]
fn broadcast_expands_into_targeted_rows_and_checks_capacity_atomically() {
    let base = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::Announce,
        b"hi",
    );
    let limits = OutboxLimits {
        max_records: 2,
        max_total_bytes: 4096,
        max_record_bytes: 2048,
        max_retained_settled: 8,
        max_retained_unconfirmed: 8,
    };
    let mut model = OutboxModel::new(limits).unwrap();

    // 3 targets cannot fit in max_records=2 -> atomic refusal, 0 records queued.
    let err = model
        .reserve_broadcast(
            base.clone(),
            vec!["home-a".into(), "home-b".into(), "home-c".into()],
            50,
        )
        .unwrap_err();
    assert_eq!(err.code(), OUTBOX_FULL);
    assert_eq!(model.snapshot().queued_count, 0);

    // Duplicate target refused.
    let dup_err = model
        .reserve_broadcast(base.clone(), vec!["home-a".into(), "home-a".into()], 51)
        .unwrap_err();
    assert_eq!(dup_err.code(), "invalid_request");

    // 2 distinct targets fit and create two independent targeted records.
    let reservation = model
        .reserve_broadcast(base, vec!["home-a".into(), "home-b".into()], 52)
        .unwrap();
    assert_eq!(reservation.records.len(), 2);
    assert_eq!(
        reservation.records[0].frame.request.target_installation_id,
        "home-a"
    );
    assert_eq!(
        reservation.records[1].frame.request.target_installation_id,
        "home-b"
    );
    assert_ne!(
        reservation.records[0].frame.id,
        reservation.records[1].frame.id
    );
    let receipts = model.commit_enqueue(reservation);
    assert_eq!(receipts.len(), 2);
    assert_eq!(model.snapshot().queued_count, 2);
}

#[test]
fn stream_replacement_and_stale_detach_cannot_remove_replacement() {
    let mut model = OutboxModel::new(OutboxLimits::default()).unwrap();
    let pair = InstallationPair::new("cloud-1", "home-a");

    assert_eq!(model.attach_stream(pair.clone(), "acct-1".into(), 1), None);
    assert!(model.is_current_stream(&pair, 1));

    assert_eq!(
        model.attach_stream(pair.clone(), "acct-1".into(), 2),
        Some(1)
    );
    assert!(!model.is_current_stream(&pair, 1));
    assert!(model.is_current_stream(&pair, 2));

    // Detaching old stream_id=1 must NOT detach replacement stream_id=2.
    assert_eq!(model.detach_stream(1), None);
    assert!(model.is_current_stream(&pair, 2));

    // Old stream sees StreamSuperseded.
    assert_eq!(
        model.begin_next_drain(&pair, "acct-1", 1),
        NextDrainAction::StreamSuperseded
    );

    // Detaching current stream_id=2 removes it.
    assert_eq!(model.detach_stream(2), Some(pair.clone()));
    assert!(!model.is_current_stream(&pair, 2));
}

#[test]
fn recovery_converts_in_flight_to_delivery_unconfirmed_and_preserves_queued() {
    let req1 = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::WorkerReport,
        b"r1",
    );
    let req2 = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::WorkerReport,
        b"r2",
    );
    let rec_inflight = PersistedOutboxRecord {
        version: PersistedOutboxRecord::CURRENT_VERSION,
        accounted_bytes: req1.accounted_bytes(),
        frame: super::super::types::OutboxFrame {
            id: OutboxId::parse("in-flight-id").unwrap(),
            sequence: 7,
            enqueued_at_ms: 1000,
            payload_digest: req1.payload_digest(),
            request: req1,
        },
        state: PersistedActiveState::InFlight {
            stream_id: 42,
            claimed_at_ms: 1005,
        },
    };
    let rec_queued = PersistedOutboxRecord {
        version: PersistedOutboxRecord::CURRENT_VERSION,
        accounted_bytes: req2.accounted_bytes(),
        frame: super::super::types::OutboxFrame {
            id: OutboxId::parse("queued-id").unwrap(),
            sequence: 8,
            enqueued_at_ms: 1010,
            payload_digest: req2.payload_digest(),
            request: req2,
        },
        state: PersistedActiveState::Queued,
    };

    let (model, transitions) = OutboxModel::from_recovered(
        OutboxLimits::default(),
        vec![rec_queued, rec_inflight],
        Vec::new(),
        Vec::new(),
        2000,
    )
    .unwrap();

    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0].stale_active_sequence, 7);
    assert_eq!(transitions[0].unconfirmed.id.as_str(), "in-flight-id");
    assert_eq!(
        transitions[0].unconfirmed.reason,
        UnconfirmedReason::ProcessCrashAfterClaim
    );
    assert_eq!(transitions[0].unconfirmed.bytes_written, None);

    let status = model
        .record_status(&OutboxId::parse("in-flight-id").unwrap())
        .unwrap();
    assert_eq!(status.code(), DELIVERY_UNCONFIRMED);
    assert_eq!(status.bytes_written(), None);
    assert!(status.is_unconfirmed());

    assert_eq!(model.snapshot().queued_count, 1);
    assert_eq!(model.snapshot().unconfirmed_count, 1);
}

#[test]
fn unconfirmed_preserves_true_and_normalizes_false_to_none() {
    assert_eq!(
        StreamDeliveryOutcome::normalize_unconfirmed_bytes_written(Some(true)),
        Some(true)
    );
    assert_eq!(
        StreamDeliveryOutcome::normalize_unconfirmed_bytes_written(Some(false)),
        None
    );
    assert_eq!(
        StreamDeliveryOutcome::normalize_unconfirmed_bytes_written(None),
        None
    );

    let mut model = OutboxModel::new(OutboxLimits::default()).unwrap();
    let req = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::PlanRpc,
        b"call",
    );
    let res = model
        .reserve_enqueue(req, Some(OutboxId::parse("u-1").unwrap()), 10)
        .unwrap();
    model.commit_enqueue(res);

    let receipt = model
        .settle_unconfirmed(
            &OutboxId::parse("u-1").unwrap(),
            UnconfirmedReason::AckTimeout,
            Some(false),
            Some("never claim false on unconfirmed".into()),
            20,
        )
        .unwrap();
    assert_eq!(receipt.bytes_written, None);
    assert_eq!(
        model
            .record_status(&OutboxId::parse("u-1").unwrap())
            .unwrap()
            .bytes_written(),
        None
    );
}

#[test]
fn operations_bind_outer_and_inner_routes_and_persist_no_auth_headers_or_secrets() {
    assert_eq!(
        OutboundOperation::Announce.outer_route(),
        OUTER_ANNOUNCE_ROUTE
    );
    assert_eq!(OutboundOperation::Announce.inner_route(), None);
    assert_eq!(
        OutboundOperation::Announce.direct_outer_route(),
        Some(OUTER_ANNOUNCE_ROUTE)
    );

    for op in [
        OutboundOperation::WorkerAck,
        OutboundOperation::WorkerReport,
        OutboundOperation::PlanRpc,
    ] {
        assert_eq!(op.outer_route(), OUTER_RELAY_ROUTE);
        assert_eq!(op.inner_route(), Some(INNER_RPC_ROUTE));
        assert_eq!(op.direct_outer_route(), Some(OUTER_RPC_ROUTE));
    }

    assert_eq!(
        OutboundOperation::Notification.outer_route(),
        OUTER_RELAY_ROUTE
    );
    assert_eq!(
        OutboundOperation::Notification.inner_route(),
        Some(INNER_NOTIFICATION_ROUTE)
    );
    assert_eq!(OutboundOperation::Notification.direct_outer_route(), None);

    for op in OutboundOperation::ALL {
        assert_eq!(op.http_method(), "POST");
        assert_eq!(op.allowed_http_methods(), &["POST"]);
        let req = sample_request("acct-1", "cloud-1", "home-a", op, b"payload");
        let wire = req.to_outer_wire_request().expect("valid wire envelope");
        assert_eq!(wire.route, op.outer_route());
        if let Some(inner) = op.inner_route() {
            assert_eq!(wire.body["method"], inner.method);
            assert_eq!(wire.body["path"], inner.path);
        }
        let json = serde_json::to_string(&req).unwrap();
        for forbidden in ["headers", "authorization", "bearer", "private_key", "token"] {
            assert!(
                !json.to_ascii_lowercase().contains(forbidden),
                "persisted request must not contain {forbidden}: {json}"
            );
        }
    }
}

#[test]
fn operations_enforce_closed_body_and_origin_binding_contracts() {
    let agent_ctx = |tab_id: &str| CallerContext {
        origin: AuthenticatedOrigin::Agent,
        agent: Some(AuthenticatedAgent {
            instance_id: "cloud-1".into(),
            tab_id: tab_id.into(),
            can_spawn: false,
        }),
        expires_at_ms: None,
    };
    let owner_ctx = CallerContext {
        origin: AuthenticatedOrigin::Owner,
        agent: None,
        expires_at_ms: None,
    };
    let make = |op: OutboundOperation, caller_context: CallerContext, body: serde_json::Value| {
        OutboundPeerRequest {
            account_id: "acct-1".into(),
            source_installation_id: "cloud-1".into(),
            target_installation_id: "home-a".into(),
            operation: op,
            caller_context,
            body: serde_json::to_vec(&body).unwrap(),
        }
    };

    // Announce refuses Agent origin.
    let mut bad_announce = sample_request(
        "acct-1",
        "cloud-1",
        "home-a",
        OutboundOperation::Announce,
        b"a",
    );
    bad_announce.caller_context = agent_ctx("tab-1");
    assert!(bad_announce.validate().is_err());

    // WorkerAck refuses wrong RPC method, forged worker_tab_id, and server-injected fields.
    assert!(make(
        OutboundOperation::WorkerAck,
        agent_ctx("tab-worker-1"),
        json!({
            "method": WORKER_REPORT_RPC_METHOD,
            "request_id": "req-1",
            "params": {
                "plan_id": "pl_1",
                "task_id": "task_1",
                "attempt_id": "att_1",
                "worker_tab_id": "tab-worker-1"
            }
        }),
    )
    .validate()
    .is_err());
    assert!(make(
        OutboundOperation::WorkerAck,
        agent_ctx("tab-worker-1"),
        json!({
            "method": WORKER_ACK_RPC_METHOD,
            "request_id": "req-1",
            "params": {
                "plan_id": "pl_1",
                "task_id": "task_1",
                "attempt_id": "att_1",
                "worker_tab_id": "foreign-tab"
            }
        }),
    )
    .validate()
    .is_err());
    assert!(make(
        OutboundOperation::WorkerAck,
        agent_ctx("tab-worker-1"),
        json!({
            "method": WORKER_ACK_RPC_METHOD,
            "request_id": "req-1",
            "params": {
                "plan_id": "pl_1",
                "task_id": "task_1",
                "attempt_id": "att_1",
                "worker_tab_id": "tab-worker-1",
                "authenticated_mesh_peer_id": "spoofed-peer"
            }
        }),
    )
    .validate()
    .is_err());

    // WorkerReport refuses forged reporter_tab_id and non-in_review status.
    assert!(make(
        OutboundOperation::WorkerReport,
        agent_ctx("tab-worker-1"),
        json!({
            "method": WORKER_REPORT_RPC_METHOD,
            "request_id": "req-2",
            "params": {
                "plan_id": "pl_1",
                "task_id": "task_1",
                "attempt_id": "att_1",
                "report": {
                    "id": "rep-1",
                    "message": "done",
                    "status": "in_review",
                    "reporter_tab_id": "other-tab"
                }
            }
        }),
    )
    .validate()
    .is_err());
    assert!(make(
        OutboundOperation::WorkerReport,
        agent_ctx("tab-worker-1"),
        json!({
            "method": WORKER_REPORT_RPC_METHOD,
            "request_id": "req-2",
            "params": {
                "plan_id": "pl_1",
                "task_id": "task_1",
                "attempt_id": "att_1",
                "report": {
                    "id": "rep-1",
                    "message": "done",
                    "status": "done",
                    "reporter_tab_id": "tab-worker-1"
                }
            }
        }),
    )
    .validate()
    .is_err());

    // Notification admits Owner/Mesh display-only payloads and refuses Agent/Service or forbidden fields.
    assert!(make(
        OutboundOperation::Notification,
        agent_ctx("tab-worker-1"),
        json!({"title": "hi"}),
    )
    .validate()
    .is_err());
    assert!(make(
        OutboundOperation::Notification,
        CallerContext {
            origin: AuthenticatedOrigin::Service,
            agent: None,
            expires_at_ms: None,
        },
        json!({"title": "hi"}),
    )
    .validate()
    .is_err());
    for origin in [AuthenticatedOrigin::Owner, AuthenticatedOrigin::Mesh] {
        assert!(make(
            OutboundOperation::Notification,
            CallerContext {
                origin,
                agent: None,
                expires_at_ms: None,
            },
            json!({"title": "hi", "body": "ok"}),
        )
        .validate()
        .is_ok());
    }
    for (field, value) in [
        ("source_peer", json!("spoofed")),
        ("tab_id", json!("local-tab")),
        ("workspace_id", json!("local-ws")),
        ("target_tab_id", json!("local-tab")),
        ("chat_message_id", json!("msg-1")),
        ("offer", json!("desktop")),
        ("offer_accept", json!("Enable")),
        ("actions", json!([{"type": "answer", "label": "Yes"}])),
    ] {
        let mut payload = json!({"title": "hi"});
        payload[field] = value;
        assert!(
            make(OutboundOperation::Notification, owner_ctx.clone(), payload)
                .validate()
                .is_err(),
            "Notification must reject '{field}'"
        );
    }

    // PlanRpc refuses Agent origin and disallowed or worker-lifecycle methods.
    assert!(make(
        OutboundOperation::PlanRpc,
        agent_ctx("tab-worker-1"),
        json!({"method": "plan.get", "request_id": "req-3", "params": {"plan_id": "pl_1"}}),
    )
    .validate()
    .is_err());
    for forbidden_method in [
        "team.adopt_agent",
        "team.close",
        "shell.exec",
        WORKER_ACK_RPC_METHOD,
        WORKER_REPORT_RPC_METHOD,
    ] {
        assert!(
            make(
                OutboundOperation::PlanRpc,
                owner_ctx.clone(),
                json!({"method": forbidden_method, "request_id": "req-4", "params": {}}),
            )
            .validate()
            .is_err(),
            "PlanRpc must reject {forbidden_method}"
        );
    }
}
