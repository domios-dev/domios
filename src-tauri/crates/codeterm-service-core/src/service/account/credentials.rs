use crate::secrets::SecretStoreState;
use async_trait::async_trait;
use codeterm_account::{
    protected_credentials::ProtectedCredentials, Account, CredentialStore, Error, Secret,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

const MESH_INSTALLATION_KEY: &str = "domios-account-mesh-installation-key";
fn credential_name(provider: &str) -> String {
    format!("domios-account-{provider}-refresh")
}

pub struct AccountCredentials {
    local: Arc<ProtectedCredentials>,
    root: PathBuf,
}

pub(crate) struct AccountDataRoot(pub PathBuf);

#[derive(Default)]
pub(crate) struct AccountCredentialCell(tokio::sync::OnceCell<Arc<AccountCredentials>>);

impl crate::service::ServiceContext {
    pub(crate) fn account_data_root(&self) -> Result<PathBuf, String> {
        credential_root(self.try_ext::<AccountDataRoot>())
    }

    pub(crate) async fn account_credentials(&self) -> Result<Arc<AccountCredentials>, String> {
        let root = self.account_data_root()?;
        let cell = self
            .extensions
            .get_or_init::<AccountCredentialCell>(AccountCredentialCell::default);
        cell.0
            .get_or_try_init(|| async { AccountCredentials::new(root).await.map(Arc::new) })
            .await
            .cloned()
    }
}

fn credential_root(explicit: Option<&AccountDataRoot>) -> Result<PathBuf, String> {
    if let Some(root) = explicit {
        return Ok(root.0.clone());
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        Err("Account test fixture requires an isolated credential root".into())
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        Ok(crate::paths::data_dir_path())
    }
}

impl AccountCredentials {
    pub async fn new(root: PathBuf) -> Result<Self, String> {
        let local_root = root.clone();
        let local =
            tokio::task::spawn_blocking(move || Arc::new(ProtectedCredentials::new(local_root)))
                .await
                .map_err(|_| "Cannot initialize account credential file".to_owned())?;
        Ok(Self { local, root })
    }
    async fn blocking<T: Send + 'static>(
        &self,
        operation: impl FnOnce(Arc<ProtectedCredentials>) -> codeterm_account::Result<T>
            + Send
            + 'static,
    ) -> codeterm_account::Result<T> {
        let local = self.local.clone();
        tokio::task::spawn_blocking(move || operation(local))
            .await
            .map_err(|_| Error::Credentials)?
    }
    async fn load_named(&self, name: String) -> codeterm_account::Result<Option<Secret>> {
        self.blocking(move |local| local.load(&name)).await
    }
    async fn save_named(&self, name: String, token: &Secret) -> codeterm_account::Result<()> {
        let token = token.clone();
        self.blocking(move |local| local.save(&name, &token)).await
    }
    async fn delete_named(&self, name: String) -> codeterm_account::Result<()> {
        self.blocking(move |local| local.delete(&name)).await
    }
    pub fn start_migration(
        self: &Arc<Self>,
        store: SecretStoreState,
        provider: String,
        account: Arc<Account>,
    ) {
        let credentials = self.clone();
        let root = self.root.clone();
        tokio::spawn(async move {
            let legacy = LegacyStores {
                store,
                native: Arc::new(NativeLegacy(root.to_string_lossy().into_owned())),
            };
            let migration = async {
                let refresh_name = credential_name(&provider);
                let (migrated_refresh, _) = tokio::join!(
                    credentials.migrate_named(&refresh_name, &legacy),
                    credentials.migrate_named(MESH_INSTALLATION_KEY, &legacy)
                );
                migrated_refresh
            };
            if matches!(
                tokio::time::timeout(Duration::from_secs(15), migration).await,
                Ok(true)
            ) {
                let _ = account.refresh().await;
            }
        });
    }
    async fn migrate_named(&self, name: &str, source: &dyn LegacySource) -> bool {
        let owned = name.to_owned();
        if !self
            .blocking(move |local| local.claim_migration(&owned))
            .await
            .unwrap_or(false)
        {
            return false;
        }
        let read = source.load(name).await;
        let owned = name.to_owned();
        let persisted = self
            .blocking(move |local| local.complete_migration(&owned, read))
            .await
            .unwrap_or(false);
        if persisted {
            source.delete(name).await;
        }
        persisted
    }
}
#[async_trait]
impl CredentialStore for AccountCredentials {
    fn session_only(&self, provider: &str) -> bool {
        self.local.session_only(&credential_name(provider))
    }
    async fn load(&self, provider: &str) -> codeterm_account::Result<Option<Secret>> {
        self.load_named(credential_name(provider)).await
    }
    async fn save(&self, provider: &str, token: &Secret) -> codeterm_account::Result<()> {
        self.save_named(credential_name(provider), token).await
    }
    async fn delete(&self, provider: &str) -> codeterm_account::Result<()> {
        self.delete_named(credential_name(provider)).await
    }
}
#[async_trait]
impl codeterm_account::account_mesh::InstallationKeyStore for AccountCredentials {
    async fn load(&self) -> codeterm_account::Result<Option<Secret>> {
        self.load_named(MESH_INSTALLATION_KEY.into()).await
    }
    async fn save(&self, secret: &Secret) -> codeterm_account::Result<()> {
        self.save_named(MESH_INSTALLATION_KEY.into(), secret).await
    }
}

