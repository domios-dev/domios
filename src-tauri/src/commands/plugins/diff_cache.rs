use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DIFF_REFRESH_AFTER: Duration = Duration::from_secs(300);

/// Last-known channel diff plus its refresh state. Computing the diff does a
/// per-channel git clone (4-7s), so it must NEVER run inside the snapshot hot
/// path — stale-but-instant beats fresh-but-blocking. An instance keeps the state
/// self-contained so it can be exercised in isolation (no global test contention).
struct DiffCache {
    cache: Mutex<Option<(Instant, Value)>>,
    refreshing: AtomicBool,
    /// Set by a plugin/channel action so the next read refetches even within the TTL.
    force: AtomicBool,
}

impl DiffCache {
    const fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            refreshing: AtomicBool::new(false),
            force: AtomicBool::new(false),
        }
    }

    /// Return the last-known diff immediately. When the cache is missing, stale,
    /// or force-flagged, hand `produce` to `spawn` (prod: a background thread;
    /// tests: inline) — never run it on the caller. Cold first call returns a stub.
    fn read<P, S>(&self, produce: P, spawn: S) -> Value
    where
        P: Fn() -> Result<Value, String> + Send + 'static,
        S: FnOnce(P),
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
            spawn(produce);
        }
        match snapshot {
            Some((_, v)) => v,
            None => serde_json::json!({ "diff": [], "loading": true }),
        }
    }

    fn run_refresh<P>(&self, produce: P)
    where
        P: Fn() -> Result<Value, String>,
    {
        let started = Instant::now();
        match produce() {
            Ok(v) => {
                if let Ok(mut g) = self.cache.lock() {
                    *g = Some((Instant::now(), v));
                }
                log::debug!(
                    "[plugin-diff] background refresh ok in {}ms",
                    started.elapsed().as_millis()
                );
            }
            Err(e) => log::warn!("[plugin-diff] background refresh failed: {e}"),
        }
        self.refreshing.store(false, Ordering::SeqCst);
    }

    /// Synchronously recompute the diff and store it, returning the fresh value. Unlike
    /// `read`, this never returns stale data or defers to a background thread: the
    /// explicit "refresh channels" action needs the result cached *now*, while any
    /// just-added local channel source still exists, so the snapshot it returns (and the
    /// next passive read) serve the freshly-fetched channels. On producer error the
    /// last-known diff is kept (no empty flash). Passive reads keep using `read`.
    fn refresh_now<P>(&self, produce: P) -> Value
    where
        P: Fn() -> Result<Value, String>,
    {
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
                    .unwrap_or_else(|| serde_json::json!({ "diff": [], "loading": true }))
            }
        }
    }
}

static DIFF: DiffCache = DiffCache::new();

/// Return the last-known channel diff; refresh runs on a background thread,
/// never inline in the snapshot hot path.
pub(super) fn cached_diff(produce: fn() -> Result<Value, String>) -> Value {
    DIFF.read(produce, |p| {
        std::thread::spawn(move || DIFF.run_refresh(p));
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
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    // fully isolated and run the refresh inline (deterministic, no leaked threads).
    fn producer(calls: Arc<AtomicUsize>) -> impl Fn() -> Result<Value, String> + Send + 'static {
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({ "diff": [{ "id": "fresh" }] }))
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

        let out = dc.read(producer(calls.clone()), |p| dc.run_refresh(p));

        assert_eq!(out["diff"][0]["id"], "cached");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a fresh cached diff must NOT run the network producer"
        );
        assert!(!dc.refreshing.load(Ordering::SeqCst));
    }

    #[test]
    fn cold_cache_returns_loading_stub_then_refresh_populates() {
        let dc = DiffCache::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let out = dc.read(producer(calls.clone()), |p| dc.run_refresh(p));
        // Hot path returned the loading stub (captured pre-refresh state).
        assert_eq!(out["loading"], serde_json::json!(true));
        assert_eq!(out["diff"], serde_json::json!([]));

        // Refresh ran exactly once and populated the cache; nothing left in flight.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!dc.refreshing.load(Ordering::SeqCst));
        assert_eq!(
            dc.cache.lock().unwrap().clone().unwrap().1["diff"][0]["id"],
            "fresh"
        );

        // Warm + fresh: a second read serves cache, producer not run again.
        let again = dc.read(producer(calls.clone()), |p| dc.run_refresh(p));
        assert_eq!(again["diff"][0]["id"], "fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn force_keeps_old_diff_visible_and_triggers_one_refresh() {
        let dc = DiffCache::new();
        *dc.cache.lock().unwrap() = Some((
            Instant::now(),
            serde_json::json!({ "diff": [{ "id": "old" }] }),
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        dc.force.store(true, Ordering::SeqCst); // what invalidate_diff() sets

        let out = dc.read(producer(calls.clone()), |p| dc.run_refresh(p));

        // Old diff stays visible (no empty flash) while the refresh runs.
        assert_eq!(out["diff"][0]["id"], "old");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            !dc.force.load(Ordering::SeqCst),
            "force flag must be consumed by the committed refresh"
        );
        assert_eq!(
            dc.cache.lock().unwrap().clone().unwrap().1["diff"][0]["id"],
            "fresh"
        );
    }

    #[test]
    fn explicit_refresh_now_caches_synchronously_and_next_read_serves_it() {
        let dc = DiffCache::new();
        // Cold cache + force set — the state invalidate_diff() leaves after a channel action.
        dc.force.store(true, Ordering::SeqCst);
        let calls = Arc::new(AtomicUsize::new(0));

        // Explicit refresh computes inline and returns the fresh value immediately (not stale,
        // no background defer) — this is what makes the just-added local channel show up now.
        let out = dc.refresh_now(producer(calls.clone()));
        assert_eq!(out["diff"][0]["id"], "fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            !dc.force.load(Ordering::SeqCst),
            "force flag must be consumed by the synchronous refresh"
        );

        // The next passive read serves that fresh value without re-running the producer.
        let again = dc.read(producer(calls.clone()), |p| dc.run_refresh(p));
        assert_eq!(again["diff"][0]["id"], "fresh");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a synchronously-refreshed fresh cache must not re-run the producer"
        );
        assert!(!dc.refreshing.load(Ordering::SeqCst));
    }
}
