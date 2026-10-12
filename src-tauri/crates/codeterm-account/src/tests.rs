use super::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Default)]
struct Store(Mutex<Option<Secret>>);
#[async_trait]
impl CredentialStore for Store {
    async fn load(&self, _: &str) -> Result<Option<Secret>> {
        Ok(self.0.lock().await.clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> Result<()> {
        *self.0.lock().await = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> Result<()> {
        *self.0.lock().await = None;
        Ok(())
    }
}
struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        panic!("local login must not open a browser")
    }
}
fn flow() -> LoginFlow {
    LoginFlow::from_parts(
        "fixture-state-matching".into(),
        Secret::new("fixture-verifier"),
        "domios://auth/callback".into(),
    )
}
fn callback(state: &str) -> Callback {
    Callback {
        code: "c".repeat(43),
        state: state.into(),
    }
}
#[test]
fn pkce_rfc7636_appendix_b() {
    assert_eq!(
        LoginFlow::challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}
#[test]
fn callback_wrong_state_stays_pending() {
    let mut flow = flow();
    assert!(matches!(
        flow.complete(callback("wrong-state")),
        Err(Error::WrongState)
    ));
    let request = flow.complete(callback("fixture-state-matching")).unwrap();
    assert_eq!(request.verifier.expose(), "fixture-verifier");
}
#[test]
fn callback_second_refused() {
    let mut flow = flow();
    flow.complete(callback("fixture-state-matching")).unwrap();
    assert!(matches!(
        flow.complete(callback("fixture-state-matching")),
        Err(Error::Consumed)
    ));
}
#[test]
fn callback_parsing_refuses_wrong_path_duplicate_fields_and_credentials() {
    let good = format!(
        "domios://auth/callback?code={}&state=fixture-state-matching",
        "c".repeat(43)
    );
    assert!(Callback::parse(&good).is_ok());
    assert!(Callback::parse(&good.replace("/callback", "/other")).is_err());
    assert!(Callback::parse(&format!("{good}&state=second")).is_err());
    assert!(Callback::parse(&good.replace("//auth", "//user@auth")).is_err());
}
#[test]
fn signin_url_contains_only_challenge_and_state() {
    let url = flow()
        .signin_url("https://accounts.example.test/signin/")
        .unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query.len(), 3);
    assert_eq!(
        query["code_challenge"],
        LoginFlow::challenge("fixture-verifier")
    );
    assert!(!url.as_str().contains("fixture-verifier"));
    assert!(flow()
        .signin_url("http://accounts.example.test/signin/")
        .is_err());
}
struct WaitingBrowser;
#[async_trait]
impl Browser for WaitingBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        Ok(())
    }
}
#[tokio::test(start_paused = true)]
async fn login_failure_deadline_returns_timeout() {
    let store = Arc::new(Store::default());
    let account = Arc::new(Account::new(
        Arc::new(adapters::fake::Fake::default()),
        store.clone(),
        Duration::from_secs(1),
    ));
    assert_eq!(
        account.login(&WaitingBrowser).await.unwrap_err(),
        Error::Timeout
    );
    assert_eq!(
        account.status().await,
        Status {
            identity: None,
            pending: false
        }
    );
    assert!(store.0.lock().await.is_none());
}
#[tokio::test]
async fn local_login_logout_deletes_refresh_token() {
    let store = Arc::new(Store::default());
    let account = Arc::new(Account::new(
        Arc::new(adapters::local::Local),
        store.clone(),
        Duration::from_secs(1),
    ));
    assert_eq!(
        account
            .login(&NoBrowser)
            .await
            .unwrap()
            .identity
            .unwrap()
            .uid,
        "local-dev-user"
    );
    assert!(store.0.lock().await.is_some());
    account.logout().await.unwrap();
    assert!(store.0.lock().await.is_none());
    assert!(account.session().await.is_none());
}
#[tokio::test]
async fn refresh_replaces_id_token() {
    let store = Arc::new(Store::default());
    store
        .save("fake", &Secret::new("fake-refresh"))
        .await
        .unwrap();
    let account = Account::new(
        Arc::new(adapters::fake::Fake::default()),
        store,
        Duration::from_secs(1),
    );
    account.refresh().await.unwrap();
    let before = account.session().await.unwrap().id_token;
    account.refresh().await.unwrap();
    assert_ne!(
        before.expose(),
        account.session().await.unwrap().id_token.expose()
    );
}

