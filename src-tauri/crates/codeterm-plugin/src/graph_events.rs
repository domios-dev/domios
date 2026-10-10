use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

use crate::event_source::EventSourceProvider;
use crate::js_runtime::manifest::NodeSchema;

pub const MAX_SUBSCRIPTIONS: usize = 1024;
pub const MAX_PENDING_PER_SUBSCRIPTION: usize = 64;
pub const MAX_PENDING_GLOBAL: usize = 4096;
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
pub const MAX_SUBSCRIPTION_TOKEN_BYTES: usize = 256;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_DEDUP_IDS: usize = 256;
pub const MAX_REFUSAL_JOURNAL: usize = 4096;
pub const MAX_EVENTS_PER_MINUTE: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmitRefusal {
    UnknownSubscription,
    BadToken,
    ForeignPlugin,
    InvalidPayload,
    Oversize,
    Duplicate,
    Backpressure,
    RateLimited,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventEnvelope {
    pub subscription_id: String,
    pub pipeline_id: String,
    pub kind: String,
    pub event_id: String,
    pub payload: Value,
    pub admitted_at_ms: u64,
}

#[derive(Clone, Debug)]
pub struct AdmissionRequest<'a> {
    pub subscription_id: &'a str,
    pub token: &'a str,
    pub plugin_id: &'a str,
    pub event_id: &'a str,
    pub payload: &'a Value,
    pub payload_bytes: usize,
    pub now_ms: u64,
}

