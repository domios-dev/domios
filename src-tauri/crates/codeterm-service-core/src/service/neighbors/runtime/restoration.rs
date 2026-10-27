use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;

type WatchStartup<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, String>> + Send + 'a>>;

fn needs_key_recovery(
    status: &ReceiverStatus,
    local_key: Option<&str>,
    cleanup_pending: bool,
) -> bool {
    cleanup_pending
        || status.legacy_active && status.receiver_revision == 0
        || status.receiving_here()
            && (local_key.is_none() || local_key != status.receiver_key_id.as_deref())
}

impl ServiceContext {
    #[cfg(any(test, not(feature = "test-support")))]
    pub(super) fn spawn_neighbor_restoration(
        &self,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        *self.neighbor_cell().restoration.lock() = Some(tokio::spawn(future));
    }

    pub(super) fn cancel_neighbor_restoration(&self) {
        if let Some(task) = self.neighbor_cell().restoration.lock().take() {
            task.abort();
        }
    }

    pub(super) fn restore_neighbor_watches(&self, generation: u64) -> WatchStartup<'_> {
        Box::pin(async move {
            self.require_neighbors_enabled()
                .map_err(|error| error.to_string())?;
            self.require_neighbor_generation(generation)?;
            let (registry, scope, metadata) = self.neighbor_key_ports().await?;
            let transport = self.neighbor_transport(&registry)?;
            self.watch_neighbor_receiver(transport.clone(), generation)
                .await?;
            let status = self
                .refresh_neighbor_receiver_readonly(&registry, generation)
                .await?;
            let settings = metadata
                .settings()
                .await
                .map_err(|error| error.to_string())?;
            let keys = if status.receiving_here() {
                (InstallationKeys {
                    store: &OsKeyStore,
                    registry: registry.as_ref(),
                    scope: scope.clone(),
                    uid: &transport.uid,
                })
                .load()
                .await
                .map_err(|error| error.to_string())?
            } else {
                None
            };
            let local_key = keys
                .as_ref()
                .map(|keys| keys.verifier_key_id(&transport.uid));
            self.require_neighbor_generation(generation)?;
            self.require_neighbors_enabled()
                .map_err(|error| error.to_string())?;
            if needs_key_recovery(&status, local_key.as_deref(), settings.cleanup_pending) {
                self.neighbor_cell().receiver.lock().take();
                self.stop_neighbor_listeners();
                self.ui.emit(
                    "neighbor-runtime-status",
                    json!({"receiver_unavailable":true,"reason":"receiver_key_recovery_required"}),
                );
                return Ok(false);
            }
            self.start_neighbor_listeners_mode(
                transport, registry, scope, metadata, generation, false,
            )
            .await?;
            Ok(true)
        })
    }

    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn restore_neighbors_once(&self) {
        if self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .is_none()
        {
            return;
        }
        if self.require_neighbors_enabled().is_err()
            || self
                .neighbor_cell()
                .bootstrapped
                .swap(true, Ordering::AcqRel)
        {
            return;
        }
        let service = self.clone();
        let generation = self.neighbor_history_generation();
        self.spawn_neighbor_restoration(async move {
            let result = service.restore_neighbor_watches(generation).await;
            if service.require_neighbor_generation(generation).is_err() || service.require_neighbors_enabled().is_err() { return; }
            match result {
                Ok(true) => {},
                Ok(false) => { let _ = service.neighbor_operation(owner::Operation::Restore).await; },
                Err(error) => {
                    service.neighbor_cell().bootstrapped.store(false, Ordering::Release);
                    log::debug!("[neighbors] restoration failed: {error}");
                    service.ui.emit("neighbor-runtime-status", json!({"receiver_unavailable":true,"reason":"neighbor_reception_start_failed","message":error}));
                },
            }
        });
    }
}

#[cfg(test)]
mod tests;
