use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;

pub(super) fn friend_status(
    list: &Value,
    friendship: &str,
    account: &ReceiverStatus,
    installation: &str,
) -> codeterm_neighbors::Result<ReceiverStatus> {
    let friend = list["friends"]
        .as_array()
        .ok_or(Error::InvalidEnvelope)?
        .iter()
        .find(|row| row["friendship_id"] == friendship)
        .ok_or(Error::FriendshipEnded)?;
    match friend.get("my_receiver") {
        Some(value) => {
            let projection =
                serde_json::from_value(value.clone()).map_err(|_| Error::InvalidEnvelope)?;
            receiver::friends::FriendReceiver::from_projection(projection, installation)
                .map(|receiver| receiver.status)
                .map_err(|_| Error::InvalidEnvelope)
        }
        None => Ok(account.clone()),
    }
}

fn first_claim_revision(
    list: &Value,
    friendship: &str,
    account: &ReceiverStatus,
    installation: &str,
) -> codeterm_neighbors::Result<Option<u64>> {
    let friend = friend_status(list, friendship, account, installation)?;
    if account.receiver_installation_id.is_some() || account.legacy_active {
        return Ok(None);
    }
    for row in list["friends"].as_array().ok_or(Error::InvalidEnvelope)? {
        let id = row["friendship_id"]
            .as_str()
            .ok_or(Error::InvalidEnvelope)?;
        if friend_status(list, id, account, installation)?
            .receiver_installation_id
            .is_some()
        {
            return Ok(None);
        }
    }
    Ok(friend
        .receiver_installation_id
        .is_none()
        .then_some(friend.receiver_revision))
}

fn receiver_refusal(friendship: &str, name: &str, friend: &ReceiverStatus) -> String {
    match friend.receiver_installation_id {
        Some(_) => format!("receiving_elsewhere: Messages from {name} are received on {}. Receive them here? Use domios account neighbors receive-here {friendship} --expected-receiver-revision {} --takeover-confirmed only after the owner confirms.", friend.receiver_machine_label.as_deref().unwrap_or("another machine"), friend.receiver_revision),
        None => "receiver_not_chosen: No machine receives this friend's messages. Choose Receive this friend here.".into(),
    }
}

impl ServiceContext {
    pub(in crate::service::neighbors) async fn claim_first_neighbor_receiver(
        &self,
        friendship: &str,
        uid: &str,
        generation: u64,
    ) -> Result<(), String> {
        self.neighbor_operation(owner::Operation::ClaimFirstReceiver {
            friendship: friendship.into(),
            uid: uid.into(),
            expected_generation: generation,
        })
        .await?;
        Ok(())
    }

