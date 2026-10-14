//! Closed-chat evidence reconciliation for Chat sends.
//!
//! A send whose live delta stream was not running (Chat closed during
//! delivery) can still be classified honestly: each send captures its bound
//! session and a safe [`SendEvidenceAnchor`] when one exists. Ledger reads
//! inspect ONLY that session's bounded append window. Pre-send history, other
//! sessions, and already-used turns can never confirm a send.

use crate::agent::registry::AgentRegistry;
use crate::service::chat_send_transaction::{
    contains_written_payload, normalize_written_payload, written_payload_match_end, ChatSendLedger,
    ChatSendStatus, DeferredFirstTurnEvidence, EvidenceWindow, SendEvidenceAnchor, SendLiveBinding,
};
use crate::service::tab_chat::registry::{JsonlSource, SourceCapture};
use crate::service::ServiceContext;
use std::collections::HashMap;
use std::sync::Arc;

mod deferred_first_turn;
mod pending_receipt;
pub(crate) mod session_append;
mod turn_window;

use turn_window::window_evidence_records;
pub(crate) use turn_window::window_has_foreign_user_turn;
use turn_window::{structured_evidence_records, StructuredUserTurn};

#[cfg(test)]
#[path = "chat_send_evidence/deferred_first_turn_tests.rs"]
mod deferred_first_turn_tests;

#[cfg(test)]
pub(crate) fn contains_exact_user_turn(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    payload: &str,
) -> bool {
    let expected = normalize_written_payload(payload);
    !expected.is_empty()
        && window_evidence_records(registry, anchor)
            .iter()
            .any(|turn| contains_written_payload(turn, &expected))
}

#[derive(Default)]
pub(crate) struct ExactTurnCursor {
    jsonl_offset: Option<u64>,
    confirmed: bool,
    pub(crate) matched_user_id: Option<String>,
}

impl ExactTurnCursor {
    pub(crate) fn for_anchor(anchor: Option<&SendEvidenceAnchor>) -> Self {
        let mut cursor = Self::default();
        if let Some(SendEvidenceAnchor {
            window: EvidenceWindow::Jsonl { transcript_offset },
            ..
        }) = anchor
        {
            cursor.jsonl_offset = Some(*transcript_offset);
        }
        cursor
    }
}

pub(crate) fn contains_exact_user_turn_since(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    payload: &str,
    cursor: &mut ExactTurnCursor,
) -> bool {
    if cursor.confirmed {
        return true;
    }
    let expected = normalize_written_payload(payload);
    if expected.is_empty() {
        return false;
    }
    let found = match &anchor.window {
        EvidenceWindow::Jsonl { transcript_offset } => {
            let Some(path) = registry
                .with_provider(&anchor.provider_id, |provider| {
                    provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
                })
                .flatten()
            else {
                return false;
            };
            let from_offset = cursor.jsonl_offset.unwrap_or(*transcript_offset);
            let Some(delta) = registry.with_provider(&anchor.provider_id, |provider| {
                provider.parse_session_delta(&path, from_offset)
            }) else {
                return false;
            };
            cursor.jsonl_offset = Some(delta.new_offset.max(from_offset));
            delta.messages.iter().enumerate().any(|(index, rich)| {
                let matched = turn_window::rich_evidence_segments(rich)
                    .iter()
                    .any(|segment| written_payload_match_end(segment, &expected, anchor.turn_normalization, 0).is_some());
                if matched && crate::flow_trace::enabled() {
                    cursor.matched_user_id = (rich.uuid.as_ref().is_some_and(|id| !id.is_empty())
                        || rich.record_start.is_some()).then(||
                        crate::service::tab_chat::rich_to_chat_msgs_live(rich, index)
                            .into_iter().next().map(|message| message.id)
                    ).flatten();
                }
                matched
            })
        }
        EvidenceWindow::Structured { .. } => window_evidence_records(registry, anchor)
            .iter()
            .any(|turn| {
                let observed = normalize_written_payload(turn);
                let matched = written_payload_match_end(&observed, &expected, anchor.turn_normalization, 0).is_some();
                log::debug!(
                    "[session-turn-evidence] structured-candidate provider={} session={} expected_bytes={} observed_bytes={} matched={}",
                    anchor.provider_id, anchor.session_id, expected.len(), observed.len(), matched,
                );
                matched
            }),
    };
    log::debug!(
        "[session-turn-evidence] exact-match provider={} session={} window={:?} expected_bytes={} matched={}",
        anchor.provider_id, anchor.session_id, anchor.window, expected.len(), found,
    );
    cursor.confirmed = found;
    found
}

