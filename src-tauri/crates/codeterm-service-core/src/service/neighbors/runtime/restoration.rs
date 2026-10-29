use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;

#[cfg(not(any(test, feature = "test-support")))]
type WatchStartup<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, String>> + Send + 'a>>;

#[cfg(any(test, not(feature = "test-support")))]
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

#[derive(Default)]
struct RestorationRearm(Arc<super::super::recovery::Rearm>);

impl ServiceContext {
    pub(super) async fn advertise_neighbor_files(
        &self,
        registry: &FirebaseRegistry,
        status: &ReceiverStatus,
        keys: &codeterm_neighbors::crypto::PrivateKeys,
        generation: u64,
    ) -> codeterm_neighbors::Result<bool> {
        if !status.receiving_here() {
            return Ok(false);
        }
        let mut advertised = self.neighbor_cell().file_advertised_generation.lock().await;
        self.require_neighbor_history_generation(generation)?;
        if *advertised == Some(generation) {
            return Ok(false);
        }
        let changed = registry.advertise_file_support(status, keys).await?;
        self.require_neighbor_history_generation(generation)?;
        *advertised = Some(generation);
        if changed {
            self.neighbor_cell().receiver.lock().take();
        }
        Ok(changed)
    }

    #[cfg(any(test, not(feature = "test-support")))]
    pub(super) fn spawn_neighbor_restoration(
        &self,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        *self.neighbor_cell().restoration.lock() = Some(tokio::spawn(future));
    }

    pub(in crate::service::neighbors) fn rearm_neighbor_restoration(&self) {
        self.extensions
            .get_or_init(RestorationRearm::default)
            .0
            .signal();
        if self
            .neighbor_cell()
            .restoration
            .lock()
            .as_ref()
            .is_none_or(|task| task.is_finished())
        {
            self.neighbor_cell()
                .bootstrapped
                .store(false, Ordering::Release);
        }
        #[cfg(not(any(test, feature = "test-support")))]
        self.restore_neighbors_once();
    }

    #[cfg(any(test, not(feature = "test-support")))]
    fn start_neighbor_restoration_with<F, A>(
        &self,
        generation: u64,
        attempt: A,
        clock: Arc<dyn codeterm_neighbors::supervisor::RetryClock>,
    ) where
        F: std::future::Future<Output = codeterm_neighbors::Result<()>> + Send + 'static,
        A: FnMut() -> F + Send + 'static,
    {
        if self
            .neighbor_cell()
            .bootstrapped
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        let rearm = self
            .extensions
            .get_or_init(RestorationRearm::default)
            .0
            .clone();
        let service = self.clone();
        self.spawn_neighbor_restoration(async move {
            let _ = super::super::recovery::run(&rearm, clock.as_ref(), attempt, || {
                let service = service.clone();
                async move {
                    let cancelled = service.neighbor_cell().control_cancel.notified();
                    tokio::pin!(cancelled);
                    cancelled.as_mut().enable();
                    if service.require_neighbor_generation(generation).is_err()
                        || service.require_neighbors_enabled().is_err()
                    {
                        return;
                    }
                    cancelled.await;
                }
            })
            .await;
        });
    }

    pub(super) fn cancel_neighbor_restoration(&self) {
        if let Some(task) = self.neighbor_cell().restoration.lock().take() {
            task.abort();
        }
    }

    #[cfg(not(any(test, feature = "test-support")))]
    pub(super) fn restore_neighbor_watches(&self, generation: u64) -> WatchStartup<'_> {
        Box::pin(async move {
            if self.neighbor_access_locked().await {
                return Err(Error::Locked.to_string());
            }
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
            self.neighbor_operation(owner::Operation::ReceiverChanged)
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
        if self.require_neighbors_enabled().is_err() {
            return;
        }
        let service = self.clone();
        let generation = self.neighbor_history_generation();
        self.start_neighbor_restoration_with(generation, move || {
            let service = service.clone();
            async move {
                let result = async {
                    if !service.restore_neighbor_watches(generation).await? {
                        service.neighbor_operation(owner::Operation::Restore).await?;
                    }
                    Ok::<(), String>(())
                }.await;
                service.require_neighbor_generation(generation).map_err(|_| Error::Revoked)?;
                service.require_neighbors_enabled()?;
                result.map_err(|error| {
                    log::debug!("[neighbors] restoration failed: {error}");
                    service.ui.emit("neighbor-runtime-status", json!({"receiver_unavailable":true,"reason":"neighbor_reception_start_failed","message":error}));
                    super::super::recovery::startup_error(&error)
                })
            }
        }, Arc::new(codeterm_neighbors::supervisor::RuntimeClock));
    }
}

#[cfg(test)]
mod tests;
