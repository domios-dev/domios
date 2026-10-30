use super::*;
use codeterm_neighbors::state::StateListener;

#[async_trait::async_trait]
pub(super) trait SessionPorts: Send + Sync {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>>;
    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>>;

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