/// A cursor-less NON-empty structured store has no bounded append window and
/// stays live-sink-only.
pub(crate) fn structured_boundary(
    chat: &crate::agent::chat_types::StructuredChat,
) -> Option<EvidenceWindow> {
    if chat.cursor.is_none() && !chat.messages.is_empty() {
        return None;
    }
    Some(EvidenceWindow::Structured {
        cursor: chat.cursor.clone(),
    })
}

fn jsonl_boundary(
    path: Option<std::path::PathBuf>,
    source: &JsonlSource,
    session_id: &str,
) -> (Option<EvidenceWindow>, Option<Arc<str>>) {
    let Some(path) = path else {
        return (None, None);
    };
    match source.capture(session_id, &path) {
        SourceCapture::Present { len } => (
            Some(EvidenceWindow::Jsonl {
                transcript_offset: len,
            }),
            None,
        ),
        SourceCapture::Absent { witness } => (None, Some(witness)),
        SourceCapture::Unreadable => (None, None),
    }
}

#[derive(Clone, Default)]
pub(crate) struct CapturedSendEvidence {
    pub(crate) anchor: Option<SendEvidenceAnchor>,
    pub(crate) live_binding: Option<SendLiveBinding>,
    pub(crate) turn_normalization: crate::agent::provider::TurnEvidenceNormalization,
    pub(crate) absence_witness: Option<Arc<str>>,
    pub(crate) session_starts_on_first_user_turn: bool,
    pub(crate) deferred_first_turn: Option<DeferredFirstTurnEvidence>,
}

/// Exact payloads consume one send each; an anchored turn may also confirm one
/// ordered group when its content is the concatenation of those payloads.
pub(crate) fn scan_transcript_evidence(
    registry: &AgentRegistry,
    ledger: &ChatSendLedger,
    tab_id: &str,
    mut on_confirmed: impl FnMut(ChatSendStatus),
) {
    let candidates = ledger.evidence_candidates(tab_id);
    let structured_windows = candidates
        .iter()
        .map(|candidate| structured_evidence_records(registry, &candidate.anchor))
        .collect::<Vec<_>>();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        let turns = structured_windows[candidate_index]
            .as_ref()
            .map(|turns| {
                turns
                    .iter()
                    .cloned()
                    .map(|turn| (turn, true))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                window_evidence_records(registry, &candidate.anchor)
                    .into_iter()
                    .map(|content| (StructuredUserTurn::without_identity(content), false))
                    .collect()
            });
        log::debug!(
            "[session-turn-evidence] scan tab={} send={} session={} window={:?} user_turns={} payload_bytes={}",
            tab_id, candidate.send_id, candidate.anchor.session_id,
            candidate.anchor.window, turns.len(), candidate.payload.len(),
        );
        for (turn, has_structured_identity) in turns {
            let eligible_send_ids = if has_structured_identity && turn.id.is_some() {
                candidates
                    .iter()
                    .enumerate()
                    .filter(|(index, other)| {
                        same_evidence_source(&candidate.anchor, &other.anchor)
                            && structured_windows[*index].as_ref().is_some_and(|window| {
                                window.iter().any(|other_turn| {
                                    other_turn.id == turn.id && other_turn.content == turn.content
                                })
                            })
                    })
                    .map(|(_, other)| other.send_id.clone())
                    .collect::<Vec<_>>()
            } else {
                candidates
                    .iter()
                    .filter(|other| other.anchor == candidate.anchor)
                    .map(|other| other.send_id.clone())
                    .collect::<Vec<_>>()
            };
            let confirmed = ledger.confirm_merged_evidence_turn(
                tab_id,
                &candidate.anchor,
                &turn.content,
                &eligible_send_ids,
            );
            for status in confirmed {
                on_confirmed(status);
            }
        }
    }
    for candidate in ledger.evidence_candidates(tab_id) {
        let anchor = &candidate.anchor;
        let matching_turns =
            unconsumed_matching_turns(registry, ledger, tab_id, anchor, &candidate.payload);
        let claimed = ledger.claimed_turn_anchors(tab_id, &anchor.session_id, &candidate.payload);
        let assignable = matching_turns > claimed.len()
            || nested_windows_assign_every_send(
                claimed
                    .iter()
                    .map(|claimed| {
                        unconsumed_matching_turns(
                            registry,
                            ledger,
                            tab_id,
                            claimed,
                            &candidate.payload,
                        )
                    })
                    .chain(std::iter::once(matching_turns))
                    .collect(),
            );
        if assignable {
            if let Some(status) = ledger.confirm_send(&candidate.send_id) {
                on_confirmed(status);
            }
        }
    }
}

