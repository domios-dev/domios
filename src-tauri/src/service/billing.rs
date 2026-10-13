use super::ServiceContext;
use codeterm_billing::{adapters, Billing, Entitlements, RefreshEvent, Snapshot};
use std::{sync::Arc, time::Instant};
use tauri::Manager;

impl ServiceContext {
    pub async fn billing(&self) -> Result<Arc<Billing>, String> {
        let billing = self
            .billing
            .get_or_try_init(|| async {
                let state = self
                    .app_handle
                    .state::<crate::config::ConfigState>()
                    .inner()
                    .clone();
                let config = state.lock().billing.clone();
                let provider = config
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("cloud");
                let adapter: Arc<dyn codeterm_billing::BillingProvider> = match provider {
                    "cloud" => adapters::cloud::Cloud::configured(
                        config.get("endpoint").and_then(|v| v.as_str()),
                    )?,
                    "local" if cfg!(debug_assertions) => {
                        let origin = url::Url::parse(
                            config
                                .get("origin")
                                .and_then(|v| v.as_str())
                                .unwrap_or("http://127.0.0.1:15100/"),
                        )
                        .map_err(|_| "Invalid local billing origin")?;
                        if origin.scheme() != "http"
                            || origin.host_str() != Some("127.0.0.1")
                            || !origin.username().is_empty()
                            || origin.password().is_some()
                        {
                            return Err("Local billing requires loopback HTTP".into());
                        }
                        Arc::new(adapters::local::Local {
                            origin,
                            read: Box::new(move || {
                                let value = state.lock().billing.get("entitlements").cloned();
                                match value {
                                    Some(value) => serde_json::from_value(value)
                                        .map_err(|_| "Invalid local entitlements".into()),
                                    None => Ok(Entitlements::default()),
                                }
                            }),
                        })
                    }
                    "fake" if cfg!(debug_assertions) => Arc::new(adapters::fake::Fake::default()),
                    _ => return Err("Unknown or unavailable billing provider".into()),
                };
                let mut billing = Billing::new(adapter);
                if let Ok(worker) = self.analytics_worker().await {
                    billing =
                        billing.with_observer(super::analytics::billing_observer(worker.clone()));
                }
                Ok::<_, String>(Arc::new(billing))
            })
            .await?;
        Ok(billing.clone())
    }
    pub async fn account_plan(&self) -> Result<Snapshot, String> {
        let session = self.account().await?.session().await;
        if session.is_none() {
            if let Some(billing) = self.billing.get() {
                billing.snapshot(None);
            }
            return Ok(Snapshot::default());
        }
        Ok(self.billing().await?.snapshot(session.as_ref()))
    }
    pub async fn refresh_billing(&self, event: RefreshEvent) -> Result<Snapshot, String> {
        let account = self.account().await?;
        let session = account.session().await;
        if session.is_none() {
            if let Some(billing) = self.billing.get() {
                billing.snapshot(None);
            }
            return Ok(Snapshot::default());
        }
        let billing = self.billing().await?;
        billing
            .refresh(session.as_ref(), event, Instant::now())
            .await;
        let current = account.session().await;
        Ok(billing.snapshot(current.as_ref()))
    }
    pub async fn billing_open(&self, plan: Option<&str>) -> Result<(), String> {
        let session = self
            .account()
            .await?
            .session()
            .await
            .ok_or("Sign in before managing a subscription")?;
        let billing = self.billing().await?;
        let url = match plan {
            Some(plan) => billing.checkout_url(&session, plan).await?,
            None => billing.portal_url(&session).await?,
        };
        if self
            .account()
            .await?
            .session()
            .await
            .as_ref()
            .map(|s| &s.identity.uid)
            != Some(&session.identity.uid)
        {
            return Err("Account changed while opening billing".into());
        }
        tokio::task::spawn_blocking(move || crate::commands::system::open_url(url.to_string()))
            .await
            .map_err(|_| "System browser unavailable")?
            .map_err(|_| "System browser unavailable")?;
        match plan {
            Some(plan) => {
                billing.checkout_browser_opened(&session, codeterm_billing::Plan::parse(plan)?)
            }
            None => billing.browser_opened(&session),
        }
        Ok(())
    }
}
