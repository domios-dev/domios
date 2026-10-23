use super::ServiceContext;
use codeterm_billing::{adapters, Billing, Entitlements, RefreshEvent, Snapshot};
pub use codeterm_entitlements::{
    LimitGate, LimitKey, LimitRefusal, LimitSummary, FREE_LIMIT_REACHED,
};
use std::{sync::Arc, time::Instant};

pub struct BillingCell(pub Arc<tokio::sync::OnceCell<Arc<Billing>>>);

impl ServiceContext {
    /// Capture before taking the mux lock. Dormant unless this build pins Phase B enforcement;
    /// before billing starts it is the compiled free baseline, as Phase B grants without a verdict.
    pub fn limit_gate(&self) -> LimitGate {
        if !codeterm_entitlements::enforcing(codeterm_entitlements::ACTIVATION.as_ref()) {
            return LimitGate::dormant();
        }
        self.ext::<BillingCell>().0.get().map_or_else(
            || LimitGate::new(codeterm_entitlements::compiled_free(), true),
            |billing| billing.limit_gate(),
        )
    }

    pub async fn installation_id(&self) -> Result<codeterm_account::InstallationId, String> {
        tokio::task::spawn_blocking(|| {
            codeterm_account::Account::installation_id(
                &codeterm_account::FileInstallationStore::new(crate::paths::data_dir()),
            )
            .map_err(|error| format!("Installation identity unavailable: {error}"))
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn billing(&self) -> Result<Arc<Billing>, String> {
        let billing = self
            .ext::<crate::service::billing::BillingCell>()
            .0
            .get_or_try_init(|| async {
                let state = self.ext::<crate::config::ConfigState>().clone();
                let config = state.lock().billing.clone();
                let provider = config
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("cloud");
                let installation_id = self.installation_id().await?;
                let adapter: Arc<dyn codeterm_billing::BillingProvider> = match provider {
                    "cloud" => adapters::cloud::Cloud::configured(
                        config.get("endpoint").and_then(|v| v.as_str()),
                        installation_id.clone(),
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
                let mut billing = Billing::new(adapter).with_grants(
                    &installation_id,
                    Arc::new(codeterm_billing::FileGrantStore::new(
                        crate::paths::data_dir(),
                    )),
                )?;
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
            if let Some(billing) = self.ext::<crate::service::billing::BillingCell>().0.get() {
                snapshot_off_runtime(billing.clone(), None).await?;
            }
            return Ok(Snapshot::default());
        }
        snapshot_off_runtime(self.billing().await?, session).await
    }
    pub async fn refresh_billing(&self, event: RefreshEvent) -> Result<Snapshot, String> {
        let account = self.account().await?;
        let session = account.session().await;
        if session.is_none() {
            if let Some(billing) = self.ext::<crate::service::billing::BillingCell>().0.get() {
                snapshot_off_runtime(billing.clone(), None).await?;
            }
            return Ok(Snapshot::default());
        }
        let billing = self.billing().await?;
        billing
            .refresh(session.as_ref(), event, Instant::now())
            .await;
        self.unlock_account_mesh_after_plan_change();
        let current = account.session().await;
        snapshot_off_runtime(billing, current).await
    }
    /// `theme` applies to checkout only; the portal is Paddle's own page.
    pub async fn billing_open(
        &self,
        plan: Option<&str>,
        theme: Option<codeterm_billing::CheckoutTheme>,
    ) -> Result<(), String> {
        let session = self
            .account()
            .await?
            .session()
            .await
            .ok_or("Sign in before managing a subscription")?;
        let billing = self.billing().await?;
        let (url, checkout_key) = match plan {
            Some(key) => {
                let key = codeterm_billing::CheckoutKey::parse(key)?;
                let url = billing.checkout_url(&session, key).await?;
                let url = match theme {
                    Some(theme) => codeterm_billing::checkout_url_with_theme(url, theme),
                    None => url,
                };
                (url, Some(key))
            }
            None => (billing.portal_url(&session).await?, None),
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
        tokio::task::spawn_blocking(move || codeterm_util::open_url::open_url(url.to_string()))
            .await
            .map_err(|_| "System browser unavailable")?
            .map_err(|_| "System browser unavailable")?;
        match checkout_key {
            Some(key) => billing.checkout_browser_opened(&session, key.plan()),
            None => billing.browser_opened(&session),
        }
        Ok(())
    }
}

async fn snapshot_off_runtime(
    billing: Arc<Billing>,
    session: Option<codeterm_account::Session>,
) -> Result<Snapshot, String> {
    tokio::task::spawn_blocking(move || billing.snapshot(session.as_ref()))
        .await
        .map_err(|error| error.to_string())
}

/// Every live tab counts toward `agent.tabs`, whatever its role or purpose.
pub fn admit_new_tab(
    gate: &LimitGate,
    mux: &crate::multiplexer::state::MultiplexerState,
) -> Result<(), String> {
    gate.admit(LimitKey::AgentTabs, mux.all_tabs().count())
        .map_err(|refusal| refusal.to_string())
}

pub fn admit_new_team(gate: &LimitGate) -> impl FnOnce(usize) -> Result<(), String> + '_ {
    move |teams| {
        gate.admit(LimitKey::Teams, teams)
            .map_err(|refusal| refusal.to_string())
    }
}
