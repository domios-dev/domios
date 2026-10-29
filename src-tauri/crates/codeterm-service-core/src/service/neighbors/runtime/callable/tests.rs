use super::*;

#[test]
fn friend_receiver_refusals_preserve_feature_key_and_machine_slot_reasons() {
    for (reason, expected) in [
        ("feature_unsupported", Error::FeatureUnsupported),
        ("key_revoked", Error::Revoked),
        ("key_not_found", Error::KeyNotFound),
        ("receiver_revision_stale", Error::ReceiverRevisionStale),
        ("not_receiver", Error::NotReceiver),
    ] {
        assert_eq!(
            refusal(&json!({"error":{"details":{"reason":reason}}})),
            expected
        );
    }
    assert_eq!(
        refusal(&json!({"error":{"details":{"reason":"key_limit","limit":5,"active":5}}})),
        Error::KeyLimit {
            limit: 5,
            active: 5
        }
    );
    assert_eq!(
        refusal(&json!({"error":{"details":{"reason":"key_limit","limit":5}}})),
        Error::Refused
    );
}

#[test]
fn recipient_pause_remains_a_typed_refusal_for_messages_and_file_grants() {
    assert_eq!(
        refusal(
            &json!({"error":{"status":"FAILED_PRECONDITION","details":{"reason":"recipient_paused"}}})
        ),
        Error::RecipientPaused
    );
    assert!(Error::RecipientPaused
        .to_string()
        .starts_with("recipient_paused:"));
}

#[test]
fn aggregate_daily_refusal_preserves_limit_and_utc_reset_without_guessing_missing_fields() {
    let error = refusal(
        &json!({"error":{"status":"RESOURCE_EXHAUSTED","details":{"reason":"aggregate_daily_limited","limit":500,"resets_at":1793145600u64}}}),
    );
    assert_eq!(
        error,
        Error::AggregateDailyLimited {
            limit: 500,
            resets_at: 1793145600
        }
    );
    let display = error.to_string();
    let details: Value =
        serde_json::from_str(display.strip_prefix("aggregate_daily_limited:").unwrap()).unwrap();
    assert_eq!(details, json!({"limit":500,"resets_at":1793145600u64}));
    for details in [
        json!({"reason":"aggregate_daily_limited","limit":500}),
        json!({"reason":"aggregate_daily_limited","limit":"500","resets_at":1793145600u64}),
    ] {
        assert_eq!(
            refusal(&json!({"error":{"details":details}})),
            Error::Refused
        );
    }
}
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
        own_keys: Default::default(),
        receiver_status: Default::default(),
    };
    calls.validate_session(&captured).await.unwrap();
    account.logout().await.unwrap();
    assert_eq!(calls.validate_session(&captured).await, Err(Error::Revoked));
    account.login(Arc::new(NoBrowser)).await.unwrap();
    assert_eq!(calls.validate_session(&captured).await, Err(Error::Revoked));
}

#[test]
fn server_ciphertext_size_refusal_reports_the_plaintext_limit() {
    let error = refusal(
        &json!({"error":{"status":"INVALID_ARGUMENT","message":"Invalid Neighbor envelope","details":{"reason":"too_large"}}}),
    );
    assert_eq!(error, Error::MessageTooLarge { limit: 32752 });
    let wire = error.to_string();
    let details: Value =
        serde_json::from_str(wire.strip_prefix("message_too_large:").unwrap()).unwrap();
    assert_eq!(details, json!({"limit":32752}));
}

#[tokio::test]
async fn cached_own_inventory_and_receiver_reads_still_require_the_current_account_session() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let calls = Calls {
        service: fixture.service.clone(),
        uid: "owner".into(),
        generation: 1,
        own_keys: tokio::sync::Mutex::new(Some(json!([]))),
        receiver_status: tokio::sync::Mutex::new(Some(json!({}))),
    };
    assert_eq!(
        calls.invoke("friendsOwnKeys", json!({})).await,
        Err(Error::Revoked)
    );
    assert_eq!(
        calls.invoke("friendsReceiverStatus", json!({})).await,
        Err(Error::Revoked)
    );
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

#[test]
fn outage_responses_retry_but_permission_auth_and_entitlement_refusals_stop() {
    for status in [401, 429, 500, 502, 503] {
        assert_eq!(callable_error(status, None), Error::Transport);
        assert_eq!(callable_error(status, Some(&json!({"error":{"status":"INTERNAL"}}))), Error::Transport);
    }
    assert_eq!(callable_error(403, None), Error::PermissionDenied);
    assert!(!codeterm_neighbors::supervisor::retryable(Error::PermissionDenied));
    for reason in ["friends_locked", "entitlement_locked"] {
        assert_eq!(callable_error(403, Some(&json!({"error":{"details":{"reason":reason}}}))), Error::Locked);
    }
}

#[test]
fn callable_failure_preserves_structured_delivery_limits() {
    assert_eq!(callable_error(429, Some(&json!({"error":{"details":{"reason":"rate_limited","retry_after_seconds":45}}}))), Error::RateLimited { retry_after_seconds: 45 });
}

mod auth;
