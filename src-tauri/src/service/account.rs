use crate::secrets::{SaveRequest, SecretError, SecretStoreState, SecretString};
use async_trait::async_trait;
use codeterm_account::{Account, Browser, Config, CredentialStore, Error, Secret};
use std::{sync::Arc, time::Duration};

pub struct AccountCredentials {
    store: SecretStoreState,
    operation: Arc<tokio::sync::Mutex<()>>,
}
impl AccountCredentials {
    pub fn new(store: SecretStoreState) -> Self {
        Self {
            store,
            operation: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}
fn credential_name(provider: &str) -> String {
    format!("domios-account-{provider}-refresh")
}
#[async_trait]
impl CredentialStore for AccountCredentials {
    async fn load(&self, provider: &str) -> codeterm_account::Result<Option<Secret>> {
        let store = self.store.read().await.clone();
        let name = credential_name(provider);
        let operation = self.operation.clone().lock_owned().await;
        let runtime = tokio::runtime::Handle::current();
        let result = tokio::task::spawn_blocking(move || {
            let _operation = operation;
            runtime.block_on(store.get(&name))
        })
        .await
        .map_err(|_| Error::Credentials)?;
        match result {
            Ok(value) => Ok(Some(Secret::new(value.value.expose_secret()))),
            Err(SecretError::NotFound) => Ok(None),
            Err(_) => Err(Error::Credentials),
        }
    }
    async fn save(&self, provider: &str, token: &Secret) -> codeterm_account::Result<()> {
        let store = self.store.read().await.clone();
        let name = credential_name(provider);
        let value = SecretString::from(token.expose().to_owned());
        let operation = self.operation.clone().lock_owned().await;
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _operation = operation;
            runtime.block_on(store.save(SaveRequest {
                name,
                value,
                notes: None,
                folder: None,
                organization: None,
                collection: None,
                overwrite: true,
            }))
        })
        .await
        .map_err(|_| Error::Credentials)?
        .map_err(|_| Error::Credentials)?;
        Ok(())
    }
    async fn delete(&self, provider: &str) -> codeterm_account::Result<()> {
        let store = self.store.read().await.clone();
        let name = credential_name(provider);
        let operation = self.operation.clone().lock_owned().await;
        let runtime = tokio::runtime::Handle::current();
        let result = tokio::task::spawn_blocking(move || {
            let _operation = operation;
            runtime.block_on(store.delete(&name))
        })
        .await
        .map_err(|_| Error::Credentials)?;
        match result {
            Ok(()) | Err(SecretError::NotFound) => Ok(()),
            Err(_) => Err(Error::Credentials),
        }
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
        tokio::task::spawn_blocking(move || crate::commands::system::open_url(url))
            .await
            .map_err(|_| Error::Browser)?
            .map_err(|_| Error::Browser)
    }
}
impl super::ServiceContext {
    pub async fn account(&self) -> Result<Arc<Account>, String> {
        let account = self
            .account
            .get_or_try_init(|| async {
                let value = {
                    use tauri::Manager;
                    let config = self.app_handle.state::<crate::config::ConfigState>();
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
                let analytics = self.analytics_worker().await.ok().cloned();
                let account = Account::new(
                    provider,
                    Arc::new(AccountCredentials::new(self.secrets.clone())),
                    Duration::from_secs(config.login_timeout_secs),
                );
                let account = Arc::new(match analytics {
                    Some(worker) => {
                        account.with_identity_observer(Arc::new(AnalyticsIdentity(worker)))
                    }
                    None => account,
                });
                let _ = account.refresh().await;
                Ok::<_, String>(account)
            })
            .await?;
        Ok(account.clone())
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
        Ok(status)
    }
}
pub fn register_deep_links(
    app: &tauri::AppHandle,
    service: super::ServiceState,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_deep_link::DeepLinkExt;
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    app.deep_link().register_all()?;
    let dispatch = move |urls: Vec<url::Url>| {
        let service = service.clone();
        tauri::async_runtime::spawn(async move {
            for url in urls {
                if url.scheme() == "domios"
                    && url.host_str() == Some("billing")
                    && url.path() == "/return"
                {
                    let _ = service
                        .refresh_billing(codeterm_billing::RefreshEvent::CheckoutReturned)
                        .await;
                    continue;
                }
                if let Ok(callback) = codeterm_account::Callback::parse(url.as_str()) {
                    if let Ok(account) = service.account().await {
                        let _ = account.complete(callback).await;
                    }
                }
            }
        });
    };
    if let Some(urls) = app.deep_link().get_current()? {
        dispatch(urls);
    }
    app.deep_link()
        .on_open_url(move |event| dispatch(event.urls()));
    Ok(())
}
