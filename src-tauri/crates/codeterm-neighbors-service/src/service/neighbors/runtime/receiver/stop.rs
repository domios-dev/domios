use super::*;

#[derive(Default)]
pub struct Pending(parking_lot::Mutex<Option<u64>>);

pub trait NeighborsExt {
    fn neighbor_stop_local_pending(&self, generation: u64) -> bool;
    fn queue_neighbor_stop_local(&self);
    fn wait_neighbor_stop_local(
        &self,
        generation: u64,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
    fn drain_neighbor_stop_local(
        &self,
        generation: u64,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
    fn own_neighbor_stop_local<F, R>(
        &self,
        cleanup: F,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send
    where
        F: FnOnce() -> R + Send + 'static,
        R: std::future::Future<Output = Result<(), String>> + Send + 'static;
}

impl NeighborsExt for ServiceContext {
    fn neighbor_stop_local_pending(&self, generation: u64) -> bool {
        *self.extensions.get_or_init(Pending::default).0.lock() == Some(generation)
    }
    fn queue_neighbor_stop_local(&self) {
        self.neighbor_reconciliation().stop_pending();
        let generation = self.neighbor_cell().generation.load(Ordering::Acquire);
        *self.extensions.get_or_init(Pending::default).0.lock() = Some(generation);
        if self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .is_none()
        {
            return;
        }
        let service = self.clone();
        crate::async_runtime::spawn(async move {
            if let Err(error) = service.wait_neighbor_stop_local(generation).await {
                log::debug!("[neighbors] local concierge retirement failed: {error}");
                service.ui.emit(
                    "neighbor-runtime-status",
                    json!(
                        codeterm_neighbors::responses::NeighborListenerEvent::Unavailable {
                            receiver_unavailable: true,
                            friendship_id: None,
                            friend_receivers_version: None,
                            reason: Some("concierge_retirement_failed".into()),
                            message: None
                        }
                    ),
                );
            }
        });
    }

    async fn wait_neighbor_stop_local(&self, generation: u64) -> Result<(), String> {
        self.neighbor_operation(owner::Operation::StopLocal {
            expected_generation: generation,
        })
        .await
        .map(|_| ())
    }

    async fn drain_neighbor_stop_local(&self, generation: u64) -> Result<(), String> {
        if !self.neighbor_stop_local_pending(generation) {
            return Ok(());
        }
        let Some(account) = self.ext::<crate::service::account::AccountCell>().0.get() else {
            return Ok(());
        };
        let Some(session) = account.session().await else {
            return Ok(());
        };
        let uid = session.identity.uid;
        let service = self.clone();
        self.own_neighbor_stop_local(move || async move {
            drain_neighbor_stop_local_owned(&service, generation, &uid).await
        })
        .await
    }

    async fn own_neighbor_stop_local<F, R>(&self, cleanup: F) -> Result<(), String>
    where
        F: FnOnce() -> R + Send + 'static,
        R: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        self.neighbor_reconciliation()
            .submit(
                crate::service::neighbors::reconciliation::Kind::StopLocal,
                move || async move {
                    cleanup().await.map_err(|error| {
                        log::debug!("[neighbors] owned OFF cleanup failed: {error}");
                        Error::Gateway
                    })
                },
            )
            .map_err(|error| error.to_string())?
            .wait()
            .await
            .map_err(|error| error.to_string())
    }
}

async fn drain_neighbor_stop_local_owned(
    service: &ServiceContext,
    generation: u64,
    uid: &str,
) -> Result<(), String> {
    service.require_neighbor_generation(generation)?;
    if !service.neighbor_stop_local_pending(generation) {
        return Ok(());
    }
    let metadata = Metadata::new(service.account_data_root()?, uid);
    service
        .retire_local_neighbor_concierges(&metadata, uid, generation)
        .await?;
    service.require_neighbor_generation(generation)?;
    let mut pending = service.extensions.get_or_init(Pending::default).0.lock();
    if *pending == Some(generation) {
        *pending = None;
    }
    Ok(())
}

#[cfg(test)]
pub mod tests;
