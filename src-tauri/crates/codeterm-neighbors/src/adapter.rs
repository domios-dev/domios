mod firestore;
#[cfg(test)]
mod tests;

use crate::keys::KeyRegistry;
use crate::receive::{InboxListener, NeighborTransport};
use crate::wire::{Cursor, DeliveryResult, DeliveryStatus, PeerKeys, SignedEnvelope};
use crate::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use zeroize::Zeroizing;

#[async_trait]
pub trait AuthenticatedCall: Send + Sync {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value>;
    async fn id_token(&self) -> Result<Zeroizing<String>>;
}

pub struct FirebaseTransport {
    pub calls: Arc<dyn AuthenticatedCall>,
    pub project: String,
    pub uid: String,
}

#[async_trait]
impl NeighborTransport for FirebaseTransport {
    async fn send(&self, message: &SignedEnvelope) -> Result<DeliveryStatus> {
        message.envelope.validate()?;
        if message.envelope.sender_uid != self.uid {
            return Err(Error::WrongPeer);
        }
        self.calls
            .invoke(
                "friendsSend",
                serde_json::to_value(message).map_err(|_| Error::InvalidEnvelope)?,
            )
            .await?;
        Ok(DeliveryStatus::Pending)
    }
    async fn listen_from_cursor(
        &self,
        friendship: &str,
        cursor: Option<&Cursor>,
    ) -> Result<Box<dyn InboxListener>> {
        firestore::listen(self, friendship, cursor).await
    }
    async fn ack(&self, sender_uid: &str, message_id: &str) -> Result<DeliveryResult> {
        let result = self
            .calls
            .invoke(
                "friendsAck",
                json!({"sender_uid":sender_uid,"message_id":message_id}),
            )
            .await?;
        serde_json::from_value(result).map_err(|_| Error::Transport)
    }
    async fn peer_keys(&self, friendship: &str) -> Result<PeerKeys> {
        let body = self
            .calls
            .invoke("friendsPeerKeys", json!({"friendship_id":friendship}))
            .await?;
        serde_json::from_value(body).map_err(|_| Error::Transport)
    }
    async fn delivery_status(&self, message_id: &str) -> Result<DeliveryResult> {
        let body = self
            .calls
            .invoke(
                "friendsDeliveryStatus",
                json!({"sender_uid":self.uid,"message_id":message_id}),
            )
            .await?;
        serde_json::from_value(body).map_err(|_| Error::Transport)
    }
}

pub struct FirebaseRegistry {
    pub calls: Arc<dyn AuthenticatedCall>,
    pub uid: String,
}

#[async_trait]
impl KeyRegistry for FirebaseRegistry {
    async fn enroll(
        &self,
        signing_spki: &str,
        encryption_spki: &str,
        replace_others: bool,
    ) -> Result<String> {
        let mut data = json!({"public_key_spki_b64":signing_spki,"encryption_public_key_spki_b64":encryption_spki});
        if replace_others {
            data["replace_others"] = true.into();
        }
        let body = self.calls.invoke("friendsEnrollVerifierKey", data).await?;
        body.get("verifier_key_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(Error::Transport)
    }
    async fn revoke(&self, id: &str) -> Result<()> {
        let result = self
            .calls
            .invoke("friendsRevokeVerifierKey", json!({"verifier_key_id":id}))
            .await
            .map(|_| ());
        match result {
            Err(Error::KeyNotFound) => Ok(()),
            result => result,
        }
    }

    async fn active_key_ids(&self) -> Result<Vec<String>> {
        let body = self.calls.invoke("friendsOwnKeys", json!({})).await?;
        let keys = body.as_array().ok_or(Error::Transport)?;
        keys.iter()
            .map(|key| {
                key.get("key_id")
                    .and_then(Value::as_str)
                    .filter(|id| crate::wire::key_id(id))
                    .map(str::to_owned)
                    .ok_or(Error::Transport)
            })
            .collect()
    }
}
