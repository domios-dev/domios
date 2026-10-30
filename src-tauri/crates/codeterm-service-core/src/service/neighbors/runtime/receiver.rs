use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;
pub(super) mod friends;
pub(super) mod selection;
mod startup;
mod stop;
mod watch;

impl ServiceContext {
    pub(super) fn receiver_view(status: &ReceiverStatus) -> Value {
        json!({"receiving_here":status.receiving_here(), "receiver_machine_label":status.receiver_machine_label,
            "receiving_enabled":status.receiving_enabled,"revision":status.receiver_revision,"is_me":status.is_me})
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
        let mut attempt: usize = 0;
        let listener = loop {
            let connection = tokio::select! {
                biased;
                _ = &mut cancelled => return Err(Error::Disabled.to_string()),
                result = transport.listen_control() => result,
            };
            match connection {
                Ok(listener) => break listener,
                Err(error) => {
                    self.neighbor_watch_failure("control", error);
                    if !codeterm_neighbors::supervisor::retryable(error) {
                        self.neighbor_watch_state(
                            "control",
                            codeterm_neighbors::supervisor::ListenerState::Stopped,
                            None,
                        );
                        return Err(error.to_string());
                    }
                    attempt = attempt.saturating_add(1);
                    tokio::select! {
                        biased;
                        _ = &mut cancelled => return Err(Error::Disabled.to_string()),
                        _ = codeterm_neighbors::supervisor::reconnect_backoff(attempt) => {},
                    }
                    self.require_neighbor_generation(generation)?;
                }
            }
        };
        self.require_neighbor_generation(generation)?;
        let watch = watch::ControlWatch {
            service: self.clone(),
            transport,
            generation,
            progress: AtomicU64::new(0),
            first: tokio::sync::Mutex::new(Some(listener)),
        };
        let task = tokio::spawn(async move {
            let _ = codeterm_neighbors::supervisor::supervise_documents(&watch).await;
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
