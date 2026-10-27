use super::refresh_required;
use serde_json::json;

#[test]
fn admission_refreshes_signed_access_without_using_the_claim_as_a_grant() {
    for outcome in ["granted", "already_granted"] {
        assert_eq!(refresh_required(&json!({"schema_version":1,"outcome":outcome,
            "trial_end":1700604800,"friend_request_id":null})), Ok(true));
    }
    for outcome in ["not_invited", "inviter_not_paid", "already_paid", "not_fresh", "invite_expired"] {
        assert_eq!(refresh_required(&json!({"schema_version":1,"outcome":outcome,
            "trial_end":null,"friend_request_id":null})), Ok(false));
    }
}

#[test]
fn malformed_claims_never_request_an_access_refresh() {
    for value in [
        json!({"schema_version":2,"outcome":"granted","trial_end":1700604800,"friend_request_id":null}),
        json!({"schema_version":1,"outcome":"granted","trial_end":null,"friend_request_id":null}),
        json!({"schema_version":1,"outcome":"not_invited","trial_end":1700604800,"friend_request_id":null}),
        json!({"schema_version":1,"outcome":"granted","trial_end":1700604800,"friend_request_id":"foreign"}),
        json!({"schema_version":1,"outcome":"unknown","trial_end":null,"friend_request_id":null}),
    ] {
        assert!(refresh_required(&value).is_err());
    }
}
