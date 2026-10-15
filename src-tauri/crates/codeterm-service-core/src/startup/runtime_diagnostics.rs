use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, OnceLock, Weak,
};
use std::time::{Duration, Instant};

const STALL_THRESHOLD: Duration = Duration::from_millis(100);
const REQUEST_THRESHOLD: Duration = Duration::from_millis(200);
static REQUESTS: OnceLock<Mutex<BTreeMap<u64, Weak<String>>>> = OnceLock::new();
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

fn scheduling_delay(elapsed: Duration, budget: Duration) -> Option<Duration> {
    let delay = elapsed.saturating_sub(budget);
    (delay > STALL_THRESHOLD).then_some(delay)
}

pub struct Watchdog(tokio::task::JoinHandle<()>);

impl Watchdog {
    pub fn spawn(runtime: &'static str) -> Self {
        Self(tokio::spawn(watch(runtime)))
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn watch(runtime: &'static str) {
    loop {
        let started = Instant::now();
        let _ = tokio::time::timeout(STALL_THRESHOLD, std::future::pending::<()>()).await;
        if let Some(delay) = scheduling_delay(started.elapsed(), STALL_THRESHOLD) {
            let in_flight = REQUESTS
                .get()
                .and_then(|requests| requests.try_lock().ok())
                .map(|requests| {
                    requests
                        .values()
                        .filter_map(Weak::upgrade)
                        .take(32)
                        .collect::<Vec<_>>()
                });
            log::debug!(
                "[RUNTIME-STALL] {} {}ms in_flight={:?}",
                runtime,
                delay.as_millis(),
                in_flight
            );
        }
    }
}

pub struct RequestTiming {
    id: u64,
    label: Arc<String>,
    started: Instant,
}

impl RequestTiming {
    pub fn start(method: &str, route: &str) -> Self {
        let timing = Self {
            id: NEXT_REQUEST.fetch_add(1, Ordering::Relaxed),
            label: Arc::new(format!("{method} {route}")),
            started: Instant::now(),
        };
        if let Ok(mut requests) = REQUESTS.get_or_init(Mutex::default).try_lock() {
            requests.retain(|_, label| label.strong_count() > 0);
            if requests.len() < 256 {
                requests.insert(timing.id, Arc::downgrade(&timing.label));
            }
        }
        timing
    }
}

impl Drop for RequestTiming {
    fn drop(&mut self) {
        if let Some(mut requests) = REQUESTS.get().and_then(|requests| requests.try_lock().ok()) {
            requests.remove(&self.id);
        }
        let elapsed = self.started.elapsed();
        if elapsed > REQUEST_THRESHOLD {
            log::debug!("[SLOW-REST] {} {}ms", self.label, elapsed.as_millis());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_stall_excludes_timer_budget_and_requires_over_100ms_delay() {
        let budget = Duration::from_millis(100);
        assert_eq!(scheduling_delay(Duration::ZERO, budget), None);
        assert_eq!(scheduling_delay(Duration::from_millis(200), budget), None);
        assert_eq!(
            scheduling_delay(Duration::from_millis(201), budget),
            Some(Duration::from_millis(101))
        );
    }
}
