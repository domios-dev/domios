use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use tokio::sync::watch;

use crate::{
    CollectionRef, ListFilter, OrganizationRef, SaveRequest, SecretError, SecretRef, SecretStore,
    SecretValue, StoreCapabilities, StoreStatus, UnlockCredentials,
};

type Initialization = Result<(), SecretError>;

struct InitializedStore {
    store: Arc<dyn SecretStore>,
    flight: OnceLock<watch::Receiver<Option<Initialization>>>,
}

pub(crate) fn wrap(store: Arc<dyn SecretStore>) -> Arc<dyn SecretStore> {
    Arc::new(InitializedStore {
        store,
        flight: OnceLock::new(),
    })
}

impl InitializedStore {
    async fn initialization(&self) -> Result<Initialization, SecretError> {
        let mut completed = self
            .flight
            .get_or_init(|| {
                let (sender, receiver) = watch::channel(None);
                let store = self.store.clone();
                tokio::spawn(async move {
                    let _ = sender.send_replace(Some(store.init().await));
                });
                receiver
            })
            .clone();
        loop {
            let result = completed.borrow_and_update().clone();
            if let Some(result) = result {
                return Ok(result);
            }
            completed
                .changed()
                .await
                .map_err(|_| SecretError::Backend {
                    message: "secret backend initialization was interrupted".into(),
                })?;
        }
    }

    async fn ready(&self) -> Result<(), SecretError> {
        // An init error is nonfatal: explicit unlock must still be able to recover the backend.
        self.initialization().await.map(|_| ())
    }
}

#[async_trait]
impl SecretStore for InitializedStore {
    fn id(&self) -> &'static str {
        self.store.id()
    }

    fn allows_account_credentials(&self) -> bool {
        self.store.allows_account_credentials()
    }

    fn capabilities(&self) -> StoreCapabilities {
        self.store.capabilities()
    }

    fn auto_unlock_armed(&self) -> bool {
        self.store.auto_unlock_armed()
    }

    async fn status(&self) -> StoreStatus {
        match self.ready().await {
            Ok(()) => self.store.status().await,
            Err(error) => StoreStatus::Unavailable {
                reason: error.to_string(),
            },
        }
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.ready().await?;
        self.store.unlock(creds).await
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.ready().await?;
        self.store.lock().await
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.ready().await?;
        self.store.logout().await
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        self.ready().await?;
        self.store.save(req).await
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        self.ready().await?;
        self.store.get(name_or_id).await
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        self.ready().await?;
        self.store.list(filter).await
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.ready().await?;
        self.store.search(query).await
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        self.ready().await?;
        self.store.delete(name_or_id).await
    }

    async fn has(&self, name_or_id: &str) -> Result<bool, SecretError> {
        self.ready().await?;
        self.store.has(name_or_id).await
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        self.ready().await?;
        self.store.organizations().await
    }

    async fn collections(
        &self,
        organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        self.ready().await?;
        self.store.collections(organization_id).await
    }

    async fn sync(&self) -> Result<(), SecretError> {
        self.ready().await?;
        self.store.sync().await
    }

    async fn init(&self) -> Result<(), SecretError> {
        self.initialization().await?
    }
}

#[cfg(test)]
mod tests;
