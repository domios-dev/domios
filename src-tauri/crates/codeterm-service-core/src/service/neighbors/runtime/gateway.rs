use super::Metadata;
use crate::service::{
    messenger::{Delivery, Messenger},
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

mod replies;
pub(super) use replies::Target;
#[cfg(test)]
mod tests;

pub(super) struct Gateway {
    pub service: ServiceContext,
    pub metadata: Metadata,
    pub friendship: String,
    pub tab: String,
    pub reply_target: Option<Target>,
    pub generation: u64,
    pub history_generation: u64,
    pub stopped: tokio::sync::watch::Receiver<bool>,
}

impl ServiceContext {
    pub(super) fn neighbor_gateway_ready(&self, tab_id: &str) -> bool {
        let runtime = crate::service::agent_runtime::requested_for_tab(self, tab_id);
        let mux = self.mux.read();
        mux.get_tab(tab_id).is_some_and(|tab| {
            !tab.frozen
                && !tab.agent_booting
                && tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)
                && tab.launch_session_authenticated()
                && (!runtime || crate::service::agent_runtime::engine::has_live_bound_session(tab))
        })
    }

    pub(super) async fn prepare_neighbor_gateway(&self, tab_id: &str) -> Result<()> {
        let friendship = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.neighbor_authority.as_ref())
            .map(|authority| authority.friendship_id.clone())
            .ok_or(Error::Gateway)?;
        self.require_neighbor_concierge_receiver(&friendship)?;
        self.validate_neighbor_gateway(tab_id)
            .map_err(|_| Error::Gateway)?;
        let service = self.clone();
        let tab = tab_id.to_owned();
        tokio::task::spawn_blocking(move || service.wake_frozen_tab(&tab))
            .await
            .map_err(|_| Error::Gateway)?
            .map_err(|error| {
                log::debug!("[neighbors] concierge wake tab={tab_id}: {error}");
                Error::Gateway
            })?;
        if crate::service::agent_runtime::requested_for_tab(self, tab_id) {
            crate::service::agent_runtime::engine::ensure(self, tab_id)
                .await
                .map_err(|error| {
                    log::debug!("[neighbors] concierge startup tab={tab_id}: {error}");
                    Error::Gateway
                })?;
        }
        Ok(())
    }

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
    async fn acknowledged(
        &self,
        message: &InboxMessage,
        result: codeterm_neighbors::wire::DeliveryResult,
    ) -> Result<()> {
        if result.status == codeterm_neighbors::wire::DeliveryStatus::Delivered {
            self.service
                .commit_neighbor_incoming_ack(
                    &self.metadata,
                    &self.friendship,
                    &message.doc.envelope.message_id,
                    self.history_generation,
                )
                .await?;
        }
        Ok(())
    }

    async fn deliver(
        &self,
        message: &InboxMessage,
        plaintext: Zeroizing<String>,
    ) -> Result<GatewayReceipt> {
        if matches!(
            message.doc.envelope.kind,
            codeterm_neighbors::wire::Kind::Answer | codeterm_neighbors::wire::Kind::Refusal
        ) {
            return self.deliver_reply(message, plaintext).await;
        }
        let history_generation = self.history_generation;
        self.current()?;
        if self
            .service
            .mux
            .read()
            .get_tab(&self.tab)
            .and_then(|tab| tab.neighbor_authority.as_ref())
            .is_some_and(|authority| authority.uid != message.doc.envelope.recipient_uid)
        {
            return Err(Error::WrongPeer);
        }
        let events = self.service.events.subscribe();
        let path = format!("gateway-{}.json", self.friendship);
        let previous: Option<Receipt> = self.metadata.read(path.clone()).await?;
        match resume(previous.as_ref(), &message.id, &self.tab) {
            Resume::Delivered => return Ok(GatewayReceipt::Delivered),
            Resume::Wait(delivery_id) => {
                return self
                    .await_pending(message, path.to_owned(), delivery_id, events, None)
                    .await
            }
            Resume::WrongGateway => return Err(Error::Gateway),
            Resume::Unconfirmed => return Ok(GatewayReceipt::SubmitUnconfirmed),
            Resume::Admit => {}
        }
        self.current()?;
        let plaintext = self
            .service
            .receive_neighbor_files(&self.metadata, message, plaintext, self.generation)
            .await?;
        self.service
            .append_neighbor_history(
                &self.metadata,
                &self.friendship,
                super::super::handling::incoming(
                    message,
                    &plaintext,
                    Some(if self.service.neighbor_friend_paused(&self.friendship) {
                        "paused"
                    } else {
                        "queued_for_concierge"
                    }),
                ),
                history_generation,
            )
            .await?;
        let permission = self
            .metadata
            .settings()
            .await?
            .permissions
            .get(&self.friendship)
            .cloned()
            .unwrap_or_default();
        let result = codeterm_neighbors::gateway::route_request(
            &permission,
            message.doc.envelope.kind,
            self.service.neighbor_friend_paused(&self.friendship),
            &Request {
                gateway: self,
                message,
                plaintext,
                path,
                events: parking_lot::Mutex::new(Some(events)),
                permission: permission.clone(),
            },
        )
        .await;
        let state = match &result {
            Ok(GatewayReceipt::Delivered) => {
                Some((super::super::history::handling::State::NoReply, None))
            }
            _ => Some((
                super::super::history::handling::State::Failed,
                Some("concierge_delivery_unconfirmed"),
            )),
        };
        if let Some((state, reason)) = state {
            self.service
                .neighbor_handling_delivery(
                    &self.metadata,
                    &self.friendship,
                    &message.doc.envelope.message_id,
                    state,
                    reason,
                    self.history_generation,
                )
                .await?;
        }
        result
    }
}

