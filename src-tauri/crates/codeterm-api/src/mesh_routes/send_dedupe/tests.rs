use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn ok_resp(tag: i64) -> RelayResponse {
    RelayResponse {
        ok: true,
        status: 200,
        data: Some(serde_json::json!({ "tag": tag })),
        error: None,
    }
}

fn owner(admission: Admission) -> Arc<DeliveryEntry> {
    match admission {
        Admission::Owner(entry) => entry,
        Admission::Duplicate(_) => panic!("expected Owner, got Duplicate"),
        Admission::AtCapacity => panic!("expected Owner, got AtCapacity"),
    }
}

#[test]
fn dedupe_key_is_sender_scoped() {
    assert_eq!(dedupe_key("peerA", "m1"), dedupe_key("peerA", "m1"));
    assert_ne!(
        dedupe_key("peerA", "m1"),
        dedupe_key("peerB", "m1"),
        "same delivery id from a different sender is a different key"
    );
    assert_ne!(dedupe_key("peerA", "m1"), dedupe_key("peerA", "m2"));
}

#[test]
fn same_sender_and_key_admits_once() {
    let dedupe = MeshDeliveryDedupe::new();
    let key = dedupe_key("peerA", "m1");
    owner(dedupe.acquire(key.clone(), 1000)).complete(ok_resp(1));
    assert!(
        matches!(dedupe.acquire(key, 1000), Admission::Duplicate(_)),
        "the same key must not re-dispatch"
    );
}

#[test]
fn distinct_keys_admit_independently() {
    let dedupe = MeshDeliveryDedupe::new();
    assert!(matches!(
        dedupe.acquire(dedupe_key("peerA", "m1"), 1000),
        Admission::Owner(_)
    ));
    assert!(matches!(
        dedupe.acquire(dedupe_key("peerA", "m2"), 1000),
        Admission::Owner(_)
    ));
    assert!(matches!(
        dedupe.acquire(dedupe_key("peerB", "m1"), 1000),
        Admission::Owner(_)
    ));
}

#[tokio::test]
async fn concurrent_duplicates_dispatch_once_and_replay() {
    let dedupe = Arc::new(MeshDeliveryDedupe::new());
    let dispatched = Arc::new(AtomicUsize::new(0));
    let key = dedupe_key("peerA", "storm-1");

    let mut handles = Vec::new();
    for _ in 0..24 {
        let dedupe = dedupe.clone();
        let dispatched = dispatched.clone();
        let key = key.clone();
        handles.push(tokio::spawn(async move {
            match dedupe.acquire(key, 5000) {
                Admission::Owner(entry) => {
                    dispatched.fetch_add(1, Ordering::SeqCst);
                    let resp = ok_resp(42);
                    entry.complete(resp.clone());
                    resp
                }
                Admission::Duplicate(entry) => entry.wait().await,
                Admission::AtCapacity => panic!("a single key never saturates the window"),
            }
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }

    assert_eq!(
        dispatched.load(Ordering::SeqCst),
        1,
        "the local send is invoked exactly once under a duplicate storm"
    );
    assert_eq!(results.len(), 24);
    for resp in &results {
        assert!(resp.ok);
        assert_eq!(
            resp.data,
            Some(serde_json::json!({ "tag": 42 })),
            "every duplicate replays the same optimistic response"
        );
    }
}

#[test]
fn completed_storm_is_bounded_to_capacity() {
    let mut store = Store::default();
    for i in 0..=DEDUPE_CAPACITY {
        owner(store.acquire(format!("k{i}"), 1000)).complete(ok_resp(i as i64));
    }
    assert_eq!(
        store.values.len(),
        DEDUPE_CAPACITY,
        "completed entries are bounded at capacity"
    );
    assert!(
        !store.values.contains_key("k0"),
        "oldest completed entry is evicted"
    );
    assert!(
        store.values.contains_key(&format!("k{DEDUPE_CAPACITY}")),
        "newest entry retained"
    );
}

#[test]
fn in_flight_distinct_storm_is_bounded_by_admission() {
    let mut store = Store::default();
    // Fill capacity with in-flight leaders (never completed).
    let mut leaders = Vec::new();
    for i in 0..DEDUPE_CAPACITY {
        leaders.push(owner(store.acquire(format!("k{i}"), 1000)));
    }
    assert_eq!(store.values.len(), DEDUPE_CAPACITY);

    // New distinct keys are rejected — never grow the map, never dispatch.
    for i in 0..64 {
        assert!(
            matches!(
                store.acquire(format!("new{i}"), 1000),
                Admission::AtCapacity
            ),
            "a new distinct key is rejected while the window is full of in-flight leaders"
        );
        assert!(
            store.values.len() <= DEDUPE_CAPACITY,
            "the map never exceeds the hard cap"
        );
        assert!(
            !store.values.contains_key(&format!("new{i}")),
            "a rejected key is not admitted"
        );
    }

    // A duplicate of an in-flight leader still replays — never rejected.
    assert!(
        matches!(
            store.acquire("k0".to_string(), 1000),
            Admission::Duplicate(_)
        ),
        "a same-key waiter replays the in-flight leader"
    );

    // Completing a leader frees exactly one slot; other in-flight leaders survive.
    leaders[0].complete(ok_resp(0));
    assert!(matches!(
        store.acquire("fresh".to_string(), 1000),
        Admission::Owner(_)
    ));
    assert!(
        store.values.contains_key("k1"),
        "other in-flight leaders are untouched"
    );
    assert!(store.values.len() <= DEDUPE_CAPACITY);
}

#[test]
fn forgotten_key_re_dispatches() {
    let dedupe = MeshDeliveryDedupe::new();
    let key = dedupe_key("peerA", "m1");
    owner(dedupe.acquire(key.clone(), 1000)).complete(ok_resp(1));
    dedupe.forget(&key);
    assert!(
        matches!(dedupe.acquire(key, 1000), Admission::Owner(_)),
        "a forgotten (not-delivered) key re-dispatches on the next attempt"
    );
}

#[test]
fn expired_completed_entry_re_dispatches() {
    let mut store = Store::default();
    owner(store.acquire("k".to_string(), 0)).complete(ok_resp(1));
    assert!(
        matches!(
            store.acquire("k".to_string(), DEDUPE_TTL_MS + 1),
            Admission::Owner(_)
        ),
        "past the TTL the id re-dispatches (window is bounded)"
    );
}

#[test]
fn within_ttl_eviction_does_not_resurrect_duplicate() {
    let mut store = Store::default();
    owner(store.acquire("k".to_string(), 0)).complete(ok_resp(1));
    // Unrelated deliveries arrive (well below capacity): purge must not drop k
    // while it is still inside its TTL.
    for i in 0..10 {
        owner(store.acquire(format!("other{i}"), 100)).complete(ok_resp(i));
    }
    assert!(
        matches!(
            store.acquire("k".to_string(), DEDUPE_TTL_MS - 1),
            Admission::Duplicate(_)
        ),
        "within the TTL the duplicate still dedupes — eviction never resurrects it"
    );
}
