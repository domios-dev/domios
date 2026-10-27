use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;

impl ServiceContext {
    pub(super) fn require_neighbor_receiver(
        &self,
        revision: Option<u64>,
    ) -> codeterm_neighbors::Result<()> {
        if !self
            .neighbor_cell()
            .receiver
            .lock()
            .as_ref()
            .is_some_and(|status| {
                status.receiving_here()
                    && revision.is_none_or(|revision| revision == status.receiver_revision)
            })
        {
            return Err(Error::NotReceiver);
        }
        Ok(())
    }

    pub(super) fn receiver_view(status: &ReceiverStatus) -> Value {
        json!({"receiving_here":status.receiving_here(), "receiver_machine_label":status.receiver_machine_label,
            "receiving_enabled":status.receiving_enabled,"revision":status.receiver_revision})
    }

    pub(super) async fn refresh_neighbor_receiver(
        &self,
        registry: &FirebaseRegistry,
        scope: &KeyScope,
        generation: u64,
    ) -> Result<ReceiverStatus, String> {
        let result = registry
            .receiver_status_for(
                &InstallationKeys {
                    store: &OsKeyStore,
                    registry,
                    scope: scope.clone(),
                    uid: &registry.uid,
                },
                true,
            )
            .await;
        self.require_neighbor_generation(generation)?;
        let status = match result {
            Ok(status) => status,
            Err(error) => {
                self.neighbor_cell().receiver.lock().take();
                self.stop_neighbor_listeners();
                self.ui.emit(
                    "neighbor-runtime-status",
                    json!({"receiver_unavailable":true}),
                );
                return Err(error.to_string());
            }
        };
        let changed = self.neighbor_cell().receiver.lock().as_ref() != Some(&status);
        if changed {
            self.stop_neighbor_listeners();
            *self.neighbor_cell().receiver.lock() = Some(status.clone());
            self.ui
                .emit("neighbor-runtime-status", Self::receiver_view(&status));
        }
        Ok(status)
    }

    pub(super) async fn watch_neighbor_receiver(
        &self,
        transport: Arc<FirebaseTransport>,
        generation: u64,
    ) -> Result<(), String> {
        self.require_neighbors_enabled()
            .map_err(|e| e.to_string())?;
        if self
            .neighbor_cell()
            .control
            .lock()
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Ok(());
        }
        let mut listener = match transport.listen_control().await {
            Ok(listener) => listener,
            Err(error) => {
                self.neighbor_cell().receiver.lock().take();
                self.stop_neighbor_listeners();
                return Err(error.to_string());
            }
        };
        self.require_neighbor_generation(generation)?;
        let service = self.clone();
        let task = tokio::spawn(async move {
            while let Ok(Some(())) = listener.changed().await {
                if service.require_neighbor_generation(generation).is_err() {
                    return;
                }
                service.neighbor_cell().receiver.lock().take();
                service.stop_neighbor_listeners();
                let _ = service
                    .neighbor_operation(owner::Operation::ReceiverChanged)
                    .await;
            }
            if service.require_neighbor_generation(generation).is_ok() {
                service.neighbor_cell().receiver.lock().take();
                service.stop_neighbor_listeners();
                service.ui.emit(
                    "neighbor-runtime-status",
                    json!({"receiver_unavailable":true}),
                );
            }
        });
        let mut slot = self.neighbor_cell().control.lock();
        if self.require_neighbors_enabled().is_err() {
            task.abort();
            return Err(Error::Disabled.to_string());
        }
        *slot = Some(task);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
