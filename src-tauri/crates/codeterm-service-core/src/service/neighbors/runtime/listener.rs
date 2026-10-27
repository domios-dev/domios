use super::*;
use async_trait::async_trait;
use codeterm_neighbors::{
    supervisor::{supervise, InboxSession, ListenerState, ReceptionEvents},
    wire::{Cursor, InboxMessage},
};

pub(super) struct Handle {
    pub task: tokio::task::JoinHandle<()>,
    pub retry: Arc<tokio::sync::Notify>,
}

struct Session {
    service: ServiceContext,
    transport: Arc<FirebaseTransport>,
    scope: KeyScope,
    metadata: Metadata,
    friendship: String,
    tab: String,
    generation: u64,
    retry: Arc<tokio::sync::Notify>,
}

impl Session {
    fn current(&self) -> codeterm_neighbors::Result<()> {
        if self.service.neighbor_cell().listener_generation.load(Ordering::Acquire) != self.generation {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    fn keys(&self) -> InstallationKeys<'_> {
        InstallationKeys {
            store: &OsKeyStore,
            registry: self.transport.as_ref(),
            scope: self.scope.clone(),
            uid: &self.transport.uid,
        }
    }
}

#[async_trait]
impl InboxSession for Session {
    async fn prepare(&self) -> codeterm_neighbors::Result<()> {
        self.current()?;
        self.keys().load().await?.ok_or(Error::Revoked)?;
        self.current()
    }
    async fn cursor(&self) -> codeterm_neighbors::Result<Option<Cursor>> {
        self.metadata.load(&self.friendship).await
    }
    async fn receive(&self, messages: &mut [InboxMessage]) -> codeterm_neighbors::Result<()> {
        self.current()?;
        if self
            .service
            .neighbor_registry(super::super::NeighborCommand::List)
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
            tab: self.tab.clone(),
            generation: self.generation,
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
    fn state(&self, status: ListenerState) {
        if self.current().is_err() { return; }
        self.service
            .neighbor_cell()
            .states
            .lock()
            .insert(self.friendship.clone(), status);
        self.service.ui.emit(
            "neighbor-runtime-status",
            json!({"friendship_id":self.friendship,"status":status}),
        );
    }
    async fn retry_after_failure(&self, _: usize) -> codeterm_neighbors::Result<()> {
        self.retry.notified().await;
        self.current()
    }
}

pub(super) async fn run(
    service: ServiceContext,
    transport: Arc<FirebaseTransport>,
    scope: KeyScope,
    metadata: Metadata,
    friendship: String,
    tab: String,
    generation: u64,
    retry: Arc<tokio::sync::Notify>,
) {
    let session = Session {
        service,
        transport,
        scope,
        metadata,
        friendship,
        tab,
        generation,
        retry,
    };
    let _ = supervise(
        session.transport.as_ref(),
        &session.friendship,
        &session,
        &session,
    )
    .await;
}
