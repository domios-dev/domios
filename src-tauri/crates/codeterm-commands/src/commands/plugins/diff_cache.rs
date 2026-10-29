use serde_json::Value;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const DIFF_REFRESH_AFTER: Duration = Duration::from_secs(300);
const FAILURE_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Last-known channel diff plus its refresh state.
struct DiffCache {
    cache: Mutex<Option<(Instant, Value)>>,
    failure: Mutex<Option<(Instant, Value)>>,
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
            failure: Mutex::new(None),
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

    fn recent_failure(&self, now: Instant) -> Option<Value> {
        self.failure.lock().ok().and_then(|failure| {
            failure
                .as_ref()
                .filter(|(at, _)| now.saturating_duration_since(*at) < FAILURE_RETRY_AFTER)
                .map(|(_, value)| value.clone())
        })
    }

    fn record_failure(&self, error: &str) {
        *self.failure.lock().unwrap_or_else(|p| p.into_inner()) = Some((
            Instant::now(),
            serde_json::json!({ "diff": [], "errors": { "refresh": error } }),
        ));
    }

    /// Return the last-known diff and refresh stale data in the background.
    fn read<P, S>(&self, produce: P, spawn: S) -> Value
    where
        P: Fn() -> Result<Value, String> + Send + 'static,
        S: FnOnce(u64, P),
    {
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let snapshot = cache.clone();
        let stale = self.force.load(Ordering::SeqCst)
            || (self.recent_failure(Instant::now()).is_none()
                && snapshot
                    .as_ref()
                    .map(|(at, _)| at.elapsed() >= DIFF_REFRESH_AFTER)
                    .unwrap_or(true));
        if stale
            && self
                .refreshing
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            // Consume the force flag now we've committed; an action mid-refresh re-sets it.
            self.force.store(false, Ordering::SeqCst);
            let generation = self.generation.load(Ordering::SeqCst);
            drop(cache);
            spawn(generation, produce);
        } else {
            drop(cache);
        }
        match snapshot {
            Some((_, v)) => v,
            None => self
                .recent_failure(Instant::now())
                .unwrap_or_else(Self::loading),
        }
    }

    fn publish<P>(&self, generation: u64, produce: P)
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
                *self.failure.lock().unwrap_or_else(|p| p.into_inner()) = None;
                log::debug!(
                    "[plugin-diff] background refresh ok in {}ms",
                    started.elapsed().as_millis()
                );
            }
            Ok(Ok(_)) => {
                log::debug!("[plugin-diff] discarded superseded background refresh");
            }
            Ok(Err(error)) => {
                if self.generation.load(Ordering::SeqCst) == generation {
                    self.record_failure(&error);
                }
                log::warn!("[plugin-diff] background refresh failed: {error}");
            }
            Err(_) => {
                if self.generation.load(Ordering::SeqCst) == generation {
                    self.record_failure("Plugin channel refresh panicked");
                }
                log::warn!("[plugin-diff] background refresh panicked");
            }
        }
    }

    fn run_refresh<P>(&self, generation: u64, produce: P)
    where
        P: FnOnce() -> Result<Value, String>,
    {
        self.publish(generation, produce);
        let _cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
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
        if !self.force.load(Ordering::SeqCst) {
            if let Some(failure) = self.recent_failure(Instant::now()) {
                return failure;
            }
        }
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.force.store(false, Ordering::SeqCst);
            let generation = self.generation.load(Ordering::SeqCst);
            drop(cache);
            self.run_refresh(generation, produce);
            return self
                .cached_value()
                .or_else(|| self.recent_failure(Instant::now()))
                .unwrap_or_else(Self::loading);
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
            .or_else(|| self.recent_failure(Instant::now()))
            .unwrap_or_else(Self::loading)
    }

    fn refresh_now<P>(&self, produce: P) -> Value
    where
        P: Fn() -> Result<Value, String>,
    {
        let generation = {
            let _cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            self.force.swap(false, Ordering::SeqCst);
            self.generation.fetch_add(1, Ordering::SeqCst) + 1
        };
        self.publish(generation, produce);
        self.cached_value()
            .or_else(|| self.recent_failure(Instant::now()))
            .unwrap_or_else(Self::loading)
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
mod tests;
