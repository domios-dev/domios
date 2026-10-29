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

pub fn pending_limit_gate() -> LimitGate {
    LimitGate::new(codeterm_entitlements::compiled_free(), false)
}

impl ServiceContext {
    pub async fn billing_plan_operation(
        &self,
        operation: codeterm_billing::PlanOperation,
    ) -> Result<serde_json::Value, String> {
        let account = self.account().await?;
        let session = account
            .current_session()
            .await
            .map_err(|_| "Sign in to manage your plan")?
            .ok_or("Sign in to manage your plan")?;
        let result = self
            .billing()
            .await?
            .plan_operation(&session, operation)
            .await;
        self.sync_limit_gate();
        self.unlock_account_mesh_after_plan_change();
        if account
            .session()
            .await
            .as_ref()
            .map(|current| (&current.identity.uid, current.login_generation))
            != Some((&session.identity.uid, session.login_generation))
        {
            return Err("Account changed while managing your plan".into());
        }
        result
    }
    /// Capture before taking the mux lock.
    pub fn limit_gate(&self) -> LimitGate {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(gate) = self.try_ext::<super::test_fixture::FixtureLimitGate>() {
            return gate.0.clone();
        }
        self.ext::<BillingCell>().0.get().map_or_else(
            || {
                self.try_ext::<super::account::startup::StartupAccount>()
                    .filter(|account| account.is_valid())
                    .map_or_else(pending_limit_gate, |account| account.gate())
            },
            |billing| {
                if billing.binding_generation() == 0 {
                    if let Some(account) = self
                        .try_ext::<super::account::startup::StartupAccount>()
                        .filter(|account| account.is_valid())
                    {
                        return account.gate();
                    }
                }
                billing.limit_gate()
            },
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
                        self.account_mesh().await?.mesh.clone(),
                        self.session_revocations(),
                    )?;
                let mut billing = Billing::new(adapter).with_grants(
                    &installation_id,
                    Arc::new(codeterm_billing::FileGrantStore::new(
                        self.account_data_root()?,
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
                self.apply_limit_gate(self.limit_gate());
                self.spawn_entitlement_deadlines(billing.clone());
                Ok::<_, String>(billing)
            })
            .await?;
        self.apply_limit_gate(self.limit_gate());
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
        self.sync_limit_gate();
        result
    }
    pub(super) async fn clear_account_plan(&self) -> Result<(), String> {
        self.account_plan().await?;
        let root = self.account_data_root()?;
        tokio::task::spawn_blocking(move || {
            use codeterm_billing::GrantStore;
            let store = codeterm_billing::FileGrantStore::new(root);
            store.clear()?;
            store.save_hint(false)
        })
        .await
        .map_err(|error| error.to_string())??;
        Ok(())
    }
    async fn account_plan_inner(&self) -> Result<Snapshot, String> {
        let binding = self
            .ext::<BillingCell>()
            .0
            .get()
            .map(|billing| billing.binding_generation());
        let session = self.account().await?.session().await;
        if session.is_none() {
            if let Some(snapshot) = self
                .try_ext::<super::account::startup::StartupAccount>()
                .and_then(|account| account.snapshot())
            {
                return Ok(snapshot);
            }
            if let (Some(billing), Some(binding)) = (self.ext::<BillingCell>().0.get(), binding) {
                snapshot_off_runtime(billing.clone(), None, Some(binding)).await?;
            }
            return Ok(Snapshot::default());
        }
        snapshot_off_runtime(self.billing().await?, session, None).await
    }
    pub async fn refresh_billing(&self, event: RefreshEvent) -> Result<Snapshot, String> {
        let result = self.refresh_billing_inner(event).await;
        self.sync_limit_gate();
        result
    }
    async fn refresh_billing_inner(&self, event: RefreshEvent) -> Result<Snapshot, String> {
        let binding = self
            .ext::<BillingCell>()
            .0
            .get()
            .map(|billing| billing.binding_generation());
        let account = self.account().await?;
        let session = account.session().await;
        if session.is_none() {
            if let (Some(billing), Some(binding)) = (self.ext::<BillingCell>().0.get(), binding) {
                snapshot_off_runtime(billing.clone(), None, Some(binding)).await?;
            }
            return Ok(Snapshot::default());
        }
        let billing = self.billing().await?;
        let event = session.as_ref().map_or(event, |session| {
            billing.browser_return_event(session, event)
        });
        billing
            .refresh(session.as_ref(), event, Instant::now())
            .await;
        self.unlock_account_mesh_after_plan_change();
        let binding = billing.binding_generation();
        let current = account.session().await;
        snapshot_off_runtime(billing, current, Some(binding)).await
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
            .map(|s| (&s.identity.uid, s.login_generation))
            != Some((&session.identity.uid, session.login_generation))
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
        self.ui.emit(
            "billing-browser-opened",
            serde_json::json!({"uid": session.identity.uid}),
        );
        Ok(())
    }
}

async fn snapshot_off_runtime(
    billing: Arc<Billing>,
    session: Option<codeterm_account::Session>,
    expected_generation: Option<u64>,
) -> Result<Snapshot, String> {
    tokio::task::spawn_blocking(move || match expected_generation {
        Some(generation) => billing.snapshot_if_current(session.as_ref(), generation),
        None => billing.snapshot(session.as_ref()),
    })
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
