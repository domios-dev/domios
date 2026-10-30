use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReceiverStatus {
    pub receiver_revision: u64,
    pub receiver_installation_id: Option<String>,
    pub receiver_encryption_key_id: Option<String>,
    pub receiver_machine_label: Option<String>,
    pub receiving_enabled: bool,
    pub is_me: bool,
}

impl ReceiverStatus {
    pub fn effective(receiver: &crate::wire::EffectiveReceiver, installation: &str) -> Self {
        Self {
            receiver_revision: receiver.revision,
            receiver_installation_id: receiver.installation_id.clone(),
            receiver_encryption_key_id: receiver.enc_key_id.clone(),
            receiver_machine_label: receiver.machine_label.clone(),
            receiving_enabled: receiver.receiving,
            is_me: receiver.installation_id.as_deref() == Some(installation),
        }
    }
    pub fn receiving_here(&self) -> bool {
        self.is_me && self.receiving_enabled
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FriendReceiverRow {
    pub friendship_id: String,
    pub machine_label: Option<String>,
    pub revision: u64,
    pub receiving: bool,
    pub is_me: bool,
}

impl super::FirebaseRegistry {
    pub async fn receiver_status(&self) -> crate::Result<Vec<FriendReceiverRow>> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Status {
            schema_version: u8,
            receivers: Vec<FriendReceiverRow>,
        }
        let value = self
            .calls
            .invoke(
                "friendsReceiverStatus",
                serde_json::json!({"installation_id":self.installation_id}),
            )
            .await?;
        let status: Status =
            serde_json::from_value(value).map_err(|_| crate::Error::InvalidEnvelope)?;
        if status.schema_version != 1
            || status.receivers.iter().any(|row| {
                !crate::wire::hex_id(&row.friendship_id, 64)
                    || row.revision > crate::wire::MAX_SAFE_INTEGER
            })
        {
            return Err(crate::Error::InvalidEnvelope);
        }
        Ok(status.receivers)
    }
}

pub(super) fn operation_id() -> String {
    use rand::RngCore;
    let mut bytes = [0; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
