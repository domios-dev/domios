use serde::Serialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use codeterm_types::notification::{AnswerSlot, NotificationAction};

use super::answer::{self, AnswerAdmission};
use super::answer_delivery::{self, AnswerDeliveryPlan};
use super::authority::SurfaceOffer;
use super::channel_ask;
use super::channel_placement::{self, ChannelPlacement, SpentQuestion};
use super::dedupe::{
    ConditionLedger, ConditionTransition, DeliveryAdmission, DeliveryLedger, NotificationIdentity,
};
use super::deep_link::NotificationTarget;
use super::push::NotifyLevel;

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: String,
    pub tab_id: Option<String>,
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_name: Option<String>,
    /// Navigation hint only: the tab whose chat holds the message this is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_message_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub severity: NotificationSeverity,
    /// The axis a caller may name explicitly, independent of `severity`. Absent
    /// is a legal, distinct state, never defaulted from `severity` here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<NotifyLevel>,
    pub timestamp: u64,
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<NotificationAction>>,
    /// Opened when the record offers a choice. Carries the asking tab captured
    /// at send time and the one answer it may ever hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<AnswerSlot>,
    /// The surface this question offers to turn on, and the one label that
    /// turns it on. Daemon-constructed, and read only when an answer is
    /// recorded — an untapped offer changes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_offer: Option<SurfaceOffer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticky: Option<bool>,
    /// Set when the push sink refused the record. Never surfaced to the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_failed: Option<bool>,
    /// Where each channel put the message that carried this record, in that
    /// channel's own terms. Written by the channel, read by the same channel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<ChannelPlacement>,
}

/// Daemon-owned origin of a notification. Every field is resolved against live
/// daemon state; nothing here may be stored straight from a caller payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NotificationProvenance {
    pub tab_id: Option<String>,
    pub tab_title: Option<String>,
    pub workspace_id: Option<String>,
    pub peer_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub enum NotificationSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DedupAdmission {
    Added(String),
    Duplicate(String),
    Coalesced(String),
}

/// Outcome of push admission. Only `Deliver` and `Supersede` may reach a sink;
/// every variant still leaves the caller with a durable record id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushAdmission {
    Deliver(String),
    /// Same event key while the record is still open: no second record, no push.
    Duplicate(String),
    /// Novel event key: the open record is rewritten in place and pushed once.
    Supersede(String),
    /// Lifetime ceiling reached: the record still lands, the push does not.
    Suppressed(String),
}

impl PushAdmission {
    pub fn id(&self) -> &str {
        match self {
            Self::Deliver(id)
            | Self::Duplicate(id)
            | Self::Supersede(id)
            | Self::Suppressed(id) => id,
        }
    }

    pub fn should_push(&self) -> bool {
        matches!(self, Self::Deliver(_) | Self::Supersede(_))
    }

    /// A duplicate reuses an existing open record, so no create event is due.
    pub fn opened_or_changed_record(&self) -> bool {
        !matches!(self, Self::Duplicate(_))
    }
}

/// What one notification asks: the buttons it offers, and — when it is an
/// offer to be louder — the surface a tap would turn on. One value, so the
/// choice set and the offer that rides on it cannot be stored apart.
#[derive(Debug, Clone, Default)]
pub struct Ask {
    pub actions: Option<Vec<NotificationAction>>,
    pub offer: Option<SurfaceOffer>,
}

impl Ask {
    pub fn offering(actions: Option<Vec<NotificationAction>>) -> Self {
        Self {
            actions,
            offer: None,
        }
    }
}

/// A push-eligible request. `scope` and `event_key` are daemon-derived; the
/// caller cannot opt out of either.
pub struct PushRequest {
    pub scope: String,
    pub level: NotifyLevel,
    pub event_key: String,
    pub title: String,
    pub body: Option<String>,
    pub severity: NotificationSeverity,
    pub provenance: NotificationProvenance,
    pub per_scope_limit: u32,
    /// A levelled notification carries its question too: an escalation that
    /// asks must be answerable on the channel it reached.
    pub ask: Ask,
}

