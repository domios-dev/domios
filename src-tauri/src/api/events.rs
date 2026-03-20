/// Event broadcasting — Webhooks for external agent integration.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct CodeTermEvent {
    pub event_type: String,
    pub data: serde_json::Value,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRegistration {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub pane_id: Option<String>,
    #[serde(default)]
    pub once: bool,
    #[serde(default)]
    pub payload_extra: Option<serde_json::Value>,
}

pub struct EventBroadcaster {
    webhooks: Arc<Mutex<HashMap<String, WebhookRegistration>>>,
}

impl EventBroadcaster {
    pub fn new() -> Self {
        Self {
            webhooks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn broadcast(&self, event_type: &str, data: serde_json::Value) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = CodeTermEvent {
            event_type: event_type.to_string(),
            data: data.clone(),
            timestamp,
        };

        // Get matching webhooks
        let (matching, to_remove): (Vec<_>, Vec<_>) = {
            let hooks = self.webhooks.lock();
            let mut matching = Vec::new();
            let mut to_remove = Vec::new();

            for (id, hook) in hooks.iter() {
                // Filter by event type
                if !hook.events.is_empty() && !hook.events.contains(&event_type.to_string()) {
                    continue;
                }
                // Filter by pane_id
                if let Some(ref filter_pane) = hook.pane_id {
                    if let Some(event_pane) = data.get("pane_id").and_then(|v| v.as_str()) {
                        if filter_pane != event_pane { continue; }
                    }
                }
                matching.push(hook.clone());
                if hook.once {
                    to_remove.push(id.clone());
                }
            }
            (matching, to_remove)
        };

        // Remove one-shot hooks
        if !to_remove.is_empty() {
            let mut hooks = self.webhooks.lock();
            for id in &to_remove {
                hooks.remove(id);
            }
        }

        // Fire webhooks
        for hook in matching {
            let url = hook.url.clone();
            let headers = hook.headers.clone();
            let mut payload = serde_json::json!({
                "event": event.event_type,
                "data": event.data,
                "timestamp": event.timestamp,
            });
            if let Some(extra) = &hook.payload_extra {
                payload["extra"] = extra.clone();
            }

            tokio::spawn(async move {
                let client = reqwest::Client::new();
                let mut req = client.post(&url)
                    .json(&payload)
                    .timeout(std::time::Duration::from_secs(5));
                for (k, v) in &headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                let _ = req.send().await;
            });
        }
    }

    pub fn register_webhook(&self, reg: WebhookRegistration) -> String {
        let id = reg.id.clone();
        self.webhooks.lock().insert(id.clone(), reg);
        id
    }

    pub fn unregister_webhook(&self, id: &str) {
        self.webhooks.lock().remove(id);
    }

    pub fn list_webhooks(&self) -> Vec<WebhookRegistration> {
        self.webhooks.lock().values().cloned().collect()
    }
}

pub type EventBroadcasterState = Arc<EventBroadcaster>;