fn unconsumed_matching_turns(
    registry: &AgentRegistry,
    ledger: &ChatSendLedger,
    tab_id: &str,
    anchor: &SendEvidenceAnchor,
    payload: &str,
) -> usize {
    let turn_contents = structured_evidence_records(registry, anchor)
        .map(|turns| turns.into_iter().map(|turn| turn.content).collect())
        .unwrap_or_else(|| window_evidence_records(registry, anchor));
    let mut matching_turns = 0;
    let mut seen_turns = HashMap::<String, usize>::new();
    for content in &turn_contents {
        if !contains_written_payload(content, payload) {
            continue;
        }
        let occurrence = seen_turns.entry(content.clone()).or_default();
        *occurrence += 1;
        if *occurrence > ledger.consumed_merged_turn_count(tab_id, anchor, content) {
            matching_turns += 1;
        }
    }
    matching_turns
}

/// Same-session append windows are nested suffixes, so every send owns a distinct
/// turn iff the k-th smallest window holds at least k matching turns.
fn nested_windows_assign_every_send(mut window_turns: Vec<usize>) -> bool {
    window_turns.sort_unstable();
    window_turns
        .iter()
        .enumerate()
        .all(|(rank, turns)| *turns > rank)
}

fn same_evidence_source(left: &SendEvidenceAnchor, right: &SendEvidenceAnchor) -> bool {
    left.provider_id == right.provider_id
        && left.cwd == right.cwd
        && left.session_id == right.session_id
        && left.turn_normalization == right.turn_normalization
}

