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
}

#[derive(Debug)]
pub struct EncodedFrame {
    pub seq: u64,
    pub event_type: String,
    pub bytes: Arc<str>,
    pub size: usize,
}

impl EncodedFrame {
    fn new(seq: u64, boot: &str, event: &SystemEvent) -> Self {
        let (event_type, data) = match event {
            SystemEvent::Generic { event_type, data } => (event_type.clone(), data.clone()),
            other => (
                other.type_name().to_string(),
                serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
            ),
        };
        let bytes: Arc<str> = serde_json::json!({
            "seq": seq,
            "boot": boot,
            "event_type": event_type,
            "data": data,
        })
        .to_string()
        .into();
        let size = bytes.len();
        Self {
            seq,
            event_type,
            bytes,
            size,
        }
    }
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
    boot_id: String,
}

impl ReplayRing {
    fn push(&mut self, event: SystemEvent, now: Instant) -> SeqEvent {
        let seq = self.next_seq;
        self.next_seq = seq + 1;
        let frame = Arc::new(EncodedFrame::new(seq, &self.boot_id, &event));
        self.encoded_events += 1;
        self.retained_bytes += frame.size;
        let event = SeqEvent { seq, event, frame };
        self.entries.push_back(RingEntry {
            seq,
            at: now,
            event: Arc::new(event.clone()),
        });
        while self.entries.len() > RING_MAX_COUNT
            || (self.entries.len() > 1 && self.retained_bytes > RING_MAX_BYTES)
            || self
                .entries
                .front()
                .is_some_and(|e| now.duration_since(e.at) > RING_MAX_AGE)
        {
            if let Some(entry) = self.entries.pop_front() {
                self.retained_bytes -= entry.event.frame.size;
            }
        }
        self.retained_bytes_high_water = self.retained_bytes_high_water.max(self.retained_bytes);
        event
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
    webhooks: Arc<Mutex<HashMap<String, WebhookRegistration>>>,
    system_bus: broadcast::Sender<SeqEvent>,
    boot_id: String,
    ring: Mutex<ReplayRing>,
}

impl EventBroadcaster {
    pub fn new() -> Self {
        let (system_bus, _) = broadcast::channel::<SeqEvent>(SYSTEM_BUS_CAPACITY);
        let boot_id = Uuid::new_v4().simple().to_string();
        Self {
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            system_bus,
            boot_id: boot_id.clone(),
            ring: Mutex::new(ReplayRing {
                entries: VecDeque::new(),
                next_seq: 1,
                retained_bytes: 0,
                retained_bytes_high_water: 0,
                encoded_events: 0,
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
        }
    }

    /// Publish a typed system event. Fans out to in-process subscribers
    /// (plugins, Tauri bridge) AND to matching webhooks via the JSON path.
    pub fn publish(&self, event: SystemEvent) {
        let event_type = event.type_name();
        let data = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
        self.send_bus(event);
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

    /// The single bus fan-out choke point — every published event passes here.
    /// Allocating the D1 cursor and publishing it are ONE critical section, so
    /// delivery order == seq order; the subscriber's `seq <= replayed_max`
    /// guard silently drops an out-of-order arrival, which ring order alone
    /// does not prevent. `ring` stays a leaf and `broadcast::send` is
    /// synchronous and non-blocking, so the guard never spans an `.await`.
    fn send_bus(&self, event: SystemEvent) {
        let mut ring = self.ring.lock();
        let event = ring.push(event, Instant::now());
        let _ = self.system_bus.send(event);
    }

    /// Subscribe to the typed in-process event bus. Each subscriber gets
    /// its own receiver; slow subscribers lag and must handle `Lagged`.
    pub fn subscribe(&self) -> broadcast::Receiver<SeqEvent> {
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

        let (matching, to_remove): (Vec<_>, Vec<_>) = {
            let hooks = self.webhooks.lock();
            let mut matching = Vec::new();
            let mut to_remove = Vec::new();

            for (id, hook) in hooks.iter() {
                if !hook.events.is_empty() && !hook.events.contains(&event_type.to_string()) {
                    continue;
                }
                if let Some(ref filter_tab) = hook.tab_id {
                    if let Some(event_tab) = data.get("tab_id").and_then(|v| v.as_str()) {
                        if filter_tab != event_tab {
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

        if !to_remove.is_empty() {
            let mut hooks = self.webhooks.lock();
            for id in &to_remove {
                hooks.remove(id);
            }
        }

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
        assert!(matches!(ev.event, SystemEvent::MuxStateChanged));
    }

    #[tokio::test]
    async fn publish_carries_payload() {
        let bc = EventBroadcaster::new();
        let mut rx = bc.subscribe();
        bc.publish(SystemEvent::TabTitlePinned {
            tab_id: "p1".into(),
            title: "hello".into(),
        });
        match rx.recv().await.unwrap().event {
            SystemEvent::TabTitlePinned { tab_id, title } => {
                assert_eq!(tab_id, "p1");
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
        bc.publish(SystemEvent::TabFocused { tab_id: "p".into() });
        assert!(matches!(
            rx1.recv().await.unwrap().event,
            SystemEvent::TabFocused { .. }
        ));
        assert!(matches!(
            rx2.recv().await.unwrap().event,
            SystemEvent::TabFocused { .. }
        ));
    }

    #[tokio::test]
    async fn slow_subscriber_lags_without_blocking_publisher() {
        let bc = EventBroadcaster::new();
        let mut rx = bc.subscribe();
        for i in 0..300u64 {
            bc.publish(SystemEvent::AgentIdle {
                tab_id: "p".into(),
                duration_secs: i,
            });
        }
        // Channel capacity is 256; oldest entries are dropped. First recv should signal Lagged, then continue yielding the remaining tail.
        let first = rx.recv().await;
        assert!(matches!(first, Err(broadcast::error::RecvError::Lagged(_))));
    }

    fn flood(bc: &EventBroadcaster, n: u64) {
        for i in 0..n {
            bc.send_bus(SystemEvent::AgentIdle {
                tab_id: "p".into(),
                duration_secs: i,
            });
        }
    }

    #[test]
    fn replay_from_cursor_returns_missed_in_order() {
        let bc = EventBroadcaster::new();
        flood(&bc, 5); // seqs 1..=5
        let snap = bc.snapshot_ring();
        let boot = snap.boot_id.clone();
        match snap.plan(
            Some((&boot, 2)),
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX,
            },
        ) {
            ReplayPlan::Covered { replay } => {
                let seqs: Vec<u64> = replay.iter().map(|e| e.seq).collect();
                assert_eq!(seqs, vec![3, 4, 5]);
            }
            _ => panic!("expected Covered"),
        }
    }

    #[test]
    fn covered_at_head_replays_nothing() {
        let bc = EventBroadcaster::new();
        flood(&bc, 5);
        let snap = bc.snapshot_ring();
        let boot = snap.boot_id.clone();
        match snap.plan(
            Some((&boot, 5)),
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX,
            },
        ) {
            ReplayPlan::Covered { replay } => assert!(replay.is_empty()),
            _ => panic!("expected Covered"),
        }
    }

    #[test]
    fn stale_cursor_signals_evicted() {
        let bc = EventBroadcaster::new();
        flood(&bc, RING_MAX_COUNT as u64 + 10); // oldest retained advances past 1
        let snap = bc.snapshot_ring();
        assert_eq!(snap.entries.len(), RING_MAX_COUNT);
        let oldest = snap.entries.first().unwrap().seq;
        let boot = snap.boot_id.clone();
        match snap.plan(
            Some((&boot, 0)),
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX,
            },
        ) {
            ReplayPlan::Evicted {
                oldest: o, replay, ..
            } => {
                assert_eq!(o, oldest);
                assert_eq!(replay.len(), RING_MAX_COUNT);
            }
            _ => panic!("expected Evicted"),
        }
    }

    #[test]
    fn boot_change_signals() {
        let bc = EventBroadcaster::new();
        flood(&bc, 3);
        let snap = bc.snapshot_ring();
        match snap.plan(
            Some(("some-other-boot", 1)),
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX,
            },
        ) {
            ReplayPlan::BootChanged { next_seq } => assert_eq!(next_seq, 4),
            _ => panic!("expected BootChanged"),
        }
    }

    #[test]
    fn absent_cursor_starts_from_now() {
        let bc = EventBroadcaster::new();
        flood(&bc, 3);
        let snap = bc.snapshot_ring();
        assert!(matches!(
            snap.plan(
                None,
                ReplayBudget {
                    max_bytes: usize::MAX,
                    max_events: usize::MAX
                }
            ),
            ReplayPlan::FromNow
        ));
    }

    #[test]
    fn ring_count_bound_holds_under_flood() {
        let bc = EventBroadcaster::new();
        flood(&bc, RING_MAX_COUNT as u64 * 2);
        let snap = bc.snapshot_ring();
        assert_eq!(snap.entries.len(), RING_MAX_COUNT);
        // Oldest retained = total - MAX + 1; newest = total. Gap-free seqs.
        assert_eq!(snap.entries.first().unwrap().seq, RING_MAX_COUNT as u64 + 1);
        assert_eq!(snap.entries.last().unwrap().seq, RING_MAX_COUNT as u64 * 2);
        assert_eq!(snap.next_seq, RING_MAX_COUNT as u64 * 2 + 1);
    }

    #[test]
    fn ring_age_bound_evicts_front() {
        let mut ring = ReplayRing {
            entries: VecDeque::new(),
            next_seq: 1,
            retained_bytes: 0,
            retained_bytes_high_water: 0,
            encoded_events: 0,
            boot_id: "test".into(),
        };
        let t0 = Instant::now();
        ring.push(SystemEvent::MuxStateChanged, t0);
        ring.push(SystemEvent::MuxStateChanged, t0 + Duration::from_secs(1));
        assert_eq!(ring.entries.len(), 2);
        // now = t0 + 121s; seq1 age 121s (>120) evicted, seq2 age 120s (not >120) kept.
        ring.push(
            SystemEvent::MuxStateChanged,
            t0 + RING_MAX_AGE + Duration::from_secs(1),
        );
        assert_eq!(ring.entries.len(), 2);
        assert_eq!(ring.entries.front().unwrap().seq, 2);
    }

    #[test]
    fn live_tail_after_replay_has_no_gap_or_dup() {
        let bc = EventBroadcaster::new();
        let mut rx = bc.subscribe(); // subscribe FIRST (race-free attach)
        flood(&bc, 5); // seqs 1..=5 buffered in rx AND recorded in the ring
        let snap = bc.snapshot_ring();
        let boot = snap.boot_id.clone();
        let cursor: u64 = 2;
        let replay = match snap.plan(
            Some((&boot, cursor)),
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX,
            },
        ) {
            ReplayPlan::Covered { replay } => replay,
            _ => panic!("expected Covered"),
        };
        let mut replayed_max = cursor;
        let mut seen = Vec::new();
        for se in &replay {
            assert!(se.seq > replayed_max, "replay must be strictly increasing");
            replayed_max = se.seq;
            seen.push(se.seq);
        }
        flood(&bc, 1); // a live event lands (seq 6)
                       // Live loop dedupes the snapshot/live overlap by seq.
        while let Ok(se) = rx.try_recv() {
            if se.seq <= replayed_max {
                continue;
            }
            replayed_max = se.seq;
            seen.push(se.seq);
        }
        assert_eq!(seen, vec![3, 4, 5, 6]); // exact missed set, no gap, no dup at the seam
    }

    /// Concurrency load for the ordering test. A window of
    /// PUBLISHERS * PER_PUBLISHER stays inside the bus capacity and the ring
    /// bound, so a correct run cannot lag or evict; ROUNDS repeats the race.
    const ORDER_PUBLISHERS: usize = 24;
    const ORDER_PER_PUBLISHER: usize = 10;
    const ORDER_ROUNDS: usize = 20;

    #[test]
    fn concurrent_publishers_deliver_every_seq_in_order() {
        use std::sync::Barrier;
        let total = ORDER_PUBLISHERS * ORDER_PER_PUBLISHER;
        assert!(total < SYSTEM_BUS_CAPACITY, "window must fit the bus");
        assert!(total < RING_MAX_COUNT, "window must fit the ring");
        let expected: Vec<u64> = (1..=total as u64).collect();

        for round in 0..ORDER_ROUNDS {
            let bc = Arc::new(EventBroadcaster::new());
            let mut rx = bc.subscribe();
            let gate = Arc::new(Barrier::new(ORDER_PUBLISHERS));
            let mut handles = Vec::with_capacity(ORDER_PUBLISHERS);
            for p in 0..ORDER_PUBLISHERS {
                let bc = Arc::clone(&bc);
                let gate = Arc::clone(&gate);
                handles.push(std::thread::spawn(move || {
                    gate.wait();
                    for i in 0..ORDER_PER_PUBLISHER {
                        bc.send_bus(SystemEvent::AgentIdle {
                            tab_id: format!("p{p}"),
                            duration_secs: i as u64,
                        });
                    }
                }));
            }
            for h in handles {
                h.join().expect("publisher panicked");
            }

            let mut delivered = Vec::with_capacity(total);
            loop {
                match rx.try_recv() {
                    Ok(se) => delivered.push(se.seq),
                    Err(broadcast::error::TryRecvError::Empty)
                    | Err(broadcast::error::TryRecvError::Closed) => break,
                    Err(broadcast::error::TryRecvError::Lagged(n)) => {
                        panic!("round {round}: spurious overflow, bus lagged by {n}")
                    }
                }
            }
            assert_eq!(delivered.len(), total, "round {round}: lost a publication");
            assert_eq!(
                delivered,
                expected,
                "round {round}: delivery order != seq order, first inversion at index {:?}",
                delivered
                    .iter()
                    .zip(expected.iter())
                    .position(|(a, b)| a != b)
            );
        }
    }

    /// Mirrors the `events_stream` decision table in web_routes.rs (read-only
    /// here): one overflow frame per plan classification, one per live `Lagged`.
    fn overflow_count_for(plan: &ReplayPlan) -> usize {
        match plan {
            ReplayPlan::FromNow | ReplayPlan::Covered { .. } => 0,
            ReplayPlan::BootChanged { .. } | ReplayPlan::Evicted { .. } => 1,
        }
    }

    #[test]
    fn full_ring_emits_exactly_one_overflow() {
        let bc = EventBroadcaster::new();
        flood(&bc, RING_MAX_COUNT as u64 + 64);

        let mut rx = bc.subscribe();
        let snap = bc.snapshot_ring();
        let boot = snap.boot_id.clone();
        let plan = snap.plan(
            Some((&boot, 0)),
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX,
            },
        );
        let mut overflows = overflow_count_for(&plan);
        let (mut replayed_max, replay) = match plan {
            ReplayPlan::Evicted {
                next_seq, replay, ..
            } => (next_seq.saturating_sub(1), replay),
            _ => panic!("expected Evicted for a full ring with a stale cursor"),
        };
        assert_eq!(
            overflows, 1,
            "stale cursor must produce exactly one overflow"
        );
        assert_eq!(replay.len(), RING_MAX_COUNT);

        flood(&bc, 4);
        let mut live = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(se) => {
                    if se.seq <= replayed_max {
                        continue;
                    }
                    replayed_max = se.seq;
                    live.push(se.seq);
                }
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => overflows += 1,
            }
        }
        assert_eq!(live.len(), 4, "live tail after overflow must be gap-free");
        assert_eq!(overflows, 1, "a full ring must emit exactly one overflow");
    }

    #[test]
    fn live_loop_still_drops_replay_overlap() {
        let bc = EventBroadcaster::new();
        let mut rx = bc.subscribe();
        flood(&bc, 5);
        let snap = bc.snapshot_ring();
        let boot = snap.boot_id.clone();
        let cursor = 2u64;
        let replay = match snap.plan(
            Some((&boot, cursor)),
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX,
            },
        ) {
            ReplayPlan::Covered { replay } => replay,
            _ => panic!("expected Covered"),
        };
        let mut replayed_max = cursor;
        let mut yielded = Vec::new();
        for se in &replay {
            replayed_max = se.seq;
            yielded.push(se.seq);
        }
        assert_eq!(yielded, vec![3, 4, 5]);

        flood(&bc, 1);
        let mut dropped = 0usize;
        while let Ok(se) = rx.try_recv() {
            if se.seq <= replayed_max {
                dropped += 1;
                continue;
            }
            replayed_max = se.seq;
            yielded.push(se.seq);
        }
        assert_eq!(dropped, 5, "the guard must drop the whole 1..=5 overlap");
        assert_eq!(
            yielded,
            vec![3, 4, 5, 6],
            "no duplicate at the reconnect seam"
        );
    }

    #[test]
    fn send_bus_critical_section_stays_under_lock_hold_threshold() {
        const SAMPLES: u64 = 10_000;
        let bc = EventBroadcaster::new();
        let _rx = bc.subscribe();
        let mut worst = Duration::ZERO;
        for i in 0..SAMPLES {
            let started = Instant::now();
            bc.send_bus(SystemEvent::AgentIdle {
                tab_id: "p".into(),
                duration_secs: i,
            });
            worst = worst.max(started.elapsed());
        }
        println!("send_bus worst critical section over {SAMPLES} calls: {worst:?}");
        assert!(
            worst < Duration::from_millis(50),
            "critical section crossed the [LOCK-HOLD] threshold: {worst:?}"
        );
    }

    fn large_event(bytes: usize) -> SystemEvent {
        SystemEvent::Generic {
            event_type: "large".into(),
            data: serde_json::json!({"text": "x".repeat(bytes)}),
        }
    }

    #[test]
    fn byte_eviction_is_oldest_first_and_gap_free() {
        let bc = EventBroadcaster::new();
        for _ in 0..5 {
            bc.send_bus(large_event(2 * 1024 * 1024));
            assert!(bc.replay_stats().retained_bytes <= RING_MAX_BYTES);
        }
        let snap = bc.snapshot_ring();
        assert_eq!(
            snap.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(snap.next_seq, 6);
        assert_eq!(
            bc.replay_stats().retained_bytes,
            snap.entries.iter().map(|e| e.frame.size).sum::<usize>()
        );
    }

    #[test]
    fn oversized_singleton_is_visible_and_evicted_by_next_push() {
        let bc = EventBroadcaster::new();
        bc.send_bus(large_event(RING_MAX_BYTES));
        let stats = bc.replay_stats();
        assert!(stats.oversize_singleton);
        assert!(stats.retained_bytes > RING_MAX_BYTES);
        assert_eq!(stats.retained_events, 1);
        let snap = bc.snapshot_ring();
        assert!(
            matches!(snap.plan(Some((bc.boot_id(), 0)), ReplayBudget::default()),
            ReplayPlan::Evicted { replay, .. } if replay.is_empty())
        );
        bc.send_bus(SystemEvent::MuxStateChanged);
        let stats = bc.replay_stats();
        assert!(!stats.oversize_singleton);
        assert_eq!(stats.retained_events, 1);
        assert_eq!(bc.snapshot_ring().entries[0].seq, 2);
        assert_eq!(stats.retained_bytes_high_water, snap.entries[0].frame.size);
    }

    #[test]
    fn replay_budget_exact_fit_and_newest_contiguous_suffix() {
        let bc = EventBroadcaster::new();
        flood(&bc, 3);
        let snap = bc.snapshot_ring();
        let bytes = snap.entries.iter().map(|e| e.frame.size).sum::<usize>();
        assert!(matches!(snap.plan(Some((bc.boot_id(), 0)),
            ReplayBudget { max_bytes: bytes, max_events: 3 }),
            ReplayPlan::Covered { replay } if replay.len() == 3));
        for budget in [
            ReplayBudget {
                max_bytes: bytes - 1,
                max_events: 3,
            },
            ReplayBudget {
                max_bytes: bytes,
                max_events: 2,
            },
        ] {
            match snap.plan(Some((bc.boot_id(), 0)), budget) {
                ReplayPlan::Evicted { replay, .. } => {
                    assert_eq!(replay.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![2, 3]);
                    assert!(replay.iter().map(|e| e.frame.size).sum::<usize>() <= budget.max_bytes);
                }
                _ => panic!("expected budget overflow"),
            }
        }
        assert!(
            matches!(snap.plan(Some((bc.boot_id(), u64::MAX)), ReplayBudget::default()),
            ReplayPlan::Covered { replay } if replay.is_empty())
        );
    }

    #[test]
    fn replay_default_limits_hold_for_covered_and_evicted_cursors() {
        let bc = EventBroadcaster::new();
        flood(&bc, 2100);
        let snap = bc.snapshot_ring();
        for cursor in [0, 52] {
            match snap.plan(Some((bc.boot_id(), cursor)), ReplayBudget::default()) {
                ReplayPlan::Evicted { replay, .. } => {
                    assert_eq!(replay.len(), REPLAY_MAX_EVENTS);
                    assert_eq!(replay.last().unwrap().seq, 2100);
                    assert!(replay.iter().map(|e| e.frame.size).sum::<usize>() <= REPLAY_MAX_BYTES);
                }
                _ => panic!("expected replay limit"),
            }
        }
    }

    #[test]
    fn replay_default_byte_budget_keeps_only_the_newest_fitting_events() {
        let bc = EventBroadcaster::new();
        for _ in 0..3 {
            bc.send_bus(large_event(1024 * 1024));
        }
        let snap = bc.snapshot_ring();
        match snap.plan(Some((bc.boot_id(), 0)), ReplayBudget::default()) {
            ReplayPlan::Evicted { replay, .. } => {
                assert_eq!(replay.len(), 1);
                assert_eq!(replay[0].seq, 3);
                assert!(replay[0].frame.size <= REPLAY_MAX_BYTES);
            }
            _ => panic!("expected byte budget overflow"),
        }
    }

    #[test]
    fn publish_encodes_once_and_shares_frame_with_every_subscriber_and_replay() {
        let bc = EventBroadcaster::new();
        let mut receivers: Vec<_> = (0..8).map(|_| bc.subscribe()).collect();
        bc.publish(SystemEvent::MuxStateChanged);
        let snap = bc.snapshot_ring();
        let frame = &snap.entries[0].frame;
        for rx in &mut receivers {
            let received = rx.try_recv().unwrap();
            assert!(Arc::ptr_eq(frame, &received.frame));
            assert_eq!(received.seq, frame.seq);
        }
        assert_eq!(bc.replay_stats().encoded_events, 1);
        assert_eq!(bc.replay_stats().published_events, 1);
        let next = bc.snapshot_ring();
        assert!(Arc::ptr_eq(&snap.entries[0], &next.entries[0]));
    }

    #[test]
    fn type_name_covers_all_variants() {
        assert_eq!(
            SystemEvent::MuxStateChanged.type_name(),
            "mux-state-changed"
        );
        assert_eq!(
            SystemEvent::TabTitlePinned {
                tab_id: "a".into(),
                title: "b".into()
            }
            .type_name(),
            "tab-title-pinned",
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
mod web_safe_tests {
    use super::web_safe_event_payload;

    #[test]
    fn config_events_do_not_publish_api_tokens_to_sse() {
        let payload = serde_json::json!({
            "api": { "token": "permanent-secret", "port": 7685 },
            "mesh": { "enabled": true }
        });
        let safe = web_safe_event_payload("config-changed", payload);
        assert_eq!(safe["api"]["token"], serde_json::Value::Null);
        assert_eq!(safe["api"]["port"], 7685);
    }
}

impl codeterm_plugin::host_traits::PluginEventSink for EventBroadcaster {
    fn contributions_changed(&self, kind: &str) {
        self.publish(SystemEvent::ContributionsChanged {
            kind: kind.to_string(),
        });
    }
}
