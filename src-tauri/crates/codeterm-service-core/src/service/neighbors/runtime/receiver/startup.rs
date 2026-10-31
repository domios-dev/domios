use super::*;
use friends::FriendReceiver;
use std::collections::BTreeMap;

impl ServiceContext {
    pub(in crate::service::neighbors) async fn start_neighbor_listeners(
        &self,
        transport: Arc<FirebaseTransport>,
        registry: Arc<FirebaseRegistry>,
        scope: KeyScope,
        metadata: Metadata,
        generation: u64,
    ) -> Result<(), String> {
        self.start_neighbor_listeners_mode(transport, registry, scope, metadata, generation, true)
            .await
    }

    pub(in crate::service::neighbors) async fn reconcile_neighbor_friend_receiver_rows(
        &self,
        metadata: &Metadata,
        registry: &FirebaseRegistry,
        list: &Value,
        local_key: Option<&str>,
        generation: u64,
    ) -> Result<(), String> {
        let mut receivers = BTreeMap::new();
        for friend in list["friends"]
            .as_array()
            .ok_or("Invalid Neighbors response")?
        {
            let id = friend["friendship_id"]
                .as_str()
                .filter(|id| hex_id(id, 64))
                .ok_or("Invalid Neighbor friendship")?;
            let receiver = match friend.get("my_receiver") {
                Some(value) => FriendReceiver::from_projection(
                    serde_json::from_value(value.clone())
                        .map_err(|_| Error::InvalidEnvelope.to_string())?,
                    &registry.installation_id,
                )?,
                None => return Err(Error::InvalidEnvelope.to_string()),
            };
            receivers.insert(id.to_owned(), receiver);
        }
        if receivers.values().any(|receiver| {
            receiver.status.receiving_here()
                && (local_key.is_none()
                    || receiver.status.receiver_encryption_key_id.as_deref() != local_key)
        }) {
            return Err(Error::LocalKeyUnavailable.to_string());
        }
        let changes = self.remember_neighbor_friend_receivers(receivers, generation)?;
        self.publish_neighbor_friend_receiver_changes(metadata, &registry.uid, changes, generation)
            .await
    }

