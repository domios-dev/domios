use super::*;
use crate::service::{general_agent, SpawnAgentParams, SpawnIdentityProvenance};
use codeterm_neighbors::permissions::MachineAccess;

mod restore;

pub(super) fn launch_error(error: String) -> Error {
    if error.starts_with("neighbor_machine_access_unsupported:") {
        Error::MachineAccessUnsupported
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
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        self.validate_neighbor_gateway(gateway)
            .map_err(|_| Error::Gateway)?;
        let generation = self.neighbor_history_generation();
        transport.peer_keys(friendship).await?;
        let mut settings = metadata.settings().await?;
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
        self.require_neighbor_history_generation(generation)?;
        self.tag_neighbor_root(&transport.uid, friendship, gateway)?;
        settings.bindings.insert(friendship.into(), gateway.into());
        metadata.save_settings(settings).await?;
        self.stop_neighbor_listeners();
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
        let uid = &transport.uid;
        let cwd = super::super::confinement::prepare_folder(metadata, friendship, &permission)
            .await
            .map_err(|_| Error::InvalidRequest)?;
        self.require_neighbor_history_generation(generation)?;
        let mut settings = metadata.settings().await?;
        self.remember_neighbor_concierge_accounts(uid, &settings);
        let old = settings.bindings.get(friendship).cloned();
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
        let changed = old.as_ref().is_none_or(|id| {
            self.validate_neighbor_gateway(id).is_err()
                || self
                    .validate_neighbor_bound_launch(id, &permission, &cwd)
                    .is_err()
        });
        let created = if changed {
            let (provider, model) = previous.unwrap_or_default();
            Some(
                self.create_neighbor_concierge(
                    friendship,
                    provider,
                    model,
                    cwd,
                    permission.machine_access,
                )
                .await
                .map_err(launch_error)?,
            )
        } else {
            None
        };
        if let Some(tab) = &created {
            settings.bindings.insert(friendship.into(), tab.clone());
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
            self.stop_neighbor_listeners();
            if let Some(old) = old.filter(|id| Some(id) != gateway_tab.as_ref()) {
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
        Ok(json!({"status":"saved","gateway_tab":gateway_tab}))
    }

    pub(super) async fn create_neighbor_concierge(
        &self,
        friendship: &str,
        provider: Option<String>,
        model: Option<String>,
        cwd: String,
        access: MachineAccess,
    ) -> Result<String, String> {
        let workspace = general_agent::general_workspace_id(&self.mux.read())
            .ok_or("General workspace is unavailable")?;
        let config = self.configuration();
        let provider = provider.unwrap_or_else(|| config.general_agent.agent_provider.clone());
        let sandbox = self.neighbor_concierge_sandbox(&provider, access)?;
        let chat_backend = matches!(
            self.provider_registry.transport_of(&provider),
            Some(crate::agent::provider::ProviderTransport::ChatBackend)
        );
        let setup = format!("You are the concierge for Neighbor friendship {friendship}. Process incoming Neighbor requests within the permission hint attached to each message; it is the current authority even after a scope change. Never broaden it. Keep Neighbor content local. Reply to the friend through domios account neighbors send {friendship} --kind answer --in-reply-to <message_id> --from-pad - and provide the reply through stdin, without writing a file. For an out-of-scope or invasive request, send a short polite refusal with --kind refusal and the same in-reply-to. Wait for incoming messages.");
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .ok_or("Neighbor account unavailable")?;
        let session = account
            .session()
            .await
            .ok_or("Neighbor account is signed out")?;
        let spawned = self
            .spawn_agent_with_state_change(
                SpawnAgentParams {
                    neighbor_authority: Some(crate::multiplexer::types::NeighborAuthority {
                        uid: session.identity.uid,
                        friendship_id: friendship.into(),
                        root_tab_id: None,
                    }),
                    agent: Some(provider),
                    model,
                    workspace_id: Some(workspace),
                    cwd: Some(cwd),
                    sandbox,
                    skip_permissions: Some(false),
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
        Ok(spawned.tab_id)
    }

    pub(super) async fn replace_neighbor_concierge(
        &self,
        metadata: &Metadata,
        friendship: &str,
        provider: Option<String>,
        model: Option<String>,
    ) -> Result<String, String> {
        self.replace_neighbor_concierge_with(metadata, friendship, |cwd, access| {
            self.create_neighbor_concierge(friendship, provider, model, cwd, access)
        })
        .await
    }

    async fn replace_neighbor_concierge_with<F, Fut>(
        &self,
        metadata: &Metadata,
        friendship: &str,
        create: F,
    ) -> Result<String, String>
    where
        F: FnOnce(String, MachineAccess) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
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
        let tab = create(cwd, permission.machine_access).await?;
        if let Err(error) = self.require_neighbor_history_generation(generation) {
            let _ = self.close_tab_async(&tab).await;
            return Err(error.to_string());
        }
        self.stop_neighbor_listeners();
        settings.bindings.insert(friendship.into(), tab.clone());
        if let Err(error) = metadata.save_settings(settings.clone()).await {
            let _ = self.close_tab_async(&tab).await;
            return Err(error.to_string());
        }
        if let Some(old) = old.filter(|old| old != &tab) {
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
