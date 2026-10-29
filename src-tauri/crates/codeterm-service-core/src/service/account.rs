pub mod deep_links;
pub mod startup;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use codeterm_account::{Account, Browser, Config, Error};
use std::{sync::Arc, time::Duration};

pub struct AccountCell(pub Arc<tokio::sync::OnceCell<Arc<Account>>>);

pub(crate) mod callable_admission;
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
struct AccountIdentity(std::sync::Weak<super::ServiceContext>, String);

#[async_trait]
impl codeterm_account::IdentityObserver for AccountIdentity {
    async fn session_refreshed(&self, session: &codeterm_account::Session) {
        if let Some(service) = self.0.upgrade() {
            service.account_mesh_credentials_refreshed(session);
        }
    }

    async fn changed(&self, identity: Option<&codeterm_account::Identity>) {
        if let Some(service) = self.0.upgrade() {
            if let Some(cached) = service.try_ext::<startup::StartupAccount>() {
                if identity.is_none_or(|identity| identity.uid != cached.identity.uid) {
                    cached.invalidate();
                }
            }
            if let Ok(root) = service.account_data_root() {
                let backend = self.1.clone();
                let persistent = service
                    .account_credentials()
                    .await
                    .is_ok_and(|credentials| {
                        !codeterm_account::CredentialStore::session_only(
                            credentials.as_ref(),
                            &backend,
                        )
                    });
                let identity = identity.filter(|_| persistent).cloned();
                if !persistent {
                    if let Some(cached) = service.try_ext::<startup::StartupAccount>() {
                        cached.invalidate();
                    }
                }
                match tokio::task::spawn_blocking(move || {
                    startup::remember(&root, &backend, identity)
                })
                .await
                {
                    Ok(Ok(())) => {}
                    _ => log::debug!("Local account identity snapshot unavailable"),
                }
            }
            service.retire_callable_waiters();
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
                    vec![Arc::new(AccountIdentity(
                        self.weak_self(),
                        provider.id().to_owned(),
                    ))];
                if let Ok(worker) = self.analytics_worker().await {
                    observers.push(Arc::new(AnalyticsIdentity(worker.clone())));
                }
                // Mesh is optional: its unavailability never prevents the account from starting.
                if let Ok(mesh) = self.account_mesh().await {
                    observers.insert(0, mesh.mesh.clone());
                }
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
                if account.refresh().await == Err(Error::Credentials) {
                    if let Some(cached) = self.try_ext::<startup::StartupAccount>() {
                        cached.invalidate();
                    }
                    if let Ok(root) = self.account_data_root() {
                        let backend = credential_provider.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            startup::remember(&root, &backend, None)
                        })
                        .await;
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
