use super::store::Notification;

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
