use super::*;

impl ChatSendLedger {
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
        evidence: crate::service::chat_send_evidence::CapturedSendEvidence,
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
        if status.state != ChatSendState::Pending {
            return None;
        }
        status.state = ChatSendState::Written;
        crate::flow_trace::record(send_id, "send.written", &status.tab_id, "", "pty", None);
        status.error = None;
        status.pending_reason = None;
        let status = status.clone();
        self.arm_written_deadline(&mut state, send_id);
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
}