struct PushSlot {
    event_key: String,
    notification_id: String,
}

pub struct NotificationStore {
    notifications: Vec<Notification>,
    max_notifications: usize,
    /// One open coalescing slot per (scope, level). Closed on read, clear,
    /// eviction or tab close — never by elapsed time.
    push_slots: HashMap<(String, NotifyLevel), PushSlot>,
    /// Lifetime push count per scope. A count, so no clock can advance it.
    push_counts: HashMap<String, u32>,
    /// Live identities of unread records: the same message twice opens one.
    deliveries: DeliveryLedger,
    /// Standing conditions already announced, so re-observation stays quiet.
    conditions: ConditionLedger,
}

impl NotificationStore {
    pub fn new(max: usize) -> Self {
        Self {
            notifications: Vec::new(),
            max_notifications: max,
            push_slots: HashMap::new(),
            push_counts: HashMap::new(),
            deliveries: DeliveryLedger::default(),
            conditions: ConditionLedger::default(),
        }
    }

    /// Add a notification. Returns the notification ID.
    pub fn add(
        &mut self,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        tab_id: Option<String>,
        workspace_id: Option<String>,
    ) -> String {
        self.add_ext(
            title,
            body,
            severity,
            NotificationProvenance {
                tab_id,
                workspace_id,
                ..Default::default()
            },
            Ask::default(),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_ext(
        &mut self,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        provenance: NotificationProvenance,
        ask: Ask,
        sticky: Option<bool>,
        level: Option<NotifyLevel>,
    ) -> String {
        let Ask { actions, offer } = ask;
        let id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Send-time capture: the asking tab is read from the provenance this
        // record is being created with, never resolved when an answer is tapped.
        let answer = answer::slot_for(actions.as_deref(), provenance.tab_id.as_deref());
        let notification = Notification {
            id: id.clone(),
            tab_id: provenance.tab_id,
            workspace_id: provenance.workspace_id,
            tab_title: provenance.tab_title,
            peer_name: provenance.peer_name,
            target_tab_id: None,
            chat_message_id: None,
            title,
            body,
            severity,
            level,
            timestamp,
            read: false,
            actions,
            answer,
            surface_offer: offer,
            sticky,
            push_failed: None,
            placements: Vec::new(),
        };

        self.notifications.push(notification);

        while self.notifications.len() > self.max_notifications {
            let evicted = self.notifications.remove(0);
            self.close_push_slot_for(&evicted.id);
            self.deliveries.release(&evicted.id);
        }

        id
    }

    /// Add a notification unless an identical one is already live. Identity is
    /// derived here, so no caller can opt out of it.
    #[allow(clippy::too_many_arguments)]
    pub fn add_deduped(
        &mut self,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        provenance: NotificationProvenance,
        ask: Ask,
        sticky: Option<bool>,
        level: Option<NotifyLevel>,
    ) -> DeliveryAdmission {
        let identity = NotificationIdentity::of(&provenance, &severity, &title, body.as_deref());
        if let Some(id) = self.deliveries.existing(&identity) {
            return DeliveryAdmission::Duplicate(id);
        }
        let id = self.add_ext(title, body, severity, provenance, ask, sticky, level);
        self.deliveries.bind(identity, id.clone());
        DeliveryAdmission::Deliver(id)
    }

    /// Observe a condition that stays true. Only the absent -> present edge is
    /// news; every later observation of the same condition is the same news.
    pub fn observe_condition_present(
        &mut self,
        scope: &str,
        condition: &str,
    ) -> ConditionTransition {
        self.conditions.observe_present(scope, condition)
    }

    /// The condition resolved, so a later recurrence is a fresh transition.
    pub fn observe_condition_absent(&mut self, scope: &str, condition: &str) {
        self.conditions.observe_absent(scope, condition);
    }

    /// Admit a push-eligible notification. Dedup and coalescing are decided
    /// here so no caller can opt out of them.
    pub fn add_push(&mut self, request: PushRequest) -> PushAdmission {
        let slot_key = (request.scope.clone(), request.level);
        let open = self.push_slots.get(&slot_key).map(|slot| {
            (
                slot.event_key == request.event_key,
                slot.notification_id.clone(),
            )
        });

        if let Some((true, id)) = &open {
            return PushAdmission::Duplicate(id.clone());
        }

        let PushRequest {
            scope,
            level,
            event_key,
            title,
            body,
            severity,
            provenance,
            per_scope_limit,
            ask,
        } = request;

        // Questions cannot be coalesced because rewriting one under the user's finger could make a tap answer a different record.
        let reused = open.and_then(|(_, id)| {
            (!self.offers_choice(&id)
                && self.rewrite_in_place(
                    &id,
                    title.clone(),
                    body.clone(),
                    severity.clone(),
                    &provenance,
                    level,
                ))
            .then_some(id)
        });
        let is_supersede = reused.is_some();
        let id = match reused {
            Some(id) => id,
            None => self.add_ext(title, body, severity, provenance, ask, None, Some(level)),
        };

        self.push_slots.insert(
            slot_key,
            PushSlot {
                event_key,
                notification_id: id.clone(),
            },
        );

        let count = self.push_counts.entry(scope).or_insert(0);
        *count = count.saturating_add(1);
        if *count > per_scope_limit {
            return PushAdmission::Suppressed(id);
        }
        if is_supersede {
            PushAdmission::Supersede(id)
        } else {
            PushAdmission::Deliver(id)
        }
    }

    /// Content-only rewrite: the coalesced record keeps its id, position and
    /// opening timestamp so it reads as one event, not a new one.
    #[allow(clippy::too_many_arguments)]
    fn rewrite_in_place(
        &mut self,
        id: &str,
        title: String,
        body: Option<String>,
        severity: NotificationSeverity,
        provenance: &NotificationProvenance,
        level: NotifyLevel,
    ) -> bool {
        let Some(target) = self.notifications.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        target.title = title;
        target.body = body;
        target.severity = severity;
        target.level = Some(level);
        target.tab_id = provenance.tab_id.clone();
        target.tab_title = provenance.tab_title.clone();
        target.workspace_id = provenance.workspace_id.clone();
        target.peer_name = provenance.peer_name.clone();
        target.read = false;
        target.push_failed = None;
        true
    }

    /// Record that the push sink refused this notification. The caller's id is
    /// unaffected; delivery failure never travels back to the agent.
    pub fn mark_push_failed(&mut self, id: &str) -> bool {
        match self.notifications.iter_mut().find(|n| n.id == id) {
            Some(target) => {
                target.push_failed = Some(true);
                true
            }
            None => false,
        }
    }

    /// Close every coalescing boundary owned by a closing tab. Tab ids are not
    /// reused, so dropping the counter cannot reset a live scope's ceiling.
    pub fn close_push_scope(&mut self, scope: &str) {
        self.push_slots
            .retain(|(slot_scope, _), _| slot_scope != scope);
        self.push_counts.remove(scope);
        self.deliveries.release_scope(scope);
        self.conditions.close_scope(scope);
    }

    fn close_push_slot_for(&mut self, id: &str) {
        self.push_slots.retain(|_, slot| slot.notification_id != id);
    }

    /// Point this record at the message the latest send named. Always a full
    /// overwrite, so a rewritten record never keeps a stale jump target.
    pub fn set_target(&mut self, id: &str, target: NotificationTarget) -> bool {
        let Some(record) = self.notifications.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        record.target_tab_id = target.tab_id;
        record.chat_message_id = target.chat_message_id;
        true
    }

    pub fn notification(&self, id: &str) -> Option<&Notification> {
        self.notifications
            .iter()
            .find(|notification| notification.id == id)
    }

    pub fn list(&self, unread_only: bool) -> Vec<&Notification> {
        if unread_only {
            self.notifications.iter().filter(|n| !n.read).collect()
        } else {
            self.notifications.iter().collect()
        }
    }

    pub fn mark_read(&mut self, id: &str) -> bool {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.read = true;
            self.close_push_slot_for(id);
            self.deliveries.release(id);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self, id: Option<&str>) {
        match id {
            Some(id) => {
                self.notifications.retain(|n| n.id != id);
                self.close_push_slot_for(id);
                self.deliveries.release(id);
            }
            None => {
                self.notifications.clear();
                self.push_slots.clear();
                self.deliveries.clear();
            }
        }
    }

    /// The one answer this notification may hold, and the tab it was asked
    /// from. Read-only: recording goes through `record_answer`.
    pub fn answer_slot(&self, id: &str) -> Option<&AnswerSlot> {
        self.notifications
            .iter()
            .find(|n| n.id == id)?
            .answer
            .as_ref()
    }

    /// Whether this record still stands as a question.
    pub fn offers_choice(&self, id: &str) -> bool {
        self.notifications
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.actions.as_deref())
            .is_some_and(|actions| !channel_ask::offered_labels(actions).is_empty())
    }

    /// The label at one offered position of this record. A tapped token names
    /// a position, and only the record decides which labels it ever offered —
    /// so a token cannot name a label this notification never carried.
    pub fn offered_choice(&self, id: &str, index: usize) -> Option<String> {
        let actions = self
            .notifications
            .iter()
            .find(|n| n.id == id)?
            .actions
            .as_deref()?;
        channel_ask::label_at(actions, index).map(str::to_string)
    }

    /// The options a channel carries for this record, each with the opaque
    /// token that names its position back to this very store.
    pub fn offered_choices_for_channel(&self, id: &str) -> Vec<channel_ask::ChannelChoice> {
        match self
            .notifications
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.actions.as_deref())
        {
            Some(actions) => channel_ask::choices_for(id, actions),
            None => Vec::new(),
        }
    }

