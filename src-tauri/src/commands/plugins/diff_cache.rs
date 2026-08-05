use serde_json::Value;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const DIFF_REFRESH_AFTER: Duration = Duration::from_secs(300);

/// Last-known channel diff plus its refresh state.
struct DiffCache {
    cache: Mutex<Option<(Instant, Value)>>,
    ready: Condvar,
    refreshing: AtomicBool,
    /// Set by a plugin/channel action so the next read refetches even within the TTL.
    force: AtomicBool,
    generation: AtomicU64,
}

impl DiffCache {
    const fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            ready: Condvar::new(),
            refreshing: AtomicBool::new(false),
            force: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    fn loading() -> Value {
        serde_json::json!({ "diff": [], "loading": true })
    }

    fn cached_value(&self) -> Option<Value> {
        self.cache
            .lock()
            .ok()
            .and_then(|cache| cache.as_ref().map(|(_, value)| value.clone()))
    }

    /// Return the last-known diff and refresh stale data in the background.
    fn read<P, S>(&self, produce: P, spawn: S) -> Value
    where
        P: Fn() -> Result<Value, String> + Send + 'static,
        S: FnOnce(u64, P),
    {
        let snapshot = self.cache.lock().ok().and_then(|g| g.clone());
        let stale = self.force.load(Ordering::SeqCst)
            || snapshot
                .as_ref()
                .map(|(at, _)| at.elapsed() >= DIFF_REFRESH_AFTER)
                .unwrap_or(true);
        if stale
            && self
                .refreshing
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            // Consume the force flag now we've committed; an action mid-refresh re-sets it.
            self.force.store(false, Ordering::SeqCst);
            spawn(self.generation.load(Ordering::SeqCst), produce);
        }
        match snapshot {
            Some((_, v)) => v,
            None => Self::loading(),
        }
    }

    fn run_refresh<P>(&self, generation: u64, produce: P)
    where
        P: FnOnce() -> Result<Value, String>,
    {
        let started = Instant::now();
        let result = panic::catch_unwind(AssertUnwindSafe(produce));
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match result {
            Ok(Ok(value)) if self.generation.load(Ordering::SeqCst) == generation => {
                *cache = Some((Instant::now(), value));
                log::debug!(
                    "[plugin-diff] background refresh ok in {}ms",
                    started.elapsed().as_millis()
                );
            }
            Ok(Ok(_)) => {
                log::debug!("[plugin-diff] discarded superseded background refresh");
            }
            Ok(Err(error)) => {
                log::warn!("[plugin-diff] background refresh failed: {error}");
            }
            Err(_) => {
                log::warn!("[plugin-diff] background refresh panicked");
            }
        }
        self.refreshing.store(false, Ordering::SeqCst);
        self.ready.notify_all();
    }

    fn read_cold<P>(&self, produce: P) -> Value
    where
        P: FnOnce() -> Result<Value, String>,
    {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, value)) = cache.as_ref() {
            return value.clone();
        }
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.force.store(false, Ordering::SeqCst);
            drop(cache);
            let generation = self.generation.load(Ordering::SeqCst);
            self.run_refresh(generation, produce);
            return self.cached_value().unwrap_or_else(Self::loading);
        }

        while cache.is_none() && self.refreshing.load(Ordering::SeqCst) {
            cache = self
                .ready
                .wait(cache)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        cache
            .as_ref()
            .map(|(_, value)| value.clone())
            .unwrap_or_else(Self::loading)
    }

    fn refresh_now<P>(&self, produce: P) -> Value
    where
        P: Fn() -> Result<Value, String>,
    {
        self.generation.fetch_add(1, Ordering::SeqCst);
        match produce() {
            Ok(v) => {
                if let Ok(mut g) = self.cache.lock() {
                    *g = Some((Instant::now(), v.clone()));
                }
                self.force.store(false, Ordering::SeqCst);
                v
            }
            Err(e) => {
                log::warn!("[plugin-diff] synchronous refresh failed: {e}");
                self.cache
                    .lock()
                    .ok()
                    .and_then(|g| g.clone())
                    .map(|(_, v)| v)
                    .unwrap_or_else(Self::loading)
            }
        }
    }
}

static DIFF: DiffCache = DiffCache::new();

/// Cold snapshots run the producer on the existing blocking snapshot worker so
/// the first response is complete. Warm stale snapshots refresh in background.
pub(super) fn cached_diff(produce: fn() -> Result<Value, String>) -> Value {
    if DIFF.cached_value().is_none() {
        return DIFF.read_cold(produce);
    }
    DIFF.read(produce, |generation, p| {
        std::thread::spawn(move || DIFF.run_refresh(generation, p));
    })
}

/// Force the next snapshot to refetch the diff (after a plugin/channel action).
/// Keeps the last-known diff visible until the refresh lands — no empty flash.
pub(super) fn invalidate_diff() {
    DIFF.force.store(true, Ordering::SeqCst);
}

/// Synchronously recompute + cache the channel diff, returning the fresh value. The
/// explicit "refresh channels" action uses this so the snapshot it returns (and the next
/// passive GET) serve freshly-fetched channels instead of a stale background value.
/// Passive snapshot reads keep using the background [`cached_diff`] path.
pub(super) fn refresh_diff_now(produce: fn() -> Result<Value, String>) -> Value {
    DIFF.refresh_now(produce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize},
        Arc, Barrier, Condvar, Mutex,
    };
    use std::thread;

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
    fn cold_error_is_not_cached_and_next_read_retries() {
        let dc = DiffCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = calls.clone();

        let first = dc.read_cold(move || {
            first_calls.fetch_add(1, Ordering::SeqCst);
            Err("offline".to_string())
        });
        assert_eq!(first["loading"], true);
        assert!(dc.cache.lock().unwrap().is_none());
        assert!(!dc.refreshing.load(Ordering::SeqCst));

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
        assert_eq!(first["loading"], true);
        assert!(panicked.load(Ordering::SeqCst));
        assert!(!dc.refreshing.load(Ordering::SeqCst));

        let second = dc.read_cold(|| Ok(serde_json::json!({ "diff": [{ "id": "recovered" }] })));
        assert_eq!(second["diff"][0]["id"], "recovered");
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
}
