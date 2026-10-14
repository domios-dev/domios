use super::*;
use crate::service::pending_delivery_payloads::user_sends::{
    continuity::{ReceiptState, UserSendReceipt},
    UserSendState,
};

impl ChatSendLedger {
    pub(crate) fn reserved_user_status(&self, reservation: &UserReservation) -> ChatSendStatus {
        let state = self.state.lock();
        ChatSendStatus {
            send_id: reservation.send_id.clone(),
            tab_id: reservation.authority.tab_id.clone(),
            state: ChatSendState::Pending,
            error: None,
            ledger_id: state.ledger_id.clone(),
            sequence: reservation.ledger_sequence,
            retained_from: reservation.ledger_sequence,
            correlation_id: reservation.client_token.clone(),
            pending_reason: None,
            system_send: None,
        }
    }

    pub(crate) fn admit_user(&self, record: &UserSendRecord) -> ChatSendStatus {
        Self::admit_user_locked(&mut self.state.lock(), record)
    }

    fn admit_user_locked(
        state: &mut ChatSendLedgerState,
        record: &UserSendRecord,
    ) -> ChatSendStatus {
        state.next_sequence = state
            .next_sequence
            .max(record.ledger_sequence.saturating_add(1));
        if !state.by_id.contains_key(&record.send_id) {
            crate::flow_trace::record(
                &record.send_id,
                "send.started",
                &record.authority.tab_id,
                &record.authority.provider_id,
                "accepted",
                None,
            );
        }
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
            system_send: None,
        };
        state
            .owned
            .insert(record.send_id.clone(), (record.authority.clone(), false));
        if record.held_before_readiness {
            state
                .held_before_readiness_by_id
                .insert(record.send_id.clone(), record.authority.clone());
        } else {
            state.held_before_readiness_by_id.remove(&record.send_id);
        }
        let latest_sequence = state
            .latest_by_tab
            .get(&status.tab_id)
            .and_then(|id| state.by_id.get(id))
            .map_or(0, |latest| latest.sequence);
        if status.sequence >= latest_sequence {
            state
                .latest_by_tab
                .insert(status.tab_id.clone(), status.send_id.clone());
        }
        if !state.by_id.contains_key(&status.send_id) {
            let position = state.order.iter().position(|id| {
                state
                    .by_id
                    .get(id)
                    .is_some_and(|prior| prior.sequence > status.sequence)
            });
            if let Some(position) = position {
                state.order.insert(position, status.send_id.clone());
            } else {
                state.order.push_back(status.send_id.clone());
            }
        }
        state.anchor_by_id.remove(&status.send_id);
        state.live_binding_by_id.remove(&status.send_id);
        state.absence_witness_by_id.remove(&status.send_id);
        state.sessionless_first_turn_by_id.remove(&status.send_id);
        state.deferred_first_turn_by_id.remove(&status.send_id);
        state.terminal_failures.remove(&status.send_id);
        state.payload_by_id.remove(&status.send_id);
        state.by_id.insert(status.send_id.clone(), status.clone());
        if let Some(token) = &record.client_token {
            state
                .token_by_id
                .insert(status.send_id.clone(), token.clone());
        }
        Self::prune_terminal(state);
        Self::with_retention_window(state, status)
    }

    pub(crate) fn restore_user_record(&self, record: &UserSendRecord) {
        let mut state = self.state.lock();
        if state.by_id.contains_key(&record.send_id) {
            return;
        }
        Self::admit_user_locked(&mut state, record);
        if record.state != UserSendState::Queued {
            state.owned.remove(&record.send_id);
            state.held_before_readiness_by_id.remove(&record.send_id);
            if let Some(status) = state.by_id.get_mut(&record.send_id) {
                status.state = ChatSendState::Unknown;
            }
        }
    }

    pub(crate) fn restore_user_receipt(&self, receipt: &UserSendReceipt) {
        let mut state = self.state.lock();
        if let Some(current) = state.by_id.get(&receipt.send_id) {
            let active = state
                .owned
                .get(&receipt.send_id)
                .is_some_and(|(_, active)| *active);
            if receipt_ends_held_status(&current.state, active, receipt.state) {
                let Some(status) = state.by_id.get_mut(&receipt.send_id) else {
                    return;
                };
                status.state = ChatSendState::Failed;
                status.error = Some(receipt_failure_reason(receipt.state).into());
                status.pending_reason = None;
                state.owned.remove(&receipt.send_id);
                state.held_before_readiness_by_id.remove(&receipt.send_id);
                state.terminal_failures.insert(receipt.send_id.clone());
                state
                    .latest_failure_by_tab
                    .insert(receipt.tab_id.clone(), receipt.send_id.clone());
            }
            return;
        }
        let (send_state, error) = match receipt.state {
            ReceiptState::Failed | ReceiptState::Degraded => (
                ChatSendState::Failed,
                Some(receipt_failure_reason(receipt.state).into()),
            ),
            ReceiptState::Uncertain => (ChatSendState::Unknown, None),
            ReceiptState::Delivered => (ChatSendState::Sent, None),
        };
        let status = ChatSendStatus {
            send_id: receipt.send_id.clone(),
            tab_id: receipt.tab_id.clone(),
            state: send_state.clone(),
            error,
            ledger_id: state.ledger_id.clone(),
            sequence: receipt.ledger_sequence,
            retained_from: receipt.ledger_sequence,
            correlation_id: receipt.client_token.clone(),
            pending_reason: None,
            system_send: None,
        };
        state.next_sequence = state
            .next_sequence
            .max(receipt.ledger_sequence.saturating_add(1));
        let position = state.order.iter().position(|id| {
            state
                .by_id
                .get(id)
                .is_some_and(|prior| prior.sequence > receipt.ledger_sequence)
        });
        if let Some(position) = position {
            state.order.insert(position, receipt.send_id.clone());
        } else {
            state.order.push_back(receipt.send_id.clone());
        }
        if let Some(token) = &receipt.client_token {
            state
                .token_by_id
                .insert(receipt.send_id.clone(), token.clone());
        }
        if send_state == ChatSendState::Failed {
            state.terminal_failures.insert(receipt.send_id.clone());
            state
                .latest_failure_by_tab
                .insert(receipt.tab_id.clone(), receipt.send_id.clone());
        }
        state.by_id.insert(receipt.send_id.clone(), status);
        Self::prune_terminal(&mut state);
    }
}

fn receipt_failure_reason(receipt: ReceiptState) -> &'static str {
    match receipt {
        ReceiptState::Degraded => "launch_degraded_before_write",
        _ => "launch_ended_before_write",
    }
}

fn receipt_ends_held_status(current: &ChatSendState, active: bool, receipt: ReceiptState) -> bool {
    *current == ChatSendState::Pending
        && !active
        && matches!(receipt, ReceiptState::Failed | ReceiptState::Degraded)
}

#[cfg(test)]
mod tests;
