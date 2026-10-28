use super::*;

impl ServiceContext {
    pub(in super::super::super) async fn restore_neighbor_concierge(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
    ) -> Result<(), String> {
        self.restore_neighbor_concierge_with(
            metadata,
            uid,
            friendship,
            |provider, model, cwd, access| {
                self.create_neighbor_concierge(friendship, provider, model, cwd, access)
            },
        )
        .await
    }

    async fn restore_neighbor_concierge_with<F, Fut>(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        create: F,
    ) -> Result<(), String>
    where
        F: FnOnce(Option<String>, Option<String>, String, MachineAccess) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        let generation = self.neighbor_history_generation();
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let Some(tab_id) = settings.bindings.get(friendship) else {
            return Ok(());
        };
        self.validate_neighbor_gateway(tab_id)?;
        self.tag_neighbor_root(uid, friendship, tab_id)
            .map_err(|error| error.to_string())?;
        let permission = settings
            .permissions
            .get(friendship)
            .cloned()
            .unwrap_or_default();
        let cwd = crate::service::neighbors::confinement::prepare_folder(
            metadata,
            friendship,
            &permission,
        )
        .await?;
        self.require_neighbor_history_generation(generation)
            .map_err(|error| error.to_string())?;
        if self
            .validate_neighbor_bound_launch(tab_id, &permission, &cwd)
            .is_ok()
        {
            return Ok(());
        }
        let (provider, model) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or("Neighbor concierge disappeared during restore")?;
            (
                tab.agent_type.clone(),
                tab.agent_session_model
                    .clone()
                    .or(tab.agent_provider_resolved_model.clone())
                    .or(tab.restorable_launch.requested_model.clone()),
            )
        };
        self.replace_neighbor_concierge_with(metadata, friendship, |cwd, access| {
            create(provider, model, cwd, access)
        })
        .await?;
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        self.remember_neighbor_concierge_accounts(uid, &settings);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
