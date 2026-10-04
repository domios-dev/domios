use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};
use tauri::Manager;

use super::pending_delivery_payloads::user_sends::{
    PreparedUserBody, UserReservation, UserSendAuthority, UserSendRecord, MAX_PREPARED_BYTES,
};

use parking_lot::Mutex;
use serde::Serialize;

const MAX_RESULTS: usize = 256;
const MAX_IN_FLIGHT: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSendState {
    #[serde(rename = "accepted")]
    Pending,
    Unknown,
    #[serde(rename = "confirmed")]
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
}

/// Capability-based append-window boundary captured at send start.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    live_binding_by_id: HashMap<String, SendLiveBinding>,
    latest_by_tab: HashMap<String, String>,
    latest_failure_by_tab: HashMap<String, String>,
    order: VecDeque<String>,
    owned: HashMap<String, (UserSendAuthority, bool)>,
    owned_live_turns: HashMap<String, HashSet<(String, String, String)>>,
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
            live_binding_by_id: HashMap::new(),
            latest_by_tab: HashMap::new(),
            latest_failure_by_tab: HashMap::new(),
            order: VecDeque::new(),
            owned: HashMap::new(),
            owned_live_turns: HashMap::new(),
        }
    }
}

/// The shape a delivered payload takes when it reappears as a session user
/// turn: control bytes stripped by the send path, surrounding whitespace
/// trimmed by TUI/transcript rendering.
pub(crate) fn normalize_turn_evidence_with(
    text: &str,
    normalization: crate::agent::provider::TurnEvidenceNormalization,
) -> String {
    normalization.normalize(&crate::service::pty_io::sanitize_agent_prompt_input(text))
}

#[derive(Default)]
pub(crate) struct ChatSendLedger {
    state: Mutex<ChatSendLedgerState>,
    owner: Mutex<Option<Weak<super::ServiceContext>>>,
}

impl ChatSendLedger {
    pub(crate) fn status(&self, send_id: &str) -> Option<ChatSendStatus> {
        let state = self.state.lock();
        state.by_id.get(send_id).cloned().map(|status| Self::with_retention_window(&state, status))
    }

    pub(crate) fn reserve_identity(&self) -> (String, u64) {
        let mut state = self.state.lock();
        let sequence = state.next_sequence;
        state.next_sequence += 1;
        (uuid::Uuid::new_v4().to_string(), sequence)
    }

    pub(crate) fn admit_user(&self, record: &UserSendRecord) -> ChatSendStatus {
        let mut state = self.state.lock();
        let status = ChatSendStatus {
            send_id: record.send_id.clone(),
            tab_id: record.authority.tab_id.clone(),
            state: ChatSendState::Pending,
            error: None,
            ledger_id: state.ledger_id.clone(),
            sequence: record.ledger_sequence,
            retained_from: record.ledger_sequence,
            correlation_id: record.client_token.clone(),
            pending_reason: None,
        };
        state.owned.insert(record.send_id.clone(), (record.authority.clone(), false));
        let latest_sequence = state.latest_by_tab.get(&status.tab_id)
            .and_then(|id| state.by_id.get(id)).map_or(0, |latest| latest.sequence);
        if status.sequence >= latest_sequence {
            state.latest_by_tab.insert(status.tab_id.clone(), status.send_id.clone());
        }
        if !state.by_id.contains_key(&status.send_id) {
            let position = state.order.iter().position(|id| state.by_id.get(id).is_some_and(|prior| prior.sequence > status.sequence));
            if let Some(position) = position { state.order.insert(position, status.send_id.clone()); }
            else { state.order.push_back(status.send_id.clone()); }
        }
        state.anchor_by_id.remove(&status.send_id);
        state.live_binding_by_id.remove(&status.send_id);
        state.payload_by_id.remove(&status.send_id);
        state.by_id.insert(status.send_id.clone(), status.clone());
        if let Some(token) = &record.client_token {
            state.token_by_id.insert(status.send_id.clone(), token.clone());
        }
        Self::prune_terminal(&mut state);
        Self::with_retention_window(&state, status)
    }

