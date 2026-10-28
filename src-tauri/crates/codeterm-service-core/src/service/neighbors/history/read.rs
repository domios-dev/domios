use super::{handling::State, History, Submission};
use codeterm_neighbors::wire::DeliveryStatus;

impl History {
    pub(crate) fn reserve_read_through(&mut self, message: &str, enabled: bool, paused: bool) -> bool {
        if !enabled || paused || self.ended { return false; }
        let Some(next) = self.entries.iter().position(|entry| entry.direction == "in" && entry.message_id == message) else { return false };
        if self.read_receipt_through.as_ref().and_then(|previous| self.entries.iter().position(|entry| entry.direction == "in" && &entry.message_id == previous)).is_some_and(|previous| next <= previous) {
            return false;
        }
        self.read_receipt_through = Some(message.into());
        true
    }

    pub(crate) fn concierge_read_through(&self) -> Option<String> {
        self.entries.iter().rev().find(|entry| entry.direction == "in" && matches!(entry.handling.handling_state,
            Some(State::Working | State::Answered | State::Refused | State::NoReply)))
            .map(|entry| entry.message_id.clone())
    }

    pub(crate) fn apply_read_through(&mut self, uid: &str, message: &str, at: u64) -> bool {
        if self.ended { return false; }
        let Some(end) = self.entries.iter().position(|entry| entry.direction == "out" && entry.sender_uid == uid && entry.message_id == message) else { return false };
        let mut changed = false;
        for entry in &mut self.entries[..=end] {
            if entry.direction != "out" || entry.sender_uid != uid { continue; }
            if entry.message_id != message && (entry.submission == Some(Submission::Refused) || entry.delivery_status == Some(DeliveryStatus::NotDelivered)) { continue; }
            if entry.read_at.is_none() {
                entry.read_at = Some(at);
                entry.delivered_at.get_or_insert(at);
                entry.delivery_status = Some(DeliveryStatus::Delivered);
                entry.submission = Some(Submission::Accepted);
                entry.reason = None;
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests;
