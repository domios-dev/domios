use crate::agent::provider::{ReplyRelayEvidence, ReplyRelayTurn};
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
    Notice {
        text: String,
        user_turn_id: Option<String>,
    },
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
    if !matches_source(pending, source) || turn_already_relayed {
        return ReplyRelayDecision::Wait;
    }
    let Some(turn) = turn else {
        return ReplyRelayDecision::Wait;
    };
    if !turn.complete
        || turn.assistant_turn_id.is_empty()
        || turn.answer.trim().is_empty()
        || !matches_user_turn(
            pending,
            &turn.user_turn_id,
            turn.user_record_start,
            &turn.user_text,
        )
    {
        return ReplyRelayDecision::Wait;
    }
    ReplyRelayDecision::Send {
        answer: turn.answer.clone(),
        hops: pending.hops + 1,
        user_turn_id: turn.user_turn_id.clone(),
    }
}

pub fn decide_reply_relay_evidence(
    pending: &PendingReplyRelay,
    source: RelaySource<'_>,
    evidence: Option<&ReplyRelayEvidence>,
    sender_open: bool,
    max_hops: u8,
    turn_already_relayed: bool,
) -> ReplyRelayDecision {
    let evidence = evidence.filter(|evidence| evidence.is_valid());
    let common = decide_reply_relay(
        pending,
        source,
        None,
        sender_open,
        max_hops,
        turn_already_relayed,
    );
    if !matches!(common, ReplyRelayDecision::Wait) {
        return common;
    }
    if !matches_source(pending, source) || turn_already_relayed {
        return ReplyRelayDecision::Wait;
    }
    match evidence {
        Some(ReplyRelayEvidence::Answered(turn)) => {
            decide_reply_relay(pending, source, Some(turn), sender_open, max_hops, false)
        }
        Some(ReplyRelayEvidence::Ended(turn))
            if matches_user_turn(
                pending,
                &turn.user_turn_id,
                turn.user_record_start,
                &turn.user_text,
            ) =>
        {
            ReplyRelayDecision::Notice {
                text: format!("turn ended: {}", turn.outcome.as_str()),
                user_turn_id: Some(turn.user_turn_id.clone()),
            }
        }
        Some(ReplyRelayEvidence::Unavailable(state)) => ReplyRelayDecision::Notice {
            text: format!(
                "Reply relay is not active for this session: {}",
                state.reason
            ),
            user_turn_id: None,
        },
        _ => ReplyRelayDecision::Wait,
    }
}

fn matches_source(pending: &PendingReplyRelay, source: RelaySource<'_>) -> bool {
    !source.provider_id.is_empty()
        && !source.session_id.is_empty()
        && !source.binding_nonce.is_empty()
        && !pending.delivery_id.is_empty()
        && !pending.sender_tab.is_empty()
        && source.provider_id == pending.provider_id
        && source.session_id == pending.session_id
        && source.binding_nonce == pending.binding_nonce
}

fn matches_user_turn(pending: &PendingReplyRelay, id: &str, start: u64, text: &str) -> bool {
    !id.is_empty()
        && start >= pending.after_offset
        && !normalize(&pending.user_text, pending).is_empty()
        && normalize(text, pending) == normalize(&pending.user_text, pending)
}

#[derive(Default)]
pub struct ReplyRelayModel {
    pending: VecDeque<PendingReplyRelay>,
    delivery_ids: HashSet<String>,
    relayed_turns: HashSet<(String, String, String, String)>,
    unavailable_reason: Option<String>,
}

impl ReplyRelayModel {
    pub fn record(&mut self, pending: PendingReplyRelay) -> bool {
        if self.unavailable_reason.is_some()
            || self.delivery_ids.len() >= MAX_TRACKED_RELAY_DELIVERIES
            || pending.delivery_id.is_empty()
            || !self.delivery_ids.insert(pending.delivery_id.clone())
        {
            return false;
        }
        self.pending.push_back(pending);
        true
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable_reason.as_deref()
    }

    pub fn observe(
        &mut self,
        source: RelaySource<'_>,
        turn: Option<&ReplyRelayTurn>,
        sender_open: impl Fn(&str) -> bool,
        max_hops: u8,
    ) -> Vec<(PendingReplyRelay, ReplyRelayDecision)> {
        self.observe_evidence(
            source,
            turn.cloned().map(ReplyRelayEvidence::Answered).as_ref(),
            sender_open,
            max_hops,
        )
    }

    pub fn observe_evidence(
        &mut self,
        source: RelaySource<'_>,
        evidence: Option<&ReplyRelayEvidence>,
        sender_open: impl Fn(&str) -> bool,
        max_hops: u8,
    ) -> Vec<(PendingReplyRelay, ReplyRelayDecision)> {
        let user_turn_id = match evidence {
            Some(ReplyRelayEvidence::Answered(turn)) => Some(turn.user_turn_id.as_str()),
            Some(ReplyRelayEvidence::Ended(turn)) => Some(turn.user_turn_id.as_str()),
            _ => None,
        };
        let mut actions = Vec::new();
        let mut remaining = VecDeque::new();
        while let Some(pending) = self.pending.pop_front() {
            let key = user_turn_id.map(|user_turn_id| {
                (
                    source.provider_id.to_string(),
                    source.session_id.to_string(),
                    source.binding_nonce.to_string(),
                    user_turn_id.to_string(),
                )
            });
            let decision = decide_reply_relay_evidence(
                &pending,
                source,
                evidence,
                sender_open(&pending.sender_tab),
                max_hops,
                key.as_ref()
                    .is_some_and(|key| self.relayed_turns.contains(key)),
            );
            if matches!(decision, ReplyRelayDecision::Wait) {
                remaining.push_back(pending);
                continue;
            }
            if matches!(
                decision,
                ReplyRelayDecision::Send { .. }
                    | ReplyRelayDecision::Notice {
                        user_turn_id: Some(_),
                        ..
                    }
            ) {
                if let Some(key) = key {
                    self.relayed_turns.insert(key);
                }
            }
            if matches!(
                decision,
                ReplyRelayDecision::Notice {
                    user_turn_id: None,
                    ..
                }
            ) {
                if let Some(ReplyRelayEvidence::Unavailable(state)) = evidence {
                    self.unavailable_reason = Some(state.reason.clone());
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
