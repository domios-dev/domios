use super::*;
use codeterm_neighbors::state::StateListener;

#[async_trait::async_trait]
pub(super) trait SessionPorts: Send + Sync {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>>;
    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>>;

    async fn persist_notifications(&self, session: &Session, seen: &notifications::Seen) -> Result<()> {
        session.metadata.write("registry-notifications.json".into(), serde_json::to_value(seen).map_err(|_| Error::CursorStore)?).await
    }

    async fn activate(&self, session: &Session, list: &serde_json::Value) -> Result<()> {
        let version = list["version"].as_u64().ok_or(Error::InvalidEnvelope)?;
        session.require_activation(version).await?;
        if list["access"] != "paid" || session.service.neighbor_access_locked().await {
            session.service.pause_neighbor_projection_reception(&session.transport.uid, session.expected.login_generation, version).await?;
        } else {
            session.require_activation(version).await?;
            session.service.refresh_neighbor_registry_listeners().await.map_err(|_| Error::Transport)?;
            session.require_activation(version).await?;
        }
        Ok(())
    }

    async fn reconcile(
        &self,
        session: &Session,
        list: serde_json::Value,
        previous: Option<serde_json::Value>,
        seen: &mut notifications::Seen,
    ) -> Result<()> {
        session.reconcile(list, previous, seen).await
    }

    fn stop_receiving(&self, session: &Session) {
        session.service.pause_neighbor_reception();
    }
}

pub(super) struct LivePorts(pub Arc<FirebaseTransport>);

#[async_trait::async_trait]
impl SessionPorts for LivePorts {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        self.0.listen_state().await
    }
    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        self.0.listen_registry().await
    }
}
