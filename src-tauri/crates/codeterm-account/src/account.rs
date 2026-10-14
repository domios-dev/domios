use crate::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::{oneshot, Mutex};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Status {
    pub provider: String,
    pub identity: Option<Identity>,
    pub pending: bool,
}
#[derive(Default)]
struct State {
    session: Option<Session>,
    pending: Option<(LoginFlow, oneshot::Sender<Redemption>)>,
    generation: u64,
    busy: bool,
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
        }
    }
    pub async fn complete(&self, callback: Callback) -> Result<()> {
        self.prepare_callback(callback).await?.send()
    }
    pub(crate) async fn prepare_callback(&self, callback: Callback) -> Result<Completion> {
        let mut state = self.state.lock().await;
        let (flow, _) = state.pending.as_mut().ok_or(Error::Consumed)?;
        let request = flow.complete(callback)?;
        let (_, sender) = state.pending.take().ok_or(Error::Consumed)?;
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
        let _operation = self.operation.try_lock().map_err(|_| Error::Pending)?;
        let generation = {
            let mut state = self.state.lock().await;
            state.busy = true;
            state.generation
        };
        let result = tokio::time::timeout(self.deadline, async {
            let loopback = if self.provider.needs_browser_callback() { Some(Loopback::bind().await?) } else { None };
            let redirect = match &loopback { Some(listener) => listener.redirect()?, None => "domios://auth/callback".into() };
            let flow = LoginFlow::new(redirect).with_hint(hint);
            let start = self.provider.start_login(&flow).await?;
            let tokens = match start {
                LoginStart::Immediate(tokens) => tokens,
                LoginStart::Browser(url) => {
                    let loopback = loopback.ok_or(Error::Configuration)?;
                    let (sender, receiver) = oneshot::channel();
                    {
                        let mut state = self.state.lock().await;
                        if generation != state.generation { return Err(Error::Cancelled); }
                        state.pending = Some((flow, sender));
                    }
                    browser.open(&url).await?;
                    let serve = loopback.serve(self.clone());
                    tokio::pin!(serve);
                    tokio::pin!(receiver);
                    let request = tokio::select! {
                        result = &mut receiver => result.map_err(|_| Error::Cancelled)?,
                        result = &mut serve => { result?; receiver.await.map_err(|_| Error::Cancelled)? }
                    };
                    self.provider.redeem(request).await?
                }
            };
            self.commit(tokens, generation).await
        }).await.unwrap_or(Err(Error::Timeout));
        {
            let mut state = self.state.lock().await;
            state.pending = None;
            state.busy = false;
        }
        result?;
        Ok(self.status().await)
    }
    async fn commit(&self, tokens: Tokens, generation: u64) -> Result<()> {
        let mut state = self.state.lock().await;
        if generation != state.generation {
            return Err(Error::Cancelled);
        }
        self.store
            .save(self.provider.id(), &tokens.refresh_token)
            .await?;
        state.session = Some(tokens.session);
        if let Some(observer) = &self.observer {
            observer
                .changed(state.session.as_ref().map(|session| &session.identity))
                .await;
        }
        Ok(())
    }
    pub async fn refresh(&self) -> Result<Status> {
        let _operation = self.operation.try_lock().map_err(|_| Error::Pending)?;
        let generation = self.state.lock().await.generation;
        tokio::time::timeout(self.deadline, async {
            let token = self
                .store
                .load(self.provider.id())
                .await?
                .ok_or(Error::Credentials)?;
            let tokens = self.provider.refresh(&token).await?;
            self.commit(tokens, generation).await
        })
        .await
        .map_err(|_| Error::Timeout)??;
        Ok(self.status().await)
    }
    pub async fn logout(&self) -> Result<Status> {
        let token = {
            let mut state = self.state.lock().await;
            state.generation += 1;
            state.pending = None;
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
