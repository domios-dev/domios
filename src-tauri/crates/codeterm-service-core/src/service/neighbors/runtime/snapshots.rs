use super::*;

impl ServiceContext {
    pub async fn neighbor_sidebar_projection(&self) -> Result<Value, String> {
        let generation = self.neighbor_history_generation();
        let account = self.account().await?;
        let Some(session) = account.session().await else {
            return Ok(json!({}));
        };
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let registry = self.neighbor_cell().registry_snapshot.lock().clone();
        let (receivers, _) = self.neighbor_friend_receiver_snapshot();
        let projection = {
            let names = self.neighbor_cell().names.lock();
            Value::Object(
                names
                    .iter()
                    .filter_map(|(friendship, name)| {
                        let tab = settings.bindings.get(friendship);
                        let receiver = receivers.get(friendship);
                        if tab.is_none() && receiver.is_none() {
                            return None;
                        }
                        let friend = registry.as_ref().and_then(|list| {
                            list["friends"].as_array()?.iter().find(|friend| {
                                friend["friendship_id"].as_str() == Some(friendship.as_str())
                            })
                        });
                        Some((
                            friendship.clone(),
                            json!({
                                "tab_id": tab,
                                "friend_display_name": name,
                                "friend_photo_url": friend.and_then(|friend| friend["photoURL"].as_str()),
                                "paused": self.neighbor_effective_pause(&settings, friendship),
                                "access": registry.as_ref().and_then(|list| list["access"].as_str()),
                                "receiver": receiver,
                            }),
                        ))
                    })
                    .collect(),
            )
        };
        self.require_neighbor_generation(generation)?;
        if !super::super::same_session(account.session().await.as_ref(), &session) {
            return Err("Neighbor account changed".into());
        }
        Ok(projection)
    }

    pub(in crate::service::neighbors) fn patch_neighbor_registry_pause(
        &self,
        friendship: &str,
        paused: bool,
    ) {
        if let Some(snapshot) = self.neighbor_cell().registry_snapshot.lock().as_mut() {
            if let Some(friends) = snapshot["friends"].as_array_mut() {
                for friend in friends {
                    if friend["friendship_id"] == friendship {
                        friend["my_paused"] = json!(paused);
                        friend["paused"] = json!(paused);
                    }
                }
            }
        }
    }
}

impl ServiceContext {
    pub(in crate::service::neighbors) async fn refresh_neighbor_registry_listeners(
        &self,
    ) -> Result<(), String> {
        self.neighbor_operation(super::owner::Operation::RegistryChanged)
            .await
            .map(|_| ())
    }

    pub(in crate::service::neighbors) fn remember_neighbor_registry(&self, list: &Value) {
        if self.require_neighbors_enabled().is_err() {
            return;
        }
        let friends = list["friends"]
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
            .filter_map(|friend| {
                let id = friend["friendship_id"]
                    .as_str()
                    .filter(|id| hex_id(id, 64))?;
                let name: String = friend["displayName"]
                    .as_str()
                    .unwrap_or("Neighbor")
                    .chars()
                    .take(48)
                    .collect();
                let mut row = json!({"friendship_id":id,"displayName":name,"since":friend["since"].as_u64(),"my_paused":friend["my_paused"].as_bool().unwrap_or_else(|| self.neighbor_friend_paused(id)),"their_paused":friend["their_paused"].as_bool().unwrap_or(false)});
                row["photoURL"] = json!(friend["photoURL"].as_str());
                for field in ["my_receiver", "peer_receiver"] {
                    if let Some(value) = friend.get(field) { row[field] = value.clone(); }
                }
                Some(row)
            })
            .collect::<Vec<_>>();
        *self.neighbor_cell().registry_snapshot.lock() = Some(
            json!({"access":if list["access"] == "paid" {"paid"} else {"locked"},"friends":friends}),
        );
    }

    pub(in crate::service::neighbors) fn forget_neighbor_registry(&self) {
        self.neighbor_cell().registry_snapshot.lock().take();
    }

    pub(super) fn forget_neighbor_registry_friendship(&self, friendship: &str) {
        if let Some(list) = self.neighbor_cell().registry_snapshot.lock().as_mut() {
            if let Some(friends) = list["friends"].as_array_mut() {
                friends.retain(|friend| friend["friendship_id"] != friendship);
            }
        }
    }

    pub(in crate::service::neighbors) fn neighbor_registry_receiving(
        &self,
        friendship: &str,
    ) -> bool {
        self.neighbor_cell()
            .registry_snapshot
            .lock()
            .as_ref()
            .is_some_and(|list| {
                list["access"] == "paid"
                    && list["friends"].as_array().is_some_and(|friends| {
                        friends
                            .iter()
                            .any(|friend| friend["friendship_id"] == friendship)
                    })
            })
    }
}

#[cfg(test)]
mod tests;