struct Request<'a> {
    gateway: &'a Gateway,
    message: &'a InboxMessage,
    plaintext: Zeroizing<String>,
    path: String,
    events: parking_lot::Mutex<Option<tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>>>,
    permission: codeterm_neighbors::permissions::Permission,
}

#[async_trait]
impl codeterm_neighbors::gateway::RequestPorts for Request<'_> {
    async fn local_turn(&self) -> Result<GatewayReceipt> {
        if self
            .gateway
            .service
            .neighbor_friend_paused(&self.gateway.friendship)
        {
            if matches!(
                self.message.doc.envelope.kind,
                codeterm_neighbors::wire::Kind::Answer | codeterm_neighbors::wire::Kind::Refusal
            ) {
                return Ok(GatewayReceipt::Delivered);
            }
            return self.refusal().await;
        }
        let events = self.events.lock().take().ok_or(Error::Gateway)?;
        match self
            .gateway
            .deliver_allowed(
                self.message,
                &self.plaintext,
                &self.permission,
                &self.path,
                events,
            )
            .await
        {
            Err(Error::RecipientPaused) => self.refusal().await,
            result => result,
        }
    }
    async fn refusal(&self) -> Result<GatewayReceipt> {
        self.gateway
            .save_receipt(self.message, &self.path, ReceiptState::Claimed)
            .await?;
        let result = self
            .gateway
            .service
            .neighbor_operation(super::owner::Operation::Refuse {
                friendship: self.gateway.friendship.clone(),
                message_id: self.message.doc.envelope.message_id.clone(),
                listener_generation: self.gateway.generation,
            })
            .await;
        match result {
            Ok(super::owner::Outcome::Value(value)) if value.get("submission").is_none() => {
                self.gateway
                    .save_receipt(self.message, &self.path, ReceiptState::Delivered)
                    .await?;
                Ok(GatewayReceipt::Delivered)
            }
            _ => {
                self.gateway
                    .save_receipt(self.message, &self.path, ReceiptState::SubmitUnconfirmed)
                    .await?;
                Ok(GatewayReceipt::SubmitUnconfirmed)
            }
        }
    }
}

