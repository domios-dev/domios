use super::*;
use crate::service::{general_agent, SpawnAgentParams, SpawnIdentityProvenance};

pub(super) mod initial_prompt;
mod options;
mod restore;
mod retirement;
use super::super::storage::ConciergeOptions;

pub(super) fn launch_error(error: String) -> Error {
    if error.starts_with("neighbor_machine_access_unsupported:") {
        Error::MachineAccessUnsupported
    } else if error.starts_with("receiving_elsewhere:") {
        Error::ReceivingElsewhere
    } else {
        Error::Gateway
    }
}

impl ServiceContext {
    pub(super) async fn bind_neighbor_concierge(
        &self,
        metadata: &Metadata,
        transport: &FirebaseTransport,
        friendship: &str,
        gateway: &str,
    ) -> codeterm_neighbors::Result<Value> {
        self.require_neighbor_concierge_receiver(friendship)?;
        let _mutation = self
            .begin_neighbor_mutation(friendship)
            .await
            .map_err(|_| Error::Gateway)?;
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        self.validate_neighbor_gateway(gateway)
            .map_err(|_| Error::Gateway)?;
        let generation = self.neighbor_history_generation();
        transport.peer_keys(friendship).await?;
        let mut settings = metadata.settings().await?;
        let previous = settings.bindings.get(friendship).cloned();
        let permission = settings
            .permissions
            .get(friendship)
            .cloned()
            .unwrap_or_default();
        let cwd = super::super::confinement::prepare_folder(metadata, friendship, &permission)
            .await
            .map_err(|_| Error::InvalidRequest)?;
        self.validate_neighbor_bound_launch(gateway, &permission, &cwd)
            .map_err(launch_error)?;
        if !self
            .mux
            .read()
            .get_tab(gateway)
            .is_some_and(|tab| initial_prompt::prompt_matches(tab, friendship, &permission))
        {
            return Err(Error::InvalidRequest);
        }
        self.require_neighbor_history_generation(generation)?;
        self.require_neighbor_concierge_receiver(friendship)?;
        self.tag_neighbor_root(&transport.uid, friendship, gateway)?;
        settings.bindings.insert(friendship.into(), gateway.into());
        settings.dormant_concierges.remove(friendship);
        settings.initial_contexts.insert(
            friendship.into(),
            super::super::storage::InitialContext {
                gateway_tab: gateway.into(),
                digest: initial_prompt::digest(friendship, &permission),
                launch_options: Some(self.neighbor_tab_options(gateway)),
            },
        );
        metadata.save_settings(settings).await?;
        self.reconcile_neighbor_handling_restore(metadata, friendship, generation)
            .await?;
        if let Some(previous) = previous.filter(|previous| previous != gateway) {
            self.retire_neighbor_children(&previous)
                .await
                .map_err(|_| Error::Gateway)?;
            if self.mux.read().get_tab(&previous).is_some() {
                self.close_tab_async(&previous)
                    .await
                    .map_err(|_| Error::Gateway)?;
            }
        }
        self.settle_neighbor_pause(metadata, friendship).await?;
        if let Err(error) = self.require_neighbor_concierge_receiver(friendship) {
            self.retire_neighbor_concierge(metadata, &transport.uid, friendship, generation)
                .await
                .map_err(|_| Error::Gateway)?;
            return Err(error);
        }
        self.stop_neighbor_concierge_listener(friendship);
        self.emit_state_changed(crate::service::MuxEmitCause::TabMetadata);
        self.ui.emit(
            "neighbor-agent-changed",
            json!({"friendship_id":friendship,"gateway_tab":gateway}),
        );
        Ok(json!({"status":"bound"}))
    }

