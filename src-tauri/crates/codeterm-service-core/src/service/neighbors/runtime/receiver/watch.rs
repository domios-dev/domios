use super::*;
use codeterm_neighbors::{
    adapter::ControlListener,
    supervisor::{DocumentSession, ListenerState, ReceptionEvents},
};

pub(super) struct ControlWatch {
    pub service: ServiceContext,
    pub transport: Arc<FirebaseTransport>,
    pub generation: u64,
    pub progress: AtomicU64,
    pub first: tokio::sync::Mutex<Option<ControlListener>>,
}

impl ControlWatch {
    fn current(&self) -> codeterm_neighbors::Result<()> {
        self.service.require_neighbors_enabled()?;
        self.service
            .require_neighbor_generation(self.generation)
            .map_err(|_| Error::Revoked)
    }
}

#[async_trait::async_trait]
impl ReceptionEvents for ControlWatch {
    fn failure(&self, error: Error) {
        if self.current().is_err() {
            return;
        }
        self.service.neighbor_cell().receiver.lock().take();
        self.service.neighbor_watch_failure("control", error);
    }
    fn state(&self, state: ListenerState) {
        if self.current().is_ok() {
            self.service.neighbor_watch_state("control", state, None);
        }
    }
    async fn retry_after_failure(&self, attempt: usize) -> codeterm_neighbors::Result<()> {
        codeterm_neighbors::supervisor::reconnect_backoff(attempt).await;
        self.current()
    }
}

#[async_trait::async_trait]
impl DocumentSession for ControlWatch {
    fn progress(&self) -> u64 {
        self.progress.load(Ordering::Acquire)
    }
    async fn stopped(&self) {
        let cancelled = self.service.neighbor_cell().control_cancel.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();
        if self.current().is_err() {
            return;
        }
        cancelled.await;
    }
    async fn listen(&self) -> codeterm_neighbors::Result<()> {
        self.current()?;
        let first = self.first.lock().await.take();
        let mut listener = match first {
            Some(listener) => listener,
            None => self.transport.listen_control().await?,
        };
        let mut health = codeterm_neighbors::supervisor::StreamHealth::default();
        use codeterm_neighbors::supervisor::RetryClock;
        loop {
            listener.changed().await?.ok_or(Error::Transport)?;
            self.current()?;
            self.service.neighbor_cell().receiver.lock().take();
            let version = self.service.forget_neighbor_friend_receivers();
            self.service.ui.emit(
                "neighbor-runtime-status",
                json!({"receiver_unavailable":true,"friend_receivers_version":version}),
            );
            self.service.forget_neighbor_registry();
            self.service
                .neighbor_operation(owner::Operation::ReceiverChanged)
                .await
                .map_err(|_| Error::Transport)?;
            self.current()?;
            if health.observe(codeterm_neighbors::supervisor::RuntimeClock.now(), true) {
                self.progress.fetch_add(1, Ordering::AcqRel);
            }
            self.state(ListenerState::Receiving);
        }
    }
}
