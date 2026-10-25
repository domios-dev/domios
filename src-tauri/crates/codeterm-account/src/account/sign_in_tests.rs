use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(debug_assertions)]
#[tokio::test]
async fn pending_login_url_is_read_only_and_cleared_after_callback_or_logout() {
    let account = account(Arc::new(Store::default()));
    assert!(account.pending_login_url().await.is_none());
    let flow = LoginFlow::from_parts(
        "opaque-state".into(),
        Secret::new("opaque-verifier"),
        "domios://auth/callback".into(),
    );
    let url = flow.signin_url("https://example.test/signin/").unwrap();
    let (sender, receiver) = oneshot::channel();
    {
        let mut state = account.state.lock().await;
        state.pending = Some((flow, sender));
        state.pending_url = Some(url.clone());
    }
    assert_eq!(account.pending_login_url().await, Some(url.clone()));
    assert_eq!(account.pending_login_url().await, Some(url.clone()));
    assert!(!url.as_str().contains("opaque-verifier"));
    account
        .complete(Callback {
            state: "opaque-state".into(),
            code: "c".repeat(43),
        })
        .await
        .unwrap();
    assert!(account.pending_login_url().await.is_none());
    assert_eq!(receiver.await.unwrap().verifier.expose(), "opaque-verifier");
    {
        let mut state = account.state.lock().await;
        let (sender, _receiver) = oneshot::channel();
        state.pending = Some((LoginFlow::new("domios://auth/callback".into()), sender));
        state.pending_url = Some(url);
    }
    account.logout().await.unwrap();
    assert!(account.pending_login_url().await.is_none());
    assert!(account.state.lock().await.pending_url.is_none());
}

struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        panic!("Injected immediate sign-in must not open a browser")
    }
}
#[derive(Default)]
pub(super) struct Store {
    fail: AtomicBool,
}
#[async_trait]
impl CredentialStore for Store {
    async fn load(&self, _: &str) -> Result<Option<Secret>> {
        Ok(None)
    }
    async fn save(&self, _: &str, _: &Secret) -> Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            Err(Error::Credentials)
        } else {
            Ok(())
        }
    }
    async fn delete(&self, _: &str) -> Result<()> {
        Ok(())
    }
}

struct RefreshStore;
#[async_trait]
impl CredentialStore for RefreshStore {
    async fn load(&self, _: &str) -> Result<Option<Secret>> {
        Ok(Some(Secret::new("refresh-credential")))
    }
    async fn save(&self, _: &str, _: &Secret) -> Result<()> {
        Ok(())
    }
    async fn delete(&self, _: &str) -> Result<()> {
        Ok(())
    }
}

struct RefreshProvider {
    refreshes: AtomicUsize,
    failure: Option<Error>,
}
impl RefreshProvider {
    fn tokens() -> Tokens {
        Tokens {
            session: Session {
                login_generation: 0,
                identity: Identity {
                    uid: "refreshed-user".into(),
                    email: None,
                    display_name: None,
                    provider: "refresh-test".into(),
                    photo_url: None,
                },
                id_token: Secret::new("fresh-id-token"),
                expires_at: Some(u64::MAX),
            },
            refresh_token: Secret::new("refresh-credential"),
        }
    }
}
#[async_trait]
impl IdentityProvider for RefreshProvider {
    fn id(&self) -> &str {
        "refresh-test"
    }
    async fn start_login(&self, _: &LoginFlow) -> Result<LoginStart> {
        Err(Error::Provider)
    }
    async fn redeem(&self, _: Redemption) -> Result<Tokens> {
        Err(Error::Provider)
    }
    async fn refresh(&self, token: &Secret) -> Result<Tokens> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.failure {
            return Err(error);
        }
        if token.expose() != "refresh-credential" {
            return Err(Error::Credentials);
        }
        Ok(Self::tokens())
    }
    async fn revoke(&self, _: &Secret) -> Result<()> {
        Ok(())
    }
}
fn account(store: Arc<Store>) -> Arc<Account> {
    Arc::new(Account::new(
        Arc::new(adapters::local::Local),
        store,
        Duration::from_secs(1),
    ))
}

fn expired_session() -> Session {
    Session {
        login_generation: 0,
        identity: Identity {
            uid: "refreshed-user".into(),
            email: None,
            display_name: None,
            provider: "refresh-test".into(),
            photo_url: None,
        },
        id_token: Secret::new("expired-id-token"),
        expires_at: Some(0),
    }
}

