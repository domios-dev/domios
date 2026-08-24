//! Carrying a recorded answer back to the tab that asked.
//!
//! This module decides *whether* an answer is owed and *what* it says. It owns
//! no delivery mechanism: the send is handed to the existing message envelope
//! and submit path, so the receiving agent reads a frame it already recognises
//! and nothing here composes a marker of its own.

use crate::service::messenger::MessageEnvelope;

use super::store::Notification;
use super::NotifState;

/// The context tag the host envelope carries for an answer. One constant, so no
/// surface restates it and nothing hand-writes a frame.
pub const ANSWER_CONTEXT: &str = "notification-answer";

/// One standing answer and the tab it is owed to, both read from the record.
/// Nothing here is resolved: the tab was captured when the question was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerDelivery {
    pub notification_id: String,
    pub to_tab: String,
    pub label: String,
    pub question: String,
}

impl AnswerDelivery {
    /// The existing host envelope. The marker is composed by the envelope, so
    /// an answer arrives in the same frame every other host event already uses.
    pub fn envelope(&self) -> MessageEnvelope {
        MessageEnvelope::codeterm_reminder(ANSWER_CONTEXT)
    }

    /// Names the question and the record, so a tab that asked more than one
    /// thing can tell which answer this is.
    pub fn body(&self) -> String {
        format!(
            "Answer to your question \"{}\" (notification {}): {}",
            self.question.trim(),
            self.notification_id,
            self.label
        )
    }
}

/// What one record owes right now. Exactly one variant sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerDeliveryPlan {
    /// One answer stands, undelivered, with a tab recorded to carry it to.
    Owed(AnswerDelivery),
    /// The standing answer already reached the tab; a retry sends nothing.
    AlreadyDelivered,
    /// No answer stands, or no tab is recorded to carry one to.
    Nothing,
}

/// How an attempt ended. `Failed` leaves the answer standing and undelivered —
/// never answered-and-delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerDeliveryOutcome {
    Delivered,
    AlreadyDelivered,
    /// Nothing was owed, so nothing was attempted.
    NotAttempted,
    /// The envelope path refused it, with the reason it gave.
    Failed(String),
}

impl AnswerDeliveryOutcome {
    /// Whether the standing answer is in the asking tab's hands.
    pub fn reached_tab(&self) -> bool {
        matches!(self, Self::Delivered | Self::AlreadyDelivered)
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(detail) => Some(detail),
            _ => None,
        }
    }
}

pub fn plan(notifications: &[Notification], id: &str) -> AnswerDeliveryPlan {
    let Some(target) = notifications.iter().find(|n| n.id == id) else {
        return AnswerDeliveryPlan::Nothing;
    };
    let Some(slot) = target.answer.as_ref() else {
        return AnswerDeliveryPlan::Nothing;
    };
    let Some(label) = slot.answered.as_deref() else {
        return AnswerDeliveryPlan::Nothing;
    };
    if slot.delivered {
        return AnswerDeliveryPlan::AlreadyDelivered;
    }
    let Some(to_tab) = slot.asked_from_tab.as_deref() else {
        return AnswerDeliveryPlan::Nothing;
    };
    AnswerDeliveryPlan::Owed(AnswerDelivery {
        notification_id: target.id.clone(),
        to_tab: to_tab.to_string(),
        label: label.to_string(),
        question: target.title.clone(),
    })
}

/// The standing answer reached the asking tab, so the next attempt finds
/// nothing to send. Only a record that holds an answer can be marked.
pub fn mark_delivered(notifications: &mut [Notification], id: &str) -> bool {
    let Some(target) = notifications.iter_mut().find(|n| n.id == id) else {
        return false;
    };
    let Some(slot) = target.answer.as_mut() else {
        return false;
    };
    if slot.answered.is_none() {
        return false;
    }
    slot.delivered = true;
    true
}

/// Deliver whatever the record owes, at most once. `send` is the messenger
/// boundary — it receives exactly the target tab, the envelope, and the body.
/// The store guard is taken twice and never held across it, because the send
/// reaches the multiplexer.
pub fn deliver(
    store: &NotifState,
    id: &str,
    send: impl FnOnce(&str, MessageEnvelope, &str) -> Result<(), String>,
) -> AnswerDeliveryOutcome {
    let due = { store.lock().answer_delivery_plan(id) };
    let plan = match due {
        AnswerDeliveryPlan::Owed(plan) => plan,
        AnswerDeliveryPlan::AlreadyDelivered => return AnswerDeliveryOutcome::AlreadyDelivered,
        AnswerDeliveryPlan::Nothing => return AnswerDeliveryOutcome::NotAttempted,
    };
    match send(&plan.to_tab, plan.envelope(), &plan.body()) {
        Ok(()) => {
            store.lock().mark_answer_delivered(id);
            AnswerDeliveryOutcome::Delivered
        }
        Err(detail) => AnswerDeliveryOutcome::Failed(detail),
    }
}

#[cfg(test)]
mod tests;
