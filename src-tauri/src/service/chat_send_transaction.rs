use std::collections::{HashMap, VecDeque};

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
    anchor_by_id: HashMap<String, SendEvidenceAnchor>,
    live_binding_by_id: HashMap<String, SendLiveBinding>,
    latest_by_tab: HashMap<String, String>,
    latest_failure_by_tab: HashMap<String, String>,
    order: VecDeque<String>,
}

impl Default for ChatSendLedgerState {
    fn default() -> Self {
        Self {
            ledger_id: uuid::Uuid::new_v4().to_string(),
            next_sequence: 1,
            by_id: HashMap::new(),
            payload_by_id: HashMap::new(),
            anchor_by_id: HashMap::new(),
            live_binding_by_id: HashMap::new(),
            latest_by_tab: HashMap::new(),
            latest_failure_by_tab: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

/// The shape a delivered payload takes when it reappears as a session user
/// turn: control bytes stripped by the send path, surrounding whitespace
/// trimmed by TUI/transcript rendering.
pub(crate) fn normalize_turn_evidence(text: &str) -> String {
    crate::service::pty_io::sanitize_agent_prompt_input(text)
        .trim()
        .to_string()
}

#[derive(Default)]
pub(crate) struct ChatSendLedger {
    state: Mutex<ChatSendLedgerState>,
}

impl ChatSendLedger {
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

    pub(crate) fn try_start_scoped(
        &self,
        tab_id: &str,
        text: &str,
        anchor: Option<SendEvidenceAnchor>,
        live_binding: Option<SendLiveBinding>,
    ) -> Result<ChatSendStatus, String> {
        let mut state = self.state.lock();
        let in_flight = state
            .by_id
            .values()
            .filter(|status| status.state == ChatSendState::Pending)
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
        };
        state
            .latest_by_tab
            .insert(tab_id.to_string(), status.send_id.clone());
        state.order.push_back(status.send_id.clone());
        state.by_id.insert(status.send_id.clone(), status.clone());
        state
            .payload_by_id
            .insert(status.send_id.clone(), normalize_turn_evidence(text));
        if let Some(anchor) = anchor {
            state.anchor_by_id.insert(status.send_id.clone(), anchor);
        }
        if let Some(binding) = live_binding {
            state
                .live_binding_by_id
                .insert(status.send_id.clone(), binding);
        }
        while state.order.len() > MAX_RESULTS {
            let Some(position) = state.order.iter().position(|send_id| {
                state
                    .by_id
                    .get(send_id)
                    .is_some_and(|status| status.state != ChatSendState::Pending)
            }) else {
                break;
            };
            let expired = state
                .order
                .remove(position)
                .expect("the selected terminal result exists");
            state.by_id.remove(&expired);
            state.payload_by_id.remove(&expired);
            state.anchor_by_id.remove(&expired);
            state.live_binding_by_id.remove(&expired);
            state.latest_by_tab.retain(|_, send_id| send_id != &expired);
            state
                .latest_failure_by_tab
                .retain(|_, send_id| send_id != &expired);
        }
        Ok(Self::with_retention_window(&state, status))
    }

    pub(crate) fn finish(
        &self,
        send_id: &str,
        result: Result<(), String>,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let failed_tab = {
            let status = state.by_id.get_mut(send_id)?;
            // Exact turn evidence already proved delivery — a late transport
            // classification must never downgrade it back to a false failure.
            match result {
                _ if status.state == ChatSendState::Sent => None,
                Ok(()) => {
                    status.state = ChatSendState::Unknown;
                    status.error = None;
                    None
                }
                Err(error) if error.starts_with("submit_unconfirmed:") => {
                    status.state = ChatSendState::Unknown;
                    status.error = None;
                    None
                }
                Err(error) => {
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

    /// Exact-evidence reconciliation: a user turn with exactly this content was
    /// appended to the tab's own bound session transcript. Confirms the oldest
    /// unconfirmed send with the matching payload; one turn confirms at most
    /// one send. Unrelated, stale, or other-tab turns never match.
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
        let evidence = normalize_turn_evidence(turn_text);
        if evidence.is_empty() {
            return None;
        }
        let mut state = self.state.lock();
        let send_id = state
            .order
            .iter()
            .find(|send_id| {
                state.by_id.get(*send_id).is_some_and(|status| {
                    status.tab_id == tab_id && status.state != ChatSendState::Sent
                }) && state.payload_by_id.get(*send_id) == Some(&evidence)
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

    fn mark_sent(state: &mut ChatSendLedgerState, send_id: &str) -> Option<ChatSendStatus> {
        {
            let status = state.by_id.get_mut(send_id)?;
            if status.state == ChatSendState::Sent {
                return None;
            }
            status.state = ChatSendState::Sent;
            status.error = None;
        }
        state
            .latest_failure_by_tab
            .retain(|_, failed| failed != send_id);
        state.by_id.get(send_id).cloned()
    }

    /// Confirm one send by id after exact append-window evidence was found in
    /// its own recorded session transcript. No-op if already sent.
    pub(crate) fn confirm_send(&self, send_id: &str) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let status = Self::mark_sent(&mut state, send_id)?;
        Some(Self::with_retention_window(&state, status))
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
                if status.tab_id != tab_id || status.state == ChatSendState::Sent {
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
                    && matches!(status.state, ChatSendState::Unknown | ChatSendState::Failed)
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

    fn with_retention_window(
        state: &ChatSendLedgerState,
        mut status: ChatSendStatus,
    ) -> ChatSendStatus {
        status.retained_from = state
            .order
            .front()
            .and_then(|send_id| state.by_id.get(send_id))
            .map_or(state.next_sequence, |retained| retained.sequence);
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
        let Some(status) = ledger.confirm_delivered_user_turn_scoped(
            tab_id,
            source_session_id,
            source_binding_nonce,
            &msg.content,
        ) else {
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
    ) -> Result<ChatSendStatus, String> {
        let submit_claim = self.claim_chat_submit_cycle(tab_id);
        let evidence = self.chat_send_anchor(tab_id);
        let pending = self.chat_send_ledger.try_start_scoped(
            tab_id,
            &text,
            evidence.anchor.clone(),
            evidence.live_binding.clone(),
        )?;
        let response = pending.clone();
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            let status = run_chat_send(&svc.chat_send_ledger, pending, &text, |text| {
                svc.send_text_delivery_claimed(&tab_id, text, submit_claim)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::delivery_error::DeliveryError;
    use crate::service::inject_verify::InjectionError;

    fn submit_unconfirmed(detail: impl Into<String>) -> DeliveryError {
        DeliveryError::SubmitUnconfirmed {
            tab: "tab-1".into(),
            detail: detail.into(),
        }
    }

    #[test]
    fn serialized_states_separate_acceptance_uncertainty_and_confirmation() {
        assert_eq!(serde_json::to_string(&ChatSendState::Pending).unwrap(), "\"accepted\"");
        assert_eq!(serde_json::to_string(&ChatSendState::Unknown).unwrap(), "\"unknown\"");
        assert_eq!(serde_json::to_string(&ChatSendState::Sent).unwrap(), "\"confirmed\"");
        assert_eq!(serde_json::to_string(&ChatSendState::Failed).unwrap(), "\"failed\"");
    }

    #[test]
    fn unconfirmed_submit_retains_one_payload_and_correlated_result() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1", "payload", None).unwrap();
        let mut writes = Vec::new();
        let result = run_chat_send(&ledger, pending.clone(), "one payload", |text| {
            writes.push(text.to_string());
            Err(submit_unconfirmed("submit key was not acknowledged"))
        });

        assert_eq!(writes, ["one payload"]);
        assert_eq!(result.send_id, pending.send_id);
        assert_eq!(result.state, ChatSendState::Unknown);
        assert_eq!(result.error, None);
        assert_eq!(ledger.reconcile("tab-1"), vec![result]);
    }

    #[test]
    fn unknown_transaction_survives_ui_detach_and_activation() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1", "payload", None).unwrap();

        assert_eq!(ledger.reconcile("tab-1"), vec![pending.clone()]);

        let result = run_chat_send(&ledger, pending, "hello", |_| Ok(()));

        assert_eq!(result.state, ChatSendState::Unknown);
        assert_eq!(ledger.reconcile("tab-1"), vec![result]);
    }

    #[test]
    fn a_missed_failure_survives_a_newer_pending_send() {
        let ledger = ChatSendLedger::default();
        let first = ledger.try_start("tab-1", "payload", None).unwrap();
        let second = ledger.try_start("tab-1", "payload", None).unwrap();
        let failed = run_chat_send(&ledger, first, "first", |_| {
            Err(submit_unconfirmed("Enter swallowed"))
        });

        assert_eq!(ledger.reconcile("tab-1"), vec![failed]);
        assert_eq!(second.state, ChatSendState::Pending);
    }

    #[test]
    fn in_flight_cap_rejects_before_growth_and_releases_on_completion() {
        let ledger = ChatSendLedger::default();
        let mut pending = Vec::new();
        for index in 0..MAX_IN_FLIGHT {
            pending.push(
                ledger
                    .try_start(&format!("tab-{index}"), "payload", None)
                    .unwrap(),
            );
        }

        assert!(ledger.try_start("overflow", "payload", None).is_err());
        assert_eq!(ledger.state.lock().by_id.len(), MAX_IN_FLIGHT);

        ledger.finish(&pending[0].send_id, Ok(())).unwrap();
        assert!(ledger.try_start("released", "payload", None).is_ok());
        ledger
            .confirm_delivered_user_turn("tab-0", "session-0", "payload")
            .unwrap();
        assert_eq!(ledger.state.lock().by_id.len(), MAX_IN_FLIGHT + 1);
    }

    #[test]
    fn reconcile_retains_every_unknown_send_missed_while_chat_is_detached() {
        let ledger = ChatSendLedger::default();
        let first = ledger.try_start("tab-1", "payload", None).unwrap();
        let second = ledger.try_start("tab-1", "payload", None).unwrap();
        run_chat_send(&ledger, first, "first", |_| {
            Err(submit_unconfirmed("first failed"))
        });
        run_chat_send(&ledger, second, "second", |_| {
            Err(submit_unconfirmed("second failed"))
        });

        let statuses = ledger.reconcile("tab-1");
        assert_eq!(statuses.len(), 2);
        assert!(statuses
            .iter()
            .all(|status| status.state == ChatSendState::Unknown && status.error.is_none()));
    }

    #[test]
    fn reconciliation_exposes_the_backend_retention_window() {
        let ledger = ChatSendLedger::default();
        for index in 0..=MAX_RESULTS {
            let pending = ledger.try_start("tab-1", "payload", None).unwrap();
            run_chat_send(&ledger, pending, "message", |_| {
                Err(submit_unconfirmed(format!("failure-{index}")))
            });
        }

        let retained = ledger.reconcile("tab-1");
        assert_eq!(retained.len(), MAX_RESULTS);
        assert_eq!(retained[0].sequence, 2);
        assert!(retained.iter().all(|status| status.retained_from == 2));
    }

    #[test]
    fn echo_mismatch_reaches_ledger_as_submit_unconfirmed() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1", "payload", None).unwrap();
        let result = run_chat_send(&ledger, pending, "wrapped payload", |_| {
            Err(DeliveryError::from_injection(
                "tab-1",
                InjectionError::EchoMismatch {
                    expected_substr: "wrapped payload".into(),
                    screen_tail: "bordered frame".into(),
                },
            ))
        });

        assert_eq!(result.state, ChatSendState::Unknown);
        assert_eq!(result.error, None);
    }

    #[test]
    fn exact_turn_evidence_clears_a_false_failure() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1", "deploy the fix", None).unwrap();
        run_chat_send(&ledger, pending.clone(), "deploy the fix", |_| {
            Err(submit_unconfirmed("acknowledgement heuristics timed out"))
        });

        let confirmed = ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "deploy the fix")
            .expect("exact same-tab turn evidence confirms the failed send");
        assert_eq!(confirmed.send_id, pending.send_id);
        assert_eq!(confirmed.state, ChatSendState::Sent);
        assert_eq!(confirmed.error, None);
        assert_eq!(ledger.reconcile("tab-1"), vec![confirmed]);
    }

    #[test]
    fn evidence_during_flight_wins_over_late_transport_failure() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1", "restart dev", None).unwrap();
        let confirmed = ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "  restart dev  ")
            .expect("turn evidence may arrive before classification finishes");
        assert_eq!(confirmed.state, ChatSendState::Sent);

        let result = run_chat_send(&ledger, pending, "restart dev", |_| {
            Err(DeliveryError::from_injection(
                "tab-1",
                InjectionError::PtyClosed {
                    pty_id: "pty-1".into(),
                },
            ))
        });
        assert_eq!(result.state, ChatSendState::Sent);
        assert_eq!(result.error, None);
        assert_eq!(ledger.reconcile("tab-1"), vec![result]);
    }

    #[test]
    fn unrelated_stale_or_foreign_turns_never_confirm() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1", "exact payload", None).unwrap();
        run_chat_send(&ledger, pending, "exact payload", |_| {
            Err(submit_unconfirmed("no evidence"))
        });

        assert!(ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "different text")
            .is_none());
        assert!(ledger
            .confirm_delivered_user_turn("tab-2", "session-1", "exact payload")
            .is_none());
        assert!(ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "   ")
            .is_none());
        assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Unknown);
    }

    #[test]
    fn stale_session_or_binding_delta_cannot_confirm_a_rebound_send() {
        let ledger = ChatSendLedger::default();
        let pending = ledger
            .try_start_scoped(
                "tab-1",
                "exact payload",
                None,
                Some(SendLiveBinding {
                    session_id: Some("session-new".into()),
                    binding_nonce: Some("binding-new".into()),
                }),
            )
            .unwrap();
        ledger.finish(&pending.send_id, Ok(())).unwrap();

        assert!(ledger
            .confirm_delivered_user_turn_scoped(
                "tab-1",
                "session-new",
                Some("binding-old"),
                "exact payload",
            )
            .is_none());
        assert!(ledger
            .confirm_delivered_user_turn_scoped(
                "tab-1",
                "session-old",
                Some("binding-new"),
                "exact payload",
            )
            .is_none());
        assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Unknown);

        let confirmed = ledger
            .confirm_delivered_user_turn_scoped(
                "tab-1",
                "session-new",
                Some("binding-new"),
                "exact payload",
            )
            .unwrap();
        assert_eq!(confirmed.state, ChatSendState::Sent);
    }

    #[test]
    fn one_turn_confirms_at_most_one_send_oldest_first() {
        let ledger = ChatSendLedger::default();
        let first = ledger.try_start("tab-1", "same text", None).unwrap();
        let second = ledger.try_start("tab-1", "same text", None).unwrap();

        let confirmed = ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "same text")
            .unwrap();
        assert_eq!(confirmed.send_id, first.send_id);
        assert!(ledger
            .state
            .lock()
            .by_id
            .get(&second.send_id)
            .is_some_and(|status| status.state == ChatSendState::Pending));

        let confirmed_second = ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "same text")
            .unwrap();
        assert_eq!(confirmed_second.send_id, second.send_id);
        assert!(ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "same text")
            .is_none());
    }

    #[test]
    fn rebind_window_send_is_not_sent_by_transport_classification_alone() {
        let ledger = ChatSendLedger::default();
        let pending = ledger
            .try_start("tab-1", "Reply with exactly: REBIND-1", None)
            .unwrap();
        let finished = ledger.finish(&pending.send_id, Ok(())).unwrap();
        assert_eq!(
            finished.state,
            ChatSendState::Unknown,
            "transport Ok is not attribution: sent requires a turn in the tab's own bound session"
        );
    }

    #[test]
    fn transport_ok_for_rebind_window_send_defers_to_turn_evidence() {
        let ledger = ChatSendLedger::default();
        let pending = ledger
            .try_start("tab-1", "Reply with exactly: REBIND-2", None)
            .unwrap();
        let mut writes = Vec::new();
        let result = run_chat_send(
            &ledger,
            pending.clone(),
            "Reply with exactly: REBIND-2",
            |text| {
                writes.push(text.to_string());
                Ok(())
            },
        );
        assert_eq!(writes.len(), 1, "payload written exactly once");
        assert_eq!(
            result.state,
            ChatSendState::Unknown,
            "transport completion stays unknown until exact turn evidence"
        );
        let confirmed = ledger
            .confirm_delivered_user_turn("tab-1", "rebound-session", "Reply with exactly: REBIND-2")
            .expect("the attributable turn confirms the held send");
        assert_eq!(confirmed.send_id, pending.send_id);
        assert_eq!(confirmed.state, ChatSendState::Sent);
        assert_eq!(ledger.reconcile("tab-1")[0].state, ChatSendState::Sent);
    }

    #[test]
    fn already_sent_deliveries_are_never_reconfirmed() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1", "hello", None).unwrap();
        run_chat_send(&ledger, pending, "hello", |_| Ok(()));
        ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "hello")
            .unwrap();
        assert!(ledger
            .confirm_delivered_user_turn("tab-1", "session-1", "hello")
            .is_none());
    }
}