#[tokio::test]
async fn terminal_failure_stays_in_status_for_every_injected_boundary() {
    let account = account(Arc::new(Store::default()));
    for stage in [
        SignInStage::Preparation,
        SignInStage::Browser,
        SignInStage::Callback,
        SignInStage::Redemption,
        SignInStage::CredentialSave,
        SignInStage::Publication,
    ] {
        account.state.lock().await.busy = true;
        let mut attempt = SignInAttempt::default();
        let result = attempt.run(stage, async { Err(Error::Transport) }).await;
        assert_eq!(
            account.finish_login(result, &attempt).await,
            Err(Error::Transport)
        );
        let expected = Some(SignInFailure {
            stage,
            error: Error::Transport,
        });
        for _ in 0..2 {
            let status = account.status().await;
            assert!(!status.pending);
            assert!(status.identity.is_none());
            assert_eq!(status.last_sign_in_failure, expected);
            let diagnostic = serde_json::to_value(status.last_sign_in_failure).unwrap();
            assert_eq!(diagnostic.as_object().unwrap().len(), 2);
        }
    }
}

#[tokio::test]
async fn credential_save_failure_is_retained_and_successful_login_clears_it() {
    let store = Arc::new(Store::default());
    store.fail.store(true, Ordering::SeqCst);
    let account = account(store.clone());
    assert_eq!(
        account.login(Arc::new(NoBrowser)).await.unwrap_err(),
        Error::Credentials
    );
    assert_eq!(
        account.status().await.last_sign_in_failure,
        Some(SignInFailure {
            stage: SignInStage::CredentialSave,
            error: Error::Credentials,
        })
    );
    store.fail.store(false, Ordering::SeqCst);
    let status = account.login(Arc::new(NoBrowser)).await.unwrap();
    assert!(status.identity.is_some());
    assert!(status.last_sign_in_failure.is_none());
}

#[tokio::test]
async fn generation_refusal_is_attributed_to_publication_before_saving() {
    let store = Arc::new(Store::default());
    store.fail.store(true, Ordering::SeqCst);
    let account = account(store);
    let LoginStart::Immediate(tokens) = adapters::local::Local
        .start_login(&LoginFlow::new("unused".into()))
        .await
        .unwrap()
    else {
        panic!("Expected injected immediate tokens")
    };
    let mut attempt = SignInAttempt::default();
    let result = account.commit(tokens, 1, &mut attempt).await;
    assert_eq!(
        account.finish_login(result, &attempt).await,
        Err(Error::Cancelled)
    );
    assert_eq!(
        account.status().await.last_sign_in_failure,
        Some(SignInFailure {
            stage: SignInStage::Publication,
            error: Error::Cancelled,
        })
    );
    assert!(account.session().await.is_none());
}

#[tokio::test]
async fn current_session_refreshes_an_expired_id_token_with_the_saved_credential() {
    let provider = Arc::new(RefreshProvider {
        refreshes: AtomicUsize::new(0),
        failure: None,
    });
    let account = Account::new(
        provider.clone(),
        Arc::new(RefreshStore),
        Duration::from_secs(1),
    );
    account.state.lock().await.session = Some(expired_session());

    let current = account.current_session().await.unwrap().unwrap();

    assert_eq!(current.id_token.expose(), "fresh-id-token");
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn current_session_returns_one_refresh_failure_without_using_the_expired_token() {
    let provider = Arc::new(RefreshProvider {
        refreshes: AtomicUsize::new(0),
        failure: Some(Error::Credentials),
    });
    let account = Account::new(
        provider.clone(),
        Arc::new(RefreshStore),
        Duration::from_secs(1),
    );
    account.state.lock().await.session = Some(expired_session());

    assert_eq!(
        account.current_session().await.unwrap_err(),
        Error::Credentials
    );
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
    assert!(account.session().await.is_none());
    assert!(account.status().await.identity.is_none());
}

#[tokio::test]
async fn current_session_keeps_session_after_a_transient_refresh_failure() {
    let provider = Arc::new(RefreshProvider {
        refreshes: AtomicUsize::new(0),
        failure: Some(Error::Transport),
    });
    let account = Account::new(
        provider.clone(),
        Arc::new(RefreshStore),
        Duration::from_secs(1),
    );
    let expired = expired_session();
    account.state.lock().await.session = Some(expired.clone());

    assert_eq!(
        account.current_session().await.unwrap_err(),
        Error::Transport
    );
    assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
    let retained = account.session().await.expect("session remains available");
    assert_eq!(retained.id_token.expose(), expired.id_token.expose());
    assert_eq!(retained.identity.uid, expired.identity.uid);
}
