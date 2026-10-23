use super::ServiceContext;
use codeterm_billing::{adapters, Billing, RefreshEvent, Snapshot};
pub use codeterm_entitlements::{
    LimitGate, LimitKey, LimitRefusal, LimitSummary, LockedRefusal, FREE_LIMIT_REACHED,
    PLAN_LIMIT_LOCKED,
};
pub use locks::ENTITLEMENT_CHANGED_EVENT;

mod locks;
use std::{sync::Arc, time::Instant};

pub struct BillingCell(pub Arc<tokio::sync::OnceCell<Arc<Billing>>>);

impl ServiceContext {
    /// Capture before taking the mux lock. Before billing starts it is the compiled free baseline.
    pub fn limit_gate(&self) -> LimitGate {
        self.ext::<BillingCell>().0.get().map_or_else(
            || {
                LimitGate::new(
                    codeterm_entitlements::compiled_free(),
                    codeterm_entitlements::enforcing(),
                )
            },
            |billing| billing.limit_gate(),
        )
    }

    pub async fn installation_id(&self) -> Result<codeterm_account::InstallationId, String> {
        let root = self.account_data_root()?;
        tokio::task::spawn_blocking(move || {
            codeterm_account::Account::installation_id(
                &codeterm_account::FileInstallationStore::new(root),
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
                if provider != "cloud" {
                    return Err("Unknown or unavailable billing provider".into());
                }
                let installation_id = self.installation_id().await?;
                let adapter: Arc<dyn codeterm_billing::BillingProvider> =
                    adapters::cloud::Cloud::configured(
                        config.get("endpoint").and_then(|v| v.as_str()),
                        installation_id.clone(),
                    )?;
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
                let billing = Arc::new(billing);
                if let Some(session) = self.account().await?.session().await {
                    let primer = billing.clone();
                    if let Err(error) =
                        tokio::task::spawn_blocking(move || primer.prime(&session)).await
                    {
                        log::debug!("Grant prime task failed: {error}");
                    }
                }
                self.apply_limit_gate(billing.limit_gate());
                self.spawn_entitlement_deadlines(billing.clone());
                Ok::<_, String>(billing)
            })
            .await?;
        self.apply_limit_gate(billing.limit_gate());
        Ok(billing.clone())
    }
    /// Idle daemons still refresh at `refresh_after` and lock at lease or period end; CLI,
    /// socket and headless callers all read the mux gate this re-projects.
    fn spawn_entitlement_deadlines(&self, billing: Arc<Billing>) {
        let service = self.clone();
        crate::async_runtime::spawn(async move {
            billing
                .drive_deadlines(|tick| {
                    let service = service.clone();
                    async move {
                        if tick.refresh_due {
                            if let Err(error) =
                                service.refresh_billing(RefreshEvent::Deadline).await
                            {
                                log::debug!("Deadline refresh failed: {error}");
                            }
                        }
                        service.announce_limit_gate();
                    }
                })
                .await;
        });
    }
    pub async fn account_plan(&self) -> Result<Snapshot, String> {
        let result = self.account_plan_inner().await;
        if result.is_err() {
            self.apply_limit_gate(LimitGate::new(
                codeterm_entitlements::compiled_free(),
                codeterm_entitlements::enforcing(),
            ));
        } else {
            self.sync_limit_gate();
        }
        result
    }
    async fn account_plan_inner(&self) -> Result<Snapshot, String> {
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
        let result = self.refresh_billing_inner(event).await;
        if result.is_err() {
            self.apply_limit_gate(LimitGate::new(
                codeterm_entitlements::compiled_free(),
                codeterm_entitlements::enforcing(),
            ));
        } else {
            self.sync_limit_gate();
        }
        result
    }
    async fn refresh_billing_inner(&self, event: RefreshEvent) -> Result<Snapshot, String> {
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
