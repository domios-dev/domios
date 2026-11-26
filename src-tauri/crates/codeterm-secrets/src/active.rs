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

pub(crate) struct FallbackStore {
    registry: Arc<SecretRegistry>,
    configured: String,
    fallback: Arc<dyn SecretStore>,
    activated: parking_lot::Mutex<Option<Arc<dyn SecretStore>>>,
}

impl FallbackStore {
    pub(crate) fn new(
        registry: Arc<SecretRegistry>,
        configured: String,
        fallback: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            registry,
            configured,
            fallback,
            activated: parking_lot::Mutex::new(None),
        }
    }

    fn activated(&self) -> Option<Arc<dyn SecretStore>> {
        let mut slot = self.activated.lock();
        if let Some(store) = slot.as_ref() {
            return Some(store.clone());
        }
        let instance = self.registry.get(&self.configured)?;
        let store: Arc<dyn SecretStore> = Arc::new(RegisteredStore::new(
            self.registry.clone(),
            self.configured.clone(),
            instance.id(),
        ));
        *slot = Some(store.clone());
        log::info!(
            "[secrets] configured backend '{}' registered live — switched from fallback '{}'",
            self.configured,
            self.fallback.id()
        );
        Some(store)
    }

    fn delegate(&self) -> Arc<dyn SecretStore> {
        self.activated().unwrap_or_else(|| self.fallback.clone())
    }
}

#[async_trait]
impl SecretStore for FallbackStore {
    fn id(&self) -> &'static str {
        self.delegate().id()
    }

    fn fallback(&self) -> Option<crate::BackendFallback> {
        if self.activated().is_some() {
            None
        } else {
            Some(crate::BackendFallback {
                configured_backend: self.configured.clone(),
                active_backend: self.fallback.id().to_string(),
            })
        }
    }

    fn allows_account_credentials(&self) -> bool {
        self.delegate().allows_account_credentials()
    }

    fn capabilities(&self) -> StoreCapabilities {
        self.delegate().capabilities()
    }

    fn auto_unlock_armed(&self) -> bool {
        self.delegate().auto_unlock_armed()
    }

    async fn status(&self) -> StoreStatus {
        self.delegate().status().await
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.delegate().unlock(creds).await
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.delegate().lock().await
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.delegate().logout().await
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        self.delegate().save(req).await
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        self.delegate().get(name_or_id).await
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        self.delegate().list(filter).await
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.delegate().search(query).await
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        self.delegate().delete(name_or_id).await
    }

    async fn has(&self, name_or_id: &str) -> Result<bool, SecretError> {
        self.delegate().has(name_or_id).await
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        self.delegate().organizations().await
    }

    async fn collections(
        &self,
        organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        self.delegate().collections(organization_id).await
    }

    async fn sync(&self) -> Result<(), SecretError> {
        self.delegate().sync().await
    }

    async fn init(&self) -> Result<(), SecretError> {
        self.delegate().init().await
    }
}

#[cfg(test)]
mod tests;
