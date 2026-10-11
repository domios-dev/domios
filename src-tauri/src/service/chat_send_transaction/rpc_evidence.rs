use super::{ChatSendLedger, ChatSendState, ChatSendStatus, SendLiveBinding};

impl ChatSendLedger {
    pub(crate) fn trace_rpc_turn(
        &self,
        tab: &str,
        thread: &str,
        nonce: &str,
        client_id: &str,
        turn: &str,
        provider: &str,
    ) {
        if !crate::flow_trace::enabled() {
            return;
        }
        let state = self.state.lock();
        if state
            .by_id
            .get(client_id)
            .is_some_and(|status| status.tab_id == tab)
            && state.rpc_by_id.get(client_id).is_some_and(|binding| {
                binding.session_id.as_deref() == Some(thread)
                    && binding.binding_nonce.as_deref() == Some(nonce)
            })
        {
            crate::flow_trace::link_rpc(client_id, tab, provider, thread, nonce, turn);
        }
    }

    pub(crate) fn has_rpc_handoff(&self, send_id: &str) -> bool {
        self.state.lock().rpc_by_id.contains_key(send_id)
    }

    pub(crate) fn bind_rpc_send(&self, send_id: &str, binding: SendLiveBinding) -> bool {
        let mut state = self.state.lock();
        if !state
            .by_id
            .get(send_id)
            .is_some_and(|status| status.state == ChatSendState::Pending)
            || state.rpc_by_id.contains_key(send_id)
        {
            return false;
        }
        state.payload_by_id.remove(send_id);
        state.anchor_by_id.remove(send_id);
        state.absence_witness_by_id.remove(send_id);
        state.deferred_first_turn_by_id.remove(send_id);
        state.sessionless_first_turn_by_id.remove(send_id);
        state
            .live_binding_by_id
            .insert(send_id.into(), binding.clone());
        state.rpc_by_id.insert(send_id.into(), binding.clone());
        true
    }

    pub(crate) fn clear_rpc_send_after_definite_refusal(
        &self,
        send_id: &str,
        expected: &SendLiveBinding,
    ) -> bool {
        let mut state = self.state.lock();
        if !state
            .by_id
            .get(send_id)
            .is_some_and(|status| status.state == ChatSendState::Pending)
            || state.rpc_by_id.get(send_id) != Some(expected)
        {
            return false;
        }
        state.rpc_by_id.remove(send_id);
        state.live_binding_by_id.remove(send_id);
        true
    }

    pub(crate) fn mark_rpc_accepted(&self, send_id: &str) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        if !state.rpc_by_id.contains_key(send_id) {
            return None;
        }
        let status = state.by_id.get_mut(send_id)?;
        if status.state != ChatSendState::Pending {
            return None;
        }
        status.state = ChatSendState::Written;
        crate::flow_trace::record(send_id, "send.written", &status.tab_id, "", "rpc", None);
        status.error = None;
        status.pending_reason = None;
        let status = status.clone();
        Some(Self::with_retention_window(&state, status))
    }

    pub(crate) fn confirm_rpc_send(
        &self,
        tab: &str,
        thread: &str,
        nonce: &str,
        client_id: &str,
    ) -> Option<ChatSendStatus> {
        {
            let state = self.state.lock();
            let status = state.by_id.get(client_id)?;
            let binding = state.rpc_by_id.get(client_id)?;
            if status.tab_id != tab
                || binding.session_id.as_deref() != Some(thread)
                || binding.binding_nonce.as_deref() != Some(nonce)
            {
                return None;
            }
        }
        self.confirm_send(client_id)
    }

    pub(crate) fn mark_rpc_uncertain(&self, send_id: &str) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        if !state.rpc_by_id.contains_key(send_id) {
            return None;
        }
        let status = state.by_id.get_mut(send_id)?;
        if matches!(
            status.state,
            ChatSendState::Sent | ChatSendState::Failed | ChatSendState::Unknown
        ) {
            return None;
        }
        status.state = ChatSendState::Unknown;
        status.error = None;
        status.pending_reason = None;
        let status = status.clone();
        let status = Self::with_retention_window(&state, status);
        drop(state);
        self.wake_owner(&status.tab_id);
        Some(status)
    }

    pub(crate) fn rpc_uncertain_for_launch(
        &self,
        tab: &str,
        nonce: Option<&str>,
    ) -> Vec<ChatSendStatus> {
        let ids = {
            let state = self.state.lock();
            state
                .rpc_by_id
                .iter()
                .filter(|(id, binding)| {
                    binding.binding_nonce.as_deref() == nonce
                        && state
                            .by_id
                            .get(*id)
                            .is_some_and(|status| status.tab_id == tab)
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        ids.iter()
            .filter_map(|id| self.mark_rpc_uncertain(id))
            .collect()
    }
}

#[cfg(test)]
mod tests;
