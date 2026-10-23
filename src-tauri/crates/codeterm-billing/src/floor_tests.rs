use super::*;
use std::sync::atomic::Ordering;

const ISSUED: u64 = 1_700_000_000;

fn vector_reading(id: &str) -> Reading {
    let vector = producer_vectors().into_iter().find(|v| v.id == id).unwrap();
    Reading::from(serde_json::from_str::<ApplicationEnvelope>(vector.response.get()).unwrap())
}

fn started(
    store: Arc<MemoryGrants>,
    replies: Vec<Result<Reading>>,
    now: u64,
) -> (Billing, Arc<std::sync::atomic::AtomicU64>) {
    let (time, clock) = clock(now);
    let billing = Billing::new(GrantReplies::new(replies)).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        store,
        clock,
    );
    (billing, time)
}

fn authority(billing: &Billing) -> Authority {
    billing
        .snapshot(Some(&session("fixture-user")))
        .access
        .unwrap()
        .authority
}

#[tokio::test]
async fn restored_older_grant_after_restart_is_refused_by_the_durable_floor() {
    let store = Arc::new(MemoryGrants::default());
    let user = session("fixture-user");
    let (billing, _) = started(store.clone(), vec![Ok(paid_grant())], ISSUED + 60);
    billing.prime(&user);
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    let paid_bytes = store.0.lock().clone();
    let (billing, _) = started(
        store.clone(),
        vec![Ok(vector_reading("signed-free-downgrade"))],
        ISSUED + 120,
    );
    billing.prime(&user);
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert_eq!(authority(&billing), Authority::Free);
    assert_eq!(
        store.4.lock().as_ref().unwrap().revisions
            [&crate::Floors::key("fixture-user", "fixture-installation")]
            .revision,
        11
    );

    *store.0.lock() = paid_bytes;
    let (restarted, _) = started(store.clone(), vec![], ISSUED + 180);
    assert_eq!(restarted.prime(&user), None);
    assert_eq!(authority(&restarted), Authority::Free);
    assert!(store.0.lock().is_none());
}

#[tokio::test]
async fn unreadable_floor_fails_closed_for_the_stored_grant() {
    struct Corrupt(MemoryGrants);
    impl GrantStore for Corrupt {
        fn load(&self) -> Result<Option<serde_json::Value>> {
            self.0.load()
        }
        fn save(&self, raw: &serde_json::Value) -> Result<()> {
            self.0.save(raw)
        }
        fn clear(&self) -> Result<()> {
            self.0.clear()
        }
        fn load_hint(&self) -> Result<bool> {
            self.0.load_hint()
        }
        fn save_hint(&self, paid: bool) -> Result<()> {
            self.0.save_hint(paid)
        }
        fn load_floors(&self) -> Result<Option<crate::Floors>> {
            Err("corrupt".into())
        }
        fn save_floors(&self, _: &crate::Floors) -> Result<()> {
            panic!("unreadable floors must never be rewritten")
        }
    }
    let inner = MemoryGrants::default();
    *inner.0.lock() = paid_grant().grant;
    let (_, clock) = clock(ISSUED + 60);
    let billing = Billing::new(GrantReplies::new(vec![Ok(paid_grant())])).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(Corrupt(inner)),
        clock,
    );
    billing.prime(&session("fixture-user"));
    billing.observe();
    assert_eq!(authority(&billing), Authority::Free);
    billing
        .refresh(
            Some(&session("fixture-user")),
            RefreshEvent::SignIn,
            Instant::now(),
        )
        .await;
    assert_eq!(authority(&billing), Authority::Free);
}

#[test]
fn clock_set_back_after_observed_expiry_never_revives_the_lease() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    let user = session("fixture-user");
    let (billing, time) = started(store.clone(), vec![], ISSUED + 60);
    billing.prime(&user);
    assert_eq!(authority(&billing), Authority::Billing);

    time.store(ISSUED + 86_400, Ordering::SeqCst);
    assert_eq!(authority(&billing), Authority::Free);
    billing.observe();
    time.store(ISSUED + 60, Ordering::SeqCst);
    assert_eq!(authority(&billing), Authority::Free);
    assert!(billing.limit_gate().summary().workspaces.is_some());

    let (restarted, _) = started(store.clone(), vec![], ISSUED + 60);
    restarted.prime(&user);
    assert_eq!(authority(&restarted), Authority::Free);
}

#[test]
fn clock_set_back_while_fresh_does_not_extend_the_lease() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    let user = session("fixture-user");
    let (billing, time) = started(store, vec![], ISSUED + 86_000);
    billing.prime(&user);
    assert_eq!(authority(&billing), Authority::Billing);
    time.store(ISSUED + 60, Ordering::SeqCst);
    time.store(ISSUED + 86_399, Ordering::SeqCst);
    assert_eq!(authority(&billing), Authority::Billing);
    time.store(ISSUED + 86_400, Ordering::SeqCst);
    assert_eq!(authority(&billing), Authority::Free);
    time.store(ISSUED + 1_000, Ordering::SeqCst);
    assert_eq!(authority(&billing), Authority::Free);
}

