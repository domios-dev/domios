use crate::agent::provider::ReplyRelayTurn;
use std::collections::{HashSet, VecDeque};

pub const DEFAULT_REPLY_RELAY_MAX_HOPS: u8 = 8;
pub const MAX_TRACKED_RELAY_DELIVERIES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingReplyRelay {
    pub sender_tab: String,
    pub delivery_id: String,
    pub provider_id: String,
    pub session_id: String,
    pub binding_nonce: String,
    pub after_offset: u64,
    pub user_text: String,
    pub turn_normalization: crate::agent::provider::TurnEvidenceNormalization,
    pub hops: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelaySource<'a> {
    pub provider_id: &'a str,
    pub session_id: &'a str,
    pub binding_nonce: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplyRelayDecision {
    Wait,
    DropClosedSender,
    DropHopLimit,
    Send {
        answer: String,
        hops: u8,
        user_turn_id: String,
    },
}

pub fn reply_relay_eligible(
    capability: bool,
    routed_tab_message: bool,
    sender_is_agent: bool,
    has_reply_hint: bool,
    sender_is_remote: bool,
) -> bool {
    capability && routed_tab_message && sender_is_agent && has_reply_hint && !sender_is_remote
}

pub fn decide_reply_relay(
    pending: &PendingReplyRelay,
    source: RelaySource<'_>,
    turn: Option<&ReplyRelayTurn>,
    sender_open: bool,
    max_hops: u8,
    turn_already_relayed: bool,
) -> ReplyRelayDecision {
    if !sender_open {
        return ReplyRelayDecision::DropClosedSender;
    }
    if pending.hops >= max_hops {
        return ReplyRelayDecision::DropHopLimit;
    }
    if source.provider_id.is_empty()
        || source.session_id.is_empty()
        || source.binding_nonce.is_empty()
        || pending.delivery_id.is_empty()
        || pending.sender_tab.is_empty()
        || source.provider_id != pending.provider_id
        || source.session_id != pending.session_id
        || source.binding_nonce != pending.binding_nonce
        || turn_already_relayed
    {
        return ReplyRelayDecision::Wait;
    }
    let Some(turn) = turn else {
        return ReplyRelayDecision::Wait;
    };
    if !turn.complete
        || turn.user_record_start < pending.after_offset
        || turn.user_turn_id.is_empty()
        || turn.assistant_turn_id.is_empty()
        || turn.answer.trim().is_empty()
        || normalize(&pending.user_text, pending).is_empty()
        || normalize(&turn.user_text, pending) != normalize(&pending.user_text, pending)
    {
        return ReplyRelayDecision::Wait;
    }
    ReplyRelayDecision::Send {
        answer: turn.answer.clone(),
        hops: pending.hops + 1,
        user_turn_id: turn.user_turn_id.clone(),
    }
}

#[derive(Default)]
pub struct ReplyRelayModel {
    pending: VecDeque<PendingReplyRelay>,
    delivery_ids: HashSet<String>,
    relayed_turns: HashSet<(String, String, String, String)>,
}

impl ReplyRelayModel {
    pub fn record(&mut self, pending: PendingReplyRelay) -> bool {
        if self.delivery_ids.len() >= MAX_TRACKED_RELAY_DELIVERIES
            || pending.delivery_id.is_empty()
            || !self.delivery_ids.insert(pending.delivery_id.clone())
        {
            return false;
        }
        self.pending.push_back(pending);
        true
    }

    pub fn observe(
        &mut self,
        source: RelaySource<'_>,
        turn: Option<&ReplyRelayTurn>,
        sender_open: impl Fn(&str) -> bool,
        max_hops: u8,
    ) -> Vec<(PendingReplyRelay, ReplyRelayDecision)> {
        let mut actions = Vec::new();
        let mut remaining = VecDeque::new();
        while let Some(pending) = self.pending.pop_front() {
            let key = turn.map(|turn| {
                (
                    source.provider_id.to_string(),
                    source.session_id.to_string(),
                    source.binding_nonce.to_string(),
                    turn.user_turn_id.clone(),
                )
            });
            let decision = decide_reply_relay(
                &pending,
                source,
                turn,
                sender_open(&pending.sender_tab),
                max_hops,
                key.as_ref()
                    .is_some_and(|key| self.relayed_turns.contains(key)),
            );
            if matches!(decision, ReplyRelayDecision::Wait) {
                remaining.push_back(pending);
                continue;
            }
            if matches!(decision, ReplyRelayDecision::Send { .. }) {
                if let Some(key) = key {
                    self.relayed_turns.insert(key);
                }
            }
            actions.push((pending, decision));
        }
        self.pending = remaining;
        actions
    }
}

fn normalize(text: &str, pending: &PendingReplyRelay) -> String {
    crate::service::chat_send_transaction::normalize_turn_evidence_with(
        text,
        pending.turn_normalization,
    )
}

#[cfg(test)]
mod tests;
