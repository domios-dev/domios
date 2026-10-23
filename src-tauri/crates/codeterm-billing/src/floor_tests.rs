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

#[test]
fn unreadable_floor_fails_closed_for_the_stored_grant() {
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
            Ok(())
        }
    }
    let inner = MemoryGrants::default();
    *inner.0.lock() = paid_grant().grant;
    let (_, clock) = clock(ISSUED + 60);
    let billing = Billing::new(GrantReplies::new(vec![])).with_test_grants(
        activation("fixture-online-v1"),
        "fixture-installation",
        Arc::new(Corrupt(inner)),
        clock,
    );
    billing.prime(&session("fixture-user"));
    assert_eq!(authority(&billing), Authority::Free);
}

#[test]
fn clock_set_back_after_observed_expiry_never_revives_the_lease() {
    let store = Arc::new(MemoryGrants::default());
    *store.0.lock() = paid_grant().grant;
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
    *store.0.lock() = paid_grant().grant;
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
