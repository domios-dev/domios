use super::*;
use parking_lot::Mutex as StoreMutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct Store {
    token: StoreMutex<Option<Secret>>,
    saves: AtomicUsize,
}
#[async_trait]
impl CredentialStore for Store {
    async fn load(&self, _: &str) -> Result<Option<Secret>> {
        Ok(self.token.lock().clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> Result<()> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        *self.token.lock() = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> Result<()> {
        *self.token.lock() = None;
        Ok(())
    }
}
#[derive(Default)]
struct Provider {
    pause: AtomicBool,
    calls: AtomicUsize,
    started: Notify,
    release: Notify,
    next_token: StoreMutex<String>,
}
#[async_trait]
impl IdentityProvider for Provider {
    fn id(&self) -> &str {
        "fixture"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(&self, _: &LoginFlow) -> Result<LoginStart> {
        let mut tokens = adapters::local::Local
            .refresh(&Secret::new("local-dev-credential"))
            .await?;
        tokens.refresh_token = Secret::new("initial");
        Ok(LoginStart::Immediate(tokens))
    }
    async fn redeem(&self, _: Redemption) -> Result<Tokens> {
        Err(Error::Provider)
    }
    async fn refresh(&self, _: &Secret) -> Result<Tokens> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        if self.pause.load(Ordering::SeqCst) {
            self.release.notified().await;
        }
        let mut tokens = adapters::local::Local
            .refresh(&Secret::new("local-dev-credential"))
            .await?;
        tokens.refresh_token = Secret::new(self.next_token.lock().clone());
        tokens.session.id_token = Secret::new(format!("id-{call}"));
        Ok(tokens)
    }
    async fn revoke(&self, _: &Secret) -> Result<()> {
        panic!("local sign-out must never use the old credential for network cleanup")
    }
}
struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        panic!("no browser")
    }
}
fn fixture() -> (Arc<Account>, Arc<Store>, Arc<Provider>) {
    let store = Arc::new(Store::default());
    let provider = Arc::new(Provider::default());
    *provider.next_token.lock() = "initial".into();
    let account = Arc::new(Account::new(
        provider.clone(),
        store.clone(),
        Duration::from_secs(10),
    ));
    (account, store, provider)
}

#[tokio::test(start_paused = true)]
async fn refresh_saves_only_a_changed_refresh_token() {
    let (account, store, provider) = fixture();
    account.login(Arc::new(NoBrowser)).await.unwrap();
    assert_eq!(store.saves.load(Ordering::SeqCst), 1);
    let first = account.fresh_session().await.unwrap().unwrap();
    let second = account.fresh_session().await.unwrap().unwrap();
    assert_ne!(first.id_token.expose(), second.id_token.expose());
    assert_eq!(store.saves.load(Ordering::SeqCst), 1);
    *provider.next_token.lock() = "rotated".into();
    account.fresh_session().await.unwrap();
    account.fresh_session().await.unwrap();
    assert_eq!(store.saves.load(Ordering::SeqCst), 2);
    assert_eq!(store.token.lock().as_ref().unwrap().expose(), "rotated");
}

#[tokio::test(start_paused = true)]
async fn logout_cancels_refresh_and_stays_signed_out_after_reconnect_and_restart() {
    let (account, store, provider) = fixture();
    account.login(Arc::new(NoBrowser)).await.unwrap();
    provider.pause.store(true, Ordering::SeqCst);
    let refreshing = tokio::spawn({
        let account = account.clone();
        async move { account.fresh_session().await }
    });
    provider.started.notified().await;
    let stale_generation = account.session().await.unwrap().login_generation;
    let status = account.logout().await.unwrap();
    assert!(status.identity.is_none());
    assert!(!status.pending);
    assert_eq!(refreshing.await.unwrap().unwrap_err(), Error::Cancelled);
    provider.pause.store(false, Ordering::SeqCst);
    provider.release.notify_one();
    assert_eq!(account.refresh().await.unwrap_err(), Error::Cancelled);
    let LoginStart::Immediate(stale) = provider
        .start_login(&LoginFlow::new("unused".into()))
        .await
        .unwrap()
    else {
        panic!("immediate")
    };
    assert_eq!(
        account
            .commit(stale, stale_generation, &mut SignInAttempt::default())
            .await,
        Err(Error::Cancelled)
    );
    assert!(account.fresh_session().await.unwrap().is_none());
    assert!(store.token.lock().is_none());
    assert_eq!(store.saves.load(Ordering::SeqCst), 1);
    let restarted = Account::new(provider.clone(), store, Duration::from_secs(10));
    assert_eq!(restarted.refresh().await.unwrap_err(), Error::Credentials);
    assert!(restarted.status().await.identity.is_none());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
