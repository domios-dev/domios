use crate::*;
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
}
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
}
pub struct Billing {
    provider: Arc<dyn BillingProvider>,
    cache: Mutex<Cache>,
}
impl Billing {
    pub fn new(provider: Arc<dyn BillingProvider>) -> Self {
        Self {
            provider,
            cache: Mutex::new(Cache::default()),
        }
    }
    fn bind(cache: &mut Cache, session: Option<&codeterm_account::Session>) {
        let uid = session.map(|s| s.identity.uid.clone());
        if cache.uid != uid {
            cache.uid = uid;
            cache.generation += 1;
            cache.snapshot = Snapshot::default();
            cache.last_focus = None;
            cache.checkout_pending = false;
        }
    }
    pub fn snapshot(&self, session: Option<&codeterm_account::Session>) -> Snapshot {
        let mut cache = self.cache.lock();
        Self::bind(&mut cache, session);
        cache.snapshot.clone()
    }
    pub fn entitlement_allows(
        &self,
        session: Option<&codeterm_account::Session>,
        capability: Capability,
    ) -> bool {
        entitlement_allows(&self.snapshot(session).entitlements, capability)
    }
    pub async fn refresh(
        &self,
        session: Option<&codeterm_account::Session>,
        event: RefreshEvent,
        now: Instant,
    ) -> Snapshot {
        let generation = {
            let mut cache = self.cache.lock();
            Self::bind(&mut cache, session);
            if session.is_none() {
                return cache.snapshot.clone();
            }
            if event == RefreshEvent::Focus
                && !cache.checkout_pending
                && cache.last_focus.is_some_and(|last| {
                    now.saturating_duration_since(last) < Duration::from_secs(60)
                })
            {
                return cache.snapshot.clone();
            }
            cache.last_focus = Some(now);
            cache.checkout_pending = false;
            cache.generation += 1;
            cache.generation
        };
        let result = match session {
            Some(session) => self.provider.entitlements(session).await,
            None => return Snapshot::default(),
        };
        let mut cache = self.cache.lock();
        if cache.generation == generation {
            match result {
                Ok(entitlements) => {
                    cache.snapshot = Snapshot {
                        entitlements,
                        stale: false,
                    }
                }
                Err(_) => cache.snapshot.stale = true,
            }
        }
        cache.snapshot.clone()
    }
    pub async fn checkout_url(
        &self,
        session: &codeterm_account::Session,
        plan: &str,
    ) -> Result<url::Url> {
        self.provider.checkout_url(session, plan).await
    }
    pub async fn portal_url(&self, session: &codeterm_account::Session) -> Result<url::Url> {
        self.provider.portal_url(session).await
    }
    pub fn browser_opened(&self, session: &codeterm_account::Session) {
        let mut cache = self.cache.lock();
        Self::bind(&mut cache, Some(session));
        cache.checkout_pending = true;
    }
}
