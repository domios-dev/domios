use std::sync::{Arc, Weak};

use super::{normalize_turn_evidence_with, ChatSendLedger, ChatSendState, ChatSendStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TurnPosition {
    pub(crate) record_start: Option<u64>,
    pub(crate) record_row: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct LiveProvenance {
    pub(crate) source_session_id: String,
    pub(crate) source_binding_nonce: Option<String>,
    pub(crate) range: crate::service::tab_chat::tailer::SourceRange,
    pub(crate) positions: Vec<TurnPosition>,
}

pub(super) struct LiveEvidenceProgress {
    session_id: String,
    binding_nonce: String,
    incarnation_id: Arc<str>,
    evidence_scope_id: Arc<str>,
    end_offset: u64,
}

pub(super) struct ConsumedTurnWatermark {
    session_id: String,
    binding_nonce: String,
    incarnation_id: Arc<str>,
    evidence_scope_id: Arc<str>,
    record_start: u64,
    record_row: u32,
    after: u64,
}

impl ChatSendLedger {
    pub(super) fn advance_live_evidence(
        &self,
        tab_id: &str,
        session_id: &str,
        nonce: Option<&str>,
        provenance: &LiveProvenance,
    ) -> bool {
        let Some(nonce) = nonce else {
            return false;
        };
        if provenance.source_session_id != session_id
            || provenance.source_binding_nonce.as_deref() != Some(nonce)
        {
            return false;
        }
        let Some(incarnation) = provenance.range.incarnation.as_ref() else {
            return false;
        };
        let source_witness = incarnation.absence_witness.as_ref();
        let range = &provenance.range;
        if range.offset <= range.start_offset {
            return false;
        }

        let mut state = self.state.lock();
        let eligible = state.order.iter().any(|id| {
            let Some((authority, active)) = state.owned.get(id) else {
                return false;
            };
            let binding_matches = state.live_binding_by_id.get(id).is_none_or(|binding| {
                binding
                    .session_id
                    .as_deref()
                    .is_none_or(|session| session == session_id)
                    && binding
                        .binding_nonce
                        .as_deref()
                        .is_none_or(|bound_nonce| bound_nonce == nonce)
            });
            let anchored = state.anchor_by_id.get(id).is_some_and(|anchor| {
                anchor.session_id == session_id
                    && matches!(
                        &anchor.window,
                        super::EvidenceWindow::Jsonl { transcript_offset }
                            if range.offset > *transcript_offset
                    )
            });
            let absence_witness_matches = source_witness.is_some_and(|witness| {
                state
                    .absence_witness_by_id
                    .get(id)
                    .is_some_and(|stored| stored == witness)
            });
            let lazy_sessionless = source_witness.is_none()
                && state.sessionless_first_turn_by_id.contains(id)
                && authority.session_id.is_none()
                && authority.observed_session_id.is_none()
                && state.live_binding_by_id.get(id).is_some_and(|binding| {
                    binding.session_id.is_none() && binding.binding_nonce.as_deref() == Some(nonce)
                });
            *active
                && authority.tab_id == tab_id
                && authority.binding_nonce == nonce
                && binding_matches
                && (anchored || absence_witness_matches || lazy_sessionless)
        });
        if !eligible {
            return false;
        }
        let evidence_scope_id = source_witness
            .cloned()
            .unwrap_or_else(|| incarnation.id.clone());

        let same_incarnation =
            state
                .live_evidence_progress_by_tab
                .get(tab_id)
                .is_some_and(|progress| {
                    progress.session_id == session_id
                        && progress.binding_nonce == nonce
                        && progress.incarnation_id == incarnation.id
                        && progress.evidence_scope_id == evidence_scope_id
                });
        if let Some(progress) = state.live_evidence_progress_by_tab.get(tab_id) {
            if same_incarnation && range.start_offset < progress.end_offset {
                return false;
            }
            if !same_incarnation && progress.evidence_scope_id == evidence_scope_id {
                return false;
            }
        }
        let retires_watermark = state
            .consumed_turn_by_tab
            .get(tab_id)
            .is_some_and(|watermark| {
                watermark.session_id == session_id
                    && watermark.binding_nonce == nonce
                    && watermark.incarnation_id == incarnation.id
                    && watermark.evidence_scope_id == evidence_scope_id
                    && range.start_offset >= watermark.after
            });
        if retires_watermark {
            state.consumed_turn_by_tab.remove(tab_id);
        }
        state.live_evidence_progress_by_tab.insert(
            tab_id.to_string(),
            LiveEvidenceProgress {
                session_id: session_id.to_string(),
                binding_nonce: nonce.to_string(),
                incarnation_id: incarnation.id.clone(),
                evidence_scope_id,
                end_offset: range.offset,
            },
        );
        true
    }

    pub(super) fn confirm_owned_live_turn(
        &self,
        tab_id: &str,
        session_id: &str,
        nonce: Option<&str>,
        message: &crate::agent::chat_types::ChatMsg,
        position: TurnPosition,
        provenance: &LiveProvenance,
    ) -> Option<ChatSendStatus> {
        let nonce = nonce?;
        let record_start = position.record_start?;
        let incarnation = provenance.range.incarnation.as_ref()?;
        let source_witness = incarnation.absence_witness.as_ref();
        if provenance.source_session_id != session_id
            || provenance.source_binding_nonce.as_deref() != Some(nonce)
            || record_start < provenance.range.start_offset
            || record_start >= provenance.range.offset
        {
            return None;
        }
        let candidate = {
            let state = self.state.lock();
            let progress = state.live_evidence_progress_by_tab.get(tab_id)?;
            let evidence_scope_id = source_witness
                .cloned()
                .unwrap_or_else(|| incarnation.id.clone());
            if progress.session_id != session_id
                || progress.binding_nonce != nonce
                || progress.incarnation_id != incarnation.id
                || progress.evidence_scope_id != evidence_scope_id
                || progress.end_offset != provenance.range.offset
            {
                return None;
            }
            if state
                .consumed_turn_by_tab
                .get(tab_id)
                .is_some_and(|watermark| {
                    watermark.session_id == session_id
                        && watermark.binding_nonce == nonce
                        && watermark.incarnation_id == incarnation.id
                        && watermark.evidence_scope_id == evidence_scope_id
                        && (record_start < watermark.record_start
                            || (record_start == watermark.record_start
                                && position.record_row <= watermark.record_row))
                })
            {
                return None;
            }
            state.order.iter().find_map(|id| {
                let (authority, active) = state.owned.get(id)?;
                let status = state.by_id.get(id)?;
                let binding_matches = state.live_binding_by_id.get(id).is_none_or(|binding| {
                    binding
                        .session_id
                        .as_deref()
                        .is_none_or(|session| session == session_id)
                        && binding
                            .binding_nonce
                            .as_deref()
                            .is_none_or(|bound_nonce| bound_nonce == nonce)
                });
                let anchored = state.anchor_by_id.get(id).is_some_and(|anchor| {
                    anchor.session_id == session_id
                        && matches!(
                            &anchor.window,
                            super::EvidenceWindow::Jsonl { transcript_offset }
                                if record_start >= *transcript_offset
                        )
                });
                let absence_witness_matches = source_witness.is_some_and(|witness| {
                    state
                        .absence_witness_by_id
                        .get(id)
                        .is_some_and(|stored| stored == witness)
                });
                let lazy_sessionless = source_witness.is_none()
                    && state.sessionless_first_turn_by_id.contains(id)
                    && authority.session_id.is_none()
                    && authority.observed_session_id.is_none()
                    && state.live_binding_by_id.get(id).is_some_and(|binding| {
                        binding.session_id.is_none()
                            && binding.binding_nonce.as_deref() == Some(nonce)
                    });
                (*active
                    && authority.tab_id == tab_id
                    && authority.binding_nonce == nonce
                    && matches!(
                        status.state,
                        ChatSendState::Written | ChatSendState::Unknown
                    )
                    && binding_matches
                    && (anchored || absence_witness_matches || lazy_sessionless))
                    .then(|| {
                        (
                            id.clone(),
                            authority.clone(),
                            *active,
                            state.payload_by_id.get(id).cloned(),
                            state.turn_normalization_by_id.get(id).copied(),
                            evidence_scope_id.clone(),
                        )
                    })
            })
        }?;
        let (send_id, authority, active, expected, normalization, evidence_scope_id) = candidate;
        let text = normalize_turn_evidence_with(&message.content, normalization?);
        if text.is_empty() || expected.as_deref() != Some(text.as_str()) {
            return None;
        }
        let owner = self.owner.lock().as_ref().and_then(Weak::upgrade)?;
        if !active || !owner.user_authority_is_current_session(&authority, session_id, nonce) {
            return None;
        }

        let mut state = self.state.lock();
        let still_owned = state
            .owned
            .get(&send_id)
            .is_some_and(|(current, active)| *active && current == &authority);
        let still_current =
            state
                .live_evidence_progress_by_tab
                .get(tab_id)
                .is_some_and(|progress| {
                    progress.session_id == session_id
                        && progress.binding_nonce == nonce
                        && progress.incarnation_id == incarnation.id
                        && progress.evidence_scope_id == evidence_scope_id
                        && progress.end_offset == provenance.range.offset
                });
        if !still_owned || !still_current {
            return None;
        }
        let status = Self::mark_sent(&mut state, &send_id)?;
        state.consumed_turn_by_tab.insert(
            tab_id.to_string(),
            ConsumedTurnWatermark {
                session_id: session_id.to_string(),
                binding_nonce: nonce.to_string(),
                incarnation_id: incarnation.id.clone(),
                evidence_scope_id,
                record_start,
                record_row: position.record_row,
                after: record_start.saturating_add(1),
            },
        );
        let status = Self::with_retention_window(&state, status);
        drop(state);
        self.wake_owner(tab_id);
        Some(status)
    }
}
