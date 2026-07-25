use serde_json::Value;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const DIFF_REFRESH_AFTER: Duration = Duration::from_secs(300);

#[derive(Default)]
struct DiffRefreshState {
    running_generation: u64,
    completed_generation: u64,
}

/// Last-known channel diff plus its refresh state. Computing the diff does a
/// per-channel git clone (4-7s), so it must NEVER run inside the snapshot hot
/// path — stale-but-instant beats fresh-but-blocking. An instance keeps the state
/// self-contained so it can be exercised in isolation (no global test contention).
struct DiffCache {
    cache: Mutex<Option<(Instant, Value)>>,
    refreshing: AtomicBool,
    refresh_generation: AtomicU64,
    refresh_state: Mutex<DiffRefreshState>,
    refresh_cond: Condvar,
    /// Set by a plugin/channel action so the next read refetches even within the TTL.
    force: AtomicBool,
}

impl DiffCache {
    const fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            refreshing: AtomicBool::new(false),
            refresh_generation: AtomicU64::new(0),
            refresh_state: Mutex::new(DiffRefreshState::default()),
            refresh_cond: Condvar::new(),
            force: AtomicBool::new(false),
        }
    }

    fn next_generation(&self) -> u64 {
        self.refresh_generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1)
    }

    fn mark_running(&self, generation: u64) {
        if let Ok(mut state) = self.refresh_state.lock() {
            state.running_generation = generation;
        }
    }

    fn mark_completed(&self, generation: u64) {
        if let Ok(mut state) = self.refresh_state.lock() {
            if state.running_generation == generation {
                state.running_generation = 0;
            }
            if state.completed_generation < generation {
                state.completed_generation = generation;
            }
            self.refresh_cond.notify_all();
        }
    }

    /// Return the last-known diff immediately. When the cache is missing, stale,
    /// or force-flagged, hand `produce` to `spawn` (prod: a background thread;
    /// tests: inline) — never run it on the caller. Cold first call returns a stub.
    fn read<P, S>(&self, produce: P, spawn: S) -> (Value, bool, Option<u64>)
    where
        P: Fn() -> Result<Value, String> + Send + 'static,
        S: FnOnce(P, u64),
    {
        let snapshot = self.cache.lock().ok().and_then(|g| g.clone());
        let stale = self.force.load(Ordering::SeqCst)
            || snapshot
                .as_ref()
                .map(|(at, _)| at.elapsed() >= DIFF_REFRESH_AFTER)
                .unwrap_or(true);
        let mut generation = None;
        if stale
            && self
                .refreshing
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            // Consume the force flag now we've committed; an action mid-refresh re-sets it.
            self.force.store(false, Ordering::SeqCst);
            let current_generation = self.next_generation();
            self.mark_running(current_generation);
            generation = Some(current_generation);
            spawn(produce, current_generation);
        } else if stale {
            generation = self
                .refresh_state
                .lock()
                .ok()
                .and_then(|state| Some(state.running_generation).filter(|g| *g != 0));
        }

        let is_stub = snapshot.is_none();
        let value = match snapshot {
            Some((_, v)) => v,
            None => serde_json::json!({ "diff": [], "loading": true }),
        };
        (value, is_stub, generation)
    }

    fn run_refresh_with_generation<P>(&self, produce: P, generation: u64)
    where
        P: Fn() -> Result<Value, String>,
    {
        let started = Instant::now();
        let produced = panic::catch_unwind(AssertUnwindSafe(&produce));
        match produced {
            Ok(Ok(value)) => {
                if let Ok(mut g) = self.cache.lock() {
                    *g = Some((Instant::now(), value));
                }
                log::debug!(
                    "[plugin-diff] background refresh ok in {}ms",
                    started.elapsed().as_millis()
                );
            }
            Ok(Err(error)) => {
                log::warn!("[plugin-diff] background refresh failed: {error}");
                self.force.store(false, Ordering::SeqCst);
                self.mark_completed(generation);
                self.refreshing.store(false, Ordering::SeqCst);
                return;
            }
            Err(payload) => {
                let reason = if let Some(reason) = payload.downcast_ref::<String>() {
                    reason.to_string()
                } else if let Some(reason) = payload.downcast_ref::<&'static str>() {
                    reason.to_string()
                } else {
                    "diff refresh panic".to_string()
                };
                log::warn!("[plugin-diff] background refresh panicked: {reason}");
                self.force.store(false, Ordering::SeqCst);
                self.mark_completed(generation);
                self.refreshing.store(false, Ordering::SeqCst);
                return;
            }
        };
        self.force.store(false, Ordering::SeqCst);
        self.mark_completed(generation);
        self.refreshing.store(false, Ordering::SeqCst);
    }

    fn run_refresh<P>(&self, produce: P, generation: u64)
    where
        P: Fn() -> Result<Value, String>,
    {
        self.run_refresh_with_generation(produce, generation)
    }

    fn wait_for_completion(&self, generation: u64) -> bool {
        if generation == 0 {
            return true;
        }
        let mut state = match self.refresh_state.lock() {
            Ok(state) => state,
            Err(_) => return false,
        };
        while state.completed_generation < generation {
            match self.refresh_cond.wait(state) {
                Ok(next_state) => {
                    state = next_state;
                    if state.completed_generation >= generation {
                        return true;
                    }
                }
                Err(poisoned) => {
                    state = poisoned.into_inner();
                    if state.completed_generation >= generation {
                        return true;
                    }
                }
            }
        }
        true
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
    let (first, is_stub, generation) = DIFF.read(produce, |p, generation| {
        std::thread::spawn(move || DIFF.run_refresh_with_generation(p, generation));
    });
    if is_stub && generation.is_some() {
        let generation = generation.unwrap_or_default();
        DIFF.wait_for_completion(generation);
        if let Some((_, v)) = DIFF.cache.lock().ok().and_then(|g| g.clone()) {
            return v;
        }
    }

    first
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
            Ok(serde_json::json!({ "diff": [{ "id": "fresh" }] }))
        }
    }

    fn blocking_producer(
        calls: Arc<AtomicUsize>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    ) -> impl Fn() -> Result<Value, String> + Send + 'static {
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            let (lock, notify) = &*gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = notify.wait(open).unwrap();
            }
            Ok(serde_json::json!({
                "diff": [{
                    "id": "git",
                    "local_version": "0.1.0",
                    "remote_version": "0.1.2",
                    "drift": "update_available"
                }]
            }))
        }
    }

    fn release_gate(gate: &Arc<(Mutex<bool>, Condvar)>) {
        let (lock, notify) = &**gate;
        let mut open = lock.lock().unwrap();
        *open = true;
        notify.notify_all();
    }

    fn fail_once_then_succeed(
        calls: Arc<AtomicUsize>,
        should_fail: Arc<AtomicBool>,
        value: Value,
    ) -> impl Fn() -> Result<Value, String> + Send + 'static {
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            if should_fail.swap(false, Ordering::SeqCst) {
                return Err("boom".into());
            }
            Ok(value.clone())
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

        let generation = dc.next_generation();
        let (out, ..) = dc.read(producer(calls.clone()), |p, _| {
            dc.run_refresh(p, generation)
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
    fn cold_cache_waits_for_background_refresh_completion() {
        let dc = DiffCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));

        let (out, .., generation) = dc.read(
            blocking_producer(calls.clone(), gate.clone()),
            |p, generation| {
                thread::spawn(move || dc.run_refresh_with_generation(p, generation));
            },
        );

        assert_eq!(out["loading"], serde_json::json!(true));
        assert_eq!(out["diff"], serde_json::json!([]));
        let generation = generation.expect("cold cache should start one background generation");
        assert_eq!(dc.refreshing.load(Ordering::SeqCst), true);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        release_gate(&gate);
        assert!(dc.wait_for_completion(generation));

        let (again, .., generation_second) = dc.read(producer(calls.clone()), |p, _| {
            let generation = dc.next_generation();
            dc.run_refresh(p, generation);
        });
        assert_eq!(again["diff"][0]["id"], "git");
        assert_eq!(again["diff"][0]["local_version"], "0.1.0");
        assert_eq!(again["diff"][0]["remote_version"], "0.1.2");
        assert_eq!(again["diff"][0]["drift"], "update_available");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(generation_second, None);
        assert!(!dc.refreshing.load(Ordering::SeqCst));
        assert!(!dc.force.load(Ordering::SeqCst));
    }

    #[test]
    fn force_keeps_old_diff_visible_and_triggers_one_refresh() {
        let dc = DiffCache::new();
        *dc.cache.lock().unwrap() = Some((
            Instant::now(),
            serde_json::json!({ "diff": [{ "id": "old" }] }),
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        dc.force.store(true, Ordering::SeqCst);

        let (out, .., generation) = dc.read(
            blocking_producer(calls.clone(), gate.clone()),
            |p, generation| {
                thread::spawn(move || dc.run_refresh_with_generation(p, generation));
            },
        );

        assert_eq!(out["diff"][0]["id"], "old");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!dc.force.load(Ordering::SeqCst));
        assert_eq!(generation, Some(1));

        release_gate(&gate);
        assert!(dc.wait_for_completion(1));
        let refreshed = dc.cache.lock().unwrap().clone().unwrap().1;
        assert_eq!(refreshed["diff"][0]["id"], "git");

        let (again, .., generation_again) = dc.read(producer(calls.clone()), |p, _| {
            let generation = dc.next_generation();
            dc.run_refresh(p, generation);
        });
        assert_eq!(again["diff"][0]["id"], "git");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(generation_again, None);
    }

    #[test]
    fn explicit_refresh_now_caches_synchronously_and_next_read_serves_it() {
        let dc = DiffCache::new();
        dc.force.store(true, Ordering::SeqCst);
        let calls = Arc::new(AtomicUsize::new(0));

        let out = dc.refresh_now(producer(calls.clone()));
        assert_eq!(out["diff"][0]["id"], "fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!dc.force.load(Ordering::SeqCst));

        let (again, .., generation_again) = dc.read(producer(calls.clone()), |p, _| {
            let generation = dc.next_generation();
            dc.run_refresh(p, generation);
        });
        assert_eq!(again["diff"][0]["id"], "fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(generation_again, None);
        assert!(!dc.refreshing.load(Ordering::SeqCst));
    }

    #[test]
    fn concurrent_reads_share_one_background_refresh() {
        let dc = Arc::new(DiffCache::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let barrier = Arc::new(Barrier::new(9));
        let mut handles = Vec::new();

        for _ in 0..8 {
            let dc = dc.clone();
            let calls = calls.clone();
            let gate = gate.clone();
            let ready = barrier.clone();
            handles.push(thread::spawn(move || {
                ready.wait();
                dc.read(
                    blocking_producer(calls.clone(), gate.clone()),
                    |p, generation| {
                        thread::spawn(move || dc.run_refresh_with_generation(p, generation));
                    },
                )
            }));
        }

        barrier.wait();
        let mut outputs = Vec::with_capacity(8);
        for handle in handles {
            outputs.push(handle.join().unwrap());
        }

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let generation = outputs[0].2.unwrap();
        assert!(outputs
            .iter()
            .all(|(_, is_stub, gen)| is_stub && gen == &Some(generation)));

        release_gate(&gate);
        assert!(dc.wait_for_completion(generation));

        let (cached, is_stub, _) = dc.read(producer(calls), |p, _| {
            let generation = dc.next_generation();
            dc.run_refresh(p, generation);
        });
        assert_eq!(cached["diff"][0]["id"], "git");
        assert_eq!(cached["diff"][0]["drift"], "update_available");
        assert_eq!(is_stub, false);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cold_cache_error_is_not_cached_and_retries() {
        let dc = DiffCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let fail_once = Arc::new(AtomicBool::new(true));

        let producer = fail_once_then_succeed(
            calls.clone(),
            fail_once.clone(),
            serde_json::json!({
                "diff": [{ "id": "refreshed" }],
            }),
        );
        let (out, .., first_generation) = dc.read(producer, |p, generation| {
            thread::spawn(move || dc.run_refresh_with_generation(p, generation));
        });

        assert_eq!(out["loading"], serde_json::json!(true));
        assert_eq!(out["diff"], serde_json::json!([]));
        let first_generation =
            first_generation.expect("cold cache should start one background generation");
        assert!(dc.wait_for_completion(first_generation));
        assert!(
            dc.cache.lock().unwrap().is_none(),
            "error must not cache a fallback loading row"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let producer = fail_once_then_succeed(
            calls.clone(),
            fail_once.clone(),
            serde_json::json!({
                "diff": [{ "id": "refreshed" }],
            }),
        );
        let (next, .., second_generation) = dc.read(producer, |p, generation| {
            thread::spawn(move || dc.run_refresh_with_generation(p, generation));
        });
        assert_eq!(next["loading"], serde_json::json!(true));
        assert_eq!(second_generation, Some(first_generation + 1));
        assert!(dc.wait_for_completion(second_generation.unwrap()));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let updated = dc.cache.lock().unwrap().clone().unwrap().1;
        assert_eq!(updated["diff"][0]["id"], "refreshed");
    }

    #[test]
    fn panicking_producer_clears_refresh_state() {
        let dc = DiffCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_panic = calls.clone();
        let should_panic = Arc::new(AtomicBool::new(true));
        let panic_once = {
            let should_panic = should_panic.clone();
            move || {
                calls_for_panic.fetch_add(1, Ordering::SeqCst);
                if should_panic.swap(false, Ordering::SeqCst) {
                    panic!("boom");
                }
                Ok(serde_json::json!({ "diff": [{ "id": "recovered" }] }))
            }
        };

        let (out, .., first_generation) = dc.read(panic_once, |p, generation| {
            thread::spawn(move || dc.run_refresh_with_generation(p, generation));
        });
        assert_eq!(out["loading"], serde_json::json!(true));
        let first_generation =
            first_generation.expect("cold cache should start one background generation");
        assert!(dc.wait_for_completion(first_generation));
        assert!(
            !dc.refreshing.load(Ordering::SeqCst),
            "panic must clear refresh flag"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            dc.cache.lock().unwrap().is_none(),
            "panic must not cache a loading stub"
        );

        let (again, .., second_generation) = dc.read(
            || Ok(serde_json::json!({ "diff": [{ "id": "recovered" }] })),
            |p, generation| {
                thread::spawn(move || dc.run_refresh_with_generation(p, generation));
            },
        );
        assert_eq!(second_generation, Some(first_generation + 1));
        assert!(dc.wait_for_completion(second_generation.unwrap()));
        let updated = dc.cache.lock().unwrap().clone().unwrap().1;
        assert_eq!(updated["diff"][0]["id"], "recovered");
    }

    #[test]
    fn slow_cold_refresh_completion_is_observed() {
        let dc = DiffCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let producer_started = Arc::new((Mutex::new(false), Condvar::new()));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let producer = {
            let calls = calls.clone();
            let producer_started = producer_started.clone();
            let gate = gate.clone();
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                let (started_lock, started_notify) = &*producer_started;
                let mut started = started_lock.lock().unwrap();
                *started = true;
                started_notify.notify_all();
                drop(started);

                let (lock, notify) = &*gate;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = notify.wait(open).unwrap();
                }
                Ok(serde_json::json!({
                    "diff": [{
                        "id": "git",
                        "local_version": "0.1.0",
                        "remote_version": "0.1.2",
                        "drift": "update_available"
                    }]
                }))
            }
        };

        let (out, .., generation) = dc.read(producer, |p, generation| {
            thread::spawn(move || dc.run_refresh_with_generation(p, generation));
        });
        assert_eq!(out["loading"], serde_json::json!(true));
        assert_eq!(out["diff"], serde_json::json!([]));

        let (started_lock, started_notify) = &*producer_started;
        let mut started = started_lock.lock().unwrap();
        while !*started {
            started = started_notify.wait(started).unwrap();
        }
        release_gate(&gate);
        let generation = generation.expect("cold cache should start one background generation");
        assert!(dc.wait_for_completion(generation));
        assert_eq!(dc.wait_for_completion(generation), true);
        let completed = dc.cache.lock().unwrap().clone().unwrap().1;
        assert_eq!(completed["diff"][0]["id"], "git");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
