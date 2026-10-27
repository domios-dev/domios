use super::*;
use codeterm_account::*;
use std::{sync::Arc, time::Duration};

struct Provider;

fn hosted(mut tokens: Tokens) -> Tokens {
    tokens.session.identity.provider = "neighbor-fixture".into();
    tokens
}

#[async_trait::async_trait]
impl IdentityProvider for Provider {
    fn id(&self) -> &str {
        "neighbor-fixture"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(&self, flow: &LoginFlow) -> codeterm_account::Result<LoginStart> {
        match adapters::local::Local.start_login(flow).await? {
            LoginStart::Immediate(tokens) => Ok(LoginStart::Immediate(hosted(tokens))),
            LoginStart::Browser(_) => panic!("fixture never opens a browser"),
        }
    }
    async fn redeem(&self, _: Redemption) -> codeterm_account::Result<Tokens> {
        Err(Error::Provider)
    }
    async fn refresh(&self, token: &Secret) -> codeterm_account::Result<Tokens> {
        adapters::local::Local.refresh(token).await.map(hosted)
    }
    async fn revoke(&self, _: &Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Credentials(parking_lot::Mutex<Option<Secret>>);

#[async_trait::async_trait]
impl CredentialStore for Credentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        Ok(self.0.lock().clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> codeterm_account::Result<()> {
        *self.0.lock() = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        *self.0.lock() = None;
        Ok(())
    }
}

struct NoBrowser;

#[async_trait::async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> codeterm_account::Result<()> {
        panic!("fixture never opens a browser")
    }
}

impl crate::service::test_fixture::ServiceFixture {
    pub async fn seed_neighbor_concierge(
        &self,
        tab: &str,
        friendship: &str,
        permission: Permission,
    ) {
        let account = self
            .service
            .ext::<crate::service::account::AccountCell>()
            .0
            .get_or_init(|| async {
                let account = Arc::new(Account::new(
                    Arc::new(Provider),
                    Arc::new(Credentials::default()),
                    Duration::from_secs(1),
                ));
                account.login(Arc::new(NoBrowser)).await.unwrap();
                account
            })
            .await;
        let session = account.session().await.unwrap();
        let metadata = Metadata::new(
            self.service.account_data_root().unwrap(),
            &session.identity.uid,
        );
        let mut settings = metadata.settings().await.unwrap();
        settings.enabled = true;
        settings.bindings.insert(friendship.into(), tab.into());
        settings.permissions.insert(friendship.into(), permission);
        metadata.save_settings(settings.clone()).await.unwrap();
        self.service
            .remember_neighbor_concierge_accounts(&session.identity.uid, &settings);
    }
}
