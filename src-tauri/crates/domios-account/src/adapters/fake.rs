use crate::*;
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Default)]
pub struct Fake {
    count: AtomicUsize,
    pending: std::sync::Mutex<Option<std::collections::HashMap<String, String>>>,
}
impl Fake {
    fn tokens(&self) -> Tokens {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        Tokens {
            session: Session {
                identity: Identity {
                    uid: "fake-user".into(),
                    email: None,
                    display_name: None,
                    provider: "fake".into(),
                },
                id_token: Secret::new(format!("fake-id-{n}")),
            },
            refresh_token: Secret::new("fake-refresh"),
        }
    }
}
#[async_trait]
impl IdentityProvider for Fake {
    fn id(&self) -> &str {
        "fake"
    }
    async fn start_login(&self, flow: &LoginFlow) -> Result<LoginStart> {
        let url = flow.signin_url("https://fake.invalid/signin/")?;
        *self.pending.lock().map_err(|_| Error::Provider)? =
            Some(url.query_pairs().into_owned().collect());
        Ok(LoginStart::Browser(url))
    }
    async fn redeem(&self, request: Redemption) -> Result<Tokens> {
        let mut pending = self.pending.lock().map_err(|_| Error::Provider)?;
        let challenge = pending.as_ref().ok_or(Error::Provider)?;
        if challenge.get("state") != Some(&request.state)
            || challenge.get("redirect_uri") != Some(&request.redirect)
            || challenge.get("code_challenge")
                != Some(&LoginFlow::challenge(request.verifier.expose()))
        {
            return Err(Error::Provider);
        }
        *pending = None;
        Ok(self.tokens())
    }
    async fn refresh(&self, token: &Secret) -> Result<Tokens> {
        if token.expose() != "fake-refresh" {
            return Err(Error::Credentials);
        }
        Ok(self.tokens())
    }
    async fn revoke(&self, _: &Secret) -> Result<()> {
        Ok(())
    }
}
