use crate::sign_in_diagnostic::SignInAttempt;
use crate::*;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{oneshot, Mutex, Notify};

fn unix_time() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn token_expired(expires_at: u64) -> bool {
    match unix_time() {
        Some(now) => expires_at <= now,
        None => true,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Status {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub server_stop_unconfirmed: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub session_revoked: bool,
    pub provider: String,
    pub identity: Option<Identity>,
    pub pending: bool,
    pub session_only: bool,
    pub last_sign_in_failure: Option<SignInFailure>,
}
#[derive(Default)]
struct State {
    session_revoked: bool,
    session: Option<Session>,
    pending: Option<(LoginFlow, oneshot::Sender<Redemption>)>,
    generation: u64,
    refresh_blocked: bool,
    refresh_revision: u64,
    last_refresh: Option<(u64, Result<()>)>,
    busy: bool,
    login_cancel: Option<oneshot::Sender<()>>,
    #[cfg(debug_assertions)]
    pending_url: Option<url::Url>,
    last_sign_in_failure: Option<SignInFailure>,
}
pub struct Account {
    provider: Arc<dyn IdentityProvider>,
    store: Arc<dyn CredentialStore>,
    state: Mutex<State>,
    operation: Mutex<()>,
    refresh_cancel: Notify,
    deadline: Duration,
    observer: Option<Arc<dyn IdentityObserver>>,
    admission: Option<Arc<dyn SessionAdmission>>,
    browser_return_to_app: bool,
}
impl Account {
    pub fn new(
        provider: Arc<dyn IdentityProvider>,
        store: Arc<dyn CredentialStore>,
        deadline: Duration,
    ) -> Self {
        Self {
            provider,
            store,
            deadline,
            observer: None,
            admission: None,
            browser_return_to_app: false,
            state: Mutex::new(State::default()),
            operation: Mutex::new(()),
            refresh_cancel: Notify::new(),
        }
    }
    pub fn with_identity_observer(mut self, observer: Arc<dyn IdentityObserver>) -> Self {
        self.observer = Some(observer);
        self
    }
    pub fn with_session_admission(mut self, admission: Arc<dyn SessionAdmission>) -> Self {
        self.admission = Some(admission);
        self
    }
    pub fn with_browser_return_to_app(mut self, enabled: bool) -> Self {
        self.browser_return_to_app = enabled;
        self
    }
    pub async fn status(&self) -> Status {
        let state = self.state.lock().await;
        Status {
            server_stop_unconfirmed: false,
            session_revoked: state.session_revoked,
            provider: self.provider.id().into(),
            identity: state
                .session
                .as_ref()
                .filter(|_| !state.refresh_blocked)
                .map(|s| s.identity.clone()),
            pending: state.busy,
            session_only: state.session.is_some() && self.store.session_only(self.provider.id()),
            last_sign_in_failure: state.last_sign_in_failure,
        }
    }
    #[cfg(debug_assertions)]
    pub async fn pending_login_url(&self) -> Option<url::Url> {
        let state = self.state.lock().await;
        state.pending.as_ref()?;
        state.pending_url.clone()
    }
    pub async fn complete(&self, callback: Callback) -> Result<()> {
        self.prepare_callback(callback).await?.send()
    }
    pub(crate) async fn prepare_callback(&self, callback: Callback) -> Result<Completion> {
        let mut state = self.state.lock().await;
        let (flow, _) = state.pending.as_mut().ok_or(Error::Consumed)?;
        let request = flow.complete(callback)?;
        let (_, sender) = state.pending.take().ok_or(Error::Consumed)?;
        #[cfg(debug_assertions)]
        {
            state.pending_url = None;
        }
        Ok(Completion { sender, request })
    }
    pub async fn login(self: &Arc<Self>, browser: Arc<dyn Browser>) -> Result<Status> {
        self.login_with_hint(browser, LoginHint::default()).await
    }
    pub async fn login_with_hint(
        self: &Arc<Self>,
        browser: Arc<dyn Browser>,
        hint: LoginHint,
    ) -> Result<Status> {
        let account = self.clone();
        tokio::spawn(async move { account.run_login(browser.as_ref(), hint).await })
            .await
            .map_err(|_| Error::Cancelled)?
    }
    async fn run_login(self: &Arc<Self>, browser: &dyn Browser, hint: LoginHint) -> Result<Status> {
        log::info!(target: "codeterm_account::sign_in", "sign_in attempt outcome=started stage={} error_kind=none", SignInStage::Preparation.name());
        let _operation = match self.operation.try_lock() {
            Ok(operation) => operation,
            Err(_) => {
                let cancel = self.state.lock().await.login_cancel.take();
                let Some(cancel) = cancel else {
                    return Err(Error::Pending);
                };
                let _ = cancel.send(());
                self.operation.lock().await
            }
        };
        let (cancel, mut cancelled) = oneshot::channel();
        let generation = {
            let mut state = self.state.lock().await;
            state.generation += 1;
            state.busy = true;
            state.login_cancel = Some(cancel);
            state.last_sign_in_failure = None;
            state.generation
        };
        let mut attempt = SignInAttempt::default();
        log::debug!(target: "codeterm_account::sign_in", "sign_in detail stage={} provider_hint_present={} mode_hint_present={}", SignInStage::Preparation.name(), hint.provider.is_some(), hint.mode.is_some());
        let result = tokio::select! {
            _ = &mut cancelled => Err(Error::Cancelled),
            result = tokio::time::timeout(self.deadline, async {
                let loopback = attempt.run(SignInStage::Preparation, async {
                    if self.provider.needs_browser_callback() { Loopback::bind().await.map(Some) } else { Ok(None) }
                }).await?;
                let redirect = match &loopback { Some(listener) => listener.redirect()?, None => "domios://auth/callback".into() };
                let flow = LoginFlow::new(redirect).with_hint(hint);
                let start = attempt.run(SignInStage::Preparation, self.provider.start_login(&flow)).await?;
                let tokens = match start {
                    LoginStart::Immediate(tokens) => tokens,
                    LoginStart::Browser(url) => {
                        let loopback = loopback.ok_or(Error::Configuration)?;
                        let (sender, receiver) = oneshot::channel();
                        {
                            let mut state = self.state.lock().await;
                            if generation != state.generation { return Err(Error::Cancelled); }
                            state.pending = Some((flow, sender));
                            #[cfg(debug_assertions)]
                            { state.pending_url = Some(url.clone()); }
                        }
                        attempt.run(SignInStage::Browser, browser.open(&url)).await?;
                        let request = {
                            let serve = loopback.serve(self.clone(), self.browser_return_to_app);
                            tokio::pin!(serve);
                            tokio::pin!(receiver);
                            attempt.run(SignInStage::Callback, async { tokio::select! {
                                result = &mut receiver => result.map_err(|_| Error::Cancelled),
                                result = &mut serve => { result?; receiver.await.map_err(|_| Error::Cancelled) }
                            } }).await?
                        };
                        attempt.run(SignInStage::Redemption, self.provider.redeem(request)).await?
                    }
                };
                self.commit(tokens, generation, &mut attempt).await
            }) => result.unwrap_or(Err(Error::Timeout)),
        };
        self.finish_login(result, &attempt).await?;
        Ok(self.status().await)
    }
    async fn finish_login(&self, result: Result<()>, attempt: &SignInAttempt) -> Result<()> {
        let mut state = self.state.lock().await;
        state.pending = None;
        state.login_cancel = None;
        #[cfg(debug_assertions)]
        {
            state.pending_url = None;
        }
        state.busy = false;
        if result.is_err() {
            let generation = state.generation;
            if let Some(session) = state.session.as_mut() {
                session.login_generation = generation;
            }
        }
        state.last_sign_in_failure = if state.refresh_blocked && result == Err(Error::Cancelled) {
            None
        } else {
            attempt.failure(result.as_ref().err().copied())
        };
        if let Some(failure) = state.last_sign_in_failure {
            log::info!(target: "codeterm_account::sign_in", "sign_in attempt outcome=failure stage={} error_kind={}", failure.stage.name(), crate::sign_in_diagnostic::error_kind(failure.error));
        } else {
            log::info!(target: "codeterm_account::sign_in", "sign_in attempt outcome=success stage={} error_kind=none", SignInStage::Publication.name());
        }
        result
    }
    async fn commit(
        &self,
        tokens: Tokens,
        generation: u64,
        attempt: &mut SignInAttempt,
    ) -> Result<()> {
        self.commit_tokens(tokens, generation, attempt, true).await
    }
    async fn commit_tokens(
        &self,
        mut tokens: Tokens,
        generation: u64,
        attempt: &mut SignInAttempt,
        announce_sign_in: bool,
    ) -> Result<()> {
        if let Some(admission) = &self.admission {
            attempt
                .run(SignInStage::Publication, admission.admit(&tokens.session))
                .await?;
        }
        let mut state = self.state.lock().await;
        attempt
            .run(SignInStage::Publication, async {
                if generation != state.generation {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            })
            .await?;
        let saved = self.store.load(self.provider.id()).await?;
        if saved.as_ref().map(Secret::expose) != Some(tokens.refresh_token.expose()) {
            attempt
                .run(
                    SignInStage::CredentialSave,
                    self.store.save(self.provider.id(), &tokens.refresh_token),
                )
                .await?;
        }
        attempt.enter(SignInStage::Publication);
        log::debug!(target: "codeterm_account::sign_in", "sign_in detail stage={} identity_present=true", SignInStage::Publication.name());
        tokens.session.login_generation = generation;
        let announce_identity = announce_sign_in
            || state.session.as_ref().map(|session| &session.identity)
                != Some(&tokens.session.identity);
        state.session = Some(tokens.session);
        state.refresh_blocked = false;
        state.session_revoked = false;
        state.last_sign_in_failure = None;
        if let Some(observer) = self.observer.as_ref().filter(|_| announce_identity) {
            observer
                .changed(state.session.as_ref().map(|session| &session.identity))
                .await;
        }
        let refreshed = if announce_sign_in {
            None
        } else {
            state.session.clone()
        };
        drop(state);
        if let (Some(observer), Some(session)) = (&self.observer, refreshed) {
            observer.session_refreshed(&session).await;
        }
        Ok(())
    }
    pub async fn refresh(&self) -> Result<Status> {
        let (generation, revision) = {
            let state = self.state.lock().await;
            (state.generation, state.refresh_revision)
        };
        self.refresh_shared(generation, revision, true).await?;
        Ok(self.status().await)
    }
    async fn refresh_shared(&self, generation: u64, revision: u64, force: bool) -> Result<()> {
        if self.state.lock().await.busy {
            return Err(Error::Pending);
        }
        let _operation = self.operation.lock().await;
        {
            let state = self.state.lock().await;
            if state.refresh_blocked {
                return Err(Error::Cancelled);
            }
            if state.refresh_revision != revision {
                if let Some((owner, result)) = state.last_refresh {
                    if owner == generation {
                        return result;
                    }
                }
            }
            if state.generation != generation {
                return Err(Error::Cancelled);
            }
            if !force
                && !state
                    .session
                    .as_ref()
                    .is_some_and(|session| session.expires_at.is_some_and(token_expired))
            {
                return Ok(());
            }
        }
        let result = self.refresh_locked().await;
        let cleared = {
            let mut state = self.state.lock().await;
            state.refresh_revision += 1;
            state.last_refresh = Some((generation, result));
            if result == Err(Error::Credentials)
                && state.generation == generation
                && state
                    .session
                    .as_ref()
                    .is_some_and(|session| force || session.expires_at.is_some_and(token_expired))
            {
                state.generation += 1;
                state.session = None;
                true
            } else {
                false
            }
        };
        if cleared {
            if let Some(observer) = &self.observer {
                observer.changed(None).await;
            }
        }
        result
    }
    async fn refresh_locked(&self) -> Result<()> {
        let cancelled = self.refresh_cancel.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();
        let generation = {
            let state = self.state.lock().await;
            if state.refresh_blocked {
                return Err(Error::Cancelled);
            }
            state.generation
        };
        tokio::time::timeout(self.deadline, async {
            let token = self
                .store
                .load(self.provider.id())
                .await?
                .ok_or(Error::Credentials)?;
            let tokens = tokio::select! {
                biased;
                _ = &mut cancelled => return Err(Error::Cancelled),
                result = self.provider.refresh(&token) => result?,
            };
            self.commit_tokens(tokens, generation, &mut SignInAttempt::default(), false)
                .await
        })
        .await
        .map_err(|_| Error::Timeout)??;
        Ok(())
    }
    pub async fn logout(&self) -> Result<Status> {
        {
            let mut state = self.state.lock().await;
            state.generation += 1;
            state.refresh_blocked = true;
            state.session = None;
            if let Some(cancel) = state.login_cancel.take() {
                let _ = cancel.send(());
            }
            self.refresh_cancel.notify_waiters();
            state.pending = None;
            #[cfg(debug_assertions)]
            {
                state.pending_url = None;
            }
            state.busy = false;
            state.session = None;
            state.session_revoked = false;
            state.last_sign_in_failure = None;
            tokio::time::timeout(self.deadline, self.store.delete(self.provider.id()))
                .await
                .map_err(|_| Error::Timeout)??;
            if let Some(observer) = &self.observer {
                observer.changed(None).await;
            }
        }
        Ok(self.status().await)
    }
    pub async fn session_revoked(self: &Arc<Self>, expected: &Session) -> Result<bool> {
        let account = self.clone();
        let expected = expected.clone();
        // Mesh blocking can cancel the request that detected this revocation.
        tokio::spawn(async move { account.clear_revoked_session(&expected).await })
            .await
            .map_err(|_| Error::Credentials)?
    }

    async fn clear_revoked_session(&self, expected: &Session) -> Result<bool> {
        let mut state = self.state.lock().await;
        if state.generation != expected.login_generation
            || !state.session.as_ref().is_some_and(|current| {
                current.login_generation == expected.login_generation
                    && current.identity.uid == expected.identity.uid
            })
        {
            return Ok(false);
        }
        state.generation += 1;
        state.refresh_blocked = true;
        state.session = None;
        state.session_revoked = true;
        state.last_sign_in_failure = None;
        if let Some(observer) = &self.observer {
            observer.changed(None).await;
        }
        tokio::time::timeout(self.deadline, self.store.delete(self.provider.id()))
            .await
            .map_err(|_| Error::Timeout)??;
        Ok(true)
    }

    pub async fn session(&self) -> Option<Session> {
        self.state.lock().await.session.clone()
    }
    pub async fn current_session(&self) -> Result<Option<Session>> {
        self.session_with_refresh(false).await
    }
    pub async fn recover_session(&self, rejected: &Session) -> Result<Option<Session>> {
        let (generation, revision) = {
            let state = self.state.lock().await;
            let current = state.session.as_ref().ok_or(Error::Cancelled)?;
            if state.refresh_blocked
                || current.login_generation != rejected.login_generation
                || current.identity.uid != rejected.identity.uid
            {
                return Err(Error::Cancelled);
            }
            if current.id_token.expose() != rejected.id_token.expose() {
                return Ok(Some(current.clone()));
            }
            (state.generation, state.refresh_revision)
        };
        self.refresh_shared(generation, revision, true).await?;
        let state = self.state.lock().await;
        if state.refresh_blocked || state.generation != generation {
            return Err(Error::Cancelled);
        }
        Ok(state.session.clone().filter(|current| {
            current.identity.uid == rejected.identity.uid
                && current.login_generation == rejected.login_generation
        }))
    }

    pub async fn fresh_session(&self) -> Result<Option<Session>> {
        self.session_with_refresh(true).await
    }
    async fn session_with_refresh(&self, force: bool) -> Result<Option<Session>> {
        let (current, refresh_blocked, generation, revision) = {
            let state = self.state.lock().await;
            (
                state.session.clone(),
                state.refresh_blocked,
                state.generation,
                state.refresh_revision,
            )
        };
        let Some(current) = current else {
            return Ok(None);
        };
        if refresh_blocked {
            return Ok(None);
        }
        if !force && !current.expires_at.is_some_and(token_expired) {
            return Ok(Some(current));
        }

        self.refresh_shared(generation, revision, force).await?;
        let state = self.state.lock().await;
        if state.refresh_blocked || state.generation != generation {
            return Ok(None);
        }
        Ok(state
            .session
            .clone()
            .filter(|session| match session.expires_at {
                None => true,
                Some(expires_at) => unix_time().is_some_and(|now| expires_at > now),
            }))
    }
}

pub(crate) struct Completion {
    sender: oneshot::Sender<Redemption>,
    request: Redemption,
}
impl Completion {
    pub(crate) fn send(self) -> Result<()> {
        self.sender.send(self.request).map_err(|_| Error::Cancelled)
    }
}

#[cfg(test)]
#[path = "account/sign_in_tests.rs"]
mod sign_in_tests;

#[cfg(test)]
#[path = "account/listener_tests.rs"]
mod listener_tests;

#[cfg(test)]
#[path = "account/logout_tests.rs"]
mod logout_tests;
