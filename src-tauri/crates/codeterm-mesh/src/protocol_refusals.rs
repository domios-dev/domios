//! Per-peer relay-protocol refusal state, so a polling peer on an unsupported
//! protocol is reported once when refusal starts and once when it ends.

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolRefusalTransition {
    Refused,
    Accepted,
}

#[derive(Debug, Default)]
pub struct ProtocolRefusals {
    refused: HashSet<String>,
}

impl ProtocolRefusals {
    pub fn observe(&mut self, peer_id: &str, refused: bool) -> Option<ProtocolRefusalTransition> {
        if refused {
            self.refused
                .insert(peer_id.to_string())
                .then_some(ProtocolRefusalTransition::Refused)
        } else {
            self.refused
                .remove(peer_id)
                .then_some(ProtocolRefusalTransition::Accepted)
        }
    }

    pub fn forget(&mut self, peer_id: &str) {
        self.refused.remove(peer_id);
    }
}

#[cfg(test)]
mod tests;
