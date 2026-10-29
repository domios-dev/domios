use crate::grant_store::Floors;
use crate::*;
use codeterm_entitlements::{
    EntitlementAuthority, LimitGate, Refusal, RevisionFloor, TimeState, Verdict, VerifiedGrant,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
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

pub(crate) fn refresh_error_kind(error: &str) -> &'static str {
    match error {
        "Installation proof unavailable" => "installation_proof_unavailable",
        "Billing server unreachable" => "transport",
        "Invalid billing response" | "Invalid callable response" => "response_decode",
        "Invalid billing request" | "Invalid installation proof" => "request_encode",
        "Invalid billing endpoint" => "configuration",
        "Cloud billing is unavailable: no build-time endpoint configured" => "unconfigured",
        codeterm_account::session_revocation::SESSION_REVOKED => "session_revoked",
        "Billing request refused" => "backend_refusal",
        _ => "provider_failure",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RefreshEvent {
    SignIn,
    Focus,
    Explicit,
    CheckoutReturned,
    PlanChanged,
    /// The verdict reached `refresh_after` while the daemon ran.
    Deadline,
}
#[derive(Default)]
struct Cache {
    uid: Option<String>,
    generation: u64,
    snapshot: Snapshot,
    login_generation: Option<u64>,
    login_floor: u64,
    retired_login: Option<u64>,
    refresh_flight: Option<tokio::sync::watch::Receiver<Option<Snapshot>>>,
    refresh_queued: bool,
    refresh_followup: bool,
    refresh_attempted_at: Option<u64>,
    retry_after: u64,
    error_revision: u64,
    reads: [reads::PlanRead; 2],
    checkout_pending: bool,
    checkout_event_pending: bool,
    observed: bool,
    neighbors_free: bool,
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
    observed: Arc<AtomicU64>,
    rearm: Arc<tokio::sync::Notify>,
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
            observed: Arc::new(AtomicU64::new(0)),
            rearm: Arc::new(tokio::sync::Notify::new()),
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
    /// Wall time that never moves backwards past the latest instant this installation observed.
    fn now(&self) -> u64 {
        let wall = (self.clock)();
        self.observed.fetch_max(wall, Ordering::SeqCst).max(wall)
    }
    fn floor_key(&self, grants: &Grants, uid: &str) -> String {
        Floors::key(uid, &grants.installation_id)
    }
    /// Persists the observed time so a restart with the clock set back cannot revive a lease.
    /// Blocking IO.
    pub fn observe(&self) {
        let Some(grants) = &self.grants else {
            return;
        };
        let now = self.now();
        let mut floors = match grants.store.load_floors() {
            Ok(Some(floors)) => floors,
            Ok(None) => return,
            Err(error) => {
                log::debug!("Grant floor unreadable: {error}");
                return;
            }
        };
        if floors.observed_at >= now {
            return;
        }
        floors.observed_at = now;
        if let Err(error) = grants.store.save_floors(&floors) {
            log::debug!("Grant floor persistence failed: {error}");
        }
    }
    fn recompute(&self, cache: &Cache) -> Snapshot {
        let mut snapshot = cache.snapshot.clone();
        snapshot.grant = cache
            .verified
            .as_ref()
            .map(|grant| grant.verdict(self.now()));
        snapshot.access = cache.uid.as_ref().map(|_| {
            Access::from_effective(
                &snapshot
                    .grant
                    .as_ref()
                    .map_or_else(codeterm_entitlements::compiled_free, Verdict::effective),
            )
        });
        snapshot.entitlements.plan = snapshot
            .grant
            .as_ref()
            .filter(|verdict| verdict.state.retains_grant())
            .map_or(Plan::Free, |verdict| Plan::from(&verdict.label));
        snapshot.checking = self.checking(cache);
        snapshot
    }
    fn checking(&self, cache: &Cache) -> bool {
        cache
            .checking_until
            .is_some_and(|deadline| self.now() < deadline)
    }
    /// Startup: verify the stored grant offline before the first lock projection. Blocking IO.
    /// Returns the checking deadline when only the paid hint stands between the user and locks.
    pub fn prime(&self, session: &codeterm_account::Session) -> Option<u64> {
        let generation = {
            let mut cache = self.cache.lock();
            if !self.bind(&mut cache, Some(session)) {
                return None;
            }
            cache.generation
        };
        let grants = self.grants.as_ref()?;
        let current = self
            .load_grant(generation, session)
            .is_some_and(|grant| grant.verdict(self.now()).state.retains_grant());
        let now = self.now();
        if current || !grants.store.load_hint().unwrap_or(false) {
            return None;
        }
        let mut cache = self.cache.lock();
        if cache.generation != generation {
            return None;
        }
        let deadline = now + CHECKING_DEADLINE_SECS;
        cache.checking_until = Some(deadline);
        self.rearm.notify_one();
        Some(deadline)
    }
    pub fn with_observer(mut self, observer: ProductObserver) -> Self {
        self.observer = Some(observer);
        self
    }
    // Identity changes can remove the grant file; callers must keep this off the async runtime.
    fn bind(&self, cache: &mut Cache, session: Option<&codeterm_account::Session>) -> bool {
        if session.is_some_and(|session| {
            session.login_generation < cache.login_floor
                || cache.retired_login.is_some_and(|retired| session.login_generation <= retired)
        }) {
            return false;
        }
        if let Some(session) = session {
            cache.login_floor = cache.login_floor.max(session.login_generation);
        } else if let Some(generation) = cache.login_generation {
            cache.retired_login = Some(generation);
        }
        let uid = session.map(|s| s.identity.uid.clone());
        let login_generation = session.map(|session| session.login_generation);
        if cache.uid != uid || cache.login_generation != login_generation {
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
            cache.login_generation = login_generation;
            cache.refresh_flight = None;
            cache.refresh_queued = false;
            cache.refresh_followup = false;
            cache.refresh_attempted_at = None;
            cache.retry_after = 0;
            cache.error_revision = 0;
            cache.reads = Default::default();
            cache.checkout_pending = false;
            cache.checkout_event_pending = false;
            cache.observed = false;
            cache.neighbors_free = false;
            self.rearm.notify_one();
        }
        true
    }
    pub fn binding_generation(&self) -> u64 {
        self.cache.lock().generation
    }
    pub fn snapshot_if_current(
        &self,
        session: Option<&codeterm_account::Session>,
        expected_generation: u64,
    ) -> Snapshot {
        let mut cache = self.cache.lock();
        if cache.generation != expected_generation {
            return self.recompute(&cache);
        }
        if !self.bind(&mut cache, session) { return Snapshot::default(); }
        self.recompute(&cache)
    }
    pub fn snapshot(&self, session: Option<&codeterm_account::Session>) -> Snapshot {
        let mut cache = self.cache.lock();
        if !self.bind(&mut cache, session) {
            return Snapshot::default();
        }
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
        let now = self.now();
        let verdict = cache.verified.as_ref().map(|grant| grant.verdict(now));
        let valid_until = cache.verified.as_ref().map(|grant| {
            let payload = grant.payload();
            payload
                .period_end
                .unwrap_or(payload.expires_at)
                .min(payload.expires_at)
        });
        drop(cache);
        let access = verdict
            .as_ref()
            .map_or_else(codeterm_entitlements::compiled_free, Verdict::effective);
        LimitGate::new(access, enforcing).with_deadline(valid_until, self.clock.clone())
    }
    pub fn neighbors_locked(&self, uid: &str) -> bool {
        let cache = self.cache.lock();
        cache.uid.as_deref() == Some(uid)
            && cache.observed
            && cache.neighbors_free
            && !cache.snapshot.stale
            && !self.checking(&cache)
            && self
                .recompute(&cache)
                .access
                .is_some_and(|access| access.authority == Authority::Free)
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
    async fn fetch_refresh(
        &self,
        session: Option<&codeterm_account::Session>,
        generation: u64,
    ) -> Snapshot {
        let error_revision = {
            let cache = self.cache.lock();
            if cache.generation != generation { return Snapshot::default(); }
            cache.error_revision
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
                        self.now(),
                    )
                    .err()
                });
            if matches!(refusal, Some(Refusal::PolicyEpoch | Refusal::Revision)) {
                if let Some(raw) = result
                    .as_ref()
                    .ok()
                    .and_then(|reading| reading.grant.as_ref())
                {
                    let _ = crate::grant_store::authenticate_observed(
                        raw,
                        grants.authority.as_ref(),
                        &session.identity.uid,
                        &grants.installation_id,
                        cached.as_ref(),
                        self.now(),
                        |issued_at| {
                            self.observed.fetch_max(issued_at, Ordering::SeqCst);
                        },
                    );
                }
                let retry = self.provider.entitlements(session).await;
                if retry.is_ok() {
                    result = retry;
                }
            }
        }
        let worker = self.clone();
        let owned_session = session.cloned();
        match tokio::task::spawn_blocking(move || {
            worker.finish_refresh(owned_session.as_ref(), generation, error_revision, result)
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
            let floors = grants.store.load_floors();
            if let Ok(Some(floors)) = &floors {
                self.observed
                    .fetch_max(floors.observed_at, Ordering::SeqCst);
            }
            let key = self.floor_key(grants, &session.identity.uid);
            match grants.store.load() {
                Ok(Some(raw)) => match crate::grant_store::authenticate(
                    &raw,
                    grants.authority.as_ref(),
                    &session.identity.uid,
                    &grants.installation_id,
                    None,
                    self.now(),
                )
                .and_then(|verified| match &floors {
                    Ok(Some(floors)) => floors
                        .revisions
                        .get(&key)
                        .ok_or(Refusal::Revision)
                        .and_then(|floor| floor.admits(&verified))
                        .map(|()| verified),
                    Ok(None) | Err(_) => Err(Refusal::Revision),
                }) {
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
        error_revision: u64,
        result: Result<Reading>,
    ) -> Snapshot {
        let mut cache = self.cache.lock();
        let previous = cache.snapshot.entitlements.clone();
        let observed = cache.observed;
        let mut refreshed = false;
        if cache.generation == generation {
            if result.is_err() {
                cache.error_revision += 1;
                cache.retry_after = self.now().saturating_add(60);
            } else if cache.error_revision == error_revision {
                cache.retry_after = 0;
                cache.reads[1].recovered();
            }
            if let Ok(reading) = &result {
                cache.checking_until = None;
                if let (Some(grants), Some(session)) = (&self.grants, session) {
                    let key = self.floor_key(grants, &session.identity.uid);
                    let floors = grants.store.load_floors();
                    if let Ok(Some(floors)) = &floors {
                        self.observed
                            .fetch_max(floors.observed_at, Ordering::SeqCst);
                    }
                    if floors.is_err() {
                        cache.verified = None;
                    }
                    let accepted = reading.grant.as_ref().map(|raw| {
                        crate::grant_store::authenticate_observed(
                            raw,
                            grants.authority.as_ref(),
                            &session.identity.uid,
                            &grants.installation_id,
                            cache.verified.as_ref(),
                            self.now(),
                            |issued_at| {
                                self.observed.fetch_max(issued_at, Ordering::SeqCst);
                            },
                        )
                        .and_then(|verified| match &floors {
                            Ok(floors) => floors
                                .as_ref()
                                .and_then(|floors| floors.revisions.get(&key))
                                .map_or(Ok(()), |floor| floor.admits(&verified))
                                .map(|()| verified),
                            Err(_) => Err(Refusal::Revision),
                        })
                    });
                    match accepted {
                        Some(Ok(verified)) => {
                            if let Some(raw) = &reading.grant {
                                if let Err(error) = grants.store.save(raw) {
                                    log::debug!("Grant persistence failed: {error}");
                                }
                            }
                            let mut floors = floors.ok().flatten().unwrap_or_default();
                            let raised =
                                RevisionFloor::raised(floors.revisions.get(&key), &verified);
                            floors.revisions.insert(key, raised);
                            floors.observed_at = floors.observed_at.max(self.now());
                            if let Err(error) = grants.store.save_floors(&floors) {
                                log::debug!("Grant floor persistence failed: {error}");
                            }
                            cache.verified = Some(verified);
                        }
                        Some(Err(refusal)) => {
                            log::debug!("Reply grant refused; retaining cached grant: {refusal:?}");
                        }
                        None => log::debug!("Reply has no grant; retaining cached grant"),
                    }
                    self.observe();
                    let now = self.now();
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
                    refreshed = true;
                    cache.observed = true;
                    cache.neighbors_free =
                        entitlements.plan == Plan::Free && access.authority == Authority::Free;
                    cache.snapshot = Snapshot {
                        entitlements,
                        stale: false,
                        access: Some(access),
                        checking: false,
                        grant: None,
                    };
                }
                Err(error) => {
                    log::debug!(
                        "Entitlement refresh failed: kind={}",
                        refresh_error_kind(&error)
                    );
                    cache.snapshot.stale = true;
                }
            }
        }
        let snapshot = self.recompute(&cache);
        if refreshed && observed && previous != snapshot.entitlements {
            cache.reads[1].invalidate();
        }
        if refreshed {
            cache.snapshot.entitlements.plan = snapshot.entitlements.plan.clone();
        }
        drop(cache);
        self.rearm.notify_one();
        if refreshed
            && (!observed
                || previous.plan != snapshot.entitlements.plan
                || previous.status != snapshot.entitlements.status)
        {
            if let Some(observer) = &self.observer {
                observer(ProductEvent::SubscriptionChanged {
                    plan: snapshot.entitlements.plan.clone(),
                    status: snapshot.entitlements.status,
                });
            }
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
    pub fn browser_return_event(
        &self,
        session: &codeterm_account::Session,
        event: RefreshEvent,
    ) -> RefreshEvent {
        let mut cache = self.cache.lock();
        if event == RefreshEvent::Focus
            && cache.uid.as_ref() == Some(&session.identity.uid)
            && cache.login_generation == Some(session.login_generation)
            && cache.checkout_pending
        {
            cache.checkout_pending = false;
            RefreshEvent::CheckoutReturned
        } else { event }
    }

    pub fn checkout_browser_opened(&self, session: &codeterm_account::Session, plan: Plan) {
        let mut cache = self.cache.lock();
        if !self.bind(&mut cache, Some(session)) {
            return;
        }
        cache.checkout_pending = true;
        cache.checkout_event_pending = true;
        drop(cache);
        if let Some(observer) = &self.observer {
            observer(ProductEvent::CheckoutStarted { plan });
        }
    }
    pub fn browser_opened(&self, session: &codeterm_account::Session) {
        let mut cache = self.cache.lock();
        if !self.bind(&mut cache, Some(session)) {
            return;
        }
        cache.checkout_pending = true;
    }
}

mod deadlines;
mod refresh;
mod reads;
pub use deadlines::DeadlineTick;

#[cfg(test)]
mod tests;