#[async_trait]
trait LegacySource: Send + Sync {
    async fn load(&self, name: &str) -> codeterm_account::Result<Option<Secret>>;
    async fn delete(&self, name: &str);
}
struct LegacyStores {
    store: SecretStoreState,
    native: Arc<dyn LegacySource>,
}
struct NativeLegacy(String);

fn combine_legacy_reads(
    native: codeterm_account::Result<Option<Secret>>,
    fallback: codeterm_account::Result<Option<Secret>>,
) -> codeterm_account::Result<Option<Secret>> {
    match (native, fallback) {
        (Ok(Some(token)), _) | (_, Ok(Some(token))) => Ok(Some(token)),
        (Ok(None), Ok(None)) => Ok(None),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}
#[async_trait]
impl LegacySource for NativeLegacy {
    async fn load(&self, name: &str) -> codeterm_account::Result<Option<Secret>> {
        codeterm_account::legacy_credentials::load(self.0.clone(), name.to_owned()).await
    }
    async fn delete(&self, name: &str) {
        let _ = codeterm_account::legacy_credentials::delete(self.0.clone(), name.to_owned()).await;
    }
}
#[async_trait]
impl LegacySource for LegacyStores {
    async fn load(&self, name: &str) -> codeterm_account::Result<Option<Secret>> {
        let native = self.native.load(name);
        let native = tokio::time::timeout(Duration::from_secs(2), native)
            .await
            .unwrap_or(Err(Error::Timeout));
        let native = match native {
            Ok(Some(secret)) => return Ok(Some(secret)),
            other => other,
        };
        let read = async {
            let store = self.store.read().await.clone();
            if !store.allows_account_credentials() {
                return Ok(None);
            }
            match store.get(name).await {
                Ok(value) => Ok(Some(Secret::new(value.value.expose_secret()))),
                Err(crate::secrets::SecretError::NotFound) => Ok(None),
                Err(_) => Err(Error::Credentials),
            }
        };
        let fallback = tokio::time::timeout(Duration::from_secs(5), read)
            .await
            .unwrap_or(Err(Error::Timeout));
        combine_legacy_reads(native, fallback)
    }
    async fn delete(&self, name: &str) {
        let native = self.native.delete(name);
        let _ = tokio::time::timeout(Duration::from_secs(2), native).await;
        let delete = async {
            let store = self.store.read().await.clone();
            if store.allows_account_credentials() {
                let _ = store.delete(name).await;
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(5), delete).await;
    }
}

#[cfg(test)]
mod tests;
