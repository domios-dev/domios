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

pub async fn observe_polls<F: std::future::Future>(
    method: &str,
    route: &str,
    future: F,
) -> F::Output {
    #[cfg(debug_assertions)]
    {
        let label = format!("{method} {route}");
        let mut future = std::pin::pin!(future);
        std::future::poll_fn(move |context| {
            let started = Instant::now();
            let result = future.as_mut().poll(context);
            let elapsed = started.elapsed();
            if elapsed >= STALL_THRESHOLD {
                log::debug!(
                    "[REST-POLL] {} execution={}ms pending={} thread={:?}",
                    label,
                    elapsed.as_millis(),
                    result.is_pending(),
                    std::thread::current().id()
                );
            }
            result
        })
        .await
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (method, route);
        future.await
    }
}
