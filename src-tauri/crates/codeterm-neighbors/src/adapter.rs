mod firestore;
pub use firestore::control::ControlListener;
mod pause;
mod receiver;
pub const PENDING_COUNT_LIMIT: u64 = 100;
pub use receiver::{Enrollment, ReceiverStatus};
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
    fn firestore_failure(
        &self,
        _operation: &'static str,
        _target: &str,
        _code: Option<i32>,
        _kind: &str,
    ) {
    }
    async fn invoke(&self, route: &str, data: Value) -> Result<Value>;
    async fn id_token(&self) -> Result<Zeroizing<String>>;
}

pub struct FirebaseTransport {
    pub calls: Arc<dyn AuthenticatedCall>,
    pub project: String,
    pub uid: String,
    pub installation_id: String,
}

impl FirebaseTransport {
    pub async fn listen_pending(&self, friendship: &str) -> Result<Box<dyn InboxListener>> {
        firestore::listen_pending(self, friendship).await
    }

    pub async fn pending_count(&self, friendship: &str) -> Result<u64> {
        firestore::pending_count(self, friendship).await
    }
    pub async fn listen_registry(&self) -> Result<Box<dyn crate::registry::RegistryListener>> {
        firestore::registry::listen(self).await
    }
}

#[async_trait]
impl NeighborTransport for FirebaseTransport {
    async fn send(&self, message: &SignedEnvelope) -> Result<DeliveryStatus> {
        message.envelope.validate()?;
        if message.envelope.sender_uid != self.uid {
            return Err(Error::WrongPeer);
        }
        self.calls
            .invoke("friendsSend", {
                let mut data = serde_json::to_value(message).map_err(|_| Error::InvalidEnvelope)?;
                data["installation_id"] = self.installation_id.clone().into();
                if let Some(reply) = &message.envelope.in_reply_to {
                    data["in_reply_to"] = reply.clone().into();
                }
                data
            })
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
                json!({"sender_uid":sender_uid,"message_id":message_id,"installation_id":self.installation_id}),
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
                json!({"sender_uid":self.uid,"message_id":message_id,"installation_id":self.installation_id}),
            )
            .await?;
        serde_json::from_value(body).map_err(|_| Error::Transport)
    }
}

#[derive(Clone)]
pub struct FirebaseRegistry {
    pub calls: Arc<dyn AuthenticatedCall>,
    pub uid: String,
    pub installation_id: String,
    pub machine_label: String,
    pub enrollment: Option<Enrollment>,
}

#[async_trait]
impl KeyRegistry for FirebaseRegistry {
    async fn enroll(
        &self,
        signing_spki: &str,
        encryption_spki: &str,
        replace_others: bool,
    ) -> Result<String> {
        let status = self.receiver_status().await?;
        let enrollment = match &self.enrollment {
            Some(enrollment) => enrollment.clone(),
            None if status.receiving_here() => Enrollment {
                expected_revision: status.receiver_revision,
                takeover: false,
                operation_id: None,
            },
            None => return Err(Error::NotReceiver),
        };
        let data = self.enrollment_data(signing_spki, encryption_spki, &enrollment, replace_others);
        let body = match self.calls.invoke("friendsEnrollVerifierKey", data).await {
            Ok(body) => body,
            Err(Error::Transport) => {
                let observed = self.receiver_status().await?;
                let expected = format!(
                    "vk_{}",
                    &crate::crypto::sha256_hex(
                        format!("verifier-key:{}:{signing_spki}", self.uid).as_bytes()
                    )[..32]
                );
                if observed.receiving_here()
                    && observed.receiver_key_id.as_deref() == Some(expected.as_str())
                {
                    json!({"verifier_key_id":expected})
                } else {
                    return Err(Error::Transport);
                }
            }
            Err(error) => return Err(error),
        };
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

impl FirebaseRegistry {
    fn enrollment_data(
        &self,
        signing_spki: &str,
        encryption_spki: &str,
        enrollment: &Enrollment,
        replace_others: bool,
    ) -> Value {
        json!({
            "public_key_spki_b64":signing_spki,
            "encryption_public_key_spki_b64":encryption_spki,
            "installation_id":self.installation_id,
            "machine_label":self.machine_label,
            "expected_receiver_revision":enrollment.expected_revision,
            "operation_id":enrollment.operation_id.clone().unwrap_or_else(receiver::operation_id),
            "takeover":enrollment.takeover && replace_others,
            "features":["files.v1"],
        })
    }
}
