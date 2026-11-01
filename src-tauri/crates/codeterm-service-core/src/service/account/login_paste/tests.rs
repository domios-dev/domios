use super::*;
use codeterm_account::{Callback, CredentialStore, Secret};

#[derive(Default)]
struct MemoryCredentials(Mutex<Option<Secret>>);

#[async_trait]
impl CredentialStore for MemoryCredentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        Ok(self.0.lock().await.clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> codeterm_account::Result<()> {
        *self.0.lock().await = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        *self.0.lock().await = None;
        Ok(())
    }
}

#[tokio::test]
async fn paste_login_returns_url_and_waits_for_verified_redemption() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    service.ext::<crate::config::ConfigState>().lock().billing =
        serde_json::json!({"endpoint":"fixture-unavailable"});
    let credentials = Arc::new(MemoryCredentials::default());
    let account = Arc::new(Account::new(
        Arc::new(codeterm_account::adapters::fake::Fake::default()),
        credentials.clone(),
        Duration::from_secs(5),
    ));
    assert!(service.ext::<AccountCell>().0.set(account.clone()).is_ok());
    let (id, authorization) = service.account_login_paste_start().await.unwrap();
    assert_eq!(
        service.account_login_paste_pending().await,
        Some((id.clone(), authorization.clone()))
    );
    assert_eq!(authorization.host_str(), Some("fake.invalid"));
    let pairs: std::collections::HashMap<_, _> = authorization.query_pairs().into_owned().collect();
    let callback = |state: &str| {
        let mut url = url::Url::parse(&pairs["redirect_uri"]).unwrap();
        url.query_pairs_mut()
            .append_pair("code", &"a".repeat(43))
            .append_pair("state", state);
        Callback::parse_paste(url.as_str()).unwrap()
    };
    assert!(service
        .account_login_paste_complete("foreign-operation", callback(&pairs["state"]))
        .await
        .is_err());
    assert!(service
        .account_login_paste_complete(&id, callback("wrong-state-0123456789"))
        .await
        .is_err());
    assert!(credentials.0.lock().await.is_none());
    assert!(account.status().await.pending);
    let settled = service
        .account_login_paste_complete(&id, callback(&pairs["state"]))
        .await
        .unwrap();
    assert_eq!(settled.identity.unwrap().uid, "fake-user");
    assert!(!settled.pending);
    assert_eq!(
        credentials.0.lock().await.as_ref().unwrap().expose(),
        "fake-refresh"
    );
    assert!(service
        .account_login_paste_complete(&id, callback(&pairs["state"]))
        .await
        .is_err());
}
