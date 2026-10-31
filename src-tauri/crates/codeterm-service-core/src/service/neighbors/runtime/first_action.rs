use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;

pub(super) fn friend_status(
    list: &Value,
    friendship: &str,
    installation: &str,
) -> codeterm_neighbors::Result<ReceiverStatus> {
    let friend = list["friends"]
        .as_array()
        .filter(|rows| rows.len() <= 200)
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
        None => Err(Error::InvalidEnvelope),
    }
}

pub(super) fn receiver_refusal(friendship: &str, name: &str, friend: &ReceiverStatus) -> String {
    match friend.receiver_installation_id {
        Some(_) => format!("receiving_elsewhere: {name} is received on {}. Move here? Use domios account neighbors receive-here {friendship} --expected-receiver-revision {} --takeover-confirmed only after the owner confirms.", friend.receiver_machine_label.as_deref().unwrap_or("another machine"), friend.receiver_revision),
        None => format!("receiver_not_chosen: No machine receives messages from {name}. Receive them here? Use domios account neighbors receive-here {friendship} --expected-receiver-revision {}.", friend.receiver_revision),
    }
}

impl ServiceContext {
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
        let list = registry
            .calls
            .invoke("friendsList", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        let friend = friend_status(&list, friendship, &registry.installation_id)
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
    ) -> Result<Option<receiver::selection::Committed>, String> {
        let list = registry
            .calls
            .invoke("friendsList", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        self.remember_neighbor_names(&list);
        let friend = friend_status(&list, friendship, &registry.installation_id)
            .map_err(|e| e.to_string())?;
        if friend.receiver_installation_id.is_none() {
            let committed = self
                .set_neighbor_friend_receiver(
                    registry,
                    scope,
                    metadata,
                    friendship,
                    receiver::selection::Selection::Set,
                    false,
                    friend.receiver_revision,
                    generation,
                )
                .await
                .map_err(|e| e.to_string())?;
            return Ok(Some(committed));
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
                None => return Err(Error::InvalidEnvelope.to_string()),
            },
            generation,
        )?;
        self.publish_neighbor_friend_receiver_changes(metadata, &registry.uid, changes, generation)
            .await?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
