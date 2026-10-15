use super::{ChatSendLedger, ChatSendLedgerState, ChatSendState, ChatSendStatus};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const EVIDENCE_WINDOW: Duration = Duration::from_secs(60);

pub(super) struct EvidenceDeadline {
    at: Instant,
    _completion: mpsc::Sender<()>,
}

impl ChatSendLedger {
    pub fn bind_settlement_owner(&self, service: &Arc<crate::service::ServiceContext>) {
        *self.owner.lock() = Some(Arc::downgrade(service));
    }

    pub(super) fn arm_written_deadline(&self, state: &mut ChatSendLedgerState, send_id: &str) {
        let now = Instant::now();
        let receiver = arm_at(state, send_id, now);
        let Some(owner) = self.owner.lock().clone() else {
            return;
        };
        let state = Arc::downgrade(&self.state);
        let send_id = send_id.to_owned();
        let deadline = now + EVIDENCE_WINDOW;
        std::thread::spawn(move || {
            if receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                != Err(mpsc::RecvTimeoutError::Timeout)
            {
                return;
            }
            let Some(state) = state.upgrade() else {
                return;
            };
            let status = expire_at(&mut state.lock(), &send_id, Instant::now());
            if let (Some(status), Some(owner)) = (status, owner.upgrade()) {
                owner.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(&status).unwrap_or_default(),
                );
                owner.wake_user_owner(
                    &status.tab_id,
                    crate::service::parked_user_delivery::UserWake::Housekeeping,
                );
            }
        });
    }
}

fn arm_at(state: &mut ChatSendLedgerState, send_id: &str, now: Instant) -> mpsc::Receiver<()> {
    let (completion, receiver) = mpsc::channel();
    state.written_deadlines.insert(
        send_id.to_owned(),
        EvidenceDeadline {
            at: now + EVIDENCE_WINDOW,
            _completion: completion,
        },
    );
    receiver
}

fn expire_at(
    state: &mut ChatSendLedgerState,
    send_id: &str,
    now: Instant,
) -> Option<ChatSendStatus> {
    if state.written_deadlines.get(send_id)?.at > now {
        return None;
    }
    state.written_deadlines.remove(send_id);
    let status = state.by_id.get_mut(send_id)?;
    if status.state != ChatSendState::Written {
        return None;
    }
    status.state = ChatSendState::Unknown;
    status.error = None;
    status.pending_reason = None;
    log::debug!("[chat-send] send_id={send_id} evidence deadline ended without an exact turn");
    let status = status.clone();
    Some(ChatSendLedger::with_retention_window(state, status))
}

#[cfg(test)]
mod tests;