    pub(super) async fn set_neighbor_permission(
        &self,
        metadata: &Metadata,
        transport: &FirebaseTransport,
        friendship: &str,
        permission: Permission,
    ) -> codeterm_neighbors::Result<Value> {
        self.require_neighbor_concierge_receiver(friendship)?;
        let _mutation = self
            .begin_neighbor_mutation(friendship)
            .await
            .map_err(|_| Error::Gateway)?;
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        permission.validate()?;
        {
            let mux = self.mux.read();
            if permission
                .workspaces
                .as_ref()
                .is_some_and(|ids| ids.iter().any(|id| mux.get_workspace(id).is_none()))
            {
                return Err(Error::InvalidRequest);
            }
        }
        let generation = self.neighbor_history_generation();
        self.validate_neighbor_memory_areas(&permission).await?;
        transport.peer_keys(friendship).await?;
        self.require_neighbor_history_generation(generation)?;
        let settings = metadata.settings().await?;
        let provider = settings
            .bindings
            .get(friendship)
            .and_then(|id| {
                self.mux
                    .read()
                    .get_tab(id)
                    .and_then(|tab| tab.agent_type.clone())
            })
            .or_else(|| {
                settings
                    .dormant_concierges
                    .get(friendship)
                    .and_then(|saved| saved.provider.clone())
            })
            .unwrap_or_else(|| self.configuration().general_agent.agent_provider.clone());
        let model = settings
            .bindings
            .get(friendship)
            .and_then(|id| {
                self.mux.read().get_tab(id).and_then(|tab| {
                    tab.agent_session_model
                        .clone()
                        .or(tab.agent_provider_resolved_model.clone())
                        .or(tab.restorable_launch.requested_model.clone())
                })
            })
            .or_else(|| {
                settings
                    .dormant_concierges
                    .get(friendship)
                    .and_then(|saved| saved.model.clone())
            });
        self.validate_neighbor_launch_options(
            &provider,
            model.as_deref(),
            &self.neighbor_launch_options(&settings, friendship),
        )
        .map_err(|_| Error::InvalidRequest)?;
        let result = self
            .apply_neighbor_permission_with(
                metadata,
                &transport.uid,
                friendship,
                permission,
                generation,
                |provider, model, cwd, permission, resume, options| {
                    self.create_neighbor_concierge(
                        friendship, provider, model, cwd, permission, resume, options,
                    )
                },
            )
            .await?;
        if let Err(error) = self.require_neighbor_concierge_receiver(friendship) {
            self.retire_neighbor_concierge(metadata, &transport.uid, friendship, generation)
                .await
                .map_err(|_| Error::Gateway)?;
            return Err(error);
        }
        Ok(result)
    }

