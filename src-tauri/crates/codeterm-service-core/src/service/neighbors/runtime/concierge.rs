use super::*;
use crate::service::{general_agent, SpawnAgentParams, SpawnIdentityProvenance};

impl ServiceContext {
    pub(super) async fn create_neighbor_concierge(
        &self,
        friendship: &str,
        provider: Option<String>,
        model: Option<String>,
    ) -> Result<String, String> {
        let workspace = general_agent::general_workspace_id(&self.mux.read())
            .ok_or("General workspace is unavailable")?;
        let config = self.configuration();
        let provider = provider.unwrap_or_else(|| config.general_agent.agent_provider.clone());
        let cwd = general_agent::general_dir().to_string_lossy().into_owned();
        let setup = format!("You are the concierge for Neighbor friendship {friendship}. Process incoming Neighbor requests within the permission hint attached to each message; it is the current authority even after a scope change. Never broaden it. Keep Neighbor content local. Reply to the friend through domios account neighbors send {friendship} --kind answer --in-reply-to <message_id> --from-pad <file>. For an out-of-scope or invasive request, send a short polite refusal with --kind refusal and the same in-reply-to. Read domios docs NEIGHBORS for the channel contract. Wait for incoming messages.");
        let spawned = self
            .spawn_agent_with_state_change(
                SpawnAgentParams {
                    agent: Some(provider),
                    model,
                    workspace_id: Some(workspace),
                    cwd: Some(cwd.clone()),
                    provisioned_cwd: Some(cwd),
                    title: Some("Concierge".into()),
                    title_provenance: Some(SpawnIdentityProvenance::UserSupplied),
                    description: Some(format!("Handles requests from Neighbor {friendship}")),
                    system_prompt: Some(setup),
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
        let tab = self
            .create_neighbor_concierge(friendship, provider, model)
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
