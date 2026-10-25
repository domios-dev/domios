pub mod deep_links;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use codeterm_account::{Account, Browser, Config, Error};
use std::{sync::Arc, time::Duration};

pub struct AccountCell(pub Arc<tokio::sync::OnceCell<Arc<Account>>>);

mod credentials;
pub(crate) use credentials::AccountCredentialCell;
pub use credentials::AccountCredentials;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use credentials::AccountDataRoot;
struct AccountRevocations(Arc<tokio::sync::OnceCell<Arc<Account>>>);

#[async_trait]
impl codeterm_account::session_revocation::SessionRevocationObserver for AccountRevocations {
    async fn revoked(&self, session: &codeterm_account::Session) {
        if let Some(account) = self.0.get() {
            if let Err(error) = account.session_revoked(session).await {
                log::error!("Revoked account credential cleanup failed: {error}");
            }
        }
    }
}

struct AnalyticsIdentity(codeterm_analytics::Worker);
struct AccountIdentity(std::sync::Weak<super::ServiceContext>);

#[async_trait]
impl codeterm_account::IdentityObserver for AccountIdentity {
    async fn changed(&self, _identity: Option<&codeterm_account::Identity>) {
        if let Some(service) = self.0.upgrade() {
            service.ui.emit("account-changed", serde_json::Value::Null);
        }
    }
}

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
    pub(super) fn session_revocations(
        &self,
    ) -> Arc<dyn codeterm_account::session_revocation::SessionRevocationObserver> {
        Arc::new(AccountRevocations(self.ext::<AccountCell>().0.clone()))
    }

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
                    codeterm_account::adapters::configured(&config, self.installation_id().await?)
                        .map_err(|e| e.to_string())?;
                let mut observers: Vec<Arc<dyn codeterm_account::IdentityObserver>> =
                    vec![Arc::new(AccountIdentity(self.weak_self()))];
                if let Ok(worker) = self.analytics_worker().await {
                    observers.push(Arc::new(AnalyticsIdentity(worker.clone())));
                }
                // Mesh is optional: its unavailability never prevents the account from starting.
                if let Ok(mesh) = self.account_mesh().await {
                    observers.insert(0, mesh.mesh.clone());
                }
                #[cfg(not(any(test, feature = "test-support")))]
                let credential_provider = provider.id().to_owned();
                let credentials = self.account_credentials().await?;
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
                #[cfg(not(any(test, feature = "test-support")))]
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
        let service = self.clone();
        tokio::spawn(async move { service.finish_account_login(hint).await })
            .await
            .map_err(|_| Error::Cancelled.to_string())?
    }
    async fn finish_account_login(
        &self,
        hint: codeterm_account::LoginHint,
    ) -> Result<codeterm_account::Status, String> {
        let status = self
            .account()
            .await?
            .login_with_hint(Arc::new(SystemBrowser), hint)
            .await
            .map_err(|e| e.to_string());
        self.ui.emit("account-changed", serde_json::Value::Null);
        let status = status?;
        if let Ok(mesh) = self.account_mesh().await {
            mesh.mesh.enrollment_signed_in();
        }
        let _ = self
            .refresh_billing(codeterm_billing::RefreshEvent::SignIn)
            .await;
        self.spawn_account_mesh_refresh(codeterm_account::account_mesh::EnrollTrigger::Automatic);
        Ok(status)
    }
}
