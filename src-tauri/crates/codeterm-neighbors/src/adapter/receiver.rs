use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReceiverStatus {
    pub receiver_revision: u64,
    pub receiver_installation_id: Option<String>,
    pub receiver_key_id: Option<String>,
    pub receiver_machine_label: Option<String>,
    pub receiving_enabled: bool,
    pub is_me: bool,
}

impl ReceiverStatus {
    pub fn receiving_here(&self) -> bool {
        self.is_me && self.receiving_enabled
    }
}

#[derive(Clone)]
pub struct Enrollment {
    pub expected_revision: u64,
    pub takeover: bool,
}

impl FirebaseRegistry {
    pub async fn receiver_status(&self) -> Result<ReceiverStatus> {
        let value = self
            .calls
            .invoke(
                "friendsReceiverStatus",
                json!({"installation_id":self.installation_id}),
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::Transport)
    }

    pub async fn disable_receiving(&self, revision: u64) -> Result<()> {
        self.calls
            .invoke(
                "friendsDisableReceiving",
                json!({
                    "installation_id":self.installation_id,
                    "expected_receiver_revision":revision,
                    "operation_id":operation_id(),
                }),
            )
            .await?;
        Ok(())
    }
}

pub(super) fn operation_id() -> String {
    use rand::RngCore;
    let mut bytes = [0; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