    async fn apply_neighbor_permission_with<F, Fut>(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        permission: Permission,
        generation: u64,
        create: F,
    ) -> codeterm_neighbors::Result<Value>
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
        let cwd = super::super::confinement::prepare_folder(metadata, friendship, &permission)
            .await
            .map_err(|_| Error::InvalidRequest)?;
        self.require_neighbor_history_generation(generation)?;
        let mut settings = metadata.settings().await?;
        self.remember_neighbor_concierge_accounts(uid, &settings);
        let old = settings.bindings.get(friendship).cloned();
        let options = self.neighbor_launch_options(&settings, friendship);
        let previous = {
            let mux = self.mux.read();
            old.as_ref().and_then(|id| mux.get_tab(id)).map(|tab| {
                (
                    tab.agent_type.clone(),
                    tab.agent_session_model
                        .clone()
                        .or(tab.agent_provider_resolved_model.clone())
                        .or(tab.restorable_launch.requested_model.clone()),
                )
            })
        };
        if let Some((Some(provider), _)) = &previous {
            let _ = self
                .neighbor_concierge_sandbox(provider, permission.machine_access)
                .map_err(launch_error)?;
        }
        let (provider, model) = previous
            .or_else(|| {
                settings
                    .dormant_concierges
                    .get(friendship)
                    .map(|saved| (saved.provider.clone(), saved.model.clone()))
            })
            .unwrap_or_default();
        self.stop_neighbor_concierge_listener(friendship);
        let created = match create(
            provider,
            model,
            cwd,
            permission.clone(),
            None,
            options.clone(),
        )
        .await
        {
            Ok(tab) => Some(tab),
            Err(error) => {
                log::debug!(
                    "[neighbors] permission apply restart friendship={friendship}: {error}"
                );
                return Err(launch_error(error));
            }
        };
        if let Some(old) = &old {
            if let Err(error) = self.retire_neighbor_children(old).await {
                if let Some(created) = &created {
                    let _ = self.close_tab_async(created).await;
                }
                log::debug!("[neighbors] retiring concierge descendants root={old}: {error}");
                return Err(Error::Gateway);
            }
        }
        if let Some(tab) = &created {
            settings.bindings.insert(friendship.into(), tab.clone());
            settings.dormant_concierges.remove(friendship);
        }
        if let Some(tab) = &created {
            settings.initial_contexts.insert(
                friendship.into(),
                super::super::storage::InitialContext {
                    gateway_tab: tab.clone(),
                    digest: initial_prompt::digest(friendship, &permission),
                    launch_options: Some(options.clone()),
                },
            );
        }
        settings.permissions.insert(friendship.into(), permission);
        let gateway_tab = settings.bindings.get(friendship).cloned();
        if let Err(error) = self.require_neighbor_history_generation(generation) {
            if let Some(tab) = &created {
                let _ = self.close_tab_async(tab).await;
            }
            return Err(error);
        }
        if let Err(error) = metadata.save_settings(settings.clone()).await {
            if let Some(tab) = &created {
                let _ = self.close_tab_async(tab).await;
            }
            return Err(error);
        }
        self.remember_neighbor_concierge_accounts(uid, &settings);
        if created.is_some() {
            self.reconcile_neighbor_handling_restore(metadata, friendship, generation)
                .await?;
            self.stop_neighbor_concierge_listener(friendship);
            if let Some(old) = old.filter(|id| {
                Some(id) != gateway_tab.as_ref() && self.mux.read().get_tab(id).is_some()
            }) {
                if let Err(error) = self.close_tab_async(&old).await {
                    log::debug!("[neighbors] retired concierge close tab={old}: {error}");
                }
            }
            self.emit_state_changed(crate::service::MuxEmitCause::TabMetadata);
            self.ui.emit(
                "neighbor-agent-changed",
                json!({"friendship_id":friendship,"gateway_tab":gateway_tab}),
            );
        }
        self.ui.emit(
            "neighbor-permission-changed",
            json!({"friendship_id":friendship}),
        );
        Ok(json!({"status":"saved","gateway_tab":gateway_tab,"history_retained":false}))
    }

    fn neighbor_initial_context_resume(&self, id: &str) -> Option<String> {
        let mux = self.mux.read();
        let tab = mux.get_tab(id)?;
        let provider = tab.agent_type.as_deref()?;
        self.agent_registry
            .with_provider(provider, |p| p.supports_initial_prompt_update_on_resume())
            .unwrap_or(false)
            .then(|| {
                crate::service::restored_launch::restored_session_target(tab).map(str::to_owned)
            })
            .flatten()
    }

    pub(super) async fn create_neighbor_concierge(
        &self,
        friendship: &str,
        provider: Option<String>,
        model: Option<String>,
        cwd: String,
        permission: Permission,
        resume_session_id: Option<String>,
        options: ConciergeOptions,
    ) -> Result<String, String> {
        self.require_neighbor_concierge_receiver(friendship)
            .map_err(|error| error.to_string())?;
        let workspace = general_agent::general_workspace_id(&self.mux.read())
            .ok_or("General workspace is unavailable")?;
        let config = self.configuration();
        let provider = provider.unwrap_or_else(|| config.general_agent.agent_provider.clone());
        self.validate_neighbor_launch_options(&provider, model.as_deref(), &options)?;
        let sandbox = self.neighbor_concierge_sandbox(&provider, permission.machine_access)?;
        let chat_backend = matches!(
            self.provider_registry.transport_of(&provider),
            Some(crate::agent::provider::ProviderTransport::ChatBackend)
        );
        let setup = initial_prompt::initial_prompt(friendship, &permission);
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .ok_or("Neighbor account unavailable")?;
        let session = account
            .session()
            .await
            .ok_or("Neighbor account is signed out")?;
        let mut access = SpawnAgentParams::default();
        self.configure_neighbor_launch_access(&provider, permission.machine_access, &mut access)?;
        let spawned = self
            .spawn_agent_with_state_change(
                SpawnAgentParams {
                    neighbor_authority: Some(crate::multiplexer::types::NeighborAuthority {
                        uid: session.identity.uid,
                        friendship_id: friendship.into(),
                        root_tab_id: None,
                    }),
                    agent: Some(provider),
                    resume_session_id,
                    model,
                    reasoning_effort: options.reasoning_effort,
                    speed: options.service_tier,
                    workspace_id: Some(workspace),
                    cwd: Some(cwd),
                    sandbox,
                    skip_permissions: access.skip_permissions,
                    permission_bypass_authorization: access.permission_bypass_authorization,
                    title: Some("Concierge".into()),
                    title_provenance: Some(SpawnIdentityProvenance::UserSupplied),
                    description: Some(format!("Handles requests from Neighbor {friendship}")),
                    task: (!chat_backend).then(|| setup.clone()),
                    system_prompt: chat_backend.then_some(setup),
                    user_created: true,
                    activate: Some(false),
                    ..Default::default()
                },
                None,
                false,
            )
            .await?;
        if let Err(error) = self.require_neighbor_concierge_receiver(friendship) {
            self.revoke_tab_credential(&spawned.tab_id);
            let _ = self.close_tab_async(&spawned.tab_id).await;
            return Err(error.to_string());
        }
        if self.neighbor_friend_paused(friendship) {
            if let Err(error) = self.pause_neighbor_tab(&spawned.tab_id, friendship).await {
                let _ = self.close_tab_async(&spawned.tab_id).await;
                return Err(format!("Pausing new Neighbor concierge failed: {error}"));
            }
        }
        Ok(spawned.tab_id)
    }

    pub(super) async fn replace_neighbor_concierge(
        &self,
        metadata: &Metadata,
        friendship: &str,
        provider: Option<String>,
        model: Option<String>,
        options: ConciergeOptions,
    ) -> Result<String, String> {
        self.replace_neighbor_concierge_with(
            metadata,
            friendship,
            false,
            options.clone(),
            |cwd, permission| {
                self.create_neighbor_concierge(
                    friendship, provider, model, cwd, permission, None, options,
                )
            },
        )
        .await
    }

    async fn replace_neighbor_concierge_with<F, Fut>(
        &self,
        metadata: &Metadata,
        friendship: &str,
        retire_before_create: bool,
        options: ConciergeOptions,
        create: F,
    ) -> Result<String, String>
    where
        F: FnOnce(String, Permission) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        self.require_neighbor_concierge_receiver(friendship)
            .map_err(|error| error.to_string())?;
        let _mutation = self.begin_neighbor_mutation(friendship).await?;
        let generation = self.neighbor_history_generation();
        let mut settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let old = settings.bindings.get(friendship).cloned();
        let permission = settings
            .permissions
            .get(friendship)
            .cloned()
            .unwrap_or_default();
        let cwd =
            super::super::confinement::prepare_folder(metadata, friendship, &permission).await?;
        self.require_neighbor_history_generation(generation)
            .map_err(|error| error.to_string())?;
        if retire_before_create {
            self.stop_neighbor_concierge_listener(friendship);
            if let Some(old) = &old {
                self.retire_neighbor_children(old).await?;
                self.close_tab_async(old).await?;
            }
        }
        let tab = match create(cwd, permission.clone()).await {
            Ok(tab) => tab,
            Err(error) => {
                if retire_before_create {
                    settings.bindings.remove(friendship);
                    settings.initial_contexts.remove(friendship);
                    metadata
                        .save_settings(settings)
                        .await
                        .map_err(|error| error.to_string())?;
                    self.ui.emit(
                        "neighbor-agent-changed",
                        json!({"friendship_id":friendship,"gateway_tab":null}),
                    );
                }
                return Err(error);
            }
        };
        if let Err(error) = self.require_neighbor_history_generation(generation) {
            let _ = self.close_tab_async(&tab).await;
            return Err(error.to_string());
        }
        self.stop_neighbor_concierge_listener(friendship);
        settings.bindings.insert(friendship.into(), tab.clone());
        settings.dormant_concierges.remove(friendship);
        settings.initial_contexts.insert(
            friendship.into(),
            super::super::storage::InitialContext {
                gateway_tab: tab.clone(),
                digest: initial_prompt::digest(friendship, &permission),
                launch_options: Some(options),
            },
        );
        if let Err(error) = metadata.save_settings(settings.clone()).await {
            let _ = self.close_tab_async(&tab).await;
            return Err(error.to_string());
        }
        if let Err(error) = self.require_neighbor_concierge_receiver(friendship) {
            let uid = self
                .mux
                .read()
                .get_tab(&tab)
                .and_then(|tab| tab.neighbor_authority.as_ref())
                .map(|authority| authority.uid.clone())
                .ok_or_else(|| error.to_string())?;
            self.retire_neighbor_concierge(metadata, &uid, friendship, generation)
                .await?;
            return Err(error.to_string());
        }
        self.reconcile_neighbor_handling_restore(metadata, friendship, generation)
            .await
            .map_err(|error| error.to_string())?;
        if let Some(old) = old.filter(|old| old != &tab && self.mux.read().get_tab(old).is_some()) {
            if let Err(error) = self.close_tab_async(&old).await {
                self.emit_state_changed(crate::service::MuxEmitCause::TabMetadata);
                self.ui.emit(
                    "neighbor-agent-changed",
                    json!({"friendship_id":friendship,"gateway_tab":tab}),
                );
                return Err(format!(
                    "Concierge rebound to {tab}; retiring {old} failed: {error}"
                ));
            }
        }
        self.emit_state_changed(crate::service::MuxEmitCause::TabMetadata);
        self.ui.emit(
            "neighbor-agent-changed",
            json!({"friendship_id":friendship,"gateway_tab":tab}),
        );
        Ok(tab)
    }
}
