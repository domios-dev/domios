use super::*;

impl NeighborRuntimeCommand {
    pub(super) fn is_local_read(&self) -> bool {
        matches!(
            self,
            Self::History { .. }
                | Self::Permission { .. }
                | Self::NotificationPreference { .. }
                | Self::Status
        )
    }
}

impl ServiceContext {
    pub(super) async fn neighbor_local_read(
        &self,
        command: &NeighborRuntimeCommand,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<Value, String> {
        let generation = self.neighbor_history_generation();
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned()
            .ok_or("Sign in with a hosted account to use Neighbors")?;
        let session = account
            .session()
            .await
            .filter(|session| session.identity.provider != "local")
            .ok_or("Sign in with a hosted account to use Neighbors")?;
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        self.authorize_neighbor_metadata(command, caller, nonce, &metadata)
            .await?;
        let value = self
            .neighbor_local_metadata(command, &metadata, generation)
            .await
            .map_err(|error| error.to_string())?;
        self.require_neighbor_generation(generation)?;
        if !super::super::same_session(account.session().await.as_ref(), &session) {
            return Err("Neighbor account changed".into());
        }
        Ok(value)
    }

    async fn neighbor_local_metadata(
        &self,
        command: &NeighborRuntimeCommand,
        metadata: &Metadata,
        generation: u64,
    ) -> codeterm_neighbors::Result<Value> {
        match command {
            NeighborRuntimeCommand::History {
                friendship_id,
                before,
                limit,
            } => {
                self.reconcile_neighbor_handling_restore(metadata, friendship_id, generation)
                    .await?;
                let page = metadata
                    .history_page(friendship_id, before.as_deref(), limit.unwrap_or(50))
                    .await?;
                let summary=page["entries"].as_array().map(|entries|entries.iter().filter(|entry|entry["direction"]=="in"&&!entry["handling_state"].is_null()).map(|entry|json!({"message_id":entry["message_id"],"handling_state":entry["handling_state"],"handling_reason":entry["handling_reason"],"handling_updated_at":entry["handling_updated_at"]})).collect::<Vec<_>>()).unwrap_or_default();
                self.remember_neighbor_handling(friendship_id, json!(summary), generation);
                Ok(page)
            }
            NeighborRuntimeCommand::Permission { friendship_id } => {
                if !hex_id(friendship_id, 64) {
                    return Err(Error::InvalidEnvelope);
                }
                let settings = metadata.settings().await?;
                let permission = settings
                    .permissions
                    .get(friendship_id)
                    .cloned()
                    .unwrap_or_default();
                self.neighbor_permission_projection(metadata, friendship_id, &permission, &settings)
            }
            NeighborRuntimeCommand::NotificationPreference { friendship_id } => {
                if !hex_id(friendship_id, 64) {
                    return Err(Error::InvalidEnvelope);
                }
                Ok(
                    json!({"friendship_id":friendship_id,"notify":metadata.settings().await?.neighbor_notify(friendship_id)}),
                )
            }
            NeighborRuntimeCommand::Status => {
                let settings = metadata.settings().await?;
                let receiver = self.neighbor_cell().receiver.lock().clone();
                let (friend_receivers, friend_receivers_version) =
                    self.neighbor_friend_receiver_snapshot();
                let configured_label = self.configuration().mesh.instance_name.clone();
                let machine_label = if configured_label.trim().is_empty() {
                    hostname::get()
                        .ok()
                        .and_then(|name| name.into_string().ok())
                        .unwrap_or_else(|| "This machine".into())
                } else {
                    configured_label
                };
                let mut ids: std::collections::BTreeSet<String> =
                    settings.bindings.keys().cloned().collect();
                ids.extend(self.neighbor_cell().names.lock().keys().cloned());
                ids.extend(self.neighbor_cell().pending_counts.lock().keys().cloned());
                let friends = json!({"friends":ids.into_iter().map(|id| json!({"friendship_id":id})).collect::<Vec<_>>()});
                let concierges = receiver
                    .as_ref()
                    .map(|receiver| self.neighbor_concierge_views(&settings, receiver, &friends))
                    .unwrap_or_else(|| json!({}));
                let notification_preferences =
                    Self::neighbor_notification_preferences(&settings, &friends);
                let here = self.configuration().neighbors.enabled
                    && receiver
                        .as_ref()
                        .is_some_and(|receiver| receiver.receiving_here());
                let key = receiver
                    .as_ref()
                    .filter(|_| here)
                    .and_then(|receiver| receiver.receiver_key_id.clone())
                    .or_else(|| {
                        friend_receivers.as_object().and_then(|friends| {
                            friends.keys().find_map(|id| {
                                self.neighbor_friend_receiver(id)
                                    .filter(|receiver| receiver.receiving_here())
                                    .and_then(|receiver| receiver.receiver_key_id)
                            })
                        })
                    });
                let states = self.neighbor_cell().states.lock().clone();
                let failures = self.neighbor_cell().failures.lock().clone();
                let permissions: serde_json::Map<String, Value> = settings
                    .permissions
                    .iter()
                    .map(|(friendship, permission)| {
                        self.neighbor_permission_projection(
                            metadata, friendship, permission, &settings,
                        )
                        .map(|value| (friendship.clone(), value))
                    })
                    .collect::<codeterm_neighbors::Result<_>>()?;
                Ok(
                    json!({"watches":self.neighbor_cell().watch_states.lock().clone(),"incoming_handling":self.neighbor_handling_status(),"enabled":self.configuration().neighbors.enabled,"enrolled":key.is_some(),"verifier_key_id":key,"bindings":settings.bindings,"permissions":permissions,"concierges":concierges,"friend_receivers":friend_receivers,"friend_receivers_version":friend_receivers_version,"machine_label":machine_label,"notification_preferences":notification_preferences,"listeners":states,"listener_errors":failures,"receiving_here":here,"receiver_machine_label":receiver.as_ref().and_then(|receiver| receiver.receiver_machine_label.clone()),"receiving_enabled":receiver.as_ref().is_some_and(|receiver| receiver.receiving_enabled),"revision":receiver.as_ref().map(|receiver| receiver.receiver_revision).unwrap_or(0),"receiver_status_known":receiver.is_some()}),
                )
            }
            _ => Err(Error::InvalidEnvelope),
        }
    }
}

#[cfg(test)]
mod tests;
