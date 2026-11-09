use std::collections::BTreeMap;
use std::future::{poll_fn, Future};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

const MAX_REQUESTS: usize = 512;
static REQUESTS: LazyLock<Arc<Registry>> = LazyLock::new(|| Arc::new(Registry::default()));

#[derive(Default)]
struct Registry {
    next: AtomicU64,
    omitted: AtomicU64,
    requests: Mutex<BTreeMap<u64, Request>>,
}

struct Request {
    method: String,
    route: String,
    started: Instant,
    phase: &'static str,
    phase_started: Instant,
    thread: String,
}

struct Activity {
    registry: Arc<Registry>,
    id: Option<u64>,
}

impl Activity {
    fn start(registry: Arc<Registry>, method: &str, route: &str) -> Self {
        let id = registry.next.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let retained = {
            let mut requests = registry
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if requests.len() >= MAX_REQUESTS {
                registry.omitted.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                requests.insert(
                    id,
                    Request {
                        method: method.chars().take(16).collect(),
                        route: route.chars().take(256).collect(),
                        started,
                        phase: "waiting",
                        phase_started: started,
                        thread: String::new(),
                    },
                );
                true
            }
        };
        Self {
            registry,
            id: retained.then_some(id),
        }
    }

    fn phase(&self, phase: &'static str) {
        let Some(id) = self.id else { return };
        let current = std::thread::current();
        let thread = format!("{} {:?}", current.name().unwrap_or("unnamed"), current.id());
        if let Some(request) = self
            .registry
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(&id)
        {
            request.phase = phase;
            request.phase_started = Instant::now();
            request.thread = thread;
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.registry
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&id);
        }
    }
}

async fn track_with<F: Future>(
    registry: Arc<Registry>,
    method: &str,
    route: &str,
    future: F,
) -> F::Output {
    let activity = Activity::start(registry, method, route);
    let mut future = std::pin::pin!(future);
    poll_fn(|cx| {
        activity.phase("polling");
        let result = future.as_mut().poll(cx);
        activity.phase(if result.is_pending() {
            "waiting"
        } else {
            "responded"
        });
        result
    })
    .await
}

pub async fn track<F: Future>(method: &str, route: &str, future: F) -> F::Output {
    track_with(REQUESTS.clone(), method, route, future).await
}

fn snapshot_from(registry: &Registry) -> String {
    let Ok(requests) = registry.requests.try_lock() else {
        return "REST request activity registry is busy; snapshot unavailable".into();
    };
    let now = Instant::now();
    let rows: Vec<_> = requests
        .iter()
        .map(|(id, request)| {
            serde_json::json!({
                "id": id,
                "method": request.method,
                "route": request.route,
                "age_ms": now.duration_since(request.started).as_millis(),
                "phase": request.phase,
                "phase_age_ms": now.duration_since(request.phase_started).as_millis(),
                "thread": request.thread,
            })
        })
        .collect();
    serde_json::json!({ "in_flight": rows, "capacity": MAX_REQUESTS,
        "omitted_since_start": registry.omitted.load(Ordering::Relaxed) })
    .to_string()
}

pub fn snapshot() -> String {
    snapshot_from(&REQUESTS)
}

#[cfg(test)]
mod tests;
