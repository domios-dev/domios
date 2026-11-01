use super::{handling::State, History, Submission};
use codeterm_neighbors::wire::DeliveryStatus;

impl History {
    fn incoming_position(&self, message: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.direction == "in" && entry.message_id == message)
    }

    pub(crate) fn request_read_through(
        &mut self,
        message: &str,
        enabled: bool,
        paused: bool,
    ) -> bool {
        if !enabled || paused || self.ended {
            return false;
        }
        let Some(next) = self.incoming_position(message) else {
            return false;
        };
        if self
            .read_requested_through
            .as_deref()
            .and_then(|id| self.incoming_position(id))
            .is_some_and(|previous| next <= previous)
        {
            return false;
        }
        self.read_requested_through = Some(message.into());
        true
    }

    pub(crate) fn acknowledge_message(&mut self, message: &str) -> bool {
        if self.incoming_position(message).is_none() {
            return false;
        }
        self.acknowledged_messages.insert(message.into())
    }

    pub(crate) fn pending_read_through(&self) -> Option<String> {
        let requested = self.read_requested_through.as_ref()?;
        let next = self.incoming_position(requested)?;
        if self
            .read_receipt_through
            .as_deref()
            .and_then(|id| self.incoming_position(id))
            .is_some_and(|previous| next <= previous)
        {
            return None;
        }
        Some(requested.clone())
    }

    pub(crate) fn reserve_read_through(
        &mut self,
        message: &str,
        enabled: bool,
        paused: bool,
    ) -> bool {
        if !enabled || paused || self.ended {
            return false;
        }
        let Some(next) = self
            .entries
            .iter()
            .position(|entry| entry.direction == "in" && entry.message_id == message)
        else {
            return false;
        };
        let previous = self.read_receipt_through.as_ref().and_then(|previous| {
            self.entries
                .iter()
                .position(|entry| entry.direction == "in" && &entry.message_id == previous)
        });
        if previous.is_some_and(|previous| next <= previous) {
            return false;
        }
        let start = previous.map_or(0, |previous| previous + 1);
        if self.entries[start..=next].iter().any(|entry| {
            entry.direction == "in" && !self.acknowledged_messages.contains(&entry.message_id)
        }) {
            return false;
        }
        self.read_receipt_through = Some(message.into());
        true
    }

    pub(crate) fn concierge_read_through(&self) -> Option<String> {
        self.entries
            .iter()
            .rev()
            .find(|entry| {
                entry.direction == "in"
                    && matches!(
                        entry.handling.handling_state,
                        Some(State::Working | State::Answered | State::Refused | State::NoReply)
                    )
            })
            .map(|entry| entry.message_id.clone())
    }

    pub(crate) fn apply_read_through(&mut self, uid: &str, message: &str, at: u64) -> bool {
        if self.ended {
            return false;
        }
        let Some(end) = self.entries.iter().position(|entry| {
            entry.direction == "out" && entry.sender_uid == uid && entry.message_id == message
        }) else {
            return false;
        };
        let previous = self.outgoing_read_through.as_ref().and_then(|id| {
            self.entries.iter().position(|entry| {
                entry.direction == "out" && entry.sender_uid == uid && &entry.message_id == id
            })
        });
        let mut changed = previous.is_none_or(|previous| end > previous);
        if changed {
            self.outgoing_read_through = Some(message.into());
            self.outgoing_read_at = Some(at);
        }
        for entry in &mut self.entries[..=end] {
            if entry.direction != "out" || entry.sender_uid != uid {
                continue;
            }
            if entry.message_id != message
                && (entry.submission == Some(Submission::Refused)
                    || entry.delivery_status == Some(DeliveryStatus::NotDelivered))
            {
                continue;
            }
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

    pub(crate) fn covered_read_at(&self, uid: &str, message: &str) -> Option<u64> {
        let end = self.entries.iter().position(|entry| {
            entry.direction == "out"
                && entry.sender_uid == uid
                && Some(&entry.message_id) == self.outgoing_read_through.as_ref()
        })?;
        let index = self.entries.iter().position(|entry| {
            entry.direction == "out" && entry.sender_uid == uid && entry.message_id == message
        })?;
        (index <= end).then_some(self.outgoing_read_at).flatten()
    }
}

#[cfg(test)]
mod tests;
