use super::*;
use async_trait::async_trait;
use codeterm_neighbors::{
    supervisor::{supervise, InboxSession, ListenerState, ReceptionEvents},
    wire::{Cursor, InboxMessage},
};

struct Session {
    service: ServiceContext,
    transport: Arc<FirebaseTransport>,
    scope: KeyScope,
    metadata: Metadata,
    friendship: String,
    tab: String,
}

impl Session {
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
        let _operation = self.service.neighbor_cell().operations.lock().await;
        self.keys().reconcile().await?.ok_or(Error::Revoked)?;
        Ok(())
    }
    async fn cursor(&self) -> codeterm_neighbors::Result<Option<Cursor>> {
        self.metadata.load(&self.friendship).await
    }
    async fn receive(&self, messages: &mut [InboxMessage]) -> codeterm_neighbors::Result<()> {
        let _operation = self.service.neighbor_cell().operations.lock().await;
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
        let lifecycle = self.keys();
        let gateway = gateway::Gateway {
            service: self.service.clone(),
            metadata: self.metadata.clone(),
            friendship: self.friendship.clone(),
            tab: self.tab.clone(),
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
    async fn retry_after_failure(&self, attempt: usize) -> codeterm_neighbors::Result<()> {
        let millis = match attempt {
            1 => 250,
            2 => 1000,
            _ => 4000,
        };
        // Owner-authorized failure backoff bounds reconnect pressure; it conveys no readiness.
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        Ok(())
    }
}

pub(super) async fn run(
    service: ServiceContext,
    transport: Arc<FirebaseTransport>,
    scope: KeyScope,
    metadata: Metadata,
    friendship: String,
    tab: String,
) {
    let session = Session {
        service,
        transport,
        scope,
        metadata,
        friendship,
        tab,
    };
    let _ = supervise(
        session.transport.as_ref(),
        &session.friendship,
        &session,
        &session,
    )
    .await;
}