    pub(crate) fn bind_user_pty_session_generation(
        &self,
        send_id: &str,
        authority: &UserSendAuthority,
    ) -> bool {
        let Some(generation) = authority.pty_session_generation else { return false; };
        let mut state = self.state.lock();
        let Some((current, _)) = state.owned.get_mut(send_id) else { return false; };
        let mut unbound = authority.clone();
        unbound.pty_session_generation = None;
        if *current == unbound {
            current.pty_session_generation = Some(generation);
            true
        } else {
            current == authority
        }
    }

    pub(crate) fn begin_user_handoff(
        &self,
        record: &UserSendRecord,
        text: &str,
        evidence: super::chat_send_evidence::CapturedSendEvidence,
    ) -> bool {
        let Some(binding) = evidence.live_binding else { return false; };
        if binding.session_id.is_none() || binding.binding_nonce.as_deref() != Some(record.authority.binding_nonce.as_str()) {
            return false;
        }
        let payload = normalize_turn_evidence_with(text, evidence.turn_normalization);
        let mut state = self.state.lock();
        let Some((authority, active)) = state.owned.get_mut(&record.send_id) else { return false; };
        if authority != &record.authority || *active { return false; }
        *active = true;
        state.live_binding_by_id.insert(record.send_id.clone(), binding);
        state.turn_normalization_by_id.insert(record.send_id.clone(), evidence.turn_normalization);
        state.payload_by_id.insert(record.send_id.clone(), payload);
        if let Some(anchor) = evidence.anchor { state.anchor_by_id.insert(record.send_id.clone(), anchor); }
        true
    }

    pub(crate) fn zero_write_user_handoff(&self, send_id: &str) {
        let mut state = self.state.lock();
        if let Some((_, active)) = state.owned.get_mut(send_id) { *active = false; }
        state.anchor_by_id.remove(send_id);
        state.live_binding_by_id.remove(send_id);
    }

    pub(crate) fn user_confirmed(&self, send_id: &str) -> bool {
        self.state.lock().by_id.get(send_id).is_some_and(|status| status.state == ChatSendState::Sent)
    }

    pub(crate) fn release_user(&self, send_id: &str) {
        let mut state = self.state.lock();
        state.owned.remove(send_id);
        Self::prune_terminal(&mut state);
    }

