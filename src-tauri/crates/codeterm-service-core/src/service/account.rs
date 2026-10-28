pub mod deep_links;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use codeterm_account::{Account, Browser, Config, Error};
use std::{sync::Arc, time::Duration};

pub struct AccountCell(pub Arc<tokio::sync::OnceCell<Arc<Account>>>);

mod credentials;
pub(crate) mod logout;
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
    async fn changed(&self, identity: Option<&codeterm_account::Identity>) {
        if let Some(service) = self.0.upgrade() {
            service.neighbor_identity_changed();
            if identity.is_some() {
                service.restore_neighbor_updates_once();
            }
            service.ui.emit("account-changed", serde_json::Value::Null);
        }
    }
}

#[async_trait]
impl codeterm_account::IdentityObserver for AnalyticsIdentity {
    async fn changed(&self, identity: Option<&codeterm_account::Identity>) {
        if self
            .0
            .account_changed(identity.map(|value| value.uid.clone()))
            .await
            .is_err()
        {
            log::debug!("[analytics] identity unavailable");
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
                    observers.insert(0, Arc::new(AnalyticsIdentity(worker.clone())));
                }
                // Mesh is optional: its unavailability never prevents the account from starting.
                if let Ok(mesh) = self.account_mesh().await {
                    observers.insert(observers.len().saturating_sub(1), mesh.mesh.clone());
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
                    .with_browser_return_to_app(
                        !crate::paths::is_dev_instance()
                            && std::env::var("CODETERM_DEV").ok().as_deref() != Some("1")
                            && std::env::var("CODETERM_INSTANCE")
                                .ok()
                                .is_none_or(|value| value.trim().is_empty())
                            && std::env::var("CODETERM_API_PORT")
                                .ok()
                                .is_none_or(|value| value == "7685"),
                    )
                    .with_identity_observer(Arc::new(
                        codeterm_account::account_mesh::IdentityObservers(observers),
                    ))
                    .with_session_admission(Arc::new(logout::SignInAdmission(self.weak_self()))),
                );
                let _ = account.refresh().await;
                if let Some(session) = account.session().await {
                    if self.sync_product_analytics(&session, None).await.is_err() {
                        log::debug!("[analytics] consent bridge unavailable");
                    }
                } else if let Ok(worker) = self.analytics_worker().await {
                    if let Ok(ticket) = worker.consent_ticket().await {
                        if ticket.uid.is_none() && ticket.withdrawals.is_empty() {
                            let _ = worker.approve_consent(ticket).await;
                        }
                    }
                }
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
        self.restore_neighbor_updates_once();
        #[cfg(not(any(test, feature = "test-support")))]
        self.restore_neighbors_once();
        Ok(account.clone())
    }
    pub async fn account_logout(&self) -> Result<codeterm_account::Status, String> {
        let service = self.clone();
        tokio::spawn(async move { service.account_logout_owned().await })
            .await
            .map_err(|_| Error::Cancelled.to_string())?
    }

    pub(crate) async fn account_logout_owned(&self) -> Result<codeterm_account::Status, String> {
        self.account_sign_out_owned()
            .await
            .map(|(status, _)| status)
    }

    pub(crate) async fn account_sign_out_owned(
        &self,
    ) -> Result<(codeterm_account::Status, logout::StopResult), String> {
        let account = self.account().await?;
        let cell = self
            .extensions
            .get_or_init::<logout::LogoutCell>(Default::default);
        let _guard = cell.0.lock().await;
        let session = account.session().await;
        let mesh = self.account_mesh().await.ok();
        self.neighbor_identity_changed();
        let block = async {
            match mesh.as_ref() {
                Some(mesh) => mesh.mesh.sign_out_local().await,
                None => self.sign_out_without_account_mesh().await,
            }
        };
        if let Err(error) = block.await {
            log::warn!("Account sign-out cleanup failed: stage=mesh_block error={error}");
        }
        if let Some(session) = session.as_ref().filter(|s| s.identity.provider != "local") {
            if let Err(error) = self.queue_server_stop(session).await {
                log::warn!(
                    "Account sign-out cleanup failed: stage=server_stop_queue error_kind={error:?}"
                );
            }
        }
        let status = account.logout().await;
        if let Some(session) = session.as_ref() {
            if let Err(error) = self.queue_neighbor_logout(session).await {
                log::warn!("Account sign-out cleanup failed: stage=neighbor_queue error={error}");
            }
        }
        if let Err(error) = self.clear_account_plan().await {
            log::warn!("Account sign-out cleanup failed: stage=plan_grant error={error}");
        }
        self.ui.emit("account-changed", serde_json::Value::Null);
        let result = self
            .revoke_signed_out_session(
                mesh,
                session.as_ref().filter(|s| s.identity.provider != "local"),
            )
            .await;
        let mut status = status.map_err(|e| e.to_string())?;
        status.server_stop_unconfirmed = result.is_err();
        Ok((status, result))
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
        if let Some(session) = self.account().await?.session().await {
            if self.sync_product_analytics(&session, None).await.is_err() {
                log::debug!("[analytics] consent bridge unavailable");
            }
        }
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
