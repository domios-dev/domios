use super::super::dedupe::{
    DedupeDecision, DedupeEntryState, DedupeError, DedupeKey, DedupeLimits, DedupeModel,
    ReceiverDedupe, ReservePlan,
};
use super::super::types::{OutboxId, OutboxLimitKind, DEDUPE_CAPACITY_FULL, DEDUPE_CONFLICT};
use super::support::{ManualClock, TestTempDir};

fn key(account: &str, src: &str, tgt: &str, id: &str) -> DedupeKey {
    DedupeKey {
        account_id: account.into(),
        source_installation_id: src.into(),
        target_installation_id: tgt.into(),
        id: OutboxId::parse(id).unwrap(),
    }
}

#[test]
fn pure_model_scopes_by_full_key_and_rejects_digest_conflicts() {
    let mut model = DedupeModel::new(DedupeLimits::default()).unwrap();
    let k1 = key("acct-1", "cloud-1", "home-a", "msg-1");
    let k_other_acct = key("acct-2", "cloud-1", "home-a", "msg-1");
    let k_other_src = key("acct-1", "cloud-2", "home-a", "msg-1");
    let k_other_tgt = key("acct-1", "cloud-1", "home-b", "msg-1");

    let (ReservePlan::PersistAndExecute(rec), _) = model
        .plan_reserve(k1.clone(), "digest-aaa".into(), 1_000)
        .unwrap()
    else {
        panic!("expected Execute for first seen key");
    };
    model.commit_reserve(*rec);

    // Same key while Reserved -> InFlightUncertain, never Execute or DuplicateCompleted.
    let (plan, _) = model
        .plan_reserve(k1.clone(), "digest-aaa".into(), 1_001)
        .unwrap();
    assert_eq!(
        plan,
        ReservePlan::Immediate(DedupeDecision::InFlightUncertain)
    );

    // Same key with conflicting digest -> refused.
    let err = model
        .plan_reserve(k1.clone(), "digest-bbb".into(), 1_002)
        .unwrap_err();
    assert_eq!(err.code(), DEDUPE_CONFLICT);
    assert!(matches!(err, DedupeError::DigestConflict { .. }));

    // Different account, source, or target with same id do not collide.
    for other in [k_other_acct, k_other_src, k_other_tgt] {
        let (plan, _) = model
            .plan_reserve(other, "digest-other".into(), 1_003)
            .unwrap();
        assert!(matches!(plan, ReservePlan::PersistAndExecute(_)));
    }
}

#[test]
fn pure_model_never_evicts_unexpired_entries_and_prunes_only_expired() {
    let limits = DedupeLimits {
        max_entries: 2,
        max_total_bytes: 16 * 1024,
        max_reply_bytes: 4 * 1024,
        ttl_ms: 5_000,
    };
    let mut model = DedupeModel::new(limits).unwrap();

    for (idx, id) in ["m-1", "m-2"].into_iter().enumerate() {
        let k = key("acct-1", "cloud-1", "home-a", id);
        let (ReservePlan::PersistAndExecute(rec), _) = model
            .plan_reserve(k.clone(), format!("d-{idx}"), 1_000)
            .unwrap()
        else {
            panic!("expected reserve");
        };
        model.commit_reserve(*rec);
        let done = model
            .prepare_complete(&k, Some(b"reply".to_vec()), 1_010)
            .unwrap();
        model.commit_complete(done).unwrap();
    }

    // At t=5999 neither entry has expired; capacity refuses without evicting m-1 or m-2.
    let k3 = key("acct-1", "cloud-1", "home-a", "m-3");
    let err = model
        .plan_reserve(k3.clone(), "d-3".into(), 5_999)
        .unwrap_err();
    assert_eq!(err.code(), DEDUPE_CAPACITY_FULL);
    assert!(matches!(
        err,
        DedupeError::CapacityFull {
            limit_kind: OutboxLimitKind::Count,
            ..
        }
    ));
    assert!(model
        .get(&key("acct-1", "cloud-1", "home-a", "m-1"))
        .is_some());
    assert!(model
        .get(&key("acct-1", "cloud-1", "home-a", "m-2"))
        .is_some());

    // At t=6000 both expired; pruning frees space and accepts m-3.
    let (plan, expired) = model.plan_reserve(k3, "d-3".into(), 6_000).unwrap();
    assert_eq!(expired.len(), 2);
    assert!(matches!(plan, ReservePlan::PersistAndExecute(_)));
}

#[tokio::test]
async fn durable_dedupe_persists_reservation_and_recovers_reply_or_crash_uncertain() {
    let temp = TestTempDir::new("domios-dedupe-durable");
    let clock = ManualClock::new(10_000);
    let limits = DedupeLimits {
        max_entries: 4,
        max_total_bytes: 32 * 1024,
        max_reply_bytes: 4 * 1024,
        ttl_ms: 60_000,
    };

    let k_completed = key("acct-1", "cloud-1", "home-a", "done-1");
    let k_crashed = key("acct-1", "cloud-1", "home-a", "crashed-1");

    {
        let dedupe = ReceiverDedupe::open(temp.path(), limits, clock.clone())
            .await
            .unwrap();

        assert_eq!(
            dedupe
                .reserve(k_completed.clone(), "dig-done".into())
                .await
                .unwrap(),
            DedupeDecision::Execute
        );
        // Reservation file exists on disk before completion.
        assert!(temp.path().join(k_completed.storage_file_name()).is_file());
        // Duplicate while still reserved -> InFlightUncertain.
        assert_eq!(
            dedupe
                .reserve(k_completed.clone(), "dig-done".into())
                .await
                .unwrap(),
            DedupeDecision::InFlightUncertain
        );

        dedupe
            .complete(&k_completed, Some(b"rpc-answer-42".to_vec()))
            .await
            .unwrap();
        assert_eq!(
            dedupe
                .reserve(k_completed.clone(), "dig-done".into())
                .await
                .unwrap(),
            DedupeDecision::DuplicateCompleted {
                reply_body: Some(b"rpc-answer-42".to_vec()),
            }
        );

        // Reserve second key and drop without completing (simulating crash mid-execution).
        assert_eq!(
            dedupe
                .reserve(k_crashed.clone(), "dig-crash".into())
                .await
                .unwrap(),
            DedupeDecision::Execute
        );
    }

    // Reopen after crash.
    clock.advance(500);
    let reopened = ReceiverDedupe::open(temp.path(), limits, clock.clone())
        .await
        .unwrap();

    // Completed entry recovers original terminal reply.
    assert_eq!(
        reopened
            .reserve(k_completed.clone(), "dig-done".into())
            .await
            .unwrap(),
        DedupeDecision::DuplicateCompleted {
            reply_body: Some(b"rpc-answer-42".to_vec()),
        }
    );

    // Unfinished reserved entry became CrashUncertain: never re-executes, never reports delivered.
    assert_eq!(
        reopened
            .reserve(k_crashed.clone(), "dig-crash".into())
            .await
            .unwrap(),
        DedupeDecision::CrashUncertain
    );
    assert!(matches!(
        reopened.record(&k_crashed).unwrap().state,
        DedupeEntryState::CrashUncertain { .. }
    ));

    // Digest conflict still refused after restart.
    let err = reopened
        .reserve(k_completed, "wrong-digest".into())
        .await
        .unwrap_err();
    assert_eq!(err.code(), DEDUPE_CONFLICT);
}
