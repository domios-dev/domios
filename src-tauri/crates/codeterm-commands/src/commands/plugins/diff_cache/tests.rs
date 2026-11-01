use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize},
    Arc, Barrier, Condvar, Mutex,
};
use std::thread;

#[test]
fn older_explicit_completions_cannot_replace_newer_success_or_install_backoff() {
    for fails in [false, true] {
        let dc = Arc::new(DiffCache::new());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker = {
            let dc = dc.clone();
            let entered = entered.clone();
            let release = release.clone();
            thread::spawn(move || {
                dc.refresh_now(|| {
                    entered.wait();
                    release.wait();
                    if fails {
                        Err("old failure".into())
                    } else {
                        Ok(serde_json::json!({ "diff": [{ "id": "old" }] }))
                    }
                })
            })
        };
        entered.wait();
        let latest = dc.refresh_now(|| Ok(serde_json::json!({ "diff": [{ "id": "new" }] })));
        release.wait();
        assert_eq!(worker.join().unwrap(), latest);
        assert_eq!(dc.cached_value().unwrap(), latest);
        assert!(dc.failure.lock().unwrap().is_none());
    }
}

#[test]
fn invalidation_during_explicit_refresh_survives_completion() {
    let dc = DiffCache::new();
    dc.refresh_now(|| {
        dc.force.store(true, Ordering::SeqCst);
        Ok(serde_json::json!({ "diff": [] }))
    });
    assert!(dc.force.load(Ordering::SeqCst));
    let calls = Arc::new(AtomicUsize::new(0));
    dc.read(producer(calls.clone()), |g, p| dc.run_refresh(g, p));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn explicit_panic_backs_off_then_explicit_retry_recovers() {
    let dc = DiffCache::new();
    let result = dc.refresh_now(|| panic!("explicit panic"));
    assert!(result["errors"]["refresh"].is_string());
    let calls = Arc::new(AtomicUsize::new(0));
    dc.read_cold(producer(calls.clone()));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    dc.refresh_now(producer(calls.clone()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(dc.failure.lock().unwrap().is_none());
}

fn producer(calls: Arc<AtomicUsize>) -> impl Fn() -> Result<Value, String> + Send + 'static {
    move || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({
            "diff": [{
                "id": "bitwarden",
                "local_version": "0.1.0",
                "remote_version": "0.1.2",
                "drift": "update_available"
            }]
        }))
    }
}

#[test]
fn fresh_cache_is_served_without_running_the_producer() {
    let dc = DiffCache::new();
    *dc.cache.lock().unwrap() = Some((
        Instant::now(),
        serde_json::json!({ "diff": [{ "id": "cached" }] }),
    ));
    let calls = Arc::new(AtomicUsize::new(0));

    let out = dc.read(producer(calls.clone()), |generation, p| {
        dc.run_refresh(generation, p)
    });

    assert_eq!(out["diff"][0]["id"], "cached");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "a fresh cached diff must NOT run the network producer"
    );
    assert!(!dc.refreshing.load(Ordering::SeqCst));
}

#[test]
fn cold_cache_returns_completed_refresh() {
    let dc = DiffCache::new();
    let calls = Arc::new(AtomicUsize::new(0));

    let out = dc.read_cold(producer(calls.clone()));

    assert_eq!(out["diff"][0]["id"], "bitwarden");
    assert_eq!(out["diff"][0]["remote_version"], "0.1.2");
    assert_eq!(out["diff"][0]["drift"], "update_available");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!dc.refreshing.load(Ordering::SeqCst));
    assert_eq!(
        dc.cache.lock().unwrap().clone().unwrap().1["diff"][0]["id"],
        "bitwarden"
    );
    let again = dc.read_cold(producer(calls.clone()));
    assert_eq!(again["diff"][0]["id"], "bitwarden");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_cold_reads_run_one_producer() {
    let dc = Arc::new(DiffCache::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(9));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let mut handles = Vec::new();

    for _ in 0..8 {
        let dc = dc.clone();
        let calls = calls.clone();
        let barrier = barrier.clone();
        let release = release.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            dc.read_cold(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                let (lock, ready) = &*release;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = ready.wait(open).unwrap();
                }
                Ok(serde_json::json!({
                    "diff": [{
                        "id": "bitwarden",
                        "local_version": "0.1.0",
                        "remote_version": "0.1.2",
                        "drift": "update_available"
                    }]
                }))
            })
        }));
    }

    barrier.wait();
    let (lock, ready) = &*release;
    let mut open = lock.lock().unwrap();
    *open = true;
    ready.notify_all();
    drop(open);

    let outputs: Vec<Value> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(outputs
        .iter()
        .all(|output| output["diff"][0]["id"] == "bitwarden"));
}

