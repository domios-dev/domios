use crate::*;
use codeterm_entitlements::{
    EntitlementAuthority, LimitGate, Refusal, TimeState, Verdict, VerifiedGrant,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub entitlements: Entitlements,
    pub stale: bool,
    pub access: Option<Access>,
    /// No verified answer yet for an account last seen paid: the UI may defer showing locks.
    pub checking: bool,
    #[serde(skip)]
    pub grant: Option<Verdict>,
}

/// Failure bound for the checking state when the first refresh never answers.
pub const CHECKING_DEADLINE_SECS: u64 = 15;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RefreshEvent {
    SignIn,
    Focus,
    CheckoutReturned,
}
#[derive(Default)]
struct Cache {
    uid: Option<String>,
    generation: u64,
    snapshot: Snapshot,
    last_focus: Option<Instant>,
    checkout_pending: bool,
    observed: bool,
    verified: Option<VerifiedGrant>,
    loaded: bool,
    checking_until: Option<u64>,
}
#[derive(Clone)]
pub struct Billing {
    provider: Arc<dyn BillingProvider>,
    cache: Arc<Mutex<Cache>>,
    observer: Option<ProductObserver>,
    grants: Option<Grants>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
}
#[derive(Clone)]
struct Grants {
    authority: Arc<dyn EntitlementAuthority>,
    installation_id: String,
    store: Arc<dyn GrantStore>,
}
impl Billing {
    pub fn new(provider: Arc<dyn BillingProvider>) -> Self {
        Self {
            provider,
            cache: Arc::new(Mutex::new(Cache::default())),
            observer: None,
            grants: None,
            clock: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }),
        }
    }
    pub fn with_grants(
        mut self,
        installation_id: &codeterm_account::InstallationId,
        store: Arc<dyn GrantStore>,
    ) -> Result<Self> {
        self.grants = Some(Grants {
            authority: codeterm_entitlements::pinned(),
            installation_id: installation_id.as_str().into(),
            store,
        });
        Ok(self)
    }
    #[cfg(test)]
    pub(crate) fn with_test_grants(
        mut self,
        activation: codeterm_entitlements::Activation,
        installation_id: &str,
        store: Arc<dyn GrantStore>,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        self.grants = Some(Grants {
            authority: Arc::new(codeterm_entitlements::PinnedAuthority::new(activation)),
            installation_id: installation_id.into(),
            store,
        });
        self.clock = clock;
        self
    }
    fn recompute(&self, cache: &Cache) -> Snapshot {
        let mut snapshot = cache.snapshot.clone();
        snapshot.grant = cache
            .verified
            .as_ref()
            .map(|grant| grant.verdict((self.clock)()));
        snapshot.access = cache.uid.as_ref().map(|_| {
            Access::from_effective(
                &snapshot
                    .grant
                    .as_ref()
                    .map_or_else(codeterm_entitlements::compiled_free, Verdict::effective),
            )
        });
        snapshot.checking = self.checking(cache);
        snapshot
    }
    fn checking(&self, cache: &Cache) -> bool {
        cache
            .checking_until
            .is_some_and(|deadline| (self.clock)() < deadline)
    }
    /// Startup: verify the stored grant offline before the first lock projection. Blocking IO.
    /// Returns the checking deadline when only the paid hint stands between the user and locks.
    pub fn prime(&self, session: &codeterm_account::Session) -> Option<u64> {
        let generation = {
            let mut cache = self.cache.lock();
            self.bind(&mut cache, Some(session));
            cache.generation
        };
        let grants = self.grants.as_ref()?;
        let now = (self.clock)();
        let current = self
            .load_grant(generation, session)
            .is_some_and(|grant| grant.verdict(now).state.retains_grant());
        if current || !grants.store.load_hint().unwrap_or(false) {
            return None;
        }
        let mut cache = self.cache.lock();
        if cache.generation != generation {
            return None;
        }
        let deadline = now + CHECKING_DEADLINE_SECS;
        cache.checking_until = Some(deadline);
        Some(deadline)
    }
    pub fn with_observer(mut self, observer: ProductObserver) -> Self {
        self.observer = Some(observer);
        self
    }
    // Identity changes can remove the grant file; callers must keep this off the async runtime.
    fn bind(&self, cache: &mut Cache, session: Option<&codeterm_account::Session>) {
        let uid = session.map(|s| s.identity.uid.clone());
        if cache.uid != uid {
            if cache.uid.is_some() {
                if let Some(grants) = &self.grants {
                    if let Err(error) = grants.store.clear() {
                        log::debug!("Grant removal failed: {error}");
                    }
                }
            }
            if cache.uid.is_some() {
                if let Some(grants) = &self.grants {
                    if let Err(error) = grants.store.save_hint(false) {
                        log::debug!("Plan hint reset failed: {error}");
                    }
                }
            }
            cache.verified = None;
            cache.loaded = false;
            cache.checking_until = None;
            cache.uid = uid;
            cache.generation += 1;
            cache.snapshot = Snapshot::default();
            cache.last_focus = None;
            cache.checkout_pending = false;
            cache.observed = false;
        }
    }
    pub fn snapshot(&self, session: Option<&codeterm_account::Session>) -> Snapshot {
        let mut cache = self.cache.lock();
        self.bind(&mut cache, session);
        self.recompute(&cache)
    }
    /// Limits bind through the verified verdict, or compiled free without one. Checking defers
    /// only how locks are shown; the unsigned hint never relaxes enforcement.
    pub fn limit_gate(&self) -> LimitGate {
        let cache = self.cache.lock();
        let enforcing = self
            .grants
            .as_ref()
            .map_or_else(codeterm_entitlements::enforcing, |grants| {
                grants.authority.enforcement().enabled
            });
        let verdict = cache
            .verified
            .as_ref()
            .map(|grant| grant.verdict((self.clock)()));
        drop(cache);
        let access = verdict
            .as_ref()
            .map_or_else(codeterm_entitlements::compiled_free, Verdict::effective);
        LimitGate::new(access, enforcing)
    }
    pub fn entitlement_allows(
        &self,
        session: Option<&codeterm_account::Session>,
        capability: Capability,
    ) -> bool {
        capability == Capability::Terminal
            || self
                .snapshot(session)
                .access
                .is_some_and(|access| access.authority != Authority::Free)
    }
    pub async fn refresh(
        &self,
        session: Option<&codeterm_account::Session>,
        event: RefreshEvent,
        now: Instant,
    ) -> Snapshot {
        let worker = self.clone();
        let owned_session = session.cloned();
        let generation = tokio::task::spawn_blocking(move || {
            worker.begin_refresh(owned_session.as_ref(), event, now)
        })
        .await;
        let generation = match generation {
            Ok(Ok(generation)) => generation,
            Ok(Err(snapshot)) => return snapshot,
            Err(error) => {
                log::debug!("Grant cache task failed: {error}");
                return self.snapshot(session);
            }
        };
        let mut result = match session {
            Some(session) => self.provider.entitlements(session).await,
            None => return Snapshot::default(),
        };
        if let (Some(grants), Some(session)) = (&self.grants, session) {
            let worker = self.clone();
            let owned_session = session.clone();
            let cached =
                tokio::task::spawn_blocking(move || worker.load_grant(generation, &owned_session))
                    .await
                    .unwrap_or(None);
            let refusal = result
                .as_ref()
                .ok()
                .and_then(|reading| reading.grant.as_ref())
                .and_then(|raw| {
                    crate::grant_store::authenticate(
                        raw,
                        grants.authority.as_ref(),
                        &session.identity.uid,
                        &grants.installation_id,
                        cached.as_ref(),
                        (self.clock)(),
                    )
                    .err()
                });
            if matches!(refusal, Some(Refusal::PolicyEpoch | Refusal::Revision)) {
                let retry = self.provider.entitlements(session).await;
                if retry.is_ok() {
                    result = retry;
                }
            }
        }
        let worker = self.clone();
        let owned_session = session.cloned();
        match tokio::task::spawn_blocking(move || {
            worker.finish_refresh(owned_session.as_ref(), generation, result)
        })
        .await
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::debug!("Grant cache task failed: {error}");
                self.snapshot(session)
            }
        }
    }
    fn begin_refresh(
        &self,
        session: Option<&codeterm_account::Session>,
        event: RefreshEvent,
        now: Instant,
    ) -> std::result::Result<u64, Snapshot> {
        let mut cache = self.cache.lock();
        self.bind(&mut cache, session);
        if session.is_none() {
            return Err(self.recompute(&cache));
        }
        if event == RefreshEvent::Focus
            && !cache
                .verified
                .as_ref()
                .is_some_and(|grant| grant.verdict((self.clock)()).state == TimeState::Stale)
            && !cache.checkout_pending
            && cache
                .last_focus
                .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(60))
        {
            return Err(self.recompute(&cache));
        }
        cache.last_focus = Some(now);
        cache.checkout_pending = false;
        cache.generation += 1;
        Ok(cache.generation)
    }
    fn load_grant(
        &self,
        generation: u64,
        session: &codeterm_account::Session,
    ) -> Option<VerifiedGrant> {
        let grants = self.grants.as_ref()?;

        let mut cache = self.cache.lock();
        if cache.generation != generation {
            return None;
        }
        if !cache.loaded {
            cache.loaded = true;
            match grants.store.load() {
                Ok(Some(raw)) => match crate::grant_store::authenticate(
                    &raw,
                    grants.authority.as_ref(),
                    &session.identity.uid,
                    &grants.installation_id,
                    None,
                    (self.clock)(),
                ) {
                    Ok(verified) => cache.verified = Some(verified),
                    Err(refusal) => {
                        log::debug!("Stored grant refused: {refusal:?}");
                        if let Err(error) = grants.store.clear() {
                            log::debug!("Grant removal failed: {error}");
                        }
                    }
                },
                Ok(None) => {}
                Err(error) => {
                    log::debug!("Stored grant unreadable: {error}");
                    if let Err(error) = grants.store.clear() {
                        log::debug!("Grant removal failed: {error}");
                    }
                }
            }
        }
        cache.verified.clone()
    }
    fn finish_refresh(
        &self,
        session: Option<&codeterm_account::Session>,
        generation: u64,
        result: Result<Reading>,
    ) -> Snapshot {
        let mut cache = self.cache.lock();
        let mut product_event = None;
        if cache.generation == generation {
            if let Ok(reading) = &result {
                cache.checking_until = None;
                if let (Some(grants), Some(session)) = (&self.grants, session) {
                    let accepted = reading.grant.as_ref().map(|raw| {
                        crate::grant_store::authenticate(
                            raw,
                            grants.authority.as_ref(),
                            &session.identity.uid,
                            &grants.installation_id,
                            cache.verified.as_ref(),
                            (self.clock)(),
                        )
                    });
                    match accepted {
                        Some(Ok(verified)) => {
                            if let Some(raw) = &reading.grant {
                                if let Err(error) = grants.store.save(raw) {
                                    log::debug!("Grant persistence failed: {error}");
                                }
                            }
                            cache.verified = Some(verified);
                        }
                        Some(Err(refusal)) => {
                            log::debug!("Reply grant refused; retaining cached grant: {refusal:?}");
                        }
                        None => log::debug!("Reply has no grant; retaining cached grant"),
                    }
                    let now = (self.clock)();
                    let paid = cache.verified.as_ref().is_some_and(|grant| {
                        grant.verdict(now).effective().authority
                            != codeterm_entitlements::Authority::Free
                    });
                    if let Err(error) = grants.store.save_hint(paid) {
                        log::debug!("Plan hint persistence failed: {error}");
                    }
                }
            }
            match result {
                Ok(Reading {
                    entitlements: None,
                    access,
                    ..
                }) => {
                    cache.snapshot.stale = true;
                    cache.snapshot.access = Some(access);
                }
                Ok(Reading {
                    entitlements: Some(entitlements),
                    access,
                    ..
                }) => {
                    let changed = !cache.observed
                        || cache.snapshot.entitlements.plan != entitlements.plan
                        || cache.snapshot.entitlements.status != entitlements.status;
                    cache.observed = true;
                    if changed {
                        product_event = Some(ProductEvent::SubscriptionChanged {
                            plan: entitlements.plan,
                            status: entitlements.status,
                        });
                    }
                    cache.snapshot = Snapshot {
                        entitlements,
                        stale: false,
                        access: Some(access),
                        checking: false,
                        grant: None,
                    };
                }
                Err(_) => cache.snapshot.stale = true,
            }
        }
        let snapshot = self.recompute(&cache);
        drop(cache);
        if let (Some(event), Some(observer)) = (product_event, &self.observer) {
            observer(event);
        }
        snapshot
    }
    pub async fn checkout_url(
        &self,
        session: &codeterm_account::Session,
        key: CheckoutKey,
    ) -> Result<url::Url> {
        self.provider.checkout_url(session, key.as_str()).await
    }
    pub async fn portal_url(&self, session: &codeterm_account::Session) -> Result<url::Url> {
        self.provider.portal_url(session).await
    }
    pub fn checkout_browser_opened(&self, session: &codeterm_account::Session, plan: Plan) {
        self.browser_opened(session);
        if let Some(observer) = &self.observer {
            observer(ProductEvent::CheckoutStarted { plan });
        }
    }
    pub fn browser_opened(&self, session: &codeterm_account::Session) {
        let mut cache = self.cache.lock();
        self.bind(&mut cache, Some(session));
        cache.checkout_pending = true;
    }
}
