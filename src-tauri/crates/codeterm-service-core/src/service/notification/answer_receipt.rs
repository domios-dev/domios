use super::{AnswerDeliveryOutcome, ServiceContext};
use crate::notification::answer_delivery::AnswerHandoff;
use crate::service::{
    chat_send_transaction::{ChatSendState, ChatSendStatus},
    delivery_error::DeliveryError,
    messenger::{Delivery, MessageEnvelope, Messenger},
};
use codeterm_events::{SeqEvent, SystemEvent};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{
    broadcast::{error::RecvError, Receiver},
    Semaphore,
};

const MAX_ANSWER_WATCHES: usize = 500;

struct AnswerWatchSlots(Arc<Semaphore>);

impl Default for AnswerWatchSlots {
    fn default() -> Self {
        Self(Arc::new(Semaphore::new(MAX_ANSWER_WATCHES)))
    }
}

impl ServiceContext {
    pub(super) fn notification_answer_handoff(
        &self,
        tab: &str,
        mut envelope: MessageEnvelope,
        body: &str,
    ) -> (AnswerHandoff, Option<String>) {
        let transport_token = uuid::Uuid::new_v4().to_string();
        envelope.id = Some(transport_token.clone());
        let send_id = match Messenger::deliver(self, tab, envelope, body) {
            Ok(Delivery::ChatBackend { message_id }) => message_id,
            Ok(Delivery::Pending { delivery_id } | Delivery::Unconfirmed { delivery_id }) => {
                delivery_id
            }
            Ok(Delivery::Pty { .. }) => return (AnswerHandoff::Unconfirmed, None),
            Err(detail) => {
                if super::super::agent_runtime::requested_for_tab(self, tab) {
                    match self
                        .pending_delivery_payloads
                        .user_sends
                        .runtime_envelope_id(tab, &format!("runtime-envelope:{transport_token}"))
                    {
                        Ok(Some(send_id)) => return self.classify_answer_receipt(tab, send_id),
                        Err(_) => return (AnswerHandoff::Unconfirmed, None),
                        Ok(None) => {}
                    }
                }
                let handoff = if DeliveryError::from_opaque(tab, &detail).bytes_written() {
                    AnswerHandoff::Unconfirmed
                } else {
                    AnswerHandoff::Refused(detail)
                };
                return (handoff, None);
            }
        };
        self.classify_answer_receipt(tab, send_id)
    }

    fn classify_answer_receipt(
        &self,
        tab: &str,
        send_id: String,
    ) -> (AnswerHandoff, Option<String>) {
        let status = self.chat_send_ledger.status(&send_id);
        if status.as_ref().is_some_and(|status| status.tab_id == tab) {
            if let Some(outcome) = exact_outcome(status, tab) {
                return (outcome, None);
            }
            return (AnswerHandoff::Unconfirmed, Some(send_id));
        }
        (AnswerHandoff::Unconfirmed, None)
    }

    pub(super) fn publish_answer_delivery(
        &self,
        id: &str,
        standing: &str,
        outcome: AnswerDeliveryOutcome,
    ) -> Value {
        let result = json!({
            "id": id,
            "answered": standing,
            "delivered": outcome.reached_tab(),
            "submit_unconfirmed": matches!(outcome, AnswerDeliveryOutcome::Pending | AnswerDeliveryOutcome::SubmitUnconfirmed),
            "deliveryError": outcome.failure(),
        });
        if outcome != AnswerDeliveryOutcome::Pending {
            self.notify_event("notification-answered", result.clone());
        }
        result
    }

    pub(super) fn watch_notification_answer(
        &self,
        id: &str,
        standing: String,
        tab: String,
        send_id: String,
        mut events: Receiver<SeqEvent>,
    ) {
        let Ok(slot) =
            Arc::clone(&self.ext_or_init(AnswerWatchSlots::default).0).try_acquire_owned()
        else {
            return;
        };
        let service = self.weak_self();
        let id = id.to_owned();
        crate::async_runtime::spawn(async move {
            let _slot = slot;
            let observe = async {
                loop {
                    let svc = service.upgrade()?;
                    let outcome = exact_outcome(svc.chat_send_ledger.status(&send_id), &tab);
                    drop(svc);
                    if outcome.is_some() {
                        return outcome;
                    }
                    loop {
                        match events.recv().await {
                            Ok(event)
                                if matches!(&event.event, SystemEvent::Generic { event_type, data }
                                if event_type == "chat-send-status" && data["send_id"].as_str() == Some(send_id.as_str())) =>
                            {
                                break
                            }
                            Err(RecvError::Lagged(_)) => break,
                            Err(RecvError::Closed) => return None,
                            Ok(_) => {}
                        }
                    }
                }
            };
            let Ok(Some(outcome)) = tokio::time::timeout(Duration::from_secs(60), observe).await
            else {
                return;
            };
            let Some(svc) = service.upgrade() else { return };
            crate::async_runtime::spawn_blocking(move || {
                let outcome = match outcome {
                    AnswerHandoff::Confirmed => {
                        if !svc.notif.lock().mark_answer_delivered(&id) {
                            return;
                        }
                        AnswerDeliveryOutcome::Delivered
                    }
                    AnswerHandoff::Refused(detail) => {
                        svc.notif.lock().release_answer_delivery(&id);
                        AnswerDeliveryOutcome::Failed(detail)
                    }
                    AnswerHandoff::Unconfirmed => return,
                };
                svc.publish_answer_delivery(&id, &standing, outcome);
            });
        });
    }
}

fn exact_outcome(status: Option<ChatSendStatus>, tab: &str) -> Option<AnswerHandoff> {
    let status = status.filter(|status| status.tab_id == tab)?;
    match status.state {
        ChatSendState::Sent => Some(AnswerHandoff::Confirmed),
        ChatSendState::Failed => {
            Some(AnswerHandoff::Refused(status.error.unwrap_or_else(|| {
                "The asking tab could not receive the answer".into()
            })))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
