use crate::crypto::{encryption_key_id, validate_encryption_spki, verify};
use crate::keys::RecipientKeys;
use crate::wire::{
    Cursor, DeliveryResult, DeliveryStatus, InboxDocument, InboxMessage, PeerKeys, SignedEnvelope,
    Timestamp,
};
use crate::{Error, Result};
use async_trait::async_trait;
use zeroize::Zeroizing;

#[async_trait]
pub trait NeighborTransport: Send + Sync {
    async fn send(&self, message: &SignedEnvelope) -> Result<DeliveryStatus>;
    async fn listen_from_cursor(
        &self,
        friendship: &str,
        cursor: Option<&Cursor>,
    ) -> Result<Box<dyn InboxListener>>;
    async fn ack(&self, sender_uid: &str, message_id: &str) -> Result<DeliveryResult>;
    async fn peer_keys(&self, friendship: &str) -> Result<PeerKeys>;
    async fn delivery_status(&self, message_id: &str) -> Result<DeliveryResult>;

    fn verify<'a>(
        &self,
        doc: &'a InboxDocument,
        peers: &'a PeerKeys,
        recipient: &str,
        now: Timestamp,
    ) -> Result<&'a str> {
        verify_inbound(doc, peers, recipient, now)
    }
}

#[async_trait]
pub trait InboxListener: Send {
    async fn next(&mut self) -> Result<Option<InboxEvent>>;
}

pub enum InboxEvent {
    Ready,
    Messages(Vec<InboxMessage>),
}

#[async_trait]
pub trait CursorStore: Send + Sync {
    async fn load(&self, friendship: &str) -> Result<Option<Cursor>>;
    async fn save(&self, friendship: &str, cursor: &Cursor) -> Result<()>;
}

pub enum GatewayReceipt {
    Delivered,
    SubmitUnconfirmed,
}

#[async_trait]
pub trait NeighborGateway: Send + Sync {
    async fn deliver(
        &self,
        message: &InboxMessage,
        plaintext: Zeroizing<String>,
    ) -> Result<GatewayReceipt>;
}

pub fn verify_inbound<'a>(
    doc: &InboxDocument,
    peers: &'a PeerKeys,
    recipient: &str,
    now: Timestamp,
) -> Result<&'a str> {
    let e = &doc.envelope;
    if !doc.received_at.valid() || !doc.expires_at.valid() || !now.valid() {
        return Err(Error::InvalidEnvelope);
    }
    if e.friendship_id != peers.friendship_id
        || e.sender_uid != peers.peer_uid
        || e.recipient_uid != recipient
    {
        return Err(Error::WrongPeer);
    }
    let key = peers
        .keys
        .iter()
        .find(|key| key.verifier_key_id == e.verifier_key_id)
        .ok_or(Error::InvalidKey)?;
    verify(e, &doc.signature, &key.public_key_spki_b64)?;
    if doc.expires_at <= now {
        return Err(Error::Expired);
    }
    validate_encryption_spki(&key.encryption_public_key_spki_b64)?;
    if key.enc_key_id != e.enc_key_ids.sender
        || encryption_key_id(&key.encryption_public_key_spki_b64) != key.enc_key_id
    {
        return Err(Error::InvalidKey);
    }
    Ok(&key.encryption_public_key_spki_b64)
}

pub struct Receiver<'a> {
    pub transport: &'a dyn NeighborTransport,
    pub cursors: &'a dyn CursorStore,
    pub gateway: &'a dyn NeighborGateway,
    pub private_keys: &'a dyn RecipientKeys,
    pub recipient_uid: &'a str,
    pub friendship_id: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveOutcome {
    Delivered,
    NotDelivered,
    Duplicate,
    Rejected(Error),
}

impl Receiver<'_> {
    pub async fn process(
        &mut self,
        message: &InboxMessage,
        now: Timestamp,
    ) -> Result<ReceiveOutcome> {
        let mut peers = None;
        self.process_with_peer_snapshot(message, now, &mut peers)
            .await
    }

    async fn process_with_peer_snapshot(
        &mut self,
        message: &InboxMessage,
        now: Timestamp,
        peers: &mut Option<PeerKeys>,
    ) -> Result<ReceiveOutcome> {
        let cursor = Cursor {
            received_at: message.doc.received_at,
            id: message.id.clone(),
        };
        cursor.validate()?;
        let expected_id = format!(
            "{}_{}",
            message.doc.envelope.sender_uid, message.doc.envelope.message_id
        );
        if message.id != expected_id || message.doc.envelope.friendship_id != self.friendship_id {
            return Err(Error::WrongPeer);
        }
        let previous = self.cursors.load(self.friendship_id).await?;
        if let Some(previous) = previous {
            previous.validate().map_err(|_| Error::CursorStore)?;
            if cursor <= previous {
                return Ok(ReceiveOutcome::Duplicate);
            }
        }
        if peers.is_none() {
            *peers = Some(self.transport.peer_keys(self.friendship_id).await?);
        }
        let peers = peers.as_ref().ok_or(Error::Transport)?;
        let plaintext = match self
            .transport
            .verify(&message.doc, peers, self.recipient_uid, now)
        {
            Ok(peer) => {
                self.private_keys
                    .decrypt_for(&message.doc.envelope, peer)
                    .await
            }
            Err(reason) => Err(reason),
        };
        let plaintext = match plaintext {
            Ok(plaintext) => plaintext,
            Err(
                reason @ (Error::LocalKeyUnavailable
                | Error::SecretStore
                | Error::Transport
                | Error::Revoked),
            ) => return Err(reason),
            Err(reason) => {
                self.cursors.save(self.friendship_id, &cursor).await?;
                return Ok(ReceiveOutcome::Rejected(reason));
            }
        };
        let envelope = &message.doc.envelope;
        if let Err(reason) = crate::files::AttachmentBody::parse(
            &plaintext,
            &crate::files::Binding {
                friendship_id: &envelope.friendship_id,
                sender_uid: &envelope.sender_uid,
                recipient_uid: &envelope.recipient_uid,
                message_id: &envelope.message_id,
            },
        ) {
            self.cursors.save(self.friendship_id, &cursor).await?;
            return Ok(ReceiveOutcome::Rejected(reason));
        }
        match self.gateway.deliver(message, plaintext).await? {
            GatewayReceipt::Delivered => {}
            GatewayReceipt::SubmitUnconfirmed => return Err(Error::Gateway),
        }
        let status = self
            .transport
            .ack(
                &message.doc.envelope.sender_uid,
                &message.doc.envelope.message_id,
            )
            .await?;
        if status.status == DeliveryStatus::Pending {
            return Err(Error::Transport);
        }
        self.cursors.save(self.friendship_id, &cursor).await?;
        Ok(if status.status == DeliveryStatus::Delivered {
            ReceiveOutcome::Delivered
        } else {
            ReceiveOutcome::NotDelivered
        })
    }

    pub async fn catch_up(
        &mut self,
        messages: &mut [InboxMessage],
        now: Timestamp,
    ) -> Result<Vec<ReceiveOutcome>> {
        messages.sort_by(|a, b| {
            a.doc
                .received_at
                .cmp(&b.doc.received_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut outcomes = Vec::with_capacity(messages.len());
        let mut peers = None;
        for message in messages {
            outcomes.push(
                self.process_with_peer_snapshot(message, now, &mut peers)
                    .await?,
            );
        }
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests;
