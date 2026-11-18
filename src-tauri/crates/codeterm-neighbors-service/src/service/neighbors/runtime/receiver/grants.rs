use super::*;
use codeterm_neighbors::permissions::MachineAccess;

#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    Keep,
    Adopt,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub choice: Choice,
    pub expected_digest: String,
    pub scope: Option<String>,
}

#[derive(serde::Serialize)]
pub struct Preview {
    pub revision: u64,
    pub previous_machine: Option<String>,
    pub local: Value,
    pub previous: Option<Value>,
    pub different: bool,
    pub digest: String,
    pub unavailable: bool,
}

fn owns_receiver_link(
    account: &str,
    installation: Option<&str>,
    link: &codeterm_mesh::account_mesh::ledger::LinkRecord,
) -> bool {
    matches!(&link.admission, codeterm_mesh::account_mesh::ledger::LinkAdmission::Account { account_id, installation_id, .. }
        if account_id == account && Some(installation_id.as_str()) == installation)
}

fn receiver_client(
    mesh: &codeterm_mesh::MeshManager,
    uid: &str,
    status: &ReceiverStatus,
) -> Result<codeterm_mesh::transport::PeerClient, &'static str> {
    let account = codeterm_account::account_mesh::account_id(uid);
    if mesh.account_state().signed_in_account() != Some(account.as_str()) {
        return Err("Mesh account does not match the current account");
    }
    let installations = mesh.peers_response().installations;
    let receiver = installations
        .iter()
        .find(|row| {
            row.active
                && !row.signed_out
                && Some(&row.installation_id) == status.receiver_installation_id.as_ref()
        })
        .ok_or("Previous receiver installation is unavailable")?;
    let id = receiver
        .instance_id
        .as_deref()
        .ok_or("Previous receiver has no admitted instance")?;
    if !mesh.link_record(id).is_some_and(|link| {
        owns_receiver_link(&account, status.receiver_installation_id.as_deref(), link)
    }) {
        return Err("Previous receiver link does not match its account and installation");
    }
    mesh.client_for(id)
        .ok_or("Previous receiver has no available authenticated client")
}

fn grant(value: &Value) -> Result<Permission, String> {
    let mut fields = value.clone();
    let object = fields.as_object_mut().ok_or("Invalid receiver grant")?;
    for field in [
        "initial_context_digest",
        "effective_folder",
        "folder_enforcement",
    ] {
        object.remove(field);
    }
    serde_json::from_value(fields).map_err(|_| "Invalid receiver grant".into())
}

impl Preview {
    fn new(
        status: &ReceiverStatus,
        local: Value,
        previous: Option<Value>,
        uid: &str,
        target: &str,
        friendship: &str,
    ) -> Result<Self, String> {
        let local_grant = grant(&local)?;
        let previous_grant = previous.as_ref().map(grant).transpose()?;
        let different = previous_grant
            .as_ref()
            .is_some_and(|old| old != &local_grant);
        let bytes = serde_json::to_vec(&json!([
            uid,
            target,
            friendship,
            status.receiver_installation_id,
            status.receiver_revision,
            local_grant,
            previous_grant
        ]))
        .map_err(|error| error.to_string())?;
        Ok(Self {
            revision: status.receiver_revision,
            previous_machine: status.receiver_machine_label.clone(),
            unavailable: !status.is_me
                && status.receiver_installation_id.is_some()
                && previous.is_none(),
            local,
            previous,
            different,
            digest: codeterm_neighbors::crypto::sha256_hex(&bytes),
        })
    }

    pub fn decide(
        &self,
        decision: Option<&Decision>,
        confirmed: bool,
    ) -> Result<Option<Permission>, String> {
        let Some(decision) = decision else {
            return if self.different {
                Err(format!("receiver_grant_confirmation_required: Review both grants and confirm keep or adopt; the concierge starts fresh. {}", serde_json::to_string(self).map_err(|error| error.to_string())?))
            } else {
                Ok(None)
            };
        };
        if !confirmed || decision.expected_digest != self.digest {
            return Err(
                "receiver_grant_stale: Refresh both grants and ask the owner to confirm again"
                    .into(),
            );
        }
        match decision.choice {
            Choice::Keep => Ok(None),
            Choice::Adopt => {
                let mut adopted = grant(self.previous.as_ref().ok_or("receiver_grant_invalid: Previous receiver grant is unavailable; adoption is not permitted")?)?;
                if let Some(scope) = &decision.scope {
                    adopted.scope.clone_from(scope);
                }
                let scope = adopted.scope.to_lowercase();
                if adopted.machine_access == MachineAccess::Full
                    && ["read-only", "read only", "read_only"]
                        .iter()
                        .any(|text| scope.contains(text))
                {
                    return Err("receiver_grant_invalid: Reconcile the adopted scope with Full machine access before Apply; retain its sharing restrictions".into());
                }
                if adopted.machine_access == MachineAccess::None {
                    return Err(
                        "receiver_grant_invalid: No provider supports machine access none".into(),
                    );
                }
                adopted
                    .validate()
                    .map_err(|error| format!("receiver_grant_invalid: {error}"))?;
                Ok(Some(adopted))
            }
        }
    }
}

pub trait NeighborsExt {
    fn neighbor_receiver_grants(
        &self,
        metadata: &Metadata,
        friendship: &str,
        status: &ReceiverStatus,
    ) -> impl std::future::Future<Output = Result<Preview, String>> + Send;
}

impl NeighborsExt for ServiceContext {
    async fn neighbor_receiver_grants(
        &self,
        metadata: &Metadata,
        friendship: &str,
        status: &ReceiverStatus,
    ) -> Result<Preview, String> {
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidRequest.to_string());
        }
        let generation = self.neighbor_history_generation();
        let account = self.account().await?;
        let session = account
            .session()
            .await
            .ok_or_else(|| Error::SessionRevoked.to_string())?;
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let permission = settings
            .permissions
            .get(friendship)
            .cloned()
            .unwrap_or_default();
        let local = json!(self
            .neighbor_permission_projection(metadata, friendship, &permission, &settings)
            .map_err(|error| error.to_string())?);
        let client = if status.is_me {
            None
        } else {
            let selected = self
                .try_ext::<crate::MeshStateHandle>()
                .ok_or("Mesh is unavailable")
                .and_then(|handle| receiver_client(&handle.lock(), &session.identity.uid, status));
            match selected {
                Ok(client) => Some(client),
                Err(reason) => {
                    log::debug!("[neighbors] previous receiver grant client unavailable: {reason}");
                    None
                }
            }
        };
        let previous = match client {
            Some(client) => match client
                .auth_post_json(
                    "/api/neighbors/runtime",
                    &json!({"operation":"permission","friendship_id":friendship}),
                )
                .await
            {
                Ok(grant) => Some(grant),
                Err(error) => {
                    log::debug!("[neighbors] previous receiver grant read unavailable: {error}");
                    None
                }
            },
            None => None,
        };
        self.require_neighbor_generation(generation)?;
        if !super::super::super::same_session(account.session().await.as_ref(), &session) {
            return Err(Error::SessionRevoked.to_string());
        }
        let installation = self.installation_id().await?;
        Preview::new(
            status,
            local,
            previous,
            &session.identity.uid,
            installation.as_str(),
            friendship,
        )
    }
}

#[cfg(test)]
mod tests;
