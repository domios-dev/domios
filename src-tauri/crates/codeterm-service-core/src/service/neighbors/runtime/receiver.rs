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
        self.require_neighbor_generation(generation)?;
        if let Some(status) = self.neighbor_cell().receiver.lock().clone() {
            return Ok(status);
        }
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
        self.finish_neighbor_receiver_refresh(result, generation)
    }

    pub(super) async fn refresh_neighbor_receiver_readonly(
        &self,
        registry: &FirebaseRegistry,
        generation: u64,
    ) -> Result<ReceiverStatus, String> {
        self.require_neighbor_generation(generation)?;
        if let Some(status) = self.neighbor_cell().receiver.lock().clone() {
            return Ok(status);
        }
        let result = registry.receiver_status().await;
        self.finish_neighbor_receiver_refresh(result, generation)
    }

    fn finish_neighbor_receiver_refresh(
        &self,
        result: codeterm_neighbors::Result<ReceiverStatus>,
        generation: u64,
    ) -> Result<ReceiverStatus, String> {
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
        let epoch = self
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire);
        let cancelled = self.neighbor_cell().control_cancel.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        if epoch
            != self
                .neighbor_cell()
                .listener_generation
                .load(Ordering::Acquire)
        {
            return Err("Neighbor listener changed".into());
        }
        let _start = tokio::select! {
            biased;
            _ = &mut cancelled => return Err(Error::Disabled.to_string()),
            guard = self.neighbor_cell().control_start.lock() => guard,
        };
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        if epoch
            != self
                .neighbor_cell()
                .listener_generation
                .load(Ordering::Acquire)
        {
            return Err("Neighbor listener changed".into());
        }
        if self
            .neighbor_cell()
            .control
            .lock()
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Ok(());
        }
        let connection = tokio::select! {
            biased;
            _ = &mut cancelled => return Err(Error::Disabled.to_string()),
            result = transport.listen_control() => result,
        };
        let mut listener = match connection {
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
                match service.restore_neighbor_watches(generation).await {
                    Ok(true) => {}
                    Ok(false) => {
                        let _ = service
                            .neighbor_operation(owner::Operation::ReceiverChanged)
                            .await;
                    }
                    Err(error) => {
                        log::debug!("[neighbors] receiver watch restoration failed: {error}");
                        service.ui.emit("neighbor-runtime-status", json!({"receiver_unavailable":true,"reason":"neighbor_reception_start_failed","message":error}));
                    }
                }
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