    fn wake_owner(&self, tab_id: &str) {
        let owner = self.owner.lock().as_ref().and_then(Weak::upgrade);
        if let Some(owner) = owner { owner.wake_user_owner(tab_id, super::parked_user_delivery::UserWake::Housekeeping); }
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
        let normalized_payload =
            normalize_turn_evidence_with(turn_evidence_text, turn_normalization);
        let mut state = self.state.lock();
        let in_flight = state
            .by_id
            .values()
            .filter(|status| status.state == ChatSendState::Pending && !state.owned.contains_key(&status.send_id))
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
                _ if user_owned && status.pending_reason.is_some() => {
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

    pub(crate) fn set_user_pending_reason(
        &self,
        send_id: &str,
        reason: Option<&str>,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let status = state.by_id.get_mut(send_id)?;
        if matches!(status.state, ChatSendState::Sent | ChatSendState::Failed) {
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

    /// Exact-evidence reconciliation: a user turn with exactly this content was
    /// appended to the tab's own bound session transcript. Confirms the oldest
    /// unconfirmed send with the matching payload; one turn confirms at most
    /// one send. Unrelated, stale, or other-tab turns never match.
    #[cfg(test)]
    pub(crate) fn confirm_delivered_user_turn(
        &self,
        tab_id: &str,
        source_session_id: &str,
        turn_text: &str,
    ) -> Option<ChatSendStatus> {
        self.confirm_delivered_user_turn_scoped(tab_id, source_session_id, None, turn_text)
    }

    pub(crate) fn confirm_delivered_user_turn_scoped(
        &self,
        tab_id: &str,
        source_session_id: &str,
        source_binding_nonce: Option<&str>,
        turn_text: &str,
    ) -> Option<ChatSendStatus> {
        let owned_wake = {
            let state = self.state.lock();
            state.owned.iter().any(|(id, (_, active))| *active
                && state.by_id.get(id).is_some_and(|status| status.tab_id == tab_id)
                && state.live_binding_by_id.get(id).is_some_and(|binding|
                    binding.session_id.as_deref() == Some(source_session_id)
                    && binding.binding_nonce.as_deref() == source_binding_nonce))
        };
        if owned_wake { self.wake_owner(tab_id); }
        let candidates = {
            let state = self.state.lock();
            state
                .order
                .iter()
                .filter_map(|send_id| {
                    let status = state.by_id.get(send_id)?;
                    let binding_matches =
                        state.live_binding_by_id.get(send_id).is_none_or(|binding| {
                            binding
                                .session_id
                                .as_deref()
                                .is_none_or(|session_id| session_id == source_session_id)
                                && binding
                                    .binding_nonce
                                    .as_deref()
                                    .is_none_or(|nonce| Some(nonce) == source_binding_nonce)
                        });
                    if state.owned.contains_key(send_id) || status.tab_id != tab_id
                        || status.state == ChatSendState::Sent
                        || !binding_matches
                    {
                        return None;
                    }
                    Some((
                        send_id.clone(),
                        state.payload_by_id.get(send_id)?.clone(),
                        state
                            .turn_normalization_by_id
                            .get(send_id)
                            .copied()
                            .unwrap_or_default(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        let matching_ids = candidates
            .into_iter()
            .filter_map(|(send_id, expected, normalization)| {
                let evidence = normalize_turn_evidence_with(turn_text, normalization);
                (!evidence.is_empty() && evidence == expected).then_some(send_id)
            })
            .collect::<Vec<_>>();
        if matching_ids.is_empty() {
            return None;
        }
        let mut state = self.state.lock();
        let send_id = state
            .order
            .iter()
            .find(|send_id| {
                matching_ids.contains(*send_id)
                    && !state.owned.contains_key(*send_id)
                    && state.by_id.get(*send_id).is_some_and(|status| {
                        status.tab_id == tab_id && status.state != ChatSendState::Sent
                    })
                    && state
                        .live_binding_by_id
                        .get(*send_id)
                        .is_none_or(|binding| {
                            binding
                                .session_id
                                .as_deref()
                                .is_none_or(|session_id| session_id == source_session_id)
                                && binding
                                    .binding_nonce
                                    .as_deref()
                                    .is_none_or(|nonce| Some(nonce) == source_binding_nonce)
                        })
            })?
            .clone();
        let status = Self::mark_sent(&mut state, &send_id)?;
        Some(Self::with_retention_window(&state, status))
    }

    fn confirm_owned_live_turn(
        &self,
        tab_id: &str,
        session_id: &str,
        nonce: Option<&str>,
        message: &crate::agent::chat_types::ChatMsg,
    ) -> Option<ChatSendStatus> {
        let nonce = nonce?;
        if message.id.is_empty() { return None; }
        let candidate = {
            let mut state = self.state.lock();
            if !state.owned.values().any(|(authority, _)| authority.tab_id == tab_id) { return None; }
            let seen = state.owned_live_turns.entry(tab_id.to_string()).or_default();
            if seen.len() >= MAX_RESULTS || !seen.insert((session_id.to_string(), nonce.to_string(), message.id.clone())) {
                return None;
            }
            state.order.iter().find_map(|id| {
                let (authority, active) = state.owned.get(id)?;
                let status = state.by_id.get(id)?;
                let binding = state.live_binding_by_id.get(id)?;
                (*active && authority.tab_id == tab_id && authority.binding_nonce == nonce
                    && status.state != ChatSendState::Sent && !state.anchor_by_id.contains_key(id)
                    && binding.session_id.as_deref() == Some(session_id)).then(|| (
                        id.clone(), state.payload_by_id.get(id).cloned(),
                        state.turn_normalization_by_id.get(id).copied(),
                    ))
            })
        }?;
        let (send_id, expected, normalization) = candidate;
        let text = normalize_turn_evidence_with(&message.content, normalization?);
        if text.is_empty() || expected.as_deref() != Some(text.as_str()) { return None; }
        self.confirm_send(&send_id)
    }

    fn mark_sent(state: &mut ChatSendLedgerState, send_id: &str) -> Option<ChatSendStatus> {
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
        state.by_id.get(send_id).cloned()
    }

    /// Confirm one send by id after exact append-window evidence was found in
    /// its own recorded session transcript. No-op if already sent.
    pub(crate) fn confirm_send(&self, send_id: &str) -> Option<ChatSendStatus> {
        let owned = {
            let state = self.state.lock();
            state.owned.get(send_id).cloned().map(|value| (value, state.live_binding_by_id.get(send_id).cloned()))
        };
        if let Some(((authority, active), binding)) = owned {
            let owner = self.owner.lock().as_ref().and_then(Weak::upgrade)?;
            let session = binding?.session_id?;
            if !active || !owner.user_authority_is_ready(&authority)
                || owner.mux.read().get_tab(&authority.tab_id).and_then(|tab| tab.agent_session_id.as_deref()) != Some(session.as_str()) {
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
                if status.tab_id != tab_id || status.state == ChatSendState::Sent
                    || state.owned.get(send_id).is_some_and(|(_, active)| !active) {
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

    /// Sends already confirmed for this tab+session+payload — each one already
    /// consumed a matching transcript turn, so a scan must find MORE matching
    /// turns than this count before it may confirm another send.
    pub(crate) fn claimed_turn_count(
        &self,
        tab_id: &str,
        session_id: &str,
        payload: &str,
    ) -> usize {
        let state = self.state.lock();
        // The sole owned handoff captures a fresh window after every earlier cycle settled.
        if state.owned.iter().any(|(id, (_, active))| *active
            && state.by_id.get(id).is_some_and(|status| status.tab_id == tab_id && status.state != ChatSendState::Sent)
            && state.payload_by_id.get(id).map(String::as_str) == Some(payload)
            && state.anchor_by_id.get(id).is_some_and(|anchor| anchor.session_id == session_id)) {
            return 0;
        }
        state
            .by_id
            .values()
            .filter(|status| status.tab_id == tab_id && status.state == ChatSendState::Sent)
            .filter(|status| {
                state.payload_by_id.get(&status.send_id).map(String::as_str) == Some(payload)
            })
            .filter(|status| {
                state
                    .anchor_by_id
                    .get(&status.send_id)
                    .is_some_and(|anchor| anchor.session_id == session_id)
            })
            .count()
    }

    pub(crate) fn reconcile(&self, tab_id: &str) -> Vec<ChatSendStatus> {
        let state = self.state.lock();
        let unresolved = state
            .order
            .iter()
            .filter_map(|send_id| state.by_id.get(send_id))
            .filter(|status| {
                status.tab_id == tab_id
                    && (matches!(status.state, ChatSendState::Unknown | ChatSendState::Failed)
                        || (status.state == ChatSendState::Pending && state.owned.contains_key(&status.send_id)))
            })
            .cloned()
            .map(|status| Self::with_retention_window(&state, status))
            .collect::<Vec<_>>();
        if !unresolved.is_empty() {
            return unresolved;
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
            state.live_binding_by_id.remove(&expired);
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
) -> Vec<ChatSendStatus> {
    let mut confirmed = Vec::new();
    for msg in messages.iter().filter(|msg| msg.kind == "user") {
        let Some(status) = ledger.confirm_owned_live_turn(tab_id, source_session_id, source_binding_nonce, msg)
            .or_else(|| ledger.confirm_delivered_user_turn_scoped(
                tab_id, source_session_id, source_binding_nonce, &msg.content,
            )) else {
            continue;
        };
        log::info!(
            "[chat-send] send_id={} tab={} session={} confirmed by exact turn evidence",
            status.send_id,
            status.tab_id,
            source_session_id
        );
        confirmed.push(status);
    }
    confirmed
}

impl crate::service::ServiceContext {
    pub(crate) fn start_chat_send(
        &self,
        tab_id: &str,
        text: String,
        submit_claim: Option<crate::service::pty_io::submit_transaction::SubmitCycleClaim>,
        client_token: Option<String>,
    ) -> Result<ChatSendStatus, String> {
        if self.tab_chat_backing(tab_id).is_none() {
            drop(submit_claim);
            let reservation = self.reserve_user_send(tab_id, client_token)?;
            return self.admit_prepared_user_send(reservation, &text, &text, None).map(|(status, _)| status);
        }
        let evidence = self.chat_send_anchor(tab_id);
        let turn_evidence_text = self.chat_send_turn_evidence(tab_id, &text);
        let pending = self.chat_send_ledger.try_start_scoped_with_normalization(
            tab_id,
            &turn_evidence_text,
            evidence.anchor.clone(),
            evidence.live_binding.clone(),
            evidence.turn_normalization,
        )?;
        let pending = match client_token {
            Some(token) => self
                .chat_send_ledger
                .attach_client_token(&pending.send_id, token)
                .unwrap_or(pending),
            None => pending,
        };
        let response = pending.clone();
        let send_id = pending.send_id.clone();
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            let status = run_chat_send(&svc.chat_send_ledger, pending, &text, |text| {
                // User-origin always-deliver: a transient pre-write unlink or an
                // unconfirmed submit degrades to pending-until-link, never a refusal.
                svc.send_chat_text_claimed(&tab_id, text, submit_claim)
                    .or_else(|err| {
                        log::info!(
                            "[chat-send] user send to Tab {tab_id} degraded to pending ({})",
                            err.code()
                        );
                        svc.park_refused_user_send(&tab_id, &send_id, text, &err);
                        err.into_surfaced(crate::service::delivery_error::SendOrigin::User)
                    })
            });
            crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
            if let Some(error) = status.error.as_deref() {
                log::warn!(
                    "[chat-send] send_id={} tab={} failed: {}",
                    status.send_id,
                    status.tab_id,
                    error
                );
            }
        });
        Ok(response)
    }

    pub(crate) fn reserve_user_send(&self, tab_id: &str, client_token: Option<String>) -> Result<UserReservation, String> {
        let authority = self.user_send_authority(tab_id)?;
        if let Some(service) = self.app_handle.try_state::<super::ServiceState>() {
            let weak = Arc::downgrade(service.inner());
            *self.chat_send_ledger.owner.lock() = Some(weak.clone());
            let tab = tab_id.to_string();
            self.pending_delivery_payloads.user_sends.set_admission_wake(tab_id, Arc::new(move || {
                if let Some(service) = weak.upgrade() { service.wake_user_owner(&tab, super::parked_user_delivery::UserWake::Housekeeping); }
            }));
        }
        // This short reservation is the service ordering point, before asynchronous preparation.
        self.pending_delivery_payloads.user_sends.reserve(authority, client_token,
            || self.chat_send_ledger.reserve_identity())
    }

    pub(crate) fn prepare_and_admit_user_send(
        &self,
        reservation: UserReservation,
        original: String,
        run_stages: bool,
    ) -> Result<super::agent_pad::ChatSendResult, String> {
        if original.len() > MAX_PREPARED_BYTES { return Err("user original exceeds 4 MiB storage bound".into()); }
        let (prepared, returned) = self.prepare_chat_send_text(&reservation.authority.tab_id, original.clone(), run_stages)?;
        let body = returned.as_ref().map(|path| self.capture_prepared_user_body(&reservation.authority.tab_id, path)).transpose()?;
        let (status, owned) = self.admit_prepared_user_send(reservation, &original, &prepared, body)?;
        Ok(super::agent_pad::ChatSendResult::new(status, owned))
    }

    fn capture_prepared_user_body(&self, tab_id: &str, returned: &std::path::Path) -> Result<PreparedUserBody, String> {
        let pad_root = self.agent_pad_data_dir.join("runtime").join("agent-pads");
        let pad = pad_root.join(tab_id);
        let inbox = pad.join("inbox");
        if returned.parent() != Some(inbox.as_path())
            || !std::fs::symlink_metadata(&pad).map_err(|_| "prepared tab scope unavailable")?.file_type().is_dir()
            || !std::fs::symlink_metadata(&inbox).map_err(|_| "prepared inbox scope unavailable")?.file_type().is_dir() {
            return Err("prepared body does not belong to the exact returned tab inbox".into());
        }
        let root = pad_root.canonicalize().map_err(|_| "prepared root scope unavailable")?;
        let canonical_pad = pad.canonicalize().map_err(|_| "prepared tab scope unavailable")?;
        if canonical_pad.parent() != Some(root.as_path()) { return Err("prepared tab escaped its owner scope".into()); }
        let expected = inbox.canonicalize().map_err(|_| "prepared inbox scope unavailable")?;
        if expected.parent() != Some(canonical_pad.as_path()) { return Err("prepared inbox escaped its tab scope".into()); }
        if !std::fs::symlink_metadata(returned).map_err(|_| "prepared body unavailable")?.file_type().is_file() {
            return Err("prepared body is not an owned regular file".into());
        }
        let actual = returned.canonicalize().map_err(|_| "prepared body scope unavailable")?;
        if actual.parent() != Some(expected.as_path()) { return Err("prepared body escaped the returned inbox scope".into()); }
        let bytes = super::pending_delivery_payloads::user_sends::read_bounded(&actual, MAX_PREPARED_BYTES as u64)?;
        Ok(PreparedUserBody { returned_path: returned.to_path_buf(), bytes })
    }

    fn admit_prepared_user_send(
        &self,
        reservation: UserReservation,
        original: &str,
        prepared: &str,
        body: Option<PreparedUserBody>,
    ) -> Result<(ChatSendStatus, Option<std::path::PathBuf>), String> {
        let store = &self.pending_delivery_payloads.user_sends;
        let final_transport = store.final_transport(&reservation, prepared, body.as_ref())?;
        let follow_up = self.agent_registry.with_provider(&reservation.authority.provider_id, |provider| {
            provider.paste_capability().and_then(|capability| capability.typed_follow_up_for(&final_transport).map(str::to_string))
        }).flatten();
        let record = store.admit_with_follow_up(&reservation, original, prepared, body, follow_up)?;
        let status = self.chat_send_ledger.admit_user(&record);
        let path = store.body_path(&record)?.map(|path| {
            path.to_str().map(|text| std::path::PathBuf::from(codeterm_paths::strip_verbatim_prefix(text)))
                .ok_or_else(|| "admission_unknown: durable body path cannot be represented".to_string())
        }).transpose()?;
        crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        let tab = record.authority.tab_id.clone();
        drop(reservation);
        self.wake_user_owner(&tab, super::parked_user_delivery::UserWake::Admission);
        Ok((status, path))
    }

    pub(crate) fn chat_send_turn_evidence(&self, tab_id: &str, text: &str) -> String {
        let provider_id = self.mux.read().get_tab(tab_id).and_then(|tab| {
            matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty)
                .then(|| tab.agent_type.clone())
                .flatten()
        });
        let Some(provider_id) = provider_id else {
            return text.to_string();
        };
        let sanitized = crate::service::pty_io::sanitize_agent_prompt_input(text);
        let follow_up = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider
                    .paste_capability()?
                    .typed_follow_up_for(&sanitized)
                    .map(str::to_string)
            })
            .flatten();
        follow_up.map_or_else(
            || text.to_string(),
            |follow_up| format!("{sanitized}{follow_up}"),
        )
    }
}

#[cfg(test)]
mod tests;