    pub(super) async fn claim_first_neighbor_receiver_owned(
        &self,
        friendship: &str,
        uid: &str,
        generation: u64,
    ) -> Result<(), String> {
        self.require_neighbors_enabled()
            .map_err(|e| e.to_string())?;
        self.require_neighbor_generation(generation)?;
        let (transport, registry, scope, metadata) = self.neighbor_ports().await?;
        if registry.uid != uid {
            return Err(Error::Revoked.to_string());
        }
        let account = registry
            .receiver_status()
            .await
            .map_err(|e| e.to_string())?;
        let list = registry
            .calls
            .invoke("friendsList", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        self.require_neighbor_generation(generation)?;
        if let Some(revision) =
            first_claim_revision(&list, friendship, &account, &registry.installation_id)
                .map_err(|e| e.to_string())?
        {
            self.set_neighbor_friend_receiver(
                &registry,
                &scope,
                &metadata,
                friendship,
                receiver::selection::Selection::Set,
                false,
                revision,
                generation,
            )
            .await
            .map_err(|e| e.to_string())?;
            self.start_neighbor_listeners(transport, registry, scope, metadata, generation)
                .await?;
        }
        Ok(())
    }

    pub(in crate::service::neighbors) async fn remember_neighbor_receiving_invite(
        &self,
        metadata: &Metadata,
        email: String,
        generation: u64,
    ) -> Result<(), String> {
        let mut emails: Vec<String> = metadata
            .read("receiving-invites.json".into())
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let email = codeterm_neighbors::crypto::sha256_hex(email.trim().to_lowercase().as_bytes());
        if !emails.contains(&email) {
            if emails.len() >= 200 {
                return Err(Error::InvalidRequest.to_string());
            }
            emails.push(email);
        }
        self.require_neighbor_generation(generation)?;
        metadata
            .write("receiving-invites.json".into(), emails)
            .await
            .map_err(|e| e.to_string())
    }

    pub(super) async fn fulfill_neighbor_receiving_invites(
        &self,
        registry: &FirebaseRegistry,
        scope: &KeyScope,
        metadata: &Metadata,
        generation: u64,
    ) -> Result<(), String> {
        self.require_neighbors_enabled()
            .map_err(|e| e.to_string())?;
        let emails: Vec<String> = metadata
            .read("receiving-invites.json".into())
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        if emails.is_empty() {
            return Ok(());
        }
        if emails.len() > 200 {
            return Err(Error::InvalidEnvelope.to_string());
        }
        let list = registry
            .calls
            .invoke("friendsList", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        let account = registry
            .receiver_status()
            .await
            .map_err(|e| e.to_string())?;
        let mut remaining = emails.clone();
        for friend in list["friends"]
            .as_array()
            .ok_or_else(|| Error::InvalidEnvelope.to_string())?
        {
            let email = codeterm_neighbors::crypto::sha256_hex(
                friend["email"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .to_lowercase()
                    .as_bytes(),
            );
            if !emails.contains(&email) {
                continue;
            }
            let id = friend["friendship_id"]
                .as_str()
                .ok_or_else(|| Error::InvalidEnvelope.to_string())?;
            remaining.retain(|pending| pending != &email);
            self.require_neighbor_generation(generation)?;
            metadata
                .write("receiving-invites.json".into(), remaining.clone())
                .await
                .map_err(|e| e.to_string())?;
            if let Some(revision) =
                first_claim_revision(&list, id, &account, &registry.installation_id)
                    .map_err(|e| e.to_string())?
            {
                self.set_neighbor_friend_receiver(
                    registry,
                    scope,
                    metadata,
                    id,
                    receiver::selection::Selection::Set,
                    false,
                    revision,
                    generation,
                )
                .await
                .map_err(|e| e.to_string())?;
            }
        }
        self.require_neighbor_generation(generation)?;
        metadata
            .write("receiving-invites.json".into(), remaining)
            .await
            .map_err(|e| e.to_string())
    }

    pub(super) async fn require_neighbor_receiver_confirmation(
        &self,
        registry: &FirebaseRegistry,
        friendship: &str,
        revision: u64,
        confirmed: bool,
    ) -> Result<(), String> {
        if confirmed {
            return Ok(());
        }
        let account = registry
            .receiver_status()
            .await
            .map_err(|e| e.to_string())?;
        let list = registry
            .calls
            .invoke("friendsList", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        let friend = friend_status(&list, friendship, &account, &registry.installation_id)
            .map_err(|e| e.to_string())?;
        if revision != friend.receiver_revision {
            return Err(Error::ReceiverRevisionStale.to_string());
        }
        if friend.receiver_installation_id.is_some() && !friend.is_me {
            self.remember_neighbor_names(&list);
            let name = self
                .neighbor_cell_names()
                .get(friendship)
                .cloned()
                .unwrap_or_else(|| "this friend".into());
            return Err(receiver_refusal(friendship, &name, &friend));
        }
        Ok(())
    }

    pub(super) async fn prepare_neighbor_owner_action(
        &self,
        registry: &FirebaseRegistry,
        scope: &KeyScope,
        metadata: &Metadata,
        friendship: &str,
        generation: u64,
    ) -> Result<(), String> {
        let account = registry
            .receiver_status()
            .await
            .map_err(|e| e.to_string())?;
        let list = registry
            .calls
            .invoke("friendsList", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        self.remember_neighbor_names(&list);
        let friend = friend_status(&list, friendship, &account, &registry.installation_id)
            .map_err(|e| e.to_string())?;
        if let Some(revision) =
            first_claim_revision(&list, friendship, &account, &registry.installation_id)
                .map_err(|e| e.to_string())?
        {
            self.set_neighbor_friend_receiver(
                registry,
                scope,
                metadata,
                friendship,
                receiver::selection::Selection::Set,
                false,
                revision,
                generation,
            )
            .await
            .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if !friend.receiving_here() {
            let name = self
                .neighbor_cell_names()
                .get(friendship)
                .cloned()
                .unwrap_or_else(|| "this friend".into());
            return Err(receiver_refusal(friendship, &name, &friend));
        }
        let changes = self.remember_neighbor_friend_receiver(
            friendship,
            match list["friends"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["friendship_id"] == friendship))
                .and_then(|row| row.get("my_receiver"))
            {
                Some(value) => receiver::friends::FriendReceiver::from_projection(
                    serde_json::from_value(value.clone())
                        .map_err(|_| Error::InvalidEnvelope.to_string())?,
                    &registry.installation_id,
                )?,
                None => receiver::friends::FriendReceiver::from_account(&friend),
            },
            generation,
        )?;
        self.publish_neighbor_friend_receiver_changes(metadata, &registry.uid, changes, generation)
            .await
    }
}

#[cfg(test)]
mod tests;
