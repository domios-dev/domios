use super::{encoding::bounded_json, CodeTermEvent, WebhookRegistration, MAX_EVENT_BYTES};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;

pub(super) const MAX_WEBHOOKS: usize = 128;
pub(super) const MAX_REGISTRATION_BYTES: usize = 16 * 1024;
const MAX_DELIVERIES: usize = 32;
const MAX_DELIVERY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct Admission {
    hooks: HashSet<String>,
    bytes: usize,
}

type Hooks = Mutex<HashMap<String, Arc<WebhookRegistration>>>;

struct Delivery {
    hook: Arc<WebhookRegistration>,
    body: Arc<String>,
    cost: usize,
    pending: Arc<Mutex<Admission>>,
}

impl Drop for Delivery {
    fn drop(&mut self) {
        let mut pending = self.pending.lock();
        pending.hooks.remove(&self.hook.id);
        pending.bytes -= self.cost;
    }
}

#[derive(Default)]
pub(super) struct DeliveryOwner {
    worker: Mutex<Option<(mpsc::Sender<Delivery>, tokio::task::AbortHandle)>>,
    pending: Arc<Mutex<Admission>>,
    refused: AtomicU64,
}

impl DeliveryOwner {
    pub(super) fn refused(&self) -> u64 {
        self.refused.load(Ordering::Relaxed)
    }

    fn enqueue(&self, hook: Arc<WebhookRegistration>, body: Arc<String>) -> bool {
        let mut worker = self.worker.lock();
        if worker.is_none() {
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                return false;
            };
            let (sender, mut receiver) = mpsc::channel::<Delivery>(MAX_DELIVERIES);
            let task = runtime.spawn(async move {
                let client = reqwest::Client::new();
                while let Some(delivery) = receiver.recv().await {
                    let mut body = delivery.body.as_ref().clone();
                    if let Some(extra) = &delivery.hook.payload_extra {
                        body.pop();
                        body.push_str(",\"extra\":");
                        body.push_str(&extra.to_string());
                        body.push('}');
                    }
                    let mut request = client
                        .post(&delivery.hook.url)
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .body(body)
                        .timeout(Duration::from_secs(5));
                    for (key, value) in &delivery.hook.headers {
                        request = request.header(key, value);
                    }
                    let _ = request.send().await;
                }
            });
            *worker = Some((sender, task.abort_handle()));
        }
        let cost = body.len() + MAX_REGISTRATION_BYTES;
        {
            let mut pending = self.pending.lock();
            if pending.hooks.contains(&hook.id)
                || pending.hooks.len() >= MAX_DELIVERIES
                || cost > MAX_DELIVERY_BYTES.saturating_sub(pending.bytes)
            {
                return false;
            }
            pending.hooks.insert(hook.id.clone());
            pending.bytes += cost;
        }
        let delivery = Delivery {
            hook,
            body,
            cost,
            pending: self.pending.clone(),
        };
        worker
            .as_ref()
            .is_some_and(|(sender, _)| sender.try_send(delivery).is_ok())
    }

    pub(super) fn broadcast(&self, hooks: &Hooks, event_type: &str, data: serde_json::Value) {
        if hooks.lock().is_empty() {
            return;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let event = CodeTermEvent {
            event_type: event_type.into(),
            data,
            timestamp,
        };
        let Some(encoded) = bounded_json(&event, MAX_EVENT_BYTES) else {
            self.refused.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let matching: Vec<_> = hooks
            .lock()
            .values()
            .filter(|hook| {
                (hook.events.is_empty() || hook.events.iter().any(|kind| kind == event_type))
                    && hook.tab_id.as_ref().is_none_or(|tab| {
                        event
                            .data
                            .get("tab_id")
                            .and_then(|v| v.as_str())
                            .is_none_or(|current| current == tab)
                    })
            })
            .cloned()
            .collect();
        let encoded = Arc::new(encoded);
        for hook in matching {
            if !self.enqueue(hook.clone(), encoded.clone()) {
                self.refused.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if hook.once {
                let mut hooks = hooks.lock();
                if hooks
                    .get(&hook.id)
                    .is_some_and(|current| Arc::ptr_eq(current, &hook))
                {
                    hooks.remove(&hook.id);
                }
            }
        }
    }
}

impl Drop for DeliveryOwner {
    fn drop(&mut self) {
        if let Some((_, task)) = self.worker.get_mut() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests;
