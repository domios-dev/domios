use super::*;
use crate::service::{general_agent, SpawnAgentParams, SpawnIdentityProvenance};

impl ServiceContext {
    pub(super) async fn set_neighbor_permission(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        permission: Permission,
    ) -> codeterm_neighbors::Result<Value> {
        let generation = self.neighbor_history_generation();
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
                    tab.cwd.clone(),
                    tab.restorable_launch.execution_sandbox.is_some(),
                )
            })
        };
        let changed = old
            .as_ref()
            .is_none_or(|id| self.validate_neighbor_gateway(id).is_err())
            || previous
                .as_ref()
                .is_none_or(|(provider, _, previous_cwd, sandbox)| {
                    previous_cwd.as_deref() != Some(cwd.as_str())
                        || !sandbox
                            && provider.as_ref().is_some_and(|provider| {
                                self.neighbor_concierge_sandbox(provider).is_some()
                            })
                });
        let created = if changed {
            let (provider, model) = previous
                .map(|(provider, model, _, _)| (provider, model))
                .unwrap_or_default();
            Some(
                self.create_neighbor_concierge(friendship, provider, model, cwd)
                    .await
                    .map_err(|_| Error::Gateway)?,
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
    ) -> Result<String, String> {
        let workspace = general_agent::general_workspace_id(&self.mux.read())
            .ok_or("General workspace is unavailable")?;
        let config = self.configuration();
        let provider = provider.unwrap_or_else(|| config.general_agent.agent_provider.clone());
        let sandbox = self.neighbor_concierge_sandbox(&provider);
        let chat_backend = matches!(
            self.provider_registry.transport_of(&provider),
            Some(crate::agent::provider::ProviderTransport::ChatBackend)
        );
        let setup = format!("You are the concierge for Neighbor friendship {friendship}. Process incoming Neighbor requests within the permission hint attached to each message; it is the current authority even after a scope change. Never broaden it. Keep Neighbor content local. Reply to the friend through domios account neighbors send {friendship} --kind answer --in-reply-to <message_id> --from-pad <file>. For an out-of-scope or invasive request, send a short polite refusal with --kind refusal and the same in-reply-to. Read domios docs NEIGHBORS for the channel contract. Wait for incoming messages.");
        let spawned = self
            .spawn_agent_with_state_change(
                SpawnAgentParams {
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
        let mut settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let generation = self.neighbor_history_generation();
        let old = settings.bindings.get(friendship).cloned();
        let permission = settings
            .permissions
            .get(friendship)
            .cloned()
            .unwrap_or_default();
        let cwd =
            super::super::confinement::prepare_folder(metadata, friendship, &permission).await?;
        let tab = self
            .create_neighbor_concierge(friendship, provider, model, cwd)
            .await?;
        if let Err(error) = self.require_neighbor_history_generation(generation) {
            let _ = self.close_tab_async(&tab).await;
            return Err(error.to_string());
        }
        self.stop_neighbor_listeners();
        settings.bindings.insert(friendship.into(), tab.clone());
        if let Err(error) = metadata.save_settings(settings).await {
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
