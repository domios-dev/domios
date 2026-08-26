//! Recording the user's choice on a notification. The asking tab is captured
//! when the question is created; answering resolves nothing.

use codeterm_types::notification::{AnswerSlot, NotificationAction};

use super::store::Notification;

/// Outcome of recording an answer. Exactly one variant mutates a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerAdmission {
    /// First and only answer: the label is now on the record.
    Recorded(String),
    /// Already answered. The standing label is returned, unchanged.
    AlreadyAnswered(String),
    /// The asking tab is gone, so nothing can carry an answer back.
    Undeliverable,
    /// No such record, no answer slot, or a label that was never offered.
    NotOffered,
}

/// The slot a new notification is born with. Built from the provenance the
/// store is already storing, so no caller can name the answering tab.
pub fn slot_for(
    actions: Option<&[NotificationAction]>,
    asking_tab: Option<&str>,
) -> Option<AnswerSlot> {
    let offers_choice = actions
        .into_iter()
        .flatten()
        .any(|action| action.answer_label().is_some());
    offers_choice.then(|| AnswerSlot::asked_from(asking_tab))
}

/// One answer per notification, decided here rather than at any surface: two
/// surfaces render the same question, and a per-surface guard would let it take
/// two answers.
pub fn record(notifications: &mut [Notification], id: &str, label: &str) -> AnswerAdmission {
    let Some(target) = notifications.iter_mut().find(|n| n.id == id) else {
        return AnswerAdmission::NotOffered;
    };
    let offered = target
        .actions
        .as_deref()
        .into_iter()
        .flatten()
        .any(|action| action.answer_label() == Some(label));
    if !offered {
        return AnswerAdmission::NotOffered;
    }
    let Some(slot) = target.answer.as_mut() else {
        return AnswerAdmission::NotOffered;
    };
    if let Some(standing) = slot.answered.as_deref() {
        return AnswerAdmission::AlreadyAnswered(standing.to_string());
    }
    if slot.asked_from_tab.is_none() {
        return AnswerAdmission::Undeliverable;
    }
    slot.answered = Some(label.to_string());
    AnswerAdmission::Recorded(label.to_string())
}

/// The tab that asked is gone. Unanswered questions become undeliverable; an
/// answer already recorded is history and stays exactly as it was.
pub fn forget_tab(notifications: &mut [Notification], tab_id: &str) {
    for notification in notifications.iter_mut() {
        let Some(slot) = notification.answer.as_mut() else {
            continue;
        };
        if slot.answered.is_none() && slot.asked_from_tab.as_deref() == Some(tab_id) {
            slot.asked_from_tab = None;
        }
    }
}

#[cfg(test)]
mod tests;
