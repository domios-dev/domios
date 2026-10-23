use super::*;
use std::sync::atomic::Ordering;

const ISSUED: u64 = 1_700_000_000;

fn billing_with(
    enforced: bool,
    store: Arc<MemoryGrants>,
    replies: Vec<Result<Reading>>,
    now: u64,
) -> (Billing, Arc<std::sync::atomic::AtomicU64>) {
    let (time, clock) = clock(now);
    let mut activation = activation("fixture-online-v1");
    activation.enforcement.enabled = enforced;
    let billing = Billing::new(GrantReplies::new(replies)).with_test_grants(
        activation,
        "fixture-installation",
        store,
        clock,
    );
    (billing, time)
}

fn stored(grant: Option<serde_json::Value>, hint: bool) -> Arc<MemoryGrants> {
    let store = Arc::new(MemoryGrants::default());
    *store.0.lock() = grant;
    *store.3.lock() = hint;
    store
}

#[test]
fn cached_valid_verdict_starts_paid_without_checking() {
    let store = stored(paid_grant().grant, false);
    let (billing, _) = billing_with(false, store, vec![], ISSUED + 60);
    let user = session("fixture-user");
    assert_eq!(billing.prime(&user), None);
    let snapshot = billing.snapshot(Some(&user));
    assert!(!snapshot.checking);
    assert_eq!(snapshot.access.unwrap().authority, Authority::Billing);
    assert_eq!(billing.limit_gate().summary().workspaces, None);
}

#[tokio::test]
async fn invalid_cache_with_paid_hint_checks_then_locks_after_deadline_offline() {
    let store = stored(paid_grant().grant, true);
    let now = ISSUED + 86_400;
    let (billing, time) = billing_with(true, store, vec![Err("offline".into())], now);
    let user = session("fixture-user");
    let deadline = billing.prime(&user).expect("checking");
    assert_eq!(deadline, now + CHECKING_DEADLINE_SECS);
    assert!(!billing.limit_gate().summary().enforcing);
    let offline = billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert!(offline.checking);
    assert_eq!(offline.access.unwrap().authority, Authority::Free);
    time.store(deadline, Ordering::SeqCst);
    assert!(!billing.snapshot(Some(&user)).checking);
    let gate = billing.limit_gate().summary();
    assert!(gate.enforcing);
    assert_eq!(
        gate.workspaces,
        codeterm_entitlements::COMPILED_FREE_LIMITS.workspaces.max
    );
}

#[test]
fn tampered_cache_is_free_and_removed() {
    let mut grant = paid_grant().grant.unwrap();
    grant["key_id"] = "tampered".into();
    let store = stored(Some(grant), false);
    let (billing, _) = billing_with(false, store.clone(), vec![], ISSUED + 60);
    let user = session("fixture-user");
    assert_eq!(billing.prime(&user), None);
    assert_eq!(
        billing.snapshot(Some(&user)).access.unwrap().authority,
        Authority::Free
    );
    assert!(store.load().unwrap().is_none());
}

#[test]
fn paid_hint_alone_never_grants_paid() {
    let (billing, _) = billing_with(true, stored(None, true), vec![], ISSUED + 60);
    let user = session("fixture-user");
    assert!(billing.prime(&user).is_some());
    let snapshot = billing.snapshot(Some(&user));
    assert!(snapshot.checking);
    assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
    assert!(!billing.entitlement_allows(Some(&user), Capability::PaidFeatures));
}

#[tokio::test]
async fn definite_answer_ends_checking_and_records_the_hint() {
    let store = stored(None, true);
    let (billing, _) = billing_with(false, store.clone(), vec![Ok(paid_grant())], ISSUED + 60);
    let user = session("fixture-user");
    assert!(billing.prime(&user).is_some());
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert!(!snapshot.checking);
    assert_eq!(snapshot.access.unwrap().authority, Authority::Billing);
    assert!(store.load_hint().unwrap());
}
