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
    let (leader, owns_first) = dedupe.acquire(key.clone(), 1000);
    assert!(owns_first, "first acquire owns the dispatch");
    leader.complete(ok_resp(1));
    let (_dup, owns_second) = dedupe.acquire(key, 1000);
    assert!(!owns_second, "the same key must not re-dispatch");
}

#[test]
fn distinct_keys_admit_independently() {
    let dedupe = MeshDeliveryDedupe::new();
    let (_a, owns_a) = dedupe.acquire(dedupe_key("peerA", "m1"), 1000);
    let (_b, owns_b) = dedupe.acquire(dedupe_key("peerA", "m2"), 1000);
    let (_c, owns_c) = dedupe.acquire(dedupe_key("peerB", "m1"), 1000);
    assert!(owns_a && owns_b && owns_c, "distinct keys each dispatch");
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
            let (entry, owns) = dedupe.acquire(key, 5000);
            if owns {
                dispatched.fetch_add(1, Ordering::SeqCst);
                let resp = ok_resp(42);
                entry.complete(resp.clone());
                resp
            } else {
                entry.wait().await
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
        let (entry, owns) = store.acquire(format!("k{i}"), 1000);
        assert!(owns);
        entry.complete(ok_resp(i as i64));
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
fn in_flight_entries_are_never_evicted() {
    let mut store = Store::default();
    // Never completed: dropping an in-flight leader would let a duplicate race a
    // second local send, so eviction must skip them even past capacity.
    for i in 0..=DEDUPE_CAPACITY {
        store.acquire(format!("k{i}"), 1000);
    }
    assert!(
        store.values.contains_key("k0"),
        "the in-flight leader survives an over-capacity storm"
    );
    assert_eq!(store.values.len(), DEDUPE_CAPACITY + 1);
}

#[test]
fn forgotten_key_re_dispatches() {
    let dedupe = MeshDeliveryDedupe::new();
    let key = dedupe_key("peerA", "m1");
    let (leader, owns_first) = dedupe.acquire(key.clone(), 1000);
    assert!(owns_first);
    leader.complete(ok_resp(1));
    dedupe.forget(&key);
    let (_next, owns_second) = dedupe.acquire(key, 1000);
    assert!(
        owns_second,
        "a forgotten (not-delivered) key re-dispatches on the next attempt"
    );
}

#[test]
fn expired_completed_entry_re_dispatches() {
    let mut store = Store::default();
    let (leader, _) = store.acquire("k".to_string(), 0);
    leader.complete(ok_resp(1));
    let (_next, owns) = store.acquire("k".to_string(), DEDUPE_TTL_MS + 1);
    assert!(
        owns,
        "past the TTL the id re-dispatches (window is bounded)"
    );
}

#[test]
fn within_ttl_eviction_does_not_resurrect_duplicate() {
    let mut store = Store::default();
    let (leader, _) = store.acquire("k".to_string(), 0);
    leader.complete(ok_resp(1));
    // Unrelated deliveries arrive (well below capacity): purge must not drop k
    // while it is still inside its TTL.
    for i in 0..10 {
        let (entry, _) = store.acquire(format!("other{i}"), 100);
        entry.complete(ok_resp(i));
    }
    let (_dup, owns) = store.acquire("k".to_string(), DEDUPE_TTL_MS - 1);
    assert!(
        !owns,
        "within the TTL the duplicate still dedupes — eviction never resurrects it"
    );
}
