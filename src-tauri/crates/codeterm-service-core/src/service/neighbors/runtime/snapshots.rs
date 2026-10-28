use super::*;

impl ServiceContext {
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
                Some(
                    json!({"friendship_id":id,"displayName":name,"since":friend["since"].as_u64(),"my_paused":friend["my_paused"].as_bool().unwrap_or_else(|| self.neighbor_friend_paused(id)),"their_paused":friend["their_paused"].as_bool().unwrap_or(false)}),
                )
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
