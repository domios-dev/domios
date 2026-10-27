use super::*;
use codeterm_account::{CredentialStore, IdentityProvider};
use serde_json::json;
use std::{sync::Arc, time::Duration};

struct ExpiredProvider;
#[async_trait]
impl IdentityProvider for ExpiredProvider {
    fn id(&self) -> &str {
        "fixture"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(
        &self,
        flow: &codeterm_account::LoginFlow,
    ) -> codeterm_account::Result<codeterm_account::LoginStart> {
        let codeterm_account::LoginStart::Immediate(mut tokens) =
            codeterm_account::adapters::local::Local
                .start_login(flow)
                .await?
        else {
            panic!("local immediate login");
        };
        tokens.session.expires_at = Some(1);
        Ok(codeterm_account::LoginStart::Immediate(tokens))
    }
    async fn redeem(
        &self,
        _: codeterm_account::Redemption,
    ) -> codeterm_account::Result<codeterm_account::Tokens> {
        panic!("no callback")
    }
    async fn refresh(
        &self,
        _: &codeterm_account::Secret,
    ) -> codeterm_account::Result<codeterm_account::Tokens> {
        panic!("post-call validation must not refresh")
    }
    async fn revoke(&self, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        panic!("no remote logout")
    }
}

struct MemoryCredential;
#[async_trait]
impl CredentialStore for MemoryCredential {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(None)
    }
    async fn save(&self, _: &str, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        Ok(())
    }
}

struct NoBrowser;
#[async_trait]
impl codeterm_account::Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> codeterm_account::Result<()> {
        panic!("no browser")
    }
}

#[tokio::test]
async fn post_call_validation_never_refreshes_and_rejects_logout_or_new_login() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let account = Arc::new(codeterm_account::Account::new(
        Arc::new(ExpiredProvider),
        Arc::new(MemoryCredential),
        Duration::from_secs(1),
    ));
    account.login(Arc::new(NoBrowser)).await.unwrap();
    let captured = account.session().await.unwrap();
    assert!(fixture
        .service
        .ext::<crate::service::account::AccountCell>()
        .0
        .set(account.clone())
        .is_ok());
    let calls = Calls {
        service: fixture.service.clone(),
        uid: captured.identity.uid.clone(),
        generation: captured.login_generation,
    };
    calls.validate_session(&captured).await.unwrap();
    account.logout().await.unwrap();
    assert_eq!(calls.validate_session(&captured).await, Err(Error::Revoked));
    account.login(Arc::new(NoBrowser)).await.unwrap();
    assert_eq!(calls.validate_session(&captured).await, Err(Error::Revoked));
}

#[test]
fn daily_limit_refusals_preserve_the_server_limit_and_reset_time() {
    let error = refusal(
        &json!({"error":{"status":"FAILED_PRECONDITION","details":{"reason":"neighbor_daily_limit","limit":7,"resets_at":1793145600u64}}}),
    );
    assert_eq!(
        error,
        Error::DailyLimit {
            limit: 7,
            resets_at: 1793145600
        }
    );
    let wire = error.to_string();
    let data: Value =
        serde_json::from_str(wire.strip_prefix("neighbor_daily_limit:").unwrap()).unwrap();
    assert_eq!(data["limit"], 7);
    assert_eq!(data["resets_at"], 1793145600u64);
    assert_eq!(
        refusal(&json!({"error":{"details":{"reason":"neighbor_daily_limit","limit":7}}})),
        Error::Refused
    );
    assert_eq!(
        refusal(&json!({"error":{"message":"limit_out_of_range","status":"INVALID_ARGUMENT"}})),
        Error::LimitOutOfRange
    );
    assert_eq!(
        refusal(&json!({"error":{"details":{"reason":"session_other"}}})),
        Error::Refused
    );
    for (reason, error) in [
        ("recipient_locked", Error::RecipientLocked),
        ("invalid_request", Error::InvalidRequest),
        ("friendship_not_found", Error::FriendshipNotFound),
        ("invalid_envelope", Error::InvalidEnvelope),
    ] {
        assert_eq!(refusal(&json!({"error":{"message":reason}})), error);
    }
    for (reason, expected) in [
        (
            "rate_limited",
            Error::RateLimited {
                retry_after_seconds: 42,
            },
        ),
        (
            "aggregate_limited",
            Error::AggregateLimited {
                retry_after_seconds: 42,
            },
        ),
    ] {
        assert_eq!(
            refusal(&json!({"error":{"details":{"reason":reason,"retry_after_seconds":42}}})),
            expected
        );
        assert_eq!(
            refusal(&json!({"error":{"details":{"reason":reason,"retry_after_seconds":"42"}}})),
            Error::Refused
        );
    }
}
