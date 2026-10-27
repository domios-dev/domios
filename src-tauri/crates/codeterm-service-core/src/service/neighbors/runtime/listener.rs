use super::*;
use async_trait::async_trait;
use codeterm_neighbors::{
    supervisor::{supervise, InboxSession, ListenerState, ReceptionEvents},
    wire::{Cursor, InboxMessage},
};

#[derive(Default)]
pub(super) struct Slots {
    pub active: HashMap<String, Handle>,
    pub retired: HashMap<String, tokio::task::JoinHandle<()>>,
}

pub(super) struct Handle {
    pub gateway_tab: Option<String>,
    pub task: tokio::task::JoinHandle<()>,
    pub retry: Arc<tokio::sync::Notify>,
    pub shutdown: tokio::sync::watch::Sender<bool>,
}

pub(super) struct Session {
    pub service: ServiceContext,
    pub transport: Arc<FirebaseTransport>,
    pub scope: KeyScope,
    pub registry: Arc<FirebaseRegistry>,
    pub metadata: Metadata,
    pub friendship: String,
    pub tab: Option<String>,
    pub generation: u64,
    pub receiver_revision: u64,
    pub retry: Arc<tokio::sync::Notify>,
    pub stopped: tokio::sync::watch::Receiver<bool>,
}

impl Session {
    pub(super) fn current(&self) -> codeterm_neighbors::Result<()> {
        self.service.require_neighbors_enabled()?;
        self.service
            .require_neighbor_receiver(Some(self.receiver_revision))?;
        if self
            .service
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire)
            != self.generation
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    fn keys(&self) -> InstallationKeys<'_> {
        InstallationKeys {
            store: &OsKeyStore,
            registry: self.registry.as_ref(),
            scope: self.scope.clone(),
            uid: &self.transport.uid,
        }
    }
}

#[async_trait]
impl InboxSession for Session {
    async fn stopped(&self) {
        let mut stopped = self.stopped.clone();
        if *stopped.borrow_and_update() {
            return;
        }
        let _ = stopped.changed().await;
    }
    async fn prepare(&self) -> codeterm_neighbors::Result<()> {
        self.current()?;
        let status = self
            .registry
            .receiver_status_for(&self.keys(), false)
            .await?;
        if !status.receiving_here() || status.receiver_revision != self.receiver_revision {
            return Err(Error::NotReceiver);
        }
        self.keys().load().await?.ok_or(Error::Revoked)?;
        self.current()
    }
    async fn cursor(&self) -> codeterm_neighbors::Result<Option<Cursor>> {
        self.metadata.load(&self.friendship).await
    }
    async fn receive(&self, messages: &mut [InboxMessage]) -> codeterm_neighbors::Result<()> {
        self.current()?;
        let tab = self.tab.as_ref().ok_or(Error::Gateway)?;
        let status = self
            .registry
            .receiver_status_for(&self.keys(), false)
            .await?;
        if !status.receiving_here() || status.receiver_revision != self.receiver_revision {
            self.service.stop_neighbor_listeners();
            return Err(Error::NotReceiver);
        }
        if self
            .service
            .neighbor_registry(super::super::NeighborCommand::List {})
            .await
            .map_err(|_| Error::Transport)?
            .get("access")
            .and_then(Value::as_str)
            != Some("paid")
        {
            return Err(Error::Locked);
        }
        self.current()?;
        let lifecycle = self.keys();
        let gateway = gateway::Gateway {
            service: self.service.clone(),
            metadata: self.metadata.clone(),
            friendship: self.friendship.clone(),
            tab: tab.clone(),
            generation: self.generation,
            stopped: self.stopped.clone(),
        };
        Receiver {
            transport: self.transport.as_ref(),
            cursors: &self.metadata,
            gateway: &gateway,
            private_keys: &lifecycle,
            recipient_uid: &self.transport.uid,
            friendship_id: &self.friendship,
        }
        .catch_up(messages, now())
        .await?;
        Ok(())
    }
}

#[async_trait]
impl ReceptionEvents for Session {
    fn failure(&self, error: Error) {
        if self.current().is_err() {
            return;
        }
        let detail = match error {
            Error::ListenProtocol { kind } => {
                json!({"kind":kind,"operation":"inbox.listen.protocol"})
            }
            Error::Firestore { code, operation } => {
                json!({"code":code,"operation":operation,"kind":"grpc"})
            }
            error => json!({"kind":format!("{error:?}")}),
        };
        log::debug!(
            "[neighbor-listen] friendship={} failure={detail}",
            self.friendship
        );
        self.service
            .neighbor_cell()
            .failures
            .lock()
            .insert(self.friendship.clone(), detail);
    }
    fn state(&self, status: ListenerState) {
        if self.current().is_err() {
            return;
        }
        if status == ListenerState::Receiving {
            self.service
                .neighbor_cell()
                .failures
                .lock()
                .remove(&self.friendship);
        }
        let error = self
            .service
            .neighbor_cell()
            .failures
            .lock()
            .get(&self.friendship)
            .cloned();
        log::debug!(
            "[neighbor-listen] friendship={} status={status:?} error={error:?}",
            self.friendship
        );
        self.service
            .neighbor_cell()
            .states
            .lock()
            .insert(self.friendship.clone(), status);
        self.service.ui.emit(
            "neighbor-runtime-status",
            json!({"friendship_id":self.friendship,"status":status,"error":error}),
        );
    }
    async fn retry_after_failure(&self, _: usize) -> codeterm_neighbors::Result<()> {
        self.retry.notified().await;
        self.current()
    }
}

pub(super) async fn run(session: Session, previous: Option<tokio::task::JoinHandle<()>>) {
    if let Some(previous) = previous {
        let _ = previous.await;
    }
    if session.current().is_err() {
        return;
    }
    if session.tab.is_none() {
        super::pending::run(&session).await;
        return;
    }
    let _ = supervise(
        session.transport.as_ref(),
        &session.friendship,
        &session,
        &session,
    )
    .await;
}