impl Gateway {
    fn current(&self) -> Result<()> {
        if *self.stopped.borrow() {
            return Err(Error::Revoked);
        }
        self.service.require_neighbors_enabled()?;
        self.service
            .require_neighbor_friend_receiver(&self.friendship, None)?;
        if self
            .service
            .mux
            .read()
            .get_tab(&self.tab)
            .and_then(|tab| tab.neighbor_authority.as_ref())
            .is_some_and(|authority| {
                authority.friendship_id != self.friendship
                    || self.reply_target.is_none() && authority.root_tab_id.is_some()
            })
        {
            return Err(Error::Gateway);
        }
        if self
            .reply_target
            .as_ref()
            .is_some_and(|target| !self.target_current(target))
        {
            return Err(Error::Gateway);
        }
        use std::sync::atomic::Ordering;
        if self
            .service
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire)
            != self.generation
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    async fn deliver_allowed(
        &self,
        message: &InboxMessage,
        plaintext: &str,
        _permission: &codeterm_neighbors::permissions::Permission,
        path: &str,
        events: tokio::sync::broadcast::Receiver<codeterm_events::SeqEvent>,
    ) -> Result<GatewayReceipt> {
        self.current()?;
        if self.delivery_muted() {
            return Err(Error::RecipientPaused);
        }
        if self.reply_target.is_none() {
            self.service
                .validate_neighbor_gateway(&self.tab)
                .map_err(|_| Error::Gateway)?;
        }
        let target = self.delivery_target(&self.tab).ok_or(Error::Gateway)?;
        let name = self
            .service
            .neighbor_cell_names()
            .get(&self.friendship)
            .cloned()
            .unwrap_or_else(|| "Neighbor".into());
        let body = Zeroizing::new(plaintext.to_owned());
        let envelope = super::concierge::initial_prompt::message_envelope(&name, message);
        let framed_bytes = Messenger::from(envelope.clone())
            .framed_delivery(&body)
            .to_message()
            .len();
        let limit = {
            let mux = self.service.mux.read();
            let tab = mux.get_tab(&self.tab).ok_or(Error::Gateway)?;
            let provider = tab.agent_type.as_deref().ok_or(Error::Gateway)?;
            // A paste limit bounds PTY input only; runtime-backed tabs take the turn as structured input.
            if matches!(tab.backing, crate::multiplexer::types::TabBacking::Pty) {
                self.service
                    .agent_registry
                    .with_provider(provider, |provider| provider.paste_limit_bytes())
                    .flatten()
            } else {
                None
            }
        };
        if limit.is_some_and(|limit| framed_bytes > limit) {
            return Err(Error::Gateway);
        }
        self.save_receipt(message, &path, ReceiptState::Claimed)
            .await?;
        if self.reply_target.is_none() {
            self.service
                .prepare_neighbor_handling(
                    &self.metadata,
                    &self.friendship,
                    &message.doc.envelope.message_id,
                    &self.tab,
                    self.history_generation,
                )
                .await?;
        }
        let service = self.service.clone();
        let tab = self.tab.clone();
        let friendship = self.friendship.clone();
        let generation = self.generation;
        let expected = target.clone();
        let muted = self.delivery_muted();
        let outcome = tokio::task::spawn_blocking(move || {
            service
                .require_neighbors_enabled()
                .map_err(|error| (error, false))?;
            if muted
                || service.neighbor_friend_paused(&friendship)
                    && service
                        .mux
                        .read()
                        .get_tab(&tab)
                        .is_some_and(|tab| tab.neighbor_authority.is_some())
            {
                return Err((Error::RecipientPaused, false));
            }
            if !expected.matches(&service) {
                return Err((Error::Gateway, false));
            }
            if service
                .neighbor_cell()
                .listener_generation
                .load(std::sync::atomic::Ordering::Acquire)
                != generation
            {
                return Err((Error::Revoked, false));
            }
            Messenger::deliver(&service, &tab, envelope, &body).map_err(|error| {
                (
                    Error::Gateway,
                    crate::service::delivery_error::DeliveryError::from_opaque(&tab, error)
                        .bytes_written(),
                )
            })
        })
        .await
        .map_err(|_| Error::Gateway)?;
        match outcome {
            Ok(Delivery::Pty { .. } | Delivery::ChatBackend { .. }) => {
                self.save_receipt(message, &path, ReceiptState::Delivered)
                    .await?;
                self.confirm_delivery(message, Some(target)).await?;
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
                self.await_pending(message, path.to_owned(), delivery_id, events, Some(target))
                    .await
            }
            Ok(Delivery::Unconfirmed { .. }) | Err((_, true)) => {
                self.save_receipt(message, &path, ReceiptState::SubmitUnconfirmed)
                    .await?;
                Ok(GatewayReceipt::SubmitUnconfirmed)
            }
            Err((error, false)) => {
                self.save_receipt(message, &path, ReceiptState::PrewriteRefused)
                    .await?;
                Err(error)
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
        self.current()?;
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
        target: Option<Target>,
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
        let mut stopped = self.stopped.clone();
        self.current()?;
        let result = tokio::select! {
                biased;
            _ = stopped.changed() => return Err(Error::Revoked),
            result = tokio::time::timeout(std::time::Duration::from_secs(90), wait) => result,
        };
        match result {
            Ok(Ok(())) => {
                self.save_receipt(message, &path, ReceiptState::Delivered)
                    .await?;
                self.confirm_delivery(message, target).await?;
                Ok(GatewayReceipt::Delivered)
            }
            _ => Ok(GatewayReceipt::SubmitUnconfirmed),
        }
    }
}
