use super::*;
use serde_json::json;

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
        ("invalid_request", Error::InvalidRequest),
        ("friendship_not_found", Error::FriendshipNotFound),
        ("invalid_envelope", Error::InvalidEnvelope),
    ] {
        assert_eq!(refusal(&json!({"error":{"message":reason}})), error);
    }
}
