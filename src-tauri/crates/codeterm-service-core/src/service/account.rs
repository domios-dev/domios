pub mod deep_links;

use crate::secrets::{SaveRequest, SecretError, SecretStoreState, SecretString};
use async_trait::async_trait;
use codeterm_account::{Account, Browser, Config, CredentialStore, Error, Secret};
use std::{sync::Arc, time::Duration};

pub struct AccountCell(pub Arc<tokio::sync::OnceCell<Arc<Account>>>);

pub struct AccountCredentials {
    store: SecretStoreState,
    operation: Arc<tokio::sync::Mutex<()>>,
    fallback: Arc<codeterm_account::protected_credentials::ProtectedCredentials>,
}
impl AccountCredentials {
    pub fn new(store: SecretStoreState) -> Self {
        Self {
            store,
            fallback: Arc::new(
                codeterm_account::protected_credentials::ProtectedCredentials::new(
                    crate::paths::data_dir().to_string_lossy().into_owned(),
                ),
            ),
            operation: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}
fn credential_name(provider: &str) -> String {
    format!("domios-account-{provider}-refresh")
}
const MESH_INSTALLATION_KEY: &str = "domios-account-mesh-installation-key";
#[async_trait]
impl CredentialStore for AccountCredentials {
    fn session_only(&self, provider: &str) -> bool {
        self.fallback.session_only(&credential_name(provider))
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
impl AccountCredentials {
    async fn load_named(&self, name: String) -> codeterm_account::Result<Option<Secret>> {
        let store = self.store.read().await.clone();
        let operation = self.operation.clone().lock_owned().await;
        let fallback = self.fallback.clone();
        let runtime = tokio::runtime::Handle::current();
        let result = tokio::task::spawn_blocking(move || {
            let _operation = operation;
            if let Some(value) = fallback.load(&name)? {
                return Ok(Some(value));
            }
            if !store.allows_account_credentials() {
                return Ok(None);
            }
            match runtime.block_on(store.get(&name)) {
                Ok(value) => Ok(Some(Secret::new(value.value.expose_secret()))),
                Err(SecretError::NotFound) => Ok(None),
                Err(_) => Err(Error::Credentials),
            }
        })
        .await
        .map_err(|_| Error::Credentials)?;
        result
    }
    async fn save_named(&self, name: String, token: &Secret) -> codeterm_account::Result<()> {
        let store = self.store.read().await.clone();
        let token = token.clone();
        let operation = self.operation.clone().lock_owned().await;
        let fallback = self.fallback.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _operation = operation;
            if store.allows_account_credentials()
                && runtime
                    .block_on(store.save(SaveRequest {
                        name: name.clone(),
                        value: SecretString::from(token.expose().to_owned()),
                        notes: None,
                        folder: None,
                        organization: None,
                        collection: None,
                        overwrite: true,
                    }))
                    .is_ok()
            {
                fallback.delete(&name)?;
                return Ok(());
            }
            fallback.save(&name, &token)
        })
        .await
        .map_err(|_| Error::Credentials)??;
        Ok(())
    }
    async fn delete_named(&self, name: String) -> codeterm_account::Result<()> {
        let store = self.store.read().await.clone();
        let operation = self.operation.clone().lock_owned().await;
        let fallback = self.fallback.clone();
        let runtime = tokio::runtime::Handle::current();
        let result = tokio::task::spawn_blocking(move || {
            let _operation = operation;
            let fallback_present = fallback.load(&name)?.is_some();
            fallback.delete(&name)?;
            if store.allows_account_credentials() {
                match runtime.block_on(store.delete(&name)) {
                    Ok(()) | Err(SecretError::NotFound) => Ok(()),
                    Err(_) if fallback_present => Ok(()),
                    Err(_) => Err(Error::Credentials),
                }
            } else {
                Ok(())
            }
        })
        .await
        .map_err(|_| Error::Credentials)?;
        result
    }
}
struct AnalyticsIdentity(codeterm_analytics::Worker);
#[async_trait]
impl codeterm_account::IdentityObserver for AnalyticsIdentity {
    async fn changed(&self, identity: Option<&codeterm_account::Identity>) {
        if self
            .0
            .set_user_id(identity.map(|value| value.uid.clone()))
            .await
            .is_err()
        {
            log::debug!("[analytics] identity unavailable");
            return;
        }
        if let Some(auth_provider) = identity
            .and_then(|value| codeterm_analytics::AuthProvider::from_catalog(&value.provider))
        {
            let _ = self
                .0
                .record(codeterm_analytics::Event::SignedIn { auth_provider })
                .await;
        }
    }
}
struct SystemBrowser;
#[async_trait]
impl Browser for SystemBrowser {
    async fn open(&self, url: &url::Url) -> codeterm_account::Result<()> {
        let url = url.to_string();
        tokio::task::spawn_blocking(move || codeterm_util::open_url::open_url(url))
            .await
            .map_err(|_| Error::Browser)?
            .map_err(|_| Error::Browser)
    }
}
impl super::ServiceContext {
    pub async fn account(&self) -> Result<Arc<Account>, String> {
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get_or_try_init(|| async {
                let value = {
                    let config = self.ext::<crate::config::ConfigState>();
                    let value = config.lock().account.clone();
                    value
                };
                let config: Config = serde_json::from_value(value)
                    .map_err(|_| "Invalid account config".to_owned())?;
                if config.login_timeout_secs == 0 || config.login_timeout_secs > 600 {
                    return Err("Invalid account deadline".into());
                }
                let provider =
                    codeterm_account::adapters::configured(&config).map_err(|e| e.to_string())?;
                let mut observers: Vec<Arc<dyn codeterm_account::IdentityObserver>> = Vec::new();
                if let Ok(worker) = self.analytics_worker().await {
                    observers.push(Arc::new(AnalyticsIdentity(worker.clone())));
                }
                // Mesh is optional: its unavailability never prevents the account from starting.
                if let Ok(mesh) = self.account_mesh().await {
                    observers.push(mesh.mesh.clone());
                }
                let account = Arc::new(
                    Account::new(
                        provider,
                        Arc::new(AccountCredentials::new(self.secrets.clone())),
                        Duration::from_secs(config.login_timeout_secs),
                    )
                    .with_identity_observer(Arc::new(
                        codeterm_account::account_mesh::IdentityObservers(observers),
                    )),
                );
                let _ = account.refresh().await;
                Ok::<_, String>(account)
            })
            .await?;
        self.account_mesh_restored();
        Ok(account.clone())
    }
    /// Mesh blocks and attempts endpoint withdrawal while this account's credentials still exist.
    pub async fn account_logout(&self) -> Result<codeterm_account::Status, String> {
        let account = self.account().await?;
        let logout = account.logout();
        let status = match self.account_mesh().await {
            Ok(mesh) => {
                let (_, persisted, status) = mesh.mesh.sign_out(logout).await;
                persisted?;
                status
            }
            Err(_) => {
                let block = self.sign_out_without_account_mesh().await;
                let status = logout.await;
                block?;
                status
            }
        };
        status.map_err(|e| e.to_string())
    }
    pub async fn account_login(
        &self,
        hint: codeterm_account::LoginHint,
    ) -> Result<codeterm_account::Status, String> {
        let status = self
            .account()
            .await?
            .login_with_hint(&SystemBrowser, hint)
            .await
            .map_err(|e| e.to_string())?;
        let _ = self
            .refresh_billing(codeterm_billing::RefreshEvent::SignIn)
            .await;
        self.spawn_account_mesh_refresh(codeterm_account::account_mesh::EnrollTrigger::Automatic);
        Ok(status)
    }
}
