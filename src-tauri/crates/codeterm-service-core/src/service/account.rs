pub mod deep_links;

use async_trait::async_trait;
use codeterm_account::{Account, Browser, Config, Error};
use std::{sync::Arc, time::Duration};

pub struct AccountCell(pub Arc<tokio::sync::OnceCell<Arc<Account>>>);

mod credentials;
pub use credentials::AccountCredentials;
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
                let credential_provider = provider.id().to_owned();
                let credentials = Arc::new(AccountCredentials::new().await?);
                let account = Arc::new(
                    Account::new(
                        provider,
                        credentials.clone(),
                        Duration::from_secs(config.login_timeout_secs),
                    )
                    .with_identity_observer(Arc::new(
                        codeterm_account::account_mesh::IdentityObservers(observers),
                    )),
                );
                let _ = account.refresh().await;
                credentials.start_migration(
                    self.secrets.clone(),
                    credential_provider,
                    account.clone(),
                );
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
