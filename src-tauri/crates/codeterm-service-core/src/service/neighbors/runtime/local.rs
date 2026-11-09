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
                    .history_response_page(friendship_id, before.as_deref(), limit.unwrap_or(50))
                    .await?;
                let summary = page
                    .entries
                    .iter()
                    .filter(|entry| entry.direction == "in")
                    .filter_map(|entry| {
                        entry.handling_state.map(|state| {
                            codeterm_neighbors::responses::NeighborHandlingSummary {
                                message_id: entry.message_id.clone(),
                                handling_state: state,
                                handling_reason: entry.handling_reason.clone(),
                                handling_updated_at: entry.handling_updated_at,
                            }
                        })
                    })
                    .collect();
                self.remember_neighbor_handling(friendship_id, summary, generation);
                Ok(json!(page))
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
                    .map(|permission| json!(permission))
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
                let concierges = self.neighbor_concierge_views(&settings, &friends);
                let notification_preferences =
                    Self::neighbor_notification_preferences(&settings, &friends);
                let key = friend_receivers.keys().find_map(|id| {
                    self.neighbor_friend_receiver(id)
                        .filter(|receiver| receiver.receiving_here())
                        .and_then(|receiver| receiver.receiver_encryption_key_id)
                });
                let states = self.neighbor_cell().states.lock().clone();
                let failures = self.neighbor_cell().failures.lock().clone();
                let permissions: std::collections::BTreeMap<
                    String,
                    codeterm_neighbors::responses::NeighborPermission,
                > = settings
                    .permissions
                    .iter()
                    .map(|(friendship, permission)| {
                        self.neighbor_permission_projection(
                            metadata, friendship, permission, &settings,
                        )
                        .map(|value| (friendship.clone(), value))
                    })
                    .collect::<codeterm_neighbors::Result<_>>()?;
                Ok(json!(codeterm_neighbors::responses::ChannelStatus {
                    watches: self.neighbor_cell().watch_states.lock().clone(),
                    incoming_handling: self.neighbor_handling_status(),
                    enabled: self.configuration().neighbors.enabled,
                    enrolled: key.is_some(),
                    bindings: settings.bindings,
                    permissions,
                    concierges,
                    friend_receivers,
                    friend_receivers_version,
                    machine_label,
                    notification_preferences,
                    listeners: states.into_iter().collect(),
                    listener_errors: failures.into_iter().collect(),
                }))
            }
            _ => Err(Error::InvalidEnvelope),
        }
    }
}

#[cfg(test)]
mod tests;
