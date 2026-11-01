use super::*;
use crate::service::neighbors::history::{Author, ReceivedBy};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct Target {
    pub tab_id: String,
    pub session_id: String,
    pub binding_nonce: String,
}

impl Target {
    pub(super) fn matches(&self, service: &ServiceContext) -> bool {
        service.mux.read().get_tab(&self.tab_id).is_some_and(|tab| {
            tab.agent_type.is_some()
                && tab.agent_session_id.as_deref() == Some(self.session_id.as_str())
                && tab.session_binding_nonce.as_deref() == Some(self.binding_nonce.as_str())
        })
    }
}

impl Gateway {
    pub(super) fn delivery_target(&self, tab_id: &str) -> Option<Target> {
        let mux = self.service.mux.read();
        let tab = mux.get_tab(tab_id)?;
        tab.agent_type.as_ref()?;
        Some(Target {
            tab_id: tab_id.into(),
            session_id: tab.agent_session_id.clone()?,
            binding_nonce: tab.session_binding_nonce.clone()?,
        })
    }

    pub(super) fn target_current(&self, target: &Target) -> bool {
        target.matches(&self.service)
    }

    pub(super) fn delivery_muted(&self) -> bool {
        self.service.neighbor_friend_paused(&self.friendship)
            && (self.reply_target.is_none()
                || self
                    .service
                    .mux
                    .read()
                    .get_tab(&self.tab)
                    .is_some_and(|tab| tab.neighbor_authority.is_some()))
    }

    async fn reply_target_for(&self, message: &InboxMessage) -> Result<Option<Target>> {
        let Some(reply) = message.doc.envelope.in_reply_to.as_deref() else {
            return Ok(None);
        };
        let history = self.metadata.history(&self.friendship).await?;
        let Some(Author::Agent { agent, .. }) = history
            .entries
            .iter()
            .find(|entry| entry.direction == "out" && entry.message_id == reply)
            .map(|entry| &entry.author)
        else {
            return Ok(None);
        };
        let target = self.delivery_target(agent);
        let eligible = self.service.mux.read().get_tab(agent).is_some_and(|tab| {
            tab.neighbor_authority
                .as_ref()
                .is_none_or(|authority| authority.friendship_id == self.friendship)
                && !(self.service.neighbor_friend_paused(&self.friendship)
                    && tab.neighbor_authority.is_some())
        });
        Ok(target.filter(|_| eligible))
    }

    pub(super) async fn confirm_delivery(
        &self,
        message: &InboxMessage,
        target: Option<Target>,
    ) -> Result<()> {
        if let Some(target) = target.filter(|target| {
            target.matches(&self.service)
                && self
                    .service
                    .mux
                    .read()
                    .get_tab(&target.tab_id)
                    .is_some_and(|tab| tab.launch_session_authenticated())
        }) {
            self.service
                .stamp_neighbor_received_by(
                    &self.metadata,
                    &self.friendship,
                    &message.doc.envelope.message_id,
                    ReceivedBy {
                        tab_id: target.tab_id,
                        session_id: target.session_id,
                        binding_nonce: target.binding_nonce,
                    },
                    self.history_generation,
                )
                .await?;
        }
        let state = if self.reply_target.is_some() {
            super::super::super::history::handling::State::NoReply
        } else {
            super::super::super::history::handling::State::DeliveredToConcierge
        };
        self.service
            .neighbor_handling_delivery(
                &self.metadata,
                &self.friendship,
                &message.doc.envelope.message_id,
                state,
                None,
                self.history_generation,
            )
            .await
    }

    pub(super) async fn deliver_reply(
        &self,
        message: &InboxMessage,
        plaintext: Zeroizing<String>,
    ) -> Result<GatewayReceipt> {
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
        let plaintext = self
            .service
            .receive_neighbor_files(&self.metadata, message, plaintext, self.generation)
            .await?;
        self.service
            .append_neighbor_history(
                &self.metadata,
                &self.friendship,
                super::super::super::handling::incoming(message, &plaintext, None),
                self.history_generation,
            )
            .await?;
        let Some(target) = self.reply_target_for(message).await? else {
            self.service
                .neighbor_handling_delivery(
                    &self.metadata,
                    &self.friendship,
                    &message.doc.envelope.message_id,
                    super::super::super::history::handling::State::NoReply,
                    None,
                    self.history_generation,
                )
                .await?;
            return Ok(GatewayReceipt::Delivered);
        };
        let reply = Gateway {
            service: self.service.clone(),
            metadata: self.metadata.clone(),
            friendship: self.friendship.clone(),
            tab: target.tab_id.clone(),
            reply_target: Some(target),
            generation: self.generation,
            history_generation: self.history_generation,
            stopped: self.stopped.clone(),
        };
        let path = format!("gateway-replies-{}.json", self.friendship);
        let previous: Option<Receipt> = self.metadata.read(path.clone()).await?;
        let events = self.service.events.subscribe();
        match resume(previous.as_ref(), &message.id, &reply.tab) {
            Resume::Delivered => {
                reply.confirm_delivery(message, None).await?;
                return Ok(GatewayReceipt::Delivered);
            }
            Resume::Wait(id) => return reply.await_pending(message, path, id, events, None).await,
            Resume::WrongGateway | Resume::Unconfirmed => {
                return Ok(GatewayReceipt::SubmitUnconfirmed)
            }
            Resume::Admit => {}
        }
        let result = reply
            .deliver_allowed(message, &plaintext, &Default::default(), &path, events)
            .await;
        if matches!(result, Ok(GatewayReceipt::Delivered)) {
            reply.confirm_delivery(message, None).await?;
        } else {
            self.service
                .neighbor_handling_delivery(
                    &self.metadata,
                    &self.friendship,
                    &message.doc.envelope.message_id,
                    super::super::super::history::handling::State::Failed,
                    Some("author_delivery_unconfirmed"),
                    self.history_generation,
                )
                .await?;
        }
        result
    }
}

#[cfg(test)]
mod tests;
