use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    CollectionRef, ListFilter, OrganizationRef, SaveRequest, SecretError, SecretRef,
    SecretRegistry, SecretStore, SecretValue, StoreCapabilities, StoreStatus, UnlockCredentials,
};

pub(crate) struct RegisteredStore {
    registry: Arc<SecretRegistry>,
    key: String,
    id: &'static str,
}

impl RegisteredStore {
    pub(crate) fn new(registry: Arc<SecretRegistry>, key: String, id: &'static str) -> Self {
        Self { registry, key, id }
    }

    fn current(&self) -> Result<Arc<dyn SecretStore>, SecretError> {
        self.registry.get(&self.key).ok_or(SecretError::Backend {
            message: "selected secret backend is unavailable".into(),
        })
    }
}

#[async_trait]
impl SecretStore for RegisteredStore {
    fn id(&self) -> &'static str {
        self.id
    }

    fn allows_account_credentials(&self) -> bool {
        self.current()
            .is_ok_and(|store| store.allows_account_credentials())
    }

    fn capabilities(&self) -> StoreCapabilities {
        self.current()
            .map(|store| store.capabilities())
            .unwrap_or(StoreCapabilities::NONE)
    }

    fn auto_unlock_armed(&self) -> bool {
        self.current().is_ok_and(|store| store.auto_unlock_armed())
    }

    async fn status(&self) -> StoreStatus {
        match self.current() {
            Ok(store) => store.status().await,
            Err(error) => StoreStatus::Unavailable {
                reason: error.to_string(),
            },
        }
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.current()?.unlock(creds).await
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.current()?.lock().await
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.current()?.logout().await
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        self.current()?.save(req).await
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        self.current()?.get(name_or_id).await
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        self.current()?.list(filter).await
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.current()?.search(query).await
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        self.current()?.delete(name_or_id).await
    }

    async fn has(&self, name_or_id: &str) -> Result<bool, SecretError> {
        self.current()?.has(name_or_id).await
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        self.current()?.organizations().await
    }

    async fn collections(
        &self,
        organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        self.current()?.collections(organization_id).await
    }

    async fn sync(&self) -> Result<(), SecretError> {
        self.current()?.sync().await
    }

    async fn init(&self) -> Result<(), SecretError> {
        self.current()?.init().await
    }
}

#[cfg(test)]
mod tests;