    /// The surface this record offers to turn on, if it is an offer at all.
    pub fn surface_offer(&self, id: &str) -> Option<SurfaceOffer> {
        self.notifications
            .iter()
            .find(|n| n.id == id)?
            .surface_offer
            .clone()
    }

    /// The level the caller declared for this record, if any. Read-only: the
    /// emitted payload reads this back so it never drifts from what is stored.
    pub fn level(&self, id: &str) -> Option<NotifyLevel> {
        self.notifications.iter().find(|n| n.id == id)?.level
    }

    /// Record the user's choice. The invariant lives here, not in any surface:
    /// the same question renders in more than one place, and a per-surface
    /// guard would let it take two answers.
    pub fn record_answer(&mut self, id: &str, label: &str) -> AnswerAdmission {
        answer::record(&mut self.notifications, id, label)
    }

    /// What one record owes the tab that asked. Read-only: the send itself
    /// happens outside the guard, through the existing envelope path.
    pub fn answer_delivery_plan(&self, id: &str) -> AnswerDeliveryPlan {
        answer_delivery::plan(&self.notifications, id)
    }

    /// The standing answer reached the asking tab.
    pub fn mark_answer_delivered(&mut self, id: &str) -> bool {
        answer_delivery::mark_delivered(&mut self.notifications, id)
    }

    /// A channel reported where it put this record's message. Stored verbatim:
    /// a locator means whatever the channel that wrote it means by it.
    pub fn remember_channel_placement(&mut self, id: &str, service: &str, locator: &str) -> bool {
        channel_placement::remember(&mut self.notifications, id, service, locator)
    }

    pub fn channel_placements(&self, id: &str) -> Vec<ChannelPlacement> {
        channel_placement::placements(&self.notifications, id)
    }

    /// What this record owes the channels that carried it once it holds an
    /// answer. Read-only: the rewriting happens outside the guard.
    pub fn spent_question(&self, id: &str) -> Option<SpentQuestion> {
        channel_placement::spent_question(&self.notifications, id)
    }

    /// The tab that asked has closed. Its unanswered questions become visibly
    /// undeliverable; answers already recorded stay exactly as they were.
    pub fn forget_answer_tab(&mut self, tab_id: &str) {
        answer::forget_tab(&mut self.notifications, tab_id);
    }

    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }
}

#[cfg(test)]
mod tests;