#[test]
fn file_store_floor_survives_restart_and_corruption_reads_as_error() {
    let root = std::env::temp_dir().join(format!(
        "codeterm-floor-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = FileGrantStore::new(root.clone());
    assert_eq!(store.load_floors().unwrap(), None);
    let mut floors = crate::Floors {
        observed_at: ISSUED + 86_400,
        ..Default::default()
    };
    floors.revisions.insert(
        crate::Floors::key("fixture-user", "fixture-installation"),
        codeterm_entitlements::RevisionFloor {
            revision: 11,
            digest: "ab".into(),
        },
    );
    store.save_floors(&floors).unwrap();
    assert_eq!(
        FileGrantStore::new(root.clone()).load_floors().unwrap(),
        Some(floors)
    );
    std::fs::write(root.join("state/account/grant_floor.json"), b"{").unwrap();
    assert!(FileGrantStore::new(root.clone()).load_floors().is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deleted_floor_with_restored_grant_is_refused() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    *store.4.lock() = None;
    let (billing, _) = started(store.clone(), vec![], ISSUED + 60);
    billing.prime(&session("fixture-user"));
    assert_eq!(authority(&billing), Authority::Free);
    assert!(store.load().unwrap().is_none());
}

#[test]
fn prime_uses_observed_floor_before_deciding_checking_and_its_deadline() {
    let store = Arc::new(MemoryGrants::default());
    store_verified(&store, &paid_grant().grant.unwrap());
    store.4.lock().as_mut().unwrap().observed_at = ISSUED + 86_400;
    *store.3.lock() = true;
    let (billing, _) = started(store, vec![], ISSUED + 60);
    assert_eq!(
        billing.prime(&session("fixture-user")),
        Some(ISSUED + 86_400 + CHECKING_DEADLINE_SECS)
    );
    assert_eq!(authority(&billing), Authority::Free);
    assert!(billing.snapshot(Some(&session("fixture-user"))).checking);
}

#[tokio::test]
async fn issued_in_future_raises_observed_and_expires_cached_grant() {
    let (activation, newer, older) = signed_paid_revisions_at(ISSUED + 86_400, ISSUED);
    let store = Arc::new(MemoryGrants::default());
    let (_, clock) = clock(ISSUED + 60);
    let billing = Billing::new(GrantReplies::new(vec![Ok(older), Ok(newer)])).with_test_grants(
        activation,
        "fixture-installation",
        store.clone(),
        clock,
    );
    let user = session("fixture-user");
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, Instant::now())
        .await;
    assert_eq!(authority(&billing), Authority::Billing);
    let snapshot = billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, Instant::now())
        .await;
    assert_eq!(snapshot.grant.unwrap().revision, 10);
    assert_eq!(authority(&billing), Authority::Free);
    assert_eq!(
        store.load_floors().unwrap().unwrap().observed_at,
        ISSUED + 86_400
    );
    assert!(!store.load_hint().unwrap());
}

#[tokio::test]
async fn clock_two_minutes_behind_refresh_keeps_paid_access() {
    let store = Arc::new(MemoryGrants::default());
    let (billing, _) = started(store.clone(), vec![Ok(paid_grant())], ISSUED - 120);
    let snapshot = billing
        .refresh(
            Some(&session("fixture-user")),
            RefreshEvent::SignIn,
            Instant::now(),
        )
        .await;
    assert_eq!(snapshot.access.unwrap().authority, Authority::Billing);
    assert!(store.load_hint().unwrap());
}

#[tokio::test]
async fn explicit_and_checkout_refresh_bypass_focus_throttle() {
    let provider = GrantReplies::new(vec![Ok(paid_grant()), Ok(paid_grant()), Ok(paid_grant())]);
    let (_, clock) = clock(ISSUED + 60);
    let billing = Billing::new(provider.clone()).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(MemoryGrants::default()),
        clock,
    );
    let user = session("fixture-user");
    let instant = Instant::now();
    billing
        .refresh(Some(&user), RefreshEvent::SignIn, instant)
        .await;
    billing
        .refresh(Some(&user), RefreshEvent::Focus, instant)
        .await;
    assert_eq!(provider.count(), 1);
    billing
        .refresh(Some(&user), RefreshEvent::Explicit, instant)
        .await;
    assert_eq!(provider.count(), 2);
    billing
        .refresh(Some(&user), RefreshEvent::CheckoutReturned, instant)
        .await;
    assert_eq!(provider.count(), 3);
}
