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
            |provider, model, cwd, permission, resume, options| {
                self.create_neighbor_concierge(
                    friendship, provider, model, cwd, permission, resume, options,
                )
            },
        )
        .await
    }

    pub(super) async fn restore_neighbor_concierge_with<F, Fut>(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        create: F,
    ) -> Result<(), String>
    where
        F: FnOnce(
            Option<String>,
            Option<String>,
            String,
            Permission,
            Option<String>,
            ConciergeOptions,
        ) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        self.require_neighbor_concierge_receiver(friendship)
            .map_err(|error| error.to_string())?;
        let generation = self.neighbor_history_generation();
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let Some(tab_id) = settings.bindings.get(friendship) else {
            if let Some(saved) = settings.dormant_concierges.get(friendship) {
                let options = saved.options.clone();
                self.replace_neighbor_concierge_with(
                    metadata,
                    friendship,
                    false,
                    options.clone(),
                    |cwd, permission| {
                        create(
                            saved.provider.clone(),
                            saved.model.clone(),
                            cwd,
                            permission,
                            None,
                            options,
                        )
                    },
                )
                .await?;
                let settings = metadata
                    .settings()
                    .await
                    .map_err(|error| error.to_string())?;
                self.remember_neighbor_concierge_accounts(uid, &settings);
            }
            return Ok(());
        };
        self.retire_superseded_neighbor_concierges(metadata, uid, friendship, tab_id)
            .await?;
        self.validate_neighbor_gateway(tab_id)?;
        self.tag_neighbor_root(uid, friendship, tab_id)
            .map_err(|error| error.to_string())?;
        let permission = settings
            .permissions
            .get(friendship)
            .cloned()
            .unwrap_or_default();
        self.remember_neighbor_restore_grant(
            tab_id,
            permission.machine_access == codeterm_neighbors::permissions::MachineAccess::Full,
        );
        let cwd = crate::service::neighbors::confinement::prepare_folder(
            metadata,
            friendship,
            &permission,
        )
        .await?;
        self.require_neighbor_history_generation(generation)
            .map_err(|error| error.to_string())?;
        let digest = initial_prompt::digest(friendship, &permission);
        let same_context = settings.initial_context_digest(friendship) == Some(digest.as_str());
        if self
            .validate_neighbor_bound_launch(tab_id, &permission, &cwd)
            .is_ok()
            && same_context
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
        let unsafe_bypass = permission.machine_access
            != codeterm_neighbors::permissions::MachineAccess::Full
            && self
                .mux
                .read()
                .get_tab(tab_id)
                .is_some_and(|tab| tab.restorable_launch.permission_bypass_requested);
        let resume = (same_context && !unsafe_bypass)
            .then(|| self.neighbor_initial_context_resume(tab_id))
            .flatten();
        let options = self.neighbor_launch_options(&settings, friendship);
        self.replace_neighbor_concierge_with(
            metadata,
            friendship,
            resume.is_some(),
            options.clone(),
            |cwd, permission| create(provider, model, cwd, permission, resume, options),
        )
        .await?;
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        self.remember_neighbor_concierge_accounts(uid, &settings);
        Ok(())
    }

    async fn retire_superseded_neighbor_concierges(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        current: &str,
    ) -> Result<(), String> {
        let generation = self.neighbor_history_generation();
        let snapshot = self.mux.read().snapshot();
        let current_exists = snapshot.tabs.contains_key(current);
        let retired: Vec<_> = snapshot
            .tabs
            .iter()
            .filter_map(|(id, tab)| {
                (id.as_str() != current
                    && tab.neighbor_authority.as_ref().is_some_and(|authority| {
                        authority.uid == uid
                            && authority.friendship_id == friendship
                            && authority
                                .root_tab_id
                                .as_deref()
                                .is_none_or(|root| root != current || !current_exists)
                    }))
                .then(|| id.clone())
            })
            .collect();
        for tab in retired {
            self.require_neighbor_history_generation(generation)
                .map_err(|error| error.to_string())?;
            let settings = metadata
                .settings()
                .await
                .map_err(|error| error.to_string())?;
            if settings.bindings.get(friendship).map(String::as_str) != Some(current) {
                return Err("Neighbor binding changed during concierge retirement".into());
            }
            if self.mux.read().get_tab(&tab).is_none() {
                continue;
            }
            self.close_tab_async(&tab)
                .await
                .map_err(|error| format!("Retiring superseded concierge {tab} failed: {error}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod digest_tests;