#[test]
fn cold_error_backs_off_then_retries_after_expiry() {
    let dc = DiffCache::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let first_calls = calls.clone();

    let first = dc.read_cold(move || {
        first_calls.fetch_add(1, Ordering::SeqCst);
        Err("offline".to_string())
    });
    assert!(first["errors"]["refresh"].is_string());
    assert!(dc.cache.lock().unwrap().is_none());
    assert!(!dc.refreshing.load(Ordering::SeqCst));

    let suppressed = dc.read_cold(producer(calls.clone()));
    assert_eq!(suppressed["errors"]["refresh"], "offline");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    dc.failure.lock().unwrap().as_mut().unwrap().0 = Instant::now() - FAILURE_RETRY_AFTER;
    let second = dc.read_cold(producer(calls.clone()));
    assert_eq!(second["diff"][0]["id"], "bitwarden");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn cold_panic_does_not_wedge_refresh() {
    let dc = DiffCache::new();
    let panicked = Arc::new(AtomicBool::new(false));
    let marker = panicked.clone();

    let first = dc.read_cold(move || -> Result<Value, String> {
        marker.store(true, Ordering::SeqCst);
        panic!("producer panic");
    });
    assert!(first["errors"]["refresh"].is_string());
    assert!(panicked.load(Ordering::SeqCst));
    assert!(!dc.refreshing.load(Ordering::SeqCst));

    dc.force.store(true, Ordering::SeqCst);
    let second = dc.read_cold(|| Ok(serde_json::json!({ "diff": [{ "id": "recovered" }] })));
    assert_eq!(second["diff"][0]["id"], "recovered");
}

#[test]
fn failure_backoff_is_bounded_and_warm_cache_is_preserved() {
    let dc = DiffCache::new();
    let now = Instant::now();
    let old = serde_json::json!({ "diff": [{ "id": "old" }] });
    *dc.cache.lock().unwrap() = Some((now - DIFF_REFRESH_AFTER, old.clone()));
    dc.run_refresh(0, || Err("offline".into()));
    let at = dc.failure.lock().unwrap().as_ref().unwrap().0;
    assert!(dc
        .recent_failure(at + FAILURE_RETRY_AFTER - Duration::from_nanos(1))
        .is_some());
    assert!(dc.recent_failure(at + FAILURE_RETRY_AFTER).is_none());
    let calls = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        dc.read(producer(calls.clone()), |g, p| dc.run_refresh(g, p)),
        old
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    dc.force.store(true, Ordering::SeqCst);
    dc.read(producer(calls.clone()), |g, p| dc.run_refresh(g, p));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(dc.failure.lock().unwrap().is_none());
}

#[test]
fn explicit_failed_refresh_backs_off_passive_reads_but_can_be_retried() {
    let dc = DiffCache::new();
    let first = dc.refresh_now(|| Err("offline".into()));
    assert_eq!(first["errors"]["refresh"], "offline");
    let calls = Arc::new(AtomicUsize::new(0));
    dc.read_cold(producer(calls.clone()));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    dc.refresh_now(producer(calls.clone()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(dc.failure.lock().unwrap().is_none());
}

#[test]
fn superseded_failure_does_not_delay_the_new_generation() {
    let dc = DiffCache::new();
    dc.generation.store(1, Ordering::SeqCst);
    dc.run_refresh(0, || Err("superseded".into()));
    assert!(dc.failure.lock().unwrap().is_none());
}

#[test]
fn force_keeps_old_diff_visible_and_triggers_one_refresh() {
    let dc = DiffCache::new();
    *dc.cache.lock().unwrap() = Some((
        Instant::now(),
        serde_json::json!({ "diff": [{ "id": "old" }] }),
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    dc.force.store(true, Ordering::SeqCst);

    let out = dc.read(producer(calls.clone()), |generation, p| {
        dc.run_refresh(generation, p)
    });

    assert_eq!(out["diff"][0]["id"], "old");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        !dc.force.load(Ordering::SeqCst),
        "force flag must be consumed by the committed refresh"
    );
    assert_eq!(
        dc.cache.lock().unwrap().clone().unwrap().1["diff"][0]["id"],
        "bitwarden"
    );
}

#[test]
fn explicit_refresh_now_caches_synchronously_and_next_read_serves_it() {
    let dc = DiffCache::new();
    dc.force.store(true, Ordering::SeqCst);
    let calls = Arc::new(AtomicUsize::new(0));

    let out = dc.refresh_now(producer(calls.clone()));
    assert_eq!(out["diff"][0]["id"], "bitwarden");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        !dc.force.load(Ordering::SeqCst),
        "force flag must be consumed by the synchronous refresh"
    );

    let again = dc.read(producer(calls.clone()), |generation, p| {
        dc.run_refresh(generation, p)
    });
    assert_eq!(again["diff"][0]["id"], "bitwarden");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a synchronously-refreshed fresh cache must not re-run the producer"
    );
    assert!(!dc.refreshing.load(Ordering::SeqCst));
}

#[test]
fn explicit_refresh_cannot_be_overwritten_by_older_background_work() {
    let dc = Arc::new(DiffCache::new());
    *dc.cache.lock().unwrap() = Some((
        Instant::now() - DIFF_REFRESH_AFTER,
        serde_json::json!({ "diff": [{ "id": "cached" }] }),
    ));
    let started = Arc::new(Barrier::new(2));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let worker = {
        let dc = dc.clone();
        let started = started.clone();
        let release = release.clone();
        thread::spawn(move || {
            dc.read(
                move || {
                    started.wait();
                    let (lock, ready) = &*release;
                    let mut open = lock.lock().unwrap();
                    while !*open {
                        open = ready.wait(open).unwrap();
                    }
                    Ok(serde_json::json!({ "diff": [{ "id": "stale" }] }))
                },
                |generation, producer| dc.run_refresh(generation, producer),
            )
        })
    };

    started.wait();
    let fresh = dc.refresh_now(|| Ok(serde_json::json!({ "diff": [{ "id": "fresh" }] })));
    assert_eq!(fresh["diff"][0]["id"], "fresh");
    let (lock, ready) = &*release;
    *lock.lock().unwrap() = true;
    ready.notify_all();
    worker.join().unwrap();

    assert_eq!(
        dc.cache.lock().unwrap().as_ref().unwrap().1["diff"][0]["id"],
        "fresh"
    );
}
