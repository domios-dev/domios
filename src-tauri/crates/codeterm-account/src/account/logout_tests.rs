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
    failure: StoreMutex<Option<Error>>,
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
        if let Some(error) = *self.failure.lock() {
            return Err(error);
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

struct RefusedAdmission;
#[async_trait]
impl SessionAdmission for RefusedAdmission {
    async fn admit(&self, _: &Session) -> Result<()> {
        Err(Error::Transport)
    }
}

#[tokio::test]
async fn deferred_server_stop_refusal_prevents_new_session_and_credential_publication() {
    let store = Arc::new(Store::default());
    let account = Account::new(
        Arc::new(Provider::default()),
        store.clone(),
        Duration::from_secs(1),
    )
    .with_session_admission(Arc::new(RefusedAdmission));
    assert_eq!(
        account.login(Arc::new(NoBrowser)).await.unwrap_err(),
        Error::Transport
    );
    assert!(account.session().await.is_none());
    assert!(account.status().await.identity.is_none());
    assert!(store.token.lock().is_none());
    assert_eq!(store.saves.load(Ordering::SeqCst), 0);
}

#[derive(Default)]
struct GatedAdmission {
    started: Notify,
    release: Notify,
}
#[async_trait]
impl SessionAdmission for GatedAdmission {
    async fn admit(&self, _: &Session) -> Result<()> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(())
    }
}

#[tokio::test]
async fn sign_out_during_server_stop_admission_never_publishes_the_waiting_session() {
    let store = Arc::new(Store::default());
    let admission = Arc::new(GatedAdmission::default());
    let account = Arc::new(
        Account::new(
            Arc::new(Provider::default()),
            store.clone(),
            Duration::from_secs(1),
        )
        .with_session_admission(admission.clone()),
    );
    let caller = account.clone();
    let signing_in = tokio::spawn(async move { caller.login(Arc::new(NoBrowser)).await });
    admission.started.notified().await;
    assert!(account.logout().await.unwrap().identity.is_none());
    admission.release.notify_one();
    assert_eq!(signing_in.await.unwrap().unwrap_err(), Error::Cancelled);
    assert!(account.session().await.is_none());
    assert!(store.token.lock().is_none());
    assert_eq!(store.saves.load(Ordering::SeqCst), 0);
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

#[tokio::test]
async fn current_session_reuses_a_valid_token_even_with_less_than_a_minute_left() {
    let (account, _, provider) = fixture();
    account.login(Arc::new(NoBrowser)).await.unwrap();
    account
        .state
        .lock()
        .await
        .session
        .as_mut()
        .unwrap()
        .expires_at = Some(unix_time().unwrap() + 30);
    let initial = account.session().await.unwrap().id_token;
    for _ in 0..3 {
        assert_eq!(
            account
                .current_session()
                .await
                .unwrap()
                .unwrap()
                .id_token
                .expose(),
            initial.expose()
        );
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_expiry_and_explicit_refreshes_share_success_and_failure() {
    for failure in [None, Some(Error::Transport), Some(Error::Credentials)] {
        let (account, _, provider) = fixture();
        account.login(Arc::new(NoBrowser)).await.unwrap();
        account
            .state
            .lock()
            .await
            .session
            .as_mut()
            .unwrap()
            .expires_at = Some(1);
        provider.pause.store(true, Ordering::SeqCst);
        *provider.failure.lock() = failure;
        let reads = async {
            tokio::join!(
                account.current_session(),
                account.current_session(),
                account.fresh_session(),
                async {
                    account.refresh().await?;
                    Ok::<_, Error>(account.session().await)
                }
            )
        };
        tokio::pin!(reads);
        tokio::select! {
            _ = provider.started.notified() => {}
            _ = &mut reads => panic!("refresh must wait for its explicit release"),
        }
        provider.release.notify_one();
        let (first, second, third, fourth) = reads.await;
        for result in [first, second, third, fourth] {
            match failure {
                Some(error) => assert_eq!(result.unwrap_err(), error),
                None => assert_eq!(result.unwrap().unwrap().id_token.expose(), "id-0"),
            }
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            account.session().await.is_none(),
            failure == Some(Error::Credentials)
        );
    }
}
