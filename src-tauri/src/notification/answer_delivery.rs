use crate::service::messenger::MessageEnvelope;

use super::channel_placement::SpentQuestion;
use super::NotifState;

pub use codeterm_notification_core::answer_delivery::{AnswerDelivery, AnswerDeliveryPlan};

pub const ANSWER_CONTEXT: &str = "notification-answer";

pub trait AnswerDeliveryEnvelope {
    fn envelope(&self) -> MessageEnvelope;
}

impl AnswerDeliveryEnvelope for AnswerDelivery {
    fn envelope(&self) -> MessageEnvelope {
        MessageEnvelope::codeterm_reminder(ANSWER_CONTEXT)
    }
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

/// The whole consequence of one recorded answer: the tab that asked receives
/// it, and every channel that showed the question stops showing it as open.
///
/// The order is the guarantee. Delivery decides the outcome and runs first;
/// `spend` runs after and returns nothing, so a channel that cannot rewrite
/// what it sent — or one whose rewrite is refused — cannot subtract from an
/// answer the user has already given.
pub fn settle(
    store: &NotifState,
    id: &str,
    send: impl FnOnce(&str, MessageEnvelope, &str) -> Result<(), String>,
    spend: impl FnOnce(&SpentQuestion),
) -> AnswerDeliveryOutcome {
    let outcome = deliver(store, id, send);
    let spent = { store.lock().spent_question(id) };
    if let Some(spent) = spent {
        spend(&spent);
    }
    outcome
}

#[cfg(test)]
mod tests;
