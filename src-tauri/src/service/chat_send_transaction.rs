use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};

mod durable_restore;
mod live_evidence;
mod rpc_evidence;
mod service_context;
mod system_send;
pub use system_send::SystemSend;
mod turn_matching;
mod turn_reconciliation;
use live_evidence::{ConsumedTurnWatermark, LiveEvidenceProgress};
pub(crate) use live_evidence::{LiveProvenance, TurnPosition};
use turn_matching::matching_turn_send_ids;
pub(crate) use turn_matching::{
    contains_written_payload, normalize_turn_evidence_with, normalize_written_payload,
    written_payload_match_end,
};

use super::pending_delivery_payloads::user_sends::{
    PreparedUserBody, UserReservation, UserSendAuthority, UserSendRecord, MAX_PREPARED_BYTES,
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const MAX_RESULTS: usize = 256;
const MAX_IN_FLIGHT: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSendState {
    #[serde(rename = "held")]
    Pending,
    Written,
    #[serde(rename = "submit_unconfirmed")]
    Unknown,
    #[serde(rename = "delivered")]
    Sent,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChatSendStatus {
    pub send_id: String,
    pub tab_id: String,
    pub state: ChatSendState,
    pub error: Option<String>,
    pub ledger_id: String,
    pub sequence: u64,
    pub retained_from: u64,
    /// Opaque client correlation token echoed verbatim from the send envelope.
    /// Field name matches the chat-core consumer (lane ca6aebc5): `correlationId`
    /// on NormalizedChatMessage. Absent when the send carried none — never synthesized.
    #[serde(rename = "correlationId", skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_send: Option<SystemSend>,
}

/// Capability-based append-window boundary captured at send start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum EvidenceWindow {
    /// Append-only transcript file: the window is bytes at/after this offset.
    Jsonl { transcript_offset: u64 },
    /// Structured store (`uses_structured_chat` capability): the window is
    /// rows after this opaque provider cursor. `None` = the store was empty at
    /// send start, so every row is post-send.
    Structured { cursor: Option<String> },
}

/// Authoritative transcript identity captured at send start. Evidence for the
/// send may only come from THIS session, inside its append window — never from
/// another/latest session and never from pre-send history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SendEvidenceAnchor {
    pub(crate) provider_id: String,
    pub(crate) cwd: String,
    pub(crate) session_id: String,
    pub(crate) window: EvidenceWindow,
    pub(crate) turn_normalization: crate::agent::provider::TurnEvidenceNormalization,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SendLiveBinding {
    pub(crate) session_id: Option<String>,
    pub(crate) binding_nonce: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeferredFirstTurnEvidence {
    pub(crate) provider_id: String,
    pub(crate) cwd: String,
    pub(crate) binding_nonce: String,
}

pub(crate) struct DeferredFirstTurnCandidate {
    pub(crate) send_id: String,
    pub(crate) payload: String,
    pub(crate) turn_normalization: crate::agent::provider::TurnEvidenceNormalization,
    pub(crate) evidence: DeferredFirstTurnEvidence,
}

pub(crate) struct EvidenceCandidate {
    pub(crate) send_id: String,
    pub(crate) payload: String,
    pub(crate) anchor: SendEvidenceAnchor,
}

struct ChatSendLedgerState {
    ledger_id: String,
    next_sequence: u64,
    by_id: HashMap<String, ChatSendStatus>,
    /// Normalized payload per send, retained only for exact turn-evidence
    /// correlation — never serialized back to the UI.
    payload_by_id: HashMap<String, String>,
    turn_normalization_by_id: HashMap<String, crate::agent::provider::TurnEvidenceNormalization>,
    /// Opaque client correlation token per send — echoed verbatim onto every
    /// emitted status, never read by evidence/confirm/retry logic.
    token_by_id: HashMap<String, String>,
    anchor_by_id: HashMap<String, SendEvidenceAnchor>,
    rpc_by_id: HashMap<String, SendLiveBinding>,
    live_binding_by_id: HashMap<String, SendLiveBinding>,
    latest_by_tab: HashMap<String, String>,
    latest_failure_by_tab: HashMap<String, String>,
    order: VecDeque<String>,
    owned: HashMap<String, (UserSendAuthority, bool)>,
    terminal_failures: HashSet<String>,
    consumed_merged_turns: HashMap<String, usize>,
    consumed_merged_turn_order: VecDeque<String>,
    held_before_readiness_by_id: HashMap<String, UserSendAuthority>,
    absence_witness_by_id: HashMap<String, Arc<str>>,
    sessionless_first_turn_by_id: HashSet<String>,
    deferred_first_turn_by_id: HashMap<String, DeferredFirstTurnEvidence>,
    claimed_first_turn_by_tab: HashMap<String, (String, String)>,
    live_evidence_progress_by_tab: HashMap<String, LiveEvidenceProgress>,
    consumed_turn_by_tab: HashMap<String, ConsumedTurnWatermark>,
}

impl Default for ChatSendLedgerState {
    fn default() -> Self {
        Self {
            ledger_id: uuid::Uuid::new_v4().to_string(),
            next_sequence: 1,
            by_id: HashMap::new(),
            payload_by_id: HashMap::new(),
            turn_normalization_by_id: HashMap::new(),
            token_by_id: HashMap::new(),
            anchor_by_id: HashMap::new(),
            rpc_by_id: HashMap::new(),
            live_binding_by_id: HashMap::new(),
            latest_by_tab: HashMap::new(),
            latest_failure_by_tab: HashMap::new(),
            order: VecDeque::new(),
            owned: HashMap::new(),
            terminal_failures: HashSet::new(),
            consumed_merged_turns: HashMap::new(),
            consumed_merged_turn_order: VecDeque::new(),
            held_before_readiness_by_id: HashMap::new(),
            absence_witness_by_id: HashMap::new(),
            sessionless_first_turn_by_id: HashSet::new(),
            deferred_first_turn_by_id: HashMap::new(),
            claimed_first_turn_by_tab: HashMap::new(),
            live_evidence_progress_by_tab: HashMap::new(),
            consumed_turn_by_tab: HashMap::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct ChatSendLedger {
    state: Mutex<ChatSendLedgerState>,
    owner: Mutex<Option<Weak<super::ServiceContext>>>,
    admission_restores: Mutex<HashMap<String, Arc<Mutex<bool>>>>,
    submit_claims:
        Mutex<HashMap<String, crate::service::pty_io::submit_transaction::SubmitCycleClaim>>,
}

impl ChatSendLedger {
    pub(crate) fn restore_before_user_admission(
        &self,
        tab_id: &str,
        restore: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let initialized = self
            .admission_restores
            .lock()
            .entry(tab_id.to_string())
            .or_default()
            .clone();
        let mut restored = initialized.lock();
        if !*restored {
            restore()?;
            *restored = true;
        }
        Ok(())
    }

    pub(crate) fn status(&self, send_id: &str) -> Option<ChatSendStatus> {
        let state = self.state.lock();
        state
            .by_id
            .get(send_id)
            .cloned()
            .map(|status| Self::with_retention_window(&state, status))
    }

    pub(crate) fn track_retired_agent_delivery(
        &self,
        tab_id: &str,
        send_id: &str,
        sequence: u64,
        payload: &str,
        anchor: SendEvidenceAnchor,
    ) {
        let mut state = self.state.lock();
        let status = ChatSendStatus {
            send_id: send_id.to_string(),
            tab_id: tab_id.to_string(),
            state: ChatSendState::Unknown,
            error: Some("submit_unconfirmed: delivery was claimed before restart".into()),
            ledger_id: state.ledger_id.clone(),
            sequence,
            retained_from: sequence,
            correlation_id: None,
            pending_reason: None,
            system_send: None,
        };
        let payload = normalize_written_payload(payload);
        let position = state.order.iter().position(|id| {
            state
                .by_id
                .get(id)
                .is_some_and(|previous| previous.sequence > sequence)
        });
        if let Some(position) = position {
            state.order.insert(position, send_id.to_string());
        } else {
            state.order.push_back(send_id.to_string());
        }
        state.payload_by_id.insert(send_id.to_string(), payload);
        state
            .turn_normalization_by_id
            .insert(send_id.to_string(), anchor.turn_normalization);
        state.anchor_by_id.insert(send_id.to_string(), anchor);
        state.by_id.insert(send_id.to_string(), status);
        state.next_sequence = state.next_sequence.max(sequence.saturating_add(1));
        let latest_sequence = state
            .latest_by_tab
            .get(tab_id)
            .and_then(|id| state.by_id.get(id))
            .map_or(0, |latest| latest.sequence);
        if sequence >= latest_sequence {
            state
                .latest_by_tab
                .insert(tab_id.to_string(), send_id.to_string());
        }
    }

    pub(crate) fn reserve_identity(&self) -> (String, u64) {
        let mut state = self.state.lock();
        let sequence = state.next_sequence;
        state.next_sequence += 1;
        (uuid::Uuid::new_v4().to_string(), sequence)
    }

    pub(crate) fn bind_user_pty_session_generation(
        &self,
        send_id: &str,
        authority: &UserSendAuthority,
    ) -> bool {
        let Some(generation) = authority.pty_session_generation else {
            return false;
        };
        let mut state = self.state.lock();
        let Some(current) = state
            .owned
            .get(send_id)
            .map(|(authority, _)| authority.clone())
        else {
            return false;
        };
        let mut unbound = authority.clone();
        unbound.pty_session_generation = None;
        if current == unbound {
            if let Some((current, _)) = state.owned.get_mut(send_id) {
                current.pty_session_generation = Some(generation);
            } else {
                return false;
            }
            if let Some(held_authority) = state.held_before_readiness_by_id.get_mut(send_id) {
                held_authority.pty_session_generation = Some(generation);
            }
            true
        } else {
            &current == authority
        }
    }

    pub(crate) fn rebind_queued_user_authority(
        &self,
        send_id: &str,
        previous: &UserSendAuthority,
        current: &UserSendAuthority,
    ) -> bool {
        let mut state = self.state.lock();
        let Some((authority, active)) = state.owned.get_mut(send_id) else {
            return false;
        };
        if authority != previous || *active {
            return false;
        }
        *authority = current.clone();
        state
            .held_before_readiness_by_id
            .insert(send_id.to_string(), current.clone());
        true
    }

    pub(crate) fn store_user_submit_claim(
        &self,
        send_id: &str,
        claim: crate::service::pty_io::submit_transaction::SubmitCycleClaim,
    ) {
        self.submit_claims
            .lock()
            .insert(send_id.to_string(), claim.with_trace_id(send_id));
    }

    pub(crate) fn take_user_submit_claim(
        &self,
        send_id: &str,
    ) -> Option<crate::service::pty_io::submit_transaction::SubmitCycleClaim> {
        self.submit_claims.lock().remove(send_id)
    }

    pub(crate) fn begin_user_handoff(
        &self,
        record: &UserSendRecord,
        text: &str,
        evidence: super::chat_send_evidence::CapturedSendEvidence,
    ) -> bool {
        let absence_witness = evidence.absence_witness.clone();
        let sessionless_first_turn = evidence.session_starts_on_first_user_turn
            && evidence
                .live_binding
                .as_ref()
                .is_some_and(|binding| binding.session_id.is_none())
            && record.authority.session_id.is_none()
            && record.authority.observed_session_id.is_none();
        let binding = evidence.live_binding;
        if binding.as_ref().is_some_and(|binding| {
            binding
                .binding_nonce
                .as_deref()
                .is_some_and(|nonce| nonce != record.authority.binding_nonce)
        }) {
            return false;
        }
        let payload = normalize_written_payload(text);
        let mut state = self.state.lock();
        let Some((authority, active)) = state.owned.get_mut(&record.send_id) else {
            return false;
        };
        if authority != &record.authority || *active {
            return false;
        }
        *active = true;
        if let Some(binding) = binding {
            state
                .live_binding_by_id
                .insert(record.send_id.clone(), binding);
        } else {
            state.live_binding_by_id.remove(&record.send_id);
        }
        state
            .turn_normalization_by_id
            .insert(record.send_id.clone(), evidence.turn_normalization);
        state.payload_by_id.insert(record.send_id.clone(), payload);
        if let Some(anchor) = evidence.anchor {
            state.anchor_by_id.insert(record.send_id.clone(), anchor);
            state.absence_witness_by_id.remove(&record.send_id);
            state.sessionless_first_turn_by_id.remove(&record.send_id);
            state.deferred_first_turn_by_id.remove(&record.send_id);
        } else if let Some(witness) = absence_witness {
            state
                .absence_witness_by_id
                .insert(record.send_id.clone(), witness);
            state.sessionless_first_turn_by_id.remove(&record.send_id);
            state.deferred_first_turn_by_id.remove(&record.send_id);
        } else if sessionless_first_turn {
            state
                .sessionless_first_turn_by_id
                .insert(record.send_id.clone());
            if let Some(evidence) = evidence.deferred_first_turn {
                if evidence.binding_nonce == record.authority.binding_nonce {
                    state
                        .deferred_first_turn_by_id
                        .insert(record.send_id.clone(), evidence);
                }
            }
        }
        true
    }

    pub(crate) fn set_deferred_first_turn_evidence(
        &self,
        send_id: &str,
        evidence: DeferredFirstTurnEvidence,
    ) -> bool {
        let mut state = self.state.lock();
        let Some(status) = state.by_id.get(send_id) else {
            return false;
        };
        let binding_matches = state
            .live_binding_by_id
            .get(send_id)
            .is_some_and(|binding| {
                binding.session_id.is_none()
                    && binding.binding_nonce.as_deref() == Some(evidence.binding_nonce.as_str())
            });
        if status.state == ChatSendState::Sent || !binding_matches {
            return false;
        }
        state
            .sessionless_first_turn_by_id
            .insert(send_id.to_string());
        state
            .deferred_first_turn_by_id
            .insert(send_id.to_string(), evidence);
        true
    }

    pub(crate) fn mark_user_written(&self, send_id: &str) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        if !state.owned.contains_key(send_id) {
            return None;
        }
        let status = state.by_id.get_mut(send_id)?;
        if matches!(
            status.state,
            ChatSendState::Written | ChatSendState::Sent | ChatSendState::Failed
        ) {
            return None;
        }
        status.state = ChatSendState::Written;
        crate::flow_trace::record(send_id, "send.written", &status.tab_id, "", "pty", None);
        status.error = None;
        status.pending_reason = None;
        let status = status.clone();
        Some(Self::with_retention_window(&state, status))
    }

    pub(crate) fn zero_write_user_handoff(&self, send_id: &str) {
        let mut state = self.state.lock();
        if let Some((_, active)) = state.owned.get_mut(send_id) {
            *active = false;
        }
        state.anchor_by_id.remove(send_id);
        state.live_binding_by_id.remove(send_id);
        state.absence_witness_by_id.remove(send_id);
        state.sessionless_first_turn_by_id.remove(send_id);
        state.deferred_first_turn_by_id.remove(send_id);
    }

    #[cfg(test)]
    pub(crate) fn user_confirmed(&self, send_id: &str) -> bool {
        self.state
            .lock()
            .by_id
            .get(send_id)
            .is_some_and(|status| status.state == ChatSendState::Sent)
    }

    pub(crate) fn user_payload_may_have_been_written(&self, send_id: &str) -> bool {
        self.state.lock().by_id.get(send_id).is_some_and(|status| {
            matches!(
                status.state,
                ChatSendState::Written | ChatSendState::Unknown | ChatSendState::Sent
            )
        })
    }

    fn wake_owner(&self, tab_id: &str) {
        let owner = self.owner.lock().as_ref().and_then(Weak::upgrade);
        if let Some(owner) = owner {
            owner.wake_user_owner(tab_id, super::parked_user_delivery::UserWake::Housekeeping);
        }
    }

    #[cfg(test)]
    pub(crate) fn try_start(
        &self,
        tab_id: &str,
        text: &str,
        anchor: Option<SendEvidenceAnchor>,
    ) -> Result<ChatSendStatus, String> {
        let live_binding = anchor.as_ref().map(|anchor| SendLiveBinding {
            session_id: Some(anchor.session_id.clone()),
            binding_nonce: None,
        });
        self.try_start_scoped(tab_id, text, anchor, live_binding)
    }

    #[cfg(test)]
    pub(crate) fn try_start_scoped(
        &self,
        tab_id: &str,
        text: &str,
        anchor: Option<SendEvidenceAnchor>,
        live_binding: Option<SendLiveBinding>,
    ) -> Result<ChatSendStatus, String> {
        let turn_normalization = anchor
            .as_ref()
            .map_or_else(Default::default, |anchor| anchor.turn_normalization);
        self.try_start_scoped_with_normalization(
            tab_id,
            text,
            anchor,
            live_binding,
            turn_normalization,
        )
    }

    pub(crate) fn try_start_scoped_with_normalization(
        &self,
        tab_id: &str,
        turn_evidence_text: &str,
        anchor: Option<SendEvidenceAnchor>,
        live_binding: Option<SendLiveBinding>,
        turn_normalization: crate::agent::provider::TurnEvidenceNormalization,
    ) -> Result<ChatSendStatus, String> {
        let normalized_payload = normalize_written_payload(turn_evidence_text);
        let mut state = self.state.lock();
        let in_flight = state
            .by_id
            .values()
            .filter(|status| {
                status.state == ChatSendState::Pending && !state.owned.contains_key(&status.send_id)
            })
            .count();
        if in_flight >= MAX_IN_FLIGHT {
            return Err(format!(
                "chat send backpressure: {MAX_IN_FLIGHT} sends are still pending"
            ));
        }
        let sequence = state.next_sequence;
        state.next_sequence += 1;
        let status = ChatSendStatus {
            send_id: uuid::Uuid::new_v4().to_string(),
            tab_id: tab_id.to_string(),
            state: ChatSendState::Pending,
            error: None,
            ledger_id: state.ledger_id.clone(),
            sequence,
            retained_from: sequence,
            correlation_id: None,
            pending_reason: None,
            system_send: None,
        };
        state
            .latest_by_tab
            .insert(tab_id.to_string(), status.send_id.clone());
        state.order.push_back(status.send_id.clone());
        state.by_id.insert(status.send_id.clone(), status.clone());
        state
            .payload_by_id
            .insert(status.send_id.clone(), normalized_payload);
        state
            .turn_normalization_by_id
            .insert(status.send_id.clone(), turn_normalization);
        if let Some(anchor) = anchor {
            state.anchor_by_id.insert(status.send_id.clone(), anchor);
        }
        if let Some(binding) = live_binding {
            state
                .live_binding_by_id
                .insert(status.send_id.clone(), binding);
        }
        crate::flow_trace::record(
            &status.send_id,
            "send.started",
            tab_id,
            "",
            "accepted",
            None,
        );
        Self::prune_terminal(&mut state);
        Ok(Self::with_retention_window(&state, status))
    }

    /// Attach the send envelope's opaque client correlation token to an existing
    /// send. Echoed verbatim onto every subsequent emitted status as
    /// `correlationId`; passthrough metadata only — never read by evidence,
    /// confirm, retry, or the write-once path. Returns the re-stamped status.
    pub(crate) fn attach_client_token(
        &self,
        send_id: &str,
        token: String,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        if !state.by_id.contains_key(send_id) {
            return None;
        }
        state.token_by_id.insert(send_id.to_string(), token);
        let status = state.by_id.get(send_id).cloned()?;
        Some(Self::with_retention_window(&state, status))
    }

    pub(crate) fn finish(
        &self,
        send_id: &str,
        result: Result<(), String>,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let user_owned = state.owned.contains_key(send_id);
        let terminal_failure = state.terminal_failures.contains(send_id);
        let rpc_accepted = state.rpc_by_id.contains_key(send_id)
            && state
                .by_id
                .get(send_id)
                .is_some_and(|status| status.state == ChatSendState::Written)
            && result.is_ok();
        let result = if user_owned { Ok(()) } else { result };
        let failed_tab = {
            let status = state.by_id.get_mut(send_id)?;
            // Exact turn evidence already proved delivery — a late transport
            // classification must never downgrade it back to a false failure.
            match result {
                _ if status.state == ChatSendState::Sent => {
                    status.pending_reason = None;
                    None
                }
                _ if terminal_failure => None,
                _ if rpc_accepted => None,
                _ if user_owned
                    && matches!(
                        status.state,
                        ChatSendState::Written | ChatSendState::Unknown
                    ) =>
                {
                    status.state = ChatSendState::Unknown;
                    status.error = None;
                    status.pending_reason = None;
                    None
                }
                _ if user_owned && status.pending_reason.is_some() => {
                    status.state = ChatSendState::Pending;
                    status.error = None;
                    None
                }
                _ if user_owned => {
                    status.state = ChatSendState::Pending;
                    status.error = None;
                    None
                }
                Ok(()) => {
                    status.pending_reason = None;
                    status.state = ChatSendState::Unknown;
                    status.error = None;
                    None
                }
                Err(error) if error.starts_with("submit_unconfirmed:") => {
                    status.pending_reason = None;
                    status.state = ChatSendState::Unknown;
                    status.error = None;
                    None
                }
                Err(error) => {
                    status.pending_reason = None;
                    status.state = ChatSendState::Failed;
                    status.error = Some(error);
                    Some(status.tab_id.clone())
                }
            }
        };
        if let Some(tab_id) = failed_tab {
            state
                .latest_failure_by_tab
                .insert(tab_id, send_id.to_string());
        }
        let status = state.by_id.get(send_id).cloned()?;
        Some(Self::with_retention_window(&state, status))
    }

    pub(crate) fn expire_owned_before_write(
        &self,
        send_id: &str,
        reason: &str,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        if state.owned.get(send_id).is_none_or(|(_, active)| *active) {
            return None;
        }
        let tab_id = {
            let status = state.by_id.get_mut(send_id)?;
            if status.state != ChatSendState::Pending {
                return None;
            }
            status.state = ChatSendState::Failed;
            status.error = Some(reason.to_string());
            status.pending_reason = None;
            status.tab_id.clone()
        };
        state.owned.remove(send_id);
        state.held_before_readiness_by_id.remove(send_id);
        state.terminal_failures.insert(send_id.to_string());
        state
            .latest_failure_by_tab
            .insert(tab_id, send_id.to_string());
        let status = state.by_id.get(send_id).cloned()?;
        Some(Self::with_retention_window(&state, status))
    }

    pub(crate) fn set_user_pending_reason(
        &self,
        send_id: &str,
        reason: Option<&str>,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let status = state.by_id.get_mut(send_id)?;
        if matches!(
            status.state,
            ChatSendState::Written
                | ChatSendState::Unknown
                | ChatSendState::Sent
                | ChatSendState::Failed
        ) {
            return None;
        }
        let reason = reason.map(str::to_string);
        if status.pending_reason == reason {
            return None;
        }
        status.pending_reason = reason;
        let status = status.clone();
        Some(Self::with_retention_window(&state, status))
    }

    fn mark_sent(state: &mut ChatSendLedgerState, send_id: &str) -> Option<ChatSendStatus> {
        if state.terminal_failures.contains(send_id) {
            return None;
        }
        {
            let status = state.by_id.get_mut(send_id)?;
            if status.state == ChatSendState::Sent {
                return None;
            }
            status.state = ChatSendState::Sent;
            status.error = None;
            status.pending_reason = None;
        }
        state
            .latest_failure_by_tab
            .retain(|_, failed| failed != send_id);
        state.sessionless_first_turn_by_id.remove(send_id);
        state.deferred_first_turn_by_id.remove(send_id);
        state.owned.remove(send_id);
        let status = state.by_id.get(send_id).cloned();
        if let Some(status) = status.as_ref() {
            crate::flow_trace::record(
                send_id,
                "send.delivered",
                &status.tab_id,
                "",
                "exact_turn",
                None,
            );
            crate::flow_trace::record(
                send_id,
                "transcript.append",
                &status.tab_id,
                "",
                "own_user_turn",
                None,
            );
        }
        status
    }

    /// Confirm one send by id after exact append-window evidence was found in
    /// its own recorded session transcript. No-op if already sent.
    pub(crate) fn confirm_send(&self, send_id: &str) -> Option<ChatSendStatus> {
        let owned = {
            let state = self.state.lock();
            state
                .owned
                .get(send_id)
                .cloned()
                .map(|value| (value, state.live_binding_by_id.get(send_id).cloned()))
        };
        if let Some(((authority, active), binding)) = owned {
            let owner = self.owner.lock().as_ref().and_then(Weak::upgrade)?;
            let (session, nonce) = binding
                .and_then(|binding| Some((binding.session_id?, binding.binding_nonce?)))
                .or_else(|| {
                    owner.mux.read().get_tab(&authority.tab_id).and_then(|tab| {
                        Some((
                            tab.agent_session_id.clone()?,
                            tab.reported_session_binding_nonce.clone()?,
                        ))
                    })
                })?;
            if !active || !owner.user_authority_is_current_session(&authority, &session, &nonce) {
                return None;
            }
        }
        let mut state = self.state.lock();
        let status = Self::mark_sent(&mut state, send_id)?;
        let status = Self::with_retention_window(&state, status);
        drop(state);
        self.wake_owner(&status.tab_id);
        Some(status)
    }

    /// Unconfirmed sends (oldest first) that carry an evidence anchor and a
    /// non-empty normalized payload — the only sends a transcript scan may touch.
    pub(crate) fn evidence_candidates(&self, tab_id: &str) -> Vec<EvidenceCandidate> {
        let state = self.state.lock();
        state
            .order
            .iter()
            .filter_map(|send_id| {
                let status = state.by_id.get(send_id)?;
                if status.tab_id != tab_id
                    || status.state == ChatSendState::Sent
                    || state.terminal_failures.contains(send_id)
                    || state.owned.get(send_id).is_some_and(|(_, active)| !active)
                {
                    return None;
                }
                if state.owned.contains_key(send_id)
                    && !matches!(
                        status.state,
                        ChatSendState::Written | ChatSendState::Unknown
                    )
                {
                    return None;
                }
                let payload = state.payload_by_id.get(send_id).cloned()?;
                if payload.is_empty() {
                    return None;
                }
                let anchor = state.anchor_by_id.get(send_id)?.clone();
                Some(EvidenceCandidate {
                    send_id: send_id.clone(),
                    payload,
                    anchor,
                })
            })
            .collect()
    }

    pub(crate) fn deferred_first_turn_candidates(
        &self,
        tab_id: &str,
    ) -> Vec<DeferredFirstTurnCandidate> {
        let state = self.state.lock();
        state
            .order
            .iter()
            .filter_map(|send_id| {
                let status = state.by_id.get(send_id)?;
                if status.tab_id != tab_id
                    || !matches!(
                        status.state,
                        ChatSendState::Written | ChatSendState::Unknown
                    )
                    || state.terminal_failures.contains(send_id)
                    || state.owned.get(send_id).is_some_and(|(_, active)| !active)
                {
                    return None;
                }
                let payload = state.payload_by_id.get(send_id)?.clone();
                if payload.is_empty() {
                    return None;
                }
                Some(DeferredFirstTurnCandidate {
                    send_id: send_id.clone(),
                    payload,
                    turn_normalization: state
                        .turn_normalization_by_id
                        .get(send_id)
                        .copied()
                        .unwrap_or_default(),
                    evidence: state.deferred_first_turn_by_id.get(send_id)?.clone(),
                })
            })
            .collect()
    }

    pub(crate) fn confirm_deferred_first_turn(
        &self,
        candidate: &DeferredFirstTurnCandidate,
        session_id: &str,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let status = state.by_id.get(&candidate.send_id)?;
        if !matches!(
            status.state,
            ChatSendState::Written | ChatSendState::Unknown
        ) || state.terminal_failures.contains(&candidate.send_id)
            || state.deferred_first_turn_by_id.get(&candidate.send_id) != Some(&candidate.evidence)
        {
            return None;
        }
        let tab_id = status.tab_id.clone();
        let claim = (
            session_id.to_string(),
            candidate.evidence.binding_nonce.clone(),
        );
        if state.claimed_first_turn_by_tab.get(&tab_id) == Some(&claim) {
            return None;
        }
        state
            .claimed_first_turn_by_tab
            .insert(tab_id.clone(), claim);
        let same_launch = state
            .deferred_first_turn_by_id
            .iter()
            .filter(|(send_id, evidence)| {
                *evidence == &candidate.evidence
                    && state
                        .by_id
                        .get(*send_id)
                        .is_some_and(|status| status.tab_id == tab_id)
            })
            .map(|(send_id, _)| send_id.clone())
            .collect::<Vec<_>>();
        for send_id in same_launch {
            state.deferred_first_turn_by_id.remove(&send_id);
        }
        let status = Self::mark_sent(&mut state, &candidate.send_id)?;
        let status = Self::with_retention_window(&state, status);
        drop(state);
        self.wake_owner(&tab_id);
        Some(status)
    }

    /// Anchors of sends already confirmed for this tab+session+payload — each
    /// one already consumed a matching transcript turn inside its own window.
    pub(crate) fn claimed_turn_anchors(
        &self,
        tab_id: &str,
        session_id: &str,
        payload: &str,
    ) -> Vec<SendEvidenceAnchor> {
        let state = self.state.lock();
        state
            .by_id
            .values()
            .filter(|status| status.tab_id == tab_id && status.state == ChatSendState::Sent)
            .filter(|status| {
                state.payload_by_id.get(&status.send_id).map(String::as_str) == Some(payload)
            })
            .filter_map(|status| state.anchor_by_id.get(&status.send_id))
            .filter(|anchor| anchor.session_id == session_id)
            .cloned()
            .collect()
    }

    pub(crate) fn reconcile(&self, tab_id: &str) -> Vec<ChatSendStatus> {
        let state = self.state.lock();
        let retained = state
            .order
            .iter()
            .filter_map(|send_id| state.by_id.get(send_id))
            .filter(|status| {
                status.tab_id == tab_id
                    && (state.latest_by_tab.get(tab_id) == Some(&status.send_id)
                        || status.state == ChatSendState::Sent
                        || matches!(
                            status.state,
                            ChatSendState::Written | ChatSendState::Unknown | ChatSendState::Failed
                        )
                        || (status.state == ChatSendState::Pending
                            && state.owned.contains_key(&status.send_id)))
            })
            .cloned()
            .map(|status| Self::with_retention_window(&state, status))
            .collect::<Vec<_>>();
        if !retained.is_empty() {
            return retained;
        }
        state
            .latest_by_tab
            .get(tab_id)
            .and_then(|send_id| state.by_id.get(send_id))
            .cloned()
            .map(|status| Self::with_retention_window(&state, status))
            .into_iter()
            .collect()
    }

    fn prune_terminal(state: &mut ChatSendLedgerState) {
        while state.order.len() > MAX_RESULTS {
            let Some(position) = state.order.iter().position(|send_id| {
                state
                    .by_id
                    .get(send_id)
                    .is_some_and(|status| status.state != ChatSendState::Pending)
                    && !state.owned.contains_key(send_id)
            }) else {
                break;
            };
            let expired = state
                .order
                .remove(position)
                .expect("the selected terminal result exists");
            state.by_id.remove(&expired);
            state.payload_by_id.remove(&expired);
            state.turn_normalization_by_id.remove(&expired);
            state.token_by_id.remove(&expired);
            state.anchor_by_id.remove(&expired);
            state.rpc_by_id.remove(&expired);
            state.live_binding_by_id.remove(&expired);
            state.absence_witness_by_id.remove(&expired);
            state.sessionless_first_turn_by_id.remove(&expired);
            state.deferred_first_turn_by_id.remove(&expired);
            state.terminal_failures.remove(&expired);
            state.held_before_readiness_by_id.remove(&expired);
            state.latest_by_tab.retain(|_, send_id| send_id != &expired);
            state
                .latest_failure_by_tab
                .retain(|_, send_id| send_id != &expired);
        }
    }

    fn with_retention_window(
        state: &ChatSendLedgerState,
        mut status: ChatSendStatus,
    ) -> ChatSendStatus {
        status.retained_from = state
            .order
            .front()
            .and_then(|send_id| state.by_id.get(send_id))
            .map_or(state.next_sequence, |retained| retained.sequence);
        // Verbatim passthrough: every emitted status carries the send's client
        // token (if any). Never read by evidence/confirm — echo only.
        status.correlation_id = state.token_by_id.get(&status.send_id).cloned();
        status
    }
}

pub(crate) fn run_chat_send(
    ledger: &ChatSendLedger,
    pending: ChatSendStatus,
    text: &str,
    submit: impl FnOnce(&str) -> Result<(), crate::service::delivery_error::DeliveryError>,
) -> ChatSendStatus {
    let result = submit(text).map_err(|error| error.ledger_message());
    crate::flow_trace::record(
        &pending.send_id,
        "send.transport",
        &pending.tab_id,
        "",
        if result.is_ok() {
            "accepted"
        } else {
            "uncertain_or_refused"
        },
        None,
    );
    ledger
        .finish(&pending.send_id, result)
        .expect("a started Chat send remains in the bounded ledger until completion")
}

/// Feed freshly appended session user turns into the ledger. Called from the
/// live delta sinks only — never from history snapshots, so stale transcript
/// rows can never confirm a send.
pub(crate) fn delivered_turn_confirmations(
    ledger: &ChatSendLedger,
    tab_id: &str,
    source_session_id: &str,
    source_binding_nonce: Option<&str>,
    messages: &[crate::agent::chat_types::ChatMsg],
    provenance: Option<&LiveProvenance>,
) -> Vec<ChatSendStatus> {
    if let Some(provenance) = provenance {
        ledger.advance_live_evidence(tab_id, source_session_id, source_binding_nonce, provenance);
    }
    let mut confirmed = Vec::new();
    for (index, msg) in messages.iter().enumerate() {
        let positioned = provenance.and_then(|provenance| {
            provenance.positions.get(index).copied().map(|position| {
                ledger.confirm_owned_live_turns(
                    tab_id,
                    source_session_id,
                    source_binding_nonce,
                    msg,
                    position,
                    provenance,
                )
            })
        });
        let statuses = positioned
            .filter(|statuses| !statuses.is_empty())
            .unwrap_or_else(|| {
                ledger.confirm_delivered_user_turns_scoped(
                    tab_id,
                    source_session_id,
                    source_binding_nonce,
                    &msg.content,
                )
            });
        for status in statuses {
            log::info!(
                "[chat-send] send_id={} tab={} session={} confirmed by exact turn evidence",
                status.send_id,
                status.tab_id,
                source_session_id
            );
            confirmed.push(status);
        }
    }
    confirmed
}

#[cfg(test)]
mod tests;
