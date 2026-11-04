use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
/// Event broadcasting — Webhooks for external agent integration,
/// plus an in-process typed broadcast bus used by plugins and by the
/// Tauri-emit bridge that keeps the frontend in sync.
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use uuid::Uuid;

mod encoding;
mod webhooks;
use encoding::PreparedEvent;
pub use encoding::MAX_EVENT_BYTES;

use codeterm_plugin::activity::PluginActivity;

/// Typed system events published on the in-process bus.
/// Plugins subscribe to this enum; the webhook broadcaster still fires
/// per-event-type HTTP POSTs for external integrations.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SystemEvent {
    MuxStateChanged,
    TabTitlePinned {
        tab_id: String,
        title: String,
    },
    TabFocused {
        tab_id: String,
    },
    TabClosed {
        tab_id: String,
    },
    /// An agent verb ran against a surface tab; `verb` is a bare token, never its arguments.
    SurfaceActivity {
        tab_id: String,
        verb: String,
        /// The acting agent when it is not the surface owner.
        #[serde(skip_serializing_if = "Option::is_none")]
        actor: Option<String>,
    },
    /// Identity-only hint; views refetch their state through their existing plugin bridge.
    SurfaceStateInvalidated {
        plugin: String,
        surface: String,
        instance: String,
        tab: String,
    },
    AgentStarted {
        tab_id: String,
        provider: String,
    },
    AgentIdle {
        tab_id: String,
        duration_secs: u64,
    },
    ConfigChanged {
        section: String,
        key: String,
    },
    /// A plugin contribution map changed (provider added/removed, etc.).
    /// `kind` identifies which contribution type changed: "agent_provider",
    /// "skill", "command", "tab_renderer", etc. Frontend hooks filter by kind
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
            Self::TabTitlePinned { .. } => "tab-title-pinned",
            Self::TabFocused { .. } => "tab-focused",
            Self::TabClosed { .. } => "tab-closed",
            Self::SurfaceActivity { .. } => "surface-activity",
            Self::SurfaceStateInvalidated { .. } => "surface-state-invalidated",
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

/// A bus event stamped with its monotonic replay cursor (D1). Plugins read
/// `.event`; the SSE bridge uses `.seq` for `?since` replay/dedupe.
#[derive(Debug, Clone)]
pub struct SeqEvent {
    pub seq: u64,
    pub event: SystemEvent,
    pub frame: Arc<EncodedFrame>,
    pub public: bool,
}

#[derive(Debug)]
pub struct EncodedFrame {
    pub seq: u64,
    pub event_type: String,
    pub bytes: Arc<String>,
    pub size: usize,
}

const RING_MAX_COUNT: usize = 2048;
const RING_MAX_AGE: Duration = Duration::from_secs(120);
pub const RING_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const REPLAY_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const REPLAY_MAX_EVENTS: usize = 512;

#[derive(Clone, Copy)]
pub struct ReplayBudget {
    pub max_bytes: usize,
    pub max_events: usize,
}

