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
    #[serde(default)]
    pub legacy_active: bool,
}

impl ReceiverStatus {
    pub fn receiving_here(&self) -> bool {
        self.is_me && self.receiving_enabled
    }

    fn legacy_pending(&self) -> bool {
        self.legacy_active && self.receiver_revision == 0 && self.receiver_installation_id.is_none()
    }
}

#[derive(Clone)]
pub struct Enrollment {
    pub expected_revision: u64,
    pub takeover: bool,
    pub operation_id: Option<String>,
}

impl FirebaseRegistry {
    pub async fn receiver_status_for(
        &self,
        keys: &crate::keys::InstallationKeys<'_>,
        adopt: bool,
    ) -> Result<ReceiverStatus> {
        let mut status = self.receiver_status().await?;
        if !status.legacy_pending() {
            return Ok(status);
        }
        let Some(local) = keys.reconcile().await? else {
            return Ok(status);
        };
        let key_id = local.verifier_key_id(&self.uid);
        if adopt {
            let registry = FirebaseRegistry {
                enrollment: Some(Enrollment {
                    expected_revision: 0,
                    takeover: false,
                    operation_id: Some(crate::crypto::sha256_hex(
                        format!(
                            "receiver-adoption:{}:{}:{key_id}",
                            self.installation_id, self.uid
                        )
                        .as_bytes(),
                    )),
                }),
                ..self.clone()
            };
            match registry
                .enroll(&local.signing_spki(), &local.encryption_spki(), false)
                .await
            {
                Ok(id) if id == key_id => {}
                Ok(_) => return Err(Error::InvalidKey),
                Err(
                    Error::Transport
                    | Error::ReceiverElsewhere
                    | Error::ReceiverRevisionStale
                    | Error::OperationIdConflict,
                ) => {}
                Err(error) => return Err(error),
            }
            status = self.receiver_status().await?;
        }
        if status.legacy_pending() && keys.registry.active_key_ids().await?.contains(&key_id) {
            status.is_me = true;
            status.receiving_enabled = true;
            status.receiver_installation_id = Some(self.installation_id.clone());
            status.receiver_key_id = Some(key_id);
            status.receiver_machine_label = Some(self.machine_label.clone());
        }
        Ok(status)
    }

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

#[cfg(test)]
mod tests;
