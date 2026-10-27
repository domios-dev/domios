use super::*;

#[test]
fn definite_rejections_preserve_metadata_and_uncertain_sends_stay_pending() {
    for (error, reason) in [
        (
            Error::DailyLimit {
                limit: 7,
                resets_at: 1793145600,
            },
            "neighbor_daily_limit",
        ),
        (
            Error::RateLimited {
                retry_after_seconds: 60,
            },
            "rate_limited",
        ),
        (
            Error::AggregateLimited {
                retry_after_seconds: 90,
            },
            "aggregate_limited",
        ),
        (Error::Locked, "friends_locked"),
        (Error::RecipientLocked, "recipient_locked"),
        (Error::RecipientNotReceiving, "recipient_not_receiving"),
    ] {
        let rejection = Rejection::from_error(error).unwrap();
        let receipt = rejection.receipt("message");
        assert_eq!(receipt["status"], "not_delivered");
        assert_eq!(receipt["message_id"], "message");
        assert_eq!(receipt["reason"], reason);
        assert!(receipt.get("body").is_none());
        match error {
            Error::DailyLimit { limit, resets_at } => {
                assert_eq!(receipt["limit"], limit);
                assert_eq!(receipt["resets_at"], resets_at);
                assert!(rejection.description().contains("UTC"));
            }
            Error::RateLimited {
                retry_after_seconds,
            }
            | Error::AggregateLimited {
                retry_after_seconds,
            } => assert_eq!(receipt["retry_after_seconds"], retry_after_seconds),
            _ => assert!(!rejection.description().is_empty()),
        }
    }
    for uncertain in [Error::Transport, Error::Gateway, Error::InvalidEnvelope] {
        assert!(Rejection::from_error(uncertain).is_none());
    }
}