#[derive(Clone, Debug)]
pub struct AdmissionSubscription<'a> {
    pub token: &'a str,
    pub plugin_id: &'a str,
    pub payload_schema: Option<&'a NodeSchema>,
    pub pending: usize,
    pub event_ids: &'a VecDeque<String>,
    pub admitted_at_ms: &'a VecDeque<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionDecision {
    Admit,
    Refuse(AdmitRefusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalOutcome {
    Refused(AdmitRefusal),
    Cancelled,
    RetentionOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub subscription_id: String,
    pub event_id: String,
    pub outcome: JournalOutcome,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSubscriptionSnapshot {
    pub state: String,
    pub delivered: u64,
    pub refused: u64,
    pub last_event_at_ms: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSubscriptionBinding {
    pub subscription_id: String,
    pub pipeline_id: String,
    pub kind: String,
    pub plugin_id: String,
    pub params_json: String,
    pub registry_generation: u64,
    pub definition_version: u64,
}

pub fn admit(
    request: &AdmissionRequest<'_>,
    subscription: Option<&AdmissionSubscription<'_>>,
    global_pending: usize,
) -> AdmissionDecision {
    if request.subscription_id.len() > MAX_ID_BYTES
        || request.plugin_id.len() > MAX_ID_BYTES
        || request.token.len() > MAX_SUBSCRIPTION_TOKEN_BYTES
        || request.event_id.len() > MAX_ID_BYTES
    {
        return AdmissionDecision::Refuse(AdmitRefusal::Oversize);
    }
    let Some(subscription) = subscription else {
        return AdmissionDecision::Refuse(AdmitRefusal::UnknownSubscription);
    };
    if !constant_time_eq(request.token.as_bytes(), subscription.token.as_bytes()) {
        return AdmissionDecision::Refuse(AdmitRefusal::BadToken);
    }
    if request.plugin_id != subscription.plugin_id {
        return AdmissionDecision::Refuse(AdmitRefusal::ForeignPlugin);
    }
    if request.payload_bytes > MAX_EVENT_BYTES
        || serde_json::to_vec(request.payload)
            .map(|payload| payload.len() > MAX_EVENT_BYTES)
            .unwrap_or(true)
        || request.event_id.is_empty()
        || request.event_id.len() > MAX_ID_BYTES
    {
        return AdmissionDecision::Refuse(AdmitRefusal::Oversize);
    }
    if subscription
        .payload_schema
        .is_some_and(|schema| schema.validate_instance(request.payload).is_err())
    {
        return AdmissionDecision::Refuse(AdmitRefusal::InvalidPayload);
    }
    if subscription
        .event_ids
        .iter()
        .any(|id| id == request.event_id)
    {
        return AdmissionDecision::Refuse(AdmitRefusal::Duplicate);
    }
    let cutoff = request.now_ms.saturating_sub(60_000);
    if subscription
        .admitted_at_ms
        .iter()
        .filter(|timestamp| **timestamp >= cutoff)
        .count()
        >= MAX_EVENTS_PER_MINUTE
    {
        return AdmissionDecision::Refuse(AdmitRefusal::RateLimited);
    }
    if subscription.pending >= MAX_PENDING_PER_SUBSCRIPTION || global_pending >= MAX_PENDING_GLOBAL
    {
        return AdmissionDecision::Refuse(AdmitRefusal::Backpressure);
    }
    AdmissionDecision::Admit
}

#[derive(Clone)]
pub struct SubscriptionTable(Arc<Inner>);

struct Inner {
    state: Mutex<State>,
}

struct State {
    subscriptions: HashMap<String, Subscription>,
    order: VecDeque<String>,
    pipeline_states: HashMap<String, EventSubscriptionSnapshot>,
    refusals: VecDeque<JournalEntry>,
    pending: usize,
}

struct Subscription {
    pipeline_id: String,
    kind: String,
    params_json: String,
    registry_generation: u64,
    definition_version: u64,
    provider: Arc<dyn EventSourceProvider>,
    token: String,
    plugin_id: String,
    payload_schema: Option<NodeSchema>,
    queue: VecDeque<EventEnvelope>,
    event_ids: VecDeque<String>,
    admitted_at_ms: VecDeque<u64>,
}

impl Default for SubscriptionTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionTable {
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                subscriptions: HashMap::new(),
                order: VecDeque::new(),
                pipeline_states: HashMap::new(),
                refusals: VecDeque::new(),
                pending: 0,
            }),
        }))
    }

    pub fn subscribe(
        &self,
        subscription_id: String,
        pipeline_id: String,
        kind: String,
        params_json: String,
        registry_generation: u64,
        definition_version: u64,
        provider: Arc<dyn EventSourceProvider>,
        token: String,
        plugin_id: String,
        payload_schema: Option<NodeSchema>,
    ) -> Result<(), String> {
        if subscription_id.is_empty()
            || subscription_id.len() > MAX_ID_BYTES
            || token.is_empty()
            || token.len() > MAX_SUBSCRIPTION_TOKEN_BYTES
            || plugin_id.is_empty()
            || plugin_id.len() > MAX_ID_BYTES
            || pipeline_id.is_empty()
            || pipeline_id.len() > MAX_ID_BYTES
            || kind.is_empty()
            || kind.len() > MAX_ID_BYTES
            || params_json.len() > MAX_EVENT_BYTES
        {
            return Err("subscription identity or token is invalid".into());
        }
        let mut state = self.0.state.lock();
        if state.subscriptions.contains_key(&subscription_id) {
            return Err("subscription id already exists".into());
        }
        if state.subscriptions.len() >= MAX_SUBSCRIPTIONS {
            return Err("subscription table is full".into());
        }
        if state.pipeline_states.len() >= MAX_SUBSCRIPTIONS
            && !state.pipeline_states.contains_key(&pipeline_id)
        {
            return Err("event subscription state table is full".into());
        }
        state
            .pipeline_states
            .entry(pipeline_id.clone())
            .or_insert_with(|| EventSubscriptionSnapshot {
                state: "subscribing".into(),
                delivered: 0,
                refused: 0,
                last_event_at_ms: None,
                reason: None,
            });
        state.order.push_back(subscription_id.clone());
        state.subscriptions.insert(
            subscription_id,
            Subscription {
                pipeline_id,
                kind,
                params_json,
                registry_generation,
                definition_version,
                provider,
                token,
                plugin_id,
                payload_schema,
                queue: VecDeque::new(),
                event_ids: VecDeque::new(),
                admitted_at_ms: VecDeque::new(),
            },
        );
        Ok(())
    }

    pub fn mark_subscribed(&self, pipeline_id: &str) {
        if let Some(summary) = self.0.state.lock().pipeline_states.get_mut(pipeline_id) {
            summary.state = "subscribed".into();
            summary.reason = None;
        }
    }

    pub fn unsubscribe(&self, subscription_id: &str) -> bool {
        let mut state = self.0.state.lock();
        let Some(subscription) = state.subscriptions.remove(subscription_id) else {
            return false;
        };
        for event in subscription.queue.iter() {
            push_journal(
                &mut state,
                JournalEntry {
                    subscription_id: subscription_id.into(),
                    event_id: event.event_id.clone(),
                    outcome: JournalOutcome::Cancelled,
                    at_ms: event.admitted_at_ms,
                },
            );
        }
        state.pending -= subscription.queue.len();
        state.order.retain(|id| id != subscription_id);
        if !state
            .subscriptions
            .values()
            .any(|item| item.pipeline_id == subscription.pipeline_id)
        {
            if let Some(summary) = state.pipeline_states.get_mut(&subscription.pipeline_id) {
                if summary.state != "unavailable" {
                    summary.state = "stopped".into();
                }
            }
        }
        drop(state);
        if let Err(error) = subscription
            .provider
            .unsubscribe(&subscription.kind, subscription_id)
        {
            log::warn!("[graph-events] provider unsubscribe failed: {error}");
        }
        true
    }

    pub fn mark_unavailable(&self, pipeline_id: &str, reason: &str) {
        let mut state = self.0.state.lock();
        if !state.pipeline_states.contains_key(pipeline_id)
            && state.pipeline_states.len() >= MAX_SUBSCRIPTIONS
        {
            return;
        }
        let summary = state
            .pipeline_states
            .entry(pipeline_id.into())
            .or_insert_with(|| EventSubscriptionSnapshot {
                state: "unavailable".into(),
                delivered: 0,
                refused: 0,
                last_event_at_ms: None,
                reason: None,
            });
        summary.state = "unavailable".into();
        summary.reason = Some(reason.chars().take(256).collect());
    }

    pub fn mark_unavailable_once(&self, pipeline_id: &str, reason: &str) -> bool {
        let mut state = self.0.state.lock();
        if !state.pipeline_states.contains_key(pipeline_id)
            && state.pipeline_states.len() >= MAX_SUBSCRIPTIONS
        {
            return false;
        }
        let summary = state
            .pipeline_states
            .entry(pipeline_id.into())
            .or_insert_with(|| EventSubscriptionSnapshot {
                state: "unavailable".into(),
                delivered: 0,
                refused: 0,
                last_event_at_ms: None,
                reason: None,
            });
        let notify = summary.state != "unavailable";
        summary.state = "unavailable".into();
        summary.reason = Some(reason.chars().take(256).collect());
        notify
    }

    pub fn clear_pipeline(&self, pipeline_id: &str) {
        let subscription_ids = self
            .0
            .state
            .lock()
            .subscriptions
            .iter()
            .filter(|(_, subscription)| subscription.pipeline_id == pipeline_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in subscription_ids {
            self.unsubscribe(&id);
        }
        self.0.state.lock().pipeline_states.remove(pipeline_id);
    }

    pub fn summary(&self, pipeline_id: &str) -> Option<EventSubscriptionSnapshot> {
        self.0
            .state
            .lock()
            .pipeline_states
            .get(pipeline_id)
            .cloned()
    }

    pub fn pipeline_ids(&self) -> Vec<String> {
        self.0
            .state
            .lock()
            .pipeline_states
            .keys()
            .cloned()
            .collect()
    }

    pub fn subscription_ids_for(&self, pipeline_id: &str) -> Vec<String> {
        self.0
            .state
            .lock()
            .subscriptions
            .iter()
            .filter(|(_, subscription)| subscription.pipeline_id == pipeline_id)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn bindings_for(&self, pipeline_id: &str) -> Vec<EventSubscriptionBinding> {
        self.0
            .state
            .lock()
            .subscriptions
            .iter()
            .filter(|(_, subscription)| subscription.pipeline_id == pipeline_id)
            .map(|(subscription_id, subscription)| EventSubscriptionBinding {
                subscription_id: subscription_id.clone(),
                pipeline_id: subscription.pipeline_id.clone(),
                kind: subscription.kind.clone(),
                plugin_id: subscription.plugin_id.clone(),
                params_json: subscription.params_json.clone(),
                registry_generation: subscription.registry_generation,
                definition_version: subscription.definition_version,
            })
            .collect()
    }

    pub fn bindings(&self) -> Vec<EventSubscriptionBinding> {
        self.0
            .state
            .lock()
            .subscriptions
            .iter()
            .map(|(subscription_id, subscription)| EventSubscriptionBinding {
                subscription_id: subscription_id.clone(),
                pipeline_id: subscription.pipeline_id.clone(),
                kind: subscription.kind.clone(),
                plugin_id: subscription.plugin_id.clone(),
                params_json: subscription.params_json.clone(),
                registry_generation: subscription.registry_generation,
                definition_version: subscription.definition_version,
            })
            .collect()
    }

    pub fn pipeline_id_for(&self, subscription_id: &str) -> Option<String> {
        self.0
            .state
            .lock()
            .subscriptions
            .get(subscription_id)
            .map(|subscription| subscription.pipeline_id.clone())
    }

    pub fn next_for(&self, pipeline_id: &str) -> Option<EventEnvelope> {
        let state = self.0.state.lock();
        state.order.iter().find_map(|id| {
            let subscription = state.subscriptions.get(id)?;
            if subscription.pipeline_id == pipeline_id {
                subscription.queue.front().cloned()
            } else {
                None
            }
        })
    }

    pub fn consume(&self, subscription_id: &str, event_id: &str) -> Option<EventEnvelope> {
        let mut state = self.0.state.lock();
        let subscription = state.subscriptions.get_mut(subscription_id)?;
        if subscription.queue.front()?.event_id != event_id {
            return None;
        }
        let event = subscription.queue.pop_front()?;
        if let Some(summary) = state.pipeline_states.get_mut(&event.pipeline_id) {
            summary.delivered = summary.delivered.saturating_add(1);
            summary.last_event_at_ms = Some(event.admitted_at_ms);
        }
        state.pending -= 1;
        Some(event)
    }

    pub fn admit(&self, request: AdmissionRequest<'_>) -> AdmissionDecision {
        let mut state = self.0.state.lock();
        let decision = state
            .subscriptions
            .get(request.subscription_id)
            .map(|subscription| AdmissionSubscription {
                token: &subscription.token,
                plugin_id: &subscription.plugin_id,
                payload_schema: subscription.payload_schema.as_ref(),
                pending: subscription.queue.len(),
                event_ids: &subscription.event_ids,
                admitted_at_ms: &subscription.admitted_at_ms,
            });
        let decision = admit(&request, decision.as_ref(), state.pending);
        match decision {
            AdmissionDecision::Admit => {
                let Some(subscription) = state.subscriptions.get_mut(request.subscription_id)
                else {
                    return AdmissionDecision::Refuse(AdmitRefusal::UnknownSubscription);
                };
                subscription.queue.push_back(EventEnvelope {
                    subscription_id: request.subscription_id.into(),
                    pipeline_id: subscription.pipeline_id.clone(),
                    kind: subscription.kind.clone(),
                    event_id: request.event_id.into(),
                    payload: request.payload.clone(),
                    admitted_at_ms: request.now_ms,
                });
                subscription.event_ids.push_back(request.event_id.into());
                if subscription.event_ids.len() > MAX_DEDUP_IDS {
                    subscription.event_ids.pop_front();
                }
                if subscription.admitted_at_ms.len() == MAX_EVENTS_PER_MINUTE {
                    subscription.admitted_at_ms.pop_front();
                }
                subscription.admitted_at_ms.push_back(request.now_ms);
                while subscription
                    .admitted_at_ms
                    .front()
                    .is_some_and(|timestamp| *timestamp < request.now_ms.saturating_sub(60_000))
                {
                    subscription.admitted_at_ms.pop_front();
                }
                state.pending += 1;
            }
            AdmissionDecision::Refuse(reason) => {
                let pipeline_id = state
                    .subscriptions
                    .get(request.subscription_id)
                    .map(|subscription| subscription.pipeline_id.clone());
                if let Some(summary) = pipeline_id
                    .as_deref()
                    .and_then(|id| state.pipeline_states.get_mut(id))
                {
                    summary.refused = summary.refused.saturating_add(1);
                    summary.reason = Some(refusal_name(reason).into());
                }
                push_journal(
                    &mut state,
                    JournalEntry {
                        subscription_id: request.subscription_id.into(),
                        event_id: request.event_id.into(),
                        outcome: JournalOutcome::Refused(reason),
                        at_ms: request.now_ms,
                    },
                );
            }
        }
        decision
    }

    pub fn admission_journal(&self) -> Vec<JournalEntry> {
        self.0.state.lock().refusals.iter().cloned().collect()
    }

    pub fn pending_len(&self) -> usize {
        self.0.state.lock().pending
    }
}

fn push_journal(state: &mut State, entry: JournalEntry) {
    if state.refusals.len() == MAX_REFUSAL_JOURNAL {
        state.refusals.pop_front();
        state.refusals.pop_front();
        state.refusals.push_back(JournalEntry {
            subscription_id: String::new(),
            event_id: String::new(),
            outcome: JournalOutcome::RetentionOverflow,
            at_ms: entry.at_ms,
        });
    }
    state.refusals.push_back(JournalEntry {
        subscription_id: bounded_id(&entry.subscription_id),
        event_id: bounded_id(&entry.event_id),
        outcome: entry.outcome,
        at_ms: entry.at_ms,
    });
}

fn refusal_name(reason: AdmitRefusal) -> &'static str {
    match reason {
        AdmitRefusal::UnknownSubscription => "unknown_subscription",
        AdmitRefusal::BadToken => "bad_token",
        AdmitRefusal::ForeignPlugin => "foreign_plugin",
        AdmitRefusal::InvalidPayload => "invalid_payload",
        AdmitRefusal::Oversize => "oversize",
        AdmitRefusal::Duplicate => "duplicate",
        AdmitRefusal::Backpressure => "backpressure",
        AdmitRefusal::RateLimited => "rate_limited",
    }
}

fn bounded_id(value: &str) -> String {
    value.chars().take(MAX_ID_BYTES).collect()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    difference == 0
}

#[cfg(test)]
mod tests;
