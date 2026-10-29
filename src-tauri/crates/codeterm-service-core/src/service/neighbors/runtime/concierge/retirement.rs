use super::super::super::storage::{ConciergeReceiver, DormantConcierge};
use super::*;
use std::collections::BTreeSet;

impl ServiceContext {
    pub(crate) fn require_neighbor_concierge_receiver(
        &self,
        friendship: &str,
    ) -> codeterm_neighbors::Result<()> {
        if self.neighbor_friend_receiving_here(friendship) {
            Ok(())
        } else {
            Err(
                if self
                    .neighbor_friend_receiver(friendship)
                    .is_some_and(|status| status.receiver_installation_id.is_some())
                {
                    Error::ReceivingElsewhere
                } else {
                    Error::ReceiverNotChosen
                },
            )
        }
    }

    pub(in crate::service::neighbors) async fn reconcile_neighbor_concierge_receiver(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        receiving_here: bool,
        receiver_revision: u64,
        generation: u64,
    ) -> Result<(), String> {
        self.reconcile_neighbor_concierge_receiver_with(
            metadata,
            uid,
            friendship,
            receiving_here,
            receiver_revision,
            generation,
            || self.restore_neighbor_concierge(metadata, uid, friendship),
        )
        .await
    }

    async fn reconcile_neighbor_concierge_receiver_with<F, Fut>(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        receiving_here: bool,
        receiver_revision: u64,
        generation: u64,
        restore: F,
    ) -> Result<(), String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        self.require_neighbor_generation(generation)?;
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope.to_string());
        }
        if self.neighbor_friend_receiving_here(friendship) != receiving_here {
            return Err(
                "neighbor_receiver_changed: Effective receiving changed during reconciliation"
                    .into(),
            );
        }
        let binding = self
            .neighbor_friend_receiver_binding(friendship)
            .ok_or_else(|| Error::ReceiverNotChosen.to_string())?;
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let changed = settings
            .concierge_receivers
            .get(friendship)
            .is_some_and(|previous| previous.binding != binding);
        if !receiving_here || changed && settings.bindings.contains_key(friendship) {
            self.retire_neighbor_concierge(metadata, uid, friendship, generation)
                .await?;
        }
        self.require_neighbor_generation(generation)?;
        let mut settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let receiver = ConciergeReceiver {
            binding,
            revision: receiver_revision,
        };
        if settings.concierge_receivers.get(friendship) != Some(&receiver) {
            settings
                .concierge_receivers
                .insert(friendship.into(), receiver);
            metadata
                .save_settings(settings)
                .await
                .map_err(|error| error.to_string())?;
        }
        if receiving_here {
            restore().await?;
        }
        Ok(())
    }

    pub(in crate::service::neighbors) async fn retire_local_neighbor_concierges(
        &self,
        metadata: &Metadata,
        uid: &str,
        generation: u64,
    ) -> Result<(), String> {
        let (mut friends, mut first_error) = match metadata.settings().await {
            Ok(settings) => (
                settings.bindings.keys().cloned().collect::<BTreeSet<_>>(),
                None,
            ),
            Err(error) => (BTreeSet::new(), Some(error.to_string())),
        };
        for tab in self.mux.read().snapshot().tabs.values() {
            if let Some(authority) = tab
                .neighbor_authority
                .as_ref()
                .filter(|authority| authority.uid == uid)
            {
                friends.insert(authority.friendship_id.clone());
            }
        }
        for friend in friends {
            if let Err(error) = self
                .retire_neighbor_concierge(metadata, uid, &friend, generation)
                .await
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(super) async fn retire_neighbor_concierge(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        generation: u64,
    ) -> Result<(), String> {
        self.require_neighbor_generation(generation)?;
        self.stop_neighbor_concierge_listener(friendship);
        let _mutation = self.begin_neighbor_mutation(friendship).await?;
        let (mut settings, mut first_error) = match metadata.settings().await {
            Ok(settings) => (Some(settings), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let root = settings
            .as_ref()
            .and_then(|settings| settings.bindings.get(friendship))
            .cloned();
        let snapshot = self.mux.read().snapshot();
        let mut targets: Vec<_> = snapshot
            .tabs
            .iter()
            .filter_map(|(id, tab)| {
                (tab.neighbor_authority.as_ref().is_some_and(|authority| {
                    authority.uid == uid && authority.friendship_id == friendship
                }) || root.as_ref() == Some(id) && tab.neighbor_authority.is_none())
                .then(|| id.clone())
            })
            .collect();
        if root.is_none() && targets.is_empty() {
            self.clear_neighbor_friend_restore_grants(uid, friendship);
            return first_error.map_or(Ok(()), Err);
        }
        if let (Some(id), Some(settings)) = (&root, settings.as_mut()) {
            let tab = snapshot.tabs.get(id).filter(|tab| {
                tab.neighbor_authority
                    .as_ref()
                    .is_none_or(|authority| authority.uid == uid)
            });
            let options = self.neighbor_launch_options(settings, friendship);
            settings.dormant_concierges.insert(
                friendship.into(),
                DormantConcierge {
                    provider: tab.and_then(|tab| tab.agent_type.clone()),
                    model: tab.and_then(|tab| {
                        tab.agent_session_model
                            .clone()
                            .or(tab.agent_provider_resolved_model.clone())
                            .or(tab.restorable_launch.requested_model.clone())
                    }),
                    options,
                },
            );
        }
        // Revoke credentials before persistence or teardown; keep deny-only provenance until close.
        for id in &targets {
            self.revoke_tab_credential(id);
        }
        {
            let mut mux = self.mux.write();
            for id in &targets {
                if let Some(tab) = mux.get_tab_mut(id) {
                    tab.frozen = true;
                    tab.agent_booting = false;
                    tab.session_binding_nonce = None;
                    tab.reported_session_binding_nonce = None;
                }
            }
        }
        self.clear_neighbor_friend_restore_grants(uid, friendship);
        if let Some(mut settings) = settings {
            settings.bindings.remove(friendship);
            settings.initial_contexts.remove(friendship);
            if let Err(error) = metadata.save_settings(settings).await {
                first_error.get_or_insert_with(|| error.to_string());
            }
        }
        if first_error.is_none() {
            if let Err(error) = self
                .reconcile_neighbor_handling_restore(metadata, friendship, generation)
                .await
            {
                first_error = Some(error.to_string());
            }
        }
        targets.sort_by_key(|id| {
            root.as_ref() == Some(id)
                || snapshot
                    .tabs
                    .get(id)
                    .and_then(|tab| tab.neighbor_authority.as_ref())
                    .is_some_and(|authority| authority.root_tab_id.is_none())
        });
        for id in targets {
            self.require_neighbor_generation(generation)?;
            if self.mux.read().get_tab(&id).is_some() {
                if let Err(error) = self.close_tab_async(&id).await {
                    first_error.get_or_insert(error);
                }
            }
        }
        self.ui.emit(
            "neighbor-agent-changed",
            json!({"friendship_id":friendship,"gateway_tab":null,"concierge_dormant":true}),
        );
        self.emit_state_changed(crate::service::MuxEmitCause::TabLifecycle);
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
