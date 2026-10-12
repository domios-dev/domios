use crate::*;
pub struct Local;
fn tokens() -> Tokens {
    Tokens {
        session: Session {
            identity: Identity {
                uid: "local-dev-user".into(),
                email: Some("dev@localhost".into()),
                display_name: Some("Dev user".into()),
                provider: "local".into(),
            },
            id_token: Secret::new("local-memory-session"),
        },
        refresh_token: Secret::new("local-dev-credential"),
    }
}
#[async_trait]
impl IdentityProvider for Local {
    fn id(&self) -> &str {
        "local"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(&self, _: &LoginFlow) -> Result<LoginStart> {
        Ok(LoginStart::Immediate(tokens()))
    }
    async fn redeem(&self, _: Redemption) -> Result<Tokens> {
        Err(Error::Provider)
    }
    async fn refresh(&self, token: &Secret) -> Result<Tokens> {
        if token.expose() != "local-dev-credential" {
            return Err(Error::Credentials);
        }
        Ok(tokens())
    }
    async fn revoke(&self, _: &Secret) -> Result<()> {
        Ok(())
    }
}
