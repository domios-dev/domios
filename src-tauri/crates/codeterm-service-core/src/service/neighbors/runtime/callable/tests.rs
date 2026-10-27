use super::*;
use serde_json::json;

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
