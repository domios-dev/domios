use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
/// Event broadcasting — Webhooks for external agent integration,
/// plus an in-process typed broadcast bus used by plugins and by the
/// Tauri-emit bridge that keeps the frontend in sync.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::plugin::activity::PluginActivity;

/// Typed system events published on the in-process bus.
/// Plugins subscribe to this enum; the webhook broadcaster still fires
/// per-event-type HTTP POSTs for external integrations.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SystemEvent {
    MuxStateChanged,
    PaneTitlePinned {
        pane_id: String,
        title: String,
    },
    PaneFocused {
        pane_id: String,
    },
    PaneClosed {
        pane_id: String,
    },
    AgentStarted {
        pane_id: String,
        provider: String,
    },
    AgentIdle {
        pane_id: String,
        duration_secs: u64,
    },
    ConfigChanged {
        section: String,
        key: String,
    },
    /// A plugin contribution map changed (provider added/removed, etc.).
    /// `kind` identifies which contribution type changed: "agent_provider",
    /// "skill", "command", "pane_renderer", etc. Frontend hooks filter by kind
    /// and refetch the matching list. Generic on purpose so future contribution
    /// types reuse the same channel without churn.
    ContributionsChanged {
        kind: String,
    },
    PluginActivity(PluginActivity),
    /// Escape hatch for events that don't have a typed variant yet.
    Generic {
        event_type: String,
        data: serde_json::Value,
    },
}

impl SystemEvent {
    /// Short-form event type string for webhook filtering and logging.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::MuxStateChanged => "mux-state-changed",
            Self::PaneTitlePinned { .. } => "pane-title-pinned",
            Self::PaneFocused { .. } => "pane-focused",
            Self::PaneClosed { .. } => "pane-closed",
            Self::AgentStarted { .. } => "agent-started",
            Self::AgentIdle { .. } => "agent-idle",
            Self::ConfigChanged { .. } => "config-changed",
            Self::ContributionsChanged { .. } => "contributions-changed",
            Self::PluginActivity(_) => "plugin-activity",
            Self::Generic { .. } => "generic",
        }
    }
}

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
    system_bus: broadcast::Sender<SystemEvent>,
}

impl EventBroadcaster {
    pub fn new() -> Self {
        let (system_bus, _) = broadcast::channel::<SystemEvent>(256);
        Self {
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            system_bus,
        }
    }

    /// Publish a typed system event. Fans out to in-process subscribers
    /// (plugins, Tauri bridge) AND to matching webhooks via the JSON path.
    pub fn publish(&self, event: SystemEvent) {
        let event_type = event.type_name();
        let data = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
        let _ = self.system_bus.send(event);
        self.broadcast(event_type, data);
    }

    /// Subscribe to the typed in-process event bus. Each subscriber gets
    /// its own receiver; slow subscribers lag and must handle `Lagged`.
    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent> {
        self.system_bus.subscribe()
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
                        if filter_pane != event_pane {
                            continue;
                        }
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
                let mut req = client
                    .post(&url)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_reaches_subscriber() {
        let bc = EventBroadcaster::new();
        let mut rx = bc.subscribe();
        bc.publish(SystemEvent::MuxStateChanged);
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, SystemEvent::MuxStateChanged));
    }

    #[tokio::test]
    async fn publish_carries_payload() {
        let bc = EventBroadcaster::new();
        let mut rx = bc.subscribe();
        bc.publish(SystemEvent::PaneTitlePinned {
            pane_id: "p1".into(),
            title: "hello".into(),
        });
        match rx.recv().await.unwrap() {
            SystemEvent::PaneTitlePinned { pane_id, title } => {
                assert_eq!(pane_id, "p1");
                assert_eq!(title, "hello");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn multiple_subscribers_each_receive() {
        let bc = EventBroadcaster::new();
        let mut rx1 = bc.subscribe();
        let mut rx2 = bc.subscribe();
        bc.publish(SystemEvent::PaneFocused {
            pane_id: "p".into(),
        });
        assert!(matches!(
            rx1.recv().await.unwrap(),
            SystemEvent::PaneFocused { .. }
        ));
        assert!(matches!(
            rx2.recv().await.unwrap(),
            SystemEvent::PaneFocused { .. }
        ));
    }

    #[tokio::test]
    async fn slow_subscriber_lags_without_blocking_publisher() {
        let bc = EventBroadcaster::new();
        let mut rx = bc.subscribe();
        for i in 0..300u64 {
            bc.publish(SystemEvent::AgentIdle {
                pane_id: "p".into(),
                duration_secs: i,
            });
        }
        // Channel capacity is 256; oldest entries are dropped. First recv should signal Lagged, then continue yielding the remaining tail.
        let first = rx.recv().await;
        assert!(matches!(first, Err(broadcast::error::RecvError::Lagged(_))));
    }

    #[test]
    fn type_name_covers_all_variants() {
        assert_eq!(
            SystemEvent::MuxStateChanged.type_name(),
            "mux-state-changed"
        );
        assert_eq!(
            SystemEvent::PaneTitlePinned {
                pane_id: "a".into(),
                title: "b".into()
            }
            .type_name(),
            "pane-title-pinned",
        );
        assert_eq!(
            SystemEvent::PluginActivity(PluginActivity {
                id: "a".into(),
                plugin_id: "plugin".into(),
                kind: "install".into(),
                label: "Installing".into(),
                percent: None,
                status: "running".into(),
                error: None,
                started_at: 1,
                updated_at: 1,
            })
            .type_name(),
            "plugin-activity",
        );
    }
}

/// Emit a named frontend event BOTH to the Tauri webview and to the SSE
/// bus so the web mirror sees it. Drop-in for raw `app.emit(name, payload)`
/// at sites that matter to the browser bundle.
pub fn emit_everywhere(app: &tauri::AppHandle, event_type: &str, data: serde_json::Value) {
    use tauri::{Emitter, Manager};
    let _ = app.emit(event_type, data.clone());
    if let Some(events) = app.try_state::<EventBroadcasterState>() {
        let _ = events.system_bus.send(SystemEvent::Generic {
            event_type: event_type.to_string(),
            data,
        });
    }
}
