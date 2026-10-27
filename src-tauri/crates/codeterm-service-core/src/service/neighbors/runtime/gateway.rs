use super::Metadata;
use crate::service::{
    messenger::{Delivery, MessageEnvelope, Messenger},
    ServiceContext,
};
use async_trait::async_trait;
use codeterm_neighbors::{
    gateway::{resume, Admission as Receipt, AdmissionState as ReceiptState, Resume},
    receive::{GatewayReceipt, NeighborGateway},
    wire::InboxMessage,
    Error, Result,
};
use zeroize::Zeroizing;

pub(super) struct Gateway {
    pub service: ServiceContext,
    pub metadata: Metadata,
    pub friendship: String,
    pub tab: String,
}

impl ServiceContext {
    pub(super) fn validate_neighbor_gateway(
        &self,
        tab_id: &str,
    ) -> std::result::Result<(), String> {
        let mux = self.mux.read();
        let tab = mux
            .get_tab(tab_id)
            .ok_or("Neighbor gateway tab is missing")?;
        if tab.agent_type.is_none()
            || crate::service::general_agent::is_general_agent(tab)
            || tab.team_id.is_some()
            || tab.role.is_some()
        {
            return Err("Choose an independent agent tab for this Neighbor".into());
        }
        Ok(())
    }
}

#[async_trait]
impl NeighborGateway for Gateway {
    async fn deliver(
        &self,
        message: &InboxMessage,
        plaintext: Zeroizing<String>,
    ) -> Result<GatewayReceipt> {
        self.service
            .validate_neighbor_gateway(&self.tab)
            .map_err(|_| Error::Gateway)?;
        let events = self.service.events.subscribe();
        let path = format!("gateway-{}.json", self.friendship);
        let previous: Option<Receipt> = self.metadata.read(path.clone()).await?;
        match resume(previous.as_ref(), &message.id, &self.tab) {
            Resume::Delivered => return Ok(GatewayReceipt::Delivered),
            Resume::Wait(delivery_id) => {
                return self.await_pending(message, path, delivery_id, events).await
            }
            Resume::WrongGateway => return Err(Error::Gateway),
            Resume::Unconfirmed => return Ok(GatewayReceipt::SubmitUnconfirmed),
            Resume::Admit => {}
        }
        let permission = self
            .metadata
            .settings()
            .await?
            .permissions
            .get(&self.friendship)
            .cloned()
            .unwrap_or_default();
        let hint = permission.hint(message.doc.envelope.kind);
        let body = Zeroizing::new(format!("{hint}\nFriendship: {}\nMessage: {}\nReply through the encrypted Neighbor channel with this message as in_reply_to; there is no local tab return address.\n\n{}",
            self.friendship, message.doc.envelope.message_id, plaintext.as_str()));
        let envelope = MessageEnvelope {
            from_label: message.doc.envelope.sender_uid.clone(),
            from_neighbor: Some(self.friendship.clone()),
            id: Some(format!("neighbor:{}", message.id)),
            context: Some("neighbor-request".into()),
            ..Default::default()
        };
        let framed_bytes = Messenger::from(envelope.clone())
            .framed_delivery(&body)
            .to_message()
            .len();
        let limit = {
            let mux = self.service.mux.read();
            let provider = mux
                .get_tab(&self.tab)
                .and_then(|tab| tab.agent_type.as_deref())
                .ok_or(Error::Gateway)?;
            self.service
                .agent_registry
                .with_provider(provider, |provider| provider.paste_limit_bytes())
                .flatten()
        };
        if limit.is_some_and(|limit| framed_bytes > limit) {
            return Err(Error::Gateway);
        }
        self.save_receipt(message, &path, ReceiptState::Claimed)
            .await?;
        let service = self.service.clone();
        let tab = self.tab.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            Messenger::deliver(&service, &tab, envelope, &body).map_err(|error| {
                crate::service::delivery_error::DeliveryError::from_opaque(&tab, error)
                    .bytes_written()
            })
        })
        .await
        .map_err(|_| Error::Gateway)?;
        match outcome {
            Ok(Delivery::Pty { .. } | Delivery::ChatBackend { .. }) => {
                self.save_receipt(message, &path, ReceiptState::Delivered)
                    .await?;
                Ok(GatewayReceipt::Delivered)
            }
            Ok(Delivery::Pending { delivery_id }) => {
                self.save_receipt(
                    message,
                    &path,
                    ReceiptState::Pending {
                        delivery_id: delivery_id.clone(),
                    },
                )
                .await?;
                self.await_pending(message, path, delivery_id, events).await
            }
            Ok(Delivery::Unconfirmed { .. }) | Err(true) => {
                self.save_receipt(message, &path, ReceiptState::SubmitUnconfirmed)
                    .await?;
                Ok(GatewayReceipt::SubmitUnconfirmed)
            }
            Err(false) => {
                self.save_receipt(message, &path, ReceiptState::PrewriteRefused)
                    .await?;
                Err(Error::Gateway)
            }
        }
    }
}

impl Gateway {
    async fn save_receipt(
        &self,
        message: &InboxMessage,
        path: &str,
        state: ReceiptState,
    ) -> Result<()> {
        self.metadata
            .write(
                path.into(),
                Receipt {
                    message_id: message.id.clone(),
                    tab: self.tab.clone(),
                    state,
                },
            )
            .await
    }

    async fn await_pending(
        &self,
        message: &InboxMessage,
        path: String,
        delivery_id: String,
        mut events: tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>,
    ) -> Result<GatewayReceipt> {
        let wait = async {
            loop {
                if self
                    .service
                    .chat_send_ledger
                    .status(&delivery_id)
                    .is_some_and(|status| {
                        matches!(
                            status.state,
                            crate::service::chat_send_transaction::ChatSendState::Written
                                | crate::service::chat_send_transaction::ChatSendState::Sent
                        )
                    })
                {
                    return Ok(());
                }
                let event = events.recv().await.map_err(|_| Error::Gateway)?;
                if let codeterm_events::SystemEvent::Generic { event_type, data } = event.event {
                    if event_type == "delivery-written"
                        && data["tab_id"] == self.tab
                        && data["delivery_id"] == delivery_id
                    {
                        return Ok(());
                    }
                    if event_type == "chat-send-status"
                        && data["send_id"] == delivery_id
                        && matches!(
                            data["state"].as_str(),
                            Some("failed" | "submit_unconfirmed")
                        )
                    {
                        return Err(Error::Gateway);
                    }
                }
            }
        };
        match tokio::time::timeout(std::time::Duration::from_secs(90), wait).await {
            Ok(Ok(())) => {
                self.save_receipt(message, &path, ReceiptState::Delivered)
                    .await?;
                Ok(GatewayReceipt::Delivered)
            }
            _ => Ok(GatewayReceipt::SubmitUnconfirmed),
        }
    }
}
