use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        panic!("Injected immediate sign-in must not open a browser")
    }
}
#[derive(Default)]
struct Store {
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
fn account(store: Arc<Store>) -> Arc<Account> {
    Arc::new(Account::new(
        Arc::new(adapters::local::Local),
        store,
        Duration::from_secs(1),
    ))
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
        account.login(&NoBrowser).await.unwrap_err(),
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
    let status = account.login(&NoBrowser).await.unwrap();
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
