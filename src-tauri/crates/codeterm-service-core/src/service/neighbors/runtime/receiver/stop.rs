use super::*;

#[derive(Default)]
struct Pending(parking_lot::Mutex<Option<u64>>);

impl ServiceContext {
    pub(in crate::service::neighbors) fn neighbor_stop_local_pending(
        &self,
        generation: u64,
    ) -> bool {
        *self.extensions.get_or_init(Pending::default).0.lock() == Some(generation)
    }
    pub(in crate::service::neighbors) fn queue_neighbor_stop_local(&self) {
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
                    json!({"receiver_unavailable":true,"reason":"concierge_retirement_failed"}),
                );
            }
        });
    }

    pub(in crate::service::neighbors) async fn wait_neighbor_stop_local(
        &self,
        generation: u64,
    ) -> Result<(), String> {
        self.neighbor_operation(owner::Operation::StopLocal {
            expected_generation: generation,
        })
        .await
        .map(|_| ())
    }

    pub(in crate::service::neighbors::runtime) async fn drain_neighbor_stop_local(
        &self,
        generation: u64,
    ) -> Result<(), String> {
        self.require_neighbor_generation(generation)?;
        if *self.extensions.get_or_init(Pending::default).0.lock() != Some(generation) {
            return Ok(());
        }
        let Some(account) = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned()
        else {
            return Ok(());
        };
        let Some(session) = account.session().await else {
            return Ok(());
        };
        self.require_neighbor_generation(generation)?;
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        self.retire_local_neighbor_concierges(&metadata, &session.identity.uid, generation)
            .await?;
        self.require_neighbor_generation(generation)?;
        let mut pending = self.extensions.get_or_init(Pending::default).0.lock();
        if *pending == Some(generation) {
            *pending = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