impl ServiceContext {
    /// Capture an append-window anchor or capability-scoped first-turn evidence.
    /// Sessionless evidence resolves only after the tab authenticates its launch session.
    pub(crate) fn chat_send_anchor(&self, tab_id: &str) -> CapturedSendEvidence {
        let captured = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).map(|tab| {
                (
                    tab.agent_type.clone(),
                    tab.cwd.clone().unwrap_or_default(),
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                )
            })
        };
        let Some((provider_id, cwd, session_id, binding_nonce)) = captured else {
            return CapturedSendEvidence::default();
        };
        let live_binding =
            (session_id.is_some() || binding_nonce.is_some()).then(|| SendLiveBinding {
                session_id: session_id.clone(),
                binding_nonce: binding_nonce.clone(),
            });
        let turn_normalization = provider_id
            .as_deref()
            .and_then(|provider_id| {
                self.agent_registry.with_provider(provider_id, |provider| {
                    provider.turn_evidence_normalization()
                })
            })
            .unwrap_or_default();
        let session_starts_on_first_user_turn = provider_id
            .as_deref()
            .and_then(|provider_id| {
                self.agent_registry.with_provider(provider_id, |provider| {
                    provider.session_starts_on_first_user_turn()
                })
            })
            .unwrap_or(false);
        let deferred_first_turn = match (&provider_id, &session_id, &binding_nonce) {
            (Some(provider_id), None, Some(binding_nonce)) if session_starts_on_first_user_turn => {
                Some(DeferredFirstTurnEvidence {
                    provider_id: provider_id.clone(),
                    cwd: cwd.clone(),
                    binding_nonce: binding_nonce.clone(),
                })
            }
            _ => None,
        };
        log::debug!(
            "[session-turn-evidence] binding-capture tab={} provider={:?} session={:?} launch={:?} deferred_first_turn={}",
            tab_id, provider_id, session_id, binding_nonce, deferred_first_turn.is_some(),
        );
        let (Some(provider_id), Some(session_id)) = (provider_id, session_id) else {
            return CapturedSendEvidence {
                anchor: None,
                live_binding,
                turn_normalization,
                absence_witness: None,
                session_starts_on_first_user_turn,
                deferred_first_turn,
            };
        };
        let structured = self
            .agent_registry
            .with_provider(&provider_id, |provider| provider.uses_structured_chat())
            .unwrap_or(false);
        let (window, absence_witness) = if structured {
            let chat = self
                .agent_registry
                .with_provider(&provider_id, |provider| {
                    provider.read_structured_chat(&cwd, &session_id, None)
                })
                .unwrap_or_default();
            log::debug!(
                "[session-turn-evidence] structured-anchor tab={} provider={} session={} cursor={:?} rows={}",
                tab_id, provider_id, session_id, chat.cursor, chat.messages.len(),
            );
            (
                chat.cursor
                    .as_ref()
                    .and_then(|_| structured_boundary(&chat)),
                None,
            )
        } else {
            let path = self
                .agent_registry
                .with_provider(&provider_id, |provider| {
                    provider.session_jsonl_path(&cwd, &session_id)
                })
                .flatten();
            let source = self.chat_registry.jsonl_source(tab_id);
            jsonl_boundary(path, &source, &session_id)
        };
        log::debug!(
            "[session-turn-evidence] append-capture tab={} provider={} session={} window={:?} absence_witness={}",
            tab_id, provider_id, session_id, window, absence_witness.is_some(),
        );
        let anchor = window.map(|window| SendEvidenceAnchor {
            provider_id,
            cwd,
            session_id: session_id.clone(),
            window,
            turn_normalization,
        });
        CapturedSendEvidence {
            anchor,
            live_binding,
            turn_normalization,
            absence_witness,
            session_starts_on_first_user_turn,
            deferred_first_turn,
        }
    }

    /// Reconcile unconfirmed sends against their recorded session append
    /// windows, emitting a `chat-send-status` update for each confirmation.
    pub(crate) fn reconcile_chat_send_evidence(&self, tab_id: &str) {
        scan_transcript_evidence(
            &self.agent_registry,
            &self.chat_send_ledger,
            tab_id,
            |status| {
                let session_id = self
                    .chat_send_ledger
                    .evidence_session_for_send(&status.send_id);
                if let Some(session_id) = session_id {
                    self.note_session_conversation(tab_id, &session_id);
                }
                log::info!(
                    "[chat-send] send_id={} tab={} confirmed by persisted session turn evidence",
                    status.send_id,
                    status.tab_id
                );
                codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
            },
        );
        for (status, session_id) in deferred_first_turn::reconcile(self, tab_id) {
            self.note_session_conversation(tab_id, &session_id);
            log::info!(
                "[chat-send] send_id={} tab={} session={} confirmed by persisted session turn evidence",
                status.send_id,
                status.tab_id,
                session_id
            );
            codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
        self.settle_pending_delivery_receipt(tab_id);
    }

    pub(crate) async fn reconcile_chat_send_hook(&self, tab_id: &str) {
        let service = self.clone();
        let tab_id = tab_id.to_string();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            service.reconcile_chat_send_evidence(&tab_id);
            service.record_provider_conversation(&tab_id);
        })
        .await
        {
            log::warn!("[chat-send] hook reconciliation failed: {}", error);
        }
    }
}

#[cfg(test)]
mod tests;
