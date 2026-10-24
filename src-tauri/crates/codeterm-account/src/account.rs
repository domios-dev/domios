use crate::sign_in_diagnostic::SignInAttempt;
use crate::*;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{oneshot, Mutex};

const ID_TOKEN_REFRESH_WINDOW_SECONDS: u64 = 60;

fn unix_time() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn token_expiring(expires_at: u64) -> bool {
    match unix_time() {
        Some(now) => expires_at <= now.saturating_add(ID_TOKEN_REFRESH_WINDOW_SECONDS),
        None => true,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Status {
    pub provider: String,
    pub identity: Option<Identity>,
    pub pending: bool,
    pub session_only: bool,
    pub last_sign_in_failure: Option<SignInFailure>,
}
#[derive(Default)]
struct State {
    session: Option<Session>,
    pending: Option<(LoginFlow, oneshot::Sender<Redemption>)>,
    generation: u64,
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
    deadline: Duration,
    observer: Option<Arc<dyn IdentityObserver>>,
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
            state: Mutex::new(State::default()),
            operation: Mutex::new(()),
        }
    }
    pub fn with_identity_observer(mut self, observer: Arc<dyn IdentityObserver>) -> Self {
        self.observer = Some(observer);
        self
    }
    pub async fn status(&self) -> Status {
        let state = self.state.lock().await;
        Status {
            provider: self.provider.id().into(),
            identity: state.session.as_ref().map(|s| s.identity.clone()),
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
    pub async fn login(self: &Arc<Self>, browser: &dyn Browser) -> Result<Status> {
        self.login_with_hint(browser, LoginHint::default()).await
    }
    pub async fn login_with_hint(
        self: &Arc<Self>,
        browser: &dyn Browser,
        hint: LoginHint,
    ) -> Result<Status> {
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
                            let serve = loopback.serve(self.clone());
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
        state.last_sign_in_failure = attempt.failure(result.as_ref().err().copied());
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
        tokens: Tokens,
        generation: u64,
        attempt: &mut SignInAttempt,
        announce_sign_in: bool,
    ) -> Result<()> {
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
        attempt
            .run(
                SignInStage::CredentialSave,
                self.store.save(self.provider.id(), &tokens.refresh_token),
            )
            .await?;
        attempt.enter(SignInStage::Publication);
        log::debug!(target: "codeterm_account::sign_in", "sign_in detail stage={} identity_present=true", SignInStage::Publication.name());
        let announce_identity = announce_sign_in
            || state.session.as_ref().map(|session| &session.identity)
                != Some(&tokens.session.identity);
        state.session = Some(tokens.session);
        state.last_sign_in_failure = None;
        if let Some(observer) = self.observer.as_ref().filter(|_| announce_identity) {
            observer
                .changed(state.session.as_ref().map(|session| &session.identity))
                .await;
        }
        Ok(())
    }
    pub async fn refresh(&self) -> Result<Status> {
        let _operation = self.operation.try_lock().map_err(|_| Error::Pending)?;
        self.refresh_locked().await?;
        Ok(self.status().await)
    }
    async fn refresh_locked(&self) -> Result<()> {
        let generation = self.state.lock().await.generation;
        tokio::time::timeout(self.deadline, async {
            let token = self
                .store
                .load(self.provider.id())
                .await?
                .ok_or(Error::Credentials)?;
            let tokens = self.provider.refresh(&token).await?;
            self.commit_tokens(tokens, generation, &mut SignInAttempt::default(), false)
                .await
        })
        .await
        .map_err(|_| Error::Timeout)??;
        Ok(())
    }
    pub async fn logout(&self) -> Result<Status> {
        let token = {
            let mut state = self.state.lock().await;
            state.generation += 1;
            if let Some(cancel) = state.login_cancel.take() {
                let _ = cancel.send(());
            }
            state.pending = None;
            #[cfg(debug_assertions)]
            {
                state.pending_url = None;
            }
            state.busy = false;
            state.session = None;
            if let Some(observer) = &self.observer {
                observer.changed(None).await;
            }
            tokio::time::timeout(self.deadline, async {
                let token = self.store.load(self.provider.id()).await?;
                self.store.delete(self.provider.id()).await?;
                Ok::<_, Error>(token)
            })
            .await
            .map_err(|_| Error::Timeout)??
        };
        if let Some(token) = token {
            tokio::time::timeout(self.deadline, self.provider.revoke(&token))
                .await
                .map_err(|_| Error::Timeout)??;
        }
        Ok(self.status().await)
    }
    pub async fn session(&self) -> Option<Session> {
        self.state.lock().await.session.clone()
    }
    pub async fn current_session(&self) -> Result<Option<Session>> {
        self.session_with_refresh(false).await
    }
    pub async fn fresh_session(&self) -> Result<Option<Session>> {
        self.session_with_refresh(true).await
    }
    async fn session_with_refresh(&self, force: bool) -> Result<Option<Session>> {
        let current = self.state.lock().await.session.clone();
        let Some(current) = current else {
            return Ok(None);
        };
        if !force && !current.expires_at.is_some_and(token_expiring) {
            return Ok(Some(current));
        }

        let _operation = self.operation.lock().await;
        let current = self.state.lock().await.session.clone();
        let Some(current) = current else {
            return Ok(None);
        };
        if force || current.expires_at.is_some_and(token_expiring) {
            if let Err(error) = self.refresh_locked().await {
                if error == Error::Credentials {
                    let expired = {
                        let mut state = self.state.lock().await;
                        if state.session.as_ref().is_some_and(|session| {
                            force || session.expires_at.is_some_and(token_expiring)
                        }) {
                            state.generation += 1;
                            state.session = None;
                            true
                        } else {
                            false
                        }
                    };
                    if expired {
                        if let Some(observer) = &self.observer {
                            observer.changed(None).await;
                        }
                    }
                }
                return Err(error);
            }
        }
        let current = self.state.lock().await.session.clone();
        Ok(current.filter(|session| match session.expires_at {
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