struct CallbackBrowser {
    account: Arc<Account>,
}
#[async_trait]
impl Browser for CallbackBrowser {
    async fn open(&self, url: &url::Url) -> Result<()> {
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        assert_eq!(
            self.account
                .complete(callback("wrong-state-value"))
                .await
                .unwrap_err(),
            Error::WrongState
        );
        assert!(self.account.status().await.pending);
        self.account.complete(callback(&state)).await?;
        assert_eq!(
            self.account.complete(callback(&state)).await.unwrap_err(),
            Error::Consumed
        );
        Ok(())
    }
}
#[tokio::test]
async fn deep_link_entry_redeems_only_matching_callback_once() {
    let store = Arc::new(Store::default());
    let account = Arc::new(Account::new(
        Arc::new(adapters::fake::Fake::default()),
        store.clone(),
        Duration::from_secs(1),
    ));
    let status = account
        .login(&CallbackBrowser {
            account: account.clone(),
        })
        .await
        .unwrap();
    assert_eq!(status.identity.unwrap().uid, "fake-user");
    assert!(!status.pending);
    assert!(store.0.lock().await.is_some());
}
struct CancelBrowser {
    account: Arc<Account>,
}
#[async_trait]
impl Browser for CancelBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        self.account.logout().await?;
        Ok(())
    }
}
#[tokio::test]
async fn logout_cancels_pending_login_without_storing_credential() {
    let store = Arc::new(Store::default());
    let account = Arc::new(Account::new(
        Arc::new(adapters::fake::Fake::default()),
        store.clone(),
        Duration::from_secs(1),
    ));
    assert_eq!(
        account
            .login(&CancelBrowser {
                account: account.clone()
            })
            .await
            .unwrap_err(),
        Error::Cancelled
    );
    assert!(store.0.lock().await.is_none());
    assert!(account.status().await.identity.is_none());
}

#[tokio::test]
async fn provider_refuses_wrong_verifier_and_duplicate_redemption() {
    let provider = adapters::fake::Fake::default();
    provider.start_login(&flow()).await.unwrap();
    let mut wrong = flow().complete(callback("fixture-state-matching")).unwrap();
    wrong.verifier = Secret::new("wrong-verifier");
    assert!(matches!(provider.redeem(wrong).await, Err(Error::Provider)));
    provider
        .redeem(flow().complete(callback("fixture-state-matching")).unwrap())
        .await
        .unwrap();
    assert!(matches!(
        provider
            .redeem(flow().complete(callback("fixture-state-matching")).unwrap())
            .await,
        Err(Error::Provider)
    ));
}

#[derive(Default)]
struct Observer(Mutex<Vec<Option<String>>>);
#[async_trait]
impl IdentityObserver for Observer {
    async fn changed(&self, identity: Option<&Identity>) {
        self.0
            .lock()
            .await
            .push(identity.map(|value| value.uid.clone()));
    }
}
#[tokio::test]
async fn identity_observer_tracks_committed_sign_in_and_sign_out() {
    let observer = Arc::new(Observer::default());
    let account = Arc::new(
        Account::new(
            Arc::new(adapters::local::Local),
            Arc::new(Store::default()),
            Duration::from_secs(1),
        )
        .with_identity_observer(observer.clone()),
    );
    let status = account.login(&NoBrowser).await.unwrap();
    assert_eq!(
        *observer.0.lock().await,
        vec![status.identity.map(|value| value.uid)]
    );
    account.logout().await.unwrap();
    assert_eq!(observer.0.lock().await.last(), Some(&None));
}