impl Default for ReplayBudget {
    fn default() -> Self {
        Self {
            max_bytes: REPLAY_MAX_BYTES,
            max_events: REPLAY_MAX_EVENTS,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReplayStats {
    pub retained_bytes: usize,
    pub retained_events: usize,
    pub retained_bytes_high_water: usize,
    pub encoded_events: u64,
    pub published_events: u64,
    pub oversize_singleton: bool,
    pub webhook_refusals: u64,
}

struct RingEntry {
    seq: u64,
    at: Instant,
    event: Arc<SeqEvent>,
}

struct ReplayRing {
    entries: VecDeque<RingEntry>,
    next_seq: u64,
    retained_bytes: usize,
    retained_bytes_high_water: usize,
    encoded_events: u64,
    #[cfg(test)]
    boot_id: String,
}

impl ReplayRing {
    fn append(
        &mut self,
        prepared: PreparedEvent,
        now: Instant,
        public: bool,
    ) -> (SeqEvent, Vec<Arc<SeqEvent>>) {
        let seq = self.next_seq;
        self.next_seq = seq + 1;
        let (event, replay_event, frame) = prepared.stamp(seq);
        self.encoded_events += 1;
        self.retained_bytes += frame.size;
        let event = SeqEvent {
            seq,
            event,
            frame,
            public,
        };
        self.entries.push_back(RingEntry {
            seq,
            at: now,
            event: Arc::new(SeqEvent {
                seq,
                event: replay_event,
                frame: event.frame.clone(),
                public,
            }),
        });
        let mut retired = Vec::new();
        while self.entries.len() > RING_MAX_COUNT
            || (self.entries.len() > 1 && self.retained_bytes > RING_MAX_BYTES)
            || self
                .entries
                .front()
                .is_some_and(|e| now.duration_since(e.at) > RING_MAX_AGE)
        {
            if let Some(entry) = self.entries.pop_front() {
                self.retained_bytes -= entry.event.frame.size;
                retired.push(entry.event);
            }
        }
        self.retained_bytes_high_water = self.retained_bytes_high_water.max(self.retained_bytes);
        (event, retired)
    }

    #[cfg(test)]
    fn push(&mut self, event: SystemEvent, now: Instant, public: bool) -> SeqEvent {
        self.append(PreparedEvent::new(event, &self.boot_id), now, public)
            .0
    }
}

/// Race-free ring snapshot taken AFTER `subscribe()`: the retained events plus
/// the bounds needed to classify a client `since` cursor.
pub struct RingSnapshot {
    pub boot_id: String,
    pub entries: Vec<Arc<SeqEvent>>,
    pub next_seq: u64,
}

/// Classification of a client `?since=B:S` cursor against a ring snapshot (D1).
pub enum ReplayPlan {
    /// No cursor — stream live from now (old-client behavior).
    FromNow,
    /// Boot changed — client must full-resync; overflow frame then live.
    BootChanged { next_seq: u64 },
    /// Cursor covered by the ring — replay events with `seq > S` in order.
    Covered { replay: Vec<Arc<SeqEvent>> },
    /// Cursor or budget exceeded — overflow, then replay the retained suffix.
    Evicted {
        oldest: u64,
        next_seq: u64,
        replay: Vec<Arc<SeqEvent>>,
    },
}

impl RingSnapshot {
    pub fn plan(&self, since: Option<(&str, u64)>, budget: ReplayBudget) -> ReplayPlan {
        let Some((boot, cursor)) = since else {
            return ReplayPlan::FromNow;
        };
        if boot != self.boot_id {
            return ReplayPlan::BootChanged {
                next_seq: self.next_seq,
            };
        }
        let oldest = self.entries.first().map(|e| e.seq).unwrap_or(self.next_seq);
        let needed = self.entries.partition_point(|e| e.seq <= cursor);
        let mut start = self.entries.len();
        let mut bytes = 0;
        while start > needed && self.entries.len() - start < budget.max_events {
            let size = self.entries[start - 1].frame.size;
            if size > budget.max_bytes.saturating_sub(bytes) {
                break;
            }
            bytes += size;
            start -= 1;
        }
        let replay = self.entries[start..].to_vec();
        if cursor.saturating_add(1) >= oldest && start == needed {
            ReplayPlan::Covered { replay }
        } else {
            ReplayPlan::Evicted {
                oldest,
                next_seq: self.next_seq,
                replay,
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRegistration {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub tab_id: Option<String>,
    #[serde(default)]
    pub once: bool,
    #[serde(default)]
    pub payload_extra: Option<serde_json::Value>,
}

/// In-process bus depth. A subscriber that falls this far behind lags and
/// must recover its own state rather than trust the frames it missed.
pub const SYSTEM_BUS_CAPACITY: usize = 256;

pub struct EventBroadcaster {
    webhooks: Arc<Mutex<HashMap<String, Arc<WebhookRegistration>>>>,
    system_bus: broadcast::Sender<SeqEvent>,
    boot_id: String,
    ring: Mutex<ReplayRing>,
    delivery: webhooks::DeliveryOwner,
}

impl EventBroadcaster {
    pub fn new() -> Self {
        let (system_bus, _) = broadcast::channel::<SeqEvent>(SYSTEM_BUS_CAPACITY);
        let boot_id = Uuid::new_v4().simple().to_string();
        Self {
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            delivery: webhooks::DeliveryOwner::default(),
            system_bus,
            boot_id: boot_id.clone(),
            ring: Mutex::new(ReplayRing {
                entries: VecDeque::new(),
                next_seq: 1,
                retained_bytes: 0,
                retained_bytes_high_water: 0,
                encoded_events: 0,
                #[cfg(test)]
                boot_id,
            }),
        }
    }

    /// This boot's cursor namespace; a change (daemon restart) forces a client
    /// full-resync via the `boot-changed` overflow frame.
    pub fn boot_id(&self) -> &str {
        &self.boot_id
    }

    /// The seq the next publish will take — the overflow-recovery baseline.
    pub fn current_seq(&self) -> u64 {
        self.ring.lock().next_seq
    }

    /// Snapshot the replay ring. MUST be called AFTER `subscribe()` so the
    /// live receiver already covers anything published past this point (the
    /// SSE handler dedupes the overlap by `seq`).
    pub fn snapshot_ring(&self) -> RingSnapshot {
        let ring = self.ring.lock();
        RingSnapshot {
            boot_id: self.boot_id.clone(),
            entries: ring
                .entries
                .iter()
                .map(|e| {
                    debug_assert_eq!(e.seq, e.event.seq);
                    Arc::clone(&e.event)
                })
                .collect(),
            next_seq: ring.next_seq,
        }
    }

    pub fn replay_stats(&self) -> ReplayStats {
        let ring = self.ring.lock();
        ReplayStats {
            retained_bytes: ring.retained_bytes,
            retained_events: ring.entries.len(),
            retained_bytes_high_water: ring.retained_bytes_high_water,
            encoded_events: ring.encoded_events,
            published_events: ring.next_seq - 1,
            oversize_singleton: ring.entries.len() == 1 && ring.retained_bytes > RING_MAX_BYTES,
            webhook_refusals: self.delivery.refused(),
        }
    }

    /// Publish a typed system event. Fans out to in-process subscribers
    /// (plugins, Tauri bridge) AND to matching webhooks via the JSON path.
    pub fn publish(&self, event: SystemEvent) {
        let prepared = PreparedEvent::new(event, &self.boot_id);
        let (event_type, data) = prepared.webhook_data();
        self.commit_prepared(prepared, true);
        self.broadcast(event_type, data);
    }

    /// Publish a string-named frontend event onto the in-process bus so web
    /// clients see it. The generic counterpart of `publish()`; `emit_everywhere`
    /// routes through here after firing the native `app.emit`.
    pub fn publish_generic(&self, event_type: &str, data: serde_json::Value) {
        self.send_bus(SystemEvent::Generic {
            event_type: event_type.to_string(),
            data,
        });
    }

    /// Service-only signals share ordering and replay, without exposing native payloads to SSE or webhooks.
    pub fn publish_internal(&self, event_type: &str, data: serde_json::Value) {
        self.send_bus_with_visibility(
            SystemEvent::Generic {
                event_type: event_type.to_owned(),
                data,
            },
            false,
        );
    }

    /// The single bus fan-out choke point — every published event passes here.
    /// Allocating the D1 cursor and publishing it are ONE critical section, so
    /// delivery order == seq order; the subscriber's `seq <= replayed_max`
    /// guard silently drops an out-of-order arrival, which ring order alone
    /// does not prevent. `ring` stays a leaf and `broadcast::send` is
    /// synchronous and non-blocking, so the guard never spans an `.await`.
    fn send_bus(&self, event: SystemEvent) {
        self.send_bus_with_visibility(event, true);
    }

    fn send_bus_with_visibility(&self, event: SystemEvent, public: bool) {
        self.prepare_and_send(event, public, PreparedEvent::new);
    }

    fn prepare_and_send(
        &self,
        event: SystemEvent,
        public: bool,
        prepare: impl FnOnce(SystemEvent, &str) -> PreparedEvent,
    ) {
        let prepared = prepare(event, &self.boot_id);
        self.commit_prepared(prepared, public);
    }

    fn commit_prepared(&self, prepared: PreparedEvent, public: bool) {
        let retired = {
            let mut ring = self.ring.lock();
            let (event, retired) = ring.append(prepared, Instant::now(), public);
            let _ = self.system_bus.send(event);
            retired
        };
        drop(retired);
    }

    /// Subscribe to the typed in-process event bus. Each subscriber gets
    /// its own receiver; slow subscribers lag and must handle `Lagged`.
    pub fn subscribe(&self) -> broadcast::Receiver<SeqEvent> {
        self.system_bus.subscribe()
    }

    pub fn subscribe_from_now(&self) -> (broadcast::Receiver<SeqEvent>, u64) {
        let ring = self.ring.lock();
        (self.system_bus.subscribe(), ring.next_seq.saturating_sub(1))
    }

    pub fn broadcast(&self, event_type: &str, data: serde_json::Value) {
        self.delivery.broadcast(&self.webhooks, event_type, data);
    }

    pub fn register_webhook(&self, reg: WebhookRegistration) -> Result<String, &'static str> {
        if encoding::bounded_json(&reg, webhooks::MAX_REGISTRATION_BYTES).is_none() {
            return Err("webhook registration exceeds payload limit");
        }
        let mut hooks = self.webhooks.lock();
        if !hooks.contains_key(&reg.id) && hooks.len() >= webhooks::MAX_WEBHOOKS {
            return Err("webhook registration capacity reached");
        }
        let id = reg.id.clone();
        hooks.insert(id.clone(), Arc::new(reg));
        Ok(id)
    }

    pub fn unregister_webhook(&self, id: &str) {
        self.webhooks.lock().remove(id);
    }

    pub fn list_webhooks(&self) -> Vec<WebhookRegistration> {
        self.webhooks
            .lock()
            .values()
            .map(|hook| hook.as_ref().clone())
            .collect()
    }
}

pub type EventBroadcasterState = Arc<EventBroadcaster>;

#[cfg(test)]
mod tests;

pub fn web_safe_event_payload(event_type: &str, mut value: serde_json::Value) -> serde_json::Value {
    if event_type == "config-changed" {
        if let Some(api) = value
            .get_mut("api")
            .and_then(serde_json::Value::as_object_mut)
        {
            api.insert("token".into(), serde_json::Value::Null);
        }
    }
    value
}

#[cfg(test)]
mod web_safe_tests;

impl codeterm_plugin::host_traits::PluginEventSink for EventBroadcaster {
    fn contributions_changed(&self, kind: &str) {
        self.publish(SystemEvent::ContributionsChanged {
            kind: kind.to_string(),
        });
    }
}