    pub(in crate::service::neighbors::runtime) async fn publish_neighbor_friend_receiver_changes(
        &self,
        metadata: &Metadata,
        uid: &str,
        changes: Vec<friends::Change>,
        generation: u64,
    ) -> Result<(), String> {
        for change in changes {
            self.require_neighbor_generation(generation)?;
            self.stop_neighbor_concierge_listener(&change.friendship_id);
            let status = change.current.as_ref().map(|receiver| &receiver.status);
            let here = status.is_some_and(ReceiverStatus::receiving_here);
            let revision = status
                .map(|status| status.receiver_revision)
                .or_else(|| {
                    change
                        .previous
                        .as_ref()
                        .map(|receiver| receiver.status.receiver_revision)
                })
                .unwrap_or(0);
            let mut view = status
                .map(Self::receiver_view)
                .unwrap_or_else(|| json!({"receiver_unavailable":true}));
            view["friendship_id"] = json!(change.friendship_id);
            view["friend_receivers_version"] = json!(change.version);
            self.ui.emit("neighbor-runtime-status", view);
            if !here && status.is_some() {
                self.reconcile_neighbor_concierge_receiver(
                    metadata,
                    uid,
                    &change.friendship_id,
                    false,
                    revision,
                    generation,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub(in crate::service::neighbors) async fn start_neighbor_listeners_mode(
        &self,
        transport: Arc<FirebaseTransport>,
        registry: Arc<FirebaseRegistry>,
        scope: KeyScope,
        metadata: Metadata,
        generation: u64,
        recover_keys: bool,
    ) -> Result<(), String> {
        if self.neighbor_access_locked().await { return Err(Error::Locked.to_string()); }
        self.require_neighbors_enabled().map_err(|error| error.to_string())?;
        self.require_neighbor_generation(generation)?;
        if self.neighbor_stop_local_pending(generation) {
            self.drain_neighbor_stop_local(generation).await?;
        }
        let account = self.ext::<crate::service::account::AccountCell>().0.get()
            .ok_or("Neighbor account unavailable")?;
        let session = account.session().await.ok_or("Neighbor account signed out")?;
        if session.identity.uid != transport.uid { return Err(Error::Revoked.to_string()); }
        let (ticket, list) = self.neighbor_activation_ticket(&session.identity.uid, session.login_generation)
            .map_err(|error| error.to_string())?;
        let prepare_service = self.clone();
        let apply_service = self.clone();
        let prepare_transport = transport.clone();
        let prepare_registry = registry.clone();
        let prepare_scope = scope.clone();
        let receiving = super::super::super::updates::receives_friends(&list, &registry.installation_id);
        self.activate_prepared_neighbor(ticket, generation, async move {
            if !receiving { return Ok(None); }
            let store = prepare_service.neighbor_key_store().map_err(|error| error.to_string())?;
            let lifecycle = InstallationKeys {
                store: &store, registry: prepare_registry.as_ref(), scope: prepare_scope,
                uid: &prepare_transport.uid,
            };
            let local = if recover_keys { lifecycle.reconcile().await } else { lifecycle.load().await }
                .map_err(|error| error.to_string())?;
            prepare_service.require_neighbor_generation(generation)?;
            Ok(local.as_ref().map(|keys| keys.encryption_key_id()))
        }, move |local_id| async move {
            apply_service.apply_neighbor_friend_listeners(transport, registry, scope, metadata, generation, list, local_id).await
        }).await
    }

    async fn activate_prepared_neighbor<P, Prep, Apply, Applied>(
        &self,
        ticket: super::super::super::projection::ActivationTicket,
        generation: u64,
        preparation: Prep,
        apply: Apply,
    ) -> Result<(), String>
    where
        P: Send + 'static,
        Prep: std::future::Future<Output = Result<P, String>>,
        Apply: FnOnce(P) -> Applied + Send + 'static,
        Applied: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let prepared = preparation.await;
        let service = self.clone();
        self.neighbor_reconciliation().submit(super::super::super::reconciliation::Kind::Activation, move || async move {
            service.require_neighbor_generation(generation).map_err(|_| Error::Revoked)?;
            service.require_neighbor_activation_account(&ticket).await?;
            service.require_neighbor_activation_ticket(&ticket)?;
            let outcome = match prepared { Ok(value) => apply(value).await, Err(error) => Err(error) };
            if let Err(error) = outcome {
                if service.require_neighbor_generation(generation).is_ok()
                    && service.require_neighbor_activation_ticket(&ticket).is_ok()
                {
                    let version = service.forget_neighbor_friend_receivers();
                    service.stop_neighbor_listeners();
                    service.ui.emit("neighbor-runtime-status", json!({"receiver_unavailable":true,"friend_receivers_version":version}));
                }
                log::debug!("[neighbors] current activation failed: {error}");
                return Err(Error::Gateway);
            }
            Ok(())
        }).map_err(|error| error.to_string())?.wait().await.map_err(|error| error.to_string())
    }

    async fn apply_neighbor_friend_listeners(
        &self,
        transport: Arc<FirebaseTransport>,
        registry: Arc<FirebaseRegistry>,
        scope: KeyScope,
        metadata: Metadata,
        generation: u64,
        list: Value,
        local_id: Option<String>,
    ) -> Result<(), String> {
        if self.neighbor_access_locked().await {
            self.pause_neighbor_reception();
            return Err(Error::Locked.to_string());
        }
        self.require_neighbor_generation(generation)?;
        if !super::super::super::updates::receives_friends(&list, &registry.installation_id) {
            self.reconcile_neighbor_friend_receiver_rows(
                &metadata, &registry, &list, None, generation,
            )
            .await?;
            self.stop_neighbor_listeners();
            return Ok(());
        }
        self.remember_neighbor_names(&list);
        self.reconcile_neighbor_friend_receiver_rows(
            &metadata,
            &registry,
            &list,
            local_id.as_deref(),
            generation,
        )
        .await?;
        if list["access"] != "paid" {
            self.stop_neighbor_listeners();
            return Ok(());
        }
        let mut settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        self.remember_neighbor_concierge_accounts(&transport.uid, &settings);
        self.remember_neighbor_pause_settings(&transport.uid, &settings);
        self.persist_neighbor_pauses(&metadata, generation)
            .await
            .map_err(|error| error.to_string())?;
        for friend in list["friends"]
            .as_array()
            .ok_or("Invalid Neighbors response")?
        {
            let id = friend["friendship_id"]
                .as_str()
                .ok_or("Invalid Neighbor friendship")?;
            let Some(receiver) = self
                .neighbor_friend_receiver(id)
                .filter(ReceiverStatus::receiving_here)
            else {
                continue;
            };
            if local_id.as_deref() != receiver.receiver_encryption_key_id.as_deref()
                || local_id.is_none()
            {
                self.forget_neighbor_friend_receivers();
                self.stop_neighbor_listeners();
                return Err(Error::LocalKeyUnavailable.to_string());
            }
            self.reconcile_neighbor_concierge_receiver(
                &metadata,
                &registry.uid,
                id,
                true,
                receiver.receiver_revision,
                generation,
            )
            .await?;
            self.require_neighbor_generation(generation)?;
        }
        settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        for id in settings.bindings.keys() {
            self.settle_neighbor_pause(&metadata, id)
                .await
                .map_err(|error| error.to_string())?;
        }
        if !settings.enabled
            && self
                .neighbor_friend_receiver_views()
                .as_object()
                .is_some_and(|views| views.values().any(|view| view["receiving_here"] == true))
        {
            settings.enabled = true;
            metadata
                .save_settings(settings.clone())
                .await
                .map_err(|error| error.to_string())?;
        }
        let listener_generation = self
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire);
        for friend in list["friends"]
            .as_array()
            .ok_or("Invalid Neighbors response")?
        {
            let id = friend["friendship_id"]
                .as_str()
                .ok_or("Invalid Neighbor friendship")?;
            let Some(receiver) = self
                .neighbor_friend_receiver(id)
                .filter(ReceiverStatus::receiving_here)
            else {
                continue;
            };
            let tab = settings
                .bindings
                .get(id)
                .filter(|tab| self.validate_neighbor_gateway(tab).is_ok())
                .cloned();
            let mut listeners = self.neighbor_cell().listeners.lock();
            if listeners
                .active
                .get(id)
                .is_some_and(|handle| !handle.task.is_finished() && handle.gateway_tab == tab)
            {
                continue;
            }
            if let Some(previous) = listeners.active.remove(id) {
                let _ = previous.shutdown.send(true);
                listeners.retired.insert(id.to_owned(), previous.task);
            }
            let retry = Arc::new(tokio::sync::Notify::new());
            let (shutdown, stopped) = tokio::sync::watch::channel(false);
            let previous = listeners.retired.remove(id);
            let task = tokio::spawn(listener::run(
                listener::Session {
                    service: self.clone(),
                    transport: transport.clone(),
                    scope: scope.clone(),
                    registry: registry.clone(),
                    metadata: metadata.clone(),
                    friendship: id.to_owned(),
                    tab: tab.clone(),
                    generation: listener_generation,
                    receiver_revision: receiver.receiver_revision,
                    retry: retry.clone(),
                    stopped,
                },
                previous,
            ));
            listeners.active.insert(
                id.to_owned(),
                listener::Handle {
                    gateway_tab: tab,
                    task,
                    retry,
                    shutdown,
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
