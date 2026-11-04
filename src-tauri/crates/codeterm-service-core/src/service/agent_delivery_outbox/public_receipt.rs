use crate::service::chat_send_transaction::{ChatSendState, ChatSendStatus};
use crate::service::messenger::Delivery;
use crate::service::pending_delivery_payloads::agent_deliveries::parse_agent_delivery_send_id;
use crate::service::ServiceContext;
use std::time::Duration;

impl ServiceContext {
    pub fn public_send_restores_agent(&self, tab_id: &str) -> bool {
        self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            !tab.is_chat_backend()
                && tab.agent_type.is_some()
                && (tab.restored_from_snapshot || tab.frozen)
        })
    }

    pub async fn await_restored_send_receipt(
        &self,
        tab_id: &str,
        delivery: Delivery,
        timeout: Duration,
    ) -> Delivery {
        let delivery = self.public_delivery_receipt(tab_id, delivery);
        let Delivery::Pending { delivery_id } = &delivery else {
            return delivery;
        };
        let Some((owner_tab, raw_id)) = parse_agent_delivery_send_id(delivery_id) else {
            return delivery;
        };
        if owner_tab != tab_id {
            return delivery;
        }
        // Subscribe before inspecting persistence so a write between the two cannot be missed.
        let mut events = self.events.subscribe();
        let mut uncertain = false;
        let wait = async {
            loop {
                let inspected = self.inspect_agent_write_receipt(delivery_id).await;
                match inspected {
                    Ok((true, _)) => return true,
                    Ok((false, Some(status))) => match status.state {
                        ChatSendState::Written | ChatSendState::Sent => return true,
                        ChatSendState::Failed => return false,
                        ChatSendState::Unknown => uncertain = true,
                        ChatSendState::Pending => uncertain = false,
                    },
                    _ => uncertain = true,
                }
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            if let codeterm_events::SystemEvent::Generic { event_type, data } =
                                event.event
                            {
                                if data["tab_id"] == tab_id && data["delivery_id"] == raw_id {
                                    if event_type == "delivery-written" {
                                        return true;
                                    }
                                    if event_type == "agent-delivery-status" {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
                    }
                }
            }
        };
        let written = tokio::time::timeout(timeout, wait).await;
        if matches!(written, Ok(true)) {
            return Delivery::Pty {
                text: String::new(),
            };
        }
        if written.is_err() {
            match self.inspect_agent_write_receipt(delivery_id).await {
                Ok((true, _)) => {
                    return Delivery::Pty {
                        text: String::new(),
                    }
                }
                Ok((false, Some(status))) => match status.state {
                    ChatSendState::Written | ChatSendState::Sent => {
                        return Delivery::Pty {
                            text: String::new(),
                        }
                    }
                    ChatSendState::Unknown => uncertain = true,
                    ChatSendState::Pending | ChatSendState::Failed => uncertain = false,
                },
                _ => uncertain = true,
            }
        }
        if uncertain {
            Delivery::Unconfirmed {
                delivery_id: delivery_id.clone(),
            }
        } else {
            delivery
        }
    }

    async fn inspect_agent_write_receipt(
        &self,
        public_id: &str,
    ) -> Result<(bool, Option<ChatSendStatus>), String> {
        let store = self.pending_delivery_payloads.agent_deliveries.clone();
        let id = public_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let written = store.inspected_payload_written(&id)?;
            Ok((written, store.inspect(&id)?))
        })
        .await
        .map_err(|error| error.to_string())?
    }
}
